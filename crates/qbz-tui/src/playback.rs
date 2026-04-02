//! Playback orchestration for the TUI.
//!
//! Replicates the desktop app's playback pipeline:
//! 1. Load quality preference from AudioSettings
//! 2. Check L2 disk cache before network
//! 3. Get stream URL from Qobuz API
//! 4. Download audio data
//! 5. Cache download (unless streaming_only)
//! 6. Play via player.play_data()
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
fn load_quality_settings() -> (Quality, bool) {
    use qbz_audio::settings::AudioSettingsStore;

    match AudioSettingsStore::new() {
        Ok(store) => match store.get_settings() {
            Ok(settings) => {
                let streaming_only = settings.streaming_only;
                log::info!(
                    "[TUI] Audio settings loaded: streaming_only={}",
                    streaming_only,
                );
                (Quality::HiRes, streaming_only)
            }
            Err(e) => {
                log::warn!("[TUI] Failed to load audio settings: {}, using defaults", e);
                (Quality::HiRes, false)
            }
        },
        Err(e) => {
            log::warn!(
                "[TUI] Failed to open audio settings store: {}, using defaults",
                e
            );
            (Quality::HiRes, false)
        }
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
    let _ = status_tx.send(PlaybackStatus::Buffering("Getting stream URL...".into()));

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

    // 4. Download the full audio file
    let _ = status_tx.send(PlaybackStatus::Buffering("Downloading...".into()));

    let response = reqwest::get(&stream_url.url)
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

    // 5. Cache the download (unless streaming_only mode)
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

    // 6. Play via player
    let player = core.player();
    player
        .play_data(audio_vec, track_id)
        .map_err(|e| format!("Playback failed: {}", e))?;

    // 7. Notify UI: playing
    let _ = status_tx.send(PlaybackStatus::Playing);

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
