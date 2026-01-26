//! MusicBrainz integration module
//!
//! Provides entity resolution and caching for MusicBrainz lookups.
//! This module operates as a background semantic engine - it enriches
//! data without affecting the UI directly (Stage 1).
//!
//! ## Architecture
//!
//! - `client.rs`: HTTP client with rate limiting (1 req/sec)
//! - `models.rs`: API response types and resolved entity types
//! - `cache.rs`: SQLite-based cache with TTL expiration
//!
//! ## Usage
//!
//! The module is accessed via Tauri commands in `commands/musicbrainz.rs`.

pub mod cache;
pub mod client;
pub mod models;
pub mod smart_playlists;

pub use cache::{CacheStats, MusicBrainzCache, MusicBrainzCacheState};
pub use client::{MusicBrainzClient, MusicBrainzConfig};
pub use models::{
    AlbumAppearance, ArtistRelationships, ArtistType, MatchConfidence, MusicianAppearances,
    MusicianConfidence, Period, RelatedArtist, ResolvedArtist, ResolvedMusician, ResolvedRelease,
    ResolvedTrack,
};

use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state wrapper for Tauri
pub struct MusicBrainzSharedState {
    pub client: Arc<MusicBrainzClient>,
    pub cache: Arc<Mutex<MusicBrainzCache>>,
}

impl MusicBrainzSharedState {
    pub fn new() -> Result<Self, String> {
        let cache_state = MusicBrainzCacheState::new()?;
        Ok(Self {
            client: Arc::new(MusicBrainzClient::new()),
            cache: cache_state.cache,
        })
    }
}
