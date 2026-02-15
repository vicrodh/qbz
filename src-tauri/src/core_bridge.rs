//! Core Bridge
//!
//! Bridges the new QbzCore architecture with the existing Tauri app.
//! This allows gradual migration - new code uses QbzCore, old code
//! continues to work until migrated.
//!
//! ARCHITECTURE: This bridge owns the Player from qbz-player crate.
//! V2 commands should use CoreBridge for playback, NOT AppState.

use std::sync::Arc;
use tokio::sync::RwLock;

use qbz_core::QbzCore;
use qbz_models::{Album, Artist, QueueState, RepeatMode, SearchResultsPage, Track, UserSession};
use qbz_player::{Player, PlaybackState};
use qbz_audio::{AudioSettings, AudioDiagnostic, settings::AudioSettingsStore};

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
    ///
    /// Loads audio settings from qbz_audio::AudioSettingsStore, creates a Player
    /// from qbz-player crate, and passes it to QbzCore.
    /// This is the V2 architecture - playback goes through QbzCore.
    pub async fn new(adapter: TauriAdapter) -> Result<Self, String> {
        // Load audio settings using qbz_audio store
        let (device_name, audio_settings) = AudioSettingsStore::new()
            .ok()
            .and_then(|store| {
                store.get_settings().ok().map(|settings| {
                    (settings.output_device.clone(), settings)
                })
            })
            .unwrap_or_else(|| {
                log::info!("[CoreBridge] No saved audio settings, using defaults");
                (None, AudioSettings::default())
            });

        log::info!(
            "[CoreBridge] Creating Player with device={:?}, exclusive={}, dac_passthrough={}",
            device_name,
            audio_settings.exclusive_mode,
            audio_settings.dac_passthrough
        );

        // Create Player from qbz-player crate
        let diagnostic = AudioDiagnostic::new();
        let player = Player::new(device_name, audio_settings, None, diagnostic);

        // Create QbzCore with the player
        let core = QbzCore::new(adapter, player);
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
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Album>, String> {
        self.core
            .search_albums(query, limit, offset, search_type)
            .await
            .map_err(|e| e.to_string())
    }

    /// Search for tracks
    pub async fn search_tracks(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Track>, String> {
        self.core
            .search_tracks(query, limit, offset, search_type)
            .await
            .map_err(|e| e.to_string())
    }

    /// Search for artists
    pub async fn search_artists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Artist>, String> {
        self.core
            .search_artists(query, limit, offset, search_type)
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

    // ==================== Playback Commands ====================

    /// Pause playback
    pub fn pause(&self) -> Result<(), String> {
        self.core.pause().map_err(|e| e.to_string())
    }

    /// Resume playback
    pub fn resume(&self) -> Result<(), String> {
        self.core.resume().map_err(|e| e.to_string())
    }

    /// Stop playback
    pub fn stop(&self) -> Result<(), String> {
        self.core.stop().map_err(|e| e.to_string())
    }

    /// Seek to position in seconds
    pub fn seek(&self, position: u64) -> Result<(), String> {
        self.core.seek(position).map_err(|e| e.to_string())
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) -> Result<(), String> {
        self.core.set_volume(volume).map_err(|e| e.to_string())
    }

    /// Get current playback state
    pub fn get_playback_state(&self) -> PlaybackState {
        self.core.get_playback_state()
    }

    /// Get the player (for advanced usage, e.g. play_track)
    pub fn player(&self) -> Arc<Player> {
        self.core.player()
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
