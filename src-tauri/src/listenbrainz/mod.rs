//! ListenBrainz integration module
//!
//! Provides scrobbling and now-playing notifications to ListenBrainz.
//! Uses personal user tokens (not OAuth) for authentication.
//!
//! ## Architecture
//!
//! - `client.rs`: HTTP client for ListenBrainz API
//! - `models.rs`: Submission payload types
//! - `cache.rs`: SQLite-based offline queue and token persistence
//!
//! ## Usage
//!
//! The module is accessed via Tauri commands in `commands/listenbrainz.rs`.
//!
//! ## Scrobbling Rules
//!
//! - Now Playing: Submitted when track starts
//! - Scrobble: Submitted after 50% of track OR 4 minutes played
//! - Skip: No submission if < 30 seconds played
//!
//! ## MusicBrainz Integration
//!
//! Uses Stage 1 MusicBrainz data to enrich scrobbles with:
//! - Recording MBID
//! - Release MBID
//! - Artist MBIDs
//! - ISRC codes

pub mod cache;
pub mod client;
pub mod models;

pub use cache::{ListenBrainzCache, ListenBrainzCacheState, QueueStats};
pub use client::{ListenBrainzClient, ListenBrainzConfig};
pub use models::{
    AdditionalInfo, Listen, ListenBrainzStatus, ListenType, QueuedListen, SubmitListensPayload,
    TrackMetadata, UserInfo,
};

use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state wrapper for Tauri
pub struct ListenBrainzSharedState {
    pub client: Arc<Mutex<ListenBrainzClient>>,
    pub cache: Arc<Mutex<Option<ListenBrainzCache>>>,
}

impl ListenBrainzSharedState {
    pub fn new() -> Result<Self, String> {
        let cache_state = ListenBrainzCacheState::new()?;

        // Check for saved credentials and enabled state
        let (token, user_name, enabled) = {
            let cache_guard = cache_state.cache.blocking_lock();
            if let Some(cache) = cache_guard.as_ref() {
                let (token, user_name) = cache.get_credentials().unwrap_or((None, None));
                let enabled = cache.is_enabled().unwrap_or(true);
                (token, user_name, enabled)
            } else {
                (None, None, true)
            }
        };

        // Create client with saved config
        let config = ListenBrainzConfig {
            enabled,
            token: token.clone(),
            user_name: user_name.clone(),
        };
        let client = ListenBrainzClient::with_config(config);

        if token.is_some() && user_name.is_some() {
            log::info!("ListenBrainz: restored session from persisted credentials");
        }

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            cache: cache_state.cache,
        })
    }

    pub fn new_empty() -> Self {
        let client = ListenBrainzClient::with_config(ListenBrainzConfig {
            enabled: true,
            token: None,
            user_name: None,
        });
        Self {
            client: Arc::new(Mutex::new(client)),
            cache: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn init_at(&self, base_dir: &Path) -> Result<(), String> {
        let cache_dir = base_dir.join("cache");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
        let db_path = cache_dir.join("listenbrainz_cache.db");
        let new_cache = ListenBrainzCache::new(&db_path)?;
        log::info!("ListenBrainz cache initialized at {:?}", db_path);

        // Restore credentials and enabled state from new cache
        let (token, user_name) = new_cache.get_credentials().unwrap_or((None, None));
        let enabled = new_cache.is_enabled().unwrap_or(true);

        let mut guard = self.cache.lock().await;
        *guard = Some(new_cache);
        drop(guard);

        // Restore enabled state and credentials to in-memory client
        {
            let client = self.client.lock().await;
            client.set_enabled(enabled).await;
            if let (Some(tok), Some(uname)) = (token, user_name) {
                log::info!("ListenBrainz: restoring persisted session");
                client.restore_token(tok, uname).await;
            }
        }

        Ok(())
    }

    pub async fn teardown(&self) {
        // Clear in-memory token to prevent cross-user token leaks
        self.client.lock().await.disconnect().await;
        // Close cache DB connection
        let mut guard = self.cache.lock().await;
        *guard = None;
    }
}
