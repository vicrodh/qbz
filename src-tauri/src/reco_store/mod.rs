//! Recommendation store module
//!
//! Persists lightweight usage events for home recommendations.

pub mod commands;
pub mod db;

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use db::RecoStoreDb;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoEventType {
    Play,
    Favorite,
    PlaylistAdd,
}

impl RecoEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Favorite => "favorite",
            Self::PlaylistAdd => "playlist_add",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoItemType {
    Track,
    Album,
    Artist,
}

impl RecoItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::Artist => "artist",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoEventInput {
    pub event_type: RecoEventType,
    pub item_type: RecoItemType,
    pub track_id: Option<u64>,
    pub album_id: Option<String>,
    pub artist_id: Option<u64>,
    pub playlist_id: Option<u64>,
    pub genre_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopArtistSeed {
    pub artist_id: u64,
    pub play_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeSeeds {
    pub recently_played_album_ids: Vec<String>,
    pub continue_listening_track_ids: Vec<u64>,
    pub top_artist_ids: Vec<TopArtistSeed>,
    pub favorite_album_ids: Vec<String>,
    pub favorite_track_ids: Vec<u64>,
}

/// Fully resolved home page data returned by reco_get_home_resolved
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeResolved {
    pub recently_played_albums: Vec<AlbumCardMeta>,
    pub continue_listening_tracks: Vec<TrackDisplayMeta>,
    pub top_artists: Vec<ArtistCardMeta>,
    pub favorite_albums: Vec<AlbumCardMeta>,
}

/// Minimal album metadata for home card display
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumCardMeta {
    pub id: String,
    pub artwork: String,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<u64>,
    pub genre: String,
    pub quality: String,
    pub release_date: Option<String>,
}

/// Minimal track metadata for home display
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDisplayMeta {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_art: String,
    pub album_id: Option<String>,
    pub artist_id: Option<u64>,
    pub duration: String,
    pub duration_seconds: u32,
    pub hires: bool,
    pub bit_depth: Option<u32>,
    pub sampling_rate: Option<f64>,
    pub isrc: Option<String>,
}

/// Minimal artist metadata for home card display
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistCardMeta {
    pub id: u64,
    pub name: String,
    pub image: Option<String>,
    pub play_count: Option<u32>,
}

/// Recommendation store state shared across commands
pub struct RecoState {
    pub db: Arc<Mutex<Option<RecoStoreDb>>>,
}

impl RecoState {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz")
            .join("reco");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create reco directory: {}", e))?;

        let db_path = data_dir.join("events.db");
        let db = RecoStoreDb::new(&db_path)?;

        Ok(Self {
            db: Arc::new(Mutex::new(Some(db))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            db: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn init_at(&self, base_dir: &Path) -> Result<(), String> {
        let reco_dir = base_dir.join("reco");
        std::fs::create_dir_all(&reco_dir)
            .map_err(|e| format!("Failed to create reco directory: {}", e))?;
        let db_path = reco_dir.join("events.db");
        let new_db = RecoStoreDb::new(&db_path)?;
        let mut guard = self.db.lock().await;
        *guard = Some(new_db);
        Ok(())
    }

    pub async fn teardown(&self) {
        let mut guard = self.db.lock().await;
        *guard = None;
    }
}
