//! Tauri commands for settings export/import via the desktop UI.
//!
//! These commands handle the JSON serialization/deserialization layer.
//! File I/O (open/save dialogs) is handled on the frontend side.

use crate::runtime::RuntimeError;
use crate::settings_export::{
    apply_audio, apply_developer, apply_download, apply_favorites, apply_graphics,
    apply_image_cache, apply_playback, apply_remote_control, apply_tray, apply_window,
    ExportedSettings,
};

/// Collect all exportable settings and return them as a pretty-printed JSON string.
///
/// No file I/O is performed here — the frontend receives the string and saves it
/// via the system save dialog (Tauri dialog plugin or equivalent).
#[tauri::command]
pub fn v2_export_settings_json() -> Result<String, RuntimeError> {
    let settings = ExportedSettings::collect();
    serde_json::to_string_pretty(&settings)
        .map_err(|e| RuntimeError::Internal(format!("Failed to serialize settings: {}", e)))
}

/// Accept a JSON string (from a file the frontend opened via dialog) and apply
/// each non-None settings section to its corresponding store.
///
/// All per-field errors are collected and returned together rather than aborting
/// on the first failure, so a partial import is still applied.
#[tauri::command]
pub fn v2_import_settings_json(json: String) -> Result<(), RuntimeError> {
    let settings: ExportedSettings = serde_json::from_str(&json)
        .map_err(|e| RuntimeError::Internal(format!("Failed to parse settings JSON: {}", e)))?;

    let mut errors: Vec<String> = Vec::new();

    if let Some(audio) = settings.audio {
        apply_audio(&audio, &mut errors);
    }
    if let Some(download) = settings.download {
        apply_download(&download, &mut errors);
    }
    if let Some(playback) = settings.playback {
        apply_playback(&playback, &mut errors);
    }
    if let Some(favorites) = settings.favorites {
        apply_favorites(&favorites, &mut errors);
    }
    if let Some(tray) = settings.tray {
        apply_tray(&tray, &mut errors);
    }
    if let Some(graphics) = settings.graphics {
        apply_graphics(&graphics, &mut errors);
    }
    if let Some(developer) = settings.developer {
        apply_developer(&developer, &mut errors);
    }
    if let Some(remote_control) = settings.remote_control {
        apply_remote_control(&remote_control, &mut errors);
    }
    if let Some(window) = settings.window {
        apply_window(&window, &mut errors);
    }
    if let Some(image_cache) = settings.image_cache {
        apply_image_cache(&image_cache, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(RuntimeError::Internal(format!(
            "{} error(s) during import:\n{}",
            errors.len(),
            errors.join("\n")
        )))
    }
}
