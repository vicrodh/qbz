//! Tauri commands for lyrics

use tauri::State;
use serde::Serialize;

use super::{build_cache_key, LyricsPayload, LyricsState};
use super::providers::{fetch_lrclib, fetch_lyrics_ovh};

#[tauri::command]
pub async fn lyrics_get(
    track_id: Option<u64>,
    title: String,
    artist: String,
    album: Option<String>,
    duration_secs: Option<u64>,
    state: State<'_, LyricsState>,
) -> Result<Option<LyricsPayload>, String> {
    let title_trimmed = title.trim();
    let artist_trimmed = artist.trim();

    if title_trimmed.is_empty() || artist_trimmed.is_empty() {
        return Err("Lyrics lookup requires title and artist".to_string());
    }

    let cache_key = build_cache_key(title_trimmed, artist_trimmed, duration_secs);

    // Try cache by track_id first, then by key
    {
        let db_opt__ = state.db.lock().await;
        let db = db_opt__.as_ref().ok_or("No active session - please log in")?;
        if let Some(id) = track_id {
            if let Ok(Some(payload)) = db.get_by_track_id(id) {
                return Ok(Some(payload));
            }
        }

        if let Ok(Some(payload)) = db.get_by_cache_key(&cache_key) {
            return Ok(Some(payload));
        }
    }

    // Provider chain: LRCLIB -> lyrics.ovh
    if let Some(data) = fetch_lrclib(title_trimmed, artist_trimmed, duration_secs).await? {
        let payload = LyricsPayload {
            track_id,
            title: title_trimmed.to_string(),
            artist: artist_trimmed.to_string(),
            album: album.clone(),
            duration_secs,
            plain: data.plain,
            synced_lrc: data.synced_lrc,
            provider: data.provider,
            cached: false,
        };

        let db_opt__ = state.db.lock().await;
        let db = db_opt__.as_ref().ok_or("No active session - please log in")?;
        db.upsert(&cache_key, &payload)?;
        return Ok(Some(payload));
    }

    if let Some(data) = fetch_lyrics_ovh(title_trimmed, artist_trimmed).await? {
        let payload = LyricsPayload {
            track_id,
            title: title_trimmed.to_string(),
            artist: artist_trimmed.to_string(),
            album,
            duration_secs,
            plain: data.plain,
            synced_lrc: data.synced_lrc,
            provider: data.provider,
            cached: false,
        };

        let db_opt__ = state.db.lock().await;
        let db = db_opt__.as_ref().ok_or("No active session - please log in")?;
        db.upsert(&cache_key, &payload)?;
        return Ok(Some(payload));
    }

    Ok(None)
}

#[tauri::command]
pub async fn lyrics_clear_cache(state: State<'_, LyricsState>) -> Result<(), String> {
    let db_opt__ = state.db.lock().await;
    let db = db_opt__.as_ref().ok_or("No active session - please log in")?;
    db.clear()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsCacheStats {
    pub entries: u64,
    pub size_bytes: u64,
}

#[tauri::command]
pub async fn lyrics_get_cache_stats(state: State<'_, LyricsState>) -> Result<LyricsCacheStats, String> {
    let entries = {
        let db_opt__ = state.db.lock().await;
        let db = db_opt__.as_ref().ok_or("No active session - please log in")?;
        db.count_entries()?
    };

    // Approximate on-disk usage as the SQLite DB file size.
    let db_path = dirs::cache_dir()
        .ok_or("Could not determine cache directory")?
        .join("qbz")
        .join("lyrics")
        .join("lyrics.db");
    let size_bytes = std::fs::metadata(&db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(LyricsCacheStats { entries, size_bytes })
}
