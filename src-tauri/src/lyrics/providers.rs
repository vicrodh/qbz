//! Lyrics providers

use reqwest::Client;
use serde::Deserialize;
use urlencoding::encode;

use super::{normalize, LyricsProvider};

#[derive(Debug, Clone)]
pub struct LyricsData {
    pub plain: Option<String>,
    pub synced_lrc: Option<String>,
    pub provider: LyricsProvider,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibItem {
    #[serde(default)]
    pub track_name: String,
    #[serde(default)]
    pub artist_name: String,
    pub album_name: Option<String>,
    pub duration: Option<f64>,
    pub instrumental: Option<bool>,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
}

pub async fn fetch_lrclib(
    title: &str,
    artist: &str,
    duration_secs: Option<u64>,
) -> Result<Option<LyricsData>, String> {
    let client = Client::new();
    let mut best = fetch_lrclib_get(&client, title, artist).await?;

    if best.is_none() {
        let results = fetch_lrclib_search(&client, title, artist).await?;
        best = pick_best_match(&results, title, artist, duration_secs);
    }

    let Some(item) = best else {
        return Ok(None);
    };

    if item.instrumental.unwrap_or(false) {
        return Ok(None);
    }

    let plain = item.plain_lyrics.and_then(clean_lyrics);
    let synced = item.synced_lyrics.and_then(clean_lyrics);

    if plain.is_none() && synced.is_none() {
        return Ok(None);
    }

    Ok(Some(LyricsData {
        plain,
        synced_lrc: synced,
        provider: LyricsProvider::Lrclib,
    }))
}

pub async fn fetch_lyrics_ovh(title: &str, artist: &str) -> Result<Option<LyricsData>, String> {
    let artist_encoded = encode(artist);
    let title_encoded = encode(title);
    let url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        artist_encoded, title_encoded
    );

    let response = Client::new()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("lyrics.ovh request failed: {}", e))?;

    if !response.status().is_success() {
        return Ok(None);
    }

    #[derive(Deserialize)]
    struct OvhResponse {
        lyrics: Option<String>,
    }

    let data: OvhResponse = response
        .json()
        .await
        .map_err(|e| format!("lyrics.ovh response parse failed: {}", e))?;

    let plain = data.lyrics.and_then(clean_lyrics);
    if plain.is_none() {
        return Ok(None);
    }

    Ok(Some(LyricsData {
        plain,
        synced_lrc: None,
        provider: LyricsProvider::Ovh,
    }))
}

async fn fetch_lrclib_get(
    client: &Client,
    title: &str,
    artist: &str,
) -> Result<Option<LrclibItem>, String> {
    let response = client
        .get("https://lrclib.net/api/get")
        .query(&[("track_name", title), ("artist_name", artist)])
        .send()
        .await
        .map_err(|e| format!("LRCLIB get request failed: {}", e))?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let item: LrclibItem = response
        .json()
        .await
        .map_err(|e| format!("LRCLIB get response parse failed: {}", e))?;

    Ok(Some(item))
}

async fn fetch_lrclib_search(
    client: &Client,
    title: &str,
    artist: &str,
) -> Result<Vec<LrclibItem>, String> {
    let query = format!("{} {}", artist, title);
    let response = client
        .get("https://lrclib.net/api/search")
        .query(&[("q", &query)])
        .send()
        .await
        .map_err(|e| format!("LRCLIB search request failed: {}", e))?;

    if !response.status().is_success() {
        return Ok(Vec::new());
    }

    let items: Vec<LrclibItem> = response
        .json()
        .await
        .map_err(|e| format!("LRCLIB search response parse failed: {}", e))?;

    Ok(items)
}

fn pick_best_match(
    items: &[LrclibItem],
    title: &str,
    artist: &str,
    duration_secs: Option<u64>,
) -> Option<LrclibItem> {
    let normalized_title = normalize(title);
    let normalized_artist = normalize(artist);
    let target_duration = duration_secs.unwrap_or(0) as f64;

    let mut best: Option<(i32, &LrclibItem)> = None;

    for item in items {
        let item_title = normalize(&item.track_name);
        let item_artist = normalize(&item.artist_name);

        let mut score = 0;

        if item_title == normalized_title {
            score += 3;
        }
        if item_artist == normalized_artist {
            score += 3;
        }
        if item_title == normalized_title && item_artist == normalized_artist {
            score += 4;
        }

        if let Some(duration) = item.duration {
            if target_duration > 0.0 {
                let diff = (duration - target_duration).abs();
                if diff <= 2.0 {
                    score += 3;
                } else if diff <= 5.0 {
                    score += 1;
                }
            }
        }

        if item.synced_lyrics.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false) {
            score += 2;
        } else if item.plain_lyrics.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false) {
            score += 1;
        }

        match best {
            Some((best_score, _)) if score <= best_score => {}
            _ => best = Some((score, item)),
        }
    }

    best.map(|(_, item)| item.clone())
}

fn clean_lyrics(value: String) -> Option<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
