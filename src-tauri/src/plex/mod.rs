//! Plex LAN-only POC integration.
//!
//! This module intentionally avoids transcoding endpoints and uses `/library/parts/.../file...`
//! so playback uses original media bytes served by Plex Media Server.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexServerInfo {
    pub friendly_name: Option<String>,
    pub version: Option<String>,
    pub machine_identifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexMusicSection {
    pub key: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexTrack {
    pub rating_key: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub artwork_path: Option<String>,
    pub part_key: Option<String>,
    pub container: Option<String>,
    pub codec: Option<String>,
    pub channels: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub sampling_rate_hz: Option<u32>,
    pub bit_depth: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexPlayResult {
    pub rating_key: String,
    pub part_key: String,
    pub part_url: String,
    pub bytes: usize,
    pub direct_play_confirmed: bool,
    pub content_type: Option<String>,
    pub sampling_rate_hz: Option<u32>,
    pub bit_depth: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct PlexResolvedMedia {
    pub rating_key: String,
    pub playback_id: u64,
    pub part_key: String,
    pub part_url: String,
    pub bytes: Vec<u8>,
    pub direct_play_confirmed: bool,
    pub content_type: Option<String>,
    pub sampling_rate_hz: Option<u32>,
    pub bit_depth: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexPinStartResult {
    pub pin_id: u64,
    pub code: String,
    pub auth_url: String,
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexPinCheckResult {
    pub authorized: bool,
    pub expired: bool,
    pub auth_token: Option<String>,
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexCachedAlbum {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artwork_path: Option<String>,
    pub track_count: u32,
    pub total_duration_secs: u64,
    pub format: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: u32,
    pub source: String,
    pub likely_single_file_album: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexCachedTrack {
    pub id: u64,
    pub rating_key: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
    pub format: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: u32,
    pub artwork_path: Option<String>,
    pub source: String,
    pub album_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlexTrackQualityUpdate {
    pub rating_key: String,
    pub container: Option<String>,
    pub sampling_rate_hz: Option<u32>,
    pub bit_depth: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlexPinResponse {
    id: u64,
    code: String,
    #[serde(default)]
    auth_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Default)]
struct TrackBuilder {
    rating_key: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration_ms: Option<u64>,
    artwork_path: Option<String>,
    part_key: Option<String>,
    container: Option<String>,
    codec: Option<String>,
    channels: Option<u32>,
    bitrate_kbps: Option<u32>,
    sampling_rate_hz: Option<u32>,
    bit_depth: Option<u32>,
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn open_plex_cache_db() -> Result<Connection, String> {
    let data_dir = dirs::data_dir()
        .ok_or("Could not determine data directory")?
        .join("qbz");
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create Plex cache dir: {}", e))?;

    let db_path = data_dir.join("plex_cache.db");
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open Plex cache database: {}", e))?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .map_err(|e| format!("Failed to enable WAL for Plex cache database: {}", e))?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS plex_cache_sections (
            section_key TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            server_id TEXT,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS plex_cache_tracks (
            rating_key TEXT PRIMARY KEY,
            section_key TEXT NOT NULL,
            server_id TEXT,
            title TEXT NOT NULL,
            artist TEXT,
            album TEXT,
            duration_ms INTEGER,
            artwork_path TEXT,
            part_key TEXT,
            container TEXT,
            codec TEXT,
            channels INTEGER,
            bitrate_kbps INTEGER,
            sampling_rate_hz INTEGER,
            bit_depth INTEGER,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_plex_cache_tracks_section ON plex_cache_tracks(section_key);
        ",
    )
    .map_err(|e| format!("Failed to initialize Plex cache schema: {}", e))?;

    Ok(conn)
}

fn build_plex_client() -> Result<reqwest::Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert("X-Plex-Product", HeaderValue::from_static("QBZ"));
    headers.insert("X-Plex-Version", HeaderValue::from_static("0.1-poc"));
    headers.insert("X-Plex-Device", HeaderValue::from_static("QBZ Desktop"));
    headers.insert(
        "X-Plex-Platform",
        HeaderValue::from_static(std::env::consts::OS),
    );
    headers.insert(
        "X-Plex-Client-Identifier",
        HeaderValue::from_static("qbz-plex-lan-poc"),
    );

    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("Failed to create Plex HTTP client: {}", e))
}

fn build_plex_auth_client(client_identifier: &str) -> Result<reqwest::Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert("X-Plex-Product", HeaderValue::from_static("QBZ"));
    headers.insert("X-Plex-Version", HeaderValue::from_static("0.1-poc"));
    headers.insert("X-Plex-Device", HeaderValue::from_static("QBZ Desktop"));
    headers.insert(
        "X-Plex-Platform",
        HeaderValue::from_static(std::env::consts::OS),
    );
    headers.insert(
        "X-Plex-Client-Identifier",
        HeaderValue::from_str(client_identifier)
            .map_err(|e| format!("Invalid Plex client identifier: {}", e))?,
    );
    headers.insert("Accept", HeaderValue::from_static("application/json"));

    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("Failed to create Plex auth HTTP client: {}", e))
}

fn build_plex_auth_url() -> String {
    "https://plex.tv/link".to_string()
}

fn with_token(url: &str, token: &str) -> String {
    let sep = if url.contains('?') { "&" } else { "?" };
    format!("{url}{sep}X-Plex-Token={token}")
}

fn parse_u64(v: Option<String>) -> Option<u64> {
    v.and_then(|s| s.parse::<u64>().ok())
}

fn parse_u32(v: Option<String>) -> Option<u32> {
    v.and_then(|s| s.parse::<u32>().ok())
}

fn decode_xml_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'&' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b';' && (j - i) <= 12 {
                j += 1;
            }

            if j < bytes.len() && bytes[j] == b';' {
                let entity = &input[i + 1..j];
                let decoded = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                        u32::from_str_radix(&entity[2..], 16)
                            .ok()
                            .and_then(char::from_u32)
                    }
                    _ if entity.starts_with('#') => {
                        entity[1..].parse::<u32>().ok().and_then(char::from_u32)
                    }
                    _ => None,
                };

                if let Some(ch) = decoded {
                    out.push(ch);
                    i = j + 1;
                    continue;
                }
            }
        }

        if let Some(ch) = input[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    out
}

fn normalize_album_title(artist: Option<&str>, album: &str) -> String {
    let trimmed_album = album.trim();
    let Some(artist_value) = artist.map(str::trim).filter(|a| !a.is_empty()) else {
        return trimmed_album.to_string();
    };

    for sep in [" - ", " — ", " – ", ": "] {
        let prefix = format!("{artist_value}{sep}");
        if trimmed_album.starts_with(&prefix) {
            return trimmed_album[prefix.len()..].trim().to_string();
        }
    }

    trimmed_album.to_string()
}

fn get_attr(tag: &str, key: &str) -> Option<String> {
    let tag_bytes = tag.as_bytes();
    let key_bytes = key.as_bytes();
    if key_bytes.is_empty() || tag_bytes.len() < key_bytes.len() + 2 {
        return None;
    }

    let mut i = 0usize;
    while i + key_bytes.len() + 2 <= tag_bytes.len() {
        if &tag_bytes[i..i + key_bytes.len()] != key_bytes {
            i += 1;
            continue;
        }

        let prev = if i == 0 { b' ' } else { tag_bytes[i - 1] };
        if !prev.is_ascii_whitespace() && prev != b'<' {
            i += 1;
            continue;
        }

        let eq_idx = i + key_bytes.len();
        if eq_idx >= tag_bytes.len() || tag_bytes[eq_idx] != b'=' {
            i += 1;
            continue;
        }
        let quote_idx = eq_idx + 1;
        if quote_idx >= tag_bytes.len() || tag_bytes[quote_idx] != b'"' {
            i += 1;
            continue;
        }

        let value_start = quote_idx + 1;
        let value_rel_end = tag[value_start..].find('"')?;
        let value_end = value_start + value_rel_end;
        return Some(decode_xml_entities(tag[value_start..value_end].trim()));
    }

    None
}

fn find_first_tag(xml: &str, tag_name: &str) -> Option<String> {
    let needle = format!("<{tag_name}");
    let start = xml.find(&needle)?;
    let rest = &xml[start..];
    let end = rest.find('>')?;
    Some(rest[..=end].to_string())
}

fn collect_start_tags(xml: &str, tag_name: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let open = format!("<{tag_name}");
    let mut offset = 0usize;

    while let Some(pos) = xml[offset..].find(&open) {
        let start = offset + pos;
        let rest = &xml[start..];
        let Some(end) = rest.find('>') else {
            break;
        };
        tags.push(rest[..=end].to_string());
        offset = start + end + 1;
    }

    tags
}

fn collect_tag_blocks(xml: &str, tag_name: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let open = format!("<{tag_name}");
    let close = format!("</{tag_name}>");
    let mut offset = 0usize;

    while let Some(pos) = xml[offset..].find(&open) {
        let start = offset + pos;
        let rest = &xml[start..];
        let Some(open_end_rel) = rest.find('>') else {
            break;
        };
        let open_end = start + open_end_rel;
        let start_tag = &xml[start..=open_end];
        let is_self_closing = start_tag.trim_end().ends_with("/>");

        if is_self_closing {
            blocks.push((start_tag.to_string(), String::new()));
            offset = open_end + 1;
            continue;
        }

        let after_open = open_end + 1;
        let Some(close_rel) = xml[after_open..].find(&close) else {
            break;
        };
        let close_start = after_open + close_rel;
        let inner = &xml[after_open..close_start];
        blocks.push((start_tag.to_string(), inner.to_string()));
        offset = close_start + close.len();
    }

    blocks
}

fn parse_server_info(xml: &str) -> PlexServerInfo {
    let tag = find_first_tag(xml, "MediaContainer");
    PlexServerInfo {
        friendly_name: tag.as_ref().and_then(|t| get_attr(t, "friendlyName")),
        version: tag.as_ref().and_then(|t| get_attr(t, "version")),
        machine_identifier: tag.as_ref().and_then(|t| get_attr(t, "machineIdentifier")),
    }
}

fn parse_music_sections(xml: &str) -> Vec<PlexMusicSection> {
    let mut sections = Vec::new();
    for tag in collect_start_tags(xml, "Directory") {
        if get_attr(&tag, "type").as_deref() != Some("artist") {
            continue;
        }
        if let (Some(key), Some(title)) = (get_attr(&tag, "key"), get_attr(&tag, "title")) {
            sections.push(PlexMusicSection { key, title });
        }
    }
    sections
}

fn parse_track_block(start_tag: &str, inner_xml: &str) -> Option<PlexTrack> {
    let mut t = TrackBuilder {
        rating_key: get_attr(start_tag, "ratingKey"),
        title: get_attr(start_tag, "title"),
        artist: get_attr(start_tag, "grandparentTitle")
            .or_else(|| get_attr(start_tag, "originalTitle")),
        album: get_attr(start_tag, "parentTitle"),
        duration_ms: parse_u64(get_attr(start_tag, "duration")),
        artwork_path: get_attr(start_tag, "thumb")
            .or_else(|| get_attr(start_tag, "parentThumb"))
            .or_else(|| get_attr(start_tag, "grandparentThumb")),
        ..Default::default()
    };

    if let Some(title) = t.title.as_mut() {
        *title = title.trim().to_string();
    }
    if let Some(artist) = t.artist.as_mut() {
        *artist = artist.trim().to_string();
    }
    if let Some(album) = t.album.as_mut() {
        let normalized = normalize_album_title(t.artist.as_deref(), album);
        *album = normalized;
    }

    for media_tag in collect_start_tags(inner_xml, "Media") {
        t.container = get_attr(&media_tag, "container");
        t.bitrate_kbps = parse_u32(get_attr(&media_tag, "bitrate"));
        t.sampling_rate_hz = parse_u32(get_attr(&media_tag, "samplingRate"));
        t.bit_depth = parse_u32(get_attr(&media_tag, "bitDepth"));
        break;
    }

    // Stream metadata is more accurate for audio details than Media-level fields.
    // Prefer selected audio stream when available, otherwise use the first audio stream.
    let mut selected_audio_stream: Option<String> = None;
    let mut first_audio_stream: Option<String> = None;
    for stream_tag in collect_start_tags(inner_xml, "Stream") {
        let is_audio = get_attr(&stream_tag, "streamType").as_deref() == Some("2")
            || get_attr(&stream_tag, "codecType").as_deref() == Some("audio");
        if !is_audio {
            continue;
        }
        if first_audio_stream.is_none() {
            first_audio_stream = Some(stream_tag.clone());
        }
        if get_attr(&stream_tag, "selected").as_deref() == Some("1") {
            selected_audio_stream = Some(stream_tag);
            break;
        }
    }

    if let Some(stream_tag) = selected_audio_stream.or(first_audio_stream) {
        if let Some(codec) = get_attr(&stream_tag, "codec") {
            t.codec = Some(codec);
        }
        if let Some(channels) = parse_u32(get_attr(&stream_tag, "channels")) {
            t.channels = Some(channels);
        }
        if let Some(bitrate) = parse_u32(get_attr(&stream_tag, "bitrate")) {
            t.bitrate_kbps = Some(bitrate);
        }
        if let Some(rate) = parse_u32(get_attr(&stream_tag, "samplingRate")) {
            t.sampling_rate_hz = Some(rate);
        }
        if let Some(depth) = parse_u32(get_attr(&stream_tag, "bitDepth")) {
            t.bit_depth = Some(depth);
        }
    }

    for part_tag in collect_start_tags(inner_xml, "Part") {
        t.part_key = get_attr(&part_tag, "key");
        if t.part_key.is_some() {
            break;
        }
    }

    let (Some(rating_key), Some(title)) = (t.rating_key, t.title) else {
        return None;
    };

    Some(PlexTrack {
        rating_key,
        title,
        artist: t.artist,
        album: t.album,
        duration_ms: t.duration_ms,
        artwork_path: t.artwork_path,
        part_key: t.part_key,
        container: t.container,
        codec: t.codec,
        channels: t.channels,
        bitrate_kbps: t.bitrate_kbps,
        sampling_rate_hz: t.sampling_rate_hz,
        bit_depth: t.bit_depth,
    })
}

fn parse_tracks(xml: &str, limit: Option<u32>) -> Vec<PlexTrack> {
    let mut tracks = Vec::new();

    for (start_tag, inner_xml) in collect_tag_blocks(xml, "Track") {
        if let Some(track) = parse_track_block(&start_tag, &inner_xml) {
            tracks.push(track);
            if let Some(max) = limit {
                if tracks.len() >= max as usize {
                    break;
                }
            }
        }
    }

    tracks
}

fn synthetic_track_id(rating_key: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    rating_key.hash(&mut hasher);
    hasher.finish()
}

fn playback_track_id(rating_key: &str) -> u64 {
    rating_key
        .parse::<u64>()
        .unwrap_or_else(|_| synthetic_track_id(rating_key))
}

fn plex_album_key(artist: &str, album: &str) -> String {
    let mut hasher = DefaultHasher::new();
    artist.hash(&mut hasher);
    "::".hash(&mut hasher);
    album.hash(&mut hasher);
    format!("plex:{}", hasher.finish())
}

fn is_direct_part_key(part_key: &str) -> bool {
    part_key.starts_with("/library/parts/") && part_key.contains("/file")
}

#[tauri::command]
pub async fn plex_ping(base_url: String, token: String) -> Result<PlexServerInfo, String> {
    let client = build_plex_client()?;
    let base = normalize_base_url(&base_url);
    let url = with_token(&format!("{base}/"), &token);

    let xml = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Plex ping request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Plex ping status error: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read Plex ping response: {}", e))?;

    Ok(parse_server_info(&xml))
}

#[tauri::command]
pub async fn plex_get_music_sections(
    base_url: String,
    token: String,
) -> Result<Vec<PlexMusicSection>, String> {
    let client = build_plex_client()?;
    let base = normalize_base_url(&base_url);
    let url = with_token(&format!("{base}/library/sections"), &token);

    let xml = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Plex sections request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Plex sections status error: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read Plex sections response: {}", e))?;

    Ok(parse_music_sections(&xml))
}

#[tauri::command]
pub async fn plex_get_section_tracks(
    base_url: String,
    token: String,
    section_key: String,
    limit: Option<u32>,
) -> Result<Vec<PlexTrack>, String> {
    let client = build_plex_client()?;
    let base = normalize_base_url(&base_url);
    let list_url = format!("{base}/library/sections/{section_key}/all?type=10");
    let url = with_token(&list_url, &token);

    let xml = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Plex tracks request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Plex tracks status error: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read Plex tracks response: {}", e))?;

    // Treat limit=0 as "no limit" to match frontend semantics.
    let effective_limit = limit.filter(|v| *v > 0);
    Ok(parse_tracks(&xml, effective_limit))
}

#[tauri::command]
pub async fn plex_get_track_metadata(
    base_url: String,
    token: String,
    rating_key: String,
) -> Result<PlexTrack, String> {
    let client = build_plex_client()?;
    let base = normalize_base_url(&base_url);
    let detail_url = format!("{base}/library/metadata/{rating_key}");
    let url = with_token(&detail_url, &token);

    let xml = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Plex track metadata request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Plex track metadata status error: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read Plex track metadata response: {}", e))?;

    parse_tracks(&xml, Some(1))
        .into_iter()
        .next()
        .ok_or_else(|| "Plex track metadata not found".to_string())
}

#[tauri::command]
pub async fn plex_auth_pin_start(client_identifier: String) -> Result<PlexPinStartResult, String> {
    let client = build_plex_auth_client(&client_identifier)?;
    let pin = client
        .post("https://plex.tv/api/v2/pins?strong=false")
        .send()
        .await
        .map_err(|e| format!("Plex auth pin request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Plex auth pin status error: {}", e))?
        .json::<PlexPinResponse>()
        .await
        .map_err(|e| format!("Failed to parse Plex auth pin response: {}", e))?;

    Ok(PlexPinStartResult {
        pin_id: pin.id,
        code: pin.code.clone(),
        auth_url: build_plex_auth_url(),
        expires_in: pin.expires_in,
    })
}

#[tauri::command]
pub async fn plex_auth_pin_check(
    client_identifier: String,
    pin_id: u64,
    code: Option<String>,
) -> Result<PlexPinCheckResult, String> {
    let client = build_plex_auth_client(&client_identifier)?;
    let base_url = format!("https://plex.tv/api/v2/pins/{}", pin_id);
    let request = if let Some(pin_code) = code {
        client.get(format!("{}?code={}", base_url, pin_code))
    } else {
        client.get(base_url)
    };
    let pin = request
        .send()
        .await
        .map_err(|e| format!("Plex auth pin check request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Plex auth pin check status error: {}", e))?
        .json::<PlexPinResponse>()
        .await
        .map_err(|e| format!("Failed to parse Plex auth pin check response: {}", e))?;

    Ok(PlexPinCheckResult {
        authorized: pin.auth_token.is_some(),
        expired: pin.expires_in == Some(0),
        auth_token: pin.auth_token,
        expires_in: pin.expires_in,
    })
}

#[tauri::command]
pub async fn plex_open_auth_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| format!("Failed to open browser: {}", e))
}

#[tauri::command]
pub fn plex_cache_get_sections() -> Result<Vec<PlexMusicSection>, String> {
    let conn = open_plex_cache_db()?;
    let mut stmt = conn
        .prepare(
            "SELECT section_key, title
             FROM plex_cache_sections
             ORDER BY title COLLATE NOCASE",
        )
        .map_err(|e| format!("Failed to prepare Plex cache sections query: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(PlexMusicSection {
                key: row.get(0)?,
                title: row.get(1)?,
            })
        })
        .map_err(|e| format!("Failed to query Plex cache sections: {}", e))?;

    let mut sections = Vec::new();
    for row in rows {
        sections.push(row.map_err(|e| format!("Failed to read Plex cache section row: {}", e))?);
    }
    Ok(sections)
}

#[tauri::command]
pub fn plex_cache_save_sections(
    server_id: Option<String>,
    sections: Vec<PlexMusicSection>,
) -> Result<usize, String> {
    let mut conn = open_plex_cache_db()?;
    let tx = conn
        .transaction()
        .map_err(|e| format!("Failed to start Plex cache sections transaction: {}", e))?;

    let now = now_epoch_secs();
    for section in &sections {
        tx.execute(
            "INSERT INTO plex_cache_sections (section_key, title, server_id, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(section_key) DO UPDATE SET
                title = excluded.title,
                server_id = excluded.server_id,
                updated_at = excluded.updated_at",
            params![section.key, section.title, server_id, now],
        )
        .map_err(|e| format!("Failed to upsert Plex cache section: {}", e))?;
    }

    tx.commit()
        .map_err(|e| format!("Failed to commit Plex cache sections transaction: {}", e))?;
    Ok(sections.len())
}

#[tauri::command]
pub fn plex_cache_get_tracks(
    section_key: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<PlexTrack>, String> {
    let conn = open_plex_cache_db()?;
    let max = limit.unwrap_or(200) as i64;
    let mut tracks = Vec::new();

    if let Some(section) = section_key {
        let mut stmt = conn
            .prepare(
                "SELECT rating_key, title, artist, album, duration_ms, artwork_path, part_key, container,
                        codec, channels, bitrate_kbps, sampling_rate_hz, bit_depth
                 FROM plex_cache_tracks
                 WHERE section_key = ?1
                 ORDER BY artist COLLATE NOCASE, album COLLATE NOCASE, title COLLATE NOCASE
                 LIMIT ?2",
            )
            .map_err(|e| format!("Failed to prepare Plex cache tracks query: {}", e))?;
        let rows = stmt
            .query_map(params![section, max], |row| {
                Ok(PlexTrack {
                    rating_key: row.get(0)?,
                    title: decode_xml_entities(row.get::<_, String>(1)?.trim()),
                    artist: row
                        .get::<_, Option<String>>(2)?
                        .map(|v| decode_xml_entities(v.trim())),
                    album: row
                        .get::<_, Option<String>>(3)?
                        .map(|v| decode_xml_entities(v.trim())),
                    duration_ms: row.get(4)?,
                    artwork_path: row.get(5)?,
                    part_key: row.get(6)?,
                    container: row.get(7)?,
                    codec: row.get(8)?,
                    channels: row.get(9)?,
                    bitrate_kbps: row.get(10)?,
                    sampling_rate_hz: row.get(11)?,
                    bit_depth: row.get(12)?,
                })
            })
            .map_err(|e| format!("Failed to query Plex cache tracks: {}", e))?;
        for row in rows {
            tracks.push(row.map_err(|e| format!("Failed to read Plex cache track row: {}", e))?);
        }
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT rating_key, title, artist, album, duration_ms, artwork_path, part_key, container,
                        codec, channels, bitrate_kbps, sampling_rate_hz, bit_depth
                 FROM plex_cache_tracks
                 ORDER BY updated_at DESC
                 LIMIT ?1",
            )
            .map_err(|e| format!("Failed to prepare Plex cache tracks query: {}", e))?;
        let rows = stmt
            .query_map(params![max], |row| {
                Ok(PlexTrack {
                    rating_key: row.get(0)?,
                    title: decode_xml_entities(row.get::<_, String>(1)?.trim()),
                    artist: row
                        .get::<_, Option<String>>(2)?
                        .map(|v| decode_xml_entities(v.trim())),
                    album: row
                        .get::<_, Option<String>>(3)?
                        .map(|v| decode_xml_entities(v.trim())),
                    duration_ms: row.get(4)?,
                    artwork_path: row.get(5)?,
                    part_key: row.get(6)?,
                    container: row.get(7)?,
                    codec: row.get(8)?,
                    channels: row.get(9)?,
                    bitrate_kbps: row.get(10)?,
                    sampling_rate_hz: row.get(11)?,
                    bit_depth: row.get(12)?,
                })
            })
            .map_err(|e| format!("Failed to query Plex cache tracks: {}", e))?;
        for row in rows {
            tracks.push(row.map_err(|e| format!("Failed to read Plex cache track row: {}", e))?);
        }
    }

    Ok(tracks)
}

#[tauri::command]
pub fn plex_cache_get_albums() -> Result<Vec<PlexCachedAlbum>, String> {
    let conn = open_plex_cache_db()?;
    let mut stmt = conn
        .prepare(
            "SELECT artist, album, duration_ms, artwork_path, container, sampling_rate_hz, bit_depth
             FROM plex_cache_tracks",
        )
        .map_err(|e| format!("Failed to prepare Plex cache album aggregation query: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        })
        .map_err(|e| {
            format!(
                "Failed to query Plex cache tracks for album aggregation: {}",
                e
            )
        })?;

    let mut grouped: HashMap<String, PlexCachedAlbum> = HashMap::new();

    for row in rows {
        let (
            artist_opt,
            album_opt,
            duration_ms_opt,
            artwork_path,
            container,
            sampling_rate_hz_opt,
            bit_depth_opt,
        ) = row.map_err(|e| format!("Failed to read Plex cache aggregation row: {}", e))?;
        let artist = artist_opt
            .map(|v| decode_xml_entities(v.trim()))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "Unknown Artist".to_string());
        let album_raw = album_opt
            .map(|v| decode_xml_entities(v.trim()))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "Unknown Album".to_string());
        let album = normalize_album_title(Some(&artist), &album_raw);
        let album_key = plex_album_key(&artist, &album);

        let entry = grouped
            .entry(album_key.clone())
            .or_insert_with(|| PlexCachedAlbum {
                id: album_key.clone(),
                title: album.clone(),
                artist: artist.clone(),
                artwork_path: artwork_path.clone(),
                track_count: 0,
                total_duration_secs: 0,
                format: container.clone().unwrap_or_else(|| "flac".to_string()),
                bit_depth: bit_depth_opt.map(|v| v as u32),
                sample_rate: sampling_rate_hz_opt.map(|v| v as u32).unwrap_or(44100),
                source: "plex".to_string(),
                likely_single_file_album: false,
            });

        entry.track_count += 1;
        if let Some(duration_ms) = duration_ms_opt {
            entry.total_duration_secs += (duration_ms as u64) / 1000;
        }
        if entry.artwork_path.is_none() && artwork_path.is_some() {
            entry.artwork_path = artwork_path;
        }
        if let Some(container_value) = container {
            if entry.format == "flac" || entry.format.is_empty() {
                entry.format = container_value;
            }
        }
        if let Some(rate) = sampling_rate_hz_opt {
            let rate_u = rate as u32;
            if rate_u > entry.sample_rate {
                entry.sample_rate = rate_u;
            }
        }
        if let Some(depth) = bit_depth_opt {
            let depth_u = depth as u32;
            if depth_u > entry.bit_depth.unwrap_or(0) {
                entry.bit_depth = Some(depth_u);
            }
        }
    }

    let mut albums: Vec<PlexCachedAlbum> = grouped.into_values().collect();

    // Detect likely single-file albums (one track, duration > 10 minutes)
    for album in &mut albums {
        if album.track_count == 1 && album.total_duration_secs > 600 {
            album.likely_single_file_album = true;
        }
    }

    albums.sort_by(|a, b| {
        let artist_cmp = a.artist.to_lowercase().cmp(&b.artist.to_lowercase());
        if artist_cmp != std::cmp::Ordering::Equal {
            return artist_cmp;
        }
        a.title.to_lowercase().cmp(&b.title.to_lowercase())
    });
    Ok(albums)
}

#[tauri::command]
pub fn plex_cache_get_album_tracks(album_key: String) -> Result<Vec<PlexCachedTrack>, String> {
    let conn = open_plex_cache_db()?;
    let mut stmt = conn
        .prepare(
            "SELECT rating_key, title, artist, album, duration_ms, container, bit_depth, sampling_rate_hz, artwork_path
             FROM plex_cache_tracks
             ORDER BY title COLLATE NOCASE",
        )
        .map_err(|e| format!("Failed to prepare Plex cache album tracks query: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })
        .map_err(|e| format!("Failed to query Plex cache album tracks: {}", e))?;

    let mut tracks = Vec::new();
    for row in rows {
        let (
            rating_key,
            title,
            artist_opt,
            album_opt,
            duration_ms_opt,
            container_opt,
            bit_depth_opt,
            sampling_rate_opt,
            artwork_path,
        ) = row.map_err(|e| format!("Failed to read Plex cache album track row: {}", e))?;
        let artist = artist_opt
            .map(|v| decode_xml_entities(v.trim()))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "Unknown Artist".to_string());
        let album_raw = album_opt
            .map(|v| decode_xml_entities(v.trim()))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "Unknown Album".to_string());
        let album = normalize_album_title(Some(&artist), &album_raw);
        if plex_album_key(&artist, &album) != album_key {
            continue;
        }
        tracks.push(PlexCachedTrack {
            id: playback_track_id(&rating_key),
            rating_key,
            title: decode_xml_entities(title.trim()),
            artist,
            album,
            duration_secs: duration_ms_opt.map(|v| (v as u64) / 1000).unwrap_or(0),
            format: container_opt.unwrap_or_else(|| "flac".to_string()),
            bit_depth: bit_depth_opt.map(|v| v as u32),
            sample_rate: sampling_rate_opt.map(|v| v as u32).unwrap_or(44100),
            artwork_path,
            source: "plex".to_string(),
            album_key: album_key.clone(),
        });
    }
    Ok(tracks)
}

#[tauri::command]
pub fn plex_cache_search_tracks(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<PlexCachedTrack>, String> {
    let conn = open_plex_cache_db()?;
    let max = limit.unwrap_or(5000) as i64;
    let needle = format!("%{}%", query.to_lowercase());
    let mut stmt = conn
        .prepare(
            "SELECT rating_key, title, artist, album, duration_ms, container, bit_depth, sampling_rate_hz, artwork_path
             FROM plex_cache_tracks
             WHERE ?1 = '' OR
                   lower(title) LIKE ?2 OR
                   lower(COALESCE(artist, '')) LIKE ?2 OR
                   lower(COALESCE(album, '')) LIKE ?2
             ORDER BY artist COLLATE NOCASE, album COLLATE NOCASE, title COLLATE NOCASE
             LIMIT ?3",
        )
        .map_err(|e| format!("Failed to prepare Plex cache search query: {}", e))?;

    let rows = stmt
        .query_map(params![query.trim(), needle, max], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })
        .map_err(|e| format!("Failed to query Plex cache search tracks: {}", e))?;

    let mut tracks = Vec::new();
    for row in rows {
        let (
            rating_key,
            title,
            artist_opt,
            album_opt,
            duration_ms_opt,
            container_opt,
            bit_depth_opt,
            sampling_rate_opt,
            artwork_path,
        ) = row.map_err(|e| format!("Failed to read Plex cache search row: {}", e))?;
        let artist = artist_opt
            .map(|v| decode_xml_entities(v.trim()))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "Unknown Artist".to_string());
        let album_raw = album_opt
            .map(|v| decode_xml_entities(v.trim()))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "Unknown Album".to_string());
        let album = normalize_album_title(Some(&artist), &album_raw);
        tracks.push(PlexCachedTrack {
            id: playback_track_id(&rating_key),
            rating_key: rating_key.clone(),
            title: decode_xml_entities(title.trim()),
            artist: artist.clone(),
            album: album.clone(),
            duration_secs: duration_ms_opt.map(|v| (v as u64) / 1000).unwrap_or(0),
            format: container_opt.unwrap_or_else(|| "flac".to_string()),
            bit_depth: bit_depth_opt.map(|v| v as u32),
            sample_rate: sampling_rate_opt.map(|v| v as u32).unwrap_or(44100),
            artwork_path,
            source: "plex".to_string(),
            album_key: plex_album_key(&artist, &album),
        });
    }
    Ok(tracks)
}

#[tauri::command]
pub fn plex_cache_save_tracks(
    server_id: Option<String>,
    section_key: String,
    tracks: Vec<PlexTrack>,
) -> Result<usize, String> {
    let mut conn = open_plex_cache_db()?;
    let tx = conn
        .transaction()
        .map_err(|e| format!("Failed to start Plex cache tracks transaction: {}", e))?;

    // Save existing hydrated quality data before clearing the section
    let mut hydrated_quality: HashMap<String, (Option<String>, Option<i64>, Option<i64>)> =
        HashMap::new();
    {
        let mut stmt = tx
            .prepare(
                "SELECT rating_key, container, sampling_rate_hz, bit_depth
                 FROM plex_cache_tracks
                 WHERE section_key = ?1 AND (sampling_rate_hz IS NOT NULL OR bit_depth IS NOT NULL)",
            )
            .map_err(|e| format!("Failed to prepare hydrated quality query: {}", e))?;
        let rows = stmt
            .query_map(params![section_key], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })
            .map_err(|e| format!("Failed to read hydrated quality data: {}", e))?;
        for row in rows {
            let (key, container, rate, depth) =
                row.map_err(|e| format!("Failed to read hydrated quality row: {}", e))?;
            hydrated_quality.insert(key, (container, rate, depth));
        }
    }

    tx.execute(
        "DELETE FROM plex_cache_tracks WHERE section_key = ?1",
        params![section_key],
    )
    .map_err(|e| format!("Failed to clear old Plex cache tracks for section: {}", e))?;

    let now = now_epoch_secs();
    for track in &tracks {
        // Use hydrated quality if bulk data has NULL and we have previously hydrated values
        let saved = hydrated_quality.get(&track.rating_key);
        let container = track
            .container
            .as_ref()
            .cloned()
            .or_else(|| saved.and_then(|s| s.0.clone()));
        let sampling_rate_hz = track
            .sampling_rate_hz
            .map(|v| v as i64)
            .or_else(|| saved.and_then(|s| s.1));
        let bit_depth = track
            .bit_depth
            .map(|v| v as i64)
            .or_else(|| saved.and_then(|s| s.2));

        tx.execute(
            "INSERT INTO plex_cache_tracks
             (rating_key, section_key, server_id, title, artist, album, duration_ms, artwork_path,
              part_key, container, codec, channels, bitrate_kbps, sampling_rate_hz, bit_depth, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                track.rating_key,
                section_key,
                server_id,
                track.title,
                track.artist,
                track.album,
                track.duration_ms.map(|v| v as i64),
                track.artwork_path,
                track.part_key,
                container,
                track.codec,
                track.channels.map(|v| v as i64),
                track.bitrate_kbps.map(|v| v as i64),
                sampling_rate_hz,
                bit_depth,
                now,
            ],
        )
        .map_err(|e| format!("Failed to insert Plex cache track: {}", e))?;
    }

    tx.commit()
        .map_err(|e| format!("Failed to commit Plex cache tracks transaction: {}", e))?;
    Ok(tracks.len())
}

#[tauri::command]
pub fn plex_cache_update_track_quality(
    updates: Vec<PlexTrackQualityUpdate>,
) -> Result<usize, String> {
    if updates.is_empty() {
        return Ok(0);
    }

    let mut conn = open_plex_cache_db()?;
    let tx = conn.transaction().map_err(|e| {
        format!(
            "Failed to start Plex cache quality update transaction: {}",
            e
        )
    })?;

    let now = now_epoch_secs();
    let mut updated_rows = 0usize;
    for update in &updates {
        let affected = tx
            .execute(
                "UPDATE plex_cache_tracks
                 SET container = COALESCE(?2, container),
                     sampling_rate_hz = COALESCE(?3, sampling_rate_hz),
                     bit_depth = COALESCE(?4, bit_depth),
                     updated_at = ?5
                 WHERE rating_key = ?1",
                params![
                    update.rating_key,
                    update.container,
                    update.sampling_rate_hz.map(|v| v as i64),
                    update.bit_depth.map(|v| v as i64),
                    now,
                ],
            )
            .map_err(|e| format!("Failed to update Plex cache track quality: {}", e))?;
        updated_rows += affected;
    }

    tx.commit().map_err(|e| {
        format!(
            "Failed to commit Plex cache quality update transaction: {}",
            e
        )
    })?;

    Ok(updated_rows)
}

#[tauri::command]
pub fn plex_cache_get_tracks_needing_hydration(limit: Option<u32>) -> Result<Vec<String>, String> {
    let conn = open_plex_cache_db()?;
    let max = limit.unwrap_or(50) as i64;
    let mut stmt = conn
        .prepare(
            "SELECT rating_key FROM plex_cache_tracks
             WHERE sampling_rate_hz IS NULL OR bit_depth IS NULL
             LIMIT ?1",
        )
        .map_err(|e| format!("Failed to prepare hydration query: {}", e))?;
    let rows = stmt
        .query_map(params![max], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Failed to query tracks needing hydration: {}", e))?;
    let mut keys = Vec::new();
    for row in rows {
        keys.push(row.map_err(|e| format!("Failed to read hydration row: {}", e))?);
    }
    Ok(keys)
}

#[tauri::command]
pub fn plex_cache_clear() -> Result<(), String> {
    let conn = open_plex_cache_db()?;
    conn.execute("DELETE FROM plex_cache_tracks", [])
        .map_err(|e| format!("Failed to clear Plex cache tracks: {}", e))?;
    conn.execute("DELETE FROM plex_cache_sections", [])
        .map_err(|e| format!("Failed to clear Plex cache sections: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn plex_play_track(
    base_url: String,
    token: String,
    rating_key: String,
    app_state: State<'_, AppState>,
) -> Result<PlexPlayResult, String> {
    let resolved = plex_resolve_track_media(base_url, token, rating_key).await?;

    app_state
        .player
        .play_data(resolved.bytes.clone(), resolved.playback_id)
        .map_err(|e| format!("Failed to play Plex track: {}", e))?;

    Ok(PlexPlayResult {
        rating_key: resolved.rating_key,
        part_key: resolved.part_key,
        part_url: resolved.part_url,
        bytes: resolved.bytes.len(),
        direct_play_confirmed: resolved.direct_play_confirmed,
        content_type: resolved.content_type,
        sampling_rate_hz: resolved.sampling_rate_hz,
        bit_depth: resolved.bit_depth,
    })
}

pub async fn plex_resolve_track_media(
    base_url: String,
    token: String,
    rating_key: String,
) -> Result<PlexResolvedMedia, String> {
    let client = build_plex_client()?;
    let base = normalize_base_url(&base_url);

    let metadata_url = with_token(&format!("{base}/library/metadata/{rating_key}"), &token);
    let metadata_xml = client
        .get(metadata_url)
        .send()
        .await
        .map_err(|e| format!("Plex metadata request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Plex metadata status error: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read Plex metadata response: {}", e))?;

    let mut tracks = parse_tracks(&metadata_xml, Some(1));
    let track = tracks
        .pop()
        .ok_or_else(|| format!("Track {rating_key} not found in Plex metadata"))?;

    let part_key = track
        .part_key
        .clone()
        .ok_or_else(|| format!("Track {rating_key} does not include a playable Part key"))?;

    let part_url = with_token(&format!("{base}{part_key}"), &token);
    let part_response = client
        .get(&part_url)
        .send()
        .await
        .map_err(|e| format!("Plex part request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Plex part status error: {}", e))?;

    let content_type = part_response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let bytes = part_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read Plex media bytes: {}", e))?;

    let playback_id = playback_track_id(&rating_key);
    Ok(PlexResolvedMedia {
        rating_key,
        playback_id,
        part_key: part_key.clone(),
        part_url,
        bytes: bytes.to_vec(),
        direct_play_confirmed: is_direct_part_key(&part_key),
        content_type,
        sampling_rate_hz: track.sampling_rate_hz,
        bit_depth: track.bit_depth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_music_sections() {
        let xml = r#"<MediaContainer>
            <Directory key="1" title="Music" type="artist"/>
            <Directory key="2" title="Movies" type="movie"/>
        </MediaContainer>"#;
        let sections = parse_music_sections(xml);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].key, "1");
    }

    #[test]
    fn parses_tracks_with_stream_audio_metadata() {
        let xml = r#"<MediaContainer>
            <Track ratingKey="42" title="Song" grandparentTitle="Artist" parentTitle="Album" duration="123000" thumb="/library/metadata/42/thumb/1">
                <Media container="flac">
                    <Part key="/library/parts/999/file.flac"/>
                    <Stream streamType="2" codecType="audio" codec="flac" channels="2" samplingRate="96000" bitDepth="24" bitrate="3120"/>
                </Media>
            </Track>
        </MediaContainer>"#;
        let tracks = parse_tracks(xml, Some(10));
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].rating_key, "42");
        assert_eq!(
            tracks[0].part_key.as_deref(),
            Some("/library/parts/999/file.flac")
        );
        assert_eq!(
            tracks[0].artwork_path.as_deref(),
            Some("/library/metadata/42/thumb/1")
        );
        assert_eq!(tracks[0].sampling_rate_hz, Some(96000));
        assert_eq!(tracks[0].bit_depth, Some(24));
    }

    #[test]
    fn direct_part_key_detection() {
        assert!(is_direct_part_key("/library/parts/1234/file.flac"));
        assert!(!is_direct_part_key("/music/:/transcode/universal/start"));
    }

    #[test]
    fn playback_track_id_prefers_numeric_rating_key() {
        assert_eq!(playback_track_id("48012"), 48012);
    }
}
