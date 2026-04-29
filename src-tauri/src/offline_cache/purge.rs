//! Offline cache purge helper.
//!
//! Used by `session_lifecycle` to clear all cached audio when the active
//! session is torn down. Not a Tauri command — pure helper.

use crate::library::LibraryState;
use crate::offline_cache::OfflineCacheState;

/// Clear entire offline cache (internal helper)
pub async fn purge_all_cached_files(
    cache_state: &OfflineCacheState,
    library_state: &LibraryState,
) -> Result<(), String> {
    let paths = {
        let guard__ = cache_state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.clear_all()?
    };

    // Delete all files. For v1 entries `file_path` is the plain FLAC;
    // for v2 entries it's `<dir>/segments.bin` — we remove the enclosing
    // track directory so init.mp4 + manifest.json + segments.bin all go
    // in one step. Plain files still work because remove_dir_all on a
    // file path fails silently and we fall through to remove_file.
    for path in paths {
        let p = std::path::Path::new(&path);
        if !p.exists() {
            continue;
        }
        // Heuristic: the v2 layout puts everything inside `tracks-cmaf/<id>/`.
        // If the parent directory matches that shape, remove the directory.
        let looks_like_v2 = p
            .parent()
            .and_then(|parent| parent.parent())
            .and_then(|root| root.file_name())
            .and_then(|n| n.to_str())
            == Some("tracks-cmaf");
        if looks_like_v2 {
            if let Some(track_dir) = p.parent() {
                let _ = std::fs::remove_dir_all(track_dir);
                continue;
            }
        }
        let _ = std::fs::remove_file(p);
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

    // And the tracks-cmaf directory (v2 bundles) — belt-and-suspenders
    // so orphan bundles from corrupt DB rows get cleaned up too.
    let tracks_cmaf_dir = cache_dir.join("tracks-cmaf");
    if tracks_cmaf_dir.exists() {
        let _ = std::fs::remove_dir_all(&tracks_cmaf_dir);
        log::info!("[OfflineCache/Purge] Removed tracks-cmaf/ directory");
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
