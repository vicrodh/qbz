//! QBZ Integrations
//!
//! Third-party service integrations for QBZ music player.
//! This crate is designed to work WITHOUT Tauri - it can be used by
//! any Rust frontend (Tauri, TUI, CLI, etc.)
//!
//! ## Features
//!
//! - `cache` (default): Enable SQLite caching for offline support
//!
//! ## Modules
//!
//! - `lastfm`: Last.fm scrobbling and now-playing
//! - `listenbrainz`: ListenBrainz scrobbling with MBID enrichment
//! - `musicbrainz`: MusicBrainz entity resolution and metadata enrichment
//!
//! ## Architecture
//!
//! Each integration follows the same pattern:
//! - `client.rs`: HTTP client for the service API
//! - `models.rs`: Request/response data types
//! - `cache.rs`: SQLite persistence (requires `cache` feature)
//!
//! The crate exposes async APIs that can be called from any async runtime.

pub mod error;
pub mod lastfm;
pub mod listenbrainz;
pub mod musicbrainz;

pub use error::{IntegrationError, IntegrationResult};

// Re-export main types for convenience
pub use lastfm::{LastFmClient, LastFmSession};
pub use listenbrainz::{ListenBrainzClient, ListenBrainzConfig, ListenType};
pub use musicbrainz::{MusicBrainzClient, MusicBrainzConfig};
