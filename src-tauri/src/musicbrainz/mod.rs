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
    AlbumAppearance, ArtistFullResponse, ArtistRelationships, ArtistType, MatchConfidence, Medium,
    MediumTrack, MusicianAppearances, MusicianConfidence, Period, RelatedArtist, Relation,
    ReleaseFullResponse, ReleaseSearchResponse, ResolvedArtist, ResolvedMusician, ResolvedRelease,
    ResolvedTrack, Tag,
};

use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state wrapper for Tauri
pub struct MusicBrainzSharedState {
    pub client: Arc<MusicBrainzClient>,
    pub cache: Arc<Mutex<Option<MusicBrainzCache>>>,
}

impl MusicBrainzSharedState {
    pub fn new() -> Result<Self, String> {
        let cache_state = MusicBrainzCacheState::new()?;
        Ok(Self {
            client: Arc::new(MusicBrainzClient::new()),
            cache: cache_state.cache,
        })
    }

    pub fn new_empty() -> Self {
        Self {
            client: Arc::new(MusicBrainzClient::new()),
            cache: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn init_at(&self, base_dir: &Path) -> Result<(), String> {
        let cache_dir = base_dir.join("cache");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
        let db_path = cache_dir.join("musicbrainz_cache.db");
        let new_cache = MusicBrainzCache::new(&db_path)?;
        log::info!("MusicBrainz cache initialized at {:?}", db_path);
        let mut guard = self.cache.lock().await;
        *guard = Some(new_cache);
        Ok(())
    }

    pub async fn teardown(&self) {
        let mut guard = self.cache.lock().await;
        *guard = None;
    }
}
