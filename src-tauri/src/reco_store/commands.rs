//! Tauri commands for recommendation store

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;

use crate::reco_store::db::{RecoEventRecord, RecoScoreEntry};
use crate::reco_store::{HomeSeeds, RecoEventInput, RecoState, TopArtistSeed};

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

    let db = state.db.lock().await;
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

    let db = state.db.lock().await;

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

    let db = state.db.lock().await;
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
    let limit_recent_albums = limit_recent_albums.unwrap_or(12);
    let limit_continue_tracks = limit_continue_tracks.unwrap_or(10);
    let limit_top_artists = limit_top_artists.unwrap_or(10);
    let limit_favorites = limit_favorites.unwrap_or(12);

    let db = state.db.lock().await;
    let has_scores = db.has_scores("all")?;

    let mut recently_played_album_ids = if has_scores {
        db.get_scored_album_ids("all", limit_recent_albums)?
    } else {
        Vec::new()
    };
    let mut continue_listening_track_ids = if has_scores {
        db.get_scored_track_ids("all", limit_continue_tracks)?
    } else {
        Vec::new()
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

    Ok(HomeSeeds {
        recently_played_album_ids,
        continue_listening_track_ids,
        top_artist_ids,
        favorite_album_ids,
        favorite_track_ids,
    })
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
