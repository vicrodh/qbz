//! Shared DTOs for the offline cache (moved verbatim from
//! `src-tauri/src/offline_cache/mod.rs`). Pure serde — no Tauri, no I/O.

use serde::{Deserialize, Serialize};

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
