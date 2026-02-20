//! Tauri commands for offline cache functionality

use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

use crate::api::models::Quality;
use crate::AppState;

use super::{CachedTrackInfo, OfflineCacheStats, OfflineCacheStatus, TrackCacheInfo};
use crate::offline_cache::metadata::{
    embed_artwork, fetch_complete_metadata, organize_cached_file, save_album_artwork,
    write_flac_tags,
};
use crate::offline_cache::OfflineCacheState;

/// Post-process a cached track: fetch metadata, tag FLAC, embed artwork, organize files
async fn post_process_cached_track(
    track_id: u64,
    current_path: &str,
    offline_root: &str,
    qobuz_client: &crate::api::QobuzClient,
    library_db: Arc<tokio::sync::Mutex<Option<crate::library::database::LibraryDatabase>>>,
) -> Result<String, String> {
    log::info!("Post-processing cached track {}", track_id);

    // 1. Fetch complete metadata from Qobuz
    let metadata = fetch_complete_metadata(track_id, qobuz_client).await?;

    // 2. Write FLAC tags
    write_flac_tags(current_path, &metadata).map_err(|e| format!("Failed to write tags: {}", e))?;

    // 3. Embed artwork if available
    if let Some(artwork_url) = &metadata.artwork_url {
        if let Err(e) = embed_artwork(current_path, artwork_url).await {
            log::warn!("Failed to embed artwork for track {}: {}", track_id, e);
        }
    }

    // 4. Organize file into artist/album structure
    let new_path = organize_cached_file(track_id, current_path, offline_root, &metadata)?;

    // 5. Download and save album cover art as cover.jpg
    let artwork_path = if let Some(artwork_url) = &metadata.artwork_url {
        // Extract album directory from the new_path
        if let Some(parent_dir) = std::path::Path::new(&new_path).parent() {
            match save_album_artwork(parent_dir, artwork_url).await {
                Ok(_) => {
                    let cover_path = parent_dir.join("cover.jpg").to_string_lossy().to_string();
                    log::info!("Album artwork saved for track {}: {}", track_id, cover_path);
                    Some(cover_path)
                }
                Err(e) => {
                    log::warn!("Failed to save album artwork for track {}: {}", track_id, e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        log::warn!("No artwork URL available for track {}", track_id);
        None
    };

    // 6. Extract audio properties from FLAC file
    use lofty::AudioFile;
    let (bit_depth, sample_rate) = match lofty::read_from_path(&new_path) {
        Ok(tagged_file) => {
            let properties = tagged_file.properties();
            let bit_depth = properties.bit_depth().map(|bd| bd as u32);
            let sample_rate = properties.sample_rate().map(|sr| sr as f64);
            (bit_depth, sample_rate)
        }
        Err(e) => {
            log::warn!(
                "Failed to read audio properties for track {}: {}",
                track_id,
                e
            );
            (None, None)
        }
    };

    // 7. ALWAYS insert into local library DB (visibility controlled by toggle)
    let lib_opt__ = library_db.lock().await;
    let lib_guard = lib_opt__
        .as_ref()
        .ok_or("No active session - please log in")?;

    // Generate album_group_key: album|album_artist
    let album_artist = metadata.album_artist.as_ref().unwrap_or(&metadata.artist);
    let album_group_key = format!("{}|{}", metadata.album, album_artist);

    match lib_guard.insert_qobuz_cached_track_with_grouping(
        track_id,
        &metadata.title,
        &metadata.artist,
        Some(&metadata.album),
        metadata.album_artist.as_deref(),
        metadata.track_number,
        metadata.disc_number,
        metadata.year,
        metadata.duration_secs,
        &new_path,
        &album_group_key,
        &metadata.album,
        bit_depth,
        sample_rate,
        artwork_path.as_deref(),
    ) {
        Ok(_) => log::info!(
            "Track {} inserted to local library DB with group key: {}, artwork: {:?}",
            track_id,
            album_group_key,
            artwork_path
        ),
        Err(e) => log::error!("Failed to insert track {} to library DB: {}", track_id, e),
    }

    log::info!("Track {} organized to: {}", track_id, new_path);
    Ok(new_path)
}

/// Cache a track for offline playback
#[tauri::command]
pub async fn cache_track_for_offline(
    track_id: u64,
    title: String,
    artist: String,
    album: Option<String>,
    album_id: Option<String>,
    duration_secs: u64,
    quality: String,
    bit_depth: Option<u32>,
    sample_rate: Option<f64>,
    state: State<'_, AppState>,
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    log::info!(
        "Command: cache_track_for_offline {} - {} by {}",
        track_id,
        title,
        artist
    );

    let track_info = TrackCacheInfo {
        track_id,
        title,
        artist,
        album,
        album_id,
        duration_secs,
        quality,
        bit_depth,
        sample_rate,
    };

    // Determine file path (we'll use flac as default format)
    let file_path = cache_state.track_file_path(track_id, "flac");
    let file_path_str = file_path.to_string_lossy().to_string();

    // Insert into database as queued
    {
        let guard__ = cache_state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.insert_track(&track_info, &file_path_str)?;
    }

    // Clone what we need for the spawn
    let client = state.client.clone();
    let fetcher = cache_state.fetcher.clone();
    let db = cache_state.db.clone();
    let offline_root = cache_state.get_cache_path();
    let library_db = library_state.db.clone();
    let app = app_handle.clone();
    let semaphore = cache_state.cache_semaphore.clone();

    // Spawn caching task
    tokio::spawn(async move {
        let _permit = match semaphore.acquire_owned().await {
            Ok(permit) => permit,
            Err(err) => {
                log::error!(
                    "Failed to acquire cache slot for track {}: {}",
                    track_id,
                    err
                );
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(
                        track_id,
                        OfflineCacheStatus::Failed,
                        Some("Failed to start caching"),
                    );
                }
                let _ = app.emit(
                    "offline:caching_failed",
                    serde_json::json!({
                        "trackId": track_id,
                        "error": "Failed to acquire cache slot"
                    }),
                );
                return;
            }
        };

        // Update status to caching
        {
            if let Some(db_guard) = db.lock().await.as_ref() {
                let _ = db_guard.update_status(track_id, OfflineCacheStatus::Downloading, None);
            }
        }

        let _ = app.emit(
            "offline:caching_started",
            serde_json::json!({
                "trackId": track_id
            }),
        );

        // Get stream URL with highest quality available
        let stream_url = {
            let client_guard = client.read().await;
            client_guard
                .get_stream_url_with_fallback(track_id, Quality::UltraHiRes)
                .await
        };

        let url = match stream_url {
            Ok(s) => s.url,
            Err(e) => {
                log::error!("Failed to get stream URL for track {}: {}", track_id, e);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(
                        track_id,
                        OfflineCacheStatus::Failed,
                        Some(&format!("Failed to get stream URL: {}", e)),
                    );
                }
                let _ = app.emit(
                    "offline:caching_failed",
                    serde_json::json!({
                        "trackId": track_id,
                        "error": e.to_string()
                    }),
                );
                return;
            }
        };

        // Fetch and cache the file
        match fetcher
            .fetch_to_file(&url, &file_path, track_id, Some(&app))
            .await
        {
            Ok(size) => {
                log::info!("Caching complete for track {}: {} bytes", track_id, size);
                {
                    if let Some(db_guard) = db.lock().await.as_ref() {
                        let _ = db_guard.mark_complete(track_id, size);
                    }
                }

                let _ = app.emit(
                    "offline:caching_completed",
                    serde_json::json!({
                        "trackId": track_id,
                        "size": size
                    }),
                );

                // Post-processing: metadata, tagging, artwork, organization
                log::info!("Starting post-processing for cached track {}", track_id);

                let file_path_str = file_path.to_string_lossy().to_string();
                let qobuz_client = client.read().await;
                match post_process_cached_track(
                    track_id,
                    &file_path_str,
                    &offline_root,
                    &*qobuz_client,
                    library_db.clone(),
                )
                .await
                {
                    Ok(new_path) => {
                        // Update database with new path
                        if let Some(db_guard) = db.lock().await.as_ref() {
                            if let Err(e) = db_guard.update_file_path(track_id, &new_path) {
                                log::error!("Failed to update path for track {}: {}", track_id, e);
                            }
                        }

                        let _ = app.emit(
                            "offline:caching_processed",
                            serde_json::json!({
                                "trackId": track_id,
                                "path": new_path
                            }),
                        );
                    }
                    Err(e) => {
                        log::error!(
                            "Post-processing failed for cached track {}: {}",
                            track_id,
                            e
                        );
                        // File still exists and is playable, just not organized
                    }
                }
            }
            Err(e) => {
                log::error!("Caching failed for track {}: {}", track_id, e);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(track_id, OfflineCacheStatus::Failed, Some(&e));
                }
                let _ = app.emit(
                    "offline:caching_failed",
                    serde_json::json!({
                        "trackId": track_id,
                        "error": e
                    }),
                );
            }
        }
    });

    Ok(())
}

/// Check if a track is cached and ready for playback
#[tauri::command]
pub async fn is_track_cached(
    track_id: u64,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<bool, String> {
    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.is_cached(track_id)
}

/// Get the local file path for a cached track
#[tauri::command]
pub async fn get_cached_track_path(
    track_id: u64,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<Option<String>, String> {
    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_file_path(track_id)
}

/// Get info about a specific cached track
#[tauri::command]
pub async fn get_cached_track(
    track_id: u64,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<Option<CachedTrackInfo>, String> {
    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_track(track_id)
}

/// Get all cached tracks
#[tauri::command]
pub async fn get_cached_tracks(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<Vec<CachedTrackInfo>, String> {
    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_tracks()
}

/// Get offline cache statistics
#[tauri::command]
pub async fn get_offline_cache_stats(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<OfflineCacheStats, String> {
    let limit = *cache_state.limit_bytes.lock().await;
    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_stats(&cache_state.get_cache_path(), limit)
}

/// Remove a track from the offline cache
#[tauri::command]
pub async fn remove_cached_track(
    track_id: u64,
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), String> {
    log::info!("Command: remove_cached_track {}", track_id);

    {
        let guard__ = cache_state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;

        // Get file path and delete from DB
        if let Some(file_path) = db.delete_track(track_id)? {
            // Delete the actual file
            let path = std::path::Path::new(&file_path);
            if path.exists() {
                std::fs::remove_file(path).map_err(|e| format!("Failed to delete file: {}", e))?;
            }

            // Clean up empty album folder (and artist folder if also empty)
            if let Some(album_dir) = path.parent() {
                cleanup_empty_folder(album_dir, &cache_state.cache_dir.read().unwrap());
            }
        }
    }

    // Also remove from library if it was added
    let guard__ = library_state.db.lock().await;
    let library_db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let _ = library_db.remove_qobuz_cached_track(track_id);

    Ok(())
}

/// Clear entire offline cache
#[tauri::command]
pub async fn clear_offline_cache(
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), String> {
    log::info!("Command: clear_offline_cache");
    purge_all_cached_files(cache_state.inner(), library_state.inner()).await
}

/// Clean up empty folders after deleting a track
/// Removes the album folder if empty, then the artist folder if also empty
fn cleanup_empty_folder(folder: &std::path::Path, cache_root: &std::path::Path) {
    // Don't delete the cache root itself
    if folder == cache_root || !folder.starts_with(cache_root) {
        return;
    }

    // Check if folder is empty (or only contains cover.jpg)
    if let Ok(entries) = std::fs::read_dir(folder) {
        let entries: Vec<_> = entries.flatten().collect();

        // If folder only contains non-audio files (like cover.jpg), delete the whole folder
        let has_audio_files = entries.iter().any(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "flac" || ext == "mp3" || ext == "wav" || ext == "m4a")
                .unwrap_or(false)
        });

        if !has_audio_files {
            // Delete all files in the folder (cover.jpg, etc.)
            for entry in &entries {
                let _ = std::fs::remove_file(entry.path());
            }
            // Delete the folder itself
            if std::fs::remove_dir(folder).is_ok() {
                log::info!("Removed empty album folder: {:?}", folder);

                // Try to clean up parent (artist) folder if now empty
                if let Some(parent) = folder.parent() {
                    cleanup_empty_folder(parent, cache_root);
                }
            }
        }
    }
}

/// Clear entire offline cache (internal helper)
pub async fn purge_all_cached_files(
    cache_state: &OfflineCacheState,
    library_state: &crate::library::commands::LibraryState,
) -> Result<(), String> {
    let paths = {
        let guard__ = cache_state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.clear_all()?
    };

    // Delete all files
    for path in paths {
        let p = std::path::Path::new(&path);
        if p.exists() {
            let _ = std::fs::remove_file(p);
        }
    }

    // Also clear the tracks directory (legacy unorganized files)
    let cache_dir = cache_state.cache_dir.read().unwrap().clone();
    let tracks_dir = cache_dir.join("tracks");
    if tracks_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&tracks_dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    // Clear organized artist/album folders
    // Look for any subdirectories in cache_dir that are not "tracks" or system files
    if let Ok(entries) = std::fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Skip the tracks directory and database files
                if name != "tracks" && !name.ends_with(".db") && !name.ends_with(".db-journal") {
                    // This is likely an artist folder, delete it recursively
                    if let Err(e) = std::fs::remove_dir_all(&path) {
                        log::warn!("Failed to remove folder {:?}: {}", path, e);
                    } else {
                        log::info!("Removed artist folder: {:?}", path);
                    }
                }
            }
        }
    }

    // Remove all Qobuz cached tracks from library
    let guard__ = library_state.db.lock().await;
    let library_db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let removed_count = library_db
        .remove_all_qobuz_cached_tracks()
        .map_err(|e| format!("Failed to remove cached tracks from library: {}", e))?;
    log::info!("Removed {} Qobuz cached tracks from library", removed_count);

    Ok(())
}

/// Set cache size limit
#[tauri::command]
pub async fn set_offline_cache_limit(
    limit_mb: Option<u64>,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    log::info!("Command: set_offline_cache_limit {:?} MB", limit_mb);

    let limit_bytes = limit_mb.map(|mb| mb * 1024 * 1024);
    let mut limit = cache_state.limit_bytes.lock().await;
    *limit = limit_bytes;

    // Check if we need to evict
    if let Some(limit_bytes) = limit_bytes {
        drop(limit);
        evict_if_needed(&cache_state, limit_bytes).await?;
    }

    Ok(())
}

/// Open the cache folder in the system file manager
#[tauri::command]
pub async fn open_offline_cache_folder(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    log::info!("Command: open_offline_cache_folder");

    let path = cache_state.cache_dir.read().unwrap().clone();

    // Ensure directory exists
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create cache directory: {}", e))?;

    // Open with system file manager
    open::that(&path).map_err(|e| format!("Failed to open folder: {}", e))?;

    Ok(())
}

/// Open the folder containing a specific album in the system file manager
#[tauri::command]
pub async fn open_album_folder(
    album_id: String,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    log::info!("Command: open_album_folder for album_id: {}", album_id);

    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

    // Get tracks for this album
    let tracks = db.get_all_tracks()?;
    let album_tracks: Vec<_> = tracks
        .into_iter()
        .filter(|t| t.album_id.as_deref() == Some(&album_id))
        .collect();

    if album_tracks.is_empty() {
        return Err("No cached tracks found for this album".to_string());
    }

    // Get the first track's path and extract the album directory
    let first_track_id = album_tracks[0].track_id;
    let file_path = db
        .get_file_path(first_track_id)?
        .ok_or_else(|| "Track file path not found".to_string())?;

    let track_path = std::path::Path::new(&file_path);
    let album_dir = track_path
        .parent()
        .ok_or_else(|| "Could not determine album folder".to_string())?;

    if !album_dir.exists() {
        return Err("Album folder does not exist".to_string());
    }

    // Open with system file manager
    open::that(album_dir).map_err(|e| format!("Failed to open folder: {}", e))?;

    Ok(())
}

/// Open the folder containing a specific track in the system file manager
#[tauri::command]
pub async fn open_track_folder(
    track_id: u64,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    log::info!("Command: open_track_folder for track_id: {}", track_id);

    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

    // Get the track's file path
    let file_path = db
        .get_file_path(track_id)?
        .ok_or_else(|| "Track file path not found - track may not be cached".to_string())?;

    let track_path = std::path::Path::new(&file_path);
    let track_dir = track_path
        .parent()
        .ok_or_else(|| "Could not determine track folder".to_string())?;

    if !track_dir.exists() {
        return Err("Track folder does not exist".to_string());
    }

    // Open with system file manager
    open::that(track_dir).map_err(|e| format!("Failed to open folder: {}", e))?;

    Ok(())
}

/// Check if an album is fully cached (all tracks ready)
#[tauri::command]
pub async fn check_album_fully_cached(
    album_id: String,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<bool, String> {
    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

    // Get all tracks for this album
    let tracks = db.get_all_tracks()?;
    let album_tracks: Vec<_> = tracks
        .into_iter()
        .filter(|t| t.album_id.as_deref() == Some(&album_id))
        .collect();

    if album_tracks.is_empty() {
        return Ok(false);
    }

    // Check if all tracks are ready
    for track in album_tracks {
        if track.status != crate::offline_cache::OfflineCacheStatus::Ready {
            return Ok(false);
        }
    }

    Ok(true)
}

/// Evict tracks if cache exceeds limit (LRU policy)
async fn evict_if_needed(cache_state: &OfflineCacheState, limit_bytes: u64) -> Result<(), String> {
    let guard__ = cache_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let stats = db.get_stats(&cache_state.get_cache_path(), Some(limit_bytes))?;

    if stats.total_size_bytes <= limit_bytes {
        return Ok(());
    }

    let bytes_to_free = stats.total_size_bytes - limit_bytes;
    log::info!(
        "Cache exceeds limit ({} > {}), need to free {} bytes",
        stats.total_size_bytes,
        limit_bytes,
        bytes_to_free
    );

    let tracks_to_evict = db.get_tracks_for_eviction(bytes_to_free)?;

    for (track_id, file_path) in tracks_to_evict {
        log::info!("Evicting track {} from cache", track_id);

        // Delete from DB
        db.delete_track(track_id)?;

        // Delete file
        let path = std::path::Path::new(&file_path);
        if path.exists() {
            let _ = std::fs::remove_file(path);

            // Clean up empty album folder (and artist folder if also empty)
            if let Some(album_dir) = path.parent() {
                cleanup_empty_folder(album_dir, &cache_state.cache_dir.read().unwrap());
            }
        }
    }

    Ok(())
}

// Path management commands

#[tauri::command]
pub async fn check_offline_root_mounted(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<bool, String> {
    log::info!("Command: check_offline_root_mounted");

    let root_path = cache_state
        .cache_dir
        .read()
        .unwrap()
        .to_string_lossy()
        .to_string();
    super::path_validator::is_offline_root_available(&root_path)
}

#[tauri::command]
pub async fn validate_offline_path(
    path: String,
) -> Result<super::path_validator::PathValidationResult, String> {
    log::info!("Command: validate_offline_path: {}", path);
    super::path_validator::validate_path(&path)
}

#[tauri::command]
pub async fn move_offline_cache_to_path(
    new_path: String,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<super::path_validator::MoveReport, String> {
    log::info!("Command: move_offline_cache_to_path: {}", new_path);

    let old_path = cache_state
        .cache_dir
        .read()
        .unwrap()
        .to_string_lossy()
        .to_string();
    super::path_validator::move_cached_files_to_new_path(&old_path, &new_path)
}

/// Detect legacy cached files (numeric FLAC files)
#[tauri::command]
pub async fn detect_legacy_cached_files(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<super::MigrationStatus, String> {
    log::info!("Command: detect_legacy_cached_files");

    let tracks_dir = cache_state.cache_dir.read().unwrap().join("tracks");

    match super::detect_legacy_cached_files(&tracks_dir) {
        Ok(track_ids) => Ok(super::MigrationStatus {
            has_legacy_files: !track_ids.is_empty(),
            total_tracks: track_ids.len(),
            ..Default::default()
        }),
        Err(e) => Err(e),
    }
}

/// Start migration of legacy cached files
#[tauri::command]
pub async fn start_legacy_migration(
    state: State<'_, AppState>,
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    log::info!("Command: start_legacy_migration");

    let tracks_dir = cache_state.cache_dir.read().unwrap().join("tracks");
    let track_ids = super::detect_legacy_cached_files(&tracks_dir)?;

    if track_ids.is_empty() {
        return Err("No legacy cached files found".to_string());
    }

    let offline_root = cache_state.get_cache_path();
    let qobuz_client = state.client.clone();
    let library_db = library_state.db.clone();
    let app_complete = app_handle.clone();

    // Spawn migration task
    tokio::spawn(async move {
        let status = super::migrate_legacy_cached_files(
            track_ids,
            tracks_dir,
            offline_root,
            qobuz_client,
            library_db,
        )
        .await;

        let _ = app_complete.emit("migration:complete", status);
    });

    Ok(())
}

/// Sync cached tracks to library database
/// This ensures all ready cached tracks appear in the library with source='qobuz_cached'
#[tauri::command]
pub async fn sync_offline_cache_to_library(
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
) -> Result<SyncResult, String> {
    log::info!("Command: sync_offline_cache_to_library");

    // Get ready tracks from cache (scoped to drop lock before library access)
    let ready_tracks = {
        let guard__ = cache_state.db.lock().await;
        let cache_db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        cache_db.get_ready_tracks_for_sync()?
    };

    let mut synced = 0u32;
    let mut already_present = 0u32;
    let mut errors = 0u32;

    // Process tracks with library lock (scoped)
    {
        let guard__ = library_state.db.lock().await;
        let library_db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;

        for track in ready_tracks {
            // Check if track already exists in library
            match library_db.track_exists_by_qobuz_id(track.track_id) {
                Ok(true) => {
                    already_present += 1;
                }
                Ok(false) => {
                    // Insert into library with basic metadata
                    match library_db.insert_qobuz_cached_track_direct(
                        track.track_id,
                        &track.title,
                        &track.artist,
                        track.album.as_deref(),
                        track.duration_secs,
                        &track.file_path,
                        track.bit_depth,
                        track.sample_rate,
                    ) {
                        Ok(_) => {
                            log::info!(
                                "Synced track {} to library: {}",
                                track.track_id,
                                track.title
                            );
                            synced += 1;
                        }
                        Err(e) => {
                            log::error!("Failed to sync track {}: {}", track.track_id, e);
                            errors += 1;
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to check if track {} exists: {}", track.track_id, e);
                    errors += 1;
                }
            }
        }
    }

    log::info!(
        "Sync complete: {} synced, {} already present, {} errors",
        synced,
        already_present,
        errors
    );

    Ok(SyncResult {
        synced,
        already_present,
        errors,
    })
}

/// Result of syncing downloads to library
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub synced: u32,
    pub already_present: u32,
    pub errors: u32,
}
