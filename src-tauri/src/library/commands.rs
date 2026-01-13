//! Tauri commands for local library

use std::path::Path;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::api_keys::ApiKeysState;
use crate::discogs::DiscogsClient;
use crate::library::{
    cue_to_tracks, get_artwork_cache_dir, CueParser, LibraryDatabase, LibraryScanner, LibraryStats,
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
                Ok(mut track) => {
                    // Try to extract embedded artwork, fallback to cached folder artwork
                    let artwork_cache = get_artwork_cache_dir();
                    let mut artwork_path =
                        MetadataExtractor::extract_artwork(audio_path, &artwork_cache);
                    if artwork_path.is_none() {
                        if let Some(folder_art) = MetadataExtractor::find_folder_artwork(audio_path)
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
    let mut tracks = cue_to_tracks(&cue, properties.duration_secs, format, &properties);

    let artwork_cache = get_artwork_cache_dir();
    let mut artwork_path =
        MetadataExtractor::extract_artwork(audio_path, &artwork_cache);
    if artwork_path.is_none() {
        if let Some(folder_art) = MetadataExtractor::find_folder_artwork(audio_path) {
            artwork_path = MetadataExtractor::cache_artwork_file(
                Path::new(&folder_art),
                &artwork_cache,
            );
        }
    }
    if let Some(path) = artwork_path {
        for track in tracks.iter_mut() {
            track.artwork_path = Some(path.clone());
        }
    }

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

    let db = state.db.lock().await;
    db.update_playlist_artwork(playlist_id, artwork_path.as_deref())
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

/// Check if Discogs credentials are configured (embedded or user-provided)
#[tauri::command]
pub async fn discogs_has_credentials(
    api_keys: State<'_, ApiKeysState>,
) -> Result<bool, String> {
    // Check user-provided credentials first
    let keys = api_keys.lock().await;
    if keys.discogs.is_set() {
        return Ok(true);
    }
    drop(keys);

    // Fall back to embedded/env credentials
    let client = DiscogsClient::new();
    Ok(client.has_credentials())
}

/// Fetch missing artwork from Discogs for albums without artwork
/// Returns number of albums updated
#[tauri::command]
pub async fn library_fetch_missing_artwork(
    state: State<'_, LibraryState>,
    api_keys: State<'_, ApiKeysState>,
) -> Result<u32, String> {
    log::info!("Command: library_fetch_missing_artwork");

    // Get user-provided credentials if available
    let keys = api_keys.lock().await;
    let discogs = DiscogsClient::with_user_credentials(
        keys.discogs.client_id.clone(),
        keys.discogs.client_secret.clone(),
    );
    drop(keys);

    if !discogs.has_credentials() {
        return Err("Discogs credentials not configured".to_string());
    }

    let artwork_cache = get_artwork_cache_dir();
    let mut updated_count = 0u32;

    // Get all albums without artwork
    let albums_without_artwork: Vec<(String, String)> = {
        let db = state.db.lock().await;
        db.get_albums_without_artwork()
            .map_err(|e| e.to_string())?
    };

    log::info!("Found {} albums without artwork", albums_without_artwork.len());

    for (artist, album) in albums_without_artwork {
        // Try to fetch from Discogs
        if let Some(artwork_path) = discogs.fetch_artwork(&artist, &album, &artwork_cache).await {
            // Update all tracks in this album with the artwork
            let db = state.db.lock().await;
            if db.update_album_artwork(&album, &artist, &artwork_path).is_ok() {
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
    api_keys: State<'_, ApiKeysState>,
) -> Result<Option<String>, String> {
    log::info!("Command: library_fetch_album_artwork {} - {}", artist, album);

    // Get user-provided credentials if available
    let keys = api_keys.lock().await;
    let discogs = DiscogsClient::with_user_credentials(
        keys.discogs.client_id.clone(),
        keys.discogs.client_secret.clone(),
    );
    drop(keys);

    if !discogs.has_credentials() {
        return Err("Discogs credentials not configured".to_string());
    }

    let artwork_cache = get_artwork_cache_dir();

    if let Some(artwork_path) = discogs.fetch_artwork(&artist, &album, &artwork_cache).await {
        // Update all tracks in this album with the artwork
        let db = state.db.lock().await;
        db.update_album_artwork(&album, &artist, &artwork_path)
            .map_err(|e| e.to_string())?;
        Ok(Some(artwork_path))
    } else {
        Ok(None)
    }
}
