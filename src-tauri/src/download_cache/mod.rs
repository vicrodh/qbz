//! Download cache module for offline listening
//!
//! Provides persistent disk-based caching for audio tracks:
//! - SQLite index for track metadata
//! - File-based storage for audio data
//! - LRU eviction with configurable limits
//! - Progress events for UI updates

pub mod commands;
pub mod db;
pub mod downloader;
pub mod path_validator;
pub mod metadata;

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use serde::{Deserialize, Serialize};

pub use db::DownloadCacheDb;
pub use downloader::Downloader;
pub use path_validator::{is_download_root_available, validate_path, PathStatus};
pub use metadata::{CompleteTrackMetadata, sanitize_filename};

/// Download status for a cached track
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Ready,
    Failed,
}

impl DownloadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "queued" => Self::Queued,
            "downloading" => Self::Downloading,
            "ready" => Self::Ready,
            "failed" => Self::Failed,
            _ => Self::Failed,
        }
    }
}

/// Information about a cached track
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedTrackInfo {
    pub track_id: u64,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub duration_secs: u64,
    pub file_size_bytes: u64,
    pub quality: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
    pub status: DownloadStatus,
    pub progress_percent: u8,
    pub error_message: Option<String>,
    pub created_at: String,
    pub last_accessed_at: String,
}

/// Statistics about the download cache
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCacheStats {
    pub total_tracks: usize,
    pub ready_tracks: usize,
    pub downloading_tracks: usize,
    pub failed_tracks: usize,
    pub total_size_bytes: u64,
    pub limit_bytes: Option<u64>,
    pub cache_path: String,
}

/// Progress update for a download
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub track_id: u64,
    pub progress_percent: u8,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub status: DownloadStatus,
}

/// Track metadata for initiating a download
#[derive(Debug, Clone)]
pub struct TrackDownloadInfo {
    pub track_id: u64,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub duration_secs: u64,
    pub quality: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
}

/// Download cache state manager
pub struct DownloadCacheState {
    pub db: Arc<Mutex<DownloadCacheDb>>,
    pub downloader: Arc<Downloader>,
    pub cache_dir: PathBuf,
    /// Cache limit in bytes (None = unlimited)
    pub limit_bytes: Arc<Mutex<Option<u64>>>,
    pub download_semaphore: Arc<Semaphore>,
}

impl DownloadCacheState {
    /// Initialize the download cache
    pub fn new() -> Result<Self, String> {
        let cache_dir = dirs::cache_dir()
            .ok_or("Could not determine cache directory")?
            .join("qbz-nix")
            .join("audio");

        // Create directories
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
        std::fs::create_dir_all(cache_dir.join("tracks"))
            .map_err(|e| format!("Failed to create tracks directory: {}", e))?;
        std::fs::create_dir_all(cache_dir.join("artwork"))
            .map_err(|e| format!("Failed to create artwork directory: {}", e))?;

        let db_path = cache_dir.join("index.db");
        let db = DownloadCacheDb::new(&db_path)?;

        // Default limit: 2GB
        let default_limit = Some(2 * 1024 * 1024 * 1024u64);

        let state = Self {
            db: Arc::new(Mutex::new(db)),
            downloader: Arc::new(Downloader::new()),
            cache_dir: cache_dir.clone(),
            limit_bytes: Arc::new(Mutex::new(default_limit)),
            download_semaphore: Arc::new(Semaphore::new(3)),
        };

        log::info!("Download cache initialized at: {:?}", cache_dir);

        Ok(state)
    }

    /// Get the path for a track's audio file
    pub fn track_file_path(&self, track_id: u64, format: &str) -> PathBuf {
        self.cache_dir.join("tracks").join(format!("{}.{}", track_id, format))
    }

    /// Get the path for an album's artwork
    pub fn artwork_path(&self, album_id: &str) -> PathBuf {
        self.cache_dir.join("artwork").join(format!("{}.jpg", album_id))
    }

    /// Get the cache directory path
    pub fn get_cache_path(&self) -> String {
        self.cache_dir.to_string_lossy().to_string()
    }
}
