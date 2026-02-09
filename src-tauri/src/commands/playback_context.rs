//! Playback context commands

use tauri::State;
use crate::playback_context::{PlaybackContext, ContextType, ContentSource};
use crate::AppState;

/// Get the current playback context
#[tauri::command]
pub fn get_playback_context(state: State<'_, AppState>) -> Option<PlaybackContext> {
    state.context.get_context()
}

/// Set a new playback context
#[tauri::command]
pub fn set_playback_context(
    context_type: String,
    id: String,
    label: String,
    source: String,
    track_ids: Vec<u64>,
    start_position: usize,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Parse context type
    let ctx_type = match context_type.as_str() {
        "album" => ContextType::Album,
        "playlist" => ContextType::Playlist,
        "artist_top" => ContextType::ArtistTop,
        "home_list" => ContextType::HomeList,
        "favorites" => ContextType::Favorites,
        "local_library" => ContextType::LocalLibrary,
        "radio" => ContextType::Radio,
        _ => return Err(format!("Invalid context type: {}", context_type)),
    };

    // Parse source
    let content_source = match source.as_str() {
        "qobuz" => ContentSource::Qobuz,
        "local" => ContentSource::Local,
        "plex" => ContentSource::Plex,
        _ => return Err(format!("Invalid source: {}", source)),
    };

    let context = PlaybackContext::new(
        ctx_type,
        id,
        label,
        content_source,
        track_ids,
        start_position,
    );

    state.context.set_context(context);
    Ok(())
}

/// Clear the current playback context
#[tauri::command]
pub fn clear_playback_context(state: State<'_, AppState>) {
    state.context.clear_context();
}

/// Check if a playback context is active
#[tauri::command]
pub fn has_playback_context(state: State<'_, AppState>) -> bool {
    state.context.has_context()
}
