//! Headless session lifecycle — port of session_lifecycle.rs without Tauri.
//!
//! Initializes per-user stores after login. The stores themselves are
//! Tauri-free (SQLite); only the state lookup mechanism changes.

use std::path::PathBuf;

use crate::adapter::{DaemonEvent, RuntimeEvent};

/// Per-user state, populated after successful login.
pub struct UserSession {
    pub user_id: u64,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

/// Activate a user session (headless equivalent of session_lifecycle::activate_session).
///
/// Creates per-user directories, initializes stores, syncs audio settings to player.
pub async fn activate_session(
    user_id: u64,
    core: &qbz_core::QbzCore<crate::adapter::DaemonAdapter>,
    event_tx: &tokio::sync::broadcast::Sender<DaemonEvent>,
) -> Result<UserSession, String> {
    // Demoted from info to debug to keep user_id out of default-level logs
    // (CodeQL rust/cleartext-logging). Set RUST_LOG=debug to see.
    log::debug!("[qbzd/session] Activating session for user {}", user_id);

    // Resolve per-user directories (same layout as desktop app)
    let global_data = dirs::data_dir()
        .ok_or("Could not determine data directory")?
        .join("qbz");
    let global_cache = dirs::cache_dir()
        .ok_or("Could not determine cache directory")?
        .join("qbz");

    let data_dir = global_data.join("users").join(user_id.to_string());
    let cache_dir = global_cache.join("users").join(user_id.to_string());

    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create user data dir: {}", e))?;
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create user cache dir: {}", e))?;

    log::info!("[qbzd/session] User data dir: {}", data_dir.display());
    log::info!("[qbzd/session] User cache dir: {}", cache_dir.display());

    // Note: audio settings were already loaded at DaemonCore init from the
    // global store. Per-user settings sync will be added when qbz-player
    // exposes reload_settings() in the crate (currently on legacy player only).

    // Persist last user_id for session restore on next launch
    let marker_path = global_data.join("last_user_id");
    let _ = std::fs::write(&marker_path, user_id.to_string());
    log::info!("[qbzd/session] Saved last_user_id marker");

    // Emit session activated event
    let _ = event_tx.send(DaemonEvent::Runtime(RuntimeEvent::Ready { user_id }));

    log::info!("[qbzd/session] Session activated");

    Ok(UserSession {
        user_id,
        data_dir,
        cache_dir,
    })
}

/// Load the last user_id from the marker file (for auto-login session restore).
pub fn load_last_user_id() -> Option<u64> {
    let path = dirs::data_dir()?.join("qbz").join("last_user_id");
    std::fs::read_to_string(&path)
        .ok()?
        .trim()
        .parse()
        .ok()
}
