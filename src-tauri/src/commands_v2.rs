//! V2 Commands - Using the new multi-crate architecture
//!
//! These commands use QbzCore via CoreBridge instead of the old AppState.
//! They coexist with the old commands during migration.

use tauri::State;

use qbz_models::{Album, Artist, QueueState, RepeatMode, Track, UserSession};

use crate::core_bridge::CoreBridgeState;

// ==================== Auth Commands (V2) ====================

/// Check if user is logged in (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_is_logged_in(
    bridge: State<'_, CoreBridgeState>,
) -> Result<bool, String> {
    let bridge = bridge.get().await;
    Ok(bridge.is_logged_in().await)
}

/// Login with email and password (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_login(
    email: String,
    password: String,
    bridge: State<'_, CoreBridgeState>,
) -> Result<UserSession, String> {
    let bridge = bridge.get().await;
    bridge.login(&email, &password).await
}

/// Logout current user (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_logout(
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.logout().await
}

// ==================== Queue Commands (V2) ====================

/// Get current queue state (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_queue_state(
    bridge: State<'_, CoreBridgeState>,
) -> Result<QueueState, String> {
    let bridge = bridge.get().await;
    Ok(bridge.get_queue_state().await)
}

/// Set repeat mode (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_set_repeat_mode(
    mode: RepeatMode,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.set_repeat_mode(mode).await;
    Ok(())
}

/// Toggle shuffle (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_toggle_shuffle(
    bridge: State<'_, CoreBridgeState>,
) -> Result<bool, String> {
    let bridge = bridge.get().await;
    Ok(bridge.toggle_shuffle().await)
}

/// Clear the queue (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_clear_queue(
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.clear_queue().await;
    Ok(())
}

// ==================== Search Commands (V2) ====================

/// Search for albums (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_search_albums(
    query: String,
    limit: u32,
    offset: u32,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Vec<Album>, String> {
    let bridge = bridge.get().await;
    bridge.search_albums(&query, limit, offset).await
}

/// Search for tracks (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_search_tracks(
    query: String,
    limit: u32,
    offset: u32,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Vec<Track>, String> {
    let bridge = bridge.get().await;
    bridge.search_tracks(&query, limit, offset).await
}

/// Search for artists (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_search_artists(
    query: String,
    limit: u32,
    offset: u32,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Vec<Artist>, String> {
    let bridge = bridge.get().await;
    bridge.search_artists(&query, limit, offset).await
}

// ==================== Catalog Commands (V2) ====================

/// Get album by ID (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_album(
    album_id: String,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Album, String> {
    let bridge = bridge.get().await;
    bridge.get_album(&album_id).await
}

/// Get track by ID (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_track(
    track_id: u64,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Track, String> {
    let bridge = bridge.get().await;
    bridge.get_track(track_id).await
}

/// Get artist by ID (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_artist(
    artist_id: u64,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Artist, String> {
    let bridge = bridge.get().await;
    bridge.get_artist(artist_id).await
}
