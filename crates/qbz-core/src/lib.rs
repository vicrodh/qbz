//! QBZ Core - Frontend-agnostic music player library

pub mod api;
pub mod audio;
pub mod events;
pub mod traits;
pub mod types;

use std::sync::Arc;
use tokio::sync::RwLock;

// Re-exports
pub use api::{QobuzClient, ApiError, Quality};
pub use audio::AudioPlayer;
pub use events::{CoreEvent, PlaybackState, RepeatMode, AppError};
pub use traits::{FrontendAdapter, NullAdapter};
pub use types::*;

/// Error type for QbzCore operations
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("API error: {0}")]
    Api(#[from] api::ApiError),

    #[error("Audio error: {0}")]
    Audio(String),

    #[error("Not logged in")]
    NotLoggedIn,

    #[error("Track not found: {0}")]
    TrackNotFound(u64),

    #[error("Network error: {0}")]
    Network(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Main QBZ core - holds all state and provides the public API
/// Note: Audio playback is handled separately (AudioPlayer is not Send)
pub struct QbzCore<A: FrontendAdapter> {
    adapter: Arc<A>,
    client: Arc<RwLock<QobuzClient>>,
    http_client: reqwest::Client,
}

impl<A: FrontendAdapter> QbzCore<A> {
    /// Create a new QbzCore instance with the given frontend adapter
    pub fn new(adapter: A) -> Self {
        Self {
            adapter: Arc::new(adapter),
            client: Arc::new(RwLock::new(QobuzClient::default())),
            http_client: reqwest::Client::new(),
        }
    }

    /// Get a reference to the adapter
    pub fn adapter(&self) -> &Arc<A> {
        &self.adapter
    }

    // ─── Authentication ─────────────────────────────────────────────────────

    /// Initialize the Qobuz API client (fetch bundle tokens)
    pub async fn init(&self) -> Result<(), CoreError> {
        log::info!("Initializing Qobuz API client...");
        self.client.write().await.init().await?;
        log::info!("Qobuz API client initialized");
        Ok(())
    }

    /// Login with email and password
    pub async fn login(&self, email: &str, password: &str) -> Result<UserInfo, CoreError> {
        log::info!("Logging in as {}...", email);

        let session = self.client.write().await
            .login(email, password).await?;

        let user_info = UserInfo {
            id: session.user_id,
            email: session.email.clone(),
            display_name: session.display_name.clone(),
            subscription_type: session.subscription_label.clone(),
        };

        log::info!("Login successful: {} ({})", user_info.display_name, user_info.subscription_type);

        // Notify frontend
        self.adapter.on_event(CoreEvent::LoginSuccess(user_info.clone())).await;

        Ok(user_info)
    }

    /// Check if user is logged in
    pub async fn is_logged_in(&self) -> bool {
        self.client.read().await.is_logged_in().await
    }

    /// Logout current user
    pub async fn logout(&self) {
        log::info!("Logging out...");
        self.client.write().await.logout().await;
        self.adapter.on_event(CoreEvent::LoggedOut).await;
    }

    // ─── Search (simplified for POC) ────────────────────────────────────────

    /// Search for albums
    pub async fn search_albums(&self, query: &str, limit: usize) -> Result<Vec<Album>, CoreError> {
        log::info!("Searching albums: {}", query);

        let client = self.client.read().await;
        let response = client.search_albums(query, limit as u32, 0, None).await?;

        let albums = response.items.into_iter().map(|a| Album {
            id: a.id,
            title: a.title,
            artist: a.artist.name,
            artist_id: a.artist.id,
            cover_url: a.image.large.clone(),
            release_date: a.release_date_original,
            track_count: a.tracks_count.unwrap_or(0) as u32,
            duration: a.duration.unwrap_or(0) as u64 * 1000,
            hires_available: a.hires_streamable,
            genre: a.genre.map(|g| g.name),
        }).collect();

        Ok(albums)
    }

    // ─── Home/Discover ───────────────────────────────────────────────────

    /// Get featured/new albums for home page
    pub async fn get_featured_albums(&self, limit: usize) -> Result<Vec<Album>, CoreError> {
        log::info!("Fetching featured albums...");

        let client = self.client.read().await;
        // "new-releases" is one of the common featured types
        let response = client.get_featured_albums("new-releases", limit as u32, 0, None).await?;

        let albums = response.items.into_iter().map(|a| Album {
            id: a.id,
            title: a.title,
            artist: a.artist.name,
            artist_id: a.artist.id,
            cover_url: a.image.large.clone(),
            release_date: a.release_date_original,
            track_count: a.tracks_count.unwrap_or(0) as u32,
            duration: a.duration.unwrap_or(0) as u64 * 1000,
            hires_available: a.hires_streamable,
            genre: a.genre.map(|g| g.name),
        }).collect();

        Ok(albums)
    }

    /// Get editor's picks albums
    pub async fn get_editor_picks(&self, limit: usize) -> Result<Vec<Album>, CoreError> {
        log::info!("Fetching editor picks...");

        let client = self.client.read().await;
        let response = client.get_featured_albums("editor-picks", limit as u32, 0, None).await?;

        let albums = response.items.into_iter().map(|a| Album {
            id: a.id,
            title: a.title,
            artist: a.artist.name,
            artist_id: a.artist.id,
            cover_url: a.image.large.clone(),
            release_date: a.release_date_original,
            track_count: a.tracks_count.unwrap_or(0) as u32,
            duration: a.duration.unwrap_or(0) as u64 * 1000,
            hires_available: a.hires_streamable,
            genre: a.genre.map(|g| g.name),
        }).collect();

        Ok(albums)
    }

    // ─── Search ─────────────────────────────────────────────────────────────

    /// Search for tracks
    pub async fn search_tracks(&self, query: &str, limit: usize) -> Result<Vec<Track>, CoreError> {
        log::info!("Searching tracks: {}", query);

        let client = self.client.read().await;
        let response = client.search_tracks(query, limit as u32, 0, None).await?;

        let tracks = response.items.into_iter().filter_map(|t| {
            let album = t.album?;
            Some(Track {
                id: t.id,
                title: t.title,
                artist: t.performer.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
                artist_id: t.performer.as_ref().map(|p| p.id).unwrap_or(0),
                album: album.title.clone(),
                album_id: album.id,
                duration: t.duration as u64 * 1000,
                track_number: t.track_number as u32,
                disc_number: t.media_number.unwrap_or(1) as u32,
                cover_url: album.image.large.clone(),
                hires_available: t.hires_streamable,
                sample_rate: t.maximum_sampling_rate.map(|r| (r * 1000.0) as u32),
                bit_depth: t.maximum_bit_depth.map(|b| b as u8),
            })
        }).collect();

        Ok(tracks)
    }

    // ─── Album ─────────────────────────────────────────────────────────────

    /// Get album with tracks
    pub async fn get_album_tracks(&self, album_id: &str) -> Result<(Album, Vec<Track>), CoreError> {
        log::info!("Fetching album tracks: {}", album_id);

        let client = self.client.read().await;
        let api_album = client.get_album(album_id).await?;

        let album = Album {
            id: api_album.id.clone(),
            title: api_album.title.clone(),
            artist: api_album.artist.name.clone(),
            artist_id: api_album.artist.id,
            cover_url: api_album.image.large.clone(),
            release_date: api_album.release_date_original.clone(),
            track_count: api_album.tracks_count.unwrap_or(0) as u32,
            duration: api_album.duration.unwrap_or(0) as u64 * 1000,
            hires_available: api_album.hires_streamable,
            genre: api_album.genre.as_ref().map(|g| g.name.clone()),
        };

        let tracks = api_album.tracks
            .map(|tc| tc.items)
            .unwrap_or_default()
            .into_iter()
            .map(|t| {
                Track {
                    id: t.id,
                    title: t.title,
                    artist: t.performer.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| api_album.artist.name.clone()),
                    artist_id: t.performer.as_ref().map(|p| p.id).unwrap_or(api_album.artist.id),
                    album: api_album.title.clone(),
                    album_id: api_album.id.clone(),
                    duration: t.duration as u64 * 1000,
                    track_number: t.track_number as u32,
                    disc_number: t.media_number.unwrap_or(1) as u32,
                    cover_url: api_album.image.large.clone(),
                    hires_available: t.hires_streamable,
                    sample_rate: t.maximum_sampling_rate.map(|r| (r * 1000.0) as u32),
                    bit_depth: t.maximum_bit_depth.map(|b| b as u8),
                }
            })
            .collect();

        log::info!("Album {} has {} tracks", album.title, album.track_count);
        Ok((album, tracks))
    }

    // ─── Playback ──────────────────────────────────────────────────────────

    /// Get audio data for a track (downloads the audio)
    /// Returns the audio bytes that can be played by AudioPlayer
    pub async fn get_track_audio(&self, track_id: u64) -> Result<Vec<u8>, CoreError> {
        log::info!("Getting audio for track {}", track_id);

        // Get stream URL
        let client = self.client.read().await;
        let stream_url = client.get_stream_url_with_fallback(track_id, Quality::HiRes).await?;
        drop(client); // Release lock before downloading

        log::info!(
            "Got stream URL: {} ({}kHz, {} bit)",
            stream_url.mime_type,
            stream_url.sampling_rate,
            stream_url.bit_depth.unwrap_or(16)
        );

        // Download audio data
        log::info!("Downloading audio...");
        let response = self.http_client
            .get(&stream_url.url)
            .send()
            .await
            .map_err(|e| CoreError::Network(e.to_string()))?;

        let audio_data = response
            .bytes()
            .await
            .map_err(|e| CoreError::Network(e.to_string()))?
            .to_vec();

        log::info!("Downloaded {} bytes", audio_data.len());

        Ok(audio_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_core_creation() {
        let core = QbzCore::new(NullAdapter);
        assert!(!core.is_logged_in().await);
    }
}
