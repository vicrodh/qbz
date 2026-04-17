use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

use qbz_models::{Quality, QueueTrack as CoreQueueTrack, RepeatMode};

use crate::audio::{AlsaPlugin, AudioBackendType};
use crate::cache::AudioCache;
use crate::config::audio_settings::AudioSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::library::LibraryState;
use crate::offline_cache::OfflineCacheState;
use crate::runtime::{CommandRequirement, RuntimeError, RuntimeManagerState};
use crate::AppState;

use super::{
    cached_quality_below_requested, download_with_backoff, limit_quality_for_device,
    parse_quality, try_cmaf_full_download, try_cmaf_streaming_setup, v2_cmaf_stream,
    v2_download_and_stream, v2_get_stream_info, v2_library_get_tracks_by_ids,
};
// Linux-only: ALSA hardware capability check. The callsites that use it are
// already gated on `cfg(target_os = "linux")`, so the import has to match or
// macOS fails with "unresolved import".
#[cfg(target_os = "linux")]
use super::cached_audio_incompatible_with_hw;

// ==================== Prefetch (V2) ====================

/// Number of Qobuz tracks to prefetch (not total tracks, just Qobuz).
/// Higher values keep more upcoming tracks in cache for instant playback.
/// CMAF segment downloads are ~24s per Hi-Res track, so 5 tracks means
/// the cache stays ~2 minutes ahead of playback.
const V2_PREFETCH_COUNT: usize = 5;

/// How far ahead to look for tracks to prefetch (to handle mixed playlists
/// with local/offline tracks interspersed with Qobuz tracks)
const V2_PREFETCH_LOOKAHEAD: usize = 15;

/// Maximum concurrent prefetch downloads (track-level, not segment-level).
/// 2 tracks downloading simultaneously keeps the cache filling while
/// one download is in its cooldown/decryption phase.
const V2_MAX_CONCURRENT_PREFETCH: usize = 2;

lazy_static::lazy_static! {
    /// Semaphore to limit concurrent prefetch operations
    static ref V2_PREFETCH_SEMAPHORE: tokio::sync::Semaphore =
        tokio::sync::Semaphore::new(V2_MAX_CONCURRENT_PREFETCH);
}

/// Spawn background tasks to prefetch upcoming Qobuz tracks (V2)
/// Takes upcoming tracks directly from CoreBridge (not legacy AppState queue)
fn spawn_v2_prefetch(
    bridge: Arc<RwLock<Option<crate::core_bridge::CoreBridge>>>,
    cache: Arc<AudioCache>,
    upcoming_tracks: Vec<CoreQueueTrack>,
    quality: Quality,
    streaming_only: bool,
) {
    spawn_v2_prefetch_with_hw_check(
        bridge,
        cache,
        upcoming_tracks,
        quality,
        streaming_only,
        None,
    );
}

/// Prefetch with optional hardware rate checking.
/// If `hw_device_id` is Some, checks each track's sample rate against hardware
/// and downgrades quality if needed.
fn spawn_v2_prefetch_with_hw_check(
    bridge: Arc<RwLock<Option<crate::core_bridge::CoreBridge>>>,
    cache: Arc<AudioCache>,
    upcoming_tracks: Vec<CoreQueueTrack>,
    quality: Quality,
    streaming_only: bool,
    hw_device_id: Option<String>,
) {
    // Skip prefetch entirely in streaming_only mode
    if streaming_only {
        log::debug!("[V2/PREFETCH] Skipped - streaming_only mode active");
        return;
    }

    // upcoming_tracks already provided by caller from CoreBridge
    let upcoming_tracks = upcoming_tracks;

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
            log::debug!(
                "[V2/PREFETCH] Skipping local track: {} - {}",
                track_id,
                track_title
            );
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
        let hw_device_clone = hw_device_id.clone();

        log::info!(
            "[V2/PREFETCH] Prefetching track: {} - {}",
            track_id,
            track_title
        );

        // Spawn background task for each track (with semaphore to limit concurrency)
        tokio::spawn(async move {
            // Acquire semaphore permit to limit concurrent prefetches
            let _permit = match V2_PREFETCH_SEMAPHORE.acquire().await {
                Ok(permit) => permit,
                Err(_) => {
                    log::warn!(
                        "[V2/PREFETCH] Semaphore closed, skipping track {}",
                        track_id
                    );
                    cache_clone.unmark_fetching(track_id);
                    return;
                }
            };

            // Determine effective quality (may be downgraded for hardware compatibility).
            // Iterates down the quality ladder until the returned stream sample rate is
            // something the hardware can play. Both UltraHiRes and HiRes can yield
            // 96 kHz streams, so a single UltraHiRes→HiRes step is not enough for
            // DACs that top out at 48 kHz — we continue to Lossless (44.1 kHz) if needed.
            let effective_quality = {
                let mut eq = quality;
                #[cfg(target_os = "linux")]
                if let Some(ref device_id) = hw_device_clone {
                    let bridge_guard = bridge_clone.read().await;
                    if let Some(bridge) = bridge_guard.as_ref() {
                        if let Ok(initial_url) = bridge.get_stream_url(track_id, quality).await {
                            let track_rate = (initial_url.sampling_rate * 1000.0) as u32;
                            if qbz_audio::device_supports_sample_rate(device_id, track_rate)
                                == Some(false)
                            {
                                for try_quality in [Quality::HiRes, Quality::Lossless] {
                                    if try_quality >= quality {
                                        continue;
                                    }
                                    match bridge.get_stream_url(track_id, try_quality).await {
                                        Ok(alt_url) => {
                                            let alt_rate =
                                                (alt_url.sampling_rate * 1000.0) as u32;
                                            if qbz_audio::device_supports_sample_rate(
                                                device_id, alt_rate,
                                            ) != Some(false)
                                            {
                                                log::info!(
                                                    "[V2/PREFETCH] Track {} at {}Hz incompatible with hardware, prefetching at {:?} ({}Hz)",
                                                    track_id,
                                                    track_rate,
                                                    try_quality,
                                                    alt_rate
                                                );
                                                eq = try_quality;
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            log::debug!(
                                                "[V2/PREFETCH] Failed to probe {:?} for track {}: {}",
                                                try_quality,
                                                track_id,
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                eq
            };

            let result = async {
                let bridge_guard = bridge_clone.read().await;
                let bridge = bridge_guard.as_ref().ok_or("CoreBridge not initialized")?;

                // Try CMAF first (Akamai CDN), fall back to legacy (nginx CDN)
                match try_cmaf_full_download(bridge, track_id, effective_quality).await {
                    Ok(data) => return Ok::<Vec<u8>, String>(data),
                    Err(e) => {
                        log::warn!("[V2/PREFETCH] CMAF failed for track {}: {}, trying legacy", track_id, e);
                    }
                }

                let stream_url = bridge.get_stream_url(track_id, effective_quality).await?;
                let (data, _url) = download_with_backoff(&stream_url.url, track_id, effective_quality, bridge).await?;
                Ok(data)
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
                    log::warn!(
                        "[V2/PREFETCH] Failed for track {} after all retries: {}",
                        track_id,
                        e
                    );
                }
            }

            cache_clone.unmark_fetching(track_id);
        });
    }
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
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] Command: pause_playback");
    app_state.media_controls.set_playback(false);
    let bridge = bridge.get().await;
    bridge.pause().map_err(RuntimeError::Internal)
}

/// Resume playback (V2)
#[tauri::command]
pub async fn v2_resume_playback(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] Command: resume_playback");
    app_state.media_controls.set_playback(true);
    let bridge = bridge.get().await;
    bridge.resume().map_err(RuntimeError::Internal)
}

/// Stop playback (V2)
#[tauri::command]
pub async fn v2_stop_playback(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] Command: stop_playback");
    app_state.media_controls.set_stopped();
    let bridge = bridge.get().await;
    bridge.stop().map_err(RuntimeError::Internal)
}

/// Seek to position in seconds (V2)
#[tauri::command]
pub async fn v2_seek(
    position: u64,
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] Command: seek {}", position);
    let bridge_guard = bridge.get().await;
    bridge_guard
        .seek(position)
        .map_err(RuntimeError::Internal)?;

    // Update MPRIS with effective playback state only after successful seek.
    let playback_state = bridge_guard.get_playback_state();
    app_state
        .media_controls
        .set_playback_with_progress(playback_state.is_playing, playback_state.position);

    Ok(())
}

/// Set volume (0.0 - 1.0) (V2)
///
/// When ALSA Direct hw: is active, volume is forced to 1.0 (100%)
/// because hw: bypasses all software mixing — volume must be
/// controlled at the DAC/hardware level.
#[tauri::command]
pub async fn v2_set_volume(
    volume: f32,
    bridge: State<'_, CoreBridgeState>,
    audio_state: State<'_, AudioSettingsState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    // Force 100% volume when ALSA Direct hw: is active
    let effective_volume = {
        let is_alsa_hw = audio_state
            .store
            .lock()
            .ok()
            .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()))
            .map(|s| {
                s.backend_type == Some(AudioBackendType::Alsa)
                    && s.alsa_plugin == Some(AlsaPlugin::Hw)
            })
            .unwrap_or(false);
        if is_alsa_hw { 1.0 } else { volume }
    };

    let bridge = bridge.get().await;
    bridge
        .set_volume(effective_volume)
        .map_err(RuntimeError::Internal)
}

/// Get current playback state (V2) - also updates MPRIS progress
#[tauri::command]
pub async fn v2_get_playback_state(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
) -> Result<qbz_player::PlaybackState, RuntimeError> {
    let bridge = bridge.get().await;
    let playback_state = bridge.get_playback_state();

    // Update MPRIS with current progress (called every ~500ms from frontend)
    app_state
        .media_controls
        .set_playback_with_progress(playback_state.is_playing, playback_state.position);

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
) -> Result<(), RuntimeError> {
    log::info!("[V2] Command: set_media_metadata - {} by {}", title, artist);
    crate::update_media_controls_metadata(
        &app_state.media_controls,
        &title,
        &artist,
        &album,
        duration_secs,
        cover_url,
    );
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
    library_state: State<'_, LibraryState>,
    app_handle: tauri::AppHandle,
) -> Result<bool, RuntimeError> {
    log::info!("[V2] Command: play_next_gapless for track {}", track_id);

    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();
    let current_track_id = player.state.current_track_id();
    let repeat_mode = bridge_guard.get_queue_state().await.repeat;

    // Defensive guard: never queue the currently playing track as "next".
    // This avoids infinite one-track loops when frontend queue state is stale.
    if current_track_id != 0 && repeat_mode != RepeatMode::One && current_track_id == track_id {
        log::warn!(
            "[V2/GAPLESS] Ignoring play_next_gapless for current track {}",
            track_id
        );
        return Ok(false);
    }

    // Tier order: L1 memory → L2 disk → offline cache → local library.
    //
    // L1/L2 come first because the offline cache v2 path requires a disk
    // read + AES decrypt + FLAC assembly (~5-7s for a HiRes track). If
    // the same track is already in L1 (prefetch from CDN) we'd otherwise
    // pay the decrypt cost for bytes we already have in RAM — and by the
    // time we finish, the player's gapless engine has been dropped.

    let cache = app_state.audio_cache.clone();

    // L1: in-memory
    if let Some(cached) = cache.get(track_id) {
        log::info!(
            "[V2/GAPLESS] Track {} from MEMORY cache ({} bytes)",
            track_id,
            cached.size_bytes
        );
        player
            .play_next(cached.data, track_id)
            .map_err(RuntimeError::Internal)?;
        return Ok(true);
    }

    // L2: on-disk plain-FLAC playback cache
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!(
                "[V2/GAPLESS] Track {} from DISK cache ({} bytes)",
                track_id,
                audio_data.len()
            );
            cache.insert(track_id, audio_data.clone());
            player
                .play_next(audio_data, track_id)
                .map_err(RuntimeError::Internal)?;
            return Ok(true);
        }
    }

    // Offline cache (persistent). Branch on cache_format:
    // - v2 (CMAF bundle) → decrypt to plain FLAC via load_cmaf_bundle
    // - v1 (plain FLAC)  → read segments_path directly
    {
        let bundle_row = {
            let db_opt = offline_cache.db.lock().await;
            db_opt
                .as_ref()
                .and_then(|db| db.get_cmaf_bundle(track_id).ok().flatten())
        };
        if let Some(row) = bundle_row {
            match row.cache_format {
                2 => {
                    let cache_path = offline_cache.get_cache_path();
                    let decrypted =
                        crate::offline_cache::playback::load_cmaf_bundle_with_ui_events(
                            &app_handle,
                            track_id,
                            track_id,
                            row.clone(),
                            cache_path,
                        )
                        .await;
                    if let Some(audio_data) = decrypted {
                        log::info!(
                            "[V2/GAPLESS] Track {} from OFFLINE cache (CMAF v2)",
                            track_id
                        );
                        // Warm L1 with the decrypted bytes so subsequent
                        // accesses (re-gapless, replay, scrub) skip the
                        // disk-read + decrypt and hit memory directly.
                        // Without this, every offline-cache gapless attempt
                        // re-does 5-7s of I/O + AES work.
                        app_state.audio_cache.insert(track_id, audio_data.clone());
                        player
                            .play_next(audio_data, track_id)
                            .map_err(RuntimeError::Internal)?;
                        return Ok(true);
                    } else {
                        log::warn!(
                            "[V2/GAPLESS] Track {} CMAF v2 bundle present but decrypt failed — skipping offline tier",
                            track_id
                        );
                    }
                }
                _ => {
                    let path = std::path::Path::new(&row.segments_path);
                    if !path.exists() {
                        log::warn!(
                            "[V2/GAPLESS/CACHE STALE] Track {} DB entry points to missing file {:?}",
                            track_id,
                            path
                        );
                    } else {
                        log::info!(
                            "[V2/GAPLESS] Track {} from OFFLINE cache (legacy)",
                            track_id
                        );
                        let audio_data = std::fs::read(path).map_err(|e| {
                            RuntimeError::Internal(format!("Failed to read cached file: {}", e))
                        })?;
                        player
                            .play_next(audio_data, track_id)
                            .map_err(RuntimeError::Internal)?;
                        return Ok(true);
                    }
                }
            }
        }
    }

    // Check local library. Library track ids are the row id (small
    // autoincrement), not the Qobuz track id. For source='qobuz_download'
    // tracks that happen to live in offline cache as v2 bundles, their
    // file_path in the library index is a directory, not a FLAC — we
    // need to translate row id → qobuz_track_id → offline cache lookup.
    if let Ok(track_id_i64) = i64::try_from(track_id) {
        let lib_track = v2_library_get_tracks_by_ids(vec![track_id_i64], library_state.clone())
            .await
            .ok()
            .and_then(|mut tracks| tracks.pop());

        if let Some(track) = lib_track {
            // Qobuz-cached library track: route through offline cache by
            // its Qobuz id instead of reading the display file_path.
            if track.source.as_deref() == Some("qobuz_download") {
                if let Some(qid) = track.qobuz_track_id {
                    let bundle_row = {
                        let db_opt = offline_cache.db.lock().await;
                        db_opt
                            .as_ref()
                            .and_then(|db| db.get_cmaf_bundle(qid as u64).ok().flatten())
                    };
                    if let Some(row) = bundle_row {
                        match row.cache_format {
                            2 => {
                                let cache_path = offline_cache.get_cache_path();
                                let decrypted =
                                    crate::offline_cache::playback::load_cmaf_bundle_with_ui_events(
                                        &app_handle,
                                        track_id,  // display: library row id
                                        qid as u64, // cmaf/bundle: qobuz id
                                        row.clone(),
                                        cache_path,
                                    )
                                    .await;
                                if let Some(audio_data) = decrypted {
                                    log::info!(
                                        "[V2/GAPLESS] Library track {} (qobuz {}) from OFFLINE cache (CMAF v2)",
                                        track_id,
                                        qid
                                    );
                                    // Warm L1 keyed by LIBRARY row id —
                                    // the player is fed library ids, so
                                    // future cache hits for this library
                                    // row land here.
                                    cache.insert(track_id, audio_data.clone());
                                    bridge
                                        .get()
                                        .await
                                        .player()
                                        .play_next(audio_data, track_id)
                                        .map_err(RuntimeError::Internal)?;
                                    return Ok(true);
                                }
                            }
                            _ => {
                                let path = std::path::Path::new(&row.segments_path);
                                if path.exists() {
                                    let audio_data = std::fs::read(path).map_err(|e| {
                                        RuntimeError::Internal(format!(
                                            "Failed to read v1 cached FLAC: {}",
                                            e
                                        ))
                                    })?;
                                    log::info!(
                                        "[V2/GAPLESS] Library track {} (qobuz {}) from OFFLINE cache (legacy)",
                                        track_id,
                                        qid
                                    );
                                    bridge
                                        .get()
                                        .await
                                        .player()
                                        .play_next(audio_data, track_id)
                                        .map_err(RuntimeError::Internal)?;
                                    return Ok(true);
                                }
                            }
                        }
                    }
                }
            }

            // Regular local library FLAC (user-owned file).
            let path = std::path::PathBuf::from(&track.file_path);
            if path.exists() {
                log::info!("[V2/GAPLESS] Track {} from LOCAL library", track_id);
                let audio_data = std::fs::read(&path).map_err(|e| {
                    RuntimeError::Internal(format!("Failed to read local file: {}", e))
                })?;
                bridge
                    .get()
                    .await
                    .player()
                    .play_next(audio_data, track_id)
                    .map_err(RuntimeError::Internal)?;
                return Ok(true);
            }
        }
    }

    log::info!(
        "[V2/GAPLESS] Track {} not in any cache, gapless not possible",
        track_id
    );
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
) -> Result<(), RuntimeError> {
    let preferred_quality = parse_quality(quality.as_deref());

    // Apply per-device sample rate limit if enabled
    let final_quality = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
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
        track_id,
        quality,
        preferred_quality,
        final_quality
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
    let result: Result<(), RuntimeError> = async {
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
                if !path.exists() {
                    log::warn!(
                        "[V2/PREFETCH/CACHE STALE] Track {} DB entry points to missing file {:?}",
                        track_id,
                        path
                    );
                }
                if path.exists() {
                    log::info!("[V2] Prefetching track {} from offline cache", track_id);
                    let audio_data = std::fs::read(path).map_err(|e| {
                        RuntimeError::Internal(format!("Failed to read cached file: {}", e))
                    })?;
                    cache.insert(track_id, audio_data);
                    return Ok(());
                }
            }
        }

        let bridge_guard = bridge.get().await;

        // Try CMAF first (Akamai CDN), fall back to legacy
        match try_cmaf_full_download(&*bridge_guard, track_id, final_quality).await {
            Ok(data) => {
                log::info!("[V2/CMAF] Prefetch succeeded for track {}", track_id);
                drop(bridge_guard);
                cache.insert(track_id, data);
                return Ok(());
            }
            Err(e) => {
                log::warn!("[V2/CMAF] Prefetch CMAF failed for track {}: {}, trying legacy", track_id, e);
            }
        }

        let stream_url = bridge_guard
            .get_stream_url(track_id, final_quality)
            .await
            .map_err(RuntimeError::Internal)?;
        drop(bridge_guard);

        let audio_data = super::download_audio(&stream_url.url)
            .await
            .map_err(RuntimeError::Internal)?;
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
    force_lowest_quality: Option<bool>,
    duration_secs: Option<u64>,
    bridge: State<'_, CoreBridgeState>,
    offline_cache: State<'_, OfflineCacheState>,
    audio_settings: State<'_, AudioSettingsState>,
    offline_state: State<'_, crate::offline::OfflineState>,
    app_state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<V2PlayTrackResult, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 playback
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let preferred_quality = parse_quality(quality.as_deref());

    // Apply per-device sample rate limit if enabled
    let final_quality = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
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

    // Override quality to Mp3 if force_lowest_quality is set (used by
    // QualityFallbackModal "always_fallback" preference)
    let final_quality = if force_lowest_quality.unwrap_or(false) {
        log::info!("[V2] force_lowest_quality=true, using Mp3");
        Quality::Mp3
    } else {
        final_quality
    };

    // Manual offline mode — read once up front so the cache-miss branch
    // below knows whether to fail loudly instead of silently reaching for
    // the network (issue #279).
    let manual_offline_mode = {
        let guard = offline_state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Offline state lock error: {}", e)))?;
        guard
            .as_ref()
            .and_then(|s| s.get_settings().ok())
            .map(|s| s.manual_offline_mode)
            .unwrap_or(false)
    };

    // Check streaming settings. In manual offline mode we also force
    // stream-first off at read time in case the set_manual_offline command
    // raced with a fresh settings write.
    let (stream_first_enabled, streaming_only) = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        match guard.as_ref().and_then(|s| s.get_settings().ok()) {
            Some(s) => {
                let stream_first = s.stream_first_track && !manual_offline_mode;
                (stream_first, s.streaming_only)
            }
            None => (false, false),
        }
    };

    // Determine hardware device ID for sample rate compatibility checks (ALSA only)
    #[cfg(target_os = "linux")]
    let hw_device_id: Option<String> = {
        let guard = audio_settings.store.lock().ok();
        guard
            .as_ref()
            .and_then(|g| g.as_ref())
            .and_then(|store| store.get_settings().ok())
            .and_then(|settings| {
                let is_alsa = matches!(
                    settings.backend_type,
                    Some(qbz_audio::AudioBackendType::Alsa)
                );
                if is_alsa {
                    settings.output_device.clone()
                } else {
                    None
                }
            })
    };
    #[cfg(not(target_os = "linux"))]
    let hw_device_id: Option<String> = None;

    log::info!(
        "[V2] play_track {} (quality_str={:?}, parsed={:?}, final={:?}, format_id={})",
        track_id,
        quality,
        preferred_quality,
        final_quality,
        final_quality.id()
    );

    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();

    // Fallback: if cache has lower quality than requested but network fails,
    // play the cached version rather than failing entirely.
    let mut low_quality_fallback: Option<Vec<u8>> = None;

    // Check offline cache (persistent disk cache)
    {
        // Pull the row once so we see cache_format + file_path + v2 bundle
        // columns without a second roundtrip. touch() updates last_accessed.
        let bundle_row = {
            let db_opt = offline_cache.db.lock().await;
            if let Some(db) = db_opt.as_ref() {
                if let Ok(Some(row)) = db.get_cmaf_bundle(track_id) {
                    let _ = db.touch(track_id);
                    Some(row)
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(row) = bundle_row {
            // v2 CMAF bundle: read init + encrypted segments, unwrap the
            // content key from the secret vault, decrypt to plain FLAC,
            // hand to the player just like any other cache hit.
            // spawn_blocking via the ui-events helper so the track row
            // shows an "unlocking" animation while decrypt runs.
            let audio_data_opt: Option<Vec<u8>> = if row.cache_format == 2 {
                let cache_path = offline_cache.get_cache_path();
                crate::offline_cache::playback::load_cmaf_bundle_with_ui_events(
                    &app_handle,
                    track_id,
                    track_id,
                    row.clone(),
                    cache_path,
                )
                .await
            } else {
                // cache_format = 1 (legacy plain FLAC)
                let path = std::path::Path::new(&row.segments_path);
                if !path.exists() {
                    log::warn!(
                        "[V2/CACHE STALE] Track {} DB entry points to missing file {:?} — entry is orphaned (filesystem moved/unmounted/cleaned?)",
                        track_id,
                        path
                    );
                    None
                } else {
                    match std::fs::read(path) {
                        Ok(bytes) => {
                            log::info!(
                                "[V2/CACHE HIT] Track {} from OFFLINE cache (legacy format): {:?}",
                                track_id,
                                path
                            );
                            Some(bytes)
                        }
                        Err(e) => {
                            log::warn!(
                                "[V2/CACHE] Track {} — failed to read legacy cache file: {}",
                                track_id,
                                e
                            );
                            None
                        }
                    }
                }
            };

            if let Some(audio_data) = audio_data_opt {

                // Check hardware compatibility (ALSA only)
                #[cfg(target_os = "linux")]
                let hw_incompatible =
                    cached_audio_incompatible_with_hw(&audio_data, &audio_settings);
                #[cfg(not(target_os = "linux"))]
                let hw_incompatible = false;

                // Check quality mismatch (all platforms)
                let quality_mismatch = cached_quality_below_requested(&audio_data, final_quality);

                if hw_incompatible {
                    log::info!(
                        "[V2/Quality] Skipping OFFLINE cache for track {} - incompatible sample rate",
                        track_id
                    );
                } else if quality_mismatch {
                    // Keep as fallback — don't discard, network might fail
                    low_quality_fallback = Some(audio_data);
                } else {
                    // Warm L1 with the decrypted bytes so the next
                    // access (replay, gapless re-queue, scrub) skips
                    // the 5-7s disk + AES decrypt round-trip. Without
                    // this, every offline-cache play_next_gapless fails
                    // to land in time and the player's gapless engine
                    // has already been dropped.
                    app_state.audio_cache.insert(track_id, audio_data.clone());
                    player
                        .play_data(audio_data, track_id)
                        .map_err(RuntimeError::Internal)?;

                    let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                    drop(bridge_guard);
                    spawn_v2_prefetch(
                        bridge.0.clone(),
                        app_state.audio_cache.clone(),
                        upcoming_tracks,
                        final_quality,
                        streaming_only,
                    );
                    return Ok(V2PlayTrackResult { format_id: None });
                }
            }
        }
    }

    // Check memory cache (L1)
    let cache = app_state.audio_cache.clone();
    if low_quality_fallback.is_none() {
        if let Some(cached) = cache.get(track_id) {
            log::info!(
                "[V2/CACHE HIT] Track {} from MEMORY cache ({} bytes)",
                track_id,
                cached.size_bytes
            );

            #[cfg(target_os = "linux")]
            let hw_incompatible = cached_audio_incompatible_with_hw(&cached.data, &audio_settings);
            #[cfg(not(target_os = "linux"))]
            let hw_incompatible = false;

            let quality_mismatch = cached_quality_below_requested(&cached.data, final_quality);

            if hw_incompatible {
                log::info!(
                    "[V2/Quality] Skipping MEMORY cache for track {} - incompatible sample rate",
                    track_id
                );
            } else if quality_mismatch {
                low_quality_fallback = Some(cached.data);
            } else {
                player
                    .play_data(cached.data, track_id)
                    .map_err(RuntimeError::Internal)?;

                let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                drop(bridge_guard);
                spawn_v2_prefetch(
                    bridge.0.clone(),
                    cache.clone(),
                    upcoming_tracks,
                    final_quality,
                    streaming_only,
                );
                return Ok(V2PlayTrackResult { format_id: None });
            }
        }
    }

    // Check playback cache (L2 - disk)
    if low_quality_fallback.is_none() {
        if let Some(playback_cache) = cache.get_playback_cache() {
            if let Some(audio_data) = playback_cache.get(track_id) {
                log::info!(
                    "[V2/CACHE HIT] Track {} from DISK cache ({} bytes)",
                    track_id,
                    audio_data.len()
                );

                #[cfg(target_os = "linux")]
                let hw_incompatible =
                    cached_audio_incompatible_with_hw(&audio_data, &audio_settings);
                #[cfg(not(target_os = "linux"))]
                let hw_incompatible = false;

                let quality_mismatch =
                    cached_quality_below_requested(&audio_data, final_quality);

                if hw_incompatible {
                    log::info!(
                        "[V2/Quality] Skipping DISK cache for track {} - incompatible sample rate",
                        track_id
                    );
                } else if quality_mismatch {
                    low_quality_fallback = Some(audio_data);
                } else {
                    cache.insert(track_id, audio_data.clone());
                    player
                        .play_data(audio_data, track_id)
                        .map_err(RuntimeError::Internal)?;

                    let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                    drop(bridge_guard);
                    spawn_v2_prefetch(
                        bridge.0.clone(),
                        cache.clone(),
                        upcoming_tracks,
                        final_quality,
                        streaming_only,
                    );
                    return Ok(V2PlayTrackResult { format_id: None });
                }
            }
        }
    }

    // Not in cache (or cached at lower quality) — fetch from network
    if low_quality_fallback.is_some() {
        log::info!(
            "[V2] Track {} cached at lower quality, re-downloading at {:?}...",
            track_id, final_quality
        );
    } else {
        log::info!(
            "[V2] Track {} not in cache, fetching from network...",
            track_id
        );
    }

    // Offline guard: if the user has manual offline mode enabled and we got
    // this far (meaning no cache had a usable hit), refuse to reach the
    // network. Returning an explicit error lets the frontend show a clear
    // "not available offline" toast instead of streaming invisibly — that
    // silent fallback was the root of issue #279.
    //
    // If we have a low-quality fallback in memory we play that instead; the
    // user at least hears something, which is better than nothing, and a
    // partial match is a real offline cache hit (just at a degraded tier).
    if manual_offline_mode && low_quality_fallback.is_none() {
        log::warn!(
            "[V2/OFFLINE] Track {} not in any cache and manual offline mode is ON — refusing network fetch",
            track_id
        );
        return Err(RuntimeError::TrackNotAvailableOffline);
    }

    // Try CMAF streaming pipeline (Akamai CDN, encrypted segments)
    // Only the init segment is fetched synchronously; audio segments stream in background.
    log::info!("[V2/CMAF] Attempting CMAF streaming for track {}", track_id);
    match try_cmaf_streaming_setup(&*bridge_guard, track_id, final_quality).await {
        Ok(cmaf_info) => {
            let format_id = cmaf_info.format_id;

            // Derive stream parameters from init segment and file URL metadata
            let sample_rate = cmaf_info.sampling_rate.unwrap_or(44100);
            let channels = 2u16; // FLAC from Qobuz is always stereo
            let bit_depth = cmaf_info.bit_depth.unwrap_or(16);
            let total_flac_size = cmaf_info.flac_header.len() as u64
                + cmaf_info
                    .segment_table
                    .iter()
                    .map(|s| s.byte_len as u64)
                    .sum::<u64>();

            // Estimate speed from init segment fetch (conservative: assume ~10 MB/s if
            // init was too fast to measure reliably)
            let speed_mbps = if cmaf_info.init_fetch_ms > 0 {
                let init_bytes = cmaf_info.flac_header.len() as f64 + 4096.0; // rough init size
                (init_bytes / (cmaf_info.init_fetch_ms as f64 / 1000.0)) / (1024.0 * 1024.0)
            } else {
                10.0
            };

            log::info!(
                "[V2/CMAF] Streaming setup: {}Hz, {}-bit, {:.2} MB total, {:.1} MB/s est, {} segments",
                sample_rate,
                bit_depth,
                total_flac_size as f64 / (1024.0 * 1024.0),
                speed_mbps,
                cmaf_info.n_segments
            );

            // Create streaming buffer and start playback immediately
            let buffer_writer = player
                .play_streaming_dynamic(
                    track_id,
                    sample_rate,
                    channels,
                    bit_depth,
                    total_flac_size,
                    speed_mbps,
                    duration_secs.unwrap_or(0),
                )
                .map_err(RuntimeError::Internal)?;

            // Spawn background task to fetch + decrypt + push audio segments
            let url_template = cmaf_info.url_template.clone();
            let content_key = cmaf_info.content_key;
            let flac_header = cmaf_info.flac_header;
            let n_segments = cmaf_info.n_segments;
            let cache_clone = cache.clone();
            let skip_cache = streaming_only;

            tokio::spawn(async move {
                match v2_cmaf_stream(
                    &url_template,
                    n_segments,
                    content_key,
                    flac_header,
                    buffer_writer,
                    track_id,
                    cache_clone,
                    skip_cache,
                )
                .await
                {
                    Ok(()) => {
                        if skip_cache {
                            log::info!(
                                "[V2/CMAF-STREAM COMPLETE] Track {} - NOT cached (streaming_only)",
                                track_id
                            );
                        } else {
                            log::info!(
                                "[V2/CMAF-STREAM COMPLETE] Track {} - cached for instant replay",
                                track_id
                            );
                        }
                    }
                    Err(e) => log::error!("[V2/CMAF-STREAM ERROR] Track {}: {}", track_id, e),
                }
            });

            // Prefetch next tracks in background
            let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
            drop(bridge_guard);
            spawn_v2_prefetch_with_hw_check(
                bridge.0.clone(),
                cache,
                upcoming_tracks,
                final_quality,
                streaming_only,
                hw_device_id,
            );

            return Ok(V2PlayTrackResult {
                format_id: Some(format_id),
            });
        }
        Err(e) => {
            log::warn!("[V2/CMAF] Streaming setup failed: {}, falling back to legacy download", e);
            // Fall through to existing legacy path
        }
    }

    let stream_url_result = bridge_guard
        .get_stream_url(track_id, final_quality)
        .await;

    let mut stream_url = match stream_url_result {
        Ok(url) => url,
        Err(e) => {
            // Network failed — use lower-quality fallback if available
            if let Some(fallback_data) = low_quality_fallback {
                log::warn!(
                    "[V2] Network failed for track {}: {}. Playing cached lower-quality version.",
                    track_id, e
                );
                player
                    .play_data(fallback_data, track_id)
                    .map_err(RuntimeError::Internal)?;
                let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                drop(bridge_guard);
                spawn_v2_prefetch(
                    bridge.0.clone(),
                    cache.clone(),
                    upcoming_tracks,
                    final_quality,
                    streaming_only,
                );
                return Ok(V2PlayTrackResult { format_id: None });
            }
            return Err(RuntimeError::Internal(e));
        }
    };
    log::info!(
        "[V2] Got stream URL for track {} (format_id={})",
        track_id,
        stream_url.format_id
    );

    // Smart quality downgrade for ALSA Direct: if the hardware doesn't support
    // the track's sample rate, re-request at progressively lower qualities until
    // one returns a stream the DAC can play. Both UltraHiRes and HiRes may yield
    // 96 kHz content, so a single downgrade step is not always enough (e.g., a
    // 48 kHz-only DAC requires falling through to Lossless at 44.1 kHz).
    if let Some(ref device_id) = hw_device_id {
        let track_rate = (stream_url.sampling_rate * 1000.0) as u32;
        if qbz_audio::device_supports_sample_rate(device_id, track_rate) == Some(false) {
            log::info!(
                "[V2/Quality] Hardware doesn't support {}Hz, searching for compatible quality tier",
                track_rate
            );
            for try_quality in [Quality::HiRes, Quality::Lossless] {
                if try_quality >= final_quality {
                    continue;
                }
                match bridge_guard.get_stream_url(track_id, try_quality).await {
                    Ok(alt_url) => {
                        let alt_rate = (alt_url.sampling_rate * 1000.0) as u32;
                        if qbz_audio::device_supports_sample_rate(device_id, alt_rate)
                            != Some(false)
                        {
                            log::info!(
                                "[V2/Quality] Falling back to {:?} ({}kHz) for hardware compatibility",
                                try_quality,
                                alt_url.sampling_rate
                            );
                            stream_url = alt_url;
                            break;
                        }
                        log::debug!(
                            "[V2/Quality] {:?} returns {}Hz — still incompatible, trying next tier",
                            try_quality,
                            alt_rate
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "[V2/Quality] Failed to probe {:?} stream URL: {}",
                            try_quality,
                            e
                        );
                    }
                }
            }
        }
    }

    if stream_first_enabled {
        // Streaming path: start playback before full download completes
        log::info!(
            "[V2/STREAMING] Track {} - streaming from network (cache_after: {})",
            track_id,
            !streaming_only
        );

        let stream_info = match v2_get_stream_info(&stream_url.url).await {
            Ok(info) => info,
            Err(e) => {
                // Probe failed (CDN EOF, etc.) — fall through to full download with backoff
                log::warn!(
                    "[V2/STREAMING] Probe failed for track {}: {}. Falling back to full download with backoff.",
                    track_id,
                    e
                );
                let effective_quality = Quality::from_id(stream_url.format_id)
                    .unwrap_or(final_quality);

                // Try CMAF full download first (Akamai CDN), fall back to legacy
                let audio_data = match try_cmaf_full_download(&*bridge_guard, track_id, effective_quality).await {
                    Ok(data) => {
                        log::info!("[V2/CMAF] Streaming-fallback download succeeded for track {}", track_id);
                        data
                    }
                    Err(cmaf_err) => {
                        log::warn!("[V2/CMAF] Streaming-fallback CMAF failed for track {}: {}, trying legacy", track_id, cmaf_err);
                        let (data, worked) = download_with_backoff(
                            &stream_url.url,
                            track_id,
                            effective_quality,
                            &*bridge_guard,
                        )
                        .await
                        .map_err(RuntimeError::Internal)?;
                        stream_url = worked;
                        data
                    }
                };

                let data_size = audio_data.len();
                if !streaming_only {
                    cache.insert(track_id, audio_data.clone());
                }
                player
                    .play_data(audio_data, track_id)
                    .map_err(RuntimeError::Internal)?;
                log::info!(
                    "[V2] Playing track {} ({} bytes, fallback from streaming)",
                    track_id,
                    data_size
                );
                let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                drop(bridge_guard);
                spawn_v2_prefetch_with_hw_check(
                    bridge.0.clone(),
                    cache,
                    upcoming_tracks,
                    final_quality,
                    streaming_only,
                    hw_device_id,
                );
                return Ok(V2PlayTrackResult {
                    format_id: Some(stream_url.format_id),
                });
            }
        };

        log::info!(
            "[V2/STREAMING] Info: {:.2} MB, {}Hz, {} ch, {}-bit, {:.1} MB/s",
            stream_info.content_length as f64 / (1024.0 * 1024.0),
            stream_info.sample_rate,
            stream_info.channels,
            stream_info.bit_depth,
            stream_info.speed_mbps
        );

        let buffer_writer = player
            .play_streaming_dynamic(
                track_id,
                stream_info.sample_rate,
                stream_info.channels,
                stream_info.bit_depth,
                stream_info.content_length,
                stream_info.speed_mbps,
                duration_secs.unwrap_or(0),
            )
            .map_err(RuntimeError::Internal)?;

        // Capture format_id before spawning background task
        let actual_format_id = stream_url.format_id;
        let url = stream_url.url.clone();
        let cache_clone = cache.clone();
        let content_len = stream_info.content_length;
        let skip_cache = streaming_only;

        // Spawn background download that feeds chunks to the player buffer
        tokio::spawn(async move {
            match v2_download_and_stream(
                &url,
                buffer_writer,
                track_id,
                cache_clone,
                content_len,
                skip_cache,
            )
            .await
            {
                Ok(()) => {
                    if skip_cache {
                        log::info!(
                            "[V2/STREAMING COMPLETE] Track {} - NOT cached (streaming_only)",
                            track_id
                        );
                    } else {
                        log::info!(
                            "[V2/STREAMING COMPLETE] Track {} - cached for instant replay",
                            track_id
                        );
                    }
                }
                Err(e) => log::error!("[V2/STREAMING ERROR] Track {}: {}", track_id, e),
            }
        });

        // Prefetch next tracks in background
        let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
        drop(bridge_guard);
        spawn_v2_prefetch_with_hw_check(
            bridge.0.clone(),
            cache,
            upcoming_tracks,
            final_quality,
            streaming_only,
            hw_device_id,
        );

        return Ok(V2PlayTrackResult {
            format_id: Some(actual_format_id),
        });
    }

    // Standard download path (streaming disabled) - full download before playback
    log::info!(
        "[V2/DOWNLOAD] Track {} - full download before playback (cache_after: {})",
        track_id,
        !streaming_only
    );
    let effective_quality = Quality::from_id(stream_url.format_id)
        .unwrap_or(final_quality);

    // Try CMAF full download first (Akamai CDN), fall back to legacy
    let audio_data = match try_cmaf_full_download(&*bridge_guard, track_id, effective_quality).await {
        Ok(data) => {
            log::info!("[V2/CMAF] Standard download succeeded for track {}", track_id);
            data
        }
        Err(cmaf_err) => {
            log::warn!("[V2/CMAF] Standard download CMAF failed for track {}: {}, trying legacy", track_id, cmaf_err);
            let (data, worked) = download_with_backoff(
                &stream_url.url,
                track_id,
                effective_quality,
                &*bridge_guard,
            )
            .await
            .map_err(RuntimeError::Internal)?;
            stream_url = worked;
            data
        }
    };

    let data_size = audio_data.len();

    // Cache it (unless streaming_only mode)
    if !streaming_only {
        cache.insert(track_id, audio_data.clone());
        log::info!("[V2/CACHED] Track {} stored in memory cache", track_id);
    } else {
        log::info!(
            "[V2/NOT CACHED] Track {} - streaming_only mode active",
            track_id
        );
    }

    // Play it via qbz-player
    player
        .play_data(audio_data, track_id)
        .map_err(RuntimeError::Internal)?;
    log::info!("[V2] Playing track {} ({} bytes)", track_id, data_size);

    // Prefetch next tracks in background (using CoreBridge queue)
    let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
    drop(bridge_guard);
    spawn_v2_prefetch_with_hw_check(
        bridge.0.clone(),
        cache,
        upcoming_tracks,
        final_quality,
        streaming_only,
        hw_device_id,
    );

    Ok(V2PlayTrackResult {
        format_id: Some(stream_url.format_id),
    })
}

// ==================== Playback Context Commands (V2) ====================

/// Get current playback context (V2)
#[tauri::command]
pub async fn v2_get_playback_context(
    app_state: State<'_, AppState>,
) -> Result<Option<crate::playback_context::PlaybackContext>, RuntimeError> {
    Ok(app_state.context.get_context())
}

/// Set playback context (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_set_playback_context(
    contextType: String,
    id: String,
    label: String,
    source: String,
    trackIds: Vec<u64>,
    startPosition: usize,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    use crate::playback_context::{ContentSource, ContextType, PlaybackContext};

    let ctx_type = match contextType.as_str() {
        "album" => ContextType::Album,
        "playlist" => ContextType::Playlist,
        "artist_top" => ContextType::ArtistTop,
        "label_top" => ContextType::LabelTop,
        "home_list" => ContextType::HomeList,
        "daily_q" => ContextType::DailyQ,
        "weekly_q" => ContextType::WeeklyQ,
        "fav_q" => ContextType::FavQ,
        "top_q" => ContextType::TopQ,
        "favorites" => ContextType::Favorites,
        "local_library" => ContextType::LocalLibrary,
        "radio" => ContextType::Radio,
        "search" => ContextType::Search,
        _ => {
            return Err(RuntimeError::Internal(format!(
                "Invalid context type: {}",
                contextType
            )))
        }
    };

    let content_source = match source.as_str() {
        "qobuz" => ContentSource::Qobuz,
        "local" => ContentSource::Local,
        "plex" => ContentSource::Plex,
        _ => {
            return Err(RuntimeError::Internal(format!(
                "Invalid source: {}",
                source
            )))
        }
    };

    let context =
        PlaybackContext::new(ctx_type, id, label, content_source, trackIds, startPosition);

    app_state.context.set_context(context);
    log::info!("[V2] set_playback_context: type={}", contextType);
    Ok(())
}

/// Clear playback context (V2)
#[tauri::command]
pub async fn v2_clear_playback_context(app_state: State<'_, AppState>) -> Result<(), RuntimeError> {
    app_state.context.clear_context();
    log::info!("[V2] clear_playback_context");
    Ok(())
}

/// Check if playback context is active (V2)
#[tauri::command]
pub async fn v2_has_playback_context(app_state: State<'_, AppState>) -> Result<bool, RuntimeError> {
    Ok(app_state.context.has_context())
}
