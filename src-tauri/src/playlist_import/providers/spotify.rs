//! Spotify playlist import

use serde_json::Value;

use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::models::{ImportPlaylist, ImportProvider, ImportTrack};

// Cloudflare Workers proxy URL - handles credentials
const SPOTIFY_PROXY_URL: &str = "https://qbz-api-proxy.blitzkriegfc.workers.dev/spotify";
const SPOTIFY_API_BASE: &str = "https://api.spotify.com/v1";

/// Detect if a URL is a Spotify track, album, or playlist.
pub fn detect_resource(url: &str) -> Option<super::MusicResource> {
    let lower = url.to_ascii_lowercase();
    if !lower.contains("spotify.com/") && !lower.starts_with("spotify:") {
        return None;
    }

    // Playlist check first (so parse_playlist_id takes priority for playlists)
    if parse_playlist_id(url).is_some() {
        return Some(super::MusicResource::Playlist {
            provider: super::MusicProvider::Spotify,
        });
    }

    // Track: open.spotify.com/track/<id> or spotify:track:<id>
    if lower.contains("/track/") || lower.contains(":track:") {
        return Some(super::MusicResource::Track {
            provider: super::MusicProvider::Spotify,
            url: url.to_string(),
        });
    }

    // Album: open.spotify.com/album/<id> or spotify:album:<id>
    if lower.contains("/album/") || lower.contains(":album:") {
        return Some(super::MusicResource::Album {
            provider: super::MusicProvider::Spotify,
            url: url.to_string(),
        });
    }

    None
}

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
    // Try API with token, retry once with fresh token on failure
    for attempt in 1..=2 {
        match get_app_token().await {
            Ok(token) => {
                log::info!("Spotify: API attempt {}/2, token obtained", attempt);
                match fetch_playlist_with_token(playlist_id, &token).await {
                    Ok(playlist) => {
                        log::info!("Spotify: API returned {} tracks", playlist.tracks.len());
                        return Ok(playlist);
                    }
                    Err(e) => {
                        log::warn!("Spotify: API attempt {}/2 failed: {}", attempt, e);
                    }
                }
            }
            Err(e) => {
                log::warn!("Spotify: token attempt {}/2 failed: {}", attempt, e);
            }
        }
    }

    log::warn!(
        "Spotify: API failed after 2 attempts, falling back to embed (limited to ~100 tracks)"
    );
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
        .error_for_status()
        .map_err(|e| PlaylistImportError::Http(format!("Spotify metadata: {}", e)))?
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
    let expected_total = meta
        .get("tracks")
        .and_then(|v| v.get("total"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    log::info!(
        "Spotify: playlist '{}' reports {} tracks",
        name,
        expected_total
    );

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
                ("fields", "items(track(name,artists(name),album(name),duration_ms,external_ids,id,external_urls)),next,total".to_string()),
            ])
            .send()
            .await
            .map_err(|e| PlaylistImportError::Http(e.to_string()))?
            .error_for_status()
            .map_err(|e| PlaylistImportError::Http(format!("Spotify tracks page offset={}: {}", offset, e)))?
            .json()
            .await
            .map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

        let items = response
            .get("items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                PlaylistImportError::Parse("Spotify tracks missing items".to_string())
            })?;

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
            let duration_ms = track.get("duration_ms").and_then(|v| v.as_u64());
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

        // Use both "next" field and total count to decide pagination
        let has_next = response.get("next").map(|v| !v.is_null()).unwrap_or(false);

        let response_total = response.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        log::debug!(
            "Spotify: page offset={}, got {} items, total so far={}, response.total={}, has_next={}",
            offset, items.len(), tracks.len(), response_total, has_next
        );

        // Stop if: no next page AND we have all tracks (or response says no more)
        if !has_next && tracks.len() >= response_total {
            break;
        }

        // Safety: also stop if the page returned 0 items to prevent infinite loop
        if items.is_empty() {
            log::warn!(
                "Spotify: empty page at offset {}, stopping pagination",
                offset
            );
            break;
        }

        // Extra safety: if we already have enough tracks per the total, stop
        if response_total > 0 && tracks.len() >= response_total {
            break;
        }

        offset += limit;
    }

    if expected_total > 0 && tracks.len() < expected_total {
        log::warn!(
            "Spotify: expected {} tracks but only fetched {} — possible pagination issue",
            expected_total,
            tracks.len()
        );
    }

    Ok(ImportPlaylist {
        provider: ImportProvider::Spotify,
        provider_id: playlist_id.to_string(),
        name,
        description,
        tracks,
    })
}

async fn fetch_playlist_from_embed(
    playlist_id: &str,
) -> Result<ImportPlaylist, PlaylistImportError> {
    let url = format!("https://open.spotify.com/embed/playlist/{}", playlist_id);
    let html = reqwest::get(&url)
        .await
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?
        .text()
        .await
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?;

    let json_text = extract_script(&html, "__NEXT_DATA__").ok_or_else(|| {
        PlaylistImportError::Parse("Spotify embed missing __NEXT_DATA__".to_string())
    })?;

    let data: Value =
        serde_json::from_str(&json_text).map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

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
        let duration_ms = track.get("duration").and_then(|v| v.as_u64());
        let uri = track.get("uri").and_then(|v| v.as_str()).unwrap_or("");
        let provider_id = uri
            .split(':')
            .last()
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string());
        let provider_url = provider_id
            .as_ref()
            .map(|id| format!("https://open.spotify.com/track/{}", id));

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
    // Proxy handles credentials
    let url = format!("{}/token", SPOTIFY_PROXY_URL);

    let response: Value = reqwest::Client::builder()
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::USER_AGENT,
                reqwest::header::HeaderValue::from_static("QBZ/1.0.0"),
            );
            headers
        })
        .build()
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?
        .get(&url)
        .send()
        .await
        .map_err(|e| PlaylistImportError::Http(format!("Spotify proxy: {}", e)))?
        .error_for_status()
        .map_err(|e| PlaylistImportError::Http(format!("Spotify proxy: {}", e)))?
        .json()
        .await
        .map_err(|e| PlaylistImportError::Parse(format!("Spotify proxy response: {}", e)))?;

    response
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .ok_or_else(|| PlaylistImportError::Parse("Spotify proxy missing access_token".to_string()))
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
