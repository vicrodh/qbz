//! Tauri commands for download cache functionality

use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

use crate::api::models::Quality;
use crate::AppState;

use crate::download_cache::path_validator::{self, PathValidationResult};
use crate::download_cache::{DownloadCacheDb, DownloadCacheState};
use crate::download_cache::metadata::{fetch_complete_metadata, write_flac_tags, embed_artwork, organize_download, save_album_artwork};
use super::{
    CachedTrackInfo, DownloadCacheStats, DownloadStatus,
    TrackDownloadInfo,
};

/// Post-process a downloaded track: fetch metadata, tag FLAC, embed artwork, organize files
async fn post_process_track(
    track_id: u64,
    current_path: &str,
    download_root: &str,
    qobuz_client: &crate::api::QobuzClient,
    library_db: Arc<tokio::sync::Mutex<crate::library::database::LibraryDatabase>>,
) -> Result<String, String> {
    log::info!("Post-processing track {}", track_id);

    // 1. Fetch complete metadata from Qobuz
    let metadata = fetch_complete_metadata(track_id, qobuz_client).await?;
    
    // 2. Write FLAC tags
    write_flac_tags(current_path, &metadata)
        .map_err(|e| format!("Failed to write tags: {}", e))?;
    
    // 3. Embed artwork if available
    if let Some(artwork_url) = &metadata.artwork_url {
        if let Err(e) = embed_artwork(current_path, artwork_url).await {
            log::warn!("Failed to embed artwork for track {}: {}", track_id, e);
        }
    }
    
    // 4. Organize file into artist/album structure
    let new_path = organize_download(track_id, current_path, download_root, &metadata)?;
    
    // 5. Download and save album cover art as cover.jpg
    let artwork_path = if let Some(artwork_url) = &metadata.artwork_url {
        // Extract album directory from the new_path
        if let Some(parent_dir) = std::path::Path::new(&new_path).parent() {
            match save_album_artwork(parent_dir, artwork_url).await {
                Ok(_) => {
                    let cover_path = parent_dir.join("cover.jpg").to_string_lossy().to_string();
                    log::info!("Album artwork saved for track {}: {}", track_id, cover_path);
                    Some(cover_path)
                },
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
            log::warn!("Failed to read audio properties for track {}: {}", track_id, e);
            (None, None)
        }
    };
    
    // 7. ALWAYS insert into local library DB (visibility controlled by toggle)
    let lib_guard = library_db.lock().await;
    
    // Generate album_group_key: album|album_artist
    let album_artist = metadata.album_artist.as_ref().unwrap_or(&metadata.artist);
    let album_group_key = format!("{}|{}", metadata.album, album_artist);
    
    match lib_guard.insert_qobuz_download_with_grouping(
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
        Ok(_) => log::info!("Track {} inserted to local library DB with group key: {}, artwork: {:?}", track_id, album_group_key, artwork_path),
        Err(e) => log::error!("Failed to insert track {} to library DB: {}", track_id, e),
    }
    
    log::info!("Track {} organized to: {}", track_id, new_path);
    Ok(new_path)
}

/// Download a track for offline listening
#[tauri::command]
pub async fn download_track(
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
    cache_state: State<'_, DownloadCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    log::info!("Command: download_track {} - {} by {}", track_id, title, artist);

    let track_info = TrackDownloadInfo {
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
        let db = cache_state.db.lock().await;
        db.insert_track(&track_info, &file_path_str)?;
    }

    // Clone what we need for the spawn
    let client = state.client.clone();
    let downloader = cache_state.downloader.clone();
    let db = cache_state.db.clone();
    let download_root = cache_state.get_cache_path();
    let library_db = library_state.db.clone();
    let app = app_handle.clone();
    let semaphore = cache_state.download_semaphore.clone();

    // Spawn download task
    tokio::spawn(async move {
        let _permit = match semaphore.acquire_owned().await {
            Ok(permit) => permit,
            Err(err) => {
                log::error!("Failed to acquire download slot for track {}: {}", track_id, err);
                let db_guard = db.lock().await;
                let _ = db_guard.update_status(
                    track_id,
                    DownloadStatus::Failed,
                    Some("Failed to start download"),
                );
                let _ = app.emit("download:failed", serde_json::json!({
                    "trackId": track_id,
                    "error": "Failed to acquire download slot"
                }));
                return;
            }
        };

        // Update status to downloading
        {
            let db_guard = db.lock().await;
            let _ = db_guard.update_status(track_id, DownloadStatus::Downloading, None);
        }

        let _ = app.emit("download:started", serde_json::json!({
            "trackId": track_id
        }));

        // Get stream URL with highest quality available
        let stream_url = {
            let client_guard = client.lock().await;
            client_guard
                .get_stream_url_with_fallback(track_id, Quality::UltraHiRes)
                .await
        };

        let url = match stream_url {
            Ok(s) => s.url,
            Err(e) => {
                log::error!("Failed to get stream URL for track {}: {}", track_id, e);
                let db_guard = db.lock().await;
                let _ = db_guard.update_status(
                    track_id,
                    DownloadStatus::Failed,
                    Some(&format!("Failed to get stream URL: {}", e)),
                );
                let _ = app.emit("download:failed", serde_json::json!({
                    "trackId": track_id,
                    "error": e.to_string()
                }));
                return;
            }
        };

        // Download the file
        match downloader
            .download_to_file(&url, &file_path, track_id, Some(&app))
            .await
        {
            Ok(size) => {
                log::info!("Download complete for track {}: {} bytes", track_id, size);
                {
                    let db_guard = db.lock().await;
                    let _ = db_guard.mark_complete(track_id, size);
                }
                
                let _ = app.emit("download:completed", serde_json::json!({
                    "trackId": track_id,
                    "size": size
                }));

                // Post-processing: metadata, tagging, artwork, organization
                log::info!("Starting post-processing for track {}", track_id);
                
                let file_path_str = file_path.to_string_lossy().to_string();
                let qobuz_client = client.lock().await;
                match post_process_track(
                    track_id,
                    &file_path_str,
                    &download_root,
                    &*qobuz_client,
                    library_db.clone(),
                ).await {
                    Ok(new_path) => {
                        // Update database with new path
                        let db_guard = db.lock().await;
                        if let Err(e) = db_guard.update_file_path(track_id, &new_path) {
                            log::error!("Failed to update path for track {}: {}", track_id, e);
                        }
                        
                        let _ = app.emit("download:processed", serde_json::json!({
                            "trackId": track_id,
                            "path": new_path
                        }));
                    }
                    Err(e) => {
                        log::error!("Post-processing failed for track {}: {}", track_id, e);
                        // File still exists and is playable, just not organized
                    }
                }
            }
            Err(e) => {
                log::error!("Download failed for track {}: {}", track_id, e);
                let db_guard = db.lock().await;
                let _ = db_guard.update_status(track_id, DownloadStatus::Failed, Some(&e));
                let _ = app.emit("download:failed", serde_json::json!({
                    "trackId": track_id,
                    "error": e
                }));
            }
        }
    });

    Ok(())
}

/// Check if a track is cached and ready for playback
#[tauri::command]
pub async fn is_track_downloaded(
    track_id: u64,
    cache_state: State<'_, DownloadCacheState>,
) -> Result<bool, String> {
    let db = cache_state.db.lock().await;
    db.is_cached(track_id)
}

/// Get the local file path for a cached track
#[tauri::command]
pub async fn get_downloaded_track_path(
    track_id: u64,
    cache_state: State<'_, DownloadCacheState>,
) -> Result<Option<String>, String> {
    let db = cache_state.db.lock().await;
    db.get_file_path(track_id)
}

/// Get info about a specific cached track
#[tauri::command]
pub async fn get_downloaded_track(
    track_id: u64,
    cache_state: State<'_, DownloadCacheState>,
) -> Result<Option<CachedTrackInfo>, String> {
    let db = cache_state.db.lock().await;
    db.get_track(track_id)
}

/// Get all cached tracks
#[tauri::command]
pub async fn get_downloaded_tracks(
    cache_state: State<'_, DownloadCacheState>,
) -> Result<Vec<CachedTrackInfo>, String> {
    let db = cache_state.db.lock().await;
    db.get_all_tracks()
}

/// Get download cache statistics
#[tauri::command]
pub async fn get_download_cache_stats(
    cache_state: State<'_, DownloadCacheState>,
) -> Result<DownloadCacheStats, String> {
    let db = cache_state.db.lock().await;
    let limit = *cache_state.limit_bytes.lock().await;
    db.get_stats(&cache_state.get_cache_path(), limit)
}

/// Remove a track from the download cache
#[tauri::command]
pub async fn remove_downloaded_track(
    track_id: u64,
    cache_state: State<'_, DownloadCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), String> {
    log::info!("Command: remove_downloaded_track {}", track_id);

    let db = cache_state.db.lock().await;

    // Get file path and delete from DB
    if let Some(file_path) = db.delete_track(track_id)? {
        // Delete the actual file
        let path = std::path::Path::new(&file_path);
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| format!("Failed to delete file: {}", e))?;
        }
    }
    drop(db);

    // Also remove from library if it was added
    let library_db = library_state.db.lock().await;
    let _ = library_db.remove_qobuz_download(track_id);

    Ok(())
}

/// Clear entire download cache
#[tauri::command]
pub async fn clear_download_cache(
    cache_state: State<'_, DownloadCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), String> {
    log::info!("Command: clear_download_cache");

    let db = cache_state.db.lock().await;

    // Get all file paths and clear DB
    let paths = db.clear_all()?;
    drop(db);

    // Delete all files
    for path in paths {
        let p = std::path::Path::new(&path);
        if p.exists() {
            let _ = std::fs::remove_file(p);
        }
    }

    // Also clear the tracks directory
    let tracks_dir = cache_state.cache_dir.join("tracks");
    if tracks_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&tracks_dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    // Remove all Qobuz downloads from library
    let library_db = library_state.db.lock().await;
    let removed_count = library_db.remove_all_qobuz_downloads()
        .map_err(|e| format!("Failed to remove downloads from library: {}", e))?;
    log::info!("Removed {} Qobuz downloads from library", removed_count);

    Ok(())
}

/// Set cache size limit
#[tauri::command]
pub async fn set_download_cache_limit(
    limit_mb: Option<u64>,
    cache_state: State<'_, DownloadCacheState>,
) -> Result<(), String> {
    log::info!("Command: set_download_cache_limit {:?} MB", limit_mb);

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
pub async fn open_download_cache_folder(
    cache_state: State<'_, DownloadCacheState>,
) -> Result<(), String> {
    log::info!("Command: open_download_cache_folder");

    let path = cache_state.cache_dir.clone();

    // Ensure directory exists
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create cache directory: {}", e))?;

    // Open with system file manager
    open::that(&path).map_err(|e| format!("Failed to open folder: {}", e))?;

    Ok(())
}

/// Evict tracks if cache exceeds limit (LRU policy)
async fn evict_if_needed(
    cache_state: &DownloadCacheState,
    limit_bytes: u64,
) -> Result<(), String> {
    let db = cache_state.db.lock().await;
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
        }
    }

    Ok(())
}

// Path management commands

#[tauri::command]
pub async fn check_download_root_mounted(
    cache_state: State<'_, DownloadCacheState>,
) -> Result<bool, String> {
    log::info!("Command: check_download_root_mounted");
    
    let root_path = cache_state.cache_dir.to_string_lossy().to_string();
    super::path_validator::is_download_root_available(&root_path)
}

#[tauri::command]
pub async fn validate_download_path(path: String) -> Result<super::path_validator::PathValidationResult, String> {
    log::info!("Command: validate_download_path: {}", path);
    super::path_validator::validate_path(&path)
}

#[tauri::command]
pub async fn move_downloads_to_path(
    new_path: String,
    cache_state: State<'_, DownloadCacheState>,
) -> Result<super::path_validator::MoveReport, String> {
    log::info!("Command: move_downloads_to_path: {}", new_path);
    
    let old_path = cache_state.cache_dir.to_string_lossy().to_string();
    super::path_validator::move_downloads_to_new_path(&old_path, &new_path)
}

/// Detect legacy downloads (numeric FLAC files)
#[tauri::command]
pub async fn detect_legacy_downloads(
    cache_state: State<'_, DownloadCacheState>,
) -> Result<super::MigrationStatus, String> {
    log::info!("Command: detect_legacy_downloads");
    
    let tracks_dir = cache_state.cache_dir.join("tracks");
    
    match super::detect_legacy_downloads(&tracks_dir) {
        Ok(track_ids) => {
            Ok(super::MigrationStatus {
                has_legacy_files: !track_ids.is_empty(),
                total_tracks: track_ids.len(),
                ..Default::default()
            })
        }
        Err(e) => Err(e),
    }
}

/// Start migration of legacy downloads
#[tauri::command]
pub async fn start_legacy_migration(
    state: State<'_, AppState>,
    cache_state: State<'_, DownloadCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    log::info!("Command: start_legacy_migration");
    
    let tracks_dir = cache_state.cache_dir.join("tracks");
    let track_ids = super::detect_legacy_downloads(&tracks_dir)?;
    
    if track_ids.is_empty() {
        return Err("No legacy downloads found".to_string());
    }
    
    let download_root = cache_state.get_cache_path();
    let qobuz_client = state.client.clone();
    let library_db = library_state.db.clone();
    let app_progress = app_handle.clone();
    let app_complete = app_handle.clone();
    
    // Spawn migration task
    tokio::spawn(async move {
        let status = super::migrate_legacy_downloads(
            track_ids,
            tracks_dir,
            download_root,
            qobuz_client,
            library_db,
            move |status| {
                let _ = app_progress.emit("migration:progress", status);
            },
        ).await;
        
        let _ = app_complete.emit("migration:complete", status);
    });
    
    Ok(())
}
