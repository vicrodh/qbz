//! qconnect-app
//!
//! Application adapter that composes qconnect core + protocol + transport.

mod app;
mod error;
mod events;
mod feature_flags;
pub mod queue_resolution;
pub mod renderer;
mod renderer_engine;
pub mod session;
mod state;
pub mod startup;
mod sync_state;

pub use app::{
    queue_payload_track_preview, QconnectApp, SessionApplyOutcome, SessionLoopHost,
    SessionStateTakeoverInput,
};
pub use startup::{compute_effective_startup, QconnectStartupMode};
pub use error::QconnectAppError;
pub use events::{NoOpEventSink, QconnectAppEvent, QconnectEventSink};
pub use session::{
    compute_connection_state, deferred_join_reason, find_unique_renderer_id,
    is_local_renderer_active, is_peer_renderer_active, normalize_active_renderer_id,
    quality_from_max_audio_quality, refresh_local_renderer_id, renderer_allows_remote_volume,
    should_arm_renderer_watchdog, should_reask_queue_state, ConnectionDecision, LocalIdentity,
    QconnectFileAudioQualitySnapshot, QconnectLifecycleState, QconnectRendererInfo,
    QconnectSessionRendererState, QconnectSessionState, RendererStatus, ServerActiveState,
    JOIN_SESSION_REASON_CONTROLLER_REQUEST, JOIN_SESSION_REASON_RECONNECTION,
    QCONNECT_RENDERER_LOST_TIMEOUT_MS,
};
pub use feature_flags::{
    QBZ_QCONNECT_PANEL_SWITCH, QBZ_QCONNECT_QUEUE_MODEL, QBZ_QCONNECT_STRICT_DOMAIN_ISOLATION,
    QBZ_QCONNECT_TRANSPORT,
};
pub use qconnect_core::{
    evaluate_remote_queue_admission, resolve_handoff_intent, validate_track_origins_for_admission,
    AdmissionDecision, HandoffIntent, QConnectQueueState, QConnectRendererState, QueueVersion,
    RendererCommand, TrackOrigin,
};
pub use qconnect_protocol::{QueueCommandType, RendererReport, RendererReportType};
pub use renderer_engine::QconnectRendererEngine;
pub use state::QconnectRuntimeState;
pub use sync_state::{
    ensure_session_renderer_state, sync_session_renderer_active_flags, QconnectRemoteSyncState,
};
