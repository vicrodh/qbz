//! QBZ Player - Playback engine and queue management
//!
//! This crate provides:
//! - QueueManager: Track queue management with shuffle/repeat
//! - Player: Main playback control (TODO: Phase 3b)
//! - StreamingSource: HTTP audio streaming (TODO: Phase 3b)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     qbz-player (Tier 2)                     │
//! │  Queue management, playback engine, streaming              │
//! └─────────────────────────────────────────────────────────────┘
//!                              ↑
//!              ┌───────────────┼───────────────┐
//!              │               │               │
//!         ┌────┴────┐    ┌─────┴─────┐   ┌─────┴─────┐
//!         │qbz-audio│    │qbz-models │   │qbz-qobuz  │
//!         │ Tier 1  │    │  Tier 0   │   │  Tier 1   │
//!         └─────────┘    └───────────┘   └───────────┘
//! ```
//!
//! # Usage
//!
//! ```rust
//! use qbz_player::QueueManager;
//! use qbz_models::QueueTrack;
//!
//! let queue = QueueManager::new();
//! // queue.add_track(track);
//! // queue.next();
//! ```

pub mod queue;

// Re-export main types
pub use queue::QueueManager;

// TODO: Phase 3b - Extract player module
// The player module has complex dependencies on:
// - QobuzClient (for streaming URLs)
// - Audio backend (for playback)
// - Config system (for audio settings)
// - Visualizer
//
// These need to be abstracted via traits before extraction.
// For now, the player remains in qbz-nix/src-tauri.
