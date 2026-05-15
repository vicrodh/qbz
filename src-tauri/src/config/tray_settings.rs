//! Tauri commands for tray settings.
//!
//! Persisted tray preferences live in `qbz-app`. Runtime tray creation,
//! icon updates, window behavior, and emitted events remain Tauri-owned.

use log::info;

pub use qbz_app::settings::tray::{
    normalize_tray_icon_theme, TraySettings, TraySettingsState, TraySettingsStore,
};

#[tauri::command]
pub fn get_tray_settings(
    state: tauri::State<'_, TraySettingsState>,
) -> Result<TraySettings, String> {
    state.get_settings()
}

#[tauri::command]
pub fn set_enable_tray(
    value: bool,
    state: tauri::State<'_, TraySettingsState>,
) -> Result<(), String> {
    info!(
        "[TraySettings] Setting enable_tray to {} (restart required)",
        value
    );
    state.set_enable_tray(value)
}

#[tauri::command]
pub fn set_minimize_to_tray(
    value: bool,
    state: tauri::State<'_, TraySettingsState>,
) -> Result<(), String> {
    info!("[TraySettings] Setting minimize_to_tray to {}", value);
    state.set_minimize_to_tray(value)
}

#[tauri::command]
pub fn set_close_to_tray(
    value: bool,
    state: tauri::State<'_, TraySettingsState>,
) -> Result<(), String> {
    info!("[TraySettings] Setting close_to_tray to {}", value);
    state.set_close_to_tray(value)
}
