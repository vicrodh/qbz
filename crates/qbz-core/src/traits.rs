//! Frontend adapter trait for QBZ Core
//!
//! Each frontend (Slint, Tauri, Headless) implements this trait
//! to receive events from the core and update their UI accordingly.

use async_trait::async_trait;
use crate::events::CoreEvent;

/// Trait that each frontend must implement to receive events from the core.
///
/// # Implementations
///
/// - `SlintAdapter`: Updates Slint UI properties via `invoke_from_event_loop`
/// - `TauriAdapter`: Emits Tauri events to JS frontend
/// - `HeadlessAdapter`: Updates MPRIS, broadcasts to REST/WebSocket clients
///
/// # Example
///
/// ```rust,ignore
/// use qbz_core::{FrontendAdapter, CoreEvent};
///
/// struct MyAdapter {
///     // UI handle, channels, etc.
/// }
///
/// #[async_trait]
/// impl FrontendAdapter for MyAdapter {
///     async fn on_event(&self, event: CoreEvent) {
///         match event {
///             CoreEvent::TrackChanged(track) => {
///                 // Update UI with new track info
///             }
///             CoreEvent::PlaybackStateChanged(state) => {
///                 // Update play/pause button, progress bar
///             }
///             _ => {}
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait FrontendAdapter: Send + Sync + 'static {
    /// Called when an event occurs in the core.
    ///
    /// Implementations should update their UI accordingly.
    /// This method is called from async context, so UI updates
    /// may need to be dispatched to the UI thread.
    async fn on_event(&self, event: CoreEvent);

    /// Request confirmation from the user.
    ///
    /// Used for destructive actions like clearing queue, logging out, etc.
    /// Returns `true` if confirmed, `false` if cancelled.
    ///
    /// Default implementation auto-confirms (for headless mode).
    async fn request_confirmation(&self, _title: &str, _message: &str) -> bool {
        true
    }

    /// Show a notification to the user.
    ///
    /// Used for track changes, errors, etc.
    /// Default implementation is a no-op.
    async fn show_notification(&self, _title: &str, _body: &str) {}

    /// Request the frontend to exit/close.
    ///
    /// Called when the core needs to shut down the app.
    /// Default implementation is a no-op.
    async fn request_exit(&self) {}
}

/// Null adapter for testing - does nothing with events.
///
/// Useful for unit tests that don't need UI updates.
pub struct NullAdapter;

#[async_trait]
impl FrontendAdapter for NullAdapter {
    async fn on_event(&self, _event: CoreEvent) {
        // Intentionally empty
    }
}

/// Logging adapter for debugging - logs all events.
///
/// Useful during development to see what events are being emitted.
pub struct LoggingAdapter;

#[async_trait]
impl FrontendAdapter for LoggingAdapter {
    async fn on_event(&self, event: CoreEvent) {
        log::debug!("CoreEvent: {:?}", event);
    }

    async fn request_confirmation(&self, title: &str, message: &str) -> bool {
        log::info!("Confirmation requested: {} - {}", title, message);
        true
    }

    async fn show_notification(&self, title: &str, body: &str) {
        log::info!("Notification: {} - {}", title, body);
    }
}
