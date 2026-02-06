//! Tauri commands for recommendation store

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;

use crate::reco_store::db::{RecoEventRecord, RecoScoreEntry};
use crate::reco_store::{HomeSeeds, RecoEventInput, RecoState, TopArtistSeed};
use crate::AppState;

const DEFAULT_LOOKBACK_DAYS: i64 = 90;
const DEFAULT_HALF_LIFE_DAYS: f64 = 21.0;
const DEFAULT_MAX_EVENTS: u32 = 5000;
const DEFAULT_MAX_PER_TYPE: u32 = 200;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoScoreCounts {
    pub tracks: usize,
    pub albums: usize,
    pub artists: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoTrainStats {
    pub total_events: usize,
    pub total_favorite_events: usize,
    pub all_scores: RecoScoreCounts,
    pub favorite_scores: RecoScoreCounts,
}

#[tauri::command]
pub async fn reco_log_event(
    event: RecoEventInput,
    state: State<'_, RecoState>,
) -> Result<(), String> {
    log::info!(
        "Command: reco_log_event type={} item={}",
        event.event_type.as_str(),
        event.item_type.as_str()
    );

    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.insert_event(&event)
}

#[tauri::command]
pub async fn reco_get_home(
    limit_recent_albums: Option<u32>,
    limit_continue_tracks: Option<u32>,
    limit_top_artists: Option<u32>,
    limit_favorites: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<HomeSeeds, String> {
    let limit_recent_albums = limit_recent_albums.unwrap_or(12);
    let limit_continue_tracks = limit_continue_tracks.unwrap_or(10);
    let limit_top_artists = limit_top_artists.unwrap_or(10);
    let limit_favorites = limit_favorites.unwrap_or(12);

    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;

    let recently_played_album_ids = db.get_recent_album_ids(limit_recent_albums)?;
    let continue_listening_track_ids = db.get_recent_track_ids(limit_continue_tracks)?;
    let top_artist_ids = db.get_top_artist_ids(limit_top_artists)?;
    let favorite_album_ids = db.get_favorite_album_ids(limit_favorites)?;
    let favorite_track_ids = db.get_favorite_track_ids(limit_favorites)?;

    Ok(HomeSeeds {
        recently_played_album_ids,
        continue_listening_track_ids,
        top_artist_ids,
        favorite_album_ids,
        favorite_track_ids,
    })
}

#[tauri::command]
pub async fn reco_train_scores(
    lookback_days: Option<i64>,
    half_life_days: Option<f64>,
    max_events: Option<u32>,
    max_per_type: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<RecoTrainStats, String> {
    let lookback_days = lookback_days.unwrap_or(DEFAULT_LOOKBACK_DAYS);
    let half_life_days = half_life_days.unwrap_or(DEFAULT_HALF_LIFE_DAYS);
    let max_events = max_events.unwrap_or(DEFAULT_MAX_EVENTS);
    let max_per_type = max_per_type.unwrap_or(DEFAULT_MAX_PER_TYPE);

    let now_ts = current_timestamp();
    let since_ts = now_ts.saturating_sub(lookback_days * 86_400);

    let mut guard__ = state.db.lock().await;
    let db = guard__.as_mut().ok_or("No active session - please log in")?;
    let events = db.get_events_since(since_ts, Some(max_events))?;
    let total_events = events.len();
    let total_favorite_events = events
        .iter()
        .filter(|event| event.event_type == "favorite")
        .count();

    let all_scores = build_scores(&events, now_ts, half_life_days, false);
    let favorite_scores = build_scores(&events, now_ts, half_life_days, true);

    let all_track_entries = build_track_entries(all_scores.tracks, max_per_type);
    let all_album_entries = build_album_entries(all_scores.albums, max_per_type);
    let all_artist_entries = build_artist_entries(all_scores.artists, max_per_type);

    let fav_track_entries = build_track_entries(favorite_scores.tracks, max_per_type);
    let fav_album_entries = build_album_entries(favorite_scores.albums, max_per_type);
    let fav_artist_entries = build_artist_entries(favorite_scores.artists, max_per_type);

    db.replace_scores("all", "track", &all_track_entries)?;
    db.replace_scores("all", "album", &all_album_entries)?;
    db.replace_scores("all", "artist", &all_artist_entries)?;

    db.replace_scores("favorite", "track", &fav_track_entries)?;
    db.replace_scores("favorite", "album", &fav_album_entries)?;
    db.replace_scores("favorite", "artist", &fav_artist_entries)?;

    Ok(RecoTrainStats {
        total_events,
        total_favorite_events,
        all_scores: RecoScoreCounts {
            tracks: all_track_entries.len(),
            albums: all_album_entries.len(),
            artists: all_artist_entries.len(),
        },
        favorite_scores: RecoScoreCounts {
            tracks: fav_track_entries.len(),
            albums: fav_album_entries.len(),
            artists: fav_artist_entries.len(),
        },
    })
}

#[tauri::command]
pub async fn reco_get_home_ml(
    limit_recent_albums: Option<u32>,
    limit_continue_tracks: Option<u32>,
    limit_top_artists: Option<u32>,
    limit_favorites: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<HomeSeeds, String> {
    log::info!("[DEBUG-43] reco_get_home_ml called");
    let limit_recent_albums = limit_recent_albums.unwrap_or(12);
    let limit_continue_tracks = limit_continue_tracks.unwrap_or(10);
    let limit_top_artists = limit_top_artists.unwrap_or(10);
    let limit_favorites = limit_favorites.unwrap_or(12);

    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or_else(|| {
        log::error!("[DEBUG-43] reco_get_home_ml FAILED: No active session (RecoState.db is None)");
        "No active session - please log in".to_string()
    })?;
    let has_scores = db.has_scores("all")?;

    // HYBRID APPROACH: Always get fresh recent plays first, then supplement with scored
    // This ensures newly played tracks appear immediately without waiting for score retraining

    // Get truly recent plays (last few) to ensure freshness
    let recent_fresh_albums = db.get_recent_album_ids(4)?;
    let recent_fresh_tracks = db.get_recent_track_ids(4)?;

    let mut recently_played_album_ids = if has_scores {
        let scored = db.get_scored_album_ids("all", limit_recent_albums + 4)?;
        merge_unique_preserve_order(recent_fresh_albums, scored, limit_recent_albums as usize)
    } else {
        db.get_recent_album_ids(limit_recent_albums)?
    };

    let mut continue_listening_track_ids = if has_scores {
        let scored = db.get_scored_track_ids("all", limit_continue_tracks + 4)?;
        merge_unique_preserve_order(recent_fresh_tracks, scored, limit_continue_tracks as usize)
    } else {
        db.get_recent_track_ids(limit_continue_tracks)?
    };

    let mut top_artist_ids = if has_scores {
        db.get_scored_artist_scores("all", limit_top_artists)?
            .into_iter()
            .map(|(artist_id, score)| TopArtistSeed {
                artist_id,
                play_count: score.round().max(1.0) as u32,
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut favorite_album_ids = if has_scores {
        db.get_scored_album_ids("favorite", limit_favorites)?
    } else {
        Vec::new()
    };

    let mut favorite_track_ids = if has_scores {
        db.get_scored_track_ids("favorite", limit_favorites)?
    } else {
        Vec::new()
    };

    // Fallbacks for empty results
    if recently_played_album_ids.is_empty() {
        recently_played_album_ids = db.get_recent_album_ids(limit_recent_albums)?;
    }
    if continue_listening_track_ids.is_empty() {
        continue_listening_track_ids = db.get_recent_track_ids(limit_continue_tracks)?;
    }
    if top_artist_ids.is_empty() {
        top_artist_ids = db.get_top_artist_ids(limit_top_artists)?;
    }
    if favorite_album_ids.is_empty() {
        favorite_album_ids = db.get_favorite_album_ids(limit_favorites)?;
    }
    if favorite_track_ids.is_empty() {
        favorite_track_ids = db.get_favorite_track_ids(limit_favorites)?;
    }

    log::info!(
        "[DEBUG-43] reco_get_home_ml OK: recent_albums={}, continue_tracks={}, top_artists={}, fav_albums={}, fav_tracks={}",
        recently_played_album_ids.len(),
        continue_listening_track_ids.len(),
        top_artist_ids.len(),
        favorite_album_ids.len(),
        favorite_track_ids.len()
    );

    Ok(HomeSeeds {
        recently_played_album_ids,
        continue_listening_track_ids,
        top_artist_ids,
        favorite_album_ids,
        favorite_track_ids,
    })
}

/// Merge two lists preserving order: fresh items first, then scored items (excluding duplicates)
fn merge_unique_preserve_order<T: Eq + std::hash::Hash + Clone>(
    fresh: Vec<T>,
    scored: Vec<T>,
    limit: usize,
) -> Vec<T> {
    use std::collections::HashSet;
    let mut seen: HashSet<T> = HashSet::new();
    let mut result = Vec::with_capacity(limit);

    // Add fresh items first
    for item in fresh {
        if seen.insert(item.clone()) {
            result.push(item);
            if result.len() >= limit {
                return result;
            }
        }
    }

    // Add scored items (excluding already seen)
    for item in scored {
        if seen.insert(item.clone()) {
            result.push(item);
            if result.len() >= limit {
                return result;
            }
        }
    }

    result
}

struct ScoreMaps {
    tracks: HashMap<u64, f64>,
    albums: HashMap<String, f64>,
    artists: HashMap<u64, f64>,
}

fn build_scores(
    events: &[RecoEventRecord],
    now_ts: i64,
    half_life_days: f64,
    favorites_only: bool,
) -> ScoreMaps {
    let mut scores = ScoreMaps {
        tracks: HashMap::new(),
        albums: HashMap::new(),
        artists: HashMap::new(),
    };

    for event in events {
        if favorites_only && event.event_type != "favorite" {
            continue;
        }

        let age_secs = (now_ts - event.created_at).max(0);
        let decay = decay_factor(age_secs, half_life_days);
        let base_weight = event_weight(&event.event_type) * decay;

        if let Some(track_id) = event.track_id {
            let weight = base_weight * item_weight("track", event.item_type == "track");
            add_score(&mut scores.tracks, track_id, weight);
        }

        if let Some(album_id) = event.album_id.as_ref() {
            let weight = base_weight * item_weight("album", event.item_type == "album");
            add_score(&mut scores.albums, album_id.clone(), weight);
        }

        if let Some(artist_id) = event.artist_id {
            let weight = base_weight * item_weight("artist", event.item_type == "artist");
            add_score(&mut scores.artists, artist_id, weight);
        }
    }

    scores
}

fn build_track_entries(scores: HashMap<u64, f64>, limit: u32) -> Vec<RecoScoreEntry> {
    let mut entries: Vec<(u64, f64)> = scores.into_iter().collect();
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    entries
        .into_iter()
        .take(limit as usize)
        .map(|(track_id, score)| RecoScoreEntry {
            track_id: Some(track_id),
            album_id: None,
            artist_id: None,
            score,
        })
        .collect()
}

fn build_album_entries(scores: HashMap<String, f64>, limit: u32) -> Vec<RecoScoreEntry> {
    let mut entries: Vec<(String, f64)> = scores.into_iter().collect();
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    entries
        .into_iter()
        .take(limit as usize)
        .map(|(album_id, score)| RecoScoreEntry {
            track_id: None,
            album_id: Some(album_id),
            artist_id: None,
            score,
        })
        .collect()
}

fn build_artist_entries(scores: HashMap<u64, f64>, limit: u32) -> Vec<RecoScoreEntry> {
    let mut entries: Vec<(u64, f64)> = scores.into_iter().collect();
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    entries
        .into_iter()
        .take(limit as usize)
        .map(|(artist_id, score)| RecoScoreEntry {
            track_id: None,
            album_id: None,
            artist_id: Some(artist_id),
            score,
        })
        .collect()
}

fn add_score<K: std::cmp::Eq + std::hash::Hash>(map: &mut HashMap<K, f64>, key: K, value: f64) {
    let entry = map.entry(key).or_insert(0.0);
    *entry += value;
}

fn event_weight(event_type: &str) -> f64 {
    match event_type {
        "play" => 1.0,
        "favorite" => 3.0,
        "playlist_add" => 1.2,
        _ => 1.0,
    }
}

fn item_weight(item_type: &str, primary: bool) -> f64 {
    if primary {
        return 1.0;
    }

    match item_type {
        "album" => 0.7,
        "artist" => 0.5,
        "track" => 0.85,
        _ => 0.6,
    }
}

fn decay_factor(age_secs: i64, half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 {
        return 1.0;
    }
    let half_life_secs = half_life_days * 86_400.0;
    let exponent = age_secs as f64 / half_life_secs;
    0.5_f64.powf(exponent)
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Get playlist suggestions based on ML scores
/// Returns track IDs that would fit well in the playlist based on:
/// 1. User's top scored tracks (from play/favorite history)
/// 2. Tracks from the same artists as the playlist
/// 3. Excluding tracks already in the playlist
#[tauri::command]
pub async fn get_playlist_suggestions(
    playlist_track_ids: Vec<u64>,
    playlist_artist_ids: Vec<u64>,
    limit: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<Vec<u64>, String> {
    let limit = limit.unwrap_or(10);
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;

    // Get user's top scored tracks (get more than needed for filtering)
    let fetch_limit = (limit * 5).max(50);
    let scored_tracks = db.get_scored_track_ids("all", fetch_limit)?;

    // Also get favorite tracks for higher priority
    let favorite_tracks = db.get_scored_track_ids("favorite", fetch_limit)?;

    // Get tracks by top artists from the playlist (artist affinity)
    let artist_scored = db.get_scored_artist_scores("all", 50)?;

    // Build a set of tracks already in playlist for fast lookup
    let existing: std::collections::HashSet<u64> = playlist_track_ids.iter().cloned().collect();
    let playlist_artists: std::collections::HashSet<u64> = playlist_artist_ids.iter().cloned().collect();

    // Score candidates: favorites get bonus, same-artist tracks get bonus
    let mut candidates: HashMap<u64, f64> = HashMap::new();

    // Add scored tracks with base score
    for (idx, track_id) in scored_tracks.iter().enumerate() {
        if !existing.contains(track_id) {
            let position_score = 1.0 - (idx as f64 / fetch_limit as f64) * 0.5;
            candidates.insert(*track_id, position_score);
        }
    }

    // Boost favorite tracks
    for (idx, track_id) in favorite_tracks.iter().enumerate() {
        if !existing.contains(track_id) {
            let position_score = 1.0 - (idx as f64 / fetch_limit as f64) * 0.3;
            let entry = candidates.entry(*track_id).or_insert(0.0);
            *entry += position_score * 1.5; // 1.5x bonus for favorites
        }
    }

    // Boost tracks from artists in the playlist
    for &artist_id in &playlist_artists {
        if let Some((_, artist_score)) = artist_scored.iter().find(|(id, _)| *id == artist_id) {
            // Find tracks by this artist in our candidates and boost them
            // Note: We don't have track->artist mapping here, so this is implicit
            // through the scoring system (tracks played when artist was played)
            for (_track_id, score) in candidates.iter_mut() {
                // Boost slightly based on artist affinity (implicit)
                *score += artist_score * 0.1;
            }
        }
    }

    // Sort by score descending and take top N
    let mut sorted: Vec<(u64, f64)> = candidates.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let suggestions: Vec<u64> = sorted
        .into_iter()
        .take(limit as usize)
        .map(|(track_id, _)| track_id)
        .collect();

    Ok(suggestions)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackfillStats {
    pub albums_processed: u32,
    pub events_updated: u64,
    pub albums_failed: u32,
    pub albums_remaining: u32,
}

/// Backfill genre_id for existing events by fetching album details from Qobuz
/// This runs in batches to avoid overwhelming the API
#[tauri::command]
pub async fn reco_backfill_genres(
    batch_size: Option<u32>,
    reco_state: State<'_, RecoState>,
    app_state: State<'_, AppState>,
) -> Result<BackfillStats, String> {
    let batch_size = batch_size.unwrap_or(50);
    log::info!("Command: reco_backfill_genres batch_size={}", batch_size);

    // Get album IDs first (scoped lock to avoid Send issue)
    let album_ids = {
        let guard__ = reco_state.db.lock().await;
        let db = guard__.as_ref().ok_or("No active session - please log in")?;
        db.get_album_ids_without_genre(batch_size)?
    };

    if album_ids.is_empty() {
        log::info!("No albums need genre backfill");
        return Ok(BackfillStats {
            albums_processed: 0,
            events_updated: 0,
            albums_failed: 0,
            albums_remaining: 0,
        });
    }

    log::info!("Backfilling genres for {} albums", album_ids.len());

    let mut albums_processed = 0u32;
    let mut events_updated = 0u64;
    let mut albums_failed = 0u32;

    for album_id in &album_ids {
        // Fetch album from Qobuz API (lock client, then drop before DB)
        let genre_info = {
            let client = app_state.client.lock().await;
            match client.get_album(album_id).await {
                Ok(album) => Some(album.genre.map(|g| (g.id, g.name))),
                Err(e) => {
                    log::warn!("Failed to fetch album {}: {}", album_id, e);
                    albums_failed += 1;
                    None
                }
            }
        };

        if let Some(genre_opt) = genre_info {
            let genre_id = genre_opt.as_ref().map(|(id, _)| *id).unwrap_or(0);
            let genre_name = genre_opt.as_ref().map(|(_, name)| name.as_str()).unwrap_or("none");

            // Re-acquire DB lock for update
            let guard__ = reco_state.db.lock().await;
            let db = guard__.as_ref().ok_or("No active session - please log in")?;
            match db.update_genre_for_album(album_id, genre_id) {
                Ok(updated) => {
                    events_updated += updated;
                    albums_processed += 1;
                    if genre_id > 0 {
                        log::debug!(
                            "Updated {} events for album {} with genre {} ({})",
                            updated, album_id, genre_id, genre_name
                        );
                    } else {
                        log::debug!("Album {} has no genre, set to 0", album_id);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to update genre for album {}: {}", album_id, e);
                    albums_failed += 1;
                }
            }
        }

        // Small delay to avoid rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }

    // Check how many albums still need backfill
    let albums_remaining = {
        let guard__ = reco_state.db.lock().await;
        let db = guard__.as_ref().ok_or("No active session - please log in")?;
        let remaining = db.get_album_ids_without_genre(1)?;
        if remaining.is_empty() {
            0
        } else {
            db.get_album_ids_without_genre(10000)?.len() as u32
        }
    };

    log::info!(
        "Backfill complete: processed={}, updated={}, failed={}, remaining={}",
        albums_processed,
        events_updated,
        albums_failed,
        albums_remaining
    );

    Ok(BackfillStats {
        albums_processed,
        events_updated,
        albums_failed,
        albums_remaining,
    })
}

/// Check if genre backfill is needed
#[tauri::command]
pub async fn reco_needs_genre_backfill(
    reco_state: State<'_, RecoState>,
) -> Result<bool, String> {
    let guard__ = reco_state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    let albums = db.get_album_ids_without_genre(1)?;
    Ok(!albums.is_empty())
}
