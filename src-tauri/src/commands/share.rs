//! Share-related Tauri commands

use tauri::State;

use crate::share::{ContentType, ShareError, SongLinkResponse};
use crate::AppState;
use std::collections::HashMap;

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

    if let Some(songlink) = resolve_songlink_from_itunes(&isrc, track_id, &state).await {
        return Ok(songlink);
    }

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
        return Err(ShareError::MissingIdentifier.to_string());
    }

    log::info!("Command: share_track_songlink URL={}", url);
    state
        .songlink
        .get_by_url(&url, ContentType::Track)
        .await
        .map_err(|e| e.to_string())
}

async fn resolve_songlink_from_itunes(
    isrc: &str,
    track_id: Option<u64>,
    state: &State<'_, AppState>,
) -> Option<SongLinkResponse> {
    if let Some(url) = lookup_itunes_by_isrc(isrc).await {
        return songlink_from_itunes_url(&url);
    }

    let track_id = track_id?;
    let (title, artist) = fetch_track_metadata(track_id, state).await?;
    let url = search_itunes_by_term(&title, &artist).await?;
    songlink_from_itunes_url(&url)
}

fn songlink_from_itunes_url(itunes_url: &str) -> Option<SongLinkResponse> {
    let track_id = extract_itunes_track_id(itunes_url)?;
    let page_url = format!("https://song.link/i/{}", track_id);
    log::info!("Using Song.link direct URL: {}", page_url);

    Some(SongLinkResponse {
        page_url,
        title: None,
        artist: None,
        thumbnail_url: None,
        platforms: HashMap::new(),
        identifier: track_id,
        content_type: ContentType::Track.as_str().to_string(),
    })
}

fn extract_itunes_track_id(itunes_url: &str) -> Option<String> {
    let query = itunes_url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut iter = pair.splitn(2, '=');
        if iter.next()? == "i" {
            return iter.next().map(|v| v.to_string());
        }
    }
    None
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

/// iTunes album lookup response
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItunesAlbumResult {
    collection_id: Option<u64>,
    collection_view_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItunesAlbumLookupResponse {
    result_count: u32,
    results: Vec<ItunesAlbumResult>,
}

/// Get album.link URL for an album
/// Tries iTunes lookup by UPC first, then by album title/artist search
#[tauri::command]
pub async fn share_album_songlink(
    upc: Option<String>,
    album_id: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    state: State<'_, AppState>,
) -> Result<SongLinkResponse, String> {
    let upc = upc.unwrap_or_default().trim().to_string();
    let title = title.unwrap_or_default().trim().to_string();
    let artist = artist.unwrap_or_default().trim().to_string();

    // Try iTunes lookup by UPC first
    if !upc.is_empty() {
        if let Some(songlink) = lookup_itunes_album_by_upc(&upc).await {
            return Ok(songlink);
        }
        log::debug!("UPC lookup failed, trying Odesli API...");

        // Try Odesli API with UPC
        match state.songlink.get_by_upc(&upc).await {
            Ok(result) => return Ok(result),
            Err(err) => log::warn!("Odesli UPC lookup failed: {}", err),
        }
    }

    // Try iTunes search by title/artist
    if !title.is_empty() && !artist.is_empty() {
        if let Some(songlink) = search_itunes_album(&title, &artist).await {
            return Ok(songlink);
        }
    }

    Err("Could not find album on music platforms".to_string())
}

async fn lookup_itunes_album_by_upc(upc: &str) -> Option<SongLinkResponse> {
    if upc.trim().is_empty() {
        return None;
    }

    log::info!("Looking up iTunes album by UPC: {}", upc);

    let url = "https://itunes.apple.com/lookup";
    let response = reqwest::Client::new()
        .get(url)
        .query(&[("upc", upc), ("country", "US")])
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let data: ItunesAlbumLookupResponse = response.json().await.ok()?;
    if data.result_count == 0 {
        return None;
    }

    let album = data.results.into_iter().find(|r| r.collection_id.is_some())?;
    let collection_id = album.collection_id?;

    albumlink_from_itunes_id(collection_id)
}

async fn search_itunes_album(title: &str, artist: &str) -> Option<SongLinkResponse> {
    let term = format!("{} {}", artist, title);
    log::info!("Searching iTunes for album: {}", term);

    let url = "https://itunes.apple.com/search";
    let response = reqwest::Client::new()
        .get(url)
        .query(&[
            ("term", term),
            ("entity", "album".to_string()),
            ("limit", "1".to_string()),
            ("country", "US".to_string()),
        ])
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let data: ItunesAlbumLookupResponse = response.json().await.ok()?;
    if data.result_count == 0 {
        return None;
    }

    let album = data.results.into_iter().find(|r| r.collection_id.is_some())?;
    let collection_id = album.collection_id?;

    albumlink_from_itunes_id(collection_id)
}

fn albumlink_from_itunes_id(collection_id: u64) -> Option<SongLinkResponse> {
    let page_url = format!("https://album.link/i/{}", collection_id);
    log::info!("Using Album.link direct URL: {}", page_url);

    Some(SongLinkResponse {
        page_url,
        title: None,
        artist: None,
        thumbnail_url: None,
        platforms: HashMap::new(),
        identifier: collection_id.to_string(),
        content_type: ContentType::Album.as_str().to_string(),
    })
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
