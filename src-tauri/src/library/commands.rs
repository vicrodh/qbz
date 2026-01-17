//! Tauri commands for local library

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::discogs::DiscogsClient;
use crate::library::{
    cue_to_tracks, get_artwork_cache_dir, CueParser, LibraryDatabase, LibraryFolder, LibraryScanner, LibraryStats,
    LocalAlbum, LocalArtist, LocalTrack, MetadataExtractor, ScanError, ScanProgress, ScanStatus,
};
use crate::network::{is_network_path, MountKind, NetworkFs};

/// Library state shared across commands
pub struct LibraryState {
    pub db: Arc<Mutex<LibraryDatabase>>,
    pub scan_progress: Arc<Mutex<ScanProgress>>,
    pub scan_cancel: Arc<AtomicBool>,
}

fn normalize_library_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

// === Folder Management ===

#[tauri::command]
pub async fn library_add_folder(
    path: String,
    state: State<'_, LibraryState>,
) -> Result<LibraryFolder, String> {
    log::info!("Command: library_add_folder {}", path);

    // Validate path exists and is a directory
    let path_ref = Path::new(&path);
    if !path_ref.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    if !path_ref.is_dir() {
        return Err(format!("Path is not a directory: {}", path));
    }

    // Detect if this is a network folder
    let network_info = is_network_path(path_ref);
    let (is_network, fs_type) = if network_info.is_network {
        let fs_type = network_info.mount_info.as_ref().and_then(|m| {
            match &m.kind {
                MountKind::Network(nfs) => Some(match nfs {
                    NetworkFs::Cifs => "cifs".to_string(),
                    NetworkFs::Nfs => "nfs".to_string(),
                    NetworkFs::Sshfs => "sshfs".to_string(),
                    NetworkFs::Rclone => "rclone".to_string(),
                    NetworkFs::Webdav => "webdav".to_string(),
                    NetworkFs::Gluster => "glusterfs".to_string(),
                    NetworkFs::Ceph => "ceph".to_string(),
                    NetworkFs::Other(s) => s.clone(),
                }),
                _ => None,
            }
        });
        (true, fs_type)
    } else {
        (false, None)
    };

    log::info!("Folder network info: is_network={}, fs_type={:?}", is_network, fs_type);

    let db = state.db.lock().await;
    let id = db.add_folder_with_network_info(&path, is_network, fs_type.as_deref())
        .map_err(|e| e.to_string())?;

    // Return the full folder info
    db.get_folder_by_id(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Failed to retrieve folder after insert".to_string())
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

/// Get all library folders with full metadata
/// Also refreshes network detection for folders without user override
#[tauri::command]
pub async fn library_get_folders_with_metadata(
    state: State<'_, LibraryState>,
) -> Result<Vec<LibraryFolder>, String> {
    log::info!("Command: library_get_folders_with_metadata");

    let db = state.db.lock().await;
    let mut folders = db.get_folders_with_metadata().map_err(|e| e.to_string())?;

    // Refresh network detection for folders without user override
    for folder in &mut folders {
        if !folder.user_override_network {
            let path = Path::new(&folder.path);
            let network_info = crate::network::is_network_path(path);

            // Update if detection differs from stored value
            if network_info.is_network != folder.is_network {
                log::info!(
                    "Updating network status for folder {}: {} -> {}",
                    folder.path,
                    folder.is_network,
                    network_info.is_network
                );

                // Extract network filesystem type
                let fs_type = network_info.mount_info.as_ref().and_then(|mi| {
                    if let crate::network::MountKind::Network(nfs) = &mi.kind {
                        Some(format!("{:?}", nfs).to_lowercase())
                    } else {
                        None
                    }
                });

                // Update database
                let _ = db.update_folder_settings(
                    folder.id,
                    folder.alias.as_deref(),
                    folder.enabled,
                    network_info.is_network,
                    fs_type.as_deref(),
                    false, // not user override
                );

                // Update the folder struct for return
                folder.is_network = network_info.is_network;
                folder.network_fs_type = fs_type;
            }
        }
    }

    Ok(folders)
}

/// Get a single folder by ID
#[tauri::command]
pub async fn library_get_folder(
    id: i64,
    state: State<'_, LibraryState>,
) -> Result<Option<LibraryFolder>, String> {
    log::info!("Command: library_get_folder {}", id);

    let db = state.db.lock().await;
    db.get_folder_by_id(id).map_err(|e| e.to_string())
}

/// Update folder settings (alias, enabled, network info)
#[tauri::command]
pub async fn library_update_folder_settings(
    id: i64,
    alias: Option<String>,
    enabled: bool,
    is_network: bool,
    network_fs_type: Option<String>,
    user_override_network: bool,
    state: State<'_, LibraryState>,
) -> Result<LibraryFolder, String> {
    log::info!("Command: library_update_folder_settings {} alias={:?} enabled={}", id, alias, enabled);

    let db = state.db.lock().await;
    db.update_folder_settings(
        id,
        alias.as_deref(),
        enabled,
        is_network,
        network_fs_type.as_deref(),
        user_override_network,
    ).map_err(|e| e.to_string())?;

    db.get_folder_by_id(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Folder not found after update".to_string())
}

/// Enable or disable a folder
#[tauri::command]
pub async fn library_set_folder_enabled(
    id: i64,
    enabled: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: library_set_folder_enabled {} enabled={}", id, enabled);

    let db = state.db.lock().await;
    db.set_folder_enabled(id, enabled).map_err(|e| e.to_string())
}

/// Update folder path (move folder to new location)
#[tauri::command]
pub async fn library_update_folder_path(
    id: i64,
    new_path: String,
    state: State<'_, LibraryState>,
) -> Result<LibraryFolder, String> {
    log::info!("Command: library_update_folder_path {} -> {}", id, new_path);

    // Verify the new path exists and is a directory
    let path_ref = Path::new(&new_path);
    if !path_ref.exists() {
        return Err("The selected folder does not exist".to_string());
    }
    if !path_ref.is_dir() {
        return Err("The selected path is not a folder".to_string());
    }

    let db = state.db.lock().await;
    db.update_folder_path(id, &new_path).map_err(|e| e.to_string())?;

    // Check if it's a network folder and update network info
    let network_info = crate::network::is_network_path(path_ref);
    if network_info.is_network {
        // Extract network filesystem type from mount_info
        let fs_type = network_info.mount_info.as_ref().and_then(|mi| {
            if let crate::network::MountKind::Network(nfs) = &mi.kind {
                Some(format!("{:?}", nfs).to_lowercase())
            } else {
                None
            }
        });

        // Get current folder settings to preserve enabled state
        let current = db.get_folder_by_id(id).map_err(|e| e.to_string())?;
        if let Some(folder) = current {
            db.update_folder_settings(
                id,
                folder.alias.as_deref(),
                folder.enabled,
                true, // is_network
                fs_type.as_deref(),
                false, // not user override
            ).map_err(|e| e.to_string())?;
        }
    }

    db.get_folder_by_id(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Folder not found after update".to_string())
}

/// Check network accessibility for a folder
#[tauri::command]
pub async fn library_check_folder_accessible(path: String) -> Result<bool, String> {
    log::info!("Command: library_check_folder_accessible {}", path);

    let path_ref = Path::new(&path);
    if !path_ref.exists() {
        return Ok(false);
    }

    // Try to read the directory
    match std::fs::read_dir(path_ref) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
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

    // Reset cancel flag and progress
    state.scan_cancel.store(false, Ordering::Relaxed);
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
            // Check for cancellation
            if state.scan_cancel.load(Ordering::Relaxed) {
                let mut progress = state.scan_progress.lock().await;
                progress.status = ScanStatus::Cancelled;
                progress.current_file = None;
                log::info!("Library scan cancelled by user");
                return Ok(());
            }

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
        let cue_audio_files: std::collections::HashSet<String> = scan_result
            .cue_files
            .iter()
            .filter_map(|p| {
                CueParser::parse(p).ok().map(|cue| {
                    normalize_library_path(Path::new(&cue.audio_file))
                        .to_string_lossy()
                        .to_string()
                })
            })
            .collect();

        for audio_path in &scan_result.audio_files {
            // Check for cancellation
            if state.scan_cancel.load(Ordering::Relaxed) {
                let mut progress = state.scan_progress.lock().await;
                progress.status = ScanStatus::Cancelled;
                progress.current_file = None;
                log::info!("Library scan cancelled by user");
                return Ok(());
            }

            // Skip if this file is referenced by a CUE sheet
            let canonical_path = normalize_library_path(audio_path);
            let path_str = canonical_path.to_string_lossy().to_string();
            if cue_audio_files.contains(&path_str) {
                let mut progress = state.scan_progress.lock().await;
                progress.processed_files += 1;
                continue;
            }

            {
                let mut progress = state.scan_progress.lock().await;
                progress.current_file = Some(path_str.clone());
            }

            match MetadataExtractor::extract(&canonical_path) {
                Ok(mut track) => {
                    // Try to extract embedded artwork, fallback to cached folder artwork
                    let artwork_cache = get_artwork_cache_dir();
                    let mut artwork_path =
                        MetadataExtractor::extract_artwork(&canonical_path, &artwork_cache);
                    if artwork_path.is_none() {
                        let album_hint = if !track.album_group_title.is_empty() {
                            Some(track.album_group_title.as_str())
                        } else {
                            Some(track.album.as_str())
                        };
                        if let Some(folder_art) =
                            MetadataExtractor::find_folder_artwork(&canonical_path, album_hint)
                        {
                            artwork_path = MetadataExtractor::cache_artwork_file(
                                Path::new(&folder_art),
                                &artwork_cache,
                            );
                        }
                    }
                    track.artwork_path = artwork_path;

                    let db = state.db.lock().await;
                    if let Err(e) = db.insert_track(&track) {
                        all_errors.push(ScanError {
                            file_path: path_str,
                            error: e.to_string(),
                        });
                    } else if let (Some(artwork_path), false) =
                        (track.artwork_path.as_ref(), track.album_group_key.is_empty())
                    {
                        let _ = db.update_album_group_artwork(&track.album_group_key, artwork_path);
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
    let mut cue = CueParser::parse(cue_path).map_err(|e| e.to_string())?;

    // Get audio file properties
    let audio_path = normalize_library_path(Path::new(&cue.audio_file));
    if !audio_path.exists() {
        return Err(format!("Audio file not found: {}", cue.audio_file));
    }
    cue.audio_file = audio_path.to_string_lossy().to_string();

    let properties =
        MetadataExtractor::extract_properties(audio_path.as_path()).map_err(|e| e.to_string())?;
    let format = MetadataExtractor::detect_format(audio_path.as_path());

    // Convert CUE to tracks
    let mut tracks = cue_to_tracks(&cue, properties.duration_secs, format, &properties);

    let artwork_cache = get_artwork_cache_dir();
    let mut artwork_path = MetadataExtractor::extract_artwork(audio_path.as_path(), &artwork_cache);
    if artwork_path.is_none() {
        if let Some(folder_art) = MetadataExtractor::find_folder_artwork(
            audio_path.as_path(),
            cue.title.as_deref(),
        ) {
            artwork_path = MetadataExtractor::cache_artwork_file(
                Path::new(&folder_art),
                &artwork_cache,
            );
        }
    }
    if let Some(path) = artwork_path.as_ref() {
        for track in tracks.iter_mut() {
            track.artwork_path = Some(path.clone());
        }
    }

    // Insert tracks
    let db = state.db.lock().await;
    let group_key = tracks
        .first()
        .map(|track| track.album_group_key.clone())
        .unwrap_or_default();

    for track in tracks {
        db.insert_track(&track).map_err(|e| e.to_string())?;
    }

    if let (Some(path), false) = (artwork_path.as_ref(), group_key.is_empty()) {
        let _ = db.update_album_group_artwork(&group_key, path);
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

#[tauri::command]
pub async fn library_stop_scan(state: State<'_, LibraryState>) -> Result<(), String> {
    log::info!("Command: library_stop_scan");
    state.scan_cancel.store(true, Ordering::Relaxed);
    Ok(())
}

/// Scan a single folder (by ID)
#[tauri::command]
pub async fn library_scan_folder(
    folder_id: i64,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: library_scan_folder {}", folder_id);

    // Get folder info
    let folder = {
        let db = state.db.lock().await;
        db.get_folder_by_id(folder_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Folder with ID {} not found", folder_id))?
    };

    if !folder.enabled {
        return Err("Cannot scan disabled folder".to_string());
    }

    // Refresh network detection if not user overridden
    if !folder.user_override_network {
        let path = Path::new(&folder.path);
        let network_info = crate::network::is_network_path(path);

        if network_info.is_network != folder.is_network {
            log::info!(
                "Updating network status for folder {} during scan: {} -> {}",
                folder.path,
                folder.is_network,
                network_info.is_network
            );

            let fs_type = network_info.mount_info.as_ref().and_then(|mi| {
                if let crate::network::MountKind::Network(nfs) = &mi.kind {
                    Some(format!("{:?}", nfs).to_lowercase())
                } else {
                    None
                }
            });

            let db = state.db.lock().await;
            let _ = db.update_folder_settings(
                folder.id,
                folder.alias.as_deref(),
                folder.enabled,
                network_info.is_network,
                fs_type.as_deref(),
                false,
            );
        }
    }

    // Reset cancel flag and progress
    state.scan_cancel.store(false, Ordering::Relaxed);
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

    log::info!("Scanning single folder: {}", folder.path);

    // Scan for files
    let scan_result = match scanner.scan_directory(Path::new(&folder.path)) {
        Ok(result) => result,
        Err(e) => {
            let mut progress = state.scan_progress.lock().await;
            progress.status = ScanStatus::Complete;
            progress.errors = vec![ScanError {
                file_path: folder.path.clone(),
                error: e.to_string(),
            }];
            return Err(e.to_string());
        }
    };

    let total_files = scan_result.audio_files.len() + scan_result.cue_files.len();

    // Update total count
    {
        let mut progress = state.scan_progress.lock().await;
        progress.total_files = total_files as u32;
    }

    // Process CUE files first
    for cue_path in &scan_result.cue_files {
        if state.scan_cancel.load(Ordering::Relaxed) {
            let mut progress = state.scan_progress.lock().await;
            progress.status = ScanStatus::Cancelled;
            progress.current_file = None;
            log::info!("Library scan cancelled by user");
            return Ok(());
        }

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

    // Process regular audio files
    let cue_audio_files: std::collections::HashSet<String> = scan_result
        .cue_files
        .iter()
        .filter_map(|p| {
            CueParser::parse(p).ok().map(|cue| {
                normalize_library_path(Path::new(&cue.audio_file))
                    .to_string_lossy()
                    .to_string()
            })
        })
        .collect();

    for audio_path in &scan_result.audio_files {
        if state.scan_cancel.load(Ordering::Relaxed) {
            let mut progress = state.scan_progress.lock().await;
            progress.status = ScanStatus::Cancelled;
            progress.current_file = None;
            log::info!("Library scan cancelled by user");
            return Ok(());
        }

        let canonical_path = normalize_library_path(audio_path);
        let path_str = canonical_path.to_string_lossy().to_string();
        if cue_audio_files.contains(&path_str) {
            let mut progress = state.scan_progress.lock().await;
            progress.processed_files += 1;
            continue;
        }

        {
            let mut progress = state.scan_progress.lock().await;
            progress.current_file = Some(path_str.clone());
        }

        match MetadataExtractor::extract(&canonical_path) {
            Ok(mut track) => {
                let artwork_cache = get_artwork_cache_dir();
                let mut artwork_path = MetadataExtractor::extract_artwork(&canonical_path, &artwork_cache);
                if artwork_path.is_none() {
                    if let Some(folder_art) = MetadataExtractor::find_folder_artwork(
                        canonical_path.as_path(),
                        Some(&track.album),
                    ) {
                        artwork_path = MetadataExtractor::cache_artwork_file(
                            Path::new(&folder_art),
                            &artwork_cache,
                        );
                    }
                }
                track.artwork_path = artwork_path.clone();

                let db = state.db.lock().await;
                let group_key = track.album_group_key.clone();
                if let Err(e) = db.insert_track(&track) {
                    all_errors.push(ScanError {
                        file_path: path_str,
                        error: e.to_string(),
                    });
                } else if let Some(path) = artwork_path.as_ref() {
                    if !group_key.is_empty() {
                        let _ = db.update_album_group_artwork(&group_key, path);
                    }
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

    // Update folder scan time
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let db = state.db.lock().await;
        let _ = db.update_folder_scan_time(&folder.path, now);
    }

    // Update final status
    {
        let mut progress = state.scan_progress.lock().await;
        progress.status = ScanStatus::Complete;
        progress.current_file = None;
        progress.errors = all_errors;
    }

    log::info!("Single folder scan complete");
    Ok(())
}

// === Queries ===

#[tauri::command]
pub async fn library_get_albums(
    include_hidden: Option<bool>,
    state: State<'_, LibraryState>,
    download_settings_state: State<'_, crate::config::DownloadSettingsState>,
) -> Result<Vec<LocalAlbum>, String> {
    log::info!("Command: library_get_albums");

    // Get download settings to check if we should include Qobuz downloads
    let include_qobuz = download_settings_state
        .lock()
        .map_err(|e| format!("Failed to lock download settings: {}", e))?
        .get_settings()
        .map(|s| s.show_in_library)
        .unwrap_or(false);

    let db = state.db.lock().await;
    db.get_albums_with_filter(include_hidden.unwrap_or(false), include_qobuz)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn library_get_album_tracks(
    album_group_key: String,
    state: State<'_, LibraryState>,
) -> Result<Vec<LocalTrack>, String> {
    log::info!("Command: library_get_album_tracks {}", album_group_key);

    let db = state.db.lock().await;
    db.get_album_tracks(&album_group_key)
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

// === Playlist Local Settings ===

use crate::library::{PlaylistSettings, PlaylistStats};

/// Get playlist settings by Qobuz playlist ID
#[tauri::command]
pub async fn playlist_get_settings(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<Option<PlaylistSettings>, String> {
    log::info!("Command: playlist_get_settings {}", playlist_id);

    let db = state.db.lock().await;
    db.get_playlist_settings(playlist_id)
        .map_err(|e| e.to_string())
}

/// Save or update playlist settings
#[tauri::command]
pub async fn playlist_save_settings(
    settings: PlaylistSettings,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_save_settings {}", settings.qobuz_playlist_id);

    let db = state.db.lock().await;
    db.save_playlist_settings(&settings)
        .map_err(|e| e.to_string())
}

/// Update playlist sort settings
#[tauri::command]
pub async fn playlist_set_sort(
    playlist_id: u64,
    sort_by: String,
    sort_order: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_set_sort {} {} {}", playlist_id, sort_by, sort_order);

    let db = state.db.lock().await;
    db.update_playlist_sort(playlist_id, &sort_by, &sort_order)
        .map_err(|e| e.to_string())
}

/// Update playlist custom artwork
#[tauri::command]
pub async fn playlist_set_artwork(
    playlist_id: u64,
    artwork_path: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_set_artwork {}", playlist_id);

    let final_path = if let Some(source_path) = artwork_path {
        // Copy image to persistent location
        let artwork_dir = get_artwork_cache_dir();
        let source = Path::new(&source_path);
        
        if !source.exists() {
            return Err(format!("Source image does not exist: {}", source_path));
        }

        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");
        let filename = format!("playlist_{}_{}.{}", playlist_id, chrono::Utc::now().timestamp(), extension);
        let dest_path = artwork_dir.join(filename);

        fs::copy(source, &dest_path)
            .map_err(|e| format!("Failed to copy artwork: {}", e))?;
        
        log::info!("Copied playlist artwork to: {}", dest_path.display());
        Some(dest_path.to_string_lossy().to_string())
    } else {
        None
    };

    let db = state.db.lock().await;
    db.update_playlist_artwork(playlist_id, final_path.as_deref())
        .map_err(|e| e.to_string())
}

/// Add a local track to a playlist
#[tauri::command]
pub async fn playlist_add_local_track(
    playlist_id: u64,
    local_track_id: i64,
    position: i32,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_add_local_track {} track {}", playlist_id, local_track_id);

    let db = state.db.lock().await;
    db.add_local_track_to_playlist(playlist_id, local_track_id, position)
        .map_err(|e| e.to_string())
}

/// Remove a local track from a playlist
#[tauri::command]
pub async fn playlist_remove_local_track(
    playlist_id: u64,
    local_track_id: i64,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_remove_local_track {} track {}", playlist_id, local_track_id);

    let db = state.db.lock().await;
    db.remove_local_track_from_playlist(playlist_id, local_track_id)
        .map_err(|e| e.to_string())
}

/// Get all local tracks in a playlist
#[tauri::command]
pub async fn playlist_get_local_tracks(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<Vec<LocalTrack>, String> {
    log::info!("Command: playlist_get_local_tracks {}", playlist_id);

    let db = state.db.lock().await;
    db.get_playlist_local_tracks(playlist_id)
        .map_err(|e| e.to_string())
}

/// Get all local tracks in a playlist with their positions (for mixed ordering)
#[tauri::command]
pub async fn playlist_get_local_tracks_with_position(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<Vec<crate::library::PlaylistLocalTrack>, String> {
    log::info!("Command: playlist_get_local_tracks_with_position {}", playlist_id);

    let db = state.db.lock().await;
    db.get_playlist_local_tracks_with_position(playlist_id)
        .map_err(|e| e.to_string())
}

/// Get local track counts for all playlists
#[tauri::command]
pub async fn playlist_get_all_local_track_counts(
    state: State<'_, LibraryState>,
) -> Result<std::collections::HashMap<u64, u32>, String> {
    log::info!("Command: playlist_get_all_local_track_counts");

    let db = state.db.lock().await;
    db.get_all_playlist_local_track_counts()
        .map_err(|e| e.to_string())
}

/// Clear all local tracks from a playlist
#[tauri::command]
pub async fn playlist_clear_local_tracks(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_clear_local_tracks {}", playlist_id);

    let db = state.db.lock().await;
    db.clear_playlist_local_tracks(playlist_id)
        .map_err(|e| e.to_string())
}

/// Get all playlist settings (for sidebar filter/sort)
#[tauri::command]
pub async fn playlist_get_all_settings(
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistSettings>, String> {
    log::info!("Command: playlist_get_all_settings");

    let db = state.db.lock().await;
    db.get_all_playlist_settings()
        .map_err(|e| e.to_string())
}

/// Set playlist hidden status
#[tauri::command]
pub async fn playlist_set_hidden(
    playlist_id: u64,
    hidden: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_set_hidden {} {}", playlist_id, hidden);

    let db = state.db.lock().await;
    db.set_playlist_hidden(playlist_id, hidden)
        .map_err(|e| e.to_string())
}

/// Set playlist favorite status
#[tauri::command]
pub async fn playlist_set_favorite(
    playlist_id: u64,
    favorite: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_set_favorite {} {}", playlist_id, favorite);

    let db = state.db.lock().await;
    db.set_playlist_favorite(playlist_id, favorite)
        .map_err(|e| e.to_string())
}

/// Get all favorite playlist IDs
#[tauri::command]
pub async fn playlist_get_favorites(
    state: State<'_, LibraryState>,
) -> Result<Vec<u64>, String> {
    log::info!("Command: playlist_get_favorites");

    let db = state.db.lock().await;
    db.get_favorite_playlist_ids()
        .map_err(|e| e.to_string())
}

/// Set playlist position (for custom ordering)
#[tauri::command]
pub async fn playlist_set_position(
    playlist_id: u64,
    position: i32,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_set_position {} {}", playlist_id, position);

    let db = state.db.lock().await;
    db.set_playlist_position(playlist_id, position)
        .map_err(|e| e.to_string())
}

/// Bulk reorder playlists
#[tauri::command]
pub async fn playlist_reorder(
    playlist_ids: Vec<u64>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_reorder {:?}", playlist_ids);

    let db = state.db.lock().await;
    db.reorder_playlists(&playlist_ids)
        .map_err(|e| e.to_string())
}

/// Get playlist statistics
#[tauri::command]
pub async fn playlist_get_stats(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<Option<PlaylistStats>, String> {
    log::info!("Command: playlist_get_stats {}", playlist_id);

    let db = state.db.lock().await;
    db.get_playlist_stats(playlist_id)
        .map_err(|e| e.to_string())
}

/// Get all playlist statistics (for sorting by play count)
#[tauri::command]
pub async fn playlist_get_all_stats(
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistStats>, String> {
    log::info!("Command: playlist_get_all_stats");

    let db = state.db.lock().await;
    db.get_all_playlist_stats()
        .map_err(|e| e.to_string())
}

/// Increment playlist play count (called when "Play All" is clicked)
#[tauri::command]
pub async fn playlist_increment_play_count(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<PlaylistStats, String> {
    log::info!("Command: playlist_increment_play_count {}", playlist_id);

    let db = state.db.lock().await;
    db.increment_playlist_play_count(playlist_id)
        .map_err(|e| e.to_string())
}

// === Discogs Artwork ===

/// Check if Discogs credentials are configured (proxy handles credentials)
#[tauri::command]
pub async fn discogs_has_credentials() -> Result<bool, String> {
    // Proxy always provides credentials
    Ok(true)
}

/// Fetch missing artwork from Discogs for albums without artwork
/// Returns number of albums updated
#[tauri::command]
pub async fn library_fetch_missing_artwork(
    state: State<'_, LibraryState>,
) -> Result<u32, String> {
    log::info!("Command: library_fetch_missing_artwork");

    // Get Discogs client (proxy handles credentials)
    let discogs = DiscogsClient::new();

    let artwork_cache = get_artwork_cache_dir();
    let mut updated_count = 0u32;

    // Get all albums without artwork
    let albums_without_artwork: Vec<(String, String, String)> = {
        let db = state.db.lock().await;
        db.get_albums_without_artwork()
            .map_err(|e| e.to_string())?
    };

    log::info!("Found {} albums without artwork", albums_without_artwork.len());

    for (group_key, album, artist) in albums_without_artwork {
        // Try to fetch from Discogs
        if let Some(artwork_path) = discogs.fetch_artwork(&artist, &album, &artwork_cache).await {
            // Update all tracks in this album with the artwork
            let db = state.db.lock().await;
            if db.update_album_group_artwork(&group_key, &artwork_path).is_ok() {
                updated_count += 1;
                log::info!("Updated artwork for {} - {}", artist, album);
            }
        }

        // Small delay to respect rate limits (60 requests/min)
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    log::info!("Fetched artwork for {} albums from Discogs", updated_count);
    Ok(updated_count)
}

/// Fetch artwork for a specific album from Discogs
#[tauri::command]
pub async fn library_fetch_album_artwork(
    artist: String,
    album: String,
    state: State<'_, LibraryState>,
) -> Result<Option<String>, String> {
    log::info!("Command: library_fetch_album_artwork {} - {}", artist, album);

    // Get Discogs client (proxy handles credentials)
    let discogs = DiscogsClient::new();

    let artwork_cache = get_artwork_cache_dir();

    if let Some(artwork_path) = discogs.fetch_artwork(&artist, &album, &artwork_cache).await {
        let db = state.db.lock().await;
        if let Some(group_key) = db
            .find_album_group_key(&album, &artist)
            .map_err(|e| e.to_string())?
        {
            db.update_album_group_artwork(&group_key, &artwork_path)
                .map_err(|e| e.to_string())?;
        } else {
            db.update_album_artwork(&album, &artist, &artwork_path)
                .map_err(|e| e.to_string())?;
        }
        Ok(Some(artwork_path))
    } else {
        Ok(None)
    }
}

/// Set custom artwork for an album group from a local file
#[tauri::command]
pub async fn library_set_album_artwork(
    album_group_key: String,
    artwork_path: String,
    state: State<'_, LibraryState>,
) -> Result<String, String> {
    log::info!(
        "Command: library_set_album_artwork {}",
        album_group_key
    );

    if album_group_key.is_empty() {
        return Err("Album group key is required".to_string());
    }

    let source_path = Path::new(&artwork_path);
    if !source_path.is_file() {
        return Err("Artwork file not found".to_string());
    }

    let artwork_cache = get_artwork_cache_dir();
    let cached_path = MetadataExtractor::cache_artwork_file(source_path, &artwork_cache)
        .ok_or_else(|| "Failed to cache artwork file".to_string())?;

    let db = state.db.lock().await;
    db.update_album_group_artwork(&album_group_key, &cached_path)
        .map_err(|e| e.to_string())?;

    Ok(cached_path)
}

/// Search for artists on Discogs
#[tauri::command]
pub async fn discogs_search_artist(
    query: String,
) -> Result<crate::discogs::SearchResponse, String> {
    log::info!("Command: discogs_search_artist query={}", query);

    // Get Discogs client (proxy handles credentials)
    let discogs = DiscogsClient::new();

    discogs.search_artist(&query).await
}

// === Album Settings ===

#[tauri::command]
pub async fn library_get_album_settings(
    album_group_key: String,
    state: State<'_, LibraryState>,
) -> Result<Option<crate::library::AlbumSettings>, String> {
    log::info!("Command: library_get_album_settings {}", album_group_key);

    let db = state.db.lock().await;
    db.get_album_settings(&album_group_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn library_set_album_hidden(
    album_group_key: String,
    hidden: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: library_set_album_hidden {} = {}", album_group_key, hidden);

    let db = state.db.lock().await;
    db.set_album_hidden(&album_group_key, hidden)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn library_get_hidden_albums(
    state: State<'_, LibraryState>,
) -> Result<Vec<String>, String> {
    log::info!("Command: library_get_hidden_albums");

    let db = state.db.lock().await;
    db.get_hidden_albums()
        .map_err(|e| e.to_string())
}

// === Qobuz Downloads Integration ===

#[derive(serde::Serialize)]
pub struct BackfillReport {
    pub total_downloads: usize,
    pub added_tracks: usize,
    pub skipped_tracks: usize,
    pub failed_tracks: Vec<String>,
}

#[tauri::command]
pub async fn library_backfill_downloads(
    state: State<'_, LibraryState>,
    download_cache_state: State<'_, crate::download_cache::DownloadCacheState>,
) -> Result<BackfillReport, String> {
    log::info!("Command: library_backfill_downloads");

    let mut report = BackfillReport {
        total_downloads: 0,
        added_tracks: 0,
        skipped_tracks: 0,
        failed_tracks: Vec::new(),
    };

    // Get all ready downloads directly from download cache DB
    let downloads = {
        let cache_db = download_cache_state.db.lock().await;
        
        let mut stmt = cache_db
            .conn()
            .prepare("SELECT track_id, title, artist, album, album_id, duration_secs, file_path, quality, bit_depth, sample_rate FROM cached_tracks WHERE status = 'ready'")
            .map_err(|e| format!("Failed to query downloads: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,           // track_id
                    row.get::<_, String>(1)?,                // title
                    row.get::<_, String>(2)?,                // artist
                    row.get::<_, Option<String>>(3)?,        // album
                    row.get::<_, i64>(5)? as u64,            // duration_secs
                    row.get::<_, String>(6)?,                // file_path
                    row.get::<_, Option<i64>>(8)?.map(|v| v as u32),  // bit_depth
                    row.get::<_, Option<f64>>(9)?,           // sample_rate
                ))
            })
            .map_err(|e| format!("Failed to map rows: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect downloads: {}", e))?
    }; // cache_db lock is dropped here
    
    report.total_downloads = downloads.len();

    let library_db = state.db.lock().await;

    for (track_id, title, artist, album, duration_secs, file_path, bit_depth, sample_rate) in downloads {
        // Check if already in library
        let exists = library_db
            .track_exists_by_qobuz_id(track_id)
            .unwrap_or(false);

        if exists {
            report.skipped_tracks += 1;
            continue;
        }

        // Insert into library with source = 'qobuz_download'
        match library_db.insert_qobuz_download_direct(
            track_id,
            &title,
            &artist,
            album.as_deref(),
            duration_secs,
            &file_path,
            bit_depth,
            sample_rate,
        ) {
            Ok(_) => report.added_tracks += 1,
            Err(e) => {
                log::warn!("Failed to insert track {}: {}", track_id, e);
                report.failed_tracks.push(title);
            }
        }
    }

    Ok(report)
}

// === Artist Images Management ===

#[derive(serde::Serialize)]
pub struct ArtistImageInfo {
    pub artist_name: String,
    pub image_url: Option<String>,
    pub source: Option<String>,
    pub custom_image_path: Option<String>,
}

/// Get cached artist image
#[tauri::command]
pub async fn library_get_artist_image(
    artist_name: String,
    state: State<'_, LibraryState>,
) -> Result<Option<ArtistImageInfo>, String> {
    let db = state.db.lock().await;
    db.get_artist_image(&artist_name).map_err(|e| e.to_string())
}

/// Get multiple artist images at once
#[tauri::command]
pub async fn library_get_artist_images(
    artist_names: Vec<String>,
    state: State<'_, LibraryState>,
) -> Result<Vec<ArtistImageInfo>, String> {
    let db = state.db.lock().await;
    let mut results = Vec::new();
    for name in artist_names {
        if let Ok(Some(info)) = db.get_artist_image(&name) {
            results.push(info);
        }
    }
    Ok(results)
}

/// Cache artist image from Qobuz
#[tauri::command]
pub async fn library_cache_artist_image(
    artist_name: String,
    image_url: String,
    source: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let db = state.db.lock().await;
    db.cache_artist_image(&artist_name, Some(&image_url), &source, None)
        .map_err(|e| e.to_string())
}

/// Set custom artist image
#[tauri::command]
pub async fn library_set_custom_artist_image(
    artist_name: String,
    custom_image_path: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    // Copy image to persistent location
    let artwork_dir = get_artwork_cache_dir();
    let source = Path::new(&custom_image_path);
    
    if !source.exists() {
        return Err(format!("Source image does not exist: {}", custom_image_path));
    }

    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    
    // Use artist name hash for filename to avoid filesystem issues with special characters
    use md5::{Md5, Digest};
    let mut hasher = Md5::new();
    hasher.update(artist_name.as_bytes());
    let artist_hash = format!("{:x}", hasher.finalize());
    
    let filename = format!("artist_custom_{}_{}.{}", artist_hash, chrono::Utc::now().timestamp(), extension);
    let dest_path = artwork_dir.join(filename);

    fs::copy(source, &dest_path)
        .map_err(|e| format!("Failed to copy artwork: {}", e))?;
    
    log::info!("Copied artist artwork for '{}' to: {}", artist_name, dest_path.display());

    let db = state.db.lock().await;
    db.cache_artist_image(&artist_name, None, "custom", Some(&dest_path.to_string_lossy()))
        .map_err(|e| e.to_string())
}

// === Offline Mode: Playlist Local Content Analysis ===

/// Result of analyzing a playlist's local content
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistAnalysisResult {
    pub playlist_id: u64,
    pub total_tracks: u32,
    pub local_tracks: u32,
    pub status: crate::library::database::LocalContentStatus,
}

/// Track info for local content checking
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfoForAnalysis {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
}

/// Analyze a playlist's local content availability
#[tauri::command]
pub async fn playlist_analyze_local_content(
    playlist_id: u64,
    tracks: Vec<TrackInfoForAnalysis>,
    state: State<'_, LibraryState>,
) -> Result<PlaylistAnalysisResult, String> {
    log::info!("Command: playlist_analyze_local_content for playlist {}", playlist_id);

    let db = state.db.lock().await;
    let total_tracks = tracks.len() as u32;
    let mut local_count = 0u32;

    for track in &tracks {
        // First try to match by Qobuz track ID (for downloaded tracks)
        let has_by_id = db.has_local_track_by_qobuz_id(track.id)
            .map_err(|e| e.to_string())?;

        if has_by_id {
            local_count += 1;
            continue;
        }

        // Fallback: match by title + artist + album
        let has_by_metadata = db.has_local_track_by_metadata(&track.title, &track.artist, &track.album)
            .map_err(|e| e.to_string())?;

        if has_by_metadata {
            local_count += 1;
        }
    }

    // Determine status
    let status = if total_tracks == 0 {
        crate::library::database::LocalContentStatus::Unknown
    } else if local_count == 0 {
        crate::library::database::LocalContentStatus::No
    } else if local_count == total_tracks {
        crate::library::database::LocalContentStatus::AllLocal
    } else {
        crate::library::database::LocalContentStatus::SomeLocal
    };

    // Update the playlist settings with the new status
    db.update_playlist_local_content_status(playlist_id, status)
        .map_err(|e| e.to_string())?;

    Ok(PlaylistAnalysisResult {
        playlist_id,
        total_tracks,
        local_tracks: local_count,
        status,
    })
}

/// Get the local content status for a playlist
#[tauri::command]
pub async fn playlist_get_local_content_status(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<crate::library::database::LocalContentStatus, String> {
    log::info!("Command: playlist_get_local_content_status for playlist {}", playlist_id);

    let db = state.db.lock().await;
    let settings = db.get_playlist_settings(playlist_id)
        .map_err(|e| e.to_string())?;

    Ok(settings
        .map(|s| s.has_local_content)
        .unwrap_or(crate::library::database::LocalContentStatus::Unknown))
}

/// Check if a specific track is available locally
#[tauri::command]
pub async fn playlist_track_is_local(
    qobuz_track_id: u64,
    title: String,
    artist: String,
    album: String,
    state: State<'_, LibraryState>,
) -> Result<bool, String> {
    let db = state.db.lock().await;

    // First try Qobuz track ID
    let has_by_id = db.has_local_track_by_qobuz_id(qobuz_track_id)
        .map_err(|e| e.to_string())?;

    if has_by_id {
        return Ok(true);
    }

    // Fallback to metadata
    db.has_local_track_by_metadata(&title, &artist, &album)
        .map_err(|e| e.to_string())
}

/// Get local track ID for a Qobuz track (for playback in offline mode)
#[tauri::command]
pub async fn playlist_get_local_track_id(
    qobuz_track_id: u64,
    title: String,
    artist: String,
    album: String,
    state: State<'_, LibraryState>,
) -> Result<Option<i64>, String> {
    let db = state.db.lock().await;

    // First try Qobuz track ID
    if let Some(id) = db.get_local_track_id_by_qobuz_id(qobuz_track_id)
        .map_err(|e| e.to_string())? {
        return Ok(Some(id));
    }

    // Fallback to metadata
    db.get_local_track_id_by_metadata(&title, &artist, &album)
        .map_err(|e| e.to_string())
}

/// Batch check which tracks have local copies (for offline mode)
/// Returns a list of track IDs that have local versions
#[tauri::command]
pub async fn playlist_get_tracks_with_local_copies(
    track_ids: Vec<u64>,
    state: State<'_, LibraryState>,
) -> Result<Vec<u64>, String> {
    let db = state.db.lock().await;

    let local_ids = db.get_tracks_with_local_copies(&track_ids)
        .map_err(|e| e.to_string())?;

    Ok(local_ids.into_iter().collect())
}

/// Get playlists that have local content (for offline mode filtering)
#[tauri::command]
pub async fn playlist_get_offline_available(
    include_partial: bool,
    state: State<'_, LibraryState>,
) -> Result<Vec<u64>, String> {
    log::info!("Command: playlist_get_offline_available (include_partial: {})", include_partial);

    let db = state.db.lock().await;
    let playlists = db.get_playlists_by_local_content(include_partial)
        .map_err(|e| e.to_string())?;

    Ok(playlists.iter().map(|p| p.qobuz_playlist_id).collect())
}

// === Discogs Artwork ===

/// Search Discogs for artwork options
#[tauri::command]
pub async fn discogs_search_artwork(
    artist: String,
    album: String,
    catalog_number: Option<String>,
) -> Result<Vec<crate::discogs::DiscogsImageOption>, String> {
    log::info!(
        "Command: discogs_search_artwork {} - {} (catalog: {:?})",
        artist,
        album,
        catalog_number
    );

    let client = DiscogsClient::new();
    client
        .search_artwork_options(&artist, &album, catalog_number.as_deref())
        .await
}

/// Download and save Discogs artwork
#[tauri::command]
pub async fn discogs_download_artwork(
    image_url: String,
    artist: String,
    album: String,
) -> Result<String, String> {
    log::info!("Command: discogs_download_artwork from {}", image_url);

    let cache_dir = get_artwork_cache_dir();
    let client = DiscogsClient::new();

    client.download_artwork_from_url(&image_url, &cache_dir, &artist, &album).await
}
