use async_trait::async_trait;
use qbz_models::FrontendAdapter;
use tokio::sync::broadcast;

/// Events distributed by the daemon to all connected clients.
#[derive(Debug, Clone)]
pub enum DaemonEvent {
    /// Events from qbz-core (track started, queue updated, favorites changed, etc.)
    Core(qbz_models::CoreEvent),
    /// Periodic playback state snapshot (250ms while playing)
    Playback(PlaybackSnapshot),
    /// Runtime lifecycle (login, logout, ready, degraded)
    Runtime(RuntimeEvent),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackSnapshot {
    pub state: String,
    pub track_id: u64,
    pub position_secs: u64,
    pub duration_secs: u64,
    pub volume: f32,
    pub sample_rate: u32,
    pub bit_depth: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum RuntimeEvent {
    Ready { user_id: u64 },
    LoggedOut,
    Degraded { reason: String },
}

/// FrontendAdapter implementation for the headless daemon.
/// Routes core events to the broadcast channel for SSE distribution.
pub struct DaemonAdapter {
    event_tx: broadcast::Sender<DaemonEvent>,
}

impl DaemonAdapter {
    pub fn new(event_tx: broadcast::Sender<DaemonEvent>) -> Self {
        Self { event_tx }
    }
}

#[async_trait]
impl FrontendAdapter for DaemonAdapter {
    async fn on_event(&self, event: qbz_models::CoreEvent) {
        let _ = self.event_tx.send(DaemonEvent::Core(event));
    }
}
