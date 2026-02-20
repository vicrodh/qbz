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

use qbz_audio::{settings::AudioSettingsStore, AudioDiagnostic, AudioSettings, VisualizerTap};
use qbz_core::QbzCore;
use qbz_models::{
    Album, Artist, DiscoverAlbum, DiscoverData, DiscoverPlaylistsResponse, DiscoverResponse,
    GenreInfo, LabelDetail, LabelExploreResponse, LabelPageData, PageArtistResponse, Playlist,
    PlaylistTag, Quality, QueueState,
    QueueTrack, RepeatMode, SearchResultsPage, StreamUrl, Track, UserSession,
};
use qbz_player::{PlaybackState, Player};

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
    pub async fn new(
        adapter: TauriAdapter,
        visualizer_tap: Option<VisualizerTap>,
    ) -> Result<Self, String> {
        // Load audio settings using qbz_audio store
        let (device_name, audio_settings) = AudioSettingsStore::new()
            .ok()
            .and_then(|store| {
                store
                    .get_settings()
                    .ok()
                    .map(|settings| (settings.output_device.clone(), settings))
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
        let player = Player::new(device_name, audio_settings, visualizer_tap, diagnostic);

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
        self.core
            .login(email, password)
            .await
            .map_err(|e| e.to_string())
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

    /// Set shuffle mode directly
    pub async fn set_shuffle(&self, enabled: bool) {
        self.core.set_shuffle(enabled).await
    }

    /// Clear the queue
    pub async fn clear_queue(&self) {
        self.core.clear_queue().await
    }

    /// Add a track to the end of the queue
    pub async fn add_track(&self, track: QueueTrack) {
        self.core.add_track(track).await
    }

    /// Add multiple tracks to the queue
    pub async fn add_tracks(&self, tracks: Vec<QueueTrack>) {
        self.core.add_tracks(tracks).await
    }

    /// Add a track to play next (after current)
    pub async fn add_track_next(&self, track: QueueTrack) {
        self.core.add_track_next(track).await
    }

    /// Set the entire queue (replaces existing)
    pub async fn set_queue(&self, tracks: Vec<QueueTrack>, start_index: Option<usize>) {
        self.core.set_queue(tracks, start_index).await
    }

    /// Remove a track by index
    pub async fn remove_track(&self, index: usize) -> Option<QueueTrack> {
        self.core.remove_track(index).await
    }

    /// Remove a track from the upcoming list by position
    pub async fn remove_upcoming_track(&self, upcoming_index: usize) -> Option<QueueTrack> {
        self.core.remove_upcoming_track(upcoming_index).await
    }

    /// Move a track from one position to another
    pub async fn move_track(&self, from_index: usize, to_index: usize) -> bool {
        self.core.move_track(from_index, to_index).await
    }

    /// Jump to a specific track by index
    pub async fn play_index(&self, index: usize) -> Option<QueueTrack> {
        self.core.play_index(index).await
    }

    /// Advance to next track in queue
    pub async fn next_track(&self) -> Option<QueueTrack> {
        self.core.next_track().await
    }

    /// Go to previous track in queue
    pub async fn previous_track(&self) -> Option<QueueTrack> {
        self.core.previous_track().await
    }

    /// Get multiple upcoming tracks without advancing (for prefetching)
    pub async fn peek_upcoming(&self, count: usize) -> Vec<QueueTrack> {
        self.core.peek_upcoming(count).await
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
        self.core
            .get_album(album_id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get track by ID
    pub async fn get_track(&self, track_id: u64) -> Result<Track, String> {
        self.core
            .get_track(track_id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get artist by ID
    pub async fn get_artist(&self, artist_id: u64) -> Result<Artist, String> {
        self.core
            .get_artist(artist_id)
            .await
            .map_err(|e| e.to_string())
    }

    // ==================== Favorites ====================

    /// Get favorites (albums, tracks, or artists)
    pub async fn get_favorites(
        &self,
        fav_type: &str,
        limit: u32,
        offset: u32,
    ) -> Result<serde_json::Value, String> {
        self.core
            .get_favorites(fav_type, limit, offset)
            .await
            .map_err(|e| e.to_string())
    }

    /// Add item to favorites
    pub async fn add_favorite(&self, fav_type: &str, item_id: &str) -> Result<(), String> {
        self.core
            .add_favorite(fav_type, item_id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Remove item from favorites
    pub async fn remove_favorite(&self, fav_type: &str, item_id: &str) -> Result<(), String> {
        self.core
            .remove_favorite(fav_type, item_id)
            .await
            .map_err(|e| e.to_string())
    }

    // ==================== Playlists ====================

    /// Get user playlists
    pub async fn get_user_playlists(&self) -> Result<Vec<Playlist>, String> {
        self.core
            .get_user_playlists()
            .await
            .map_err(|e| e.to_string())
    }

    /// Get playlist by ID
    pub async fn get_playlist(&self, playlist_id: u64) -> Result<Playlist, String> {
        self.core
            .get_playlist(playlist_id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Add tracks to playlist
    pub async fn add_tracks_to_playlist(
        &self,
        playlist_id: u64,
        track_ids: &[u64],
    ) -> Result<(), String> {
        self.core
            .add_tracks_to_playlist(playlist_id, track_ids)
            .await
            .map_err(|e| e.to_string())
    }

    /// Remove tracks from playlist
    pub async fn remove_tracks_from_playlist(
        &self,
        playlist_id: u64,
        playlist_track_ids: &[u64],
    ) -> Result<(), String> {
        self.core
            .remove_tracks_from_playlist(playlist_id, playlist_track_ids)
            .await
            .map_err(|e| e.to_string())
    }

    /// Create a new playlist
    pub async fn create_playlist(
        &self,
        name: &str,
        description: Option<&str>,
        is_public: bool,
    ) -> Result<Playlist, String> {
        self.core
            .create_playlist(name, description, is_public)
            .await
            .map_err(|e| e.to_string())
    }

    /// Delete a playlist
    pub async fn delete_playlist(&self, playlist_id: u64) -> Result<(), String> {
        self.core
            .delete_playlist(playlist_id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Update a playlist
    pub async fn update_playlist(
        &self,
        playlist_id: u64,
        name: Option<&str>,
        description: Option<&str>,
        is_public: Option<bool>,
    ) -> Result<Playlist, String> {
        self.core
            .update_playlist(playlist_id, name, description, is_public)
            .await
            .map_err(|e| e.to_string())
    }

    /// Search playlists
    pub async fn search_playlists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResultsPage<Playlist>, String> {
        self.core
            .search_playlists(query, limit, offset)
            .await
            .map_err(|e| e.to_string())
    }

    // ==================== Catalog Extended ====================

    /// Get tracks batch by IDs
    pub async fn get_tracks_batch(&self, track_ids: &[u64]) -> Result<Vec<Track>, String> {
        self.core
            .get_tracks_batch(track_ids)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get genres
    pub async fn get_genres(&self, parent_id: Option<u64>) -> Result<Vec<GenreInfo>, String> {
        self.core
            .get_genres(parent_id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get discover index
    pub async fn get_discover_index(
        &self,
        genre_ids: Option<Vec<u64>>,
    ) -> Result<DiscoverResponse, String> {
        self.core
            .get_discover_index(genre_ids)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get discover playlists
    pub async fn get_discover_playlists(
        &self,
        tag: Option<String>,
        genre_ids: Option<Vec<u64>>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<DiscoverPlaylistsResponse, String> {
        self.core
            .get_discover_playlists(tag, genre_ids, limit, offset)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get playlist tags
    pub async fn get_playlist_tags(&self) -> Result<Vec<PlaylistTag>, String> {
        self.core
            .get_playlist_tags()
            .await
            .map_err(|e| e.to_string())
    }

    /// Get discover albums from a specific browse endpoint
    pub async fn get_discover_albums(
        &self,
        endpoint: &str,
        genre_ids: Option<Vec<u64>>,
        offset: u32,
        limit: u32,
    ) -> Result<DiscoverData<DiscoverAlbum>, String> {
        self.core
            .get_discover_albums(endpoint, genre_ids, offset, limit)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get featured albums
    pub async fn get_featured_albums(
        &self,
        featured_type: &str,
        limit: u32,
        offset: u32,
        genre_id: Option<u64>,
    ) -> Result<SearchResultsPage<Album>, String> {
        self.core
            .get_featured_albums(featured_type, limit, offset, genre_id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get artist page (full artist details with albums, tracks, similar)
    pub async fn get_artist_page(
        &self,
        artist_id: u64,
        sort: Option<&str>,
    ) -> Result<PageArtistResponse, String> {
        self.core
            .get_artist_page(artist_id, sort)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get similar artists
    pub async fn get_similar_artists(
        &self,
        artist_id: u64,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResultsPage<Artist>, String> {
        self.core
            .get_similar_artists(artist_id, limit, offset)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get artist with albums (for album pagination)
    pub async fn get_artist_with_albums(
        &self,
        artist_id: u64,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Artist, String> {
        self.core
            .get_artist_with_albums(artist_id, limit, offset)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get label details
    pub async fn get_label(
        &self,
        label_id: u64,
        limit: u32,
        offset: u32,
    ) -> Result<LabelDetail, String> {
        self.core
            .get_label(label_id, limit, offset)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get label page (aggregated: top tracks, releases, playlists, artists)
    pub async fn get_label_page(&self, label_id: u64) -> Result<LabelPageData, String> {
        self.core
            .get_label_page(label_id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get label explore (discover more labels)
    pub async fn get_label_explore(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<LabelExploreResponse, String> {
        self.core
            .get_label_explore(limit, offset)
            .await
            .map_err(|e| e.to_string())
    }

    // ==================== Streaming ====================

    /// Get stream URL for a track with quality fallback
    pub async fn get_stream_url(
        &self,
        track_id: u64,
        quality: Quality,
    ) -> Result<StreamUrl, String> {
        self.core
            .get_stream_url(track_id, quality)
            .await
            .map_err(|e| e.to_string())
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
        let bridge = CoreBridge::new(adapter, None).await?;
        *self.0.write().await = Some(bridge);
        Ok(())
    }

    /// Get the bridge (panics if not initialized)
    pub async fn get(&self) -> impl std::ops::Deref<Target = CoreBridge> + '_ {
        tokio::sync::RwLockReadGuard::map(self.0.read().await, |opt| {
            opt.as_ref().expect("CoreBridge not initialized")
        })
    }

    /// Try to get the bridge, returns None if not initialized yet
    /// Use this for operations that should gracefully handle uninitialized state
    pub async fn try_get(&self) -> Option<impl std::ops::Deref<Target = CoreBridge> + '_> {
        let guard = self.0.read().await;
        if guard.is_some() {
            Some(tokio::sync::RwLockReadGuard::map(guard, |opt| {
                opt.as_ref().unwrap()
            }))
        } else {
            None
        }
    }

    /// Check if the bridge is initialized
    pub async fn is_initialized(&self) -> bool {
        self.0.read().await.is_some()
    }
}

impl Default for CoreBridgeState {
    fn default() -> Self {
        Self::new()
    }
}
