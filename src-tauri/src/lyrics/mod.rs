//! Lyrics module
//!
//! Fetches and caches lyrics from public providers.

pub mod cache;
pub mod commands;
pub mod providers;

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use cache::LyricsCacheDb;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsPayload {
    pub track_id: Option<u64>,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: Option<u64>,
    pub plain: Option<String>,
    pub synced_lrc: Option<String>,
    pub provider: LyricsProvider,
    pub cached: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LyricsProvider {
    Lrclib,
    Ovh,
}

impl LyricsProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lrclib => "lrclib",
            Self::Ovh => "ovh",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "ovh" => Self::Ovh,
            _ => Self::Lrclib,
        }
    }
}

/// Lyrics state shared across commands
pub struct LyricsState {
    pub db: Arc<Mutex<Option<LyricsCacheDb>>>,
}

impl LyricsState {
    pub fn new() -> Result<Self, String> {
        let cache_dir = dirs::cache_dir()
            .ok_or("Could not determine cache directory")?
            .join("qbz")
            .join("lyrics");

        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create lyrics cache directory: {}", e))?;

        let db_path = cache_dir.join("lyrics.db");
        let db = LyricsCacheDb::new(&db_path)?;

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
        let cache_dir = base_dir.join("lyrics");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create lyrics cache directory: {}", e))?;
        let db_path = cache_dir.join("lyrics.db");
        let new_db = LyricsCacheDb::new(&db_path)?;
        let mut guard = self.db.lock().await;
        *guard = Some(new_db);
        Ok(())
    }

    pub async fn teardown(&self) {
        let mut guard = self.db.lock().await;
        *guard = None;
    }
}

pub(crate) fn build_cache_key(title: &str, artist: &str, duration_secs: Option<u64>) -> String {
    let normalized_title = normalize(title);
    let normalized_artist = normalize(artist);
    let duration = duration_secs.unwrap_or(0);
    format!("{}::{}::{}", normalized_artist, normalized_title, duration)
}

pub(crate) fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
