//! Deezer playlist import

use serde_json::Value;

use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::models::{ImportPlaylist, ImportProvider, ImportTrack};

/// Detect if a URL is a Deezer track, album, or playlist.
///
/// Deezer URLs:
/// - Track: `deezer.com/track/{id}` or `deezer.com/{locale}/track/{id}`
/// - Album: `deezer.com/album/{id}` or `deezer.com/{locale}/album/{id}`
/// - Playlist: `deezer.com/playlist/{id}` or `deezer.com/{locale}/playlist/{id}`
pub fn detect_resource(url: &str) -> Option<super::MusicResource> {
    if !url.contains("deezer.com") {
        return None;
    }

    // Playlist
    if parse_playlist_id(url).is_some() {
        return Some(super::MusicResource::Playlist {
            provider: super::MusicProvider::Deezer,
        });
    }

    let parts: Vec<&str> = url.split('/').collect();
    for (idx, part) in parts.iter().enumerate() {
        match *part {
            "track" => {
                if parts.get(idx + 1).map(|s| !s.is_empty()).unwrap_or(false) {
                    return Some(super::MusicResource::Track {
                        provider: super::MusicProvider::Deezer,
                        url: url.to_string(),
                    });
                }
            }
            "album" => {
                if parts.get(idx + 1).map(|s| !s.is_empty()).unwrap_or(false) {
                    return Some(super::MusicResource::Album {
                        provider: super::MusicProvider::Deezer,
                        url: url.to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    None
}

pub fn parse_playlist_id(url: &str) -> Option<String> {
    if !url.contains("deezer.com") {
        return None;
    }

    let parts: Vec<&str> = url.split('/').collect();
    for (idx, part) in parts.iter().enumerate() {
        if *part == "playlist" {
            let id = parts.get(idx + 1)?.split('?').next()?;
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }

    None
}

pub async fn fetch_playlist(playlist_id: &str) -> Result<ImportPlaylist, PlaylistImportError> {
    let url = format!("https://api.deezer.com/playlist/{}", playlist_id);
    let data: Value = reqwest::get(&url)
        .await
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?
        .json()
        .await
        .map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

    let name = data
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Deezer Playlist")
        .to_string();
    let description = data
        .get("description")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty());

    let mut tracks = Vec::new();
    let items = data
        .get("tracks")
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| PlaylistImportError::Parse("Deezer tracks missing".to_string()))?;

    for item in items {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let artist = item
            .get("artist")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let album = item
            .get("album")
            .and_then(|v| v.get("title"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        let duration_ms = item
            .get("duration")
            .and_then(|v| v.as_u64())
            .map(|v| v * 1000);
        let isrc = item
            .get("isrc")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        let provider_id = item
            .get("id")
            .and_then(|v| v.as_u64())
            .map(|v| v.to_string());
        let provider_url = item
            .get("link")
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

    Ok(ImportPlaylist {
        provider: ImportProvider::Deezer,
        provider_id: playlist_id.to_string(),
        name,
        description,
        tracks,
    })
}
