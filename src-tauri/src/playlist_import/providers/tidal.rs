//! Tidal playlist import (OpenAPI v2)

use std::env;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::Value;

use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::models::{ImportPlaylist, ImportProvider, ImportTrack};

const TIDAL_TOKEN_URL: &str = "https://auth.tidal.com/v1/oauth2/token";
const TIDAL_API_BASE: &str = "https://openapi.tidal.com/v2";

pub fn parse_playlist_id(url: &str) -> Option<String> {
    if !url.contains("tidal.com") {
        return None;
    }

    let patterns = ["/browse/playlist/", "/playlist/"];
    for pattern in patterns {
        if let Some(idx) = url.find(pattern) {
            let mut part = &url[idx + pattern.len()..];
            if let Some(end) = part.find('?') {
                part = &part[..end];
            }
            if !part.is_empty() {
                return Some(part.to_string());
            }
        }
    }

    None
}

pub async fn fetch_playlist(playlist_id: &str) -> Result<ImportPlaylist, PlaylistImportError> {
    let token = get_app_token().await?;
    let country_code = env::var("TIDAL_COUNTRY_CODE").unwrap_or_else(|_| "US".to_string());

    let client = reqwest::Client::new();
    let meta_url = format!("{}/playlists/{}", TIDAL_API_BASE, playlist_id);
    let meta: Value = client
        .get(&meta_url)
        .header("Authorization", format!("Bearer {}", token))
        .query(&[("countryCode", &country_code)])
        .send()
        .await
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?
        .json()
        .await
        .map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

    let name = meta
        .get("data")
        .and_then(|v| v.get("attributes"))
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("Tidal Playlist")
        .to_string();
    let description = meta
        .get("data")
        .and_then(|v| v.get("attributes"))
        .and_then(|v| v.get("description"))
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty());

    let track_ids = fetch_track_ids(&client, &token, playlist_id, &country_code).await?;
    let tracks = fetch_tracks_by_ids(&client, &token, &track_ids, &country_code).await?;

    Ok(ImportPlaylist {
        provider: ImportProvider::Tidal,
        provider_id: playlist_id.to_string(),
        name,
        description,
        tracks,
    })
}

async fn fetch_track_ids(
    client: &reqwest::Client,
    token: &str,
    playlist_id: &str,
    country_code: &str,
) -> Result<Vec<String>, PlaylistImportError> {
    let mut ids = Vec::new();
    let mut next_path = format!("/playlists/{}/relationships/items?limit=100", playlist_id);

    loop {
        let url = format!("{}{}", TIDAL_API_BASE, next_path);
        let mut request = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token));
        if !next_path.contains("countryCode=") {
            request = request.query(&[("countryCode", country_code)]);
        }
        let response: Value = request
            .send()
            .await
            .map_err(|e| PlaylistImportError::Http(e.to_string()))?
            .json()
            .await
            .map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

        if let Some(data) = response.get("data").and_then(|v| v.as_array()) {
            for item in data {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    ids.push(id.to_string());
                }
            }
        }

        let next = response
            .get("links")
            .and_then(|v| v.get("next"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());

        match next {
            Some(path) if !path.is_empty() => {
                next_path = path;
            }
            _ => break,
        }
    }

    Ok(ids)
}

async fn fetch_tracks_by_ids(
    client: &reqwest::Client,
    token: &str,
    track_ids: &[String],
    country_code: &str,
) -> Result<Vec<ImportTrack>, PlaylistImportError> {
    let mut tracks = Vec::new();
    let mut chunk_start = 0usize;
    let chunk_size = 50usize;

    while chunk_start < track_ids.len() {
        let end = (chunk_start + chunk_size).min(track_ids.len());
        let chunk = &track_ids[chunk_start..end];

        let url = format!("{}/tracks", TIDAL_API_BASE);
        let response: Value = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .query(&[
                ("filter[id]", chunk.join(",")),
                ("include", "artists,albums".to_string()),
                ("countryCode", country_code.to_string()),
            ])
            .send()
            .await
            .map_err(|e| PlaylistImportError::Http(e.to_string()))?
            .json()
            .await
            .map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

        let included = response
            .get("included")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut artist_map = std::collections::HashMap::new();
        let mut album_map = std::collections::HashMap::new();

        for item in included {
            if let Some(item_type) = item.get("type").and_then(|v| v.as_str()) {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    continue;
                }

                match item_type {
                    "artists" => {
                        if let Some(name) = item
                            .get("attributes")
                            .and_then(|v| v.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            artist_map.insert(id.to_string(), name.to_string());
                        }
                    }
                    "albums" => {
                        if let Some(name) = item
                            .get("attributes")
                            .and_then(|v| v.get("title"))
                            .and_then(|v| v.as_str())
                        {
                            album_map.insert(id.to_string(), name.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Some(data) = response.get("data").and_then(|v| v.as_array()) {
            for item in data {
                let title = item
                    .get("attributes")
                    .and_then(|v| v.get("title"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string();
                let isrc = item
                    .get("attributes")
                    .and_then(|v| v.get("isrc"))
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());
                let duration_ms = item
                    .get("attributes")
                    .and_then(|v| v.get("duration"))
                    .and_then(|v| v.as_str())
                    .and_then(|v| parse_duration_ms(v));

                let artist = item
                    .get("relationships")
                    .and_then(|v| v.get("artists"))
                    .and_then(|v| v.get("data"))
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .and_then(|id| artist_map.get(id))
                    .cloned()
                    .unwrap_or_else(|| "Unknown".to_string());

                let album = item
                    .get("relationships")
                    .and_then(|v| v.get("albums"))
                    .and_then(|v| v.get("data"))
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .and_then(|id| album_map.get(id))
                    .cloned();

                let provider_id = item
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());

                tracks.push(ImportTrack {
                    title,
                    artist,
                    album,
                    duration_ms,
                    isrc,
                    provider_id,
                    provider_url: None,
                });
            }
        }

        chunk_start = end;
    }

    Ok(tracks)
}

fn parse_duration_ms(value: &str) -> Option<u64> {
    if !value.starts_with('P') {
        return None;
    }

    let mut seconds = 0u64;
    let mut parsed_any = false;
    let mut num = String::new();
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
            continue;
        }

        if num.is_empty() {
            continue;
        }

        let value_num: u64 = num.parse().ok()?;
        parsed_any = true;
        match ch {
            'H' => seconds += value_num * 3600,
            'M' => seconds += value_num * 60,
            'S' => seconds += value_num,
            _ => {}
        }
        num.clear();
    }

    if !parsed_any {
        None
    } else {
        Some(seconds * 1000)
    }
}

async fn get_app_token() -> Result<String, PlaylistImportError> {
    let client_id = env::var("TIDAL_API_CLIENT_ID")
        .map_err(|_| PlaylistImportError::MissingCredentials("TIDAL_API_CLIENT_ID".to_string()))?;
    let client_secret = env::var("TIDAL_API_CLIENT_SECRET")
        .map_err(|_| PlaylistImportError::MissingCredentials("TIDAL_API_CLIENT_SECRET".to_string()))?;

    let auth = STANDARD.encode(format!("{}:{}", client_id, client_secret));

    let response: Value = reqwest::Client::new()
        .post(TIDAL_TOKEN_URL)
        .header("Authorization", format!("Basic {}", auth))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials")
        .send()
        .await
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?
        .json()
        .await
        .map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

    response
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .ok_or_else(|| PlaylistImportError::Parse("Tidal token missing access_token".to_string()))
}
