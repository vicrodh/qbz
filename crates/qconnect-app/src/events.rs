use async_trait::async_trait;
use qconnect_core::{QConnectQueueState, QConnectRendererState, RendererCommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QconnectAppEvent {
    TransportConnected,
    TransportDisconnected,
    QueueUpdated(QConnectQueueState),
    RendererUpdated(QConnectRendererState),
    RendererCommandApplied {
        command: RendererCommand,
        state: QConnectRendererState,
    },
    PendingActionStarted {
        uuid: String,
    },
    PendingActionCompleted {
        uuid: String,
    },
    PendingActionTimedOut {
        uuid: String,
        timeout_ms: u64,
    },
    PendingActionCanceledByConcurrentRemoteEvent {
        pending_uuid: String,
        remote_action_uuid: String,
    },
    QueueErrorIgnoredByConcurrency {
        action_uuid: String,
    },
    QueueResyncTriggered,
    /// Session management event from server (types 81-87, 97-101).
    /// These don't affect the queue reducer but provide session topology info.
    SessionManagementEvent {
        message_type: String,
        payload: Value,
    },
}

#[async_trait]
pub trait QconnectEventSink: Send + Sync {
    async fn on_event(&self, event: QconnectAppEvent);
}

#[derive(Debug, Clone, Default)]
pub struct NoOpEventSink;

#[async_trait]
impl QconnectEventSink for NoOpEventSink {
    async fn on_event(&self, _event: QconnectAppEvent) {}
}
