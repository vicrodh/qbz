//! One-time migration of flat-layout databases to per-user directories
//!
//! When upgrading from the global layout to per-user isolation, this module
//! moves all existing DB files and cache directories into the first user's
//! subdirectory. A `.migrated` marker prevents re-running.

use std::path::{Path, PathBuf};

/// Marker file that indicates migration has already been performed
const MIGRATED_MARKER: &str = ".migrated";

/// Database files in the flat data directory (relative to ~/.local/share/qbz/)
const DATA_DIR_DB_FILES: &[&str] = &[
    "session.db",
    "favorites_cache.db",
    "subscription_state.db",
    "playback_preferences.db",
    "favorites_preferences.db",
    "download_settings.db",
    "audio_settings.db",
    "tray_settings.db",
    "remote_control_settings.db",
    "legal_settings.db",
    "updates.db",
    "library.db",
    "artist_blacklist.db",
    "offline_settings.db",
];

/// Subdirectory DB files (relative to ~/.local/share/qbz/)
const DATA_DIR_SUBDIR_FILES: &[(&str, &str)] = &[
    ("reco", "events.db"),
    ("cache", "api_cache.db"),
    ("cache", "artist_vectors.db"),
    ("cache", "musicbrainz_cache.db"),
    ("cache", "listenbrainz_cache.db"),
];

/// Cache directories to move (relative to ~/.cache/qbz/)
const CACHE_DIRS: &[&str] = &[
    "audio",
    "lyrics",
    "playback",
    "artwork",
    "tmp",
];

/// Data directories to move (relative to ~/.local/share/qbz/)
const DATA_DIRS: &[&str] = &[
    "thumbnails",
];

/// Check if migration has already been performed
pub fn is_migrated(global_data_dir: &Path) -> bool {
    global_data_dir.join(MIGRATED_MARKER).exists()
}

/// Perform the one-time migration of flat-layout files to per-user directory.
///
/// The first user to log in after the update inherits all existing data.
/// Any subsequent different user gets a clean slate.
pub fn migrate_flat_to_user(user_id: u64) -> Result<(), String> {
    let global_data_dir = crate::user_data::UserDataPaths::global_data_dir()?;
    let global_cache_dir = crate::user_data::UserDataPaths::global_cache_dir()?;

    if is_migrated(&global_data_dir) {
        log::debug!("Migration already completed, skipping");
        return Ok(());
    }

    log::info!("Starting flat-to-user migration for user {}", user_id);

    let user_data_dir = global_data_dir.join("users").join(user_id.to_string());
    let user_cache_dir = global_cache_dir.join("users").join(user_id.to_string());

    // Create user directories
    std::fs::create_dir_all(&user_data_dir)
        .map_err(|e| format!("Failed to create user data dir: {}", e))?;
    std::fs::create_dir_all(&user_cache_dir)
        .map_err(|e| format!("Failed to create user cache dir: {}", e))?;

    // 1. Move flat DB files from data dir
    for db_file in DATA_DIR_DB_FILES {
        move_db_with_journals(&global_data_dir, &user_data_dir, db_file);
    }

    // 2. Move subdirectory DB files (reco/events.db, cache/*.db)
    for (subdir, db_file) in DATA_DIR_SUBDIR_FILES {
        let src_dir = global_data_dir.join(subdir);
        let dst_dir = user_data_dir.join(subdir);

        if src_dir.join(db_file).exists() {
            std::fs::create_dir_all(&dst_dir)
                .map_err(|e| format!("Failed to create {}: {}", dst_dir.display(), e))?;
            move_db_with_journals(&src_dir, &dst_dir, db_file);
        }
    }

    // 3. Move data directories (thumbnails)
    for dir_name in DATA_DIRS {
        move_directory(&global_data_dir.join(dir_name), &user_data_dir.join(dir_name));
    }

    // 4. Move cache directories
    for dir_name in CACHE_DIRS {
        move_directory(&global_cache_dir.join(dir_name), &user_cache_dir.join(dir_name));
    }

    // 5. Write marker file
    let marker_path = global_data_dir.join(MIGRATED_MARKER);
    std::fs::write(&marker_path, format!("migrated_to_user={}\n", user_id))
        .map_err(|e| format!("Failed to write migration marker: {}", e))?;

    log::info!("Migration completed for user {}", user_id);
    Ok(())
}

/// Move a DB file and its WAL/SHM journal files
fn move_db_with_journals(src_dir: &Path, dst_dir: &Path, db_name: &str) {
    let extensions = ["", "-wal", "-shm"];
    for ext in extensions {
        let filename = format!("{}{}", db_name, ext);
        let src = src_dir.join(&filename);
        let dst = dst_dir.join(&filename);

        if src.exists() {
            match std::fs::rename(&src, &dst) {
                Ok(()) => log::debug!("Moved {} -> {}", src.display(), dst.display()),
                Err(e) => {
                    // Try copy + delete as fallback (cross-device moves)
                    log::warn!("Rename failed for {}, trying copy: {}", filename, e);
                    if let Err(copy_err) = copy_and_remove(&src, &dst) {
                        log::error!("Failed to migrate {}: {}", filename, copy_err);
                    }
                }
            }
        }
    }
}

/// Move a directory by renaming, with copy+delete fallback
fn move_directory(src: &Path, dst: &Path) {
    if !src.exists() || !src.is_dir() {
        return;
    }

    // Don't overwrite if destination already exists
    if dst.exists() {
        log::debug!("Destination {} already exists, skipping", dst.display());
        return;
    }

    match std::fs::rename(src, dst) {
        Ok(()) => log::debug!("Moved dir {} -> {}", src.display(), dst.display()),
        Err(e) => {
            log::warn!("Rename dir failed for {}, trying recursive copy: {}", src.display(), e);
            if let Err(copy_err) = copy_dir_recursive(src, dst) {
                log::error!("Failed to migrate dir {}: {}", src.display(), copy_err);
            } else {
                // Only remove source if copy succeeded
                let _ = std::fs::remove_dir_all(src);
            }
        }
    }
}

/// Copy a single file and remove the source
fn copy_and_remove(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::copy(src, dst)
        .map_err(|e| format!("Copy {} -> {} failed: {}", src.display(), dst.display(), e))?;
    std::fs::remove_file(src)
        .map_err(|e| format!("Remove {} after copy failed: {}", src.display(), e))?;
    Ok(())
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst)
        .map_err(|e| format!("Create dir {} failed: {}", dst.display(), e))?;

    let entries = std::fs::read_dir(src)
        .map_err(|e| format!("Read dir {} failed: {}", src.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Read entry in {} failed: {}", src.display(), e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Copy file {} failed: {}", src_path.display(), e))?;
        }
    }

    Ok(())
}

/// Get the path where a user's data directory would be
pub fn user_data_path(user_id: u64) -> Result<PathBuf, String> {
    let global = crate::user_data::UserDataPaths::global_data_dir()?;
    Ok(global.join("users").join(user_id.to_string()))
}

/// Get the path where a user's cache directory would be
pub fn user_cache_path(user_id: u64) -> Result<PathBuf, String> {
    let global = crate::user_data::UserDataPaths::global_cache_dir()?;
    Ok(global.join("users").join(user_id.to_string()))
}
