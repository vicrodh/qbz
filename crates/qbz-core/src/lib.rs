//! QBZ Core - Frontend-agnostic music player library

pub mod api;
pub mod events;
pub mod traits;
pub mod types;

use std::sync::Arc;
use tokio::sync::RwLock;

// Re-exports
pub use api::{QobuzClient, ApiError};
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
pub struct QbzCore<A: FrontendAdapter> {
    adapter: Arc<A>,
    client: Arc<RwLock<QobuzClient>>,
}

impl<A: FrontendAdapter> QbzCore<A> {
    /// Create a new QbzCore instance with the given frontend adapter
    pub fn new(adapter: A) -> Self {
        Self {
            adapter: Arc::new(adapter),
            client: Arc::new(RwLock::new(QobuzClient::default())),
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
