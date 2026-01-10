//! Recommendation store module
//!
//! Persists lightweight usage events for home recommendations.

pub mod commands;
pub mod db;

use serde::{Deserialize, Serialize};
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

/// Recommendation store state shared across commands
pub struct RecoState {
    pub db: Arc<Mutex<RecoStoreDb>>,
}

impl RecoState {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz-nix")
            .join("reco");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create reco directory: {}", e))?;

        let db_path = data_dir.join("events.db");
        let db = RecoStoreDb::new(&db_path)?;

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }
}
