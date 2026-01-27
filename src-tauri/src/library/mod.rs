//! Local music library module
//!
//! Provides functionality for scanning, indexing, and playing local audio files.
//! This module is completely independent of the Qobuz streaming functionality.

pub mod commands;
pub mod cue_parser;
pub mod database;
pub mod errors;
pub mod metadata;
pub mod models;
pub mod remote_metadata;
pub mod scanner;
pub mod tag_sidecar;
pub mod thumbnails;

pub use commands::LibraryState;
pub use cue_parser::{cue_to_tracks, CueParser, CueSheet, CueTrack};
pub use database::{
    AlbumTrackUpdate, LibraryDatabase, LibraryFolder, LibraryStats, PlaylistFolder, PlaylistSettings,
    PlaylistStats, TrackMetadataUpdateFull,
};
pub use errors::LibraryError;
pub use metadata::MetadataExtractor;
pub use models::*;
pub use tag_sidecar::*;
pub use scanner::{LibraryScanner, ScanResult};

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Get library database path in app data directory
pub fn get_db_path() -> PathBuf {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("qbz");
    std::fs::create_dir_all(&data_dir).ok();
    data_dir.join("library.db")
}

/// Get artwork cache directory
pub fn get_artwork_cache_dir() -> PathBuf {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("qbz")
        .join("artwork");
    std::fs::create_dir_all(&cache_dir).ok();
    cache_dir
}

/// Initialize library state
pub fn init_library_state() -> Result<LibraryState, LibraryError> {
    let db_path = get_db_path();
    let db = LibraryDatabase::open(&db_path)?;

    Ok(LibraryState {
        db: Arc::new(Mutex::new(db)),
        scan_progress: Arc::new(Mutex::new(ScanProgress::default())),
        scan_cancel: Arc::new(AtomicBool::new(false)),
    })
}
