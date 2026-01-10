//! Tauri commands for lyrics

use tauri::State;

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
        let db = state.db.lock().await;
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

        let db = state.db.lock().await;
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

        let db = state.db.lock().await;
        db.upsert(&cache_key, &payload)?;
        return Ok(Some(payload));
    }

    Ok(None)
}

#[tauri::command]
pub async fn lyrics_clear_cache(state: State<'_, LyricsState>) -> Result<(), String> {
    let db = state.db.lock().await;
    db.clear()
}
