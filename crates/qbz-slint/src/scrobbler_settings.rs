//! Slint-side glue for scrobbler (Last.fm + ListenBrainz) settings.
//!
//! The persisted model + SQLite store live in the shared, frontend-agnostic
//! `qbz_app::settings::scrobblers` module (ADR-006). This module only owns a
//! process-global handle to that store, bound to the active user the same way
//! `plex_settings` binds the per-user `plex_settings.db`:
//!
//!   <data_dir>/qbz/users/<user_id>/scrobbler_settings.db
//!
//! so credentials are scoped per Qobuz user. It also records the bound user
//! directory, which the `scrobble` controller needs to reach the two SHARED
//! per-user stores the offline queues live in (same files Tauri uses):
//!
//!   <user_dir>/offline_settings.db          — Last.fm `scrobble_queue`
//!   <user_dir>/cache/listenbrainz_v2.db     — LB credentials + `listen_queue`
//!
//! Runtime concerns — the auth flows, the now-playing + scrobble fire, and the
//! offline flush — live in the `scrobble` controller and call the
//! `qbz-integrations` clients directly.

use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

pub use qbz_app::settings::scrobblers::{ScrobblerSettings, ScrobblerSettingsState};

/// Process-global scrobbler settings handle. Starts session-less (`new_empty`);
/// `init_for_user` binds it to the per-user DB on shell entry.
static STATE: LazyLock<ScrobblerSettingsState> =
    LazyLock::new(ScrobblerSettingsState::new_empty);

/// The bound per-user data directory (`init_for_user`). The `scrobble`
/// controller resolves the shared offline-queue stores against it.
static USER_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// `<data_dir>/qbz/users/<user_id>/` — the per-user directory the shared store
/// appends `scrobbler_settings.db` to. Matches the Plex/tray per-user path.
fn user_dir_for(user_id: u64) -> Option<PathBuf> {
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
    let Some(dir) = user_dir_for(user_id) else {
        log::warn!("[qbz-slint] scrobbler settings: data dir unavailable, not initialized");
        return;
    };
    if let Err(e) = STATE.init_at(&dir) {
        log::error!("[qbz-slint] scrobbler settings init failed: {e}");
        return;
    }
    if let Ok(mut guard) = USER_DIR.lock() {
        *guard = Some(dir);
    }
}

/// The active user's data dir, or `None` before shell entry.
pub fn user_dir() -> Option<PathBuf> {
    USER_DIR.lock().ok().and_then(|g| g.clone())
}

/// Current persisted settings, or `ScrobblerSettings::default()` when there is
/// no active session / the read fails.
pub fn get() -> ScrobblerSettings {
    STATE.get_settings().unwrap_or_default()
}

pub fn set_enabled(value: bool) {
    if let Err(e) = STATE.set_enabled(value) {
        log::error!("[qbz-slint] scrobbler settings set_enabled failed: {e}");
    }
}

pub fn set_ui_collapsed(value: bool) {
    if let Err(e) = STATE.set_ui_collapsed(value) {
        log::error!("[qbz-slint] scrobbler settings set_ui_collapsed failed: {e}");
    }
}

pub fn set_lastfm_enabled(value: bool) {
    if let Err(e) = STATE.set_lastfm_enabled(value) {
        log::error!("[qbz-slint] scrobbler settings set_lastfm_enabled failed: {e}");
    }
}

pub fn set_lastfm_session(key: &str, username: &str) {
    if let Err(e) = STATE.set_lastfm_session(key, username) {
        log::error!("[qbz-slint] scrobbler settings set_lastfm_session failed: {e}");
    }
}

pub fn disconnect_lastfm() {
    if let Err(e) = STATE.disconnect_lastfm() {
        log::error!("[qbz-slint] scrobbler settings disconnect_lastfm failed: {e}");
    }
}

pub fn set_listenbrainz_enabled(value: bool) {
    if let Err(e) = STATE.set_listenbrainz_enabled(value) {
        log::error!("[qbz-slint] scrobbler settings set_listenbrainz_enabled failed: {e}");
    }
}

pub fn set_listenbrainz_token(token: &str, username: &str) {
    if let Err(e) = STATE.set_listenbrainz_token(token, username) {
        log::error!("[qbz-slint] scrobbler settings set_listenbrainz_token failed: {e}");
    }
}

pub fn disconnect_listenbrainz() {
    if let Err(e) = STATE.disconnect_listenbrainz() {
        log::error!("[qbz-slint] scrobbler settings disconnect_listenbrainz failed: {e}");
    }
}
