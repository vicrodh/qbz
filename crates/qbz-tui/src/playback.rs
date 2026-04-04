//! Playback orchestration for the TUI.
//!
//! Replicates the desktop app's playback pipeline:
//! 1. Load quality preference from AudioSettings
//! 2. Check L2 disk cache before network
//! 3. Get stream URL from Qobuz API
//! 4. Either stream (start playing before full download) or download-first
//! 5. Cache download (unless streaming_only)
//! 6. Play via player.play_data() or player.play_streaming_dynamic()
//! 7. Auto-advance to next track in queue when current ends

use std::sync::Arc;
use tokio::sync::mpsc;

use qbz_cache::PlaybackCache;
use qbz_core::QbzCore;
use qbz_models::{QueueTrack, Quality};

use crate::adapter::TuiAdapter;

/// Status updates sent from the playback pipeline to the UI.
#[derive(Debug, Clone)]
pub enum PlaybackStatus {
    /// Buffering phase with a human-readable message for the status bar.
    Buffering(String),
    /// Track is now playing.
    Playing,
    /// An error occurred during the pipeline.
    Error(String),
}

/// Load the user's streaming_only preference from the AudioSettings SQLite store.
///
/// Quality preference (Hi-Res/CD/MP3) is stored on the frontend side (localStorage),
/// not in AudioSettings. Default to HiRes for the TUI until we build a TUI settings view.
pub fn load_quality_settings() -> (Quality, bool) {
    use qbz_audio::settings::AudioSettingsStore;

    // Load streaming quality from TUI preference file
    let quality = {
        let quality_path = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("qbz")
            .join("tui_streaming_quality");
        match std::fs::read_to_string(&quality_path) {
            Ok(saved) => match saved.trim() {
                "MP3" => Quality::Mp3,
                "CD" => Quality::Lossless,
                "Hi-Res" => Quality::HiRes,
                "Hi-Res+" => Quality::UltraHiRes,
                _ => Quality::HiRes,
            },
            Err(_) => Quality::HiRes, // default
        }
    };

    match AudioSettingsStore::new() {
        Ok(store) => match store.get_settings() {
            Ok(settings) => {
                let streaming_only = settings.streaming_only;
                log::info!(
                    "[TUI] Audio settings loaded: quality={:?}, streaming_only={}",
                    quality,
                    streaming_only,
                );
                (quality, streaming_only)
            }
            Err(e) => {
                log::warn!("[TUI] Failed to load audio settings: {}, using defaults", e);
                (quality, false)
            }
        },
        Err(e) => {
            log::warn!(
                "[TUI] Failed to open audio settings store: {}, using defaults",
                e
            );
            (quality, false)
        }
    }
}

/// Load the stream_first_track and stream_buffer_seconds settings.
fn load_streaming_settings() -> (bool, u8) {
    use qbz_audio::settings::AudioSettingsStore;

    match AudioSettingsStore::new() {
        Ok(store) => match store.get_settings() {
            Ok(settings) => (settings.stream_first_track, settings.stream_buffer_seconds),
            Err(_) => (false, 3),
        },
        Err(_) => (false, 3),
    }
}

/// Play a Qobuz track through the full pipeline: cache check, stream URL, download, cache, play.
///
/// Sends status updates through `status_tx` so the UI can display progress.
pub async fn play_qobuz_track(
    core: &QbzCore<TuiAdapter>,
    track_id: u64,
    playback_cache: &Option<Arc<PlaybackCache>>,
    status_tx: &mpsc::UnboundedSender<PlaybackStatus>,
) -> Result<(), String> {
    // 1. Load quality preference from user's AudioSettings
    let (quality, streaming_only) = load_quality_settings();

    // 2. Check L2 disk cache before hitting the network
    if let Some(cache) = playback_cache {
        if let Some(cached_data) = cache.get(track_id) {
            log::info!(
                "[TUI] Cache HIT for track {} ({} bytes)",
                track_id,
                cached_data.len()
            );
            let _ = status_tx.send(PlaybackStatus::Buffering("Playing from cache...".into()));

            let player = core.player();
            player
                .play_data(cached_data, track_id)
                .map_err(|e| format!("Playback failed: {}", e))?;

            let _ = status_tx.send(PlaybackStatus::Playing);
            return Ok(());
        } else {
            log::debug!("[TUI] Cache MISS for track {}", track_id);
        }
    }

    // 3. Get stream URL from Qobuz API (with quality)
    let _ = status_tx.send(PlaybackStatus::Buffering("Getting stream...".into()));

    let stream_url = core
        .get_stream_url(track_id, quality)
        .await
        .map_err(|e| format!("Stream URL failed: {}", e))?;

    log::info!(
        "[TUI] Got stream URL for track {}: format_id={}, {:.1}kHz/{}bit",
        track_id,
        stream_url.format_id,
        stream_url.sampling_rate,
        stream_url.bit_depth.unwrap_or(16),
    );

    // 4. Check if streaming mode is enabled
    let (stream_first, _buffer_seconds) = load_streaming_settings();

    if stream_first {
        // STREAMING MODE: Start playing before download completes
        play_streaming_mode(
            core,
            track_id,
            &stream_url.url,
            stream_url.sampling_rate,
            stream_url.bit_depth.unwrap_or(16),
            playback_cache,
            status_tx,
            streaming_only,
        )
        .await
    } else {
        // DOWNLOAD MODE: Download entire file, then play (original behavior)
        play_download_mode(
            core,
            track_id,
            &stream_url.url,
            playback_cache,
            status_tx,
            streaming_only,
        )
        .await
    }
}

/// Download the entire file before starting playback (original behavior).
async fn play_download_mode(
    core: &QbzCore<TuiAdapter>,
    track_id: u64,
    url: &str,
    playback_cache: &Option<Arc<PlaybackCache>>,
    status_tx: &mpsc::UnboundedSender<PlaybackStatus>,
    streaming_only: bool,
) -> Result<(), String> {
    let _ = status_tx.send(PlaybackStatus::Buffering("Downloading...".into()));

    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("Download failed: {}", e))?;
    let audio_data = response
        .bytes()
        .await
        .map_err(|e| format!("Read failed: {}", e))?;

    log::info!(
        "[TUI] Downloaded {} bytes for track {}",
        audio_data.len(),
        track_id
    );

    let audio_vec = audio_data.to_vec();

    // Cache the download (unless streaming_only mode)
    if !streaming_only {
        if let Some(cache) = playback_cache {
            cache.insert(track_id, &audio_vec);
            log::debug!("[TUI] Cached track {} to L2 disk", track_id);
        }
    } else {
        log::debug!(
            "[TUI] Skipping cache write for track {} (streaming_only=true)",
            track_id
        );
    }

    // Play via player
    let player = core.player();
    player
        .play_data(audio_vec, track_id)
        .map_err(|e| format!("Playback failed: {}", e))?;

    let _ = status_tx.send(PlaybackStatus::Playing);

    Ok(())
}

/// Stream audio: probe the URL, start playback via Player's streaming API,
/// then feed chunks as they download. Audio starts within seconds.
async fn play_streaming_mode(
    core: &QbzCore<TuiAdapter>,
    track_id: u64,
    url: &str,
    api_sampling_rate: f64,
    api_bit_depth: u32,
    playback_cache: &Option<Arc<PlaybackCache>>,
    status_tx: &mpsc::UnboundedSender<PlaybackStatus>,
    streaming_only: bool,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::time::{Duration, Instant};

    let _ = status_tx.send(PlaybackStatus::Buffering("Probing stream...".into()));

    // Get the current track's duration from the queue state
    let queue_state = core.get_queue_state().await;
    let duration_secs = queue_state
        .current_track
        .as_ref()
        .map(|track| track.duration_secs)
        .unwrap_or(0);

    // --- Probe: HEAD for content-length + first 64KB for audio format + speed measurement ---
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let head_response = client
        .head(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| format!("HEAD request failed: {}", e))?;

    if !head_response.status().is_success() {
        log::warn!(
            "[TUI/STREAMING] HEAD failed ({}), falling back to download mode",
            head_response.status()
        );
        return play_download_mode(core, track_id, url, playback_cache, status_tx, streaming_only)
            .await;
    }

    let content_length = head_response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    if content_length == 0 {
        log::warn!("[TUI/STREAMING] No content-length, falling back to download mode");
        return play_download_mode(core, track_id, url, playback_cache, status_tx, streaming_only)
            .await;
    }

    // Download first 64KB to probe audio format and measure speed
    let probe_start = Instant::now();
    let range_response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Range", "bytes=0-65535")
        .send()
        .await
        .map_err(|e| format!("Range request failed: {}", e))?;

    let initial_bytes = range_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read probe bytes: {}", e))?;

    let probe_elapsed = probe_start.elapsed();
    let speed_mbps = if probe_elapsed.as_secs_f64() > 0.0 {
        (initial_bytes.len() as f64 / probe_elapsed.as_secs_f64()) / (1024.0 * 1024.0)
    } else {
        10.0 // assume fast
    };

    log::info!(
        "[TUI/STREAMING] Probe: {}KB in {:.0}ms = {:.1} MB/s",
        initial_bytes.len() / 1024,
        probe_elapsed.as_millis(),
        speed_mbps,
    );

    // Extract audio format from FLAC header (same logic as desktop)
    let (sample_rate, channels, bit_depth) =
        if initial_bytes.len() >= 26 && initial_bytes.starts_with(b"fLaC") {
            let sr = ((initial_bytes[18] as u32) << 12)
                | ((initial_bytes[19] as u32) << 4)
                | ((initial_bytes[20] as u32) >> 4);
            let ch = ((initial_bytes[20] >> 1) & 0x07) + 1;
            let bps = ((initial_bytes[20] & 0x01) << 4) | ((initial_bytes[21] >> 4) & 0x0F);
            (sr, ch as u16, (bps + 1) as u32)
        } else {
            log::warn!(
                "[TUI/STREAMING] Non-FLAC stream, using API metadata ({:.1}kHz, {}bit)",
                api_sampling_rate,
                api_bit_depth,
            );
            // sampling_rate from API is in kHz (e.g. 44.1, 96.0, 192.0)
            ((api_sampling_rate * 1000.0) as u32, 2, api_bit_depth)
        };

    log::info!(
        "[TUI/STREAMING] Format: {}Hz, {} ch, {}-bit, {:.2} MB, {:.1} MB/s, {}s",
        sample_rate,
        channels,
        bit_depth,
        content_length as f64 / (1024.0 * 1024.0),
        speed_mbps,
        duration_secs,
    );

    let _ = status_tx.send(PlaybackStatus::Buffering("Streaming...".into()));

    // --- Start streaming playback via Player ---
    let player = core.player();
    let buffer_writer = player
        .play_streaming_dynamic(
            track_id,
            sample_rate,
            channels,
            bit_depth,
            content_length,
            speed_mbps,
            duration_secs,
        )
        .map_err(|e| format!("Streaming init failed: {}", e))?;

    let _ = status_tx.send(PlaybackStatus::Playing);

    // --- Full download: feed chunks to the player buffer ---
    let response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| format!("Failed to start stream: {}", e))?;

    if !response.status().is_success() {
        let _ = buffer_writer.error(format!("Stream request failed: {}", response.status()));
        return Err(format!("Stream request failed: {}", response.status()));
    }

    let mut all_data = Vec::with_capacity(content_length as usize);
    let mut stream = response.bytes_stream();
    let mut bytes_received = 0u64;
    let download_start = Instant::now();
    let mut last_log_time = Instant::now();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| {
            let msg = format!("Stream chunk error: {}", e);
            log::error!("[TUI/STREAMING] {}", msg);
            let _ = buffer_writer.error(msg.clone());
            msg
        })?;

        bytes_received += chunk.len() as u64;

        if let Err(e) = buffer_writer.push_chunk(&chunk) {
            log::error!("[TUI/STREAMING] Failed to push chunk: {}", e);
        }

        if !streaming_only {
            all_data.extend_from_slice(&chunk);
        }

        // Log progress every ~2s
        let now = Instant::now();
        if now.duration_since(last_log_time) >= Duration::from_secs(2) {
            let progress = if content_length > 0 {
                (bytes_received as f64 / content_length as f64) * 100.0
            } else {
                0.0
            };
            let avg_speed = (bytes_received as f64 / download_start.elapsed().as_secs_f64())
                / (1024.0 * 1024.0);
            log::info!(
                "[TUI/STREAMING] {:.1}% ({:.2}/{:.2} MB) @ {:.2} MB/s",
                progress,
                bytes_received as f64 / (1024.0 * 1024.0),
                content_length as f64 / (1024.0 * 1024.0),
                avg_speed,
            );
            last_log_time = now;
        }
    }

    // Mark download as complete so the player knows there is no more data
    if let Err(e) = buffer_writer.complete() {
        log::error!("[TUI/STREAMING] Failed to mark buffer complete: {}", e);
    }

    let total_time = download_start.elapsed();
    log::info!(
        "[TUI/STREAMING] Complete: {:.2} MB in {:.1}s ({:.2} MB/s)",
        bytes_received as f64 / (1024.0 * 1024.0),
        total_time.as_secs_f64(),
        if total_time.as_secs_f64() > 0.0 {
            (bytes_received as f64 / total_time.as_secs_f64()) / (1024.0 * 1024.0)
        } else {
            0.0
        },
    );

    // Cache the complete download for instant replay
    if !streaming_only {
        if let Some(cache) = playback_cache {
            cache.insert(track_id, &all_data);
            log::info!(
                "[TUI/STREAMING] Track {} cached for future playback",
                track_id
            );
        }
    }

    Ok(())
}

/// Check if the current track has ended and auto-advance to the next in queue.
///
/// Call this every tick from the main loop. Returns the next track if
/// auto-advance was triggered.
///
/// Detection logic: the track ended if we were playing on the previous tick,
/// are no longer playing now, and the position reached (or passed) the duration.
pub async fn check_auto_advance(
    core: &QbzCore<TuiAdapter>,
    was_playing: bool,
    is_playing: bool,
    position: u64,
    duration: u64,
) -> Option<QueueTrack> {
    // Track ended: was playing, now stopped, position near/past duration
    if was_playing && !is_playing && duration > 0 && position >= duration.saturating_sub(2) {
        log::info!(
            "[TUI] Track ended (pos={}, dur={}), advancing queue",
            position,
            duration
        );
        core.next_track().await
    } else {
        None
    }
}
