//! QBZ Core - Orchestrator for QBZ music player
//!
//! This crate provides the main entry point for QBZ:
//! - QbzCore<A: FrontendAdapter>: Main orchestrator struct
//! - Connects all subsystems (audio, player, queue, API)
//! - Provides unified public API for frontends
//!
//! # Usage
//!
//! ```rust,ignore
//! use qbz_core::{QbzCore, FrontendAdapter, CoreEvent};
//!
//! struct MyAdapter;
//!
//! impl FrontendAdapter for MyAdapter {
//!     async fn on_event(&self, event: CoreEvent) {
//!         // Handle event, update UI
//!     }
//! }
//!
//! let core = QbzCore::new(MyAdapter);
//! core.init().await?;
//! core.login("email", "password").await?;
//! core.play_track(track_id).await?;
//! ```

// TODO: Phase 5 - Implement core orchestrator
// pub mod core;
// pub mod error;

// Re-exports
// pub use qbz_models::*;
// pub use core::QbzCore;
// pub use error::CoreError;
