//! V2 Commands - Using the new multi-crate architecture
//!
//! These commands use QbzCore via CoreBridge instead of the old AppState.
//! They coexist with the old commands during migration.
//!
//! For playback commands, we use a bridge pattern:
//! - CoreBridge for Qobuz API (get stream URL)
//! - AppState.player for actual audio playback (until player moves to qbz-player crate)

use tauri::State;

use qbz_models::{Album, Artist, QueueState, RepeatMode, SearchResultsPage, Track, UserSession};

use crate::artist_blacklist::BlacklistState;
use crate::core_bridge::CoreBridgeState;
use crate::AppState;

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
#[allow(non_snake_case)]
pub async fn v2_search_albums(
    query: String,
    limit: u32,
    offset: u32,
    searchType: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchResultsPage<Album>, String> {
    let bridge = bridge.get().await;
    let mut results = bridge.search_albums(&query, limit, offset, searchType.as_deref()).await?;

    // Filter out albums from blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|album| !blacklist_state.is_blacklisted(album.artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} albums from search results", filtered_count);
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

/// Search for tracks (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_search_tracks(
    query: String,
    limit: u32,
    offset: u32,
    searchType: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchResultsPage<Track>, String> {
    let bridge = bridge.get().await;
    let mut results = bridge.search_tracks(&query, limit, offset, searchType.as_deref()).await?;

    // Filter out tracks from blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|track| {
        if let Some(ref performer) = track.performer {
            !blacklist_state.is_blacklisted(performer.id)
        } else {
            true // Keep tracks without performer info
        }
    });

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} tracks from search results", filtered_count);
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

/// Search for artists (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_search_artists(
    query: String,
    limit: u32,
    offset: u32,
    searchType: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchResultsPage<Artist>, String> {
    let bridge = bridge.get().await;
    let mut results = bridge.search_artists(&query, limit, offset, searchType.as_deref()).await?;

    // Filter out blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|artist| !blacklist_state.is_blacklisted(artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} artists from search results", filtered_count);
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

// ==================== Catalog Commands (V2) ====================

/// Get album by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_album(
    albumId: String,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Album, String> {
    let bridge = bridge.get().await;
    bridge.get_album(&albumId).await
}

/// Get track by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_track(
    trackId: u64,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Track, String> {
    let bridge = bridge.get().await;
    bridge.get_track(trackId).await
}

/// Get artist by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_artist(
    artistId: u64,
    bridge: State<'_, CoreBridgeState>,
) -> Result<Artist, String> {
    let bridge = bridge.get().await;
    bridge.get_artist(artistId).await
}

// ==================== Playback Commands (V2) ====================
//
// These commands use AppState.player for actual playback (bridge pattern).
// Eventually the player should move to qbz-player crate, but for now
// we keep it in src-tauri and bridge to it.

/// Pause playback (V2)
#[tauri::command]
pub async fn v2_pause_playback(
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.player.pause()
}

/// Resume playback (V2)
#[tauri::command]
pub async fn v2_resume_playback(
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.player.resume()
}

/// Stop playback (V2)
#[tauri::command]
pub async fn v2_stop_playback(
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.player.stop()
}

/// Seek to position in seconds (V2)
#[tauri::command]
pub async fn v2_seek(
    position: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.player.seek(position)
}

/// Set volume (0.0 - 1.0) (V2)
#[tauri::command]
pub async fn v2_set_volume(
    volume: f32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.player.set_volume(volume)
}

/// Get current playback state (V2)
#[tauri::command]
pub async fn v2_get_playback_state(
    state: State<'_, AppState>,
) -> Result<crate::player::PlaybackState, String> {
    let player_state = &state.player.state;
    Ok(crate::player::PlaybackState {
        is_playing: player_state.is_playing(),
        position: player_state.current_position(),
        duration: player_state.duration(),
        track_id: player_state.current_track_id(),
        volume: player_state.volume(),
    })
}
