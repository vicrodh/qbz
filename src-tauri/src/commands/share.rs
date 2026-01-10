//! Share-related Tauri commands

use tauri::State;

use crate::share::{ShareError, SongLinkResponse};
use crate::AppState;

/// Get song.link URL for a track using Qobuz track ID
#[tauri::command]
pub async fn share_track_songlink(
    track_id: u64,
    state: State<'_, AppState>,
) -> Result<SongLinkResponse, String> {
    log::info!("Command: share_track_songlink track_id={}", track_id);

    state
        .songlink
        .get_by_track_id(&track_id.to_string())
        .await
        .map_err(|e| e.to_string())
}

/// Get song.link URL for an album
/// Requires UPC to be present in the album metadata
#[tauri::command]
pub async fn share_album_songlink(
    upc: Option<String>,
    state: State<'_, AppState>,
) -> Result<SongLinkResponse, String> {
    let upc = upc.ok_or_else(|| {
        ShareError::MissingUpc.to_string()
    })?;

    if upc.is_empty() {
        return Err(ShareError::MissingUpc.to_string());
    }

    log::info!("Command: share_album_songlink UPC={}", upc);

    state
        .songlink
        .get_by_upc(&upc)
        .await
        .map_err(|e| e.to_string())
}

/// Generate a Qobuz share URL for a track
#[tauri::command]
pub fn get_qobuz_track_url(track_id: u64) -> String {
    format!("https://www.qobuz.com/track/{}", track_id)
}

/// Generate a Qobuz share URL for an album
#[tauri::command]
pub fn get_qobuz_album_url(album_id: String) -> String {
    format!("https://open.qobuz.com/album/{}", album_id)
}

/// Generate a Qobuz share URL for an artist
#[tauri::command]
pub fn get_qobuz_artist_url(artist_id: u64) -> String {
    format!("https://www.qobuz.com/artist/{}", artist_id)
}
