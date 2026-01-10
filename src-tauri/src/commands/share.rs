//! Share-related Tauri commands

use tauri::State;

use crate::share::{ContentType, ShareError, SongLinkResponse};
use crate::AppState;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItunesLookupResponse {
    result_count: u32,
    results: Vec<ItunesTrackResult>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItunesTrackResult {
    track_view_url: Option<String>,
    collection_view_url: Option<String>,
}

/// Get song.link URL for a track using ISRC or a direct URL fallback
/// Qobuz isn't supported by Odesli, so we prefer ISRC but can fall back to URL
#[tauri::command]
pub async fn share_track_songlink(
    isrc: Option<String>,
    url: Option<String>,
    track_id: Option<u64>,
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
                if url.is_empty() && track_id.is_none() {
                    return Err(err.to_string());
                }
            }
        }
    }

    if url.is_empty() {
        if let Some(track_id) = track_id {
            if let Some(itunes_url) = resolve_itunes_url(track_id, &isrc, &state).await {
                return state
                    .songlink
                    .get_by_url(&itunes_url, ContentType::Track)
                    .await
                    .map_err(|e| e.to_string());
            }
        }

        return Err(ShareError::MissingIdentifier.to_string());
    }

    log::info!("Command: share_track_songlink URL={}", url);
    match state.songlink.get_by_url(&url, ContentType::Track).await {
        Ok(result) => Ok(result),
        Err(err) => {
            log::warn!("URL lookup failed: {}", err);
            if let Some(track_id) = track_id {
                if let Some(itunes_url) = resolve_itunes_url(track_id, &isrc, &state).await {
                    return state
                        .songlink
                        .get_by_url(&itunes_url, ContentType::Track)
                        .await
                        .map_err(|e| e.to_string());
                }
            }
            Err(err.to_string())
        }
    }
}

async fn resolve_itunes_url(
    track_id: u64,
    isrc: &str,
    state: &State<'_, AppState>,
) -> Option<String> {
    if let Some(url) = lookup_itunes_by_isrc(isrc).await {
        return Some(url);
    }

    let (title, artist) = match fetch_track_metadata(track_id, state).await {
        Some(meta) => meta,
        None => return None,
    };

    search_itunes_by_term(&title, &artist).await
}

async fn fetch_track_metadata(
    track_id: u64,
    state: &State<'_, AppState>,
) -> Option<(String, String)> {
    let client = state.client.lock().await;
    let track = client.get_track(track_id).await.ok()?;
    let title = track.title.trim().to_string();
    let artist = track
        .performer
        .map(|a| a.name)
        .unwrap_or_else(|| "".to_string())
        .trim()
        .to_string();

    if title.is_empty() || artist.is_empty() {
        return None;
    }

    Some((title, artist))
}

async fn lookup_itunes_by_isrc(isrc: &str) -> Option<String> {
    if isrc.trim().is_empty() {
        return None;
    }

    let url = "https://itunes.apple.com/lookup";
    let response = reqwest::Client::new()
        .get(url)
        .query(&[("isrc", isrc), ("country", "US")])
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let data: ItunesLookupResponse = response.json().await.ok()?;
    if data.result_count == 0 {
        return None;
    }

    data.results
        .into_iter()
        .find_map(|result| result.track_view_url.or(result.collection_view_url))
}

async fn search_itunes_by_term(title: &str, artist: &str) -> Option<String> {
    let term = format!("{} {}", artist, title);
    let url = "https://itunes.apple.com/search";
    let response = reqwest::Client::new()
        .get(url)
        .query(&[
            ("term", term),
            ("entity", "song".to_string()),
            ("limit", "1".to_string()),
            ("country", "US".to_string()),
        ])
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let data: ItunesLookupResponse = response.json().await.ok()?;
    if data.result_count == 0 {
        return None;
    }

    data.results
        .into_iter()
        .find_map(|result| result.track_view_url.or(result.collection_view_url))
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
