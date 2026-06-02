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
// QconnectSessionRendererState is referenced via `super::…` only by the test
// module now (the QconnectRemoteSyncState struct that consumed it in non-test
// code moved to qconnect-app), so gate the re-export to test builds to avoid an
// unused-import warning.
#[cfg(test)]
pub(super) use session::QconnectSessionRendererState;
pub use types::*;

/// The renderer-side pure mappers now live in the frontend-agnostic
/// `qconnect_app::renderer` module (slice 6). Re-exported here so existing
/// `super::…` references inside this module compile unchanged.
pub(super) use qconnect_app::renderer::{
    model_track_to_core_queue_track, normalize_volume_to_fraction,
    qconnect_repeat_mode_from_loop_mode,
};

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

/// The cross-submodule remote-sync accumulator now lives in the frontend-agnostic
/// `qconnect_app::sync_state` module (slice 2+4) so both the Tauri and Slint
/// adapters share one struct under one lock. Re-exported here so existing
/// `super::QconnectRemoteSyncState` references compile unchanged.
pub(super) use qconnect_app::QconnectRemoteSyncState;

pub(super) fn qconnect_now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
