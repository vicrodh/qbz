//! Playback-related Tauri commands

use tauri::State;

use crate::api::models::Quality;
use crate::player::PlaybackState;
use crate::AppState;

/// Play a track by ID
#[tauri::command]
pub async fn play_track(track_id: u64, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: play_track {}", track_id);

    let client = state.client.lock().await;

    // Default to Hi-Res quality, will fallback if not available
    state
        .player
        .play_track(&client, track_id, Quality::HiRes)
        .await
}

/// Pause playback
#[tauri::command]
pub fn pause_playback(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: pause_playback");
    state.media_controls.set_playback(false);
    state.player.pause()
}

/// Resume playback
#[tauri::command]
pub fn resume_playback(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: resume_playback");
    state.media_controls.set_playback(true);
    state.player.resume()
}

/// Stop playback
#[tauri::command]
pub fn stop_playback(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: stop_playback");
    state.media_controls.set_stopped();
    state.player.stop()
}

/// Set media controls metadata (for MPRIS integration)
#[tauri::command]
pub fn set_media_metadata(
    title: String,
    artist: String,
    album: String,
    duration_secs: Option<u64>,
    cover_url: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: set_media_metadata - {} by {}", title, artist);
    crate::update_media_controls_metadata(
        &state.media_controls,
        &title,
        &artist,
        &album,
        duration_secs,
        cover_url,
    );
    state.media_controls.set_playback(true);
    Ok(())
}

/// Set volume (0.0 - 1.0)
#[tauri::command]
pub fn set_volume(volume: f32, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: set_volume {}", volume);
    state.player.set_volume(volume)
}

/// Seek to position in seconds
#[tauri::command]
pub fn seek(position: u64, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: seek {}", position);
    state.player.seek(position)
}

/// Get current playback state
#[tauri::command]
pub fn get_playback_state(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    state.player.get_state()
}
