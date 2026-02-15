//! V2 Commands - Using the new multi-crate architecture
//!
//! These commands use QbzCore via CoreBridge instead of the old AppState.
//! They coexist with the old commands during migration.
//!
//! Playback flows through CoreBridge -> QbzCore -> Player (qbz-player crate).

use tauri::State;

use qbz_models::{Album, Artist, Quality, QueueState, RepeatMode, SearchResultsPage, Track, UserSession};

use crate::artist_blacklist::BlacklistState;
use crate::config::audio_settings::AudioSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::offline_cache::OfflineCacheState;
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
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.pause()
}

/// Resume playback (V2)
#[tauri::command]
pub async fn v2_resume_playback(
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.resume()
}

/// Stop playback (V2)
#[tauri::command]
pub async fn v2_stop_playback(
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.stop()
}

/// Seek to position in seconds (V2)
#[tauri::command]
pub async fn v2_seek(
    position: u64,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.seek(position)
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

/// Get current playback state (V2)
#[tauri::command]
pub async fn v2_get_playback_state(
    bridge: State<'_, CoreBridgeState>,
) -> Result<qbz_player::PlaybackState, String> {
    let bridge = bridge.get().await;
    Ok(bridge.get_playback_state())
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
        return Ok(V2PlayTrackResult { format_id: None });
    }

    // Check playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!("[V2/CACHE HIT] Track {} from DISK cache ({} bytes)", track_id, audio_data.len());
            cache.insert(track_id, audio_data.clone());
            player.play_data(audio_data, track_id)?;
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

    Ok(V2PlayTrackResult { format_id: Some(stream_url.format_id) })
}
