//! Core Bridge
//!
//! Bridges the new QbzCore architecture with the existing Tauri app.
//! This allows gradual migration - new code uses QbzCore, old code
//! continues to work until migrated.

use std::sync::Arc;
use tokio::sync::RwLock;

use qbz_core::QbzCore;
use qbz_models::{Album, Artist, QueueState, RepeatMode, Track, UserSession};

use crate::tauri_adapter::TauriAdapter;

/// Bridge to the new QbzCore architecture
///
/// This struct provides access to QbzCore functionality while
/// the old AppState continues to handle unmigrated features.
pub struct CoreBridge {
    core: Arc<QbzCore<TauriAdapter>>,
}

impl CoreBridge {
    /// Create a new CoreBridge with the given TauriAdapter
    pub async fn new(adapter: TauriAdapter) -> Result<Self, String> {
        let core = QbzCore::new(adapter);
        core.init().await.map_err(|e| e.to_string())?;

        Ok(Self {
            core: Arc::new(core),
        })
    }

    /// Get a reference to the underlying QbzCore
    pub fn core(&self) -> &Arc<QbzCore<TauriAdapter>> {
        &self.core
    }

    // ==================== Auth Commands ====================

    /// Check if user is logged in
    pub async fn is_logged_in(&self) -> bool {
        self.core.has_session().await
    }

    /// Login with email and password
    pub async fn login(&self, email: &str, password: &str) -> Result<UserSession, String> {
        self.core.login(email, password).await.map_err(|e| e.to_string())
    }

    /// Logout current user
    pub async fn logout(&self) -> Result<(), String> {
        self.core.logout().await.map_err(|e| e.to_string())
    }

    // ==================== Queue Commands ====================

    /// Get current queue state
    pub async fn get_queue_state(&self) -> QueueState {
        self.core.get_queue_state().await
    }

    /// Set repeat mode
    pub async fn set_repeat_mode(&self, mode: RepeatMode) {
        self.core.set_repeat_mode(mode).await
    }

    /// Toggle shuffle
    pub async fn toggle_shuffle(&self) -> bool {
        self.core.toggle_shuffle().await
    }

    /// Clear the queue
    pub async fn clear_queue(&self) {
        self.core.clear_queue().await
    }

    // ==================== Search & Catalog ====================

    /// Search for albums
    pub async fn search_albums(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Album>, String> {
        self.core
            .search_albums(query, limit, offset)
            .await
            .map_err(|e| e.to_string())
    }

    /// Search for tracks
    pub async fn search_tracks(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Track>, String> {
        self.core
            .search_tracks(query, limit, offset)
            .await
            .map_err(|e| e.to_string())
    }

    /// Search for artists
    pub async fn search_artists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Artist>, String> {
        self.core
            .search_artists(query, limit, offset)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get album by ID
    pub async fn get_album(&self, album_id: &str) -> Result<Album, String> {
        self.core.get_album(album_id).await.map_err(|e| e.to_string())
    }

    /// Get track by ID
    pub async fn get_track(&self, track_id: u64) -> Result<Track, String> {
        self.core.get_track(track_id).await.map_err(|e| e.to_string())
    }

    /// Get artist by ID
    pub async fn get_artist(&self, artist_id: u64) -> Result<Artist, String> {
        self.core.get_artist(artist_id).await.map_err(|e| e.to_string())
    }
}

/// State wrapper for Tauri's managed state
pub struct CoreBridgeState(pub Arc<RwLock<Option<CoreBridge>>>);

impl CoreBridgeState {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(None)))
    }

    /// Initialize the core bridge with the app handle
    pub async fn init(&self, adapter: TauriAdapter) -> Result<(), String> {
        let bridge = CoreBridge::new(adapter).await?;
        *self.0.write().await = Some(bridge);
        Ok(())
    }

    /// Get the bridge (panics if not initialized)
    pub async fn get(&self) -> impl std::ops::Deref<Target = CoreBridge> + '_ {
        tokio::sync::RwLockReadGuard::map(
            self.0.read().await,
            |opt| opt.as_ref().expect("CoreBridge not initialized")
        )
    }
}

impl Default for CoreBridgeState {
    fn default() -> Self {
        Self::new()
    }
}
