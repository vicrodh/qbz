//! Visualizer Commands
//!
//! Tauri commands for controlling the audio visualizer.

use tauri::State;
use crate::AppState;

/// Enable or disable the audio visualizer
#[tauri::command]
pub fn set_visualizer_enabled(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.visualizer.set_enabled(enabled);
    Ok(())
}

/// Check if the visualizer is enabled
#[tauri::command]
pub fn is_visualizer_enabled(state: State<'_, AppState>) -> bool {
    state.visualizer.is_enabled()
}
