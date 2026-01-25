//! Playlist-related Tauri commands

use tauri::State;

use crate::api::models::{Playlist, SearchResultsPage};
use crate::AppState;

/// Get user's playlists
#[tauri::command]
pub async fn get_user_playlists(
    state: State<'_, AppState>,
) -> Result<Vec<Playlist>, String> {
    log::info!("Command: get_user_playlists");

    let client = state.client.lock().await;
    client
        .get_user_playlists()
        .await
        .map_err(|e| format!("Failed to get user playlists: {}", e))
}

/// Get a specific playlist by ID
#[tauri::command]
pub async fn get_playlist(
    playlist_id: u64,
    state: State<'_, AppState>,
) -> Result<Playlist, String> {
    log::info!("Command: get_playlist {}", playlist_id);

    let client = state.client.lock().await;
    client
        .get_playlist(playlist_id)
        .await
        .map_err(|e| format!("Failed to get playlist: {}", e))
}

/// Search playlists
#[tauri::command]
pub async fn search_playlists(
    query: String,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Playlist>, String> {
    log::info!("Command: search_playlists \"{}\"", query);

    let client = state.client.lock().await;
    client
        .search_playlists(&query, limit.unwrap_or(20))
        .await
        .map_err(|e| format!("Failed to search playlists: {}", e))
}

/// Create a new playlist
#[tauri::command]
pub async fn create_playlist(
    name: String,
    description: Option<String>,
    is_public: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Playlist, String> {
    log::info!("Command: create_playlist \"{}\"", name);

    let client = state.client.lock().await;
    client
        .create_playlist(&name, description.as_deref(), is_public.unwrap_or(false))
        .await
        .map_err(|e| format!("Failed to create playlist: {}", e))
}

/// Delete a playlist
#[tauri::command]
pub async fn delete_playlist(
    playlist_id: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: delete_playlist {}", playlist_id);

    let client = state.client.lock().await;
    client
        .delete_playlist(playlist_id)
        .await
        .map_err(|e| format!("Failed to delete playlist: {}", e))
}

/// Add tracks to a playlist
#[tauri::command]
pub async fn add_tracks_to_playlist(
    playlist_id: u64,
    track_ids: Vec<u64>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: add_tracks_to_playlist {} ({} tracks)", playlist_id, track_ids.len());

    let client = state.client.lock().await;
    client
        .add_tracks_to_playlist(playlist_id, &track_ids)
        .await
        .map_err(|e| format!("Failed to add tracks to playlist: {}", e))
}

/// Remove tracks from a playlist
#[tauri::command]
pub async fn remove_tracks_from_playlist(
    playlist_id: u64,
    playlist_track_ids: Vec<u64>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: remove_tracks_from_playlist {} ({} tracks)", playlist_id, playlist_track_ids.len());

    let client = state.client.lock().await;
    client
        .remove_tracks_from_playlist(playlist_id, &playlist_track_ids)
        .await
        .map_err(|e| format!("Failed to remove tracks from playlist: {}", e))
}

/// Update playlist metadata
#[tauri::command]
pub async fn update_playlist(
    playlist_id: u64,
    name: Option<String>,
    description: Option<String>,
    is_public: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Playlist, String> {
    log::info!("Command: update_playlist {}", playlist_id);

    let client = state.client.lock().await;
    client
        .update_playlist(playlist_id, name.as_deref(), description.as_deref(), is_public)
        .await
        .map_err(|e| format!("Failed to update playlist: {}", e))
}

/// Get multiple tracks by their IDs
#[tauri::command]
pub async fn get_tracks_by_ids(
    track_ids: Vec<u64>,
    state: State<'_, AppState>,
) -> Result<Vec<crate::api::models::Track>, String> {
    log::info!("Command: get_tracks_by_ids ({} tracks)", track_ids.len());

    let client = state.client.lock().await;
    let mut tracks = Vec::new();

    for track_id in track_ids {
        match client.get_track(track_id).await {
            Ok(track) => tracks.push(track),
            Err(e) => {
                log::warn!("Failed to get track {}: {}", track_id, e);
                // Continue with other tracks even if one fails
            }
        }
    }

    Ok(tracks)
}

/// Get the current user's Qobuz ID
#[tauri::command]
pub async fn get_current_user_id(state: State<'_, AppState>) -> Result<Option<u64>, String> {
    log::info!("Command: get_current_user_id");

    let client = state.client.lock().await;
    Ok(client.get_user_id().await)
}

/// Subscribe to (copy) a playlist to the user's library
/// This creates a new playlist with all the tracks from the source playlist
#[tauri::command]
pub async fn subscribe_playlist(
    playlist_id: u64,
    state: State<'_, AppState>,
) -> Result<Playlist, String> {
    log::info!("Command: subscribe_playlist {}", playlist_id);

    let client = state.client.lock().await;

    // 1. Get the source playlist with all its tracks
    let source = client
        .get_playlist(playlist_id)
        .await
        .map_err(|e| format!("Failed to get source playlist: {}", e))?;

    // 2. Extract track IDs from the source playlist
    let track_ids: Vec<u64> = source
        .tracks
        .as_ref()
        .map(|t| t.items.iter().map(|track| track.id).collect())
        .unwrap_or_default();

    if track_ids.is_empty() {
        return Err("Source playlist has no tracks to copy".to_string());
    }

    // 3. Build description with attribution
    let attribution = format!("\n\n---\nOriginally curated by {} on Qobuz", source.owner.name);
    let new_description = match source.description {
        Some(ref desc) if !desc.is_empty() => Some(format!("{}{}", desc, attribution)),
        _ => Some(attribution.trim_start().to_string()),
    };

    // 4. Create a new playlist with the same name and attributed description
    let new_playlist = client
        .create_playlist(&source.name, new_description.as_deref(), false)
        .await
        .map_err(|e| format!("Failed to create new playlist: {}", e))?;

    // 5. Add all tracks to the new playlist
    client
        .add_tracks_to_playlist(new_playlist.id, &track_ids)
        .await
        .map_err(|e| format!("Failed to add tracks to new playlist: {}", e))?;

    // 6. Return the new playlist with updated track count
    Ok(Playlist {
        tracks_count: track_ids.len() as u32,
        ..new_playlist
    })
}
