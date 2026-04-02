//! TUI FrontendAdapter — forwards CoreEvents to the main TUI loop via a channel.

use async_trait::async_trait;
use qbz_models::{CoreEvent, FrontendAdapter};
use tokio::sync::mpsc;

/// Adapter that bridges qbz-core events to the TUI event loop.
///
/// Implements [`FrontendAdapter`] by forwarding every [`CoreEvent`] to an
/// unbounded tokio mpsc channel. The receiving end of the channel lives in
/// the main TUI loop ([`crate::app::App`]), which reads events and updates the
/// terminal UI accordingly.
pub struct TuiAdapter {
    event_tx: mpsc::UnboundedSender<CoreEvent>,
}

impl TuiAdapter {
    /// Create a new `TuiAdapter` with the given sender half of an unbounded channel.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    /// let adapter = TuiAdapter::new(tx);
    /// ```
    pub fn new(event_tx: mpsc::UnboundedSender<CoreEvent>) -> Self {
        Self { event_tx }
    }
}

#[async_trait]
impl FrontendAdapter for TuiAdapter {
    async fn on_event(&self, event: CoreEvent) {
        // Ignore send errors: they only occur when the receiver is dropped,
        // which means the TUI loop has already exited.
        let _ = self.event_tx.send(event);
    }

    async fn on_ready(&self) {
        log::info!("[TUI] Core is ready");
    }

    async fn on_shutdown(&self) {
        log::info!("[TUI] Core is shutting down");
    }
}
