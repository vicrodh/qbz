//! Playback orchestration for the TUI.
//!
//! Replicates the desktop app's playback pipeline:
//! 1. Get stream URL from Qobuz API
//! 2. Download audio data
//! 3. Play via player.play_data()
//! 4. Auto-advance to next track in queue when current ends

use tokio::sync::mpsc;

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

/// Play a Qobuz track through the full pipeline: stream URL, download, play.
///
/// Sends status updates through `status_tx` so the UI can display progress.
pub async fn play_qobuz_track(
    core: &QbzCore<TuiAdapter>,
    track_id: u64,
    quality: Quality,
    status_tx: &mpsc::UnboundedSender<PlaybackStatus>,
) -> Result<(), String> {
    // 1. Notify UI: getting stream URL
    let _ = status_tx.send(PlaybackStatus::Buffering("Getting stream URL...".into()));

    // 2. Get stream URL from Qobuz API (with quality fallback)
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

    // 3. Notify UI: downloading
    let _ = status_tx.send(PlaybackStatus::Buffering("Downloading...".into()));

    // 4. Download the full audio file
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

    // 5. Play via player
    let player = core.player();
    player
        .play_data(audio_data.to_vec(), track_id)
        .map_err(|e| format!("Playback failed: {}", e))?;

    // 6. Notify UI: playing
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
