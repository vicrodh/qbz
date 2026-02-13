//! Queue management Tauri commands

use tauri::State;

use crate::artist_blacklist::BlacklistState;
use crate::queue::{QueueState, QueueTrack, RepeatMode};
use crate::AppState;

/// Check if a track's artist is blacklisted
fn is_track_blacklisted(track: &QueueTrack, blacklist: &BlacklistState) -> bool {
    if let Some(artist_id) = track.artist_id {
        blacklist.is_blacklisted(artist_id)
    } else {
        false
    }
}

/// Add a track to the queue
#[tauri::command]
pub fn add_to_queue(
    track: QueueTrack,
    state: State<'_, AppState>,
    blacklist: State<'_, BlacklistState>,
) -> Result<(), String> {
    if is_track_blacklisted(&track, &blacklist) {
        log::debug!("Skipping blacklisted track: {} by {}", track.title, track.artist);
        return Ok(());
    }
    log::info!("Command: add_to_queue - {} by {}", track.title, track.artist);
    state.queue.add_track(track);
    Ok(())
}

/// Add a track to play next
#[tauri::command]
pub fn add_to_queue_next(
    track: QueueTrack,
    state: State<'_, AppState>,
    blacklist: State<'_, BlacklistState>,
) -> Result<(), String> {
    if is_track_blacklisted(&track, &blacklist) {
        log::debug!("Skipping blacklisted track (next): {} by {}", track.title, track.artist);
        return Ok(());
    }
    log::info!("Command: add_to_queue_next - {} by {}", track.title, track.artist);
    state.queue.add_track_next(track);
    Ok(())
}

/// Add multiple tracks to the queue
#[tauri::command]
pub fn add_tracks_to_queue(
    tracks: Vec<QueueTrack>,
    state: State<'_, AppState>,
    blacklist: State<'_, BlacklistState>,
) -> Result<(), String> {
    let filtered: Vec<QueueTrack> = tracks
        .into_iter()
        .filter(|t| !is_track_blacklisted(t, &blacklist))
        .collect();
    log::info!("Command: add_tracks_to_queue - {} tracks (after blacklist filter)", filtered.len());
    state.queue.add_tracks(filtered);
    Ok(())
}

/// Set the entire queue (replaces existing)
#[tauri::command]
pub fn set_queue(
    tracks: Vec<QueueTrack>,
    start_index: Option<usize>,
    state: State<'_, AppState>,
    blacklist: State<'_, BlacklistState>,
) -> Result<(), String> {
    let filtered: Vec<QueueTrack> = tracks
        .into_iter()
        .filter(|t| !is_track_blacklisted(t, &blacklist))
        .collect();
    log::info!("Command: set_queue - {} tracks (after blacklist filter), start at {:?}", filtered.len(), start_index);
    state.queue.set_queue(filtered, start_index);
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

/// Remove a track from the upcoming queue by its position in the upcoming list
/// (0 = first upcoming track, handles shuffle mode correctly)
#[tauri::command]
pub fn remove_upcoming_track(upcoming_index: usize, state: State<'_, AppState>) -> Result<Option<QueueTrack>, String> {
    log::info!("Command: remove_upcoming_track - upcoming_index {}", upcoming_index);
    Ok(state.queue.remove_upcoming_track(upcoming_index))
}

/// Move a track from one position to another in the queue
#[tauri::command]
pub fn move_queue_track(from_index: usize, to_index: usize, state: State<'_, AppState>) -> Result<bool, String> {
    log::info!("Command: move_queue_track - from {} to {}", from_index, to_index);
    Ok(state.queue.move_track(from_index, to_index))
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
