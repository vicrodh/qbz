//! Per-user session activation and teardown
//!
//! After login, the frontend calls `activate_user_session` with the Qobuz user_id.
//! This initializes all per-user database stores at the user-scoped directory.
//! On logout, `deactivate_user_session` tears everything down.
//!
//! These legacy commands now delegate to session_lifecycle module.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::user_data::UserDataPaths;

/// Helper to init a type-alias state (Arc<Mutex<Option<Store>>>) at a path
pub fn init_type_alias_state<S, F>(
    state: &Arc<Mutex<Option<S>>>,
    base_dir: &Path,
    constructor: F,
) -> Result<(), String>
where
    F: FnOnce(&Path) -> Result<S, String>,
{
    let store = constructor(base_dir)?;
    let mut guard = state.lock().map_err(|e| format!("Lock error: {}", e))?;
    *guard = Some(store);
    Ok(())
}

/// Helper to teardown a type-alias state
pub fn teardown_type_alias_state<S>(state: &Arc<Mutex<Option<S>>>) {
    if let Ok(mut guard) = state.lock() {
        *guard = None;
    }
}

/// Get the last active user_id for session restore on startup.
/// Returns None if no previous session or after explicit logout.
#[tauri::command]
pub fn get_last_user_id() -> Option<u64> {
    UserDataPaths::load_last_user_id()
}

/// Activate the per-user session after login.
///
/// This runs the one-time migration (if needed) and initializes all
/// per-user database stores at `~/.local/share/qbz/users/{user_id}/`
/// and cache stores at `~/.cache/qbz/users/{user_id}/`.
///
/// This is a legacy command - now delegates to session_lifecycle::activate_session().
#[tauri::command]
pub async fn activate_user_session(app: tauri::AppHandle, user_id: u64) -> Result<(), String> {
    log::info!("[Legacy] activate_user_session called - delegating to session_lifecycle");
    crate::session_lifecycle::activate_session(&app, user_id).await
}

/// Deactivate the per-user session on logout.
///
/// Tears down all per-user stores, closing database connections.
/// This is a legacy command - now delegates to session_lifecycle::deactivate_session().
#[tauri::command]
pub async fn deactivate_user_session(app: tauri::AppHandle) -> Result<(), String> {
    log::info!("[Legacy] deactivate_user_session called - delegating to session_lifecycle");
    crate::session_lifecycle::deactivate_session(&app).await
}
