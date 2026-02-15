//! Tauri Frontend Adapter
//!
//! Implements FrontendAdapter for Tauri, emitting events to the frontend via Tauri's event system.
//!
//! MIGRATION NOTE: During the V2 migration, this adapter emits BOTH:
//! - "core-event" (new unified event channel for V2 architecture)
//! - Legacy event names ("playback:state", etc.) for backwards compatibility
//!
//! Once frontend is fully migrated to listen on "core-event", legacy emissions can be removed.

use async_trait::async_trait;
use qbz_models::{CoreEvent, FrontendAdapter};
use tauri::{AppHandle, Emitter};

/// Tauri implementation of FrontendAdapter
///
/// Emits CoreEvents to the Svelte frontend via Tauri's event system.
/// During migration, emits both new and legacy event formats.
#[derive(Clone)]
pub struct TauriAdapter {
    app_handle: AppHandle,
}

impl TauriAdapter {
    /// Create a new TauriAdapter with the given AppHandle
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }

    /// Emit legacy event for backwards compatibility during migration
    fn emit_legacy_event(&self, event: &CoreEvent) {
        // Map CoreEvent variants to legacy event names
        match event {
            CoreEvent::PlaybackStateChanged { state: _ } => {
                // Legacy "playback:state" expects PlaybackEvent format
                // The full legacy polling loop in lib.rs handles this event
                // Once we migrate the polling loop to use CoreEvent, we can emit here
                log::trace!("[Legacy] PlaybackStateChanged -> handled by polling loop");
            }
            CoreEvent::QueueUpdated { state } => {
                if let Err(e) = self.app_handle.emit("queue:updated", state) {
                    log::error!("Failed to emit legacy queue:updated: {}", e);
                }
            }
            CoreEvent::ShuffleChanged { enabled } => {
                if let Err(e) = self.app_handle.emit("queue:shuffle-changed", enabled) {
                    log::error!("Failed to emit legacy queue:shuffle-changed: {}", e);
                }
            }
            CoreEvent::RepeatModeChanged { mode } => {
                let mode_str = match mode {
                    qbz_models::RepeatMode::Off => "off",
                    qbz_models::RepeatMode::All => "all",
                    qbz_models::RepeatMode::One => "one",
                };
                if let Err(e) = self.app_handle.emit("queue:repeat-changed", mode_str) {
                    log::error!("Failed to emit legacy queue:repeat-changed: {}", e);
                }
            }
            CoreEvent::LoggedIn { session } => {
                if let Err(e) = self.app_handle.emit("auth:logged-in", session) {
                    log::error!("Failed to emit legacy auth:logged-in: {}", e);
                }
            }
            CoreEvent::LoggedOut => {
                if let Err(e) = self.app_handle.emit("auth:logged-out", ()) {
                    log::error!("Failed to emit legacy auth:logged-out: {}", e);
                }
            }
            CoreEvent::Error { code, message, recoverable } => {
                #[derive(serde::Serialize, Clone)]
                struct LegacyError<'a> {
                    code: &'a str,
                    message: &'a str,
                    recoverable: bool,
                }
                if let Err(e) = self.app_handle.emit("error", LegacyError { code, message, recoverable: *recoverable }) {
                    log::error!("Failed to emit legacy error: {}", e);
                }
            }
            // Other events don't have legacy equivalents (or are handled by polling loop)
            _ => {}
        }
    }
}

#[async_trait]
impl FrontendAdapter for TauriAdapter {
    /// Emit a CoreEvent to the frontend
    ///
    /// Events are serialized to JSON and sent via Tauri's event system.
    /// The frontend listens on "core-event" channel.
    ///
    /// MIGRATION: Also emits legacy events for backwards compatibility.
    async fn on_event(&self, event: CoreEvent) {
        // Emit new unified event
        if let Err(e) = self.app_handle.emit("core-event", &event) {
            log::error!("Failed to emit core event: {}", e);
        }

        // Also emit legacy event for backwards compatibility
        self.emit_legacy_event(&event);

        // Debug logging
        log::trace!("CoreEvent emitted: {:?}", event);
    }

    async fn on_ready(&self) {
        log::info!("QBZ Core is ready");
        if let Err(e) = self.app_handle.emit("core:ready", ()) {
            log::error!("Failed to emit core:ready: {}", e);
        }
    }

    async fn on_shutdown(&self) {
        log::info!("QBZ Core is shutting down");
        if let Err(e) = self.app_handle.emit("core:shutdown", ()) {
            log::error!("Failed to emit core:shutdown: {}", e);
        }
    }
}
