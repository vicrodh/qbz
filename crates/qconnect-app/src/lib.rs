//! qconnect-app
//!
//! Application adapter that composes qconnect core + protocol + transport.

mod app;
mod error;
mod events;
mod feature_flags;
mod state;
pub mod startup;

pub use app::QconnectApp;
pub use startup::{compute_effective_startup, QconnectStartupMode};
pub use error::QconnectAppError;
pub use events::{NoOpEventSink, QconnectAppEvent, QconnectEventSink};
pub use feature_flags::{
    QBZ_QCONNECT_PANEL_SWITCH, QBZ_QCONNECT_QUEUE_MODEL, QBZ_QCONNECT_STRICT_DOMAIN_ISOLATION,
    QBZ_QCONNECT_TRANSPORT,
};
pub use qconnect_core::{
    evaluate_remote_queue_admission, resolve_handoff_intent, AdmissionDecision, HandoffIntent,
    QConnectQueueState, QConnectRendererState, QueueVersion, RendererCommand, TrackOrigin,
};
pub use qconnect_protocol::{QueueCommandType, RendererReport, RendererReportType};
pub use state::QconnectRuntimeState;
