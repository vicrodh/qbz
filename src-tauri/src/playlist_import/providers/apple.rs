//! Apple Music playlist import

use serde_json::Value;

use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::models::{ImportPlaylist, ImportProvider, ImportTrack};

pub fn parse_playlist_id(url: &str) -> Option<(String, String)> {
    if !url.contains("music.apple.com/") {
        return None;
    }

    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() < 6 {
        return None;
    }

    let storefront = parts.get(3)?.to_string();
    let playlist_id = parts.last()?.split('?').next()?.to_string();

    if playlist_id.starts_with("pl.") || playlist_id.starts_with("pl.u-") {
        Some((storefront, playlist_id))
    } else {
        None
    }
}

pub async fn fetch_playlist(storefront: &str, playlist_id: &str) -> Result<ImportPlaylist, PlaylistImportError> {
    let url = format!("https://music.apple.com/{}/playlist/{}", storefront, playlist_id);
    let html = reqwest::get(&url)
        .await
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?
        .text()
        .await
        .map_err(|e| PlaylistImportError::Http(e.to_string()))?;

    let name = extract_meta(&html, "og:title").unwrap_or_else(|| "Apple Music Playlist".to_string());
    let description = extract_meta(&html, "og:description").filter(|v| !v.is_empty());

    let json_text = extract_script(&html, "serialized-server-data")
        .ok_or_else(|| PlaylistImportError::Parse("Apple Music serialized-server-data not found".to_string()))?;

    let data: Value = serde_json::from_str(&json_text)
        .map_err(|e| PlaylistImportError::Parse(e.to_string()))?;

    let items = find_track_items(&data)
        .ok_or_else(|| PlaylistImportError::Parse("Apple Music track list not found".to_string()))?;

    let mut tracks = Vec::new();
    for item in items {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let artist = item
            .get("artistName")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let duration_ms = item
            .get("duration")
            .and_then(|v| v.as_u64());
        let provider_id = item
            .get("contentDescriptor")
            .and_then(|v| v.get("identifiers"))
            .and_then(|v| v.get("storeAdamID"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        let provider_url = item
            .get("contentDescriptor")
            .and_then(|v| v.get("url"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        let album = item
            .get("tertiaryLinks")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("title"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());

        tracks.push(ImportTrack {
            title,
            artist,
            album,
            duration_ms,
            isrc: None,
            provider_id,
            provider_url,
        });
    }

    Ok(ImportPlaylist {
        provider: ImportProvider::AppleMusic,
        provider_id: playlist_id.to_string(),
        name,
        description,
        tracks,
    })
}

fn extract_script(html: &str, id: &str) -> Option<String> {
    let marker = format!("id=\"{}\"", id);
    let start = html.find(&marker)?;
    let script_start = html[start..].find('>')? + start + 1;
    let script_end = html[script_start..].find("</script>")? + script_start;
    let raw = &html[script_start..script_end];
    Some(unescape_basic(raw))
}

fn find_track_items(data: &Value) -> Option<Vec<&Value>> {
    match data {
        Value::Object(map) => {
            if map.get("itemKind").and_then(|v| v.as_str()) == Some("trackLockup") {
                let items = map.get("items").and_then(|v| v.as_array())?;
                if !items.is_empty() {
                    return Some(items.iter().collect());
                }
            }

            for value in map.values() {
                if let Some(found) = find_track_items(value) {
                    return Some(found);
                }
            }
        }
        Value::Array(list) => {
            for value in list {
                if let Some(found) = find_track_items(value) {
                    return Some(found);
                }
            }
        }
        _ => {}
    }

    None
}

fn extract_meta(html: &str, property: &str) -> Option<String> {
    let needle = format!("property=\"{}\"", property);
    let start = html.find(&needle)?;
    let content_start = html[start..].find("content=\"")? + start + "content=\"".len();
    let content_end = html[content_start..].find('"')? + content_start;
    Some(unescape_basic(&html[content_start..content_end]))
}

fn unescape_basic(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}
