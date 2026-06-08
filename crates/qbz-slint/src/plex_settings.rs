//! Slint-side glue for Plex connection settings.
//!
//! The persisted model + SQLite store live in the shared, frontend-agnostic
//! `qbz_app::settings::plex` module (ADR-006). This module only owns a
//! process-global handle to that store, bound to the active user the same way
//! `tray_settings` binds the per-user `tray_settings.db`:
//!
//!   <data_dir>/qbz/users/<user_id>/plex_settings.db
//!
//! so credentials are scoped per Qobuz user. Runtime concerns — PIN auth,
//! ping, browse, library sync — live in `plex_auth` (the controller) and the
//! `qbz-plex` core crate.

use std::path::PathBuf;
use std::sync::LazyLock;

pub use qbz_app::settings::plex::{PlexSettings, PlexSettingsState};

/// Process-global Plex settings handle. Starts session-less (`new_empty`);
/// `init_for_user` binds it to the per-user DB on shell entry.
pub static STATE: LazyLock<PlexSettingsState> = LazyLock::new(PlexSettingsState::new_empty);

/// `<data_dir>/qbz/users/<user_id>/` — the per-user directory the shared
/// store appends `plex_settings.db` to. Matches the tray per-user path.
fn user_dir(user_id: u64) -> Option<PathBuf> {
    Some(
        dirs::data_dir()?
            .join("qbz")
            .join("users")
            .join(user_id.to_string()),
    )
}

/// Bind the store to `user_id` on shell entry. Best-effort — failures are
/// logged and leave the store session-less (settings degrade to defaults).
pub fn init_for_user(user_id: u64) {
    let Some(dir) = user_dir(user_id) else {
        log::warn!("[qbz-slint] plex settings: data dir unavailable, not initialized");
        return;
    };
    if let Err(e) = STATE.init_at(&dir) {
        log::error!("[qbz-slint] plex settings init failed: {e}");
    }
}

/// Current persisted settings, or `PlexSettings::default()` when there is no
/// active session / the read fails.
pub fn get() -> PlexSettings {
    STATE.get_settings().unwrap_or_default()
}

pub fn set_enabled(value: bool) {
    if let Err(e) = STATE.set_enabled(value) {
        log::error!("[qbz-slint] plex settings set_enabled failed: {e}");
    }
}

pub fn set_ui_collapsed(value: bool) {
    if let Err(e) = STATE.set_ui_collapsed(value) {
        log::error!("[qbz-slint] plex settings set_ui_collapsed failed: {e}");
    }
}

pub fn set_metadata_write_enabled(value: bool) {
    if let Err(e) = STATE.set_metadata_write_enabled(value) {
        log::error!("[qbz-slint] plex settings set_metadata_write_enabled failed: {e}");
    }
}

pub fn set_base_url(value: &str) {
    if let Err(e) = STATE.set_base_url(value) {
        log::error!("[qbz-slint] plex settings set_base_url failed: {e}");
    }
}

pub fn set_credentials(base_url: &str, token: &str) {
    if let Err(e) = STATE.set_credentials(base_url, token) {
        log::error!("[qbz-slint] plex settings set_credentials failed: {e}");
    }
}

pub fn set_token(value: &str) {
    if let Err(e) = STATE.set_token(value) {
        log::error!("[qbz-slint] plex settings set_token failed: {e}");
    }
}

pub fn set_manual_token_mode(value: bool) {
    if let Err(e) = STATE.set_manual_token_mode(value) {
        log::error!("[qbz-slint] plex settings set_manual_token_mode failed: {e}");
    }
}

pub fn set_machine_id(value: &str) {
    if let Err(e) = STATE.set_machine_id(value) {
        log::error!("[qbz-slint] plex settings set_machine_id failed: {e}");
    }
}

pub fn set_selected_section_keys(keys: &[String]) {
    if let Err(e) = STATE.set_selected_section_keys(keys) {
        log::error!("[qbz-slint] plex settings set_selected_section_keys failed: {e}");
    }
}

/// Returns the stable per-user `qbz-{uuid}` client id, generating + persisting
/// it on first use. Empty string on failure (logged).
pub fn get_or_create_client_id() -> String {
    match STATE.get_or_create_client_id() {
        Ok(id) => id,
        Err(e) => {
            log::error!("[qbz-slint] plex settings get_or_create_client_id failed: {e}");
            String::new()
        }
    }
}

pub fn disconnect() {
    if let Err(e) = STATE.disconnect() {
        log::error!("[qbz-slint] plex settings disconnect failed: {e}");
    }
}
