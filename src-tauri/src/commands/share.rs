//! Share-related Tauri commands

use tauri::State;

use crate::share::{ContentType, ShareError, SongLinkResponse};
use crate::AppState;

/// Get song.link URL for a track using ISRC or a direct URL fallback
/// Qobuz isn't supported by Odesli, so we prefer ISRC but can fall back to URL
#[tauri::command]
pub async fn share_track_songlink(
    isrc: Option<String>,
    url: Option<String>,
    state: State<'_, AppState>,
) -> Result<SongLinkResponse, String> {
    let isrc = isrc.unwrap_or_default().trim().to_string();
    let url = url.unwrap_or_default().trim().to_string();

    if !isrc.is_empty() {
        log::info!("Command: share_track_songlink ISRC={}", isrc);
        match state.songlink.get_by_isrc(&isrc).await {
            Ok(result) => return Ok(result),
            Err(err) => {
                log::warn!("ISRC lookup failed: {}", err);
                if url.is_empty() {
                    return Err(err.to_string());
                }
            }
        }
    }

    if url.is_empty() {
        return Err(ShareError::MissingIdentifier.to_string());
    }

    log::info!("Command: share_track_songlink URL={}", url);
    state
        .songlink
        .get_by_url(&url, ContentType::Track)
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
