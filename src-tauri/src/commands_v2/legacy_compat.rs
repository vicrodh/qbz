//! Legacy-equivalent V2 commands.
//!
//! Extracted from `commands_v2/mod.rs` — no functional changes.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use tauri::{Emitter, State};

#[cfg(target_os = "linux")]
use ashpd::desktop::notification::{Notification as PortalNotification, NotificationProxy};
#[cfg(target_os = "linux")]
use ashpd::desktop::Icon;

use crate::api::models::{
    DynamicSuggestRequest, DynamicSuggestResponse, DynamicTrackToAnalyse, PurchaseAlbum,
    PurchaseIdsResponse, PurchaseResponse, PurchaseTrack,
    SearchResultsPage as ApiSearchResultsPage,
};
use crate::integrations_v2::{LastFmV2State, ListenBrainzV2State, MusicBrainzV2State};
use crate::library::{LibraryState, LocalTrack};
use crate::lyrics::LyricsState;
use crate::musicbrainz::MusicBrainzSharedState;
use crate::offline::OfflineState;
use crate::offline_cache::OfflineCacheState;
use crate::runtime::RuntimeError;
use crate::AppState;

use super::{
    download_audio, v2_cache_notification_artwork, v2_format_notification_quality,
    v2_teardown_type_alias_state,
};
// Linux-only: turns an artwork PNG into the raw bytes Ayatana notifications
// expect. Only called inside the `cfg(target_os = "linux")` arm below, so the
// import must match.
#[cfg(target_os = "linux")]
use super::v2_prepare_notification_icon_bytes;

#[tauri::command]
pub async fn v2_show_track_notification(
    title: String,
    artist: String,
    album: String,
    artwork_url: Option<String>,
    bit_depth: Option<u32>,
    sample_rate: Option<f64>,
) -> Result<(), String> {
    log::info!(
        "Command: v2_show_track_notification - {} by {}",
        title,
        artist
    );

    let body_text = {
        let separator = if cfg!(target_os = "macos") { " \u{00b7} " } else { " \u{2022} " };
        let mut lines = Vec::new();
        let mut line1_parts = Vec::new();
        if !artist.is_empty() {
            line1_parts.push(artist.clone());
        }
        if !album.is_empty() {
            line1_parts.push(album.clone());
        }
        if !line1_parts.is_empty() {
            lines.push(line1_parts.join(separator));
        }

        let quality = v2_format_notification_quality(bit_depth, sample_rate);
        if !quality.is_empty() {
            lines.push(quality);
        }

        lines.join("\n")
    };

    #[cfg(target_os = "linux")]
    {
        let mut notification = PortalNotification::new(&title)
            .body(Some(body_text.as_str()));

        if let Some(ref url_str) = artwork_url {
            let url_clone = url_str.clone();
            let prepared = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
                let path = v2_cache_notification_artwork(&url_clone)?;
                v2_prepare_notification_icon_bytes(&path)
            })
            .await;

            match prepared {
                Ok(Ok(icon_bytes)) => {
                    log::info!("Notification artwork prepared: {} bytes", icon_bytes.len());
                    notification = notification.icon(Icon::Bytes(icon_bytes));
                }
                Ok(Err(e)) => {
                    log::warn!("Could not prepare notification artwork icon: {}", e);
                }
                Err(e) => {
                    log::warn!("Notification artwork preparation task failed: {}", e);
                }
            }
        }

        match NotificationProxy::new().await {
            Ok(proxy) => {
                if let Err(e) = proxy.add_notification("track-now-playing", notification).await {
                    log::warn!("Could not show notification via XDG portal: {}", e);
                }
            }
            Err(e) => {
                log::warn!("XDG notification portal unavailable: {}", e);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Fire-and-forget: notification delivery shouldn't block track playback response
        tokio::task::spawn_blocking(move || {
            let _ = notify_rust::set_application("com.blitzfc.qbz");

            // Cache artwork to disk if available (image_path needs a file path)
            let artwork_path = artwork_url.as_deref().and_then(|url_str| {
                match v2_cache_notification_artwork(url_str) {
                    Ok(path) => {
                        log::debug!("Notification artwork cached: {:?}", path);
                        Some(path)
                    }
                    Err(e) => {
                        log::debug!("Could not prepare notification artwork: {}", e);
                        None
                    }
                }
            });

            let mut notification = notify_rust::Notification::new();
            notification.summary(&title).body(&body_text);

            if let Some(ref path) = artwork_path {
                if let Some(path_str) = path.to_str() {
                    notification.image_path(path_str);
                }
            }

            if let Err(e) = notification.show() {
                log::warn!("Failed to show macOS notification: {}", e);
            }
        });
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (&body_text, &artwork_url);
        log::info!("Desktop notifications not implemented on this platform");
    }

    Ok(())
}

#[tauri::command]
pub async fn v2_subscribe_playlist(
    playlist_id: u64,
    state: State<'_, AppState>,
    library_state: State<'_, crate::library::LibraryState>,
) -> Result<crate::api::models::Playlist, String> {
    log::info!("Command: v2_subscribe_playlist {}", playlist_id);
    let client = state.client.read().await;

    let source = client
        .get_playlist(playlist_id)
        .await
        .map_err(|e| format!("Failed to get source playlist: {}", e))?;

    let track_ids: Vec<u64> = source
        .tracks
        .as_ref()
        .map(|t| t.items.iter().map(|track| track.id).collect())
        .unwrap_or_default();
    if track_ids.is_empty() {
        return Err("Source playlist has no tracks to copy".to_string());
    }

    let attribution = format!(
        "\n\n---\nOriginally curated by {} on Qobuz",
        source.owner.name
    );
    let new_description = match source.description {
        Some(ref desc) if !desc.is_empty() => Some(format!("{}{}", desc, attribution)),
        _ => Some(attribution.trim_start().to_string()),
    };

    let new_playlist = client
        .create_playlist(&source.name, new_description.as_deref(), false)
        .await
        .map_err(|e| format!("Failed to create new playlist: {}", e))?;

    client
        .add_tracks_to_playlist(new_playlist.id, &track_ids)
        .await
        .map_err(|e| format!("Failed to add tracks to new playlist: {}", e))?;

    if let Some(ref images) = source.images {
        if let Some(image_url) = images.first() {
            let db_guard = library_state.db.lock().await;
            if let Some(ref db) = *db_guard {
                if let Err(e) = db.update_playlist_artwork(new_playlist.id, Some(image_url)) {
                    log::warn!("Failed to save original playlist artwork: {}", e);
                }
            }
        }
    }

    client
        .get_playlist(new_playlist.id)
        .await
        .map_err(|e| format!("Failed to fetch created playlist: {}", e))
}

/// Subscribe to a Qobuz playlist (follow it in the user's Qobuz library).
/// Unlike v2_subscribe_playlist which copies tracks locally, this calls the
/// Qobuz API so the playlist appears in the user's account on all Qobuz clients.
#[tauri::command]
pub async fn v2_qobuz_subscribe_playlist(
    playlist_id: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: v2_qobuz_subscribe_playlist {}", playlist_id);
    let client = state.client.read().await;
    client
        .subscribe_playlist(playlist_id)
        .await
        .map_err(|e| format!("Failed to subscribe to playlist: {}", e))
}

/// Unsubscribe from a Qobuz playlist.
#[tauri::command]
pub async fn v2_qobuz_unsubscribe_playlist(
    playlist_id: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: v2_qobuz_unsubscribe_playlist {}", playlist_id);
    let client = state.client.read().await;
    client
        .unsubscribe_playlist(playlist_id)
        .await
        .map_err(|e| format!("Failed to unsubscribe from playlist: {}", e))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_share_track_songlink(
    isrc: Option<String>,
    url: String,
    trackId: Option<u64>,
    state: State<'_, AppState>,
) -> Result<crate::share::SongLinkResponse, RuntimeError> {
    if let Some(code) = isrc.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        return state
            .songlink
            .get_by_isrc(code)
            .await
            .map_err(|e| RuntimeError::Internal(e.to_string()));
    }

    let fallback_url = if let Some(id) = trackId {
        format!("https://play.qobuz.com/track/{}", id)
    } else {
        url
    };
    state
        .songlink
        .get_by_url(&fallback_url, crate::share::ContentType::Track)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_share_album_songlink(
    upc: Option<String>,
    albumId: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    state: State<'_, AppState>,
) -> Result<crate::share::SongLinkResponse, RuntimeError> {
    let _ = (title, artist);
    if let Some(code) = upc.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        return state
            .songlink
            .get_by_upc(code)
            .await
            .map_err(|e| RuntimeError::Internal(e.to_string()));
    }

    let fallback_url = albumId
        .map(|id| format!("https://play.qobuz.com/album/{}", id))
        .ok_or_else(|| {
            RuntimeError::Internal("Missing UPC/albumId for song.link album lookup".to_string())
        })?;
    state
        .songlink
        .get_by_url(&fallback_url, crate::share::ContentType::Album)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn v2_library_backfill_downloads(
    state: State<'_, LibraryState>,
    offline_cache_state: State<'_, crate::offline_cache::OfflineCacheState>,
) -> Result<crate::library::BackfillReport, RuntimeError> {
    use crate::library::{BackfillReport, MetadataExtractor};

    log::info!("Command: v2_library_backfill_downloads");

    let mut report = BackfillReport {
        total_downloads: 0,
        added_tracks: 0,
        repaired_tracks: 0,
        skipped_tracks: 0,
        failed_tracks: Vec::new(),
    };

    // Get all ready cached tracks directly from offline cache DB
    let cached_tracks = {
        let cache_db_opt__ = offline_cache_state.db.lock().await;
        let cache_db = cache_db_opt__
            .as_ref()
            .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;

        let mut stmt = cache_db
            .conn()
            .prepare("SELECT track_id, title, artist, album, album_id, duration_secs, file_path, quality, bit_depth, sample_rate FROM cached_tracks WHERE status = 'ready'")
            .map_err(|e| RuntimeError::Internal(format!("Failed to query cached tracks: {}", e)))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,                    // track_id
                    row.get::<_, String>(1)?,                        // title
                    row.get::<_, String>(2)?,                        // artist
                    row.get::<_, Option<String>>(3)?,                // album
                    row.get::<_, i64>(5)? as u64,                    // duration_secs
                    row.get::<_, String>(6)?,                        // file_path
                    row.get::<_, Option<i64>>(8)?.map(|v| v as u32), // bit_depth
                    row.get::<_, Option<f64>>(9)?,                   // sample_rate
                ))
            })
            .map_err(|e| RuntimeError::Internal(format!("Failed to map rows: {}", e)))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| RuntimeError::Internal(format!("Failed to collect cached tracks: {}", e)))?
    }; // cache_db lock is dropped here

    report.total_downloads = cached_tracks.len();

    let library_db_opt__ = state.db.lock().await;
    let library_db = library_db_opt__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;

    for (track_id, title, artist, album, duration_secs, file_path, bit_depth, sample_rate) in
        cached_tracks
    {
        // Strategy: Try to match by qobuz_track_id first, then by file_path
        // This handles both intact downloads and downloads damaged by scanner

        let exists_by_id = library_db
            .track_exists_by_qobuz_id(track_id)
            .unwrap_or(false);

        let exists_by_path = library_db.track_exists_by_path(&file_path).unwrap_or(false);

        if exists_by_id {
            // Track exists with correct qobuz_track_id (not damaged)
            // Check if it just needs source repair
            match library_db.is_qobuz_cached_track_by_path(&file_path) {
                Ok(true) => {
                    // Already marked as cached track, nothing to do
                    report.skipped_tracks += 1;
                }
                Ok(false) => {
                    // Has qobuz_track_id but lost source marker - unusual case
                    log::info!(
                        "Repairing source for track with intact ID {}: {}",
                        track_id,
                        title
                    );
                    match library_db.repair_qobuz_cached_track_by_path(track_id, &file_path) {
                        Ok(true) => report.repaired_tracks += 1,
                        Ok(false) => report.skipped_tracks += 1,
                        Err(e) => {
                            log::warn!("Failed to repair track {}: {}", track_id, e);
                            report.failed_tracks.push(title);
                        }
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to check cached track status for {}: {}",
                        track_id,
                        e
                    );
                    report.failed_tracks.push(title);
                }
            }
            continue;
        }

        if exists_by_path {
            // Track exists by path but lost qobuz_track_id (damaged by scanner)
            log::info!(
                "Repairing damaged cached track (lost ID) {}: {}",
                track_id,
                title
            );
            match library_db.repair_qobuz_cached_track_by_path(track_id, &file_path) {
                Ok(true) => report.repaired_tracks += 1,
                Ok(false) => report.skipped_tracks += 1,
                Err(e) => {
                    log::warn!("Failed to repair track by path {}: {}", track_id, e);
                    report.failed_tracks.push(title);
                }
            }
            continue;
        }

        // Track doesn't exist - extract track/disc number from file tags
        let (track_num, disc_num) =
            match MetadataExtractor::extract(std::path::Path::new(&file_path)) {
                Ok(meta) => (meta.track_number, meta.disc_number),
                Err(e) => {
                    log::warn!("Could not extract metadata from {}: {}", file_path, e);
                    (None, None)
                }
            };

        // Insert as new
        match library_db.insert_qobuz_cached_track_direct(
            track_id,
            &title,
            &artist,
            album.as_deref(),
            duration_secs,
            &file_path,
            bit_depth,
            sample_rate,
            track_num,
            disc_num,
        ) {
            Ok(_) => report.added_tracks += 1,
            Err(e) => {
                log::warn!("Failed to insert track {}: {}", track_id, e);
                report.failed_tracks.push(title);
            }
        }
    }

    Ok(report)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_lyrics_get(
    trackId: Option<u64>,
    title: String,
    artist: String,
    album: Option<String>,
    durationSecs: Option<u64>,
    state: State<'_, LyricsState>,
) -> Result<Option<crate::lyrics::LyricsPayload>, RuntimeError> {
    use crate::lyrics::providers::{fetch_lrclib, fetch_lyrics_ovh};
    use crate::lyrics::{build_cache_key, LyricsPayload};

    let track_id = trackId;
    let duration_secs = durationSecs;

    let title_trimmed = title.trim();
    let artist_trimmed = artist.trim();

    if title_trimmed.is_empty() || artist_trimmed.is_empty() {
        return Err(RuntimeError::Internal(
            "Lyrics lookup requires title and artist".to_string(),
        ));
    }

    let cache_key = build_cache_key(title_trimmed, artist_trimmed, duration_secs);

    // Try cache by track_id first, then by key.
    // If cached entry has plain but no synced lyrics, treat as miss and re-fetch
    // (search-first strategy is likely to find synced now).
    {
        let db_opt__ = state.db.lock().await;
        let db = db_opt__
            .as_ref()
            .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;

        let cached = if let Some(id) = track_id {
            db.get_by_track_id(id).ok().flatten()
        } else {
            None
        }
        .or_else(|| db.get_by_cache_key(&cache_key).ok().flatten());

        if let Some(payload) = cached {
            let has_synced = payload
                .synced_lrc
                .as_ref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            if has_synced {
                return Ok(Some(payload));
            }
            // plain-only cache: fall through to re-fetch for synced
        }
    }

    // Provider chain: LRCLIB (with 1 retry on network error) -> lyrics.ovh
    let lrclib_data = match fetch_lrclib(title_trimmed, artist_trimmed, duration_secs).await {
        Ok(data) => data,
        Err(e) => {
            // Network error — retry once
            eprintln!("[Lyrics] LRCLIB attempt 1 failed: {}, retrying…", e);
            match fetch_lrclib(title_trimmed, artist_trimmed, duration_secs).await {
                Ok(data) => data,
                Err(e2) => {
                    eprintln!(
                        "[Lyrics] LRCLIB attempt 2 failed: {}, falling back to lyrics.ovh",
                        e2
                    );
                    None
                }
            }
        }
    };

    if let Some(data) = lrclib_data {
        let payload = LyricsPayload {
            track_id,
            title: title_trimmed.to_string(),
            artist: artist_trimmed.to_string(),
            album: album.clone(),
            duration_secs,
            plain: data.plain,
            synced_lrc: data.synced_lrc,
            provider: data.provider,
            cached: false,
        };

        let db_opt__ = state.db.lock().await;
        let db = db_opt__
            .as_ref()
            .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
        db.upsert(&cache_key, &payload)
            .map_err(RuntimeError::Internal)?;
        return Ok(Some(payload));
    }

    if let Some(data) = fetch_lyrics_ovh(title_trimmed, artist_trimmed).await {
        let payload = LyricsPayload {
            track_id,
            title: title_trimmed.to_string(),
            artist: artist_trimmed.to_string(),
            album,
            duration_secs,
            plain: data.plain,
            synced_lrc: data.synced_lrc,
            provider: data.provider,
            cached: false,
        };

        let db_opt__ = state.db.lock().await;
        let db = db_opt__
            .as_ref()
            .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
        db.upsert(&cache_key, &payload)
            .map_err(RuntimeError::Internal)?;
        return Ok(Some(payload));
    }

    Ok(None)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct V2LyricsCacheStats {
    pub entries: u64,
    pub size_bytes: u64,
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_create_pending_playlist(
    name: String,
    description: Option<String>,
    isPublic: bool,
    trackIds: Vec<u64>,
    localTrackPaths: Vec<String>,
    state: State<'_, OfflineState>,
) -> Result<i64, RuntimeError> {
    let guard__ = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
    store
        .create_pending_playlist(
            &name,
            description.as_deref(),
            isPublic,
            &trackIds,
            &localTrackPaths,
        )
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub fn v2_get_pending_playlist_count(state: State<'_, OfflineState>) -> Result<u32, RuntimeError> {
    let guard__ = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
    store
        .get_pending_playlist_count()
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub fn v2_queue_scrobble(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: i64,
    state: State<'_, OfflineState>,
) -> Result<i64, RuntimeError> {
    let guard__ = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
    store
        .queue_scrobble(&artist, &track, album.as_deref(), timestamp)
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_get_queued_scrobbles(
    limit: Option<u32>,
    state: State<'_, OfflineState>,
) -> Result<Vec<crate::offline::QueuedScrobble>, RuntimeError> {
    let guard__ = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
    store
        .get_queued_scrobbles(limit.unwrap_or(50))
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub fn v2_get_queued_scrobble_count(state: State<'_, OfflineState>) -> Result<u32, RuntimeError> {
    let guard__ = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
    store
        .get_queued_scrobble_count()
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_cleanup_sent_scrobbles(
    olderThanDays: Option<u32>,
    state: State<'_, OfflineState>,
) -> Result<u32, RuntimeError> {
    let guard__ = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
    store
        .cleanup_sent_scrobbles(olderThanDays.unwrap_or(7))
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_track_by_path(
    filePath: String,
    state: State<'_, LibraryState>,
) -> Result<Option<LocalTrack>, RuntimeError> {
    log::info!("Command: v2_get_track_by_path {}", filePath);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
    db.get_track_by_path(&filePath)
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_check_network_path(
    path: String,
) -> Result<crate::network::NetworkPathInfo, RuntimeError> {
    Ok(crate::network::is_network_path(std::path::Path::new(&path)))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_library_update_folder_settings(
    id: i64,
    alias: Option<String>,
    enabled: bool,
    isNetwork: bool,
    networkFsType: Option<String>,
    userOverrideNetwork: bool,
    state: State<'_, LibraryState>,
) -> Result<crate::library::LibraryFolder, RuntimeError> {
    log::info!(
        "Command: v2_library_update_folder_settings {} alias={:?} enabled={}",
        id,
        alias,
        enabled
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
    db.update_folder_settings(
        id,
        alias.as_deref(),
        enabled,
        isNetwork,
        networkFsType.as_deref(),
        userOverrideNetwork,
    )
    .map_err(|e| RuntimeError::Internal(e.to_string()))?;

    db.get_folder_by_id(id)
        .map_err(|e| RuntimeError::Internal(e.to_string()))?
        .ok_or_else(|| RuntimeError::Internal("Folder not found after update".to_string()))
}

#[tauri::command]
pub async fn v2_discogs_has_credentials() -> Result<bool, RuntimeError> {
    // Proxy always provides credentials.
    Ok(true)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_discogs_search_artwork(
    artist: String,
    album: String,
    catalogNumber: Option<String>,
) -> Result<Vec<crate::discogs::DiscogsImageOption>, RuntimeError> {
    log::info!(
        "Command: v2_discogs_search_artwork {} - {} (catalog: {:?})",
        artist,
        album,
        catalogNumber
    );

    let client = crate::discogs::DiscogsClient::new();
    client
        .search_artwork_options(&artist, &album, catalogNumber.as_deref())
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_discogs_download_artwork(
    imageUrl: String,
    artist: String,
    album: String,
) -> Result<String, RuntimeError> {
    log::info!("Command: v2_discogs_download_artwork from {}", imageUrl);

    let cache_dir = crate::library::get_artwork_cache_dir();
    let client = crate::discogs::DiscogsClient::new();

    client
        .download_artwork_from_url(&imageUrl, &cache_dir, &artist, &album)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_check_album_fully_cached(
    albumId: String,
    cache_state: State<'_, crate::offline_cache::OfflineCacheState>,
) -> Result<bool, RuntimeError> {
    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;

    // Get all tracks for this album
    let tracks = db.get_all_tracks().map_err(RuntimeError::Internal)?;
    let album_tracks: Vec<_> = tracks
        .into_iter()
        .filter(|t| t.album_id.as_deref() == Some(&albumId))
        .collect();

    if album_tracks.is_empty() {
        return Ok(false);
    }

    // Check if all tracks are ready
    for track in album_tracks {
        if track.status != crate::offline_cache::OfflineCacheStatus::Ready {
            return Ok(false);
        }
    }

    Ok(true)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_check_albums_fully_cached_batch(
    albumIds: Vec<String>,
    cache_state: State<'_, crate::offline_cache::OfflineCacheState>,
) -> Result<std::collections::HashMap<String, bool>, RuntimeError> {
    use std::collections::HashMap;

    if albumIds.is_empty() {
        return Ok(HashMap::new());
    }

    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;

    let tracks = db.get_all_tracks().map_err(RuntimeError::Internal)?;

    // Group tracks by album_id
    let mut album_tracks: HashMap<&str, (usize, usize)> = HashMap::new(); // (total, ready)
    for track in &tracks {
        if let Some(ref aid) = track.album_id {
            let entry = album_tracks.entry(aid.as_str()).or_insert((0, 0));
            entry.0 += 1;
            if track.status == crate::offline_cache::OfflineCacheStatus::Ready {
                entry.1 += 1;
            }
        }
    }

    let result: HashMap<String, bool> = albumIds
        .into_iter()
        .map(|id| {
            let fully_cached = album_tracks
                .get(id.as_str())
                .map(|(total, ready)| *total > 0 && *total == *ready)
                .unwrap_or(false);
            (id, fully_cached)
        })
        .collect();

    Ok(result)
}

/// Shared helper: spawn the download task for a single track.
/// CMAF-first offline download path (v2 format).
///
/// On success: the encrypted CMAF bundle is persisted under
/// `<offline_root>/tracks-cmaf/<track_id>/`, the per-track AES content key
/// + session infos are wrapped via `qbz-secrets` and stored on the DB row,
/// `cache_format` flips to 2, `mark_complete` fires, and the library row
/// is populated with the same metadata the legacy path would populate
/// (title/artist/album, etc.).
///
/// Returns `Err` for any failure that makes CMAF unusable — the caller
/// falls back to the legacy plain-FLAC path.
async fn try_cmaf_offline_download(
    track_id: u64,
    bridge: &std::sync::Arc<tokio::sync::RwLock<Option<crate::core_bridge::CoreBridge>>>,
    db: &std::sync::Arc<tokio::sync::Mutex<Option<crate::offline_cache::OfflineCacheDb>>>,
    offline_root: &str,
    library_db: &std::sync::Arc<tokio::sync::Mutex<Option<qbz_library::LibraryDatabase>>>,
    client: &std::sync::Arc<tokio::sync::RwLock<crate::api::QobuzClient>>,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    use qbz_models::Quality;

    let offline_root_path = std::path::PathBuf::from(offline_root);

    // Progress callback: emit the same `offline:caching_progress` event
    // shape the legacy StreamFetcher fires, so the UI's progress ring
    // doesn't care whether the bytes came from CMAF or legacy.
    //
    // Note: one 'started' event here up front so the frontend sees
    // 'downloading' status immediately (the ring starts empty); actual
    // percentage updates arrive per completed segment.
    let _ = app.emit(
        "offline:caching_progress",
        serde_json::json!({
            "trackId": track_id,
            "progressPercent": 0u32,
            "bytesDownloaded": 0u64,
            "totalBytes": serde_json::Value::Null,
            "status": "downloading",
        }),
    );

    let app_for_cb = app.clone();
    let progress_cb: qbz_qobuz::CmafProgressCallback = std::sync::Arc::new(
        move |update: qbz_qobuz::CmafProgressUpdate| {
            let percent = if update.n_segments > 0 {
                (update.segments_completed as f64 / update.n_segments as f64 * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u32
            } else {
                0u32
            };
            let _ = app_for_cb.emit(
                "offline:caching_progress",
                serde_json::json!({
                    "trackId": track_id,
                    "progressPercent": percent,
                    "bytesDownloaded": update.bytes_this_segment,
                    "totalBytes": serde_json::Value::Null,
                    "status": "downloading",
                }),
            );
        },
    );

    // Fetch the raw CMAF bundle. Requires an initialized CoreBridge →
    // QobuzClient; if either is missing, bail so the legacy path runs.
    let bundle = {
        let bridge_guard = bridge.read().await;
        let bridge = bridge_guard
            .as_ref()
            .ok_or_else(|| "CoreBridge not initialized".to_string())?;
        let client_arc = bridge.core().client();
        let client_guard = client_arc.read().await;
        let qobuz_client = client_guard
            .as_ref()
            .ok_or_else(|| "QobuzClient not initialized".to_string())?;
        qbz_qobuz::cmaf::download_raw_with_progress(
            qobuz_client,
            track_id,
            Quality::UltraHiRes,
            Some(progress_cb),
        )
        .await?
    };

    // Open (or lazily init) the secret vault and wrap the keying material
    // before it touches the filesystem.
    let vault = crate::offline_cache::secret_vault::get_or_init(&offline_root_path)
        .map_err(|e| format!("SecretBox init failed: {}", e))?;
    let content_key_wrapped = vault
        .wrap(&bundle.content_key)
        .map_err(|e| format!("Failed to wrap content_key: {}", e))?;
    let infos_wrapped = vault
        .wrap(bundle.infos.as_bytes())
        .map_err(|e| format!("Failed to wrap infos: {}", e))?;

    // Persist the encrypted bundle to disk.
    let (layout, total_bytes) =
        crate::offline_cache::cmaf_store::persist_bundle(&offline_root_path, track_id, &bundle)?;

    // Flip the DB row to v2 and store the wrapped keying material.
    {
        let db_guard = db.lock().await;
        let db_ref = db_guard
            .as_ref()
            .ok_or_else(|| "Offline cache DB not open".to_string())?;
        db_ref.set_cmaf_bundle(
            track_id,
            layout.segments_path.to_string_lossy().as_ref(),
            layout.init_path.to_string_lossy().as_ref(),
            &content_key_wrapped,
            &infos_wrapped,
            bundle.format_id,
            bundle.n_segments as u32,
            total_bytes,
        )?;
        db_ref
            .mark_complete(track_id, total_bytes)
            .map_err(|e| format!("Failed to mark_complete: {}", e))?;
    }

    // Fetch metadata for the library row (same source the legacy path uses).
    // We don't write FLAC tags or embed artwork INSIDE the encrypted blob
    // — that would corrupt it. But we DO save a cover.jpg next to the
    // bundle directory so the library UI has artwork to display.
    let metadata = {
        let qobuz_client = client.read().await;
        crate::offline_cache::metadata::fetch_complete_metadata(track_id, &*qobuz_client).await
    };
    if let Ok(metadata) = metadata {
        // Download and save album artwork alongside the bundle, same as
        // the legacy path does next to the FLAC file. cover.jpg lives at
        // <offline_root>/tracks-cmaf/<track_id>/cover.jpg — set as the
        // library row's artwork_path so the UI picks it up.
        let artwork_path: Option<String> = if let Some(artwork_url) = metadata.artwork_url.as_deref() {
            match crate::offline_cache::metadata::save_album_artwork(&layout.track_dir, artwork_url).await {
                Ok(()) => {
                    let cover = layout.track_dir.join("cover.jpg");
                    if cover.exists() {
                        Some(cover.to_string_lossy().to_string())
                    } else {
                        None
                    }
                }
                Err(e) => {
                    log::warn!("[Offline/CMAF] Track {} artwork save failed: {}", track_id, e);
                    None
                }
            }
        } else {
            None
        };

        let album_artist = metadata.album_artist.as_ref().unwrap_or(&metadata.artist);
        let album_group_key = format!("{}|{}", metadata.album, album_artist);
        let lib_opt = library_db.lock().await;
        if let Some(lib_guard) = lib_opt.as_ref() {
            let _ = lib_guard.insert_qobuz_cached_track_with_grouping(
                track_id,
                &metadata.title,
                &metadata.artist,
                Some(&metadata.album),
                metadata.album_artist.as_deref(),
                metadata.track_number,
                metadata.disc_number,
                metadata.year,
                metadata.duration_secs,
                // For v2 bundles the "playable path" in the library index
                // is the track directory; the player resolves it through
                // the DB's cache_format=2 branch anyway.
                layout.track_dir.to_string_lossy().as_ref(),
                &album_group_key,
                &metadata.album,
                bundle.bit_depth,
                bundle.sampling_rate.map(|r| r as f64),
                artwork_path.as_deref(),
            );
        }
    } else if let Err(e) = metadata {
        log::warn!(
            "[Offline/CMAF] Track {} post-metadata fetch failed: {} (bundle already persisted)",
            track_id,
            e
        );
    }

    log::info!(
        "[Offline/CMAF] Track {} cached as v2 bundle: {:.2} MB under {:?}",
        track_id,
        total_bytes as f64 / (1024.0 * 1024.0),
        layout.track_dir
    );
    let _ = app.emit(
        "offline:caching_completed",
        serde_json::json!({
            "trackId": track_id,
            "size": total_bytes,
            "format": "cmaf",
        }),
    );
    let _ = app.emit(
        "offline:caching_processed",
        serde_json::json!({
            "trackId": track_id,
            "path": layout.track_dir.to_string_lossy(),
            "format": "cmaf",
        }),
    );
    Ok(())
}

/// Used by both v2_cache_track_for_offline (single) and v2_cache_tracks_batch_for_offline (batch).
#[allow(clippy::too_many_arguments)]
fn spawn_track_cache_download(
    track_id: u64,
    file_path: std::path::PathBuf,
    client: std::sync::Arc<tokio::sync::RwLock<crate::api::QobuzClient>>,
    bridge: std::sync::Arc<tokio::sync::RwLock<Option<crate::core_bridge::CoreBridge>>>,
    fetcher: std::sync::Arc<crate::offline_cache::StreamFetcher>,
    db: std::sync::Arc<tokio::sync::Mutex<Option<crate::offline_cache::OfflineCacheDb>>>,
    offline_root: String,
    library_db: std::sync::Arc<tokio::sync::Mutex<Option<qbz_library::LibraryDatabase>>>,
    app: tauri::AppHandle,
    semaphore: std::sync::Arc<tokio::sync::Semaphore>,
) {
    tokio::spawn(async move {
        let _permit = match semaphore.acquire_owned().await {
            Ok(permit) => permit,
            Err(err) => {
                log::error!(
                    "Failed to acquire cache slot for track {}: {}",
                    track_id,
                    err
                );
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(
                        track_id,
                        crate::offline_cache::OfflineCacheStatus::Failed,
                        Some("Failed to start caching"),
                    );
                }
                let _ = app.emit(
                    "offline:caching_failed",
                    serde_json::json!({
                        "trackId": track_id,
                        "error": "Failed to acquire cache slot"
                    }),
                );
                return;
            }
        };

        if let Some(db_guard) = db.lock().await.as_ref() {
            let _ = db_guard.update_status(
                track_id,
                crate::offline_cache::OfflineCacheStatus::Downloading,
                None,
            );
        }
        let _ = app.emit(
            "offline:caching_started",
            serde_json::json!({ "trackId": track_id }),
        );

        // === CMAF-first offline download (v2 format) ===
        //
        // Stores bit-identical encrypted segments + wrapped content key.
        // Falls through to the legacy path below if any step fails (no
        // CoreBridge yet, /file/url returns a non-CMAF response, network
        // flake, vault init failure, etc.). The legacy fallback keeps
        // existing users unblocked while we validate the new path.
        match try_cmaf_offline_download(
            track_id,
            &bridge,
            &db,
            &offline_root,
            &library_db,
            &client,
            &app,
        )
        .await
        {
            Ok(()) => return,
            Err(e) => {
                log::warn!(
                    "[Offline/CMAF] Track {} — CMAF path failed ({}), falling back to legacy /track/getFileUrl",
                    track_id,
                    e
                );
            }
        }

        let stream_url = {
            let client_guard = client.read().await;
            client_guard
                .get_stream_url_with_fallback(track_id, crate::api::models::Quality::UltraHiRes)
                .await
        };

        let url = match stream_url {
            Ok(s) => s.url,
            Err(e) => {
                log::error!("Failed to get stream URL for track {}: {}", track_id, e);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(
                        track_id,
                        crate::offline_cache::OfflineCacheStatus::Failed,
                        Some(&format!("Failed to get stream URL: {}", e)),
                    );
                }
                let _ = app.emit(
                    "offline:caching_failed",
                    serde_json::json!({
                        "trackId": track_id,
                        "error": e.to_string()
                    }),
                );
                return;
            }
        };

        match fetcher
            .fetch_to_file(&url, &file_path, track_id, Some(&app))
            .await
        {
            Ok(size) => {
                log::info!("Caching complete for track {}: {} bytes", track_id, size);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.mark_complete(track_id, size);
                }
                let _ = app.emit(
                    "offline:caching_completed",
                    serde_json::json!({
                        "trackId": track_id,
                        "size": size
                    }),
                );

                // Post-processing kept in V2 to avoid command->command delegation.
                let file_path_str = file_path.to_string_lossy().to_string();
                let qobuz_client = client.read().await;
                let metadata = match crate::offline_cache::metadata::fetch_complete_metadata(
                    track_id,
                    &*qobuz_client,
                )
                .await
                {
                    Ok(m) => m,
                    Err(e) => {
                        log::warn!(
                            "Post-processing metadata fetch failed for {}: {}",
                            track_id,
                            e
                        );
                        return;
                    }
                };

                if let Err(e) =
                    crate::offline_cache::metadata::write_flac_tags(&file_path_str, &metadata)
                {
                    log::warn!("Failed to write tags for {}: {}", track_id, e);
                }
                if let Some(artwork_url) = &metadata.artwork_url {
                    if let Err(e) =
                        crate::offline_cache::metadata::embed_artwork(&file_path_str, artwork_url)
                            .await
                    {
                        log::warn!("Failed to embed artwork for {}: {}", track_id, e);
                    }
                }

                let new_path = match crate::offline_cache::metadata::organize_cached_file(
                    track_id,
                    &file_path_str,
                    &offline_root,
                    &metadata,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        log::warn!("Failed to organize cached file {}: {}", track_id, e);
                        return;
                    }
                };

                // Save cover.jpg next to the organized FLAC so the library
                // UI has artwork to display.
                let artwork_path_v1: Option<String> =
                    if let Some(artwork_url) = metadata.artwork_url.as_deref() {
                        if let Some(parent_dir) = std::path::Path::new(&new_path).parent() {
                            match crate::offline_cache::metadata::save_album_artwork(
                                parent_dir,
                                artwork_url,
                            )
                            .await
                            {
                                Ok(()) => {
                                    let cover = parent_dir.join("cover.jpg");
                                    if cover.exists() {
                                        Some(cover.to_string_lossy().to_string())
                                    } else {
                                        None
                                    }
                                }
                                Err(_) => None,
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                let (bit_depth_detected, sample_rate_detected) =
                    match lofty::read_from_path(&new_path) {
                        Ok(tagged_file) => {
                            use lofty::prelude::*;
                            let properties = tagged_file.properties();
                            (
                                properties.bit_depth().map(|bd| bd as u32),
                                properties.sample_rate().map(|sr| sr as f64),
                            )
                        }
                        Err(_) => (None, None),
                    };

                let album_artist = metadata.album_artist.as_ref().unwrap_or(&metadata.artist);
                let album_group_key = format!("{}|{}", metadata.album, album_artist);
                let lib_opt = library_db.lock().await;
                if let Some(lib_guard) = lib_opt.as_ref() {
                    let _ = lib_guard.insert_qobuz_cached_track_with_grouping(
                        track_id,
                        &metadata.title,
                        &metadata.artist,
                        Some(&metadata.album),
                        metadata.album_artist.as_deref(),
                        metadata.track_number,
                        metadata.disc_number,
                        metadata.year,
                        metadata.duration_secs,
                        &new_path,
                        &album_group_key,
                        &metadata.album,
                        bit_depth_detected,
                        sample_rate_detected,
                        artwork_path_v1.as_deref(),
                    );
                }

                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_file_path(track_id, &new_path);
                }

                let _ = app.emit(
                    "offline:caching_processed",
                    serde_json::json!({
                        "trackId": track_id,
                        "path": new_path
                    }),
                );
            }
            Err(e) => {
                log::error!("Caching failed for track {}: {}", track_id, e);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(
                        track_id,
                        crate::offline_cache::OfflineCacheStatus::Failed,
                        Some(&e),
                    );
                }
                let _ = app.emit(
                    "offline:caching_failed",
                    serde_json::json!({
                        "trackId": track_id,
                        "error": e
                    }),
                );
            }
        }
    });
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn v2_cache_track_for_offline(
    track_id: u64,
    title: String,
    artist: String,
    album: Option<String>,
    album_id: Option<String>,
    duration_secs: u64,
    quality: String,
    bit_depth: Option<u32>,
    sample_rate: Option<f64>,
    state: State<'_, AppState>,
    bridge: State<'_, crate::core_bridge::CoreBridgeState>,
    cache_state: State<'_, OfflineCacheState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    log::info!(
        "Command: v2_cache_track_for_offline {} - {} by {}",
        track_id,
        title,
        artist
    );

    let track_info = crate::offline_cache::TrackCacheInfo {
        track_id,
        title,
        artist,
        album,
        album_id,
        duration_secs,
        quality,
        bit_depth,
        sample_rate,
    };

    let file_path = cache_state.track_file_path(track_id, "flac");
    let file_path_str = file_path.to_string_lossy().to_string();
    {
        let guard = cache_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session - please log in")?;
        db.insert_track(&track_info, &file_path_str)?;
    }

    spawn_track_cache_download(
        track_id,
        file_path,
        state.client.clone(),
        bridge.0.clone(),
        cache_state.fetcher.clone(),
        cache_state.db.clone(),
        cache_state.get_cache_path(),
        cache_state.library_db.clone(),
        app_handle.clone(),
        cache_state.cache_semaphore.clone(),
    );

    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTrackInfo {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub duration_secs: u64,
    pub quality: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
}

#[tauri::command]
pub async fn v2_cache_tracks_batch_for_offline(
    tracks: Vec<BatchTrackInfo>,
    state: State<'_, AppState>,
    bridge: State<'_, crate::core_bridge::CoreBridgeState>,
    cache_state: State<'_, OfflineCacheState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    log::info!(
        "Command: v2_cache_tracks_batch_for_offline ({} tracks)",
        tracks.len()
    );

    // Build TrackCacheInfo + file_path pairs for batch insert
    let mut batch: Vec<(crate::offline_cache::TrackCacheInfo, String)> =
        Vec::with_capacity(tracks.len());
    for track in &tracks {
        let file_path = cache_state.track_file_path(track.id, "flac");
        let file_path_str = file_path.to_string_lossy().to_string();
        batch.push((
            crate::offline_cache::TrackCacheInfo {
                track_id: track.id,
                title: track.title.clone(),
                artist: track.artist.clone(),
                album: track.album.clone(),
                album_id: track.album_id.clone(),
                duration_secs: track.duration_secs,
                quality: track.quality.clone(),
                bit_depth: track.bit_depth,
                sample_rate: track.sample_rate,
            },
            file_path_str,
        ));
    }

    // Single transactional batch insert
    {
        let guard = cache_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session - please log in")?;
        let refs: Vec<(&crate::offline_cache::TrackCacheInfo, String)> = batch
            .iter()
            .map(|(info, path)| (info, path.clone()))
            .collect();
        db.insert_tracks_batch(&refs)?;
    }

    // Spawn download tasks for each track
    for track in &tracks {
        let file_path = cache_state.track_file_path(track.id, "flac");
        spawn_track_cache_download(
            track.id,
            file_path,
            state.client.clone(),
            bridge.0.clone(),
            cache_state.fetcher.clone(),
            cache_state.db.clone(),
            cache_state.get_cache_path(),
            cache_state.library_db.clone(),
            app_handle.clone(),
            cache_state.cache_semaphore.clone(),
        );
    }

    Ok(())
}

#[tauri::command]
pub async fn v2_start_legacy_migration(
    state: State<'_, AppState>,
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, crate::library::LibraryState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    log::info!("Command: v2_start_legacy_migration");
    let tracks_dir = cache_state.cache_dir.read().unwrap().join("tracks");
    let track_ids = crate::offline_cache::detect_legacy_cached_files(&tracks_dir)?;

    if track_ids.is_empty() {
        return Err("No legacy cached files found".to_string());
    }

    let offline_root = cache_state.get_cache_path();
    let qobuz_client = state.client.clone();
    let library_db = library_state.db.clone();
    let app_complete = app_handle.clone();

    tokio::spawn(async move {
        let status = crate::offline_cache::migrate_legacy_cached_files(
            track_ids,
            tracks_dir,
            offline_root,
            qobuz_client,
            library_db,
        )
        .await;
        let _ = app_complete.emit("migration:complete", status);
    });
    Ok(())
}

// === Library scan / metadata helpers (inlined from legacy library/commands.rs) ===

fn library_normalize_path(path: &std::path::Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

async fn library_process_cue_file(
    cue_path: &std::path::Path,
    state: &State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    let mut cue =
        crate::library::CueParser::parse(cue_path).map_err(|e| e.to_string())?;

    // Get audio file properties
    let audio_path = library_normalize_path(std::path::Path::new(&cue.audio_file));
    if !audio_path.exists() {
        return Err(format!("Audio file not found: {}", cue.audio_file));
    }
    cue.audio_file = audio_path.to_string_lossy().to_string();

    let properties = crate::library::MetadataExtractor::extract_properties(audio_path.as_path())
        .map_err(|e| e.to_string())?;
    let format = crate::library::MetadataExtractor::detect_format(audio_path.as_path());

    // Convert CUE to tracks
    let mut tracks = crate::library::cue_to_tracks(&cue, properties.duration_secs, format, &properties);

    // Apply sidecar overrides if present (matches by file_path + cue_start_secs)
    if let Some(group_key) = tracks
        .first()
        .map(|track| track.album_group_key.trim().to_string())
        .filter(|k| !k.is_empty())
    {
        let album_dir = std::path::Path::new(&group_key);
        if album_dir.is_dir() {
            if let Ok(Some(sidecar)) = crate::library::read_album_sidecar(album_dir) {
                for track in tracks.iter_mut() {
                    crate::library::apply_sidecar_to_track(track, &sidecar);
                }
            }
        }
    }

    let artwork_cache = crate::library::get_artwork_cache_dir();
    let mut artwork_path =
        crate::library::MetadataExtractor::extract_artwork(audio_path.as_path(), &artwork_cache);
    if artwork_path.is_none() {
        if let Some(folder_art) = crate::library::MetadataExtractor::find_folder_artwork(
            audio_path.as_path(),
            cue.title.as_deref(),
        ) {
            artwork_path = crate::library::MetadataExtractor::cache_artwork_file(
                std::path::Path::new(&folder_art),
                &artwork_cache,
            );
        }
    }
    if let Some(path) = artwork_path.as_ref() {
        for track in tracks.iter_mut() {
            track.artwork_path = Some(path.clone());
        }
    }

    // Insert tracks
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let group_key = tracks
        .first()
        .map(|track| track.album_group_key.clone())
        .unwrap_or_default();

    for track in tracks {
        db.insert_track(&track).map_err(|e| e.to_string())?;
    }

    if let (Some(path), false) = (artwork_path.as_ref(), group_key.is_empty()) {
        let _ = db.update_album_group_artwork(&group_key, path);
    }

    Ok(())
}

fn library_apply_sidecar_override_if_present(
    track: &mut LocalTrack,
    cache: &mut std::collections::HashMap<String, Option<crate::library::AlbumTagSidecar>>,
) {
    let group_key = track.album_group_key.trim();
    if group_key.is_empty() {
        return;
    }

    let cached = cache.entry(group_key.to_string()).or_insert_with(|| {
        let album_dir = std::path::Path::new(group_key);
        if !album_dir.is_dir() {
            return None;
        }

        match crate::library::read_album_sidecar(album_dir) {
            Ok(sidecar) => sidecar,
            Err(err) => {
                log::warn!(
                    "Failed to read LocalLibrary sidecar for {}: {}",
                    album_dir.display(),
                    err
                );
                None
            }
        }
    });

    if let Some(sidecar) = cached.as_ref() {
        crate::library::apply_sidecar_to_track(track, sidecar);
    }
}

fn library_compute_track_artist_match(tracks: &[LocalTrack]) -> Option<String> {
    let mut artists: HashSet<String> = HashSet::new();
    for track in tracks {
        let value = track
            .album_artist
            .as_deref()
            .unwrap_or(track.artist.as_str())
            .trim();
        if value.is_empty() {
            continue;
        }
        artists.insert(value.to_string());
        if artists.len() > 1 {
            return None;
        }
    }

    artists.into_iter().next()
}

#[tauri::command]
pub async fn v2_library_scan(
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    use crate::library::{ScanError, ScanProgress, ScanStatus, LibraryScanner, MetadataExtractor, CueParser, get_artwork_cache_dir};

    log::info!("Command: library_scan");

    // Get folders to scan
    let folders = {
        let guard__ = state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.get_folders().map_err(|e| e.to_string())?
    };

    if folders.is_empty() {
        return Err("No library folders configured".to_string());
    }

    // Reset cancel flag and progress
    state.scan_cancel.store(false, Ordering::Relaxed);
    {
        let mut progress = state.scan_progress.lock().await;
        *progress = ScanProgress {
            status: ScanStatus::Scanning,
            total_files: 0,
            processed_files: 0,
            current_file: None,
            errors: Vec::new(),
        };
    }

    let scanner = LibraryScanner::new();
    let mut all_errors: Vec<ScanError> = Vec::new();
    let mut sidecar_cache: std::collections::HashMap<String, Option<crate::library::AlbumTagSidecar>> =
        std::collections::HashMap::new();

    for folder in &folders {
        log::info!("Scanning folder: {}", folder);

        // Scan for files
        let scan_result = match scanner.scan_directory(std::path::Path::new(folder)) {
            Ok(result) => result,
            Err(e) => {
                all_errors.push(ScanError {
                    file_path: folder.clone(),
                    error: e.to_string(),
                });
                continue;
            }
        };

        let total_files = scan_result.audio_files.len() + scan_result.cue_files.len();

        // Update total count
        {
            let mut progress = state.scan_progress.lock().await;
            progress.total_files += total_files as u32;
        }

        // Process CUE files first (they create multiple tracks from one file)
        for cue_path in &scan_result.cue_files {
            // Check for cancellation
            if state.scan_cancel.load(Ordering::Relaxed) {
                let mut progress = state.scan_progress.lock().await;
                progress.status = ScanStatus::Cancelled;
                progress.current_file = None;
                log::info!("Library scan cancelled by user");
                return Ok(());
            }

            {
                let mut progress = state.scan_progress.lock().await;
                progress.current_file = Some(cue_path.to_string_lossy().to_string());
            }

            match library_process_cue_file(cue_path, &state).await {
                Ok(_) => {}
                Err(e) => {
                    all_errors.push(ScanError {
                        file_path: cue_path.to_string_lossy().to_string(),
                        error: e,
                    });
                }
            }

            {
                let mut progress = state.scan_progress.lock().await;
                progress.processed_files += 1;
            }
        }

        // Process regular audio files (skip if covered by CUE)
        let cue_audio_files: HashSet<String> = scan_result
            .cue_files
            .iter()
            .filter_map(|p| {
                CueParser::parse(p).ok().map(|cue| {
                    library_normalize_path(std::path::Path::new(&cue.audio_file))
                        .to_string_lossy()
                        .to_string()
                })
            })
            .collect();

        for audio_path in &scan_result.audio_files {
            // Check for cancellation
            if state.scan_cancel.load(Ordering::Relaxed) {
                let mut progress = state.scan_progress.lock().await;
                progress.status = ScanStatus::Cancelled;
                progress.current_file = None;
                log::info!("Library scan cancelled by user");
                return Ok(());
            }

            // Skip if this file is referenced by a CUE sheet
            let canonical_path = library_normalize_path(audio_path);
            let path_str = canonical_path.to_string_lossy().to_string();
            if cue_audio_files.contains(&path_str) {
                let mut progress = state.scan_progress.lock().await;
                progress.processed_files += 1;
                continue;
            }

            {
                let mut progress = state.scan_progress.lock().await;
                progress.current_file = Some(path_str.clone());
            }

            match MetadataExtractor::extract(&canonical_path) {
                Ok(mut track) => {
                    library_apply_sidecar_override_if_present(&mut track, &mut sidecar_cache);
                    // Try to extract embedded artwork, fallback to cached folder artwork
                    let artwork_cache = get_artwork_cache_dir();
                    let mut artwork_path =
                        MetadataExtractor::extract_artwork(&canonical_path, &artwork_cache);
                    if artwork_path.is_none() {
                        let album_hint = if !track.album_group_title.is_empty() {
                            Some(track.album_group_title.as_str())
                        } else {
                            Some(track.album.as_str())
                        };
                        if let Some(folder_art) =
                            MetadataExtractor::find_folder_artwork(&canonical_path, album_hint)
                        {
                            artwork_path = MetadataExtractor::cache_artwork_file(
                                std::path::Path::new(&folder_art),
                                &artwork_cache,
                            );
                        }
                    }
                    track.artwork_path = artwork_path;

                    let guard__ = state.db.lock().await;
                    let db = guard__
                        .as_ref()
                        .ok_or("No active session - please log in")?;
                    if let Err(e) = db.insert_track(&track) {
                        all_errors.push(ScanError {
                            file_path: path_str,
                            error: e.to_string(),
                        });
                    } else if let (Some(artwork_path), false) = (
                        track.artwork_path.as_ref(),
                        track.album_group_key.is_empty(),
                    ) {
                        let _ = db.update_album_group_artwork(&track.album_group_key, artwork_path);
                    }
                }
                Err(e) => {
                    all_errors.push(ScanError {
                        file_path: path_str,
                        error: e.to_string(),
                    });
                }
            }

            {
                let mut progress = state.scan_progress.lock().await;
                progress.processed_files += 1;
            }
        }
    }

    // Clean up tracks whose files no longer exist on disk
    {
        let mut progress = state.scan_progress.lock().await;
        progress.current_file = Some("Cleaning up missing files...".to_string());
    }
    {
        let guard__ = state.db.lock().await;
        if let Some(db) = guard__.as_ref() {
            if let Ok(tracks) = db.get_all_track_paths() {
                let missing_ids: Vec<i64> = tracks
                    .iter()
                    .filter(|(_, path)| !std::path::Path::new(path).exists())
                    .map(|(id, _)| *id)
                    .collect();
                if !missing_ids.is_empty() {
                    log::info!("Removing {} tracks with missing files", missing_ids.len());
                    for chunk in missing_ids.chunks(500) {
                        if let Err(e) = db.delete_tracks_by_ids(chunk) {
                            log::error!("Failed to delete missing tracks: {}", e);
                        }
                    }
                }
            }
        }
    }

    // Mark complete
    {
        let mut progress = state.scan_progress.lock().await;
        progress.status = if all_errors.is_empty() {
            ScanStatus::Complete
        } else {
            ScanStatus::Complete // Still complete, but with errors
        };
        progress.current_file = None;
        progress.errors = all_errors;
    }

    log::info!("Library scan complete");
    Ok(())
}

#[tauri::command]
pub async fn v2_library_stop_scan(
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    log::info!("Command: library_stop_scan");
    state.scan_cancel.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn v2_library_scan_folder(
    folder_id: i64,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    use crate::library::{ScanError, ScanProgress, ScanStatus, LibraryScanner, MetadataExtractor, CueParser, get_artwork_cache_dir};

    log::info!("Command: library_scan_folder {}", folder_id);

    // Get folder info
    let folder = {
        let guard__ = state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.get_folder_by_id(folder_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Folder with ID {} not found", folder_id))?
    };

    if !folder.enabled {
        return Err("Cannot scan disabled folder".to_string());
    }

    // Refresh network detection if not user overridden
    if !folder.user_override_network {
        let path = std::path::Path::new(&folder.path);
        let network_info = crate::network::is_network_path(path);

        if network_info.is_network != folder.is_network {
            log::info!(
                "Updating network status for folder {} during scan: {} -> {}",
                folder.path,
                folder.is_network,
                network_info.is_network
            );

            let fs_type = network_info.mount_info.as_ref().and_then(|mi| {
                if let crate::network::MountKind::Network(nfs) = &mi.kind {
                    Some(format!("{:?}", nfs).to_lowercase())
                } else {
                    None
                }
            });

            let guard__ = state.db.lock().await;
            let db = guard__
                .as_ref()
                .ok_or("No active session - please log in")?;
            let _ = db.update_folder_settings(
                folder.id,
                folder.alias.as_deref(),
                folder.enabled,
                network_info.is_network,
                fs_type.as_deref(),
                false,
            );
        }
    }

    // Reset cancel flag and progress
    state.scan_cancel.store(false, Ordering::Relaxed);
    {
        let mut progress = state.scan_progress.lock().await;
        *progress = ScanProgress {
            status: ScanStatus::Scanning,
            total_files: 0,
            processed_files: 0,
            current_file: None,
            errors: Vec::new(),
        };
    }

    let scanner = LibraryScanner::new();
    let mut all_errors: Vec<ScanError> = Vec::new();
    let mut sidecar_cache: std::collections::HashMap<String, Option<crate::library::AlbumTagSidecar>> =
        std::collections::HashMap::new();

    log::info!("Scanning single folder: {}", folder.path);

    // Scan for files
    let scan_result = match scanner.scan_directory(std::path::Path::new(&folder.path)) {
        Ok(result) => result,
        Err(e) => {
            let mut progress = state.scan_progress.lock().await;
            progress.status = ScanStatus::Complete;
            progress.errors = vec![ScanError {
                file_path: folder.path.clone(),
                error: e.to_string(),
            }];
            return Err(e.to_string());
        }
    };

    let total_files = scan_result.audio_files.len() + scan_result.cue_files.len();

    // Update total count
    {
        let mut progress = state.scan_progress.lock().await;
        progress.total_files = total_files as u32;
    }

    // Process CUE files first
    for cue_path in &scan_result.cue_files {
        if state.scan_cancel.load(Ordering::Relaxed) {
            let mut progress = state.scan_progress.lock().await;
            progress.status = ScanStatus::Cancelled;
            progress.current_file = None;
            log::info!("Library scan cancelled by user");
            return Ok(());
        }

        {
            let mut progress = state.scan_progress.lock().await;
            progress.current_file = Some(cue_path.to_string_lossy().to_string());
        }

        match library_process_cue_file(cue_path, &state).await {
            Ok(_) => {}
            Err(e) => {
                all_errors.push(ScanError {
                    file_path: cue_path.to_string_lossy().to_string(),
                    error: e,
                });
            }
        }

        {
            let mut progress = state.scan_progress.lock().await;
            progress.processed_files += 1;
        }
    }

    // Process regular audio files
    let cue_audio_files: HashSet<String> = scan_result
        .cue_files
        .iter()
        .filter_map(|p| {
            CueParser::parse(p).ok().map(|cue| {
                library_normalize_path(std::path::Path::new(&cue.audio_file))
                    .to_string_lossy()
                    .to_string()
            })
        })
        .collect();

    for audio_path in &scan_result.audio_files {
        if state.scan_cancel.load(Ordering::Relaxed) {
            let mut progress = state.scan_progress.lock().await;
            progress.status = ScanStatus::Cancelled;
            progress.current_file = None;
            log::info!("Library scan cancelled by user");
            return Ok(());
        }

        let canonical_path = library_normalize_path(audio_path);
        let path_str = canonical_path.to_string_lossy().to_string();
        if cue_audio_files.contains(&path_str) {
            let mut progress = state.scan_progress.lock().await;
            progress.processed_files += 1;
            continue;
        }

        {
            let mut progress = state.scan_progress.lock().await;
            progress.current_file = Some(path_str.clone());
        }

        match MetadataExtractor::extract(&canonical_path) {
            Ok(mut track) => {
                library_apply_sidecar_override_if_present(&mut track, &mut sidecar_cache);
                let artwork_cache = get_artwork_cache_dir();
                let mut artwork_path =
                    MetadataExtractor::extract_artwork(&canonical_path, &artwork_cache);
                if artwork_path.is_none() {
                    if let Some(folder_art) = MetadataExtractor::find_folder_artwork(
                        canonical_path.as_path(),
                        Some(&track.album),
                    ) {
                        artwork_path = MetadataExtractor::cache_artwork_file(
                            std::path::Path::new(&folder_art),
                            &artwork_cache,
                        );
                    }
                }
                track.artwork_path = artwork_path.clone();

                let guard__ = state.db.lock().await;
                let db = guard__
                    .as_ref()
                    .ok_or("No active session - please log in")?;
                let group_key = track.album_group_key.clone();
                if let Err(e) = db.insert_track(&track) {
                    all_errors.push(ScanError {
                        file_path: path_str,
                        error: e.to_string(),
                    });
                } else if let Some(path) = artwork_path.as_ref() {
                    if !group_key.is_empty() {
                        let _ = db.update_album_group_artwork(&group_key, path);
                    }
                }
            }
            Err(e) => {
                all_errors.push(ScanError {
                    file_path: path_str,
                    error: e.to_string(),
                });
            }
        }

        {
            let mut progress = state.scan_progress.lock().await;
            progress.processed_files += 1;
        }
    }

    // Clean up tracks in this folder whose files no longer exist on disk
    {
        let mut progress = state.scan_progress.lock().await;
        progress.current_file = Some("Cleaning up missing files...".to_string());
    }
    {
        let guard__ = state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        if let Ok(tracks) = db.get_all_track_paths() {
            let folder_prefix = if folder.path.ends_with('/') {
                folder.path.clone()
            } else {
                format!("{}/", folder.path)
            };
            let missing_ids: Vec<i64> = tracks
                .iter()
                .filter(|(_, path)| {
                    path.starts_with(&folder_prefix) && !std::path::Path::new(path).exists()
                })
                .map(|(id, _)| *id)
                .collect();
            if !missing_ids.is_empty() {
                log::info!(
                    "Removing {} tracks with missing files from folder {}",
                    missing_ids.len(),
                    folder.path
                );
                for chunk in missing_ids.chunks(500) {
                    if let Err(e) = db.delete_tracks_by_ids(chunk) {
                        log::error!("Failed to delete missing tracks: {}", e);
                    }
                }
            }
        }

        // Update folder scan time
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = db.update_folder_scan_time(&folder.path, now);
    }

    // Update final status
    {
        let mut progress = state.scan_progress.lock().await;
        progress.status = ScanStatus::Complete;
        progress.current_file = None;
        progress.errors = all_errors;
    }

    log::info!("Single folder scan complete");
    Ok(())
}

#[tauri::command]
pub async fn v2_library_clear(
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    log::info!("Command: library_clear");

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.clear_all_tracks().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_library_update_album_metadata(
    request: crate::library::LibraryAlbumMetadataUpdateRequest,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: library_update_album_metadata {}",
        request.album_group_key
    );

    if request.album_group_key.trim().is_empty() {
        return Err("Album ID is required.".to_string());
    }
    if request.album_title.trim().is_empty() {
        return Err("Album title is required.".to_string());
    }
    if request.tracks.is_empty() {
        return Err("Album track list is empty.".to_string());
    }

    let album_dir = PathBuf::from(request.album_group_key.trim());
    if !album_dir.is_dir() {
        return Err("Album folder not found on disk.".to_string());
    }

    // Write sidecar first (persistence), then update DB.
    let sidecar_result = tokio::task::spawn_blocking({
        let album_dir = album_dir.clone();
        let request = request.clone();
        move || -> Result<(), String> {
            let album = crate::library::AlbumMetadataOverride {
                album_title: Some(request.album_title.trim().to_string()),
                album_artist: Some(request.album_artist.trim().to_string())
                    .filter(|s| !s.is_empty()),
                year: request.year,
                genre: request
                    .genre
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                catalog_number: request
                    .catalog_number
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            };

            let tracks = request
                .tracks
                .iter()
                .map(|t| crate::library::TrackMetadataOverride {
                    file_path: t.file_path.clone(),
                    cue_start_secs: t.cue_start_secs,
                    title: Some(t.title.trim().to_string()),
                    disc_number: t.disc_number,
                    track_number: t.track_number,
                })
                .collect::<Vec<_>>();

            let sidecar = crate::library::AlbumTagSidecar::new(album, tracks);
            crate::library::write_album_sidecar(&album_dir, &sidecar)
                .map_err(|e| format!("Failed to write sidecar: {}", e))?;

            Ok(())
        }
    })
    .await
    .map_err(|e| format!("Failed to write sidecar: {}", e))?;
    sidecar_result?;

    let mut guard__ = state.db.lock().await;
    let db = guard__
        .as_mut()
        .ok_or("No active session - please log in")?;
    let existing_tracks = db
        .get_album_tracks(&request.album_group_key)
        .map_err(|e| e.to_string())?;

    let track_artist_match = library_compute_track_artist_match(&existing_tracks);
    let track_updates = request
        .tracks
        .iter()
        .map(|t| crate::library::AlbumTrackUpdate {
            id: t.id,
            title: t.title.clone(),
            disc_number: t.disc_number,
            track_number: t.track_number,
        })
        .collect::<Vec<_>>();

    db.update_album_group_metadata(
        &request.album_group_key,
        &request.album_title,
        &request.album_artist,
        request.year,
        request.genre.as_deref(),
        request.catalog_number.as_deref(),
        track_artist_match.as_deref(),
        &track_updates,
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn v2_library_write_album_metadata_to_files(
    app: tauri::AppHandle,
    request: crate::library::LibraryAlbumMetadataUpdateRequest,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    use std::collections::HashMap;
    use crate::library::LibraryAlbumTrackMetadataUpdate;

    log::info!(
        "Command: library_write_album_metadata_to_files {}",
        request.album_group_key
    );

    if request.album_group_key.trim().is_empty() {
        return Err("Album ID is required.".to_string());
    }
    if request.album_title.trim().is_empty() {
        return Err("Album title is required.".to_string());
    }
    if request.tracks.is_empty() {
        return Err("Album track list is empty.".to_string());
    }

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let existing_tracks = db
        .get_album_tracks(&request.album_group_key)
        .map_err(|e| e.to_string())?;
    if existing_tracks
        .iter()
        .any(|t| t.cue_file_path.is_some() || t.cue_start_secs.is_some())
    {
        return Err("Writing tags to files is not supported for CUE-based albums. Use sidecar mode instead.".to_string());
    }
    drop(guard__);

    let album_dir = PathBuf::from(request.album_group_key.trim());
    if !album_dir.is_dir() {
        return Err("Album folder not found on disk.".to_string());
    }

    // Write embedded tags for each track file.
    let write_result = tokio::task::spawn_blocking({
        let request = request.clone();
        move || -> Result<(), String> {
            use lofty::prelude::*;
            use lofty::tag::{ItemKey, Tag};
            use lofty::config::WriteOptions;

            // Ensure we only write each file once.
            let mut by_file: HashMap<String, &LibraryAlbumTrackMetadataUpdate> = HashMap::new();
            for track in &request.tracks {
                by_file.entry(track.file_path.clone()).or_insert(track);
            }

            let total = by_file.len();
            let mut current = 0usize;

            for (file_path, track) in by_file {
                current += 1;
                // Emit progress event
                let _ = app.emit("library:tag_write_progress", serde_json::json!({
                    "current": current,
                    "total": total
                }));
                let path = std::path::Path::new(&file_path);
                if !path.is_file() {
                    return Err("One or more audio files were not found on disk.".to_string());
                }

                let mut tagged_file =
                    lofty::read_from_path(path).map_err(|_| "Failed to read audio file tags.".to_string())?;

                let primary_type = tagged_file.primary_tag_type();
                if tagged_file.primary_tag_mut().is_none() && tagged_file.first_tag_mut().is_none() {
                    tagged_file.insert_tag(Tag::new(primary_type));
                }

                {
                    let tag = if let Some(tag) = tagged_file.primary_tag_mut() {
                        tag
                    } else if let Some(tag) = tagged_file.first_tag_mut() {
                        tag
                    } else {
                        return Err("Failed to access audio file tags.".to_string());
                    };

                    tag.set_title(track.title.trim().to_string());
                    tag.set_album(request.album_title.trim().to_string());
                    tag.set_artist(request.album_artist.trim().to_string());

                    if let Some(no) = track.track_number {
                        tag.set_track(no);
                    }
                    if let Some(disc) = track.disc_number {
                        tag.set_disk(disc);
                    }

                    // Album artist (not part of Accessor).
                    if request.album_artist.trim().is_empty() {
                        tag.remove_key(ItemKey::AlbumArtist);
                    } else {
                        tag.insert_text(ItemKey::AlbumArtist, request.album_artist.trim().to_string());
                    }

                    // Year / Genre
                    if let Some(year) = request.year {
                        tag.set_date(lofty::tag::items::Timestamp {
                            year: year as u16,
                            ..Default::default()
                        });
                    } else {
                        tag.remove_date();
                    }

                    if let Some(ref genre) = request.genre {
                        let g = genre.trim();
                        if g.is_empty() {
                            tag.remove_genre();
                        } else {
                            tag.set_genre(g.to_string());
                        }
                    } else {
                        tag.remove_genre();
                    }

                    if let Some(ref cat) = request.catalog_number {
                        let c = cat.trim();
                        if c.is_empty() {
                            tag.remove_key(ItemKey::CatalogNumber);
                        } else {
                            tag.insert_text(ItemKey::CatalogNumber, c.to_string());
                        }
                    } else {
                        tag.remove_key(ItemKey::CatalogNumber);
                    }
                }

                tagged_file
                    .save_to_path(path, WriteOptions::default())
                    .map_err(|_| "Failed to write tags to audio files. Check that the album folder is mounted read-write and you have permissions.".to_string())?;
            }

            Ok(())
        }
    })
    .await
    .map_err(|e| format!("Failed to write tags: {}", e))?;
    write_result?;

    // Remove sidecar (direct-edit mode disables sidecar persistence).
    let _ = tokio::task::spawn_blocking({
        let album_dir = album_dir.clone();
        move || crate::library::delete_album_sidecar(&album_dir)
    })
    .await;

    // Update DB from the requested values.
    let mut guard__ = state.db.lock().await;
    let db = guard__
        .as_mut()
        .ok_or("No active session - please log in")?;
    let existing_tracks = db
        .get_album_tracks(&request.album_group_key)
        .map_err(|e| e.to_string())?;
    let track_artist_match = library_compute_track_artist_match(&existing_tracks);
    let track_updates = request
        .tracks
        .iter()
        .map(|t| crate::library::AlbumTrackUpdate {
            id: t.id,
            title: t.title.clone(),
            disc_number: t.disc_number,
            track_number: t.track_number,
        })
        .collect::<Vec<_>>();

    db.update_album_group_metadata(
        &request.album_group_key,
        &request.album_title,
        &request.album_artist,
        request.year,
        request.genre.as_deref(),
        request.catalog_number.as_deref(),
        track_artist_match.as_deref(),
        &track_updates,
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn v2_library_refresh_album_metadata_from_files(
    album_group_key: String,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: library_refresh_album_metadata_from_files {}",
        album_group_key
    );

    if album_group_key.trim().is_empty() {
        return Err("Album ID is required.".to_string());
    }

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let existing_tracks = db
        .get_album_tracks(&album_group_key)
        .map_err(|e| e.to_string())?;
    if existing_tracks.is_empty() {
        return Err("Album not found.".to_string());
    }
    if existing_tracks
        .iter()
        .any(|t| t.cue_file_path.is_some() || t.cue_start_secs.is_some())
    {
        return Err(
            "Refreshing metadata from files is not supported for CUE-based albums.".to_string(),
        );
    }
    drop(guard__);

    let album_dir = PathBuf::from(album_group_key.trim());
    if !album_dir.is_dir() {
        return Err("Album folder not found on disk.".to_string());
    }

    // Delete sidecar, then refresh DB from embedded tags.
    let refresh = tokio::task::spawn_blocking({
        let existing_tracks = existing_tracks.clone();
        let album_dir = album_dir.clone();
        move || -> Result<Vec<crate::library::TrackMetadataUpdateFull>, String> {
            let _ = crate::library::delete_album_sidecar(&album_dir);

            let mut updates: Vec<crate::library::TrackMetadataUpdateFull> = Vec::new();
            for track in existing_tracks {
                let path = std::path::Path::new(&track.file_path);
                if !path.is_file() {
                    return Err("One or more audio files were not found on disk.".to_string());
                }

                let extracted = crate::library::MetadataExtractor::extract(path)
                    .map_err(|_| "Failed to read audio file tags.".to_string())?;

                let album_group_title = if extracted.album_group_title.trim().is_empty() {
                    extracted.album.clone()
                } else {
                    extracted.album_group_title.clone()
                };

                updates.push(crate::library::TrackMetadataUpdateFull {
                    id: track.id,
                    title: extracted.title,
                    artist: extracted.artist,
                    album: extracted.album,
                    album_artist: extracted.album_artist,
                    album_group_title,
                    track_number: extracted.track_number,
                    disc_number: extracted.disc_number,
                    year: extracted.year,
                    genre: extracted.genre,
                    catalog_number: extracted.catalog_number,
                });
            }

            Ok(updates)
        }
    })
    .await
    .map_err(|e| format!("Failed to refresh metadata: {}", e))?;
    let updates = refresh?;

    let mut guard__ = state.db.lock().await;
    let db = guard__
        .as_mut()
        .ok_or("No active session - please log in")?;
    db.update_tracks_metadata_by_id(&updates)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn v2_factory_reset(
    app_state: State<'_, AppState>,
    user_paths: State<'_, crate::user_data::UserDataPaths>,
    session_store: State<'_, crate::session_store::SessionStoreState>,
    favorites_cache: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
    subscription_state: State<'_, crate::config::subscription_state::SubscriptionStateState>,
    playback_prefs: State<'_, crate::config::playback_preferences::PlaybackPreferencesState>,
    favorites_prefs: State<'_, crate::config::favorites_preferences::FavoritesPreferencesState>,
    download_settings: State<'_, crate::config::download_settings::DownloadSettingsState>,
    audio_settings: State<'_, crate::config::audio_settings::AudioSettingsState>,
    tray_settings: State<'_, crate::config::tray_settings::TraySettingsState>,
    remote_control_settings: State<
        '_,
        crate::config::remote_control_settings::RemoteControlSettingsState,
    >,
    allowed_origins: State<'_, crate::config::remote_control_settings::AllowedOriginsState>,
    legal_settings: State<'_, crate::config::legal_settings::LegalSettingsState>,
    updates: State<'_, crate::updates::UpdatesState>,
    library: State<'_, crate::library::LibraryState>,
    reco: State<'_, crate::reco_store::RecoState>,
    api_cache: State<'_, crate::api_cache::ApiCacheState>,
    artist_vectors: State<'_, crate::artist_vectors::ArtistVectorStoreState>,
    blacklist: State<'_, crate::artist_blacklist::BlacklistState>,
    offline: State<'_, crate::offline::OfflineState>,
    offline_cache: State<'_, crate::offline_cache::OfflineCacheState>,
    lyrics: State<'_, crate::lyrics::LyricsState>,
    musicbrainz: State<'_, MusicBrainzSharedState>,
    listenbrainz: State<'_, crate::listenbrainz::ListenBrainzSharedState>,
    listenbrainz_v2: State<'_, ListenBrainzV2State>,
    musicbrainz_v2: State<'_, MusicBrainzV2State>,
    lastfm_v2: State<'_, LastFmV2State>,
) -> Result<(), String> {
    log::warn!("FACTORY RESET: Starting - all application data will be deleted");

    let _ = app_state.player.stop();
    app_state.media_controls.set_stopped();

    session_store.teardown();
    let _ = favorites_cache.teardown();
    let _ = playback_prefs.teardown();
    let _ = favorites_prefs.teardown();
    let _ = audio_settings.teardown();
    let _ = tray_settings.teardown();
    let _ = remote_control_settings.teardown();
    let _ = allowed_origins.teardown();
    updates.teardown();
    library.teardown().await;
    reco.teardown().await;
    api_cache.teardown().await;
    artist_vectors.teardown().await;
    blacklist.teardown();
    offline.teardown();
    offline_cache.teardown().await;
    lyrics.teardown().await;
    musicbrainz.teardown().await;
    listenbrainz.teardown().await;

    // Teardown V2 integration states
    listenbrainz_v2.clear_credentials().await;
    listenbrainz_v2.teardown().await;
    musicbrainz_v2.teardown().await;
    lastfm_v2.clear_session().await;
    v2_teardown_type_alias_state(&*subscription_state);
    v2_teardown_type_alias_state(&*download_settings);
    v2_teardown_type_alias_state(&*legal_settings);

    user_paths.clear_user();
    crate::user_data::UserDataPaths::clear_last_user_id();

    if let Err(e) = crate::credentials::clear_qobuz_credentials() {
        log::error!("FACTORY RESET: Failed to clear credentials: {}", e);
    }

    if let Ok(data_dir) = crate::user_data::UserDataPaths::global_data_dir() {
        if data_dir.exists() {
            let _ = std::fs::remove_dir_all(&data_dir);
        }
    }
    if let Ok(cache_dir) = crate::user_data::UserDataPaths::global_cache_dir() {
        if cache_dir.exists() {
            let _ = std::fs::remove_dir_all(&cache_dir);
        }
    }
    if let Some(config_dir) = dirs::config_dir().map(|d| d.join("qbz")) {
        if config_dir.exists() {
            let _ = std::fs::remove_dir_all(&config_dir);
        }
    }

    log::warn!("FACTORY RESET: Complete - all application data deleted");
    Ok(())
}

#[tauri::command]
pub fn v2_set_qobuz_tos_accepted(
    state: State<'_, crate::config::legal_settings::LegalSettingsState>,
    accepted: bool,
) -> Result<(), String> {
    crate::config::legal_settings::set_qobuz_tos_accepted(state, accepted)
}

#[tauri::command]
pub fn v2_get_qobuz_tos_accepted(
    state: State<'_, crate::config::legal_settings::LegalSettingsState>,
) -> Result<bool, String> {
    crate::config::legal_settings::get_qobuz_tos_accepted(state)
}

#[tauri::command]
pub fn v2_set_update_check_on_launch(
    enabled: bool,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::set_update_check_on_launch(enabled, state)
}

#[tauri::command]
pub fn v2_set_show_whats_new_on_launch(
    enabled: bool,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::set_show_whats_new_on_launch(enabled, state)
}

#[tauri::command]
pub fn v2_get_update_preferences(
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<crate::updates::UpdatePreferences, String> {
    crate::updates::get_update_preferences(state)
}

#[tauri::command]
pub fn v2_get_current_version(state: State<'_, crate::updates::UpdatesState>) -> String {
    crate::updates::get_current_version(state)
}

#[tauri::command]
pub async fn v2_check_for_updates(
    mode: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<crate::updates::UpdateCheckResult, String> {
    crate::updates::check_for_updates(mode, state).await
}

#[tauri::command]
pub async fn v2_fetch_release_for_version(
    version: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<Option<crate::updates::ReleaseInfo>, String> {
    crate::updates::fetch_release_for_version(version, state).await
}

#[tauri::command]
pub fn v2_acknowledge_release(
    version: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::acknowledge_release(version, state)
}

#[tauri::command]
pub fn v2_ignore_release(
    version: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::ignore_release(version, state)
}

#[tauri::command]
pub fn v2_mark_whats_new_shown(
    version: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::mark_whats_new_shown(version, state)
}

#[tauri::command]
pub fn v2_has_whats_new_been_shown(
    version: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<bool, String> {
    crate::updates::has_whats_new_been_shown(version, state)
}

#[tauri::command]
pub fn v2_mark_flatpak_welcome_shown(
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::mark_flatpak_welcome_shown(state)
}

#[tauri::command]
pub fn v2_has_flatpak_welcome_been_shown(
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<bool, String> {
    crate::updates::has_flatpak_welcome_been_shown(state)
}

#[tauri::command]
pub fn v2_get_backend_logs() -> Vec<String> {
    crate::logging::get_backend_logs()
}

#[tauri::command]
pub async fn v2_upload_logs_to_paste(content: String) -> Result<String, String> {
    crate::logging::upload_logs_to_paste(content).await
}

#[tauri::command]
pub fn v2_set_show_downloads_in_library(
    show: bool,
    state: State<'_, crate::config::download_settings::DownloadSettingsState>,
) -> Result<(), String> {
    crate::config::download_settings::set_show_downloads_in_library(show, state)
}

#[tauri::command]
pub fn v2_get_download_settings(
    state: State<'_, crate::config::download_settings::DownloadSettingsState>,
) -> Result<crate::config::download_settings::DownloadSettings, String> {
    crate::config::download_settings::get_download_settings(state)
}

#[tauri::command]
pub async fn v2_lyrics_get_cache_stats(
    state: State<'_, crate::lyrics::LyricsState>,
) -> Result<V2LyricsCacheStats, String> {
    let entries = {
        let db_opt__ = state.db.lock().await;
        let db = db_opt__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.count_entries()?
    };

    // Approximate on-disk usage as the SQLite DB file size.
    let db_path = dirs::cache_dir()
        .ok_or("Could not determine cache directory")?
        .join("qbz")
        .join("lyrics")
        .join("lyrics.db");
    let size_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    Ok(V2LyricsCacheStats {
        entries,
        size_bytes,
    })
}

#[tauri::command]
pub fn v2_lastfm_has_embedded_credentials() -> bool {
    crate::lastfm::LastFmClient::has_embedded_credentials()
}

#[tauri::command]
pub async fn v2_remote_control_get_status(
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlStatus, String> {
    crate::api_server::remote_control_get_status(app_handle).await
}

#[tauri::command]
pub async fn v2_remote_control_set_enabled(
    enabled: bool,
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlStatus, String> {
    crate::api_server::remote_control_set_enabled(enabled, app_handle).await
}

#[tauri::command]
pub async fn v2_remote_control_set_port(
    port: u16,
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlStatus, String> {
    crate::api_server::remote_control_set_port(port, app_handle).await
}

#[tauri::command]
pub async fn v2_remote_control_set_secure(
    secure: bool,
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlStatus, String> {
    crate::api_server::remote_control_set_secure(secure, app_handle).await
}

#[tauri::command]
pub async fn v2_remote_control_regenerate_token(
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlQr, String> {
    crate::api_server::remote_control_regenerate_token(app_handle).await
}

#[tauri::command]
pub async fn v2_remote_control_get_pairing_qr(
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlQr, String> {
    crate::api_server::remote_control_get_pairing_qr(app_handle).await
}

#[tauri::command]
pub fn v2_is_running_in_flatpak() -> bool {
    crate::flatpak::is_running_in_flatpak()
}

#[tauri::command]
pub fn v2_get_flatpak_help_text() -> String {
    crate::flatpak::get_flatpak_guidance()
}

#[tauri::command]
pub fn v2_detect_desktop_theme() -> crate::desktop_theme::DesktopThemeInfo {
    crate::desktop_theme::detect_desktop_theme()
}

#[tauri::command]
pub fn v2_is_auto_update_eligible() -> bool {
    crate::updates::is_auto_update_eligible()
}

#[tauri::command]
pub fn v2_is_running_in_snap() -> bool {
    crate::snap::is_running_in_snap()
}

#[tauri::command]
pub fn v2_mark_snap_welcome_shown(
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::mark_snap_welcome_shown(state)
}

#[tauri::command]
pub fn v2_has_snap_welcome_been_shown(
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<bool, String> {
    crate::updates::has_snap_welcome_been_shown(state)
}

#[tauri::command]
pub async fn v2_detect_legacy_cached_files(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<crate::offline_cache::MigrationStatus, String> {
    let tracks_dir = cache_state.cache_dir.read().unwrap().join("tracks");
    let track_ids = crate::offline_cache::detect_legacy_cached_files(&tracks_dir)?;
    Ok(crate::offline_cache::MigrationStatus {
        has_legacy_files: !track_ids.is_empty(),
        total_tracks: track_ids.len(),
        ..Default::default()
    })
}

#[tauri::command]
pub fn v2_get_device_sample_rate_limit(
    state: State<'_, crate::config::audio_settings::AudioSettingsState>,
    device_id: String,
) -> Result<Option<u32>, String> {
    crate::config::audio_settings::get_device_sample_rate_limit(state, device_id)
}

#[tauri::command]
pub fn v2_set_device_sample_rate_limit(
    state: State<'_, crate::config::audio_settings::AudioSettingsState>,
    device_id: String,
    rate: Option<u32>,
) -> Result<(), String> {
    crate::config::audio_settings::set_device_sample_rate_limit(state, device_id, rate)
}

#[tauri::command]
pub fn v2_set_force_x11(
    state: State<'_, crate::config::graphics_settings::GraphicsSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    crate::config::graphics_settings::set_force_x11(state, enabled)
}

#[tauri::command]
pub fn v2_restart_app(app: tauri::AppHandle) {
    log::info!("[V2] App restart requested by user");
    app.restart();
}

// ── Purchases (Qobuz) ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct V2PurchaseFormatOption {
    pub id: u32,
    pub label: String,
    pub bit_depth: Option<u32>,
    pub sampling_rate: Option<f64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(non_snake_case)]
pub struct V2DynamicTrackToAnalyseInput {
    pub trackId: u64,
    pub artistId: u64,
    pub genreId: u64,
    pub labelId: u64,
}

fn v2_purchase_extension(format_id: u32, mime_type: &str) -> &'static str {
    if format_id == 5 || mime_type.contains("mpeg") {
        "mp3"
    } else {
        "flac"
    }
}

fn v2_purchase_target_path(
    destination: &str,
    artist_name: &str,
    album_title: &str,
    quality_dir: &str,
    track_number: u32,
    track_title: &str,
    ext: &str,
) -> PathBuf {
    let artist_dir = crate::offline_cache::metadata::sanitize_filename(artist_name);
    let album_clean = crate::offline_cache::metadata::sanitize_filename(album_title);
    let title_clean = crate::offline_cache::metadata::sanitize_filename(track_title);

    let file_name = if track_number > 0 {
        format!("{:02} - {}.{}", track_number, title_clean, ext)
    } else {
        format!("{}.{}", title_clean, ext)
    };

    // Embed quality in album folder name: "Album [FLAC][24-bit,96kHz]"
    let album_dir = if !quality_dir.is_empty() {
        let quality_clean = crate::offline_cache::metadata::sanitize_filename(quality_dir);
        format!("{} {}", album_clean, quality_clean)
    } else {
        album_clean
    };

    PathBuf::from(destination)
        .join(artist_dir)
        .join(album_dir)
        .join(file_name)
}

fn v2_apply_purchase_download_flags(
    response: &mut PurchaseResponse,
    downloaded_ids: &HashSet<i64>,
    format_map: &std::collections::HashMap<i64, Vec<u32>>,
) {
    for track in &mut response.tracks.items {
        let tid = track.id as i64;
        track.downloaded = downloaded_ids.contains(&tid);
        track.downloaded_format_ids = format_map.get(&tid).cloned().unwrap_or_default();
    }

    for album in &mut response.albums.items {
        let album_track_ids: Vec<i64> = response
            .tracks
            .items
            .iter()
            .filter(|track| {
                track
                    .album
                    .as_ref()
                    .map(|album_ref| album_ref.id == album.id)
                    .unwrap_or(false)
            })
            .map(|track| track.id as i64)
            .collect();

        album.downloaded = !album_track_ids.is_empty()
            && album_track_ids
                .iter()
                .all(|track_id| downloaded_ids.contains(track_id));
    }
}

fn v2_filter_purchase_response(mut response: PurchaseResponse, query: &str) -> PurchaseResponse {
    let q = query.to_lowercase();

    response.albums.items.retain(|album| {
        album.title.to_lowercase().contains(&q) || album.artist.name.to_lowercase().contains(&q)
    });
    response.albums.total = response.albums.items.len() as u32;
    response.albums.offset = 0;

    response.tracks.items.retain(|track| {
        track.title.to_lowercase().contains(&q)
            || track.performer.name.to_lowercase().contains(&q)
            || track
                .album
                .as_ref()
                .map(|album_ref| album_ref.title.to_lowercase().contains(&q))
                .unwrap_or(false)
    });
    response.tracks.total = response.tracks.items.len() as u32;
    response.tracks.offset = 0;

    response
}

async fn v2_download_purchase_track_impl(
    track_id: u64,
    format_id: u32,
    destination: &str,
    quality_dir: &str,
    app_state: &AppState,
) -> Result<String, String> {
    let client = app_state.client.read().await;
    let track = client
        .get_track(track_id)
        .await
        .map_err(|e| format!("Failed to fetch track {}: {}", track_id, e))?;
    let stream = client
        .get_track_file_url_by_format(track_id, format_id)
        .await
        .map_err(|e| format!("Failed to get download URL for track {}: {}", track_id, e))?;
    drop(client);

    let data = download_audio(&stream.url).await?;

    let artist_name = track
        .performer
        .as_ref()
        .map(|artist| artist.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album_title = track
        .album
        .as_ref()
        .map(|album| album.title.clone())
        .unwrap_or_else(|| "Singles".to_string());
    let extension = v2_purchase_extension(stream.format_id, &stream.mime_type);
    let target_path = v2_purchase_target_path(
        destination,
        &artist_name,
        &album_title,
        quality_dir,
        track.track_number,
        &track.title,
        extension,
    );

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create destination folder: {}", e))?;
    }

    let temp_path = target_path.with_extension(format!("{}.part", extension));
    fs::write(&temp_path, &data).map_err(|e| format!("Failed to write temporary file: {}", e))?;
    fs::rename(&temp_path, &target_path).map_err(|e| format!("Failed to finalize file: {}", e))?;

    Ok(target_path.to_string_lossy().to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_get_all(
    limit: Option<u32>,
    offset: Option<u32>,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<PurchaseResponse, String> {
    let client = app_state.client.read().await;
    let mut response = if let (Some(lim), Some(off)) = (limit, offset) {
        client
            .get_user_purchases_page(lim, off)
            .await
            .map_err(|e| format!("Failed to fetch purchases page: {}", e))?
    } else {
        client
            .get_user_purchases_all()
            .await
            .map_err(|e| format!("Failed to fetch purchases: {}", e))?
    };
    drop(client);

    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let downloaded_formats = db
        .get_downloaded_purchase_formats()
        .map_err(|e| e.to_string())?;
    let downloaded_ids: HashSet<i64> = downloaded_formats.iter().map(|(tid, _)| *tid).collect();
    let mut format_map: std::collections::HashMap<i64, Vec<u32>> = std::collections::HashMap::new();
    for (track_id, format_id) in &downloaded_formats {
        format_map
            .entry(*track_id)
            .or_default()
            .push(*format_id as u32);
    }

    v2_apply_purchase_download_flags(&mut response, &downloaded_ids, &format_map);
    Ok(response)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_get_ids(
    limit: Option<u32>,
    offset: Option<u32>,
    purchaseType: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<PurchaseIdsResponse, String> {
    let lim = limit.unwrap_or(500);
    let off = offset.unwrap_or(0);
    let type_ref = purchaseType.as_deref();

    let client = app_state.client.read().await;
    let response = client
        .get_user_purchases_ids_page_typed(type_ref, lim, off)
        .await
        .map_err(|e| format!("Failed to fetch purchase IDs page: {}", e))?;
    drop(client);

    Ok(response)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_get_by_type(
    purchaseType: String,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<PurchaseResponse, String> {
    if purchaseType != "albums" && purchaseType != "tracks" {
        return Err(format!(
            "Invalid purchase type '{}'. Expected 'albums' or 'tracks'.",
            purchaseType
        ));
    }

    let client = app_state.client.read().await;
    let mut response = client
        .get_user_purchases_all_typed(&purchaseType)
        .await
        .map_err(|e| format!("Failed to fetch {} purchases: {}", purchaseType, e))?;
    drop(client);

    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let downloaded_formats = db
        .get_downloaded_purchase_formats()
        .map_err(|e| e.to_string())?;
    let downloaded_ids: HashSet<i64> = downloaded_formats.iter().map(|(tid, _)| *tid).collect();
    let mut format_map: std::collections::HashMap<i64, Vec<u32>> = std::collections::HashMap::new();
    for (track_id, format_id) in &downloaded_formats {
        format_map
            .entry(*track_id)
            .or_default()
            .push(*format_id as u32);
    }

    if purchaseType == "tracks" {
        for track in &mut response.tracks.items {
            let tid = track.id as i64;
            track.downloaded = downloaded_ids.contains(&tid);
            track.downloaded_format_ids = format_map.get(&tid).cloned().unwrap_or_default();
        }
    } else {
        for album in &mut response.albums.items {
            let album_track_ids: Vec<i64> = album
                .tracks
                .as_ref()
                .map(|tracks_page| {
                    tracks_page
                        .items
                        .iter()
                        .map(|track| track.id as i64)
                        .collect::<Vec<i64>>()
                })
                .unwrap_or_default();

            album.downloaded = !album_track_ids.is_empty()
                && album_track_ids
                    .iter()
                    .all(|track_id| downloaded_ids.contains(track_id));
        }
    }

    Ok(response)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_search(
    query: String,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<PurchaseResponse, String> {
    let mut response = v2_purchases_get_all(None, None, app_state, library_state).await?;
    if query.trim().is_empty() {
        return Ok(response);
    }
    response = v2_filter_purchase_response(response, query.trim());
    Ok(response)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_get_album(
    albumId: String,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<PurchaseAlbum, String> {
    let client = app_state.client.read().await;
    let album = client
        .get_album(&albumId)
        .await
        .map_err(|e| format!("Failed to fetch album {}: {}", albumId, e))?;
    let purchases = client
        .get_user_purchases_all()
        .await
        .map_err(|e| format!("Failed to fetch purchases: {}", e))?;
    drop(client);

    let purchase_meta = purchases
        .albums
        .items
        .iter()
        .find(|item| item.id == albumId);

    let tracks_items: Vec<PurchaseTrack> = album
        .tracks
        .as_ref()
        .map(|tracks| {
            tracks
                .items
                .iter()
                .map(|track| PurchaseTrack {
                    id: track.id,
                    title: track.title.clone(),
                    track_number: track.track_number,
                    media_number: track.media_number,
                    duration: track.duration,
                    performer: track.performer.clone().unwrap_or_default(),
                    album: track.album.clone(),
                    hires: track.hires,
                    maximum_sampling_rate: track.maximum_sampling_rate,
                    maximum_bit_depth: track.maximum_bit_depth,
                    streamable: track.streamable,
                    downloaded: false,
                    downloaded_format_ids: Vec::new(),
                    purchased_at: purchase_meta.and_then(|item| item.purchased_at),
                })
                .collect()
        })
        .unwrap_or_default();

    let mut result = PurchaseAlbum {
        id: album.id.clone(),
        title: album.title.clone(),
        artist: album.artist.clone(),
        image: album.image.clone(),
        release_date_original: album.release_date_original.clone(),
        label: album.label.clone(),
        genre: album.genre.clone(),
        tracks_count: album.tracks_count,
        duration: album.duration,
        hires: album.hires,
        maximum_sampling_rate: album.maximum_sampling_rate,
        maximum_bit_depth: album.maximum_bit_depth,
        downloadable: purchase_meta.map(|item| item.downloadable).unwrap_or(true),
        downloaded: false,
        purchased_at: purchase_meta.and_then(|item| item.purchased_at),
        tracks: Some(ApiSearchResultsPage {
            offset: 0,
            limit: tracks_items.len() as u32,
            total: tracks_items.len() as u32,
            items: tracks_items,
        }),
    };

    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let downloaded_formats = db
        .get_downloaded_purchase_formats()
        .map_err(|e| e.to_string())?;

    // Build per-track format lookup: track_id -> Vec<format_id>
    let mut format_map: std::collections::HashMap<i64, Vec<u32>> = std::collections::HashMap::new();
    for (track_id, format_id) in &downloaded_formats {
        format_map
            .entry(*track_id)
            .or_default()
            .push(*format_id as u32);
    }
    let downloaded_ids: HashSet<i64> = downloaded_formats.iter().map(|(tid, _)| *tid).collect();

    if let Some(tracks) = &mut result.tracks {
        for track in &mut tracks.items {
            let tid = track.id as i64;
            track.downloaded = downloaded_ids.contains(&tid);
            track.downloaded_format_ids = format_map.get(&tid).cloned().unwrap_or_default();
        }
        result.downloaded = !tracks.items.is_empty()
            && tracks
                .items
                .iter()
                .all(|track| downloaded_ids.contains(&(track.id as i64)));
    }

    Ok(result)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_get_formats(
    albumId: String,
    app_state: State<'_, AppState>,
) -> Result<Vec<V2PurchaseFormatOption>, String> {
    let client = app_state.client.read().await;
    let album = client
        .get_album(&albumId)
        .await
        .map_err(|e| format!("Failed to fetch album {}: {}", albumId, e))?;
    drop(client);

    let mut formats = Vec::new();

    if album.hires && album.maximum_sampling_rate.unwrap_or(0.0) > 96.0 {
        formats.push(V2PurchaseFormatOption {
            id: 27,
            label: "[FLAC][24-bit,192kHz]".to_string(),
            bit_depth: Some(24),
            sampling_rate: Some(192.0),
        });
    }

    if album.hires {
        formats.push(V2PurchaseFormatOption {
            id: 7,
            label: "[FLAC][24-bit,96kHz]".to_string(),
            bit_depth: Some(24),
            sampling_rate: Some(96.0),
        });
    }

    formats.push(V2PurchaseFormatOption {
        id: 6,
        label: "[FLAC][16-bit,44.1kHz]".to_string(),
        bit_depth: Some(16),
        sampling_rate: Some(44.1),
    });

    formats.push(V2PurchaseFormatOption {
        id: 5,
        label: "[MP3][320kbps]".to_string(),
        bit_depth: None,
        sampling_rate: None,
    });

    Ok(formats)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_download_track(
    trackId: u64,
    formatId: u32,
    destination: String,
    qualityDir: String,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<String, String> {
    let file_path =
        v2_download_purchase_track_impl(trackId, formatId, &destination, &qualityDir, &app_state)
            .await?;

    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.mark_purchase_downloaded(trackId as i64, None, &file_path, formatId as i64)
        .map_err(|e| e.to_string())?;

    Ok(file_path)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_download_album(
    albumId: String,
    formatId: u32,
    destination: String,
    qualityDir: String,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    let client = app_state.client.read().await;
    let album = client
        .get_album(&albumId)
        .await
        .map_err(|e| format!("Failed to fetch album {}: {}", albumId, e))?;
    drop(client);

    let tracks = album
        .tracks
        .as_ref()
        .map(|list| list.items.clone())
        .unwrap_or_default();

    let mut failures: Vec<String> = Vec::new();
    for track in tracks {
        match v2_download_purchase_track_impl(
            track.id,
            formatId,
            &destination,
            &qualityDir,
            &app_state,
        )
        .await
        {
            Ok(file_path) => {
                let guard = library_state.db.lock().await;
                let db = guard.as_ref().ok_or("No active session - please log in")?;
                if let Err(err) = db.mark_purchase_downloaded(
                    track.id as i64,
                    Some(albumId.as_str()),
                    &file_path,
                    formatId as i64,
                ) {
                    failures.push(format!("track {} registry error: {}", track.id, err));
                }
            }
            Err(err) => failures.push(format!("track {}: {}", track.id, err)),
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Album download completed with errors: {}",
            failures.join(" | ")
        ))
    }
}

// ── Downloaded Purchases Registry ──

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_mark_downloaded(
    trackId: i64,
    albumId: Option<String>,
    filePath: String,
    formatId: i64,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.mark_purchase_downloaded(trackId, albumId.as_deref(), &filePath, formatId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_remove_downloaded(
    trackId: i64,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.remove_downloaded_purchase(trackId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_purchases_get_downloaded_track_ids(
    library_state: State<'_, LibraryState>,
) -> Result<Vec<i64>, String> {
    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_downloaded_purchase_track_ids()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_dynamic_suggest(
    limit: Option<u32>,
    listenedTrackIds: Option<Vec<u64>>,
    tracksToAnalyse: Option<Vec<V2DynamicTrackToAnalyseInput>>,
    app_state: State<'_, AppState>,
) -> Result<DynamicSuggestResponse, String> {
    let request = DynamicSuggestRequest {
        limit: limit.unwrap_or(50).clamp(1, 200),
        listened_tracks_ids: listenedTrackIds.unwrap_or_default(),
        track_to_analysed: tracksToAnalyse
            .unwrap_or_default()
            .into_iter()
            .map(|item| DynamicTrackToAnalyse {
                track_id: item.trackId,
                artist_id: item.artistId,
                genre_id: item.genreId,
                label_id: item.labelId,
            })
            .collect(),
    };

    let client = app_state.client.read().await;
    client
        .get_dynamic_suggest(&request)
        .await
        .map_err(|e| format!("Failed to fetch dynamic suggestions: {}", e))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_dynamic_suggest_raw(
    limit: Option<u32>,
    listenedTrackIds: Option<Vec<u64>>,
    tracksToAnalyse: Option<Vec<V2DynamicTrackToAnalyseInput>>,
    app_state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let request = DynamicSuggestRequest {
        limit: limit.unwrap_or(50).clamp(1, 200),
        listened_tracks_ids: listenedTrackIds.unwrap_or_default(),
        track_to_analysed: tracksToAnalyse
            .unwrap_or_default()
            .into_iter()
            .map(|item| DynamicTrackToAnalyse {
                track_id: item.trackId,
                artist_id: item.artistId,
                genre_id: item.genreId,
                label_id: item.labelId,
            })
            .collect(),
    };

    let client = app_state.client.read().await;
    client
        .get_dynamic_suggest_raw(&request)
        .await
        .map_err(|e| format!("Failed to fetch raw dynamic suggestions: {}", e))
}

// ── Auto-Theme commands ─────────────────────────────────────────────────────
// Image processing (decode + k-means) is CPU-bound, so heavy commands use
// spawn_blocking to avoid freezing the main thread / UI spinner.

#[tauri::command]
pub fn v2_detect_desktop_environment() -> crate::auto_theme::system::DesktopEnvironment {
    crate::auto_theme::system::detect_desktop_environment()
}

#[tauri::command]
pub fn v2_get_system_wallpaper() -> Result<String, String> {
    crate::auto_theme::system::get_system_wallpaper()
}

#[tauri::command]
pub fn v2_get_system_accent_color() -> Result<crate::auto_theme::PaletteColor, String> {
    crate::auto_theme::system::get_system_accent_color()
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_generate_theme_from_image(
    imagePath: String,
) -> Result<crate::auto_theme::GeneratedTheme, String> {
    tokio::task::spawn_blocking(move || {
        let palette = crate::auto_theme::palette::extract_palette(&imagePath)?;
        Ok(crate::auto_theme::generator::generate_theme(
            &palette, &imagePath,
        ))
    })
    .await
    .map_err(|e| format!("Theme generation task failed: {}", e))?
}

#[tauri::command]
pub async fn v2_generate_theme_from_wallpaper() -> Result<crate::auto_theme::GeneratedTheme, String>
{
    tokio::task::spawn_blocking(|| {
        let wallpaper = crate::auto_theme::system::get_system_wallpaper()?;
        let palette = crate::auto_theme::palette::extract_palette(&wallpaper)?;
        Ok(crate::auto_theme::generator::generate_theme(
            &palette, &wallpaper,
        ))
    })
    .await
    .map_err(|e| format!("Theme generation task failed: {}", e))?
}

#[tauri::command]
pub fn v2_generate_theme_from_system_colors() -> Result<crate::auto_theme::GeneratedTheme, String> {
    let scheme = crate::auto_theme::system::get_system_color_scheme()?;
    Ok(crate::auto_theme::generator::generate_theme_from_scheme(
        &scheme,
    ))
}

#[tauri::command]
pub fn v2_get_system_color_scheme() -> Result<crate::auto_theme::SystemColorScheme, String> {
    crate::auto_theme::system::get_system_color_scheme()
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_extract_palette(
    imagePath: String,
) -> Result<crate::auto_theme::ThemePalette, String> {
    tokio::task::spawn_blocking(move || crate::auto_theme::palette::extract_palette(&imagePath))
        .await
        .map_err(|e| format!("Palette extraction task failed: {}", e))?
}

