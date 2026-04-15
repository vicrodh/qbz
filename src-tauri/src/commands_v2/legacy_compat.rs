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
) -> Result<crate::library::commands::BackfillReport, RuntimeError> {
    crate::library::commands::library_backfill_downloads(state, offline_cache_state)
        .await
        .map_err(RuntimeError::Internal)
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
    crate::lyrics::commands::lyrics_get(trackId, title, artist, album, durationSecs, state)
        .await
        .map_err(RuntimeError::Internal)
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
    crate::offline::commands::create_pending_playlist(
        name,
        description,
        isPublic,
        trackIds,
        localTrackPaths,
        state,
    )
    .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub fn v2_get_pending_playlist_count(state: State<'_, OfflineState>) -> Result<u32, RuntimeError> {
    crate::offline::commands::get_pending_playlist_count(state).map_err(RuntimeError::Internal)
}

#[tauri::command]
pub fn v2_queue_scrobble(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: i64,
    state: State<'_, OfflineState>,
) -> Result<i64, RuntimeError> {
    crate::offline::commands::queue_scrobble(artist, track, album, timestamp, state)
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_get_queued_scrobbles(
    limit: Option<u32>,
    state: State<'_, OfflineState>,
) -> Result<Vec<crate::offline::QueuedScrobble>, RuntimeError> {
    crate::offline::commands::get_queued_scrobbles(limit, state).map_err(RuntimeError::Internal)
}

#[tauri::command]
pub fn v2_get_queued_scrobble_count(state: State<'_, OfflineState>) -> Result<u32, RuntimeError> {
    crate::offline::commands::get_queued_scrobble_count(state).map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_cleanup_sent_scrobbles(
    olderThanDays: Option<u32>,
    state: State<'_, OfflineState>,
) -> Result<u32, RuntimeError> {
    crate::offline::commands::cleanup_sent_scrobbles(olderThanDays, state)
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_track_by_path(
    filePath: String,
    state: State<'_, LibraryState>,
) -> Result<Option<LocalTrack>, RuntimeError> {
    crate::library::commands::get_track_by_path(filePath, state)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_check_network_path(
    path: String,
) -> Result<crate::network::NetworkPathInfo, RuntimeError> {
    Ok(crate::network::commands::check_network_path(path))
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
    crate::library::commands::library_update_folder_settings(
        id,
        alias,
        enabled,
        isNetwork,
        networkFsType,
        userOverrideNetwork,
        state,
    )
    .await
    .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_discogs_has_credentials() -> Result<bool, RuntimeError> {
    crate::library::commands::discogs_has_credentials()
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_discogs_search_artwork(
    artist: String,
    album: String,
    catalogNumber: Option<String>,
) -> Result<Vec<crate::discogs::DiscogsImageOption>, RuntimeError> {
    crate::library::commands::discogs_search_artwork(artist, album, catalogNumber)
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
    crate::library::commands::discogs_download_artwork(imageUrl, artist, album)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_check_album_fully_cached(
    albumId: String,
    cache_state: State<'_, crate::offline_cache::OfflineCacheState>,
) -> Result<bool, RuntimeError> {
    crate::offline_cache::commands::check_album_fully_cached(albumId, cache_state)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_check_albums_fully_cached_batch(
    albumIds: Vec<String>,
    cache_state: State<'_, crate::offline_cache::OfflineCacheState>,
) -> Result<std::collections::HashMap<String, bool>, RuntimeError> {
    crate::offline_cache::commands::check_albums_fully_cached_batch(albumIds, cache_state)
        .await
        .map_err(RuntimeError::Internal)
}

/// Shared helper: spawn the download task for a single track.
/// Used by both v2_cache_track_for_offline (single) and v2_cache_tracks_batch_for_offline (batch).
fn spawn_track_cache_download(
    track_id: u64,
    file_path: std::path::PathBuf,
    client: std::sync::Arc<tokio::sync::RwLock<crate::api::QobuzClient>>,
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

        // TODO: Add CMAF fallback when CoreBridge is accessible here
        // (currently uses legacy QobuzClient; spawn_track_cache_download would need
        // an Arc<RwLock<Option<CoreBridge>>> parameter to call try_cmaf_full_download)
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

                if let Some(artwork_url) = &metadata.artwork_url {
                    if let Some(parent_dir) = std::path::Path::new(&new_path).parent() {
                        let _ = crate::offline_cache::metadata::save_album_artwork(
                            parent_dir,
                            artwork_url,
                        )
                        .await;
                    }
                }

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
                        None,
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

#[tauri::command]
pub async fn v2_library_scan(state: State<'_, crate::library::LibraryState>) -> Result<(), String> {
    crate::library::library_scan_impl(state).await
}

#[tauri::command]
pub async fn v2_library_stop_scan(
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_stop_scan_impl(state).await
}

#[tauri::command]
pub async fn v2_library_scan_folder(
    folder_id: i64,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_scan_folder_impl(folder_id, state).await
}

#[tauri::command]
pub async fn v2_library_clear(
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_clear_impl(state).await
}

#[tauri::command]
pub async fn v2_library_update_album_metadata(
    request: crate::library::LibraryAlbumMetadataUpdateRequest,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_update_album_metadata_impl(request, state).await
}

#[tauri::command]
pub async fn v2_library_write_album_metadata_to_files(
    app: tauri::AppHandle,
    request: crate::library::LibraryAlbumMetadataUpdateRequest,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_write_album_metadata_to_files_impl(app, request, state).await
}

#[tauri::command]
pub async fn v2_library_refresh_album_metadata_from_files(
    album_group_key: String,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_refresh_album_metadata_from_files_impl(album_group_key, state).await
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
) -> Result<crate::lyrics::commands::LyricsCacheStats, String> {
    crate::lyrics::commands::lyrics_get_cache_stats(state).await
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

