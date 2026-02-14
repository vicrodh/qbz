//! Tauri Frontend Adapter
//!
//! Implements FrontendAdapter for Tauri, emitting events to the frontend via Tauri's event system.

use async_trait::async_trait;
use qbz_models::{CoreEvent, FrontendAdapter};
use tauri::{AppHandle, Emitter};

/// Tauri implementation of FrontendAdapter
///
/// Emits CoreEvents to the Svelte frontend via Tauri's event system.
#[derive(Clone)]
pub struct TauriAdapter {
    app_handle: AppHandle,
}

impl TauriAdapter {
    /// Create a new TauriAdapter with the given AppHandle
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl FrontendAdapter for TauriAdapter {
    /// Emit a CoreEvent to the frontend
    ///
    /// Events are serialized to JSON and sent via Tauri's event system.
    /// The frontend listens on "core-event" channel.
    async fn on_event(&self, event: CoreEvent) {
        // Emit to frontend via Tauri event system
        if let Err(e) = self.app_handle.emit("core-event", &event) {
            log::error!("Failed to emit core event: {}", e);
        }

        // Also log for debugging
        log::trace!("CoreEvent emitted: {:?}", event);
    }

    async fn on_ready(&self) {
        log::info!("QBZ Core is ready");
        // Could emit a ready event to frontend if needed
    }

    async fn on_shutdown(&self) {
        log::info!("QBZ Core is shutting down");
    }
}
