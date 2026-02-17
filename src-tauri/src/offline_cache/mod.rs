//! Offline cache module for temporary playback data
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
pub mod migration;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::sync::{Mutex, Semaphore};
use serde::{Deserialize, Serialize};

pub use db::OfflineCacheDb;
pub use downloader::StreamFetcher;
pub use path_validator::{is_offline_root_available, validate_path, PathStatus};
pub use metadata::{CompleteTrackMetadata, sanitize_filename};
pub use migration::{MigrationStatus, MigrationError, detect_legacy_cached_files, migrate_legacy_cached_files};

/// Cache status for a track in offline storage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OfflineCacheStatus {
    Queued,
    Downloading,
    Ready,
    Failed,
}

impl OfflineCacheStatus {
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
    pub status: OfflineCacheStatus,
    pub progress_percent: u8,
    pub error_message: Option<String>,
    pub created_at: String,
    pub last_accessed_at: String,
}

/// Minimal track info for syncing to library
#[derive(Debug, Clone)]
pub struct ReadyTrackForSync {
    pub track_id: u64,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: u64,
    pub file_path: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
}

/// Statistics about the offline cache
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineCacheStats {
    pub total_tracks: usize,
    pub ready_tracks: usize,
    pub downloading_tracks: usize,
    pub failed_tracks: usize,
    pub total_size_bytes: u64,
    pub limit_bytes: Option<u64>,
    pub cache_path: String,
}

/// Progress update for caching a track
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheProgress {
    pub track_id: u64,
    pub progress_percent: u8,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub status: OfflineCacheStatus,
}

/// Track metadata for initiating offline caching
#[derive(Debug, Clone)]
pub struct TrackCacheInfo {
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

/// Offline cache state manager
pub struct OfflineCacheState {
    pub db: Arc<Mutex<Option<OfflineCacheDb>>>,
    pub fetcher: Arc<StreamFetcher>,
    pub cache_dir: Arc<RwLock<PathBuf>>,
    /// Cache limit in bytes (None = unlimited)
    pub limit_bytes: Arc<Mutex<Option<u64>>>,
    pub cache_semaphore: Arc<Semaphore>,
    /// Separate library DB connection for download post-processing writes.
    /// This avoids contending with the main library DB mutex used by UI queries.
    pub library_db: Arc<Mutex<Option<qbz_library::LibraryDatabase>>>,
}

impl OfflineCacheState {
    /// Initialize the offline cache
    pub fn new() -> Result<Self, String> {
        let cache_dir = dirs::cache_dir()
            .ok_or("Could not determine cache directory")?
            .join("qbz")
            .join("audio");

        // Create directories
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
        std::fs::create_dir_all(cache_dir.join("tracks"))
            .map_err(|e| format!("Failed to create tracks directory: {}", e))?;
        std::fs::create_dir_all(cache_dir.join("artwork"))
            .map_err(|e| format!("Failed to create artwork directory: {}", e))?;

        let db_path = cache_dir.join("index.db");
        let db = OfflineCacheDb::new(&db_path)?;

        // Default limit: 2GB
        let default_limit = Some(2 * 1024 * 1024 * 1024u64);

        let state = Self {
            db: Arc::new(Mutex::new(Some(db))),
            fetcher: Arc::new(StreamFetcher::new()),
            cache_dir: Arc::new(RwLock::new(cache_dir.clone())),
            limit_bytes: Arc::new(Mutex::new(default_limit)),
            cache_semaphore: Arc::new(Semaphore::new(3)),
            library_db: Arc::new(Mutex::new(None)),
        };

        log::info!("Offline cache initialized at: {:?}", cache_dir);

        Ok(state)
    }

    pub fn new_empty() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("qbz")
            .join("audio");
        Self {
            db: Arc::new(Mutex::new(None)),
            fetcher: Arc::new(StreamFetcher::new()),
            cache_dir: Arc::new(RwLock::new(cache_dir)),
            limit_bytes: Arc::new(Mutex::new(Some(2 * 1024 * 1024 * 1024u64))),
            cache_semaphore: Arc::new(Semaphore::new(3)),
            library_db: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn init_at(&self, cache_base_dir: &std::path::Path) -> Result<(), String> {
        let cache_dir = cache_base_dir.join("audio");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
        std::fs::create_dir_all(cache_dir.join("tracks"))
            .map_err(|e| format!("Failed to create tracks directory: {}", e))?;
        std::fs::create_dir_all(cache_dir.join("artwork"))
            .map_err(|e| format!("Failed to create artwork directory: {}", e))?;
        let db_path = cache_dir.join("index.db");
        let new_db = OfflineCacheDb::new(&db_path)?;
        let mut guard = self.db.lock().await;
        *guard = Some(new_db);
        // Update cache_dir to user-scoped path
        if let Ok(mut dir_guard) = self.cache_dir.write() {
            *dir_guard = cache_dir.clone();
        }
        log::info!("Offline cache initialized at: {:?}", cache_dir);
        Ok(())
    }

    /// Open a separate library DB connection for download post-processing.
    /// Must be called after library.init_at() so the schema exists.
    pub async fn init_library_connection(&self, data_dir: &std::path::Path) -> Result<(), String> {
        let db_path = data_dir.join("library.db");
        let lib_db = qbz_library::LibraryDatabase::open(&db_path)
            .map_err(|e| format!("Failed to open download library connection: {}", e))?;
        let mut guard = self.library_db.lock().await;
        *guard = Some(lib_db);
        log::info!("Offline cache: separate library DB connection opened at {:?}", db_path);
        Ok(())
    }

    pub async fn teardown(&self) {
        // Close library connection first (before main teardown)
        {
            let mut lib_guard = self.library_db.lock().await;
            *lib_guard = None;
        }
        let mut guard = self.db.lock().await;
        *guard = None;
    }

    /// Get the path for a track's audio file
    pub fn track_file_path(&self, track_id: u64, format: &str) -> PathBuf {
        let dir = self.cache_dir.read().unwrap();
        dir.join("tracks").join(format!("{}.{}", track_id, format))
    }

    /// Get the path for an album's artwork
    pub fn artwork_path(&self, album_id: &str) -> PathBuf {
        let dir = self.cache_dir.read().unwrap();
        dir.join("artwork").join(format!("{}.jpg", album_id))
    }

    /// Get the cache directory path
    pub fn get_cache_path(&self) -> String {
        let dir = self.cache_dir.read().unwrap();
        dir.to_string_lossy().to_string()
    }
}
