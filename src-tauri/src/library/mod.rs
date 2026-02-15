//! Local music library module
//!
//! Re-exports core functionality from qbz-library crate.
//! Commands and remote metadata remain Tauri-specific.

// Tauri-specific modules (stay here)
pub mod commands;
pub mod remote_metadata;

// Re-export everything from qbz-library
pub use qbz_library::{
    // Errors
    LibraryError,
    // Models
    AudioFormat, LocalTrack, LocalAlbum, LocalArtist,
    PlaylistLocalTrack, ScanProgress, ScanStatus, ScanError,
    AudioProperties, AlbumSettings, ArtistImageInfo,
    // Scanner
    LibraryScanner, ScanResult,
    // Metadata
    MetadataExtractor,
    // CUE Parser
    CueParser, CueSheet, CueTrack, CueTime, cue_to_tracks,
    // Database
    LibraryDatabase, LibraryFolder, LibraryStats,
    AlbumTrackUpdate, TrackMetadataUpdateFull,
    PlaylistFolder, PlaylistSettings, PlaylistStats,
    LocalContentStatus,
    // Thumbnails
    generate_thumbnail, generate_thumbnail_from_bytes,
    get_thumbnail_path, get_thumbnails_dir, thumbnail_exists,
    get_or_generate_thumbnail, clear_thumbnails, get_cache_size,
    // Tag sidecar
    AlbumMetadataOverride, TrackMetadataOverride, AlbumTagSidecar,
    sidecar_path, read_album_sidecar, write_album_sidecar,
    delete_album_sidecar, apply_sidecar_to_track,
    // Utility functions
    get_db_path, get_artwork_cache_dir,
};

// Backwards compatibility: re-export as modules
pub mod database {
    pub use qbz_library::database_exports::*;
}

pub mod thumbnails {
    pub use qbz_library::{
        generate_thumbnail, generate_thumbnail_from_bytes,
        get_thumbnail_path, get_thumbnails_dir, thumbnail_exists,
        get_or_generate_thumbnail, clear_thumbnails, get_cache_size,
    };
}

// Re-export commands::LibraryState for compatibility
pub use commands::LibraryState;

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Initialize library state
pub fn init_library_state() -> Result<LibraryState, LibraryError> {
    let db_path = get_db_path();
    let db = LibraryDatabase::open(&db_path)?;

    Ok(LibraryState {
        db: Arc::new(Mutex::new(Some(db))),
        scan_progress: Arc::new(Mutex::new(ScanProgress::default())),
        scan_cancel: Arc::new(AtomicBool::new(false)),
    })
}

/// Initialize library state with no database (for deferred init)
pub fn init_library_state_empty() -> LibraryState {
    LibraryState {
        db: Arc::new(Mutex::new(None)),
        scan_progress: Arc::new(Mutex::new(ScanProgress::default())),
        scan_cancel: Arc::new(AtomicBool::new(false)),
    }
}

/// Initialize library state at a specific directory
pub fn init_library_state_at(base_dir: &Path) -> Result<LibraryState, LibraryError> {
    std::fs::create_dir_all(base_dir)
        .map_err(|e| LibraryError::Database(format!("Failed to create directory: {}", e)))?;
    let db_path = base_dir.join("library.db");
    let db = LibraryDatabase::open(&db_path)?;

    Ok(LibraryState {
        db: Arc::new(Mutex::new(Some(db))),
        scan_progress: Arc::new(Mutex::new(ScanProgress::default())),
        scan_cancel: Arc::new(AtomicBool::new(false)),
    })
}
