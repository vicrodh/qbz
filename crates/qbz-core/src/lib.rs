//! QBZ Core - Orchestrator for QBZ music player
//!
//! This crate provides the main entry point for QBZ:
//! - [`QbzCore<A: FrontendAdapter>`]: Main orchestrator struct
//! - Connects all subsystems (audio, player, queue, API)
//! - Provides unified public API for frontends
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      qbz-core (Tier 3)                      │
//! │         Main orchestrator, entry point for frontends        │
//! └─────────────────────────────────────────────────────────────┘
//!                              ↑
//!          ┌───────────────────┼───────────────────┐
//!          ↓                   ↓                   ↓
//! ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
//! │   qbz-player    │ │   qbz-audio     │ │   qbz-qobuz     │
//! │    (Tier 2)     │ │    (Tier 1)     │ │    (Tier 2)     │
//! └─────────────────┘ └─────────────────┘ └─────────────────┘
//!          ↓                   ↓                   ↓
//!                      ┌───────────────┐
//!                      │  qbz-models   │
//!                      │   (Tier 0)    │
//!                      └───────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use qbz_core::{QbzCore, CoreError};
//! use qbz_models::{FrontendAdapter, CoreEvent};
//!
//! struct MyAdapter;
//!
//! #[async_trait::async_trait]
//! impl FrontendAdapter for MyAdapter {
//!     async fn on_event(&self, event: CoreEvent) {
//!         // Handle event, update UI
//!         println!("Event: {:?}", event);
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), CoreError> {
//!     let core = QbzCore::new(MyAdapter);
//!     core.init().await?;
//!     core.login("email", "password").await?;
//!     // ... use core
//!     Ok(())
//! }
//! ```

pub mod core;
pub mod error;

// Re-exports from qbz-models for convenience
pub use qbz_models::{CoreEvent, FrontendAdapter, NoOpAdapter, LoggingAdapter};

// Re-exports from this crate
pub use core::QbzCore;
pub use error::CoreError;
