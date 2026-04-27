//! Local music library module
//!
//! Re-exports core functionality from qbz-library crate.
//! Commands and remote metadata remain Tauri-specific.

// Tauri-specific modules (stay here)
pub mod commands;
pub mod remote_metadata;
pub mod state;

// Re-export everything from qbz-library
pub use qbz_library::{
    apply_sidecar_to_track,
    clear_thumbnails,
    cue_to_tracks,
    delete_album_sidecar,
    // Thumbnails
    generate_thumbnail,
    generate_thumbnail_from_bytes,
    get_artwork_cache_dir,
    get_cache_size,
    // Utility functions
    get_db_path,
    get_or_generate_thumbnail,
    get_thumbnail_path,
    get_thumbnails_dir,
    read_album_sidecar,
    sidecar_path,
    thumbnail_exists,
    write_album_sidecar,
    // Tag sidecar
    AlbumMetadataOverride,
    AlbumSettings,
    AlbumTagSidecar,
    AlbumTrackUpdate,
    ArtistImageInfo,
    // Models
    AudioFormat,
    AudioProperties,
    // CUE Parser
    CueParser,
    CueSheet,
    CueTime,
    CueTrack,
    // Database
    LibraryDatabase,
    // Errors
    LibraryError,
    LibraryFolder,
    // Scanner
    LibraryScanner,
    LibraryStats,
    LocalAlbum,
    LocalArtist,
    LocalContentStatus,
    LocalTrack,
    // Metadata
    MetadataExtractor,
    PlaylistFolder,
    PlaylistLocalTrack,
    PlaylistSettings,
    PlaylistStats,
    ScanError,
    ScanProgress,
    ScanResult,
    ScanStatus,
    TrackMetadataOverride,
    TrackMetadataUpdateFull,
};

// Backwards compatibility: re-export as modules
pub mod database {
    pub use qbz_library::database_exports::*;
}

pub mod thumbnails {
    pub use qbz_library::{
        clear_thumbnails, generate_thumbnail, generate_thumbnail_from_bytes, get_cache_size,
        get_or_generate_thumbnail, get_thumbnail_path, get_thumbnails_dir, thumbnail_exists,
    };
}

// State and DTO types live in `state`; re-export for compatibility.
pub use state::{
    BackfillReport, CleanupResult, LibraryAlbumMetadataUpdateRequest,
    LibraryAlbumTrackMetadataUpdate, LibraryState,
};
pub use commands::{
    library_fetch_missing_artwork, library_get_album_tracks, library_get_artists,
    library_get_scan_progress, library_get_stats, library_get_thumbnail,
    library_get_thumbnails_cache_size, library_get_tracks_by_ids, library_search,
    playlist_get_custom_order, playlist_get_tracks_with_local_copies, playlist_has_custom_order,
    update_playlist_folder,
};

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
