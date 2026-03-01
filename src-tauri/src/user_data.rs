//! Per-user data path management
//!
//! Each Qobuz user gets their own subdirectory under the app's data/cache paths.
//! This module provides the central path provider that all per-user state modules
//! use to determine where to store their databases.

use std::path::PathBuf;
use std::sync::RwLock;

/// Central path provider for per-user data isolation.
///
/// Holds the current user_id and provides methods to get
/// user-scoped data and cache directories.
pub struct UserDataPaths {
    user_id: RwLock<Option<u64>>,
}

impl UserDataPaths {
    pub fn new() -> Self {
        Self {
            user_id: RwLock::new(None),
        }
    }

    /// Set the current user after login
    pub fn set_user(&self, user_id: u64) {
        *self
            .user_id
            .write()
            .expect("UserDataPaths write lock poisoned") = Some(user_id);
        log::info!("UserDataPaths: active user set");
    }

    /// Clear the current user on logout
    pub fn clear_user(&self) {
        *self
            .user_id
            .write()
            .expect("UserDataPaths write lock poisoned") = None;
        log::info!("UserDataPaths: active user cleared");
    }

    /// Get the current user ID, if set
    pub fn current_user_id(&self) -> Option<u64> {
        *self
            .user_id
            .read()
            .expect("UserDataPaths read lock poisoned")
    }

    /// Get the user-scoped data directory: ~/.local/share/qbz/users/{uid}/
    pub fn user_data_dir(&self) -> Result<PathBuf, String> {
        let uid = self
            .user_id
            .read()
            .map_err(|e| format!("UserDataPaths read lock error: {}", e))?
            .ok_or("No active user - please log in")?;

        let base = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz")
            .join("users")
            .join(uid.to_string());

        Ok(base)
    }

    /// Get the user-scoped cache directory: ~/.cache/qbz/users/{uid}/
    pub fn user_cache_dir(&self) -> Result<PathBuf, String> {
        let uid = self
            .user_id
            .read()
            .map_err(|e| format!("UserDataPaths read lock error: {}", e))?
            .ok_or("No active user - please log in")?;

        let base = dirs::cache_dir()
            .ok_or("Could not determine cache directory")?
            .join("qbz")
            .join("users")
            .join(uid.to_string());

        Ok(base)
    }

    /// Get the global (non-user-scoped) data directory: ~/.local/share/qbz/
    pub fn global_data_dir() -> Result<PathBuf, String> {
        dirs::data_dir()
            .ok_or_else(|| "Could not determine data directory".to_string())
            .map(|d| d.join("qbz"))
    }

    /// Get the global (non-user-scoped) cache directory: ~/.cache/qbz/
    pub fn global_cache_dir() -> Result<PathBuf, String> {
        dirs::cache_dir()
            .ok_or_else(|| "Could not determine cache directory".to_string())
            .map(|d| d.join("qbz"))
    }

    /// Save the last active user_id to a flat-path file so the session
    /// can be restored on next app launch (when "remember me" is active).
    pub fn save_last_user_id(user_id: u64) -> Result<(), String> {
        let path = Self::last_user_id_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("Failed to create global data directory: {}", e))?;
        }
        std::fs::write(&path, user_id.to_string())
            .map_err(|e| format!("Failed to save last user id: {}", e))?;
        log::info!("Saved last_user_id marker");
        Ok(())
    }

    /// Read the last active user_id (returns None if file doesn't exist or is invalid).
    pub fn load_last_user_id() -> Option<u64> {
        let path = Self::last_user_id_path().ok()?;
        let contents = std::fs::read_to_string(&path).ok()?;
        contents.trim().parse::<u64>().ok()
    }

    /// Clear the last user_id file (called on explicit logout).
    pub fn clear_last_user_id() {
        if let Ok(path) = Self::last_user_id_path() {
            let _ = std::fs::remove_file(&path);
            log::info!("Cleared last_user_id file");
        }
    }

    fn last_user_id_path() -> Result<PathBuf, String> {
        let dir = Self::global_data_dir()?;
        Ok(dir.join("last_user_id"))
    }
}
