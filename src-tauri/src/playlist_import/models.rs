//! Data models for playlist import

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ImportProvider {
    Spotify,
    AppleMusic,
    Tidal,
    Deezer,
}

impl ImportProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportProvider::Spotify => "spotify",
            ImportProvider::AppleMusic => "apple_music",
            ImportProvider::Tidal => "tidal",
            ImportProvider::Deezer => "deezer",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportTrack {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub isrc: Option<String>,
    pub provider_id: Option<String>,
    pub provider_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPlaylist {
    pub provider: ImportProvider,
    pub provider_id: String,
    pub name: String,
    pub description: Option<String>,
    pub tracks: Vec<ImportTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMatch {
    pub source: ImportTrack,
    pub qobuz_track_id: Option<u64>,
    pub qobuz_title: Option<String>,
    pub qobuz_artist: Option<String>,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSummary {
    pub provider: ImportProvider,
    pub playlist_name: String,
    pub total_tracks: u32,
    pub matched_tracks: u32,
    pub skipped_tracks: u32,
    pub qobuz_playlist_id: Option<u64>,
    pub matches: Vec<TrackMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportProgress {
    pub phase: String,
    pub current: u32,
    pub total: u32,
    pub matched_so_far: u32,
    pub current_track: Option<String>,
}
