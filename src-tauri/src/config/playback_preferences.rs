//! Tauri commands for playback and playback-adjacent UI preferences.

pub use qbz_app::settings::playback::{
    AutoplayMode, PlaybackPreferences, PlaybackPreferencesState, PlaybackPreferencesStore,
};

#[tauri::command]
pub fn get_playback_preferences(
    state: tauri::State<'_, PlaybackPreferencesState>,
) -> Result<PlaybackPreferences, String> {
    state.get_preferences()
}

#[tauri::command]
pub fn set_autoplay_mode(
    mode: String,
    state: tauri::State<'_, PlaybackPreferencesState>,
) -> Result<(), String> {
    let autoplay_mode = match mode.as_str() {
        "continue" => AutoplayMode::ContinueWithinSource,
        "track_only" => AutoplayMode::PlayTrackOnly,
        "infinite" => AutoplayMode::InfiniteRadio,
        _ => return Err(format!("Invalid autoplay mode: {}", mode)),
    };
    state.set_autoplay_mode(autoplay_mode)
}

#[tauri::command]
pub fn set_show_context_icon(
    show: bool,
    state: tauri::State<'_, PlaybackPreferencesState>,
) -> Result<(), String> {
    state.set_show_context_icon(show)
}

#[tauri::command]
pub fn set_persist_session(
    persist: bool,
    state: tauri::State<'_, PlaybackPreferencesState>,
) -> Result<(), String> {
    state.set_persist_session(persist)
}

#[tauri::command]
pub fn set_resume_playback_position(
    resume: bool,
    state: tauri::State<'_, PlaybackPreferencesState>,
) -> Result<(), String> {
    state.set_resume_playback_position(resume)
}
