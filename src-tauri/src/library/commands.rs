//! Tauri commands for local library

use std::fs;
use std::path::{Path, PathBuf};
use tauri::State;

use crate::discogs::DiscogsClient;
use crate::library::{
    get_artwork_cache_dir, thumbnails, ArtistImageInfo, LibraryFolder, LocalAlbum, LocalTrack,
    MetadataExtractor,
};

// Shared state types and DTOs live in `super::state`; re-export them from
// this module so historical `crate::library::commands::*` paths used by
// `commands_v2/legacy_compat.rs` continue to compile.
pub use super::state::{
    BackfillReport, CleanupResult, LibraryAlbumMetadataUpdateRequest,
    LibraryAlbumTrackMetadataUpdate, LibraryState,
};

// === Folder Management ===

#[tauri::command]
pub async fn library_remove_folder(
    path: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: library_remove_folder {}", path);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.remove_folder(&path).map_err(|e| e.to_string())?;
    db.delete_tracks_in_folder(&path)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Result of cache stats query
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    /// Size of old artwork cache in bytes (to be cleaned)
    pub artwork_cache_bytes: u64,
    /// Size of thumbnails cache in bytes
    pub thumbnails_cache_bytes: u64,
    /// Number of files in artwork cache
    pub artwork_file_count: usize,
    /// Number of files in thumbnails cache
    pub thumbnail_file_count: usize,
}

/// Get cache statistics
#[tauri::command]
pub async fn library_get_cache_stats() -> Result<CacheStats, String> {
    log::info!("Command: library_get_cache_stats");

    // Old artwork cache
    let artwork_dir = get_artwork_cache_dir();
    let (artwork_bytes, artwork_count) = if artwork_dir.exists() {
        let mut size = 0u64;
        let mut count = 0usize;
        if let Ok(entries) = fs::read_dir(&artwork_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        size += meta.len();
                        count += 1;
                    }
                }
            }
        }
        (size, count)
    } else {
        (0, 0)
    };

    // Thumbnails cache
    let thumbnails_bytes = thumbnails::get_cache_size().unwrap_or(0);
    let thumbnail_count = if let Ok(dir) = thumbnails::get_thumbnails_dir() {
        fs::read_dir(&dir).map(|e| e.count()).unwrap_or(0)
    } else {
        0
    };

    Ok(CacheStats {
        artwork_cache_bytes: artwork_bytes,
        thumbnails_cache_bytes: thumbnails_bytes,
        artwork_file_count: artwork_count,
        thumbnail_file_count: thumbnail_count,
    })
}

/// Clear the old artwork cache (full-size images that are no longer needed)
#[tauri::command]
pub async fn library_clear_artwork_cache() -> Result<u64, String> {
    log::info!("Command: library_clear_artwork_cache");

    let artwork_dir = get_artwork_cache_dir();

    if !artwork_dir.exists() {
        return Ok(0);
    }

    let mut cleared_bytes = 0u64;

    if let Ok(entries) = fs::read_dir(&artwork_dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    cleared_bytes += meta.len();
                    if let Err(e) = fs::remove_file(entry.path()) {
                        log::warn!("Failed to remove cache file {:?}: {}", entry.path(), e);
                    }
                }
            }
        }
    }

    log::info!("Cleared {} bytes from artwork cache", cleared_bytes);
    Ok(cleared_bytes)
}

/// Clear the thumbnails cache
#[tauri::command]
pub async fn library_clear_thumbnails_cache() -> Result<u64, String> {
    log::info!("Command: library_clear_thumbnails_cache");

    let size_before = thumbnails::get_cache_size().unwrap_or(0);

    thumbnails::clear_thumbnails().map_err(|e| e.to_string())?;

    log::info!("Cleared {} bytes from thumbnails cache", size_before);
    Ok(size_before)
}

/// Get a single folder by ID
#[tauri::command]
pub async fn library_get_folder(
    id: i64,
    state: State<'_, LibraryState>,
) -> Result<Option<LibraryFolder>, String> {
    log::info!("Command: library_get_folder {}", id);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_folder_by_id(id).map_err(|e| e.to_string())
}

/// Enable or disable a folder
#[tauri::command]
pub async fn library_set_folder_enabled(
    id: i64,
    enabled: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: library_set_folder_enabled {} enabled={}",
        id,
        enabled
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.set_folder_enabled(id, enabled)
        .map_err(|e| e.to_string())
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

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.update_folder_path(id, &new_path)
        .map_err(|e| e.to_string())?;

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
            )
            .map_err(|e| e.to_string())?;
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

    // Try to read the directory with a timeout to avoid hanging on network paths.
    // Network shares can be slow to answer, so use a less aggressive timeout.
    let path_clone = path.clone();
    let check_result = tokio::time::timeout(
        std::time::Duration::from_secs(6),
        tokio::task::spawn_blocking(move || std::fs::read_dir(Path::new(&path_clone)).is_ok()),
    )
    .await;

    match check_result {
        Ok(Ok(accessible)) => Ok(accessible),
        Ok(Err(_)) => {
            log::warn!("Failed to spawn blocking task for folder check: {}", path);
            Ok(false)
        }
        Err(_) => {
            // If the folder still exists, treat it as accessible to avoid false negatives
            // in mounted-but-slow network shares.
            let exists = Path::new(&path).exists();
            log::warn!(
                "Timeout checking folder accessibility: {} (exists={})",
                path,
                exists
            );
            Ok(exists)
        }
    }
}

// === Queries ===

#[tauri::command]
pub async fn library_get_albums(
    include_hidden: Option<bool>,
    exclude_network_folders: Option<bool>,
    state: State<'_, LibraryState>,
    download_settings_state: State<'_, crate::config::DownloadSettingsState>,
) -> Result<Vec<LocalAlbum>, String> {
    log::info!(
        "Command: library_get_albums (exclude_network: {:?})",
        exclude_network_folders
    );

    // Get download settings to check if we should include Qobuz downloads
    let include_qobuz = download_settings_state
        .lock()
        .map_err(|e| format!("Failed to lock download settings: {}", e))?
        .as_ref()
        .and_then(|s| s.get_settings().ok())
        .map(|s| s.show_in_library)
        .unwrap_or(false);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

    // Use optimized SQL-based filtering instead of N+1 query pattern
    let albums = db
        .get_albums_with_full_filter(
            include_hidden.unwrap_or(false),
            include_qobuz,
            exclude_network_folders.unwrap_or(false),
        )
        .map_err(|e| e.to_string())?;

    log::info!("Returning {} albums", albums.len());
    Ok(albums)
}

#[tauri::command]
pub async fn library_get_album_tracks(
    album_group_key: String,
    state: State<'_, LibraryState>,
) -> Result<Vec<LocalTrack>, String> {
    log::info!("Command: library_get_album_tracks {}", album_group_key);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_album_tracks(&album_group_key)
        .map_err(|e| e.to_string())
}

// === Playback ===

#[tauri::command]
pub async fn library_get_track(
    track_id: i64,
    state: State<'_, LibraryState>,
) -> Result<LocalTrack, String> {
    log::info!("Command: library_get_track {}", track_id);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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
        let guard__ = library_state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.get_track(track_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Track not found".to_string())?
    };

    // Read file from disk
    let file_path = Path::new(&track.file_path);
    if !file_path.exists() {
        return Err(format!("File not found: {}", track.file_path));
    }

    let audio_data = std::fs::read(file_path).map_err(|e| format!("Failed to read file: {}", e))?;

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

use crate::library::{PlaylistFolder, PlaylistSettings, PlaylistStats};

/// Get playlist settings by Qobuz playlist ID
#[tauri::command]
pub async fn playlist_get_settings(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<Option<PlaylistSettings>, String> {
    log::debug!("Command: playlist_get_settings {}", playlist_id);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_playlist_settings(playlist_id)
        .map_err(|e| e.to_string())
}

/// Save or update playlist settings
#[tauri::command]
pub async fn playlist_save_settings(
    settings: PlaylistSettings,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: playlist_save_settings {}",
        settings.qobuz_playlist_id
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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
    log::info!(
        "Command: playlist_set_sort {} {} {}",
        playlist_id,
        sort_by,
        sort_order
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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

        let extension = source.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
        let filename = format!(
            "playlist_{}_{}.{}",
            playlist_id,
            chrono::Utc::now().timestamp(),
            extension
        );
        let dest_path = artwork_dir.join(filename);

        fs::copy(source, &dest_path).map_err(|e| format!("Failed to copy artwork: {}", e))?;

        log::info!("Copied playlist artwork to: {}", dest_path.display());
        Some(dest_path.to_string_lossy().to_string())
    } else {
        None
    };

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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
    log::info!(
        "Command: playlist_add_local_track {} track {}",
        playlist_id,
        local_track_id
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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
    log::info!(
        "Command: playlist_remove_local_track {} track {}",
        playlist_id,
        local_track_id
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_playlist_local_tracks(playlist_id)
        .map_err(|e| e.to_string())
}

/// Get all local tracks in a playlist with their positions (for mixed ordering)
#[tauri::command]
pub async fn playlist_get_local_tracks_with_position(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<Vec<crate::library::PlaylistLocalTrack>, String> {
    log::debug!(
        "Command: playlist_get_local_tracks_with_position {}",
        playlist_id
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_playlist_local_tracks_with_position(playlist_id)
        .map_err(|e| e.to_string())
}

/// Get local track counts for all playlists
#[tauri::command]
pub async fn playlist_get_all_local_track_counts(
    state: State<'_, LibraryState>,
) -> Result<std::collections::HashMap<u64, u32>, String> {
    log::debug!("Command: playlist_get_all_local_track_counts");

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.clear_playlist_local_tracks(playlist_id)
        .map_err(|e| e.to_string())
}

/// Get all playlist settings (for sidebar filter/sort)
#[tauri::command]
pub async fn playlist_get_all_settings(
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistSettings>, String> {
    log::debug!("Command: playlist_get_all_settings");

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_playlist_settings().map_err(|e| e.to_string())
}

/// Set playlist hidden status
#[tauri::command]
pub async fn playlist_set_hidden(
    playlist_id: u64,
    hidden: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_set_hidden {} {}", playlist_id, hidden);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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
    log::info!(
        "Command: playlist_set_favorite {} {}",
        playlist_id,
        favorite
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.set_playlist_favorite(playlist_id, favorite)
        .map_err(|e| e.to_string())
}

/// Get all favorite playlist IDs
#[tauri::command]
pub async fn playlist_get_favorites(state: State<'_, LibraryState>) -> Result<Vec<u64>, String> {
    log::info!("Command: playlist_get_favorites");

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_favorite_playlist_ids().map_err(|e| e.to_string())
}

/// Set playlist position (for custom ordering)
#[tauri::command]
pub async fn playlist_set_position(
    playlist_id: u64,
    position: i32,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: playlist_set_position {} {}",
        playlist_id,
        position
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.reorder_playlists(&playlist_ids)
        .map_err(|e| e.to_string())
}

/// Get playlist statistics
#[tauri::command]
pub async fn playlist_get_stats(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<Option<PlaylistStats>, String> {
    log::debug!("Command: playlist_get_stats {}", playlist_id);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_playlist_stats(playlist_id)
        .map_err(|e| e.to_string())
}

/// Get all playlist statistics (for sorting by play count)
#[tauri::command]
pub async fn playlist_get_all_stats(
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistStats>, String> {
    log::debug!("Command: playlist_get_all_stats");

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_playlist_stats().map_err(|e| e.to_string())
}

/// Increment playlist play count (called when "Play All" is clicked)
#[tauri::command]
pub async fn playlist_increment_play_count(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<PlaylistStats, String> {
    log::info!("Command: playlist_increment_play_count {}", playlist_id);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.increment_playlist_play_count(playlist_id)
        .map_err(|e| e.to_string())
}

// === Playlist Custom Track Order ===

/// Get custom track order for a playlist
/// Returns Vec of (track_id, is_local, position)
#[tauri::command]
pub async fn playlist_get_custom_order(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<Vec<(i64, bool, i32)>, String> {
    log::info!("Command: playlist_get_custom_order {}", playlist_id);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_playlist_custom_order(playlist_id)
        .map_err(|e| e.to_string())
}

/// Initialize custom order for a playlist from current track arrangement
#[tauri::command]
pub async fn playlist_init_custom_order(
    playlist_id: u64,
    track_ids: Vec<(i64, bool)>, // (track_id, is_local)
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: playlist_init_custom_order {} ({} tracks)",
        playlist_id,
        track_ids.len()
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.init_playlist_custom_order(playlist_id, &track_ids)
        .map_err(|e| e.to_string())
}

/// Set entire custom order for a playlist (batch update)
#[tauri::command]
pub async fn playlist_set_custom_order(
    playlist_id: u64,
    orders: Vec<(i64, bool, i32)>, // (track_id, is_local, position)
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: playlist_set_custom_order {} ({} tracks)",
        playlist_id,
        orders.len()
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.set_playlist_custom_order(playlist_id, &orders)
        .map_err(|e| e.to_string())
}

/// Move a single track to a new position
#[tauri::command]
pub async fn playlist_move_track(
    playlist_id: u64,
    track_id: i64,
    is_local: bool,
    new_position: i32,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: playlist_move_track {} track {} -> pos {}",
        playlist_id,
        track_id,
        new_position
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.move_playlist_track(playlist_id, track_id, is_local, new_position)
        .map_err(|e| e.to_string())
}

/// Check if a playlist has custom order defined
#[tauri::command]
pub async fn playlist_has_custom_order(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<bool, String> {
    log::info!("Command: playlist_has_custom_order {}", playlist_id);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.has_playlist_custom_order(playlist_id)
        .map_err(|e| e.to_string())
}

/// Clear custom order for a playlist
#[tauri::command]
pub async fn playlist_clear_custom_order(
    playlist_id: u64,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: playlist_clear_custom_order {}", playlist_id);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.clear_playlist_custom_order(playlist_id)
        .map_err(|e| e.to_string())
}

// === Discogs Artwork ===

/// Fetch missing artwork from Discogs for albums without artwork
/// Returns number of albums updated
#[tauri::command]
pub async fn library_fetch_missing_artwork(state: State<'_, LibraryState>) -> Result<u32, String> {
    log::info!("Command: library_fetch_missing_artwork");

    // Get Discogs client (proxy handles credentials)
    let discogs = DiscogsClient::new();

    let artwork_cache = get_artwork_cache_dir();
    let mut updated_count = 0u32;

    // Get all albums without artwork
    let albums_without_artwork: Vec<(String, String, String)> = {
        let guard__ = state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.get_albums_without_artwork().map_err(|e| e.to_string())?
    };

    log::info!(
        "Found {} albums without artwork",
        albums_without_artwork.len()
    );

    for (group_key, album, artist) in albums_without_artwork {
        // Try to fetch from Discogs
        if let Some(artwork_path) = discogs.fetch_artwork(&artist, &album, &artwork_cache).await {
            // Update all tracks in this album with the artwork
            let guard__ = state.db.lock().await;
            let db = guard__
                .as_ref()
                .ok_or("No active session - please log in")?;
            if db
                .update_album_group_artwork(&group_key, &artwork_path)
                .is_ok()
            {
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
    log::info!(
        "Command: library_fetch_album_artwork {} - {}",
        artist,
        album
    );

    // Get Discogs client (proxy handles credentials)
    let discogs = DiscogsClient::new();

    let artwork_cache = get_artwork_cache_dir();

    if let Some(artwork_path) = discogs.fetch_artwork(&artist, &album, &artwork_cache).await {
        let guard__ = state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
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
    log::info!("Command: library_set_album_artwork {}", album_group_key);

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

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_album_settings(&album_group_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn library_set_album_hidden(
    album_group_key: String,
    hidden: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: library_set_album_hidden {} = {}",
        album_group_key,
        hidden
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.set_album_hidden(&album_group_key, hidden)
        .map_err(|e| e.to_string())
}


#[tauri::command]
pub async fn library_get_hidden_albums(
    state: State<'_, LibraryState>,
) -> Result<Vec<String>, String> {
    log::info!("Command: library_get_hidden_albums");

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_hidden_albums().map_err(|e| e.to_string())
}

// === Artist Images Management ===

// ArtistImageInfo is now defined in qbz-library, re-exported via crate::library

/// Get cached artist image
#[tauri::command]
pub async fn library_get_artist_image(
    artist_name: String,
    state: State<'_, LibraryState>,
) -> Result<Option<ArtistImageInfo>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_artist_image(&artist_name).map_err(|e| e.to_string())
}

/// Get multiple artist images at once
#[tauri::command]
pub async fn library_get_artist_images(
    artist_names: Vec<String>,
    state: State<'_, LibraryState>,
) -> Result<Vec<ArtistImageInfo>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let mut results = Vec::new();
    for name in artist_names {
        if let Ok(Some(info)) = db.get_artist_image(&name) {
            results.push(info);
        }
    }
    Ok(results)
}

/// Get all canonical artist names mapping
#[tauri::command]
pub async fn library_get_canonical_names(
    state: State<'_, LibraryState>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_canonical_names().map_err(|e| e.to_string())
}

/// Cache artist image from Qobuz/Discogs with canonical name
#[tauri::command]
pub async fn library_cache_artist_image(
    artist_name: String,
    image_url: String,
    source: String,
    canonical_name: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.cache_artist_image_with_canonical(
        &artist_name,
        Some(&image_url),
        &source,
        None,
        canonical_name.as_deref(),
    )
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
        return Err(format!(
            "Source image does not exist: {}",
            custom_image_path
        ));
    }

    let extension = source.extension().and_then(|e| e.to_str()).unwrap_or("jpg");

    // Use artist name hash for filename to avoid filesystem issues with special characters
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(artist_name.as_bytes());
    let artist_hash = format!("{:x}", hasher.finalize());

    let filename = format!(
        "artist_custom_{}_{}.{}",
        artist_hash,
        chrono::Utc::now().timestamp(),
        extension
    );
    let dest_path = artwork_dir.join(filename);

    fs::copy(source, &dest_path).map_err(|e| format!("Failed to copy artwork: {}", e))?;

    log::info!(
        "Copied artist artwork for '{}' to: {}",
        artist_name,
        dest_path.display()
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.cache_artist_image(
        &artist_name,
        None,
        "custom",
        Some(&dest_path.to_string_lossy()),
    )
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
    log::info!(
        "Command: playlist_analyze_local_content for playlist {}",
        playlist_id
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let total_tracks = tracks.len() as u32;
    let mut local_count = 0u32;

    for track in &tracks {
        // First try to match by Qobuz track ID (for downloaded tracks)
        let has_by_id = db
            .has_local_track_by_qobuz_id(track.id)
            .map_err(|e| e.to_string())?;

        if has_by_id {
            local_count += 1;
            continue;
        }

        // Fallback: match by title + artist + album
        let has_by_metadata = db
            .has_local_track_by_metadata(&track.title, &track.artist, &track.album)
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
    log::info!(
        "Command: playlist_get_local_content_status for playlist {}",
        playlist_id
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let settings = db
        .get_playlist_settings(playlist_id)
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
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

    // First try Qobuz track ID
    let has_by_id = db
        .has_local_track_by_qobuz_id(qobuz_track_id)
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
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

    // First try Qobuz track ID
    if let Some(id) = db
        .get_local_track_id_by_qobuz_id(qobuz_track_id)
        .map_err(|e| e.to_string())?
    {
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
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

    let local_ids = db
        .get_tracks_with_local_copies(&track_ids)
        .map_err(|e| e.to_string())?;

    Ok(local_ids.into_iter().collect())
}

/// Get playlists that have local content (for offline mode filtering)
#[tauri::command]
pub async fn playlist_get_offline_available(
    include_partial: bool,
    state: State<'_, LibraryState>,
) -> Result<Vec<u64>, String> {
    log::info!(
        "Command: playlist_get_offline_available (include_partial: {})",
        include_partial
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let playlists = db
        .get_playlists_by_local_content(include_partial)
        .map_err(|e| e.to_string())?;

    Ok(playlists.iter().map(|p| p.qobuz_playlist_id).collect())
}

/// Get multiple tracks by their IDs
#[tauri::command]
pub async fn library_get_tracks_by_ids(
    track_ids: Vec<i64>,
    state: State<'_, LibraryState>,
) -> Result<Vec<LocalTrack>, String> {
    log::info!(
        "Command: library_get_tracks_by_ids ({} tracks)",
        track_ids.len()
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let mut tracks = Vec::new();

    for track_id in track_ids {
        if let Some(track) = db.get_track(track_id).map_err(|e| e.to_string())? {
            tracks.push(track);
        }
    }

    Ok(tracks)
}

/// Get or generate a thumbnail for an artwork file
/// Returns the path to the thumbnail file
#[tauri::command]
pub async fn library_get_thumbnail(artwork_path: String) -> Result<String, String> {
    log::debug!("Command: library_get_thumbnail for {}", artwork_path);

    let source_path = PathBuf::from(&artwork_path);

    if !source_path.exists() {
        return Err(format!("Artwork file not found: {}", artwork_path));
    }

    let thumbnail_path =
        thumbnails::get_or_generate_thumbnail(&source_path).map_err(|e| e.to_string())?;

    Ok(thumbnail_path.to_string_lossy().to_string())
}

/// Clear the thumbnails cache
#[tauri::command]
pub async fn library_clear_thumbnails() -> Result<(), String> {
    log::info!("Command: library_clear_thumbnails");
    thumbnails::clear_thumbnails().map_err(|e| e.to_string())
}

/// Get the thumbnails cache size in bytes
#[tauri::command]
pub async fn library_get_thumbnails_cache_size() -> Result<u64, String> {
    log::debug!("Command: library_get_thumbnails_cache_size");
    thumbnails::get_cache_size().map_err(|e| e.to_string())
}

// === Playlist Folders ===

/// Create a new playlist folder
#[tauri::command]
pub async fn create_playlist_folder(
    name: String,
    icon_type: Option<String>,
    icon_preset: Option<String>,
    icon_color: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<PlaylistFolder, String> {
    log::info!("Command: create_playlist_folder {}", name);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.create_playlist_folder(
        &name,
        icon_type.as_deref(),
        icon_preset.as_deref(),
        icon_color.as_deref(),
    )
    .map_err(|e| e.to_string())
}

/// Get all playlist folders
#[tauri::command]
pub async fn get_playlist_folders(
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistFolder>, String> {
    log::debug!("Command: get_playlist_folders");

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_playlist_folders().map_err(|e| e.to_string())
}

/// Update a playlist folder
#[tauri::command]
pub async fn update_playlist_folder(
    id: String,
    name: Option<String>,
    icon_type: Option<String>,
    icon_preset: Option<String>,
    icon_color: Option<String>,
    custom_image_path: Option<String>,
    is_hidden: Option<bool>,
    state: State<'_, LibraryState>,
) -> Result<PlaylistFolder, String> {
    log::info!("Command: update_playlist_folder {}", id);

    // Handle custom image - copy to persistent storage if provided
    // Uses Option<Option<&str>> semantics: None = don't update, Some(None) = clear, Some(Some(path)) = set new
    let final_custom_image: Option<Option<String>> = if let Some(source_path) = custom_image_path {
        if source_path.is_empty() {
            // Empty string means clear the image
            Some(None)
        } else {
            let source = Path::new(&source_path);
            if !source.exists() {
                return Err(format!("Source image does not exist: {}", source_path));
            }

            let artwork_dir = get_artwork_cache_dir();
            let extension = source.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
            let filename = format!(
                "folder_{}_{}.{}",
                id,
                chrono::Utc::now().timestamp(),
                extension
            );
            let dest_path = artwork_dir.join(filename);

            fs::copy(source, &dest_path).map_err(|e| format!("Failed to copy image: {}", e))?;

            log::info!("Copied folder image to: {}", dest_path.display());
            Some(Some(dest_path.to_string_lossy().to_string()))
        }
    } else {
        None
    };

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.update_playlist_folder(
        &id,
        name.as_deref(),
        icon_type.as_deref(),
        icon_preset.as_deref(),
        icon_color.as_deref(),
        final_custom_image.as_ref().map(|o| o.as_deref()),
        is_hidden,
    )
    .map_err(|e| e.to_string())
}

/// Delete a playlist folder (playlists return to root)
#[tauri::command]
pub async fn delete_playlist_folder(
    id: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Command: delete_playlist_folder {}", id);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.delete_playlist_folder(&id).map_err(|e| e.to_string())
}

/// Reorder playlist folders
#[tauri::command]
pub async fn reorder_playlist_folders(
    folder_ids: Vec<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: reorder_playlist_folders ({} folders)",
        folder_ids.len()
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.reorder_playlist_folders(&folder_ids)
        .map_err(|e| e.to_string())
}

/// Move a playlist to a folder (or root if folder_id is None)
#[tauri::command]
pub async fn move_playlist_to_folder(
    playlist_id: u64,
    folder_id: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!(
        "Command: move_playlist_to_folder playlist {} to folder {:?}",
        playlist_id,
        folder_id
    );

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.move_playlist_to_folder(playlist_id, folder_id.as_deref())
        .map_err(|e| e.to_string())
}

