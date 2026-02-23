//! QBZ Core Orchestrator
//!
//! The main orchestrator that connects all QBZ subsystems and provides
//! a unified API for frontends.

use std::sync::Arc;
use tokio::sync::RwLock;

use qbz_models::{
    Album, Artist, CoreEvent, DiscoverAlbum, DiscoverData, DiscoverPlaylistsResponse,
    DiscoverResponse, FrontendAdapter, GenreInfo, LabelDetail, LabelExploreResponse,
    LabelPageData, PageArtistResponse, Playlist,
    PlaylistTag, Quality, QueueState, QueueTrack, RepeatMode, SearchResultsPage, StreamUrl, Track, UserSession,
};
use qbz_player::{Player, PlaybackState, QueueManager};
use qbz_qobuz::QobuzClient;

use crate::error::CoreError;

/// Core orchestrator for QBZ
///
/// This is the main entry point for any frontend (Tauri, Slint, Iced, CLI, etc.)
/// It provides a unified API and emits events through the FrontendAdapter.
pub struct QbzCore<A: FrontendAdapter> {
    /// Frontend adapter for event emission
    adapter: Arc<A>,
    /// Qobuz API client
    client: Arc<RwLock<Option<QobuzClient>>>,
    /// Queue manager
    queue: Arc<RwLock<QueueManager>>,
    /// Audio player
    player: Arc<Player>,
    /// Whether the core is initialized
    initialized: Arc<RwLock<bool>>,
}

impl<A: FrontendAdapter + Send + Sync + 'static> QbzCore<A> {
    /// Create a new QbzCore instance with the given frontend adapter and player
    ///
    /// The Player must be created by the frontend with appropriate audio settings.
    /// QbzCore orchestrates playback through this player.
    pub fn new(adapter: A, player: Player) -> Self {
        Self {
            adapter: Arc::new(adapter),
            client: Arc::new(RwLock::new(None)),
            queue: Arc::new(RwLock::new(QueueManager::new())),
            player: Arc::new(player),
            initialized: Arc::new(RwLock::new(false)),
        }
    }

    /// Initialize the core
    ///
    /// This should be called once at startup to set up all subsystems.
    /// This extracts Qobuz bundle tokens - user must still call login() to authenticate.
    pub async fn init(&self) -> Result<(), CoreError> {
        let mut initialized = self.initialized.write().await;
        if *initialized {
            return Ok(());
        }

        // Initialize Qobuz client
        let client = QobuzClient::new().map_err(|e| CoreError::Internal(e.to_string()))?;

        // Extract bundle tokens (required before any API calls)
        client.init().await.map_err(|e| {
            CoreError::Internal(format!("Failed to extract bundle tokens: {}", e))
        })?;

        *self.client.write().await = Some(client);

        *initialized = true;
        log::info!("QbzCore initialized with bundle tokens");
        Ok(())
    }

    /// Check if a user session exists
    pub async fn has_session(&self) -> bool {
        let client = self.client.read().await;
        if let Some(c) = client.as_ref() {
            c.is_logged_in().await
        } else {
            false
        }
    }

    /// Login with email and password
    pub async fn login(&self, email: &str, password: &str) -> Result<UserSession, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        match client.login(email, password).await {
            Ok(session) => {
                self.emit(CoreEvent::LoggedIn {
                    session: session.clone(),
                })
                .await;
                Ok(session)
            }
            Err(e) => {
                self.emit(CoreEvent::Error {
                    code: "AUTH_FAILED".to_string(),
                    message: e.to_string(),
                    recoverable: true,
                })
                .await;
                Err(CoreError::AuthFailed(e.to_string()))
            }
        }
    }

    /// Logout the current user
    pub async fn logout(&self) -> Result<(), CoreError> {
        let client = self.client.read().await;
        if let Some(c) = client.as_ref() {
            c.logout().await;
            self.emit(CoreEvent::LoggedOut).await;
        }
        Ok(())
    }

    // ==================== Queue Operations ====================

    /// Get current queue state
    pub async fn get_queue_state(&self) -> QueueState {
        let queue = self.queue.read().await;
        queue.get_state()
    }

    /// Get all queue tracks and current index (for session persistence)
    pub async fn get_all_queue_tracks(&self) -> (Vec<QueueTrack>, Option<usize>) {
        let queue = self.queue.read().await;
        queue.get_all_tracks()
    }

    /// Set repeat mode
    pub async fn set_repeat_mode(&self, mode: RepeatMode) {
        let queue = self.queue.write().await;
        queue.set_repeat(mode.clone());
        self.emit(CoreEvent::RepeatModeChanged { mode }).await;
    }

    /// Set shuffle
    pub async fn set_shuffle(&self, enabled: bool) {
        let queue = self.queue.write().await;
        queue.set_shuffle(enabled);
        self.emit(CoreEvent::ShuffleChanged { enabled }).await;
    }

    /// Toggle shuffle and return new state
    pub async fn toggle_shuffle(&self) -> bool {
        let queue = self.queue.write().await;
        let was_enabled = queue.is_shuffle();
        let new_enabled = !was_enabled;
        queue.set_shuffle(new_enabled);
        self.emit(CoreEvent::ShuffleChanged {
            enabled: new_enabled,
        })
        .await;
        new_enabled
    }

    /// Clear the queue
    pub async fn clear_queue(&self) {
        let queue = self.queue.write().await;
        queue.clear();
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Add a track to the end of the queue
    pub async fn add_track(&self, track: QueueTrack) {
        let queue = self.queue.write().await;
        queue.add_track(track);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Add multiple tracks to the queue
    pub async fn add_tracks(&self, tracks: Vec<QueueTrack>) {
        let queue = self.queue.write().await;
        queue.add_tracks(tracks);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Add a track to play next (after current)
    pub async fn add_track_next(&self, track: QueueTrack) {
        let queue = self.queue.write().await;
        queue.add_track_next(track);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Set the entire queue (replaces existing)
    pub async fn set_queue(&self, tracks: Vec<QueueTrack>, start_index: Option<usize>) {
        let queue = self.queue.write().await;
        queue.set_queue(tracks, start_index);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Remove a track by index
    pub async fn remove_track(&self, index: usize) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let removed = queue.remove_track(index);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        removed
    }

    /// Remove a track from the upcoming list by position
    pub async fn remove_upcoming_track(&self, upcoming_index: usize) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let removed = queue.remove_upcoming_track(upcoming_index);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        removed
    }

    /// Move a track from one position to another
    pub async fn move_track(&self, from_index: usize, to_index: usize) -> bool {
        let queue = self.queue.write().await;
        let success = queue.move_track(from_index, to_index);
        if success {
            self.emit(CoreEvent::QueueUpdated {
                state: queue.get_state(),
            })
            .await;
        }
        success
    }

    /// Jump to a specific track by index
    pub async fn play_index(&self, index: usize) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let track = queue.play_index(index);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        track
    }

    /// Advance to next track in queue
    pub async fn next_track(&self) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let track = queue.next();
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        track
    }

    /// Go to previous track in queue
    pub async fn previous_track(&self) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let track = queue.previous();
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        track
    }

    /// Get multiple upcoming tracks without advancing (for prefetching)
    pub async fn peek_upcoming(&self, count: usize) -> Vec<QueueTrack> {
        let queue = self.queue.read().await;
        queue.peek_upcoming(count)
    }

    // ==================== Search & Catalog ====================

    /// Search for albums
    pub async fn search_albums(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Album>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .search_albums(query, limit, offset, search_type)
            .await
            .map_err(CoreError::Api)
    }

    /// Search for tracks
    pub async fn search_tracks(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Track>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .search_tracks(query, limit, offset, search_type)
            .await
            .map_err(CoreError::Api)
    }

    /// Search for artists
    pub async fn search_artists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Artist>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .search_artists(query, limit, offset, search_type)
            .await
            .map_err(CoreError::Api)
    }

    /// Get album by ID
    pub async fn get_album(&self, album_id: &str) -> Result<Album, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client.get_album(album_id).await.map_err(CoreError::Api)
    }

    /// Get track by ID
    pub async fn get_track(&self, track_id: u64) -> Result<Track, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client.get_track(track_id).await.map_err(CoreError::Api)
    }

    /// Get artist by ID
    pub async fn get_artist(&self, artist_id: u64) -> Result<Artist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_artist_basic(artist_id)
            .await
            .map_err(CoreError::Api)
    }

    // ==================== Streaming ====================

    /// Get stream URL for a track with quality fallback
    pub async fn get_stream_url(
        &self,
        track_id: u64,
        quality: Quality,
    ) -> Result<StreamUrl, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_stream_url_with_fallback(track_id, quality)
            .await
            .map_err(CoreError::Api)
    }

    // ==================== Playback Operations ====================

    /// Pause playback
    pub fn pause(&self) -> Result<(), CoreError> {
        self.player
            .pause()
            .map_err(|e| CoreError::Playback(e))
    }

    /// Resume playback
    pub fn resume(&self) -> Result<(), CoreError> {
        self.player
            .resume()
            .map_err(|e| CoreError::Playback(e))
    }

    /// Stop playback
    pub fn stop(&self) -> Result<(), CoreError> {
        self.player
            .stop()
            .map_err(|e| CoreError::Playback(e))
    }

    /// Seek to position in seconds
    pub fn seek(&self, position: u64) -> Result<(), CoreError> {
        self.player
            .seek(position)
            .map_err(|e| CoreError::Playback(e))
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) -> Result<(), CoreError> {
        self.player
            .set_volume(volume)
            .map_err(|e| CoreError::Playback(e))
    }

    /// Get current playback state
    pub fn get_playback_state(&self) -> PlaybackState {
        let state = &self.player.state;
        PlaybackState {
            is_playing: state.is_playing(),
            position: state.current_position(),
            duration: state.duration(),
            track_id: state.current_track_id(),
            volume: state.volume(),
        }
    }

    /// Get the player (for advanced usage)
    pub fn player(&self) -> Arc<Player> {
        Arc::clone(&self.player)
    }

    // ==================== Favorites ====================

    /// Get favorites (albums, tracks, or artists)
    pub async fn get_favorites(
        &self,
        fav_type: &str,
        limit: u32,
        offset: u32,
    ) -> Result<serde_json::Value, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_favorites(fav_type, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Add item to favorites
    pub async fn add_favorite(&self, fav_type: &str, item_id: &str) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .add_favorite(fav_type, item_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Remove item from favorites
    pub async fn remove_favorite(&self, fav_type: &str, item_id: &str) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .remove_favorite(fav_type, item_id)
            .await
            .map_err(CoreError::Api)
    }

    // ==================== Playlists ====================

    /// Get user playlists
    pub async fn get_user_playlists(&self) -> Result<Vec<Playlist>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client.get_user_playlists().await.map_err(CoreError::Api)
    }

    /// Get playlist by ID
    pub async fn get_playlist(&self, playlist_id: u64) -> Result<Playlist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client.get_playlist(playlist_id).await.map_err(CoreError::Api)
    }

    /// Add tracks to playlist
    pub async fn add_tracks_to_playlist(
        &self,
        playlist_id: u64,
        track_ids: &[u64],
    ) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .add_tracks_to_playlist(playlist_id, track_ids)
            .await
            .map_err(CoreError::Api)
    }

    /// Remove tracks from playlist
    pub async fn remove_tracks_from_playlist(
        &self,
        playlist_id: u64,
        playlist_track_ids: &[u64],
    ) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .remove_tracks_from_playlist(playlist_id, playlist_track_ids)
            .await
            .map_err(CoreError::Api)
    }

    /// Create a new playlist
    pub async fn create_playlist(
        &self,
        name: &str,
        description: Option<&str>,
        is_public: bool,
    ) -> Result<Playlist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .create_playlist(name, description, is_public)
            .await
            .map_err(CoreError::Api)
    }

    /// Delete a playlist
    pub async fn delete_playlist(&self, playlist_id: u64) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .delete_playlist(playlist_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Update a playlist
    pub async fn update_playlist(
        &self,
        playlist_id: u64,
        name: Option<&str>,
        description: Option<&str>,
        is_public: Option<bool>,
    ) -> Result<Playlist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .update_playlist(playlist_id, name, description, is_public)
            .await
            .map_err(CoreError::Api)
    }

    /// Search playlists
    pub async fn search_playlists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResultsPage<Playlist>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .search_playlists(query, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get tracks batch by IDs
    pub async fn get_tracks_batch(&self, track_ids: &[u64]) -> Result<Vec<Track>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_tracks_batch(track_ids)
            .await
            .map_err(CoreError::Api)
    }

    /// Get genres
    pub async fn get_genres(&self, parent_id: Option<u64>) -> Result<Vec<GenreInfo>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_genres(parent_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Get discover index
    pub async fn get_discover_index(&self, genre_ids: Option<Vec<u64>>) -> Result<DiscoverResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_discover_index(genre_ids)
            .await
            .map_err(CoreError::Api)
    }

    /// Get discover playlists
    pub async fn get_discover_playlists(
        &self,
        tag: Option<String>,
        genre_ids: Option<Vec<u64>>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<DiscoverPlaylistsResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_discover_playlists(tag, genre_ids, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get playlist tags
    pub async fn get_playlist_tags(&self) -> Result<Vec<PlaylistTag>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_playlist_tags()
            .await
            .map_err(CoreError::Api)
    }

    /// Get discover albums from a specific browse endpoint
    pub async fn get_discover_albums(
        &self,
        endpoint: &str,
        genre_ids: Option<Vec<u64>>,
        offset: u32,
        limit: u32,
    ) -> Result<DiscoverData<DiscoverAlbum>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_discover_albums(endpoint, genre_ids, offset, limit)
            .await
            .map_err(CoreError::Api)
    }

    /// Get featured albums
    pub async fn get_featured_albums(
        &self,
        featured_type: &str,
        limit: u32,
        offset: u32,
        genre_id: Option<u64>,
    ) -> Result<SearchResultsPage<Album>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_featured_albums(featured_type, limit, offset, genre_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Get artist page (full artist details with albums, tracks, similar)
    pub async fn get_artist_page(
        &self,
        artist_id: u64,
        sort: Option<&str>,
    ) -> Result<PageArtistResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_artist_page(artist_id, sort)
            .await
            .map_err(CoreError::Api)
    }

    /// Get similar artists
    pub async fn get_similar_artists(
        &self,
        artist_id: u64,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResultsPage<Artist>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_similar_artists(artist_id, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get artist with albums (for album pagination)
    pub async fn get_artist_with_albums(
        &self,
        artist_id: u64,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Artist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_artist_with_pagination(artist_id, true, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get label details
    pub async fn get_label(
        &self,
        label_id: u64,
        limit: u32,
        offset: u32,
    ) -> Result<LabelDetail, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_label(label_id, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get label page (aggregated: top tracks, releases, playlists, artists)
    pub async fn get_label_page(&self, label_id: u64) -> Result<LabelPageData, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_label_page(label_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Get label explore (discover more labels)
    pub async fn get_label_explore(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<LabelExploreResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_label_explore(limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    // ==================== Event Emission ====================

    /// Emit an event to the frontend adapter
    async fn emit(&self, event: CoreEvent) {
        self.adapter.on_event(event).await;
    }

    /// Get the frontend adapter (for external event emission)
    pub fn adapter(&self) -> Arc<A> {
        Arc::clone(&self.adapter)
    }

    /// Get the Qobuz client (for advanced usage)
    pub fn client(&self) -> Arc<RwLock<Option<QobuzClient>>> {
        Arc::clone(&self.client)
    }

    /// Get the queue manager (for advanced usage)
    pub fn queue(&self) -> Arc<RwLock<QueueManager>> {
        Arc::clone(&self.queue)
    }
}
