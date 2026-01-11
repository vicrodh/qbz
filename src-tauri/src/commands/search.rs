//! Search commands

use tauri::State;

use crate::api::{Album, Artist, SearchResultsPage, Track};
use crate::AppState;

#[tauri::command]
pub async fn search_albums(
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Album>, String> {
    let client = state.client.lock().await;
    client
        .search_albums(&query, limit.unwrap_or(20), offset.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_tracks(
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Track>, String> {
    let client = state.client.lock().await;
    client
        .search_tracks(&query, limit.unwrap_or(20), offset.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_artists(
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Artist>, String> {
    let client = state.client.lock().await;
    client
        .search_artists(&query, limit.unwrap_or(20), offset.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_album(album_id: String, state: State<'_, AppState>) -> Result<Album, String> {
    let client = state.client.lock().await;
    client.get_album(&album_id).await.map_err(|e| e.to_string())
}

/// Get featured albums by type (new-releases, press-awards)
#[tauri::command]
pub async fn get_featured_albums(
    featured_type: String,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Album>, String> {
    let client = state.client.lock().await;
    client
        .get_featured_albums(&featured_type, limit.unwrap_or(12), offset.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_track(track_id: u64, state: State<'_, AppState>) -> Result<Track, String> {
    let client = state.client.lock().await;
    client.get_track(track_id).await.map_err(|e| e.to_string())
}

/// Get artist with albums
#[tauri::command]
pub async fn get_artist(
    artist_id: u64,
    state: State<'_, AppState>,
) -> Result<Artist, String> {
    log::info!("Command: get_artist {}", artist_id);
    let client = state.client.lock().await;
    client
        .get_artist(artist_id, true)
        .await
        .map_err(|e| e.to_string())
}
