//! Shared types used across QBZ Core
//!
//! These types represent the domain model: tracks, albums, artists, etc.

use serde::{Deserialize, Serialize};

/// User information after login
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: u64,
    pub email: String,
    pub display_name: String,
    pub subscription_type: String,
}

/// Track information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub artist_id: u64,
    pub album: String,
    pub album_id: String,
    pub duration: u64,  // milliseconds
    pub track_number: u32,
    pub disc_number: u32,
    pub cover_url: Option<String>,
    pub hires_available: bool,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
}

/// Album information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: u64,
    pub cover_url: Option<String>,
    pub release_date: Option<String>,
    pub track_count: u32,
    pub duration: u64,
    pub hires_available: bool,
    pub genre: Option<String>,
}

/// Artist information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub id: u64,
    pub name: String,
    pub image_url: Option<String>,
    pub biography: Option<String>,
}

/// Playlist information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub owner: String,
    pub track_count: u32,
    pub duration: u64,
    pub cover_url: Option<String>,
    pub is_public: bool,
}

/// Track in the playback queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueTrack {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: u64,
    pub cover_url: Option<String>,
    /// Whether the track is available for playback
    pub available: bool,
}

impl From<Track> for QueueTrack {
    fn from(track: Track) -> Self {
        Self {
            id: track.id,
            title: track.title,
            artist: track.artist,
            album: track.album,
            duration: track.duration,
            cover_url: track.cover_url,
            available: true,
        }
    }
}

/// Search results from Qobuz
#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    pub albums: Vec<Album>,
    pub tracks: Vec<Track>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
}

/// Home/Discover page data
#[derive(Debug, Clone, Default)]
pub struct HomeData {
    pub featured_albums: Vec<Album>,
    pub new_releases: Vec<Album>,
    pub playlists: Vec<Playlist>,
}
