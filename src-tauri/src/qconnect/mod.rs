//! Qobuz Connect: WebSocket-based remote control protocol.
//!
//! This module is the qbz-side glue between the qconnect-app crate
//! (transport + protocol) and the local audio engine. The work is split
//! across focused submodules; this file owns only the cross-submodule
//! glue (shared constants, the `QconnectRemoteSyncState` accumulator
//! struct, and a few protocol-mapping helpers used by multiple
//! submodules).

mod commands;
mod corebridge;
mod event_sink;
mod queue_resolution;
mod service;
mod session;
pub mod startup;
mod track_loading;
pub(crate) mod transport;
mod types;

pub use commands::*;
pub use service::QconnectServiceState;
pub use session::{QconnectRendererInfo, QconnectSessionState};
pub use types::*;

use std::collections::HashMap;

use qbz_models::{QueueTrack, RepeatMode, Track};
use qconnect_app::QConnectQueueState;

use session::{QconnectFileAudioQualitySnapshot, QconnectSessionRendererState};

const QCONNECT_REMOTE_QUEUE_SOURCE: &str = "qobuz_connect_remote";

pub(super) const PLAYING_STATE_UNKNOWN: i32 = 0;
pub(super) const PLAYING_STATE_STOPPED: i32 = 1;
pub(super) const PLAYING_STATE_PLAYING: i32 = 2;
pub(super) const PLAYING_STATE_PAUSED: i32 = 3;
pub(super) const BUFFER_STATE_OK: i32 = 2;

// AudioQuality enum: 0=unknown, 1=mp3, 2=cd, 3=hires_l1, 4=hires_l2(192k), 5=hires_l3(384k)
pub(super) const AUDIO_QUALITY_UNKNOWN: i32 = 0;
pub(super) const AUDIO_QUALITY_MP3: i32 = 1;
pub(super) const AUDIO_QUALITY_CD: i32 = 2;
pub(super) const AUDIO_QUALITY_HIRES_LEVEL1: i32 = 3;
pub(super) const AUDIO_QUALITY_HIRES_LEVEL2: i32 = 4;
pub(super) const AUDIO_QUALITY_HIRES_LEVEL3: i32 = 5;
pub(super) const DEFAULT_QCONNECT_CHANNEL_COUNT: i32 = 2;

/// Cross-submodule accumulator: caches the cloud's renderer/queue
/// snapshots, the most recent materialization, the topology of all
/// renderers in the session, and the load-attempt dedup window. Mutated
/// by event_sink (on inbound events), corebridge (on materialize/apply),
/// track_loading (on load attempt), and service (on outbound report
/// completions).
#[derive(Debug, Default)]
pub(super) struct QconnectRemoteSyncState {
    pub(super) last_renderer_queue_item_id: Option<u64>,
    pub(super) last_renderer_next_queue_item_id: Option<u64>,
    pub(super) last_renderer_track_id: Option<u64>,
    pub(super) last_renderer_next_track_id: Option<u64>,
    pub(super) last_renderer_playing_state: Option<i32>,
    pub(super) last_materialized_start_index: Option<usize>,
    pub(super) last_materialized_core_shuffle_order: Option<Vec<usize>>,
    pub(super) last_reported_file_audio_quality: Option<QconnectFileAudioQualitySnapshot>,
    pub(super) last_applied_queue_state: Option<QConnectQueueState>,
    pub(super) last_remote_queue_state: Option<QConnectQueueState>,
    pub(super) session_loop_mode: Option<i32>,
    /// Session topology — stored from session management events (types 81-87).
    pub(super) session: QconnectSessionState,
    pub(super) session_renderer_states: HashMap<i32, QconnectSessionRendererState>,
    /// Track of the most recent load attempt across paths (V2 play
    /// handoff and ensure_remote_track_loaded). Used to suppress
    /// redundant reloads when an echo SetState arrives during the
    /// in-progress buffer/decode window of a previously triggered load.
    pub(super) last_load_attempt: Option<(u64, std::time::Instant)>,
}

pub(super) fn qconnect_now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) fn qconnect_repeat_mode_from_loop_mode(loop_mode: i32) -> Option<RepeatMode> {
    // QConnect protocol loop mode values: 1 = off, 2 = repeat one, 3 = repeat all.
    match loop_mode {
        0 | 1 => Some(RepeatMode::Off),
        2 => Some(RepeatMode::One),
        3 => Some(RepeatMode::All),
        _ => None,
    }
}

pub(super) fn normalize_volume_to_fraction(volume: i32) -> f32 {
    volume.clamp(0, 100) as f32 / 100.0
}

pub(super) fn model_track_to_core_queue_track(track: &Track) -> QueueTrack {
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|album| album.image.best().cloned());
    let artist = track
        .performer
        .as_ref()
        .map(|performer| performer.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album = track
        .album
        .as_ref()
        .map(|album| album.title.clone())
        .unwrap_or_else(|| "Unknown Album".to_string());
    let album_id = track.album.as_ref().and_then(|album| {
        let trimmed = album.id.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let artist_id = track.performer.as_ref().map(|performer| performer.id);

    QueueTrack {
        id: track.id,
        title: track.title.clone(),
        version: track.version.clone(),
        artist,
        album,
        duration_secs: track.duration as u64,
        artwork_url,
        hires: track.hires,
        bit_depth: track.maximum_bit_depth,
        sample_rate: track.maximum_sampling_rate,
        is_local: false,
        album_id: album_id.clone(),
        artist_id,
        streamable: track.streamable,
        source: Some(QCONNECT_REMOTE_QUEUE_SOURCE.to_string()),
        parental_warning: track.parental_warning,
        source_item_id_hint: album_id,
    }
}

#[cfg(test)]
mod tests;
