//! Slint-side glue for tray preferences.
//!
//! The persisted model + SQLite store live in the shared, frontend-agnostic
//! `qbz_app::settings::tray` module (ADR-006). This module only owns a
//! process-global handle to that store, bound to the active user the same way
//! `library_db` binds the per-user `library.db`:
//!
//!   <data_dir>/qbz/users/<user_id>/tray_settings.db
//!
//! so the Slint and Tauri builds share the exact same per-user tray settings
//! and the same `normalize_tray_icon_theme` semantics. Runtime tray creation,
//! icon updates and window hide/show live elsewhere (the `tray` module).

use std::path::PathBuf;
use std::sync::LazyLock;

pub use qbz_app::settings::tray::{normalize_tray_icon_theme, TraySettings};
use qbz_app::settings::tray::TraySettingsState;

/// Process-global tray settings handle. Starts session-less (`new_empty`);
/// `init_for_user` binds it to the per-user DB on shell entry.
static STATE: LazyLock<TraySettingsState> = LazyLock::new(TraySettingsState::new_empty);

/// The on-screen order of the tray icon variant dropdown
/// (`AppearanceState.tray-icon-themes`): Auto / Mono light / Mono dark / Color.
/// Indices map to the canonical `tray_icon_theme` string values.
const THEME_BY_INDEX: [&str; 4] = ["auto", "mono-light", "mono-dark", "color"];

/// `<data_dir>/qbz/users/<user_id>/` — the per-user directory the shared
/// store appends `tray_settings.db` to. Matches the Tauri per-user path.
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
        log::warn!("[qbz-slint] tray settings: data dir unavailable, not initialized");
        return;
    };
    if let Err(e) = STATE.init_at(&dir) {
        log::error!("[qbz-slint] tray settings init failed: {e}");
    }
}

/// Current persisted settings, or `TraySettings::default()` when there is no
/// active session / the read fails.
pub fn get() -> TraySettings {
    STATE.get_settings().unwrap_or_default()
}

pub fn set_enable_tray(value: bool) {
    if let Err(e) = STATE.set_enable_tray(value) {
        log::error!("[qbz-slint] tray settings set_enable_tray failed: {e}");
    }
}

pub fn set_minimize_to_tray(value: bool) {
    if let Err(e) = STATE.set_minimize_to_tray(value) {
        log::error!("[qbz-slint] tray settings set_minimize_to_tray failed: {e}");
    }
}

pub fn set_close_to_tray(value: bool) {
    if let Err(e) = STATE.set_close_to_tray(value) {
        log::error!("[qbz-slint] tray settings set_close_to_tray failed: {e}");
    }
}

pub fn set_mac_hide_dock(value: bool) {
    if let Err(e) = STATE.set_mac_hide_dock(value) {
        log::error!("[qbz-slint] tray settings set_mac_hide_dock failed: {e}");
    }
}

/// Persist the tray icon variant from the dropdown index (0..=3). Out-of-range
/// indices fall back to `"auto"`.
pub fn set_icon_theme_index(index: i32) {
    let theme = THEME_BY_INDEX
        .get(usize::try_from(index).unwrap_or(0))
        .copied()
        .unwrap_or("auto");
    if let Err(e) = STATE.set_tray_icon_theme(theme) {
        log::error!("[qbz-slint] tray settings set_tray_icon_theme failed: {e}");
    }
}

/// Canonical theme string for a dropdown index (0..=3). Out-of-range → "auto".
/// Used to push the live `set_icon_theme` to the running tray.
pub fn theme_for_index(index: i32) -> String {
    THEME_BY_INDEX
        .get(usize::try_from(index).unwrap_or(0))
        .copied()
        .unwrap_or("auto")
        .to_string()
}

/// Dropdown index (0..=3) for a canonical theme string. Unknown → 0 (Auto).
pub fn icon_theme_index(theme: &str) -> i32 {
    let normalized = normalize_tray_icon_theme(theme);
    THEME_BY_INDEX
        .iter()
        .position(|t| *t == normalized)
        .map(|i| i as i32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_round_trips_canonical_values() {
        assert_eq!(icon_theme_index("auto"), 0);
        assert_eq!(icon_theme_index("mono-light"), 1);
        assert_eq!(icon_theme_index("mono-dark"), 2);
        assert_eq!(icon_theme_index("color"), 3);
        // Legacy + unknown normalize before lookup.
        assert_eq!(icon_theme_index("light"), 1);
        assert_eq!(icon_theme_index("dark"), 2);
        assert_eq!(icon_theme_index("bogus"), 0);
    }
}
