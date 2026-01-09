//! Queue management Tauri commands

use tauri::State;

use crate::queue::{QueueState, QueueTrack, RepeatMode};
use crate::AppState;

/// Add a track to the queue
#[tauri::command]
pub fn add_to_queue(track: QueueTrack, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: add_to_queue - {} by {}", track.title, track.artist);
    state.queue.add_track(track);
    Ok(())
}

/// Add multiple tracks to the queue
#[tauri::command]
pub fn add_tracks_to_queue(tracks: Vec<QueueTrack>, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: add_tracks_to_queue - {} tracks", tracks.len());
    state.queue.add_tracks(tracks);
    Ok(())
}

/// Set the entire queue (replaces existing)
#[tauri::command]
pub fn set_queue(tracks: Vec<QueueTrack>, start_index: Option<usize>, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: set_queue - {} tracks, start at {:?}", tracks.len(), start_index);
    state.queue.set_queue(tracks, start_index);
    Ok(())
}

/// Clear the queue
#[tauri::command]
pub fn clear_queue(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: clear_queue");
    state.queue.clear();
    Ok(())
}

/// Remove a track from the queue by index
#[tauri::command]
pub fn remove_from_queue(index: usize, state: State<'_, AppState>) -> Result<Option<QueueTrack>, String> {
    log::info!("Command: remove_from_queue - index {}", index);
    Ok(state.queue.remove_track(index))
}

/// Get current track in queue
#[tauri::command]
pub fn get_current_queue_track(state: State<'_, AppState>) -> Result<Option<QueueTrack>, String> {
    Ok(state.queue.current_track())
}

/// Get next track (for pre-loading/display)
#[tauri::command]
pub fn peek_next_track(state: State<'_, AppState>) -> Result<Option<QueueTrack>, String> {
    Ok(state.queue.peek_next())
}

/// Advance to next track and return it
#[tauri::command]
pub fn next_track(state: State<'_, AppState>) -> Result<Option<QueueTrack>, String> {
    log::info!("Command: next_track");
    Ok(state.queue.next())
}

/// Go to previous track and return it
#[tauri::command]
pub fn previous_track(state: State<'_, AppState>) -> Result<Option<QueueTrack>, String> {
    log::info!("Command: previous_track");
    Ok(state.queue.previous())
}

/// Jump to a specific track by index
#[tauri::command]
pub fn play_queue_index(index: usize, state: State<'_, AppState>) -> Result<Option<QueueTrack>, String> {
    log::info!("Command: play_queue_index - {}", index);
    Ok(state.queue.play_index(index))
}

/// Set shuffle mode
#[tauri::command]
pub fn set_shuffle(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: set_shuffle - {}", enabled);
    state.queue.set_shuffle(enabled);
    Ok(())
}

/// Get shuffle status
#[tauri::command]
pub fn get_shuffle(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.queue.is_shuffle())
}

/// Set repeat mode
#[tauri::command]
pub fn set_repeat(mode: String, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: set_repeat - {}", mode);
    let repeat_mode = match mode.to_lowercase().as_str() {
        "all" => RepeatMode::All,
        "one" => RepeatMode::One,
        _ => RepeatMode::Off,
    };
    state.queue.set_repeat(repeat_mode);
    Ok(())
}

/// Get repeat mode
#[tauri::command]
pub fn get_repeat(state: State<'_, AppState>) -> Result<String, String> {
    let mode = state.queue.get_repeat();
    Ok(match mode {
        RepeatMode::Off => "off".to_string(),
        RepeatMode::All => "all".to_string(),
        RepeatMode::One => "one".to_string(),
    })
}

/// Get full queue state for frontend
#[tauri::command]
pub fn get_queue_state(state: State<'_, AppState>) -> Result<QueueState, String> {
    Ok(state.queue.get_state())
}
