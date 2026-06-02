//! Cross-frontend QConnect remote-sync accumulator.
//!
//! Caches the cloud's renderer/queue snapshots, the most recent
//! materialization, the session topology, the per-renderer cached state, the
//! load-attempt dedup window, and the renderer-liveness watchdog epoch.
//!
//! This is held behind a SINGLE Mutex shared by the session/liveness path and
//! the renderer-materialize path. That single-lock sharing is load-bearing:
//! `capture_session_state_takeover_input` reads `session` together with
//! `last_renderer_playing_state`, and the materialize path reads `last_renderer_*`
//! together with `last_applied_queue_state`, all atomically. Splitting these
//! across two locks would tear the takeover decision. So it stays ONE struct
//! under ONE lock. Relocated here (slice 2+4) so both the Tauri and Slint
//! adapters share it; the owning `Mutex` is held by the adapter / `QconnectApp`.
//!
//! Mutated by the event sink (on inbound events), the renderer engine (on
//! materialize/apply), track loading (on load attempt), and the service loop
//! (on outbound report completions).

use std::collections::HashMap;
use std::time::Instant;

use qconnect_core::QConnectQueueState;

use crate::session::{
    QconnectFileAudioQualitySnapshot, QconnectSessionRendererState, QconnectSessionState,
};

#[derive(Debug, Default)]
pub struct QconnectRemoteSyncState {
    pub last_renderer_queue_item_id: Option<u64>,
    pub last_renderer_next_queue_item_id: Option<u64>,
    pub last_renderer_track_id: Option<u64>,
    pub last_renderer_next_track_id: Option<u64>,
    pub last_renderer_playing_state: Option<i32>,
    pub last_materialized_start_index: Option<usize>,
    pub last_materialized_core_shuffle_order: Option<Vec<usize>>,
    pub last_reported_file_audio_quality: Option<QconnectFileAudioQualitySnapshot>,
    /// Last reported device (DAC output) audio quality: (sampling_rate, bit_depth, nb_channels).
    /// Used to dedup outbound RndrSrvrDeviceAudioQualityChanged(27) reports.
    pub last_reported_device_audio_quality: Option<(i32, i32, i32)>,
    pub last_applied_queue_state: Option<QConnectQueueState>,
    pub last_remote_queue_state: Option<QConnectQueueState>,
    pub session_loop_mode: Option<i32>,
    /// Session topology — stored from session management events (types 81-87).
    pub session: QconnectSessionState,
    /// The session_uuid for which we last ran the full deferred renderer-join
    /// body. Used to make the deferred join idempotent (P1-8): when a SESSION_STATE
    /// arrives with the same session_uuid we skip the join reports but still
    /// re-AskForRendererState.
    pub last_joined_session_uuid: Option<String>,
    pub session_renderer_states: HashMap<i32, QconnectSessionRendererState>,
    /// Track of the most recent load attempt across paths (V2 play handoff and
    /// ensure_remote_track_loaded). Used to suppress redundant reloads when an
    /// echo SetState arrives during the in-progress buffer/decode window of a
    /// previously triggered load.
    pub last_load_attempt: Option<(u64, Instant)>,
    /// Monotonic epoch for the renderer-liveness watchdog (P0-1). Every armed
    /// RENDERER_STATE_UPDATED bumps this; a spawned 12s task captures the value
    /// and no-ops on wake if it was superseded (reset/disarm). Disarm =
    /// pause/stop/active-change/disconnect, which also bump it.
    pub watchdog_generation: u64,
}

/// Set each cached renderer's `active` flag to match the session's current
/// active renderer id. Pure mutation over the relocated accumulator; relocated
/// from the Tauri adapter (slice 2+4) so the shared session-apply logic in
/// `app.rs` and the Tauri adapter both call one definition.
pub fn sync_session_renderer_active_flags(state: &mut QconnectRemoteSyncState) {
    for (renderer_id, renderer_state) in &mut state.session_renderer_states {
        renderer_state.active = state
            .session
            .active_renderer_id
            .map(|active_renderer_id| active_renderer_id == *renderer_id);
    }
}

/// Get-or-insert the cached per-renderer state for `renderer_id`, seeding its
/// `active` flag from the session's current active renderer. Pure mutation;
/// relocated from the Tauri adapter (slice 2+4). Byte-identical behavior.
pub fn ensure_session_renderer_state(
    state: &mut QconnectRemoteSyncState,
    renderer_id: i32,
) -> &mut QconnectSessionRendererState {
    let active = state
        .session
        .active_renderer_id
        .map(|active_renderer_id| active_renderer_id == renderer_id);
    state
        .session_renderer_states
        .entry(renderer_id)
        .or_insert_with(|| QconnectSessionRendererState {
            active,
            ..Default::default()
        })
}
