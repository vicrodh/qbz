//! QBZ Library - Local music library management
//!
//! Provides functionality for scanning, indexing, and managing local audio files.
//! This crate is completely independent of Tauri and the Qobuz streaming functionality.
//!
//! ## Features
//!
//! - **Scanner**: Recursive directory scanning for audio files
//! - **Metadata**: Audio metadata extraction using lofty
//! - **Database**: SQLite persistence for library data
//! - **CUE Parser**: Support for CUE sheet single-file albums
//! - **Thumbnails**: Artwork extraction and thumbnail generation
//!
//! ## Usage
//!
//! ```no_run
//! use qbz_library::{LibraryScanner, MetadataExtractor, LibraryDatabase};
//! use std::path::Path;
//!
//! // Scan a directory for audio files
//! let scanner = LibraryScanner::new();
//! let result = scanner.scan_directory(Path::new("/path/to/music")).unwrap();
//!
//! // Extract metadata from a file
//! let track = MetadataExtractor::extract(&result.audio_files[0]).unwrap();
//!
//! // Open library database
//! let db = LibraryDatabase::open(Path::new("library.db")).unwrap();
//! ```

mod cue_parser;
mod database;
mod errors;
mod metadata;
mod models;
mod mount_info;
mod scanner;
mod tag_sidecar;
mod thumbnails;

// Re-exports
pub use cue_parser::{cue_to_tracks, CueParser, CueSheet, CueTime, CueTrack};
pub use database::{
    AlbumTrackUpdate, LibraryDatabase, LibraryFolder, LibraryStats, LocalContentStatus,
    PlaylistFolder, PlaylistSettings, PlaylistStats, TrackMetadataUpdateFull,
};
pub use errors::LibraryError;
pub use metadata::MetadataExtractor;
pub use models::*;
pub use mount_info::is_network_path;
pub use scanner::{LibraryScanner, ScanResult};
pub use thumbnails::{
    clear_thumbnails, generate_thumbnail, generate_thumbnail_from_bytes, get_cache_size,
    get_or_generate_thumbnail, get_thumbnail_path, get_thumbnails_dir, thumbnail_exists,
};

// Re-export database module for backwards compatibility
pub mod database_exports {
    pub use crate::database::*;
}
pub use tag_sidecar::*;

use std::path::PathBuf;

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
