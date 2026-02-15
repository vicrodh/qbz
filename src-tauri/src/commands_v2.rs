//! V2 Commands - Using the new multi-crate architecture
//!
//! These commands use QbzCore via CoreBridge instead of the old AppState.
//! They coexist with the old commands during migration.
//!
//! Playback flows through CoreBridge -> QbzCore -> Player (qbz-player crate).

use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

use qbz_models::{Album, Artist, Quality, QueueState, RepeatMode, SearchResultsPage, Track, UserSession};

use crate::artist_blacklist::BlacklistState;
use crate::cache::AudioCache;
use crate::config::audio_settings::AudioSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::offline_cache::OfflineCacheState;
use crate::queue::QueueManager;
use crate::AppState;

// ==================== Helper Functions ====================

/// Convert quality string from frontend to Quality enum
fn parse_quality(quality_str: Option<&str>) -> Quality {
    match quality_str {
        Some("MP3") => Quality::Mp3,
        Some("CD Quality") => Quality::Lossless,
        Some("Hi-Res") => Quality::HiRes,
        Some("Hi-Res+") => Quality::UltraHiRes,
        _ => Quality::UltraHiRes, // Default to highest
    }
}

/// Limit quality based on device's max sample rate
fn limit_quality_for_device(quality: Quality, max_sample_rate: Option<u32>) -> Quality {
    let Some(max_rate) = max_sample_rate else {
        return quality;
    };

    if max_rate <= 48000 {
        match quality {
            Quality::UltraHiRes | Quality::HiRes => {
                log::info!(
                    "[V2/Quality Limit] Device max {}Hz, limiting {} to Lossless (44.1kHz)",
                    max_rate, quality.label()
                );
                Quality::Lossless
            }
            _ => quality,
        }
    } else if max_rate <= 96000 {
        match quality {
            Quality::UltraHiRes => {
                log::info!(
                    "[V2/Quality Limit] Device max {}Hz, limiting Hi-Res+ to Hi-Res (96kHz)",
                    max_rate
                );
                Quality::HiRes
            }
            _ => quality,
        }
    } else {
        quality
    }
}

/// Download audio from URL
async fn download_audio(url: &str) -> Result<Vec<u8>, String> {
    use std::time::Duration;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    log::info!("[V2] Downloading audio...");

    let response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch audio: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read audio bytes: {}", e))?;

    log::info!("[V2] Downloaded {} bytes", bytes.len());
    Ok(bytes.to_vec())
}

// ==================== Prefetch (V2) ====================

/// Number of Qobuz tracks to prefetch (not total tracks, just Qobuz)
const V2_PREFETCH_COUNT: usize = 2;

/// How far ahead to look for tracks to prefetch (to handle mixed playlists)
const V2_PREFETCH_LOOKAHEAD: usize = 10;

/// Maximum concurrent prefetch downloads
const V2_MAX_CONCURRENT_PREFETCH: usize = 1;

lazy_static::lazy_static! {
    /// Semaphore to limit concurrent prefetch operations
    static ref V2_PREFETCH_SEMAPHORE: tokio::sync::Semaphore =
        tokio::sync::Semaphore::new(V2_MAX_CONCURRENT_PREFETCH);
}

/// Spawn background tasks to prefetch upcoming Qobuz tracks (V2)
fn spawn_v2_prefetch(
    bridge: Arc<RwLock<Option<crate::core_bridge::CoreBridge>>>,
    cache: Arc<AudioCache>,
    queue: &QueueManager,
    quality: Quality,
    streaming_only: bool,
) {
    // Skip prefetch entirely in streaming_only mode
    if streaming_only {
        log::debug!("[V2/PREFETCH] Skipped - streaming_only mode active");
        return;
    }

    // Look further ahead to find Qobuz tracks in mixed playlists
    let upcoming_tracks = queue.peek_upcoming(V2_PREFETCH_LOOKAHEAD);

    if upcoming_tracks.is_empty() {
        log::debug!("[V2/PREFETCH] No upcoming tracks to prefetch");
        return;
    }

    let mut qobuz_prefetched = 0;

    for track in upcoming_tracks {
        // Stop once we've prefetched enough Qobuz tracks
        if qobuz_prefetched >= V2_PREFETCH_COUNT {
            break;
        }

        let track_id = track.id;
        let track_title = track.title.clone();

        // Skip local tracks - they don't need prefetching from Qobuz
        if track.is_local {
            log::debug!("[V2/PREFETCH] Skipping local track: {} - {}", track_id, track_title);
            continue;
        }

        // Check if already cached or being fetched
        if cache.contains(track_id) {
            log::debug!("[V2/PREFETCH] Track {} already cached", track_id);
            qobuz_prefetched += 1;
            continue;
        }

        if cache.is_fetching(track_id) {
            log::debug!("[V2/PREFETCH] Track {} already being fetched", track_id);
            qobuz_prefetched += 1;
            continue;
        }

        // Mark as fetching
        cache.mark_fetching(track_id);
        qobuz_prefetched += 1;

        let bridge_clone = bridge.clone();
        let cache_clone = cache.clone();

        log::info!("[V2/PREFETCH] Prefetching track: {} - {}", track_id, track_title);

        // Spawn background task for each track (with semaphore to limit concurrency)
        tokio::spawn(async move {
            // Acquire semaphore permit to limit concurrent prefetches
            let _permit = match V2_PREFETCH_SEMAPHORE.acquire().await {
                Ok(permit) => permit,
                Err(_) => {
                    log::warn!("[V2/PREFETCH] Semaphore closed, skipping track {}", track_id);
                    cache_clone.unmark_fetching(track_id);
                    return;
                }
            };

            let result = async {
                let bridge_guard = bridge_clone.read().await;
                let bridge = bridge_guard.as_ref().ok_or("CoreBridge not initialized")?;
                let stream_url = bridge.get_stream_url(track_id, quality).await?;
                drop(bridge_guard);

                let data = download_audio(&stream_url.url).await?;
                Ok::<Vec<u8>, String>(data)
            }
            .await;

            match result {
                Ok(data) => {
                    // Small delay before cache insertion to avoid potential race with audio thread
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    cache_clone.insert(track_id, data);
                    log::info!("[V2/PREFETCH] Complete for track {}", track_id);
                }
                Err(e) => {
                    log::warn!("[V2/PREFETCH] Failed for track {}: {}", track_id, e);
                }
            }

            cache_clone.unmark_fetching(track_id);
        });
    }
}

// ==================== Auth Commands (V2) ====================

/// Check if user is logged in (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_is_logged_in(
    bridge: State<'_, CoreBridgeState>,
) -> Result<bool, String> {
    let bridge = bridge.get().await;
    Ok(bridge.is_logged_in().await)
}

/// Login with email and password (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_login(
    email: String,
    password: String,
    bridge: State<'_, CoreBridgeState>,
) -> Result<UserSession, String> {
    let bridge = bridge.get().await;
    bridge.login(&email, &password).await
}

/// Logout current user (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_logout(
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.logout().await
}

// ==================== Queue Commands (V2) ====================

/// Get current queue state (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_queue_state(
    bridge: State<'_, CoreBridgeState>,
) -> Result<QueueState, String> {
    let bridge = bridge.get().await;
    Ok(bridge.get_queue_state().await)
}

/// Set repeat mode (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_set_repeat_mode(
    mode: RepeatMode,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.set_repeat_mode(mode).await;
    Ok(())
}

/// Toggle shuffle (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_toggle_shuffle(
    bridge: State<'_, CoreBridgeState>,
) -> Result<bool, String> {
    let bridge = bridge.get().await;
    Ok(bridge.toggle_shuffle().await)
}

/// Clear the queue (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_clear_queue(
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.clear_queue().await;
    Ok(())
}

// ==================== Search Commands (V2) ====================

/// Search for albums (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_search_albums(
    query: String,
    limit: u32,
    offset: u32,
    searchType: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchResultsPage<Album>, String> {
    let bridge = bridge.get().await;
    let mut results = bridge.search_albums(&query, limit, offset, searchType.as_deref()).await?;

    // Filter out albums from blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|album| !blacklist_state.is_blacklisted(album.artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} albums from search results", filtered_count);
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

/// Search for tracks (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_search_tracks(
    query: String,
    limit: u32,
    offset: u32,
    searchType: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchResultsPage<Track>, String> {
    let bridge = bridge.get().await;
    let mut results = bridge.search_tracks(&query, limit, offset, searchType.as_deref()).await?;

    // Filter out tracks from blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|track| {
        if let Some(ref performer) = track.performer {
            !blacklist_state.is_blacklisted(performer.id)
        } else {
            true // Keep tracks without performer info
        }
    });

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} tracks from search results", filtered_count);
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

/// Search for artists (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_search_artists(
    query: String,
    limit: u32,
    offset: u32,
    searchType: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchResultsPage<Artist>, String> {
    let bridge = bridge.get().await;
    let mut results = bridge.search_artists(&query, limit, offset, searchType.as_deref()).await?;

    // Filter out blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|artist| !blacklist_state.is_blacklisted(artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} artists from search results", filtered_count);
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

// ==================== Catalog Commands (V2) ====================

/// Get album by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_album(
    albumId: String,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Album, String> {
    let bridge = bridge.get().await;
    bridge.get_album(&albumId).await
}

/// Get track by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_track(
    trackId: u64,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Track, String> {
    let bridge = bridge.get().await;
    bridge.get_track(trackId).await
}

/// Get artist by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_artist(
    artistId: u64,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Artist, String> {
    let bridge = bridge.get().await;
    bridge.get_artist(artistId).await
}

// ==================== Playback Commands (V2) ====================
//
// These commands use CoreBridge.player (qbz-player crate) for playback.
// This is the V2 architecture - playback flows through QbzCore.

/// Pause playback (V2)
#[tauri::command]
pub async fn v2_pause_playback(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] Command: pause_playback");
    app_state.media_controls.set_playback(false);
    let bridge = bridge.get().await;
    bridge.pause()
}

/// Resume playback (V2)
#[tauri::command]
pub async fn v2_resume_playback(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] Command: resume_playback");
    app_state.media_controls.set_playback(true);
    let bridge = bridge.get().await;
    bridge.resume()
}

/// Stop playback (V2)
#[tauri::command]
pub async fn v2_stop_playback(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] Command: stop_playback");
    app_state.media_controls.set_stopped();
    let bridge = bridge.get().await;
    bridge.stop()
}

/// Seek to position in seconds (V2)
#[tauri::command]
pub async fn v2_seek(
    position: u64,
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] Command: seek {}", position);
    let bridge_guard = bridge.get().await;
    let result = bridge_guard.seek(position);

    // Update MPRIS with new position
    let playback_state = bridge_guard.get_playback_state();
    app_state.media_controls.set_playback_with_progress(
        playback_state.is_playing,
        position,
    );

    result
}

/// Set volume (0.0 - 1.0) (V2)
#[tauri::command]
pub async fn v2_set_volume(
    volume: f32,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.set_volume(volume)
}

/// Get current playback state (V2) - also updates MPRIS progress
#[tauri::command]
pub async fn v2_get_playback_state(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
) -> Result<qbz_player::PlaybackState, String> {
    let bridge = bridge.get().await;
    let playback_state = bridge.get_playback_state();

    // Update MPRIS with current progress (called every ~500ms from frontend)
    app_state.media_controls.set_playback_with_progress(
        playback_state.is_playing,
        playback_state.position,
    );

    Ok(playback_state)
}

/// Set media controls metadata (V2 - for MPRIS integration)
#[tauri::command]
pub async fn v2_set_media_metadata(
    title: String,
    artist: String,
    album: String,
    duration_secs: Option<u64>,
    cover_url: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] Command: set_media_metadata - {} by {}", title, artist);
    crate::update_media_controls_metadata(
        &app_state.media_controls,
        &title,
        &artist,
        &album,
        duration_secs,
        cover_url,
    );
    app_state.media_controls.set_playback_with_progress(true, 0);
    Ok(())
}

/// Queue next track for gapless playback (V2 - cache-only, no download)
/// Returns true if gapless was queued, false if track not cached or ineligible
#[tauri::command]
pub async fn v2_play_next_gapless(
    track_id: u64,
    bridge: State<'_, CoreBridgeState>,
    offline_cache: State<'_, OfflineCacheState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    log::info!("[V2] Command: play_next_gapless for track {}", track_id);

    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();

    // Check offline cache (persistent disk cache)
    {
        let cached_path = {
            let db_opt = offline_cache.db.lock().await;
            if let Some(db) = db_opt.as_ref() {
                if let Ok(Some(file_path)) = db.get_file_path(track_id) {
                    Some(file_path)
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(file_path) = cached_path {
            let path = std::path::Path::new(&file_path);
            if path.exists() {
                log::info!("[V2/GAPLESS] Track {} from OFFLINE cache", track_id);
                let audio_data = std::fs::read(path)
                    .map_err(|e| format!("Failed to read cached file: {}", e))?;
                player.play_next(audio_data, track_id)?;
                return Ok(true);
            }
        }
    }

    // Check memory cache (L1)
    let cache = app_state.audio_cache.clone();
    if let Some(cached) = cache.get(track_id) {
        log::info!("[V2/GAPLESS] Track {} from MEMORY cache ({} bytes)", track_id, cached.size_bytes);
        player.play_next(cached.data, track_id)?;
        return Ok(true);
    }

    // Check playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!("[V2/GAPLESS] Track {} from DISK cache ({} bytes)", track_id, audio_data.len());
            player.play_next(audio_data, track_id)?;
            return Ok(true);
        }
    }

    log::info!("[V2/GAPLESS] Track {} not in any cache, gapless not possible", track_id);
    Ok(false)
}

/// Prefetch a track into the in-memory cache without starting playback (V2)
#[tauri::command]
pub async fn v2_prefetch_track(
    track_id: u64,
    quality: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    offline_cache: State<'_, OfflineCacheState>,
    audio_settings: State<'_, AudioSettingsState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let preferred_quality = parse_quality(quality.as_deref());

    // Apply per-device sample rate limit if enabled
    let final_quality = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        if let Some(store) = guard.as_ref() {
            if let Ok(settings) = store.get_settings() {
                if settings.limit_quality_to_device {
                    let device_id = settings.output_device.as_deref().unwrap_or("default");
                    let max_rate = settings
                        .device_sample_rate_limits
                        .get(device_id)
                        .copied()
                        .or(settings.device_max_sample_rate);
                    limit_quality_for_device(preferred_quality, max_rate)
                } else {
                    preferred_quality
                }
            } else {
                preferred_quality
            }
        } else {
            preferred_quality
        }
    };

    log::info!(
        "[V2] Command: prefetch_track {} (quality_str={:?}, parsed={:?}, final={:?})",
        track_id, quality, preferred_quality, final_quality
    );

    let cache = app_state.audio_cache.clone();

    if cache.contains(track_id) {
        log::info!("[V2] Track {} already in memory cache", track_id);
        return Ok(());
    }

    if cache.is_fetching(track_id) {
        log::info!("[V2] Track {} already being fetched", track_id);
        return Ok(());
    }

    cache.mark_fetching(track_id);
    let result = async {
        // Check persistent offline cache first
        {
            let cached_path = {
                let db_opt = offline_cache.db.lock().await;
                if let Some(db) = db_opt.as_ref() {
                    db.get_file_path(track_id).ok().flatten()
                } else {
                    None
                }
            };
            if let Some(file_path) = cached_path {
                let path = std::path::Path::new(&file_path);
                if path.exists() {
                    log::info!("[V2] Prefetching track {} from offline cache", track_id);
                    let audio_data = std::fs::read(path)
                        .map_err(|e| format!("Failed to read cached file: {}", e))?;
                    cache.insert(track_id, audio_data);
                    return Ok(());
                }
            }
        }

        let bridge_guard = bridge.get().await;
        let stream_url = bridge_guard.get_stream_url(track_id, final_quality).await?;
        drop(bridge_guard);

        let audio_data = download_audio(&stream_url.url).await?;
        cache.insert(track_id, audio_data);
        Ok(())
    }
    .await;

    cache.unmark_fetching(track_id);
    result
}

/// Result from play_track command with format info
#[derive(serde::Serialize)]
pub struct V2PlayTrackResult {
    /// The actual format_id returned by Qobuz (5=MP3, 6=FLAC 16-bit, 7=24-bit, 27=Hi-Res)
    /// None when playing from cache (format unknown)
    pub format_id: Option<u32>,
}

/// Play a track by ID (V2 - uses CoreBridge for API and playback)
///
/// This is the core playback command that:
/// 1. Checks caches (offline, memory, disk)
/// 2. Gets stream URL from Qobuz via CoreBridge
/// 3. Downloads audio
/// 4. Plays via CoreBridge.player() (qbz-player crate)
/// 5. Caches for future playback
#[tauri::command]
pub async fn v2_play_track(
    track_id: u64,
    quality: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    offline_cache: State<'_, OfflineCacheState>,
    audio_settings: State<'_, AudioSettingsState>,
    app_state: State<'_, AppState>,
) -> Result<V2PlayTrackResult, String> {
    let preferred_quality = parse_quality(quality.as_deref());

    // Apply per-device sample rate limit if enabled
    let final_quality = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        if let Some(store) = guard.as_ref() {
            if let Ok(settings) = store.get_settings() {
                if settings.limit_quality_to_device {
                    let device_id = settings.output_device.as_deref().unwrap_or("default");
                    let max_rate = settings
                        .device_sample_rate_limits
                        .get(device_id)
                        .copied()
                        .or(settings.device_max_sample_rate);
                    limit_quality_for_device(preferred_quality, max_rate)
                } else {
                    preferred_quality
                }
            } else {
                preferred_quality
            }
        } else {
            preferred_quality
        }
    };

    // Check streaming_only setting
    let streaming_only = {
        let guard = audio_settings.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        guard.as_ref().and_then(|s| s.get_settings().ok()).map(|s| s.streaming_only).unwrap_or(false)
    };

    log::info!(
        "[V2] play_track {} (quality_str={:?}, parsed={:?}, final={:?}, format_id={})",
        track_id, quality, preferred_quality, final_quality, final_quality.id()
    );

    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();

    // Check offline cache (persistent disk cache)
    {
        let cached_path = {
            let db_opt = offline_cache.db.lock().await;
            if let Some(db) = db_opt.as_ref() {
                if let Ok(Some(file_path)) = db.get_file_path(track_id) {
                    let _ = db.touch(track_id);
                    Some(file_path)
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(file_path) = cached_path {
            let path = std::path::Path::new(&file_path);
            if path.exists() {
                log::info!("[V2/CACHE HIT] Track {} from OFFLINE cache: {:?}", track_id, path);
                let audio_data = std::fs::read(path)
                    .map_err(|e| format!("Failed to read cached file: {}", e))?;
                player.play_data(audio_data, track_id)?;

                // Prefetch next tracks in background
                drop(bridge_guard);
                spawn_v2_prefetch(bridge.0.clone(), app_state.audio_cache.clone(), &app_state.queue, final_quality, streaming_only);
                return Ok(V2PlayTrackResult { format_id: None });
            }
        }
    }

    // Check memory cache (L1) - using AppState's audio_cache for now
    // TODO: Move cache to qbz-core in future refactor
    let cache = app_state.audio_cache.clone();
    if let Some(cached) = cache.get(track_id) {
        log::info!("[V2/CACHE HIT] Track {} from MEMORY cache ({} bytes)", track_id, cached.size_bytes);
        player.play_data(cached.data, track_id)?;

        // Prefetch next tracks in background
        drop(bridge_guard);
        spawn_v2_prefetch(bridge.0.clone(), cache.clone(), &app_state.queue, final_quality, streaming_only);
        return Ok(V2PlayTrackResult { format_id: None });
    }

    // Check playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!("[V2/CACHE HIT] Track {} from DISK cache ({} bytes)", track_id, audio_data.len());
            cache.insert(track_id, audio_data.clone());
            player.play_data(audio_data, track_id)?;

            // Prefetch next tracks in background
            drop(bridge_guard);
            spawn_v2_prefetch(bridge.0.clone(), cache.clone(), &app_state.queue, final_quality, streaming_only);
            return Ok(V2PlayTrackResult { format_id: None });
        }
    }

    // Not in any cache - get stream URL from Qobuz via CoreBridge
    log::info!("[V2] Track {} not in cache, fetching from network...", track_id);

    let stream_url = bridge_guard.get_stream_url(track_id, final_quality).await?;
    log::info!("[V2] Got stream URL for track {} (format_id={})", track_id, stream_url.format_id);

    // Download the audio
    let audio_data = download_audio(&stream_url.url).await?;
    let data_size = audio_data.len();

    // Cache it (unless streaming_only mode)
    if !streaming_only {
        cache.insert(track_id, audio_data.clone());
        log::info!("[V2/CACHED] Track {} stored in memory cache", track_id);
    } else {
        log::info!("[V2/NOT CACHED] Track {} - streaming_only mode active", track_id);
    }

    // Play it via qbz-player
    player.play_data(audio_data, track_id)?;
    log::info!("[V2] Playing track {} ({} bytes)", track_id, data_size);

    // Prefetch next tracks in background
    drop(bridge_guard);
    spawn_v2_prefetch(bridge.0.clone(), cache, &app_state.queue, final_quality, streaming_only);

    Ok(V2PlayTrackResult { format_id: Some(stream_url.format_id) })
}
