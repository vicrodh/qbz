//! Tauri commands for local library

use std::path::Path;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::library::{
    cue_to_tracks, CueParser, LibraryDatabase, LibraryError, LibraryScanner, LibraryStats,
    LocalAlbum, LocalArtist, LocalTrack, MetadataExtractor, ScanError, ScanProgress, ScanStatus,
};

/// Library state shared across commands
pub struct LibraryState {
    pub db: Arc<Mutex<LibraryDatabase>>,
    pub scan_progress: Arc<Mutex<ScanProgress>>,
}

// === Folder Management ===

#[tauri::command]
pub async fn library_add_folder(
    path: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: library_add_folder {}", path);

    // Validate path exists and is a directory
    let path_ref = Path::new(&path);
    if !path_ref.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    if !path_ref.is_dir() {
        return Err(format!("Path is not a directory: {}", path));
    }

    let db = state.db.lock().await;
    db.add_folder(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn library_remove_folder(
    path: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: library_remove_folder {}", path);

    let db = state.db.lock().await;
    db.remove_folder(&path).map_err(|e| e.to_string())?;
    db.delete_tracks_in_folder(&path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn library_get_folders(state: State<'_, LibraryState>) -> Result<Vec<String>, String> {
    log::info!("Command: library_get_folders");

    let db = state.db.lock().await;
    db.get_folders().map_err(|e| e.to_string())
}

// === Scanning ===

#[tauri::command]
pub async fn library_scan(state: State<'_, LibraryState>) -> Result<(), String> {
    log::info!("Command: library_scan");

    // Get folders to scan
    let folders = {
        let db = state.db.lock().await;
        db.get_folders().map_err(|e| e.to_string())?
    };

    if folders.is_empty() {
        return Err("No library folders configured".to_string());
    }

    // Reset progress
    {
        let mut progress = state.scan_progress.lock().await;
        *progress = ScanProgress {
            status: ScanStatus::Scanning,
            total_files: 0,
            processed_files: 0,
            current_file: None,
            errors: Vec::new(),
        };
    }

    let scanner = LibraryScanner::new();
    let mut all_errors: Vec<ScanError> = Vec::new();

    for folder in &folders {
        log::info!("Scanning folder: {}", folder);

        // Scan for files
        let scan_result = match scanner.scan_directory(Path::new(folder)) {
            Ok(result) => result,
            Err(e) => {
                all_errors.push(ScanError {
                    file_path: folder.clone(),
                    error: e.to_string(),
                });
                continue;
            }
        };

        let total_files = scan_result.audio_files.len() + scan_result.cue_files.len();

        // Update total count
        {
            let mut progress = state.scan_progress.lock().await;
            progress.total_files += total_files as u32;
        }

        // Process CUE files first (they create multiple tracks from one file)
        for cue_path in &scan_result.cue_files {
            {
                let mut progress = state.scan_progress.lock().await;
                progress.current_file = Some(cue_path.to_string_lossy().to_string());
            }

            match process_cue_file(cue_path, &state).await {
                Ok(_) => {}
                Err(e) => {
                    all_errors.push(ScanError {
                        file_path: cue_path.to_string_lossy().to_string(),
                        error: e,
                    });
                }
            }

            {
                let mut progress = state.scan_progress.lock().await;
                progress.processed_files += 1;
            }
        }

        // Process regular audio files (skip if covered by CUE)
        let cue_audio_files: std::collections::HashSet<_> = scan_result
            .cue_files
            .iter()
            .filter_map(|p| CueParser::parse(p).ok().map(|cue| cue.audio_file))
            .collect();

        for audio_path in &scan_result.audio_files {
            // Skip if this file is referenced by a CUE sheet
            let path_str = audio_path.to_string_lossy().to_string();
            if cue_audio_files.contains(&path_str) {
                let mut progress = state.scan_progress.lock().await;
                progress.processed_files += 1;
                continue;
            }

            {
                let mut progress = state.scan_progress.lock().await;
                progress.current_file = Some(path_str.clone());
            }

            match MetadataExtractor::extract(audio_path) {
                Ok(track) => {
                    let db = state.db.lock().await;
                    if let Err(e) = db.insert_track(&track) {
                        all_errors.push(ScanError {
                            file_path: path_str,
                            error: e.to_string(),
                        });
                    }
                }
                Err(e) => {
                    all_errors.push(ScanError {
                        file_path: path_str,
                        error: e.to_string(),
                    });
                }
            }

            {
                let mut progress = state.scan_progress.lock().await;
                progress.processed_files += 1;
            }
        }
    }

    // Mark complete
    {
        let mut progress = state.scan_progress.lock().await;
        progress.status = if all_errors.is_empty() {
            ScanStatus::Complete
        } else {
            ScanStatus::Complete // Still complete, but with errors
        };
        progress.current_file = None;
        progress.errors = all_errors;
    }

    log::info!("Library scan complete");
    Ok(())
}

/// Process a CUE file and insert its tracks
async fn process_cue_file(cue_path: &Path, state: &State<'_, LibraryState>) -> Result<(), String> {
    let cue = CueParser::parse(cue_path).map_err(|e| e.to_string())?;

    // Get audio file properties
    let audio_path = Path::new(&cue.audio_file);
    if !audio_path.exists() {
        return Err(format!("Audio file not found: {}", cue.audio_file));
    }

    let properties =
        MetadataExtractor::extract_properties(audio_path).map_err(|e| e.to_string())?;
    let format = MetadataExtractor::detect_format(audio_path);

    // Convert CUE to tracks
    let tracks = cue_to_tracks(&cue, properties.duration_secs, format, &properties);

    // Insert tracks
    let db = state.db.lock().await;
    for track in tracks {
        db.insert_track(&track).map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn library_get_scan_progress(
    state: State<'_, LibraryState>,
) -> Result<ScanProgress, String> {
    let progress = state.scan_progress.lock().await;
    Ok(progress.clone())
}

// === Queries ===

#[tauri::command]
pub async fn library_get_albums(
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, LibraryState>,
) -> Result<Vec<LocalAlbum>, String> {
    log::info!("Command: library_get_albums");

    let db = state.db.lock().await;
    db.get_albums(limit.unwrap_or(50), offset.unwrap_or(0))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn library_get_album_tracks(
    album: String,
    artist: String,
    state: State<'_, LibraryState>,
) -> Result<Vec<LocalTrack>, String> {
    log::info!("Command: library_get_album_tracks {} - {}", artist, album);

    let db = state.db.lock().await;
    db.get_album_tracks(&album, &artist)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn library_get_artists(state: State<'_, LibraryState>) -> Result<Vec<LocalArtist>, String> {
    log::info!("Command: library_get_artists");

    let db = state.db.lock().await;
    db.get_artists().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn library_search(
    query: String,
    limit: Option<u32>,
    state: State<'_, LibraryState>,
) -> Result<Vec<LocalTrack>, String> {
    log::info!("Command: library_search \"{}\"", query);

    let db = state.db.lock().await;
    db.search(&query, limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn library_get_stats(state: State<'_, LibraryState>) -> Result<LibraryStats, String> {
    log::info!("Command: library_get_stats");

    let db = state.db.lock().await;
    db.get_stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn library_clear(state: State<'_, LibraryState>) -> Result<(), String> {
    log::info!("Command: library_clear");

    let db = state.db.lock().await;
    db.clear_all_tracks().map_err(|e| e.to_string())
}

// === Playback ===

#[tauri::command]
pub async fn library_get_track(
    track_id: i64,
    state: State<'_, LibraryState>,
) -> Result<LocalTrack, String> {
    log::info!("Command: library_get_track {}", track_id);

    let db = state.db.lock().await;
    db.get_track(track_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Track not found".to_string())
}

/// Play a local track by ID
#[tauri::command]
pub async fn library_play_track(
    track_id: i64,
    library_state: State<'_, LibraryState>,
    app_state: State<'_, crate::AppState>,
) -> Result<(), String> {
    log::info!("Command: library_play_track {}", track_id);

    // Get track from database
    let track = {
        let db = library_state.db.lock().await;
        db.get_track(track_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Track not found".to_string())?
    };

    // Read file from disk
    let file_path = Path::new(&track.file_path);
    if !file_path.exists() {
        return Err(format!("File not found: {}", track.file_path));
    }

    let audio_data = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    log::info!(
        "Playing local track: {} - {} ({} bytes)",
        track.artist,
        track.title,
        audio_data.len()
    );

    // Play the audio (use track_id as u64 for player identification)
    app_state
        .player
        .play_data(audio_data, track_id as u64)
        .map_err(|e| format!("Failed to play: {}", e))?;

    // If this is a CUE track, seek to the start position
    if let Some(start_secs) = track.cue_start_secs {
        let start_pos = start_secs as u64;
        if start_pos > 0 {
            log::info!("CUE track: seeking to {} seconds", start_pos);
            // Small delay to ensure playback has started
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            app_state
                .player
                .seek(start_pos)
                .map_err(|e| format!("Failed to seek: {}", e))?;
        }
    }

    Ok(())
}
