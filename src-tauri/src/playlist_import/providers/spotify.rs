//! Spotify playlist import

use std::env;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::Value;

use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::models::{ImportPlaylist, ImportProvider, ImportTrack};

const SPOTIFY_TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const SPOTIFY_API_BASE: &str = "https://api.spotify.com/v1";

pub fn parse_playlist_id(url: &str) -> Option<String> {
    if let Some(rest) = url.strip_prefix("spotify:playlist:") {
        if !rest.is_empty() {
            return Some(rest.to_string());
        }
    }

    let patterns = [
        "open.spotify.com/playlist/",
        "open.spotify.com/embed/playlist/",
    ];
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
    if let Ok(token) = get_app_token().await {
        if let Ok(playlist) = fetch_playlist_with_token(playlist_id, &token).await {
            return Ok(playlist);
        }
    }

    fetch_playlist_from_embed(playlist_id).await
}

async fn fetch_playlist_with_token(
    playlist_id: &str,
    access_token: &str,
) -> Result<ImportPlaylist, PlaylistImportError> {
    let client = reqwest::Client::new();

    let meta_url = format!("{}/playlists/{}", SPOTIFY_API_BASE, playlist_id);
    let meta: Value = client
        .get(&meta_url)
        .header("Authorization", format!("Bearer {}", access_token))
        .query(&[("fields", "name,description,tracks.total")])
        .send()
        .await
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?
        .json()
        .await
        .map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

    let name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Spotify Playlist")
        .to_string();
    let description = meta
        .get("description")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty());

    let mut tracks = Vec::new();
    let mut offset = 0u32;
    let limit = 100u32;

    loop {
        let tracks_url = format!("{}/playlists/{}/tracks", SPOTIFY_API_BASE, playlist_id);
        let response: Value = client
            .get(&tracks_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .query(&[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
                ("fields", "items(track(name,artists(name),album(name),duration_ms,external_ids,id,external_urls)),next".to_string()),
            ])
            .send()
            .await
            .map_err(|e| PlaylistImportError::Http(e.to_string()))?
            .json()
            .await
            .map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

        let items = response
            .get("items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| PlaylistImportError::Parse("Spotify tracks missing items".to_string()))?;

        for item in items {
            let track = match item.get("track") {
                Some(v) if !v.is_null() => v,
                _ => continue,
            };

            let title = track
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let artist = join_artists(track.get("artists"));
            let album = track
                .get("album")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let duration_ms = track
                .get("duration_ms")
                .and_then(|v| v.as_u64());
            let isrc = track
                .get("external_ids")
                .and_then(|v| v.get("isrc"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let provider_id = track
                .get("id")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let provider_url = track
                .get("external_urls")
                .and_then(|v| v.get("spotify"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());

            tracks.push(ImportTrack {
                title,
                artist,
                album,
                duration_ms,
                isrc,
                provider_id,
                provider_url,
            });
        }

        let has_next = response
            .get("next")
            .and_then(|v| v.as_str())
            .map(|v| !v.is_empty())
            .unwrap_or(false);

        if !has_next {
            break;
        }

        offset += limit;
    }

    Ok(ImportPlaylist {
        provider: ImportProvider::Spotify,
        provider_id: playlist_id.to_string(),
        name,
        description,
        tracks,
    })
}

async fn fetch_playlist_from_embed(playlist_id: &str) -> Result<ImportPlaylist, PlaylistImportError> {
    let url = format!("https://open.spotify.com/embed/playlist/{}", playlist_id);
    let html = reqwest::get(&url)
        .await
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?
        .text()
        .await
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?;

    let json_text = extract_script(&html, "__NEXT_DATA__")
        .ok_or_else(|| PlaylistImportError::Parse("Spotify embed missing __NEXT_DATA__".to_string()))?;

    let data: Value = serde_json::from_str(&json_text)
        .map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

    let entity = data
        .get("props")
        .and_then(|v| v.get("pageProps"))
        .and_then(|v| v.get("state"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.get("entity"))
        .ok_or_else(|| PlaylistImportError::Parse("Spotify embed missing entity".to_string()))?;

    let name = entity
        .get("title")
        .or_else(|| entity.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("Spotify Playlist")
        .to_string();

    let mut tracks = Vec::new();
    let track_list = entity
        .get("trackList")
        .and_then(|v| v.as_array())
        .ok_or_else(|| PlaylistImportError::Parse("Spotify embed missing trackList".to_string()))?;

    for track in track_list {
        let title = track
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let artist = track
            .get("subtitle")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let duration_ms = track
            .get("duration")
            .and_then(|v| v.as_u64());
        let uri = track
            .get("uri")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let provider_id = uri.split(':').last().filter(|v| !v.is_empty()).map(|v| v.to_string());
        let provider_url = provider_id.as_ref().map(|id| format!("https://open.spotify.com/track/{}", id));

        tracks.push(ImportTrack {
            title,
            artist,
            album: None,
            duration_ms,
            isrc: None,
            provider_id,
            provider_url,
        });
    }

    Ok(ImportPlaylist {
        provider: ImportProvider::Spotify,
        provider_id: playlist_id.to_string(),
        name,
        description: None,
        tracks,
    })
}

async fn get_app_token() -> Result<String, PlaylistImportError> {
    let client_id = env::var("SPOTIFY_API_CLIENT_ID")
        .map_err(|_| PlaylistImportError::MissingCredentials("SPOTIFY_API_CLIENT_ID".to_string()))?;
    let client_secret = env::var("SPOTIFY_API_CLIENT_SECRET")
        .map_err(|_| PlaylistImportError::MissingCredentials("SPOTIFY_API_CLIENT_SECRET".to_string()))?;

    let auth = STANDARD.encode(format!("{}:{}", client_id, client_secret));

    let response: Value = reqwest::Client::new()
        .post(SPOTIFY_TOKEN_URL)
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
        .ok_or_else(|| PlaylistImportError::Parse("Spotify token missing access_token".to_string()))
}

fn join_artists(value: Option<&Value>) -> String {
    let artists = value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if artists.is_empty() {
        "Unknown".to_string()
    } else {
        artists.join(", ")
    }
}

fn extract_script(html: &str, id: &str) -> Option<String> {
    let marker = format!("id=\"{}\"", id);
    let start = html.find(&marker)?;
    let script_start = html[start..].find('>')? + start + 1;
    let script_end = html[script_start..].find("</script>")? + script_start;
    Some(html[script_start..script_end].to_string())
}
