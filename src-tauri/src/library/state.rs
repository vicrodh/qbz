//! Shared state and DTO types for the library module.
//!
//! These types were originally defined in `library/commands.rs` alongside
//! their owning Tauri commands. After legacy cleanup the commands module
//! has been deleted and these types live here. Consumers reach them via
//! the re-exports in `library/mod.rs` (`crate::library::LibraryState`).

use serde::Deserialize;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::library::{LibraryDatabase, ScanProgress};

/// Library state shared across commands
pub struct LibraryState {
    pub db: Arc<Mutex<Option<LibraryDatabase>>>,
    pub scan_progress: Arc<Mutex<ScanProgress>>,
    pub scan_cancel: Arc<AtomicBool>,
}

impl LibraryState {
    pub async fn init_at(&self, base_dir: &std::path::Path) -> Result<(), String> {
        std::fs::create_dir_all(base_dir)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
        let db_path = base_dir.join("library.db");
        let db = LibraryDatabase::open(&db_path).map_err(|e| e.to_string())?;
        let mut guard = self.db.lock().await;
        *guard = Some(db);
        Ok(())
    }

    pub async fn teardown(&self) {
        let mut guard = self.db.lock().await;
        *guard = None;
    }
}

/// Result of cleanup operation
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub checked: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAlbumTrackMetadataUpdate {
    pub id: i64,
    pub file_path: String,
    pub cue_start_secs: Option<f64>,
    pub title: String,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAlbumMetadataUpdateRequest {
    pub album_group_key: String,
    pub album_title: String,
    pub album_artist: String,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub catalog_number: Option<String>,
    pub tracks: Vec<LibraryAlbumTrackMetadataUpdate>,
}

#[derive(serde::Serialize)]
pub struct BackfillReport {
    pub total_downloads: usize,
    pub added_tracks: usize,
    pub repaired_tracks: usize,
    pub skipped_tracks: usize,
    pub failed_tracks: Vec<String>,
}
