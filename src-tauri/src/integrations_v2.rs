//! V2 Integrations State
//!
//! Tauri state wrappers for qbz-integrations crate.
//! These provide the glue between Tauri's state management and the
//! Tauri-independent integration clients.
//!
//! IMPORTANT: These wrappers must NEVER depend on legacy modules.
//! All persistence goes through V2 caches (qbz-integrations).
//! See ADR-004.

use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use qbz_integrations::{
    LastFmClient, ListenBrainzClient, ListenBrainzConfig, MusicBrainzClient, MusicBrainzConfig,
};

use qbz_integrations::listenbrainz::cache::ListenBrainzCache;
use qbz_integrations::musicbrainz::cache::MusicBrainzCache;

/// V2 ListenBrainz state wrapper for Tauri
pub struct ListenBrainzV2State {
    pub client: Arc<Mutex<ListenBrainzClient>>,
    /// V2 SQLite cache for persistence (credentials, settings, queue)
    pub cache: Arc<Mutex<Option<ListenBrainzCache>>>,
    /// Persisted token (loaded from/saved to user session)
    token: Arc<Mutex<Option<String>>>,
    user_name: Arc<Mutex<Option<String>>>,
}

impl ListenBrainzV2State {
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(ListenBrainzClient::new())),
            cache: Arc::new(Mutex::new(None)),
            token: Arc::new(Mutex::new(None)),
            user_name: Arc::new(Mutex::new(None)),
        }
    }

    /// Initialize cache at user data directory
    pub async fn init_cache_at(&self, base_dir: &Path) -> Result<(), String> {
        let cache_dir = base_dir.join("cache");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create LB cache directory: {}", e))?;
        let db_path = cache_dir.join("listenbrainz_v2.db");
        let cache = ListenBrainzCache::new(&db_path)?;
        log::info!("ListenBrainz V2 cache initialized at {:?}", db_path);
        let mut guard = self.cache.lock().await;
        *guard = Some(cache);
        Ok(())
    }

    /// Initialize from V2 cache (reads persisted credentials + enabled state)
    pub async fn init_from_cache(&self) {
        let cache_guard = self.cache.lock().await;
        if let Some(cache) = cache_guard.as_ref() {
            let (token, user_name) = cache.get_credentials().unwrap_or((None, None));
            let enabled = cache.is_enabled().unwrap_or(true);
            drop(cache_guard);

            let config = ListenBrainzConfig {
                enabled,
                token: token.clone(),
                user_name: user_name.clone(),
            };
            *self.client.lock().await = ListenBrainzClient::with_config(config);
            *self.token.lock().await = token;
            *self.user_name.lock().await = user_name;
        }
    }

    /// Initialize with saved credentials (for migration from legacy)
    pub async fn init_with_credentials(
        &self,
        token: Option<String>,
        user_name: Option<String>,
        enabled: bool,
    ) {
        let config = ListenBrainzConfig {
            enabled,
            token: token.clone(),
            user_name: user_name.clone(),
        };
        *self.client.lock().await = ListenBrainzClient::with_config(config);
        *self.token.lock().await = token;
        *self.user_name.lock().await = user_name;
    }

    /// Get current credentials for persistence
    pub async fn get_credentials(&self) -> (Option<String>, Option<String>) {
        (
            self.token.lock().await.clone(),
            self.user_name.lock().await.clone(),
        )
    }

    /// Save credentials after successful auth
    pub async fn save_credentials(&self, token: String, user_name: String) {
        *self.token.lock().await = Some(token.clone());
        *self.user_name.lock().await = Some(user_name.clone());

        // Persist to V2 cache
        let cache_guard = self.cache.lock().await;
        if let Some(cache) = cache_guard.as_ref() {
            if let Err(e) = cache.save_credentials(&token, &user_name) {
                log::warn!("Failed to persist LB credentials to V2 cache: {}", e);
            }
        }
    }

    /// Clear credentials on disconnect
    pub async fn clear_credentials(&self) {
        *self.token.lock().await = None;
        *self.user_name.lock().await = None;
        *self.client.lock().await = ListenBrainzClient::new();

        // Clear from V2 cache
        let cache_guard = self.cache.lock().await;
        if let Some(cache) = cache_guard.as_ref() {
            if let Err(e) = cache.clear_credentials() {
                log::warn!("Failed to clear LB credentials from V2 cache: {}", e);
            }
        }
    }

    /// Teardown cache (on session deactivate)
    pub async fn teardown(&self) {
        let mut guard = self.cache.lock().await;
        *guard = None;
    }
}

impl Default for ListenBrainzV2State {
    fn default() -> Self {
        Self::new()
    }
}

/// V2 MusicBrainz state wrapper for Tauri
pub struct MusicBrainzV2State {
    pub client: Arc<Mutex<MusicBrainzClient>>,
    /// V2 SQLite cache for persistence (settings, lookups)
    pub cache: Arc<Mutex<Option<MusicBrainzCache>>>,
}

impl MusicBrainzV2State {
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(MusicBrainzClient::new())),
            cache: Arc::new(Mutex::new(None)),
        }
    }

    /// Initialize cache at user data directory
    pub async fn init_cache_at(&self, base_dir: &Path) -> Result<(), String> {
        let cache_dir = base_dir.join("cache");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create MB cache directory: {}", e))?;
        let db_path = cache_dir.join("musicbrainz_v2.db");
        let cache = MusicBrainzCache::new(&db_path)?;
        log::info!("MusicBrainz V2 cache initialized at {:?}", db_path);
        let mut guard = self.cache.lock().await;
        *guard = Some(cache);
        Ok(())
    }

    /// Initialize from V2 cache (reads persisted enabled state)
    pub async fn init_from_cache(&self, use_proxy: bool) {
        let cache_guard = self.cache.lock().await;
        let enabled = if let Some(cache) = cache_guard.as_ref() {
            cache.is_enabled().unwrap_or(true)
        } else {
            true
        };
        drop(cache_guard);

        let config = MusicBrainzConfig { enabled, use_proxy };
        *self.client.lock().await = MusicBrainzClient::with_config(config);
    }

    /// Initialize with configuration
    pub async fn init_with_config(&self, enabled: bool, use_proxy: bool) {
        let config = MusicBrainzConfig { enabled, use_proxy };
        *self.client.lock().await = MusicBrainzClient::with_config(config);
    }

    /// Teardown cache (on session deactivate)
    pub async fn teardown(&self) {
        let mut guard = self.cache.lock().await;
        *guard = None;
    }
}

impl Default for MusicBrainzV2State {
    fn default() -> Self {
        Self::new()
    }
}

/// V2 Last.fm state wrapper for Tauri
pub struct LastFmV2State {
    pub client: Arc<Mutex<LastFmClient>>,
    /// Pending auth token (between get_token and get_session)
    pending_token: Arc<Mutex<Option<String>>>,
}

impl LastFmV2State {
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(LastFmClient::new())),
            pending_token: Arc::new(Mutex::new(None)),
        }
    }

    /// Initialize with saved session key
    pub async fn init_with_session(&self, session_key: Option<String>) {
        match session_key {
            Some(key) => self.client.lock().await.set_session_key(key),
            None => self.clear_session().await,
        }
    }

    /// Clear session and pending token (full teardown)
    pub async fn clear_session(&self) {
        self.client.lock().await.clear_session();
        *self.pending_token.lock().await = None;
    }

    /// Store pending token during auth flow
    pub async fn set_pending_token(&self, token: String) {
        *self.pending_token.lock().await = Some(token);
    }

    /// Get and consume pending token
    pub async fn take_pending_token(&self) -> Option<String> {
        self.pending_token.lock().await.take()
    }
}

impl Default for LastFmV2State {
    fn default() -> Self {
        Self::new()
    }
}

// Re-export types for commands
pub use qbz_integrations::lastfm::LastFmSession as LfSession;
pub use qbz_integrations::listenbrainz::ListenBrainzStatus as LbStatus;
pub use qbz_integrations::listenbrainz::UserInfo;
