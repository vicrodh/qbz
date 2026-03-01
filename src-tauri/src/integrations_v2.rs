//! V2 Integrations State
//!
//! Tauri state wrappers for qbz-integrations crate.
//! These provide the glue between Tauri's state management and the
//! Tauri-independent integration clients.

use std::sync::Arc;
use tokio::sync::Mutex;

use qbz_integrations::{
    LastFmClient, ListenBrainzClient, ListenBrainzConfig, MusicBrainzClient, MusicBrainzConfig,
};

/// V2 ListenBrainz state wrapper for Tauri
pub struct ListenBrainzV2State {
    pub client: Arc<Mutex<ListenBrainzClient>>,
    /// Persisted token (loaded from/saved to user session)
    token: Arc<Mutex<Option<String>>>,
    user_name: Arc<Mutex<Option<String>>>,
}

impl ListenBrainzV2State {
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(ListenBrainzClient::new())),
            token: Arc::new(Mutex::new(None)),
            user_name: Arc::new(Mutex::new(None)),
        }
    }

    /// Initialize with saved credentials
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
        *self.token.lock().await = Some(token);
        *self.user_name.lock().await = Some(user_name);
    }

    /// Clear credentials on disconnect
    ///
    /// This clears both wrapper fields AND resets the internal client to ensure
    /// no credentials leak across sessions.
    pub async fn clear_credentials(&self) {
        *self.token.lock().await = None;
        *self.user_name.lock().await = None;
        // Reset internal client to fresh state (clears config with token/user_name)
        *self.client.lock().await = ListenBrainzClient::new();
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
}

impl MusicBrainzV2State {
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(MusicBrainzClient::new())),
        }
    }

    /// Initialize with configuration
    pub async fn init_with_config(&self, enabled: bool, use_proxy: bool) {
        let config = MusicBrainzConfig { enabled, use_proxy };
        *self.client.lock().await = MusicBrainzClient::with_config(config);
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
    ///
    /// If `None` is passed, this performs a full session clear to ensure
    /// no credentials leak across sessions.
    pub async fn init_with_session(&self, session_key: Option<String>) {
        match session_key {
            Some(key) => self.client.lock().await.set_session_key(key),
            None => self.clear_session().await,
        }
    }

    /// Clear session and pending token (full teardown)
    ///
    /// This MUST be called on logout/session deactivate to ensure
    /// no credentials leak to a new session.
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
