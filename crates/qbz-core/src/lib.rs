//! QBZ Core - Frontend-agnostic music player library
//!
//! This crate provides the core functionality for QBZ:
//! - Qobuz API client
//! - Audio playback (via rodio/cpal)
//! - Queue management
//! - Audio caching
//!
//! ## Architecture
//!
//! The core is designed to be used by multiple frontends:
//! - `qbz-slint`: Slint UI (this POC)
//! - `qbz-nix`: Tauri + Svelte (current production app)
//! - `qbz-daemon`: Headless daemon with REST API (future)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use qbz_core::{QbzCore, FrontendAdapter, CoreEvent};
//!
//! struct MyAdapter;
//!
//! #[async_trait::async_trait]
//! impl FrontendAdapter for MyAdapter {
//!     async fn on_event(&self, event: CoreEvent) {
//!         // Handle event - update UI, etc.
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let core = QbzCore::new(MyAdapter);
//!     core.init().await.unwrap();
//!     core.login("email", "password").await.unwrap();
//! }
//! ```

pub mod events;
pub mod traits;
pub mod types;

// These will be copied from src-tauri
// pub mod api;
// pub mod audio;
// pub mod player;
// pub mod queue;
// pub mod cache;

// Re-exports
pub use events::CoreEvent;
pub use traits::{FrontendAdapter, NullAdapter};
pub use types::*;

/// Error type for QbzCore operations
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("API error: {0}")]
    Api(String),

    #[error("Audio error: {0}")]
    Audio(String),

    #[error("Not logged in")]
    NotLoggedIn,

    #[error("Track not found: {0}")]
    TrackNotFound(u64),

    #[error("Network error: {0}")]
    Network(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Placeholder for QbzCore - will be fully implemented after copying modules
pub struct QbzCore<A: FrontendAdapter> {
    adapter: std::sync::Arc<A>,
}

impl<A: FrontendAdapter> QbzCore<A> {
    pub fn new(adapter: A) -> Self {
        Self {
            adapter: std::sync::Arc::new(adapter),
        }
    }

    /// Initialize the Qobuz API client
    pub async fn init(&self) -> Result<(), CoreError> {
        log::info!("QbzCore::init() - placeholder");
        Ok(())
    }

    /// Check if user is logged in
    pub async fn is_logged_in(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_core_creation() {
        let core = QbzCore::new(NullAdapter);
        assert!(!core.is_logged_in().await);
    }
}
