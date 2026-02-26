//! Tauri commands for recommendation store

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::seq::SliceRandom;

use tauri::State;

use crate::api::models::{Album, Artist, ImageSet, Track};
use crate::api_cache::ApiCacheState;
use crate::reco_store::db::{RecoEventRecord, RecoScoreEntry};
use crate::reco_store::{
    AlbumCardMeta, ArtistCardMeta, HomeResolved, HomeSeeds, RecoEventInput, RecoState,
    TopArtistSeed, TrackDisplayMeta,
};
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
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

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
    let db = guard__
        .as_mut()
        .ok_or("No active session - please log in")?;
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

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

    get_home_seeds_internal(
        db,
        limit_recent_albums,
        limit_continue_tracks,
        limit_top_artists,
        limit_favorites,
    )
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
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

    // Get user's top scored tracks (get more than needed for filtering)
    let fetch_limit = (limit * 5).max(50);
    let scored_tracks = db.get_scored_track_ids("all", fetch_limit)?;

    // Also get favorite tracks for higher priority
    let favorite_tracks = db.get_scored_track_ids("favorite", fetch_limit)?;

    // Get tracks by top artists from the playlist (artist affinity)
    let artist_scored = db.get_scored_artist_scores("all", 50)?;

    // Build a set of tracks already in playlist for fast lookup
    let existing: std::collections::HashSet<u64> = playlist_track_ids.iter().cloned().collect();
    let playlist_artists: std::collections::HashSet<u64> =
        playlist_artist_ids.iter().cloned().collect();

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
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
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
            let client = app_state.client.read().await;
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
            let genre_name = genre_opt
                .as_ref()
                .map(|(_, name)| name.as_str())
                .unwrap_or("none");

            // Re-acquire DB lock for update
            let guard__ = reco_state.db.lock().await;
            let db = guard__
                .as_ref()
                .ok_or("No active session - please log in")?;
            match db.update_genre_for_album(album_id, genre_id) {
                Ok(updated) => {
                    events_updated += updated;
                    albums_processed += 1;
                    if genre_id > 0 {
                        log::debug!(
                            "Updated {} events for album {} with genre {} ({})",
                            updated,
                            album_id,
                            genre_id,
                            genre_name
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
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
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
pub async fn reco_needs_genre_backfill(reco_state: State<'_, RecoState>) -> Result<bool, String> {
    let guard__ = reco_state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    let albums = db.get_album_ids_without_genre(1)?;
    Ok(!albums.is_empty())
}

// ============ Home Resolved Command ============

fn format_duration(seconds: u32) -> String {
    let mins = seconds / 60;
    let secs = seconds % 60;
    format!("{}:{:02}", mins, secs)
}

fn format_quality(hires: bool, bit_depth: Option<u32>, sampling_rate: Option<f64>) -> String {
    if hires {
        if let (Some(bd), Some(sr)) = (bit_depth, sampling_rate) {
            return format!("{}bit/{}kHz", bd, sr);
        }
    }
    "CD Quality".to_string()
}

fn get_image(image: &ImageSet) -> String {
    image
        .large
        .as_ref()
        .or(image.thumbnail.as_ref())
        .or(image.small.as_ref())
        .cloned()
        .unwrap_or_default()
}

fn album_to_card_meta(album: &Album) -> AlbumCardMeta {
    AlbumCardMeta {
        id: album.id.clone(),
        artwork: get_image(&album.image),
        title: album.title.clone(),
        artist: album.artist.name.clone(),
        artist_id: if album.artist.id > 0 {
            Some(album.artist.id)
        } else {
            None
        },
        genre: album
            .genre
            .as_ref()
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "Unknown genre".to_string()),
        quality: format_quality(
            album.hires_streamable,
            album.maximum_bit_depth,
            album.maximum_sampling_rate,
        ),
        release_date: album.release_date_original.clone(),
    }
}

fn track_to_display_meta(track: &Track) -> TrackDisplayMeta {
    TrackDisplayMeta {
        id: track.id,
        title: track.title.clone(),
        artist: track
            .performer
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Unknown Artist".to_string()),
        album: track
            .album
            .as_ref()
            .map(|a| a.title.clone())
            .unwrap_or_default(),
        album_art: track
            .album
            .as_ref()
            .map(|a| get_image(&a.image))
            .unwrap_or_default(),
        album_id: track.album.as_ref().map(|a| a.id.clone()),
        artist_id: track
            .performer
            .as_ref()
            .and_then(|p| if p.id > 0 { Some(p.id) } else { None }),
        duration: format_duration(track.duration),
        duration_seconds: track.duration,
        hires: track.hires_streamable,
        bit_depth: track.maximum_bit_depth,
        sampling_rate: track.maximum_sampling_rate,
        isrc: track.isrc.clone(),
    }
}

fn artist_to_card_meta(artist: &Artist, play_count: Option<u32>) -> ArtistCardMeta {
    ArtistCardMeta {
        id: artist.id,
        name: artist.name.clone(),
        image: artist
            .image
            .as_ref()
            .map(|img| get_image(img))
            .filter(|s| !s.is_empty()),
        play_count,
    }
}

/// Internal seed-gathering logic shared between reco_get_home_ml and reco_get_home_resolved
fn get_home_seeds_internal(
    db: &crate::reco_store::db::RecoStoreDb,
    limit_recent_albums: u32,
    limit_continue_tracks: u32,
    limit_top_artists: u32,
    limit_favorites: u32,
) -> Result<HomeSeeds, String> {
    let has_scores = db.has_scores("all")?;

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

    Ok(HomeSeeds {
        recently_played_album_ids,
        continue_listening_track_ids,
        top_artist_ids,
        favorite_album_ids,
        favorite_track_ids,
    })
}

/// Return fully-resolved home page data in a single IPC call.
///
/// 3-tier resolution per entity type:
/// 1. Reco meta (SQLite, no TTL) — sub-ms, covers previously played items
/// 2. API cache (SQLite, 24h TTL) — sub-ms, covers recent API fetches
/// 3. Qobuz API (HTTP, parallel) — fallback for truly new items
#[tauri::command]
pub async fn reco_get_home_resolved(
    limit_recent_albums: Option<u32>,
    limit_continue_tracks: Option<u32>,
    limit_top_artists: Option<u32>,
    limit_favorites: Option<u32>,
    reco_state: State<'_, RecoState>,
    app_state: State<'_, AppState>,
    cache_state: State<'_, ApiCacheState>,
) -> Result<HomeResolved, String> {
    let limit_recent_albums = limit_recent_albums.unwrap_or(12);
    let limit_continue_tracks = limit_continue_tracks.unwrap_or(10);
    let limit_top_artists = limit_top_artists.unwrap_or(10);
    let limit_favorites = limit_favorites.unwrap_or(12);

    // Step 1: Get seeds (IDs) from reco DB
    let seeds = {
        let guard__ = reco_state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        get_home_seeds_internal(
            db,
            limit_recent_albums,
            limit_continue_tracks,
            limit_top_artists,
            limit_favorites,
        )?
    };

    // Step 2: Collect all unique IDs needed
    // For favorite albums: merge favorite album IDs + album IDs from favorite tracks
    // (favorite tracks → their albums is done after track resolution)
    let all_track_ids: Vec<u64> = {
        let mut ids = seeds.continue_listening_track_ids.clone();
        ids.extend(&seeds.favorite_track_ids);
        ids.sort_unstable();
        ids.dedup();
        ids
    };

    let all_artist_ids: Vec<u64> = seeds.top_artist_ids.iter().map(|s| s.artist_id).collect();

    // Build play_count map for artists
    let artist_play_counts: HashMap<u64, u32> = seeds
        .top_artist_ids
        .iter()
        .map(|s| (s.artist_id, s.play_count))
        .collect();

    // Step 3: Resolve albums (recent + favorite)
    let recently_played_albums = resolve_albums(
        &seeds.recently_played_album_ids,
        &reco_state,
        &app_state,
        &cache_state,
    )
    .await?;

    // Step 4: Resolve tracks (continue + favorite)
    let all_tracks = resolve_tracks(&all_track_ids, &reco_state, &app_state, &cache_state).await?;

    // Build track lookup
    let track_map: HashMap<u64, &TrackDisplayMeta> =
        all_tracks.iter().map(|tr| (tr.id, tr)).collect();

    let continue_listening_tracks: Vec<TrackDisplayMeta> = seeds
        .continue_listening_track_ids
        .iter()
        .filter_map(|id| track_map.get(id).map(|tr| (*tr).clone()))
        .collect();

    // Step 5: "More From Favorites" = discover albums by favorite artists,
    // excluding albums the user already has in favorites / recently played.

    // 5a: Resolve favorite albums to get their artist IDs
    let resolved_favorites = resolve_albums(
        &seeds.favorite_album_ids,
        &reco_state,
        &app_state,
        &cache_state,
    )
    .await?;

    // 5b: Collect unique artist IDs from favorite albums + favorite tracks
    let favorite_artist_ids: Vec<u64> = {
        let mut ids = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for album in &resolved_favorites {
            if let Some(aid) = album.artist_id {
                if seen.insert(aid) {
                    ids.push(aid);
                }
            }
        }
        for track_id in &seeds.favorite_track_ids {
            if let Some(track) = track_map.get(track_id) {
                if let Some(aid) = track.artist_id {
                    if seen.insert(aid) {
                        ids.push(aid);
                    }
                }
            }
        }
        ids
    };

    // 5c: Build exclusion set (already-favorited + recently played albums)
    let exclusion_set: std::sync::Arc<std::collections::HashSet<String>> = {
        let mut set: std::collections::HashSet<String> =
            seeds.favorite_album_ids.iter().cloned().collect();
        for track_id in &seeds.favorite_track_ids {
            if let Some(track) = track_map.get(track_id) {
                if let Some(ref album_id) = track.album_id {
                    if !album_id.is_empty() {
                        set.insert(album_id.clone());
                    }
                }
            }
        }
        for album_id in &seeds.recently_played_album_ids {
            set.insert(album_id.clone());
        }
        std::sync::Arc::new(set)
    };

    // 5d: Fetch albums from favorite artists (parallel)
    // Shuffle artists so every favorite gets a fair chance, cap 2 albums per artist
    const MAX_ALBUMS_PER_ARTIST: usize = 2;
    let favorite_albums = if favorite_artist_ids.is_empty() {
        Vec::new()
    } else {
        // Shuffle artists so every favorite gets a fair chance
        let shuffled_artists = {
            let mut ids = favorite_artist_ids.clone();
            ids.shuffle(&mut rand::thread_rng());
            ids
        };

        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(5));
        let client = app_state.client.clone();
        let reco_arc = reco_state.db.clone();

        let mut handles = Vec::new();
        for artist_id in shuffled_artists.iter().take(14) {
            let sem = sem.clone();
            let client = client.clone();
            let reco_arc = reco_arc.clone();
            let exclusion = exclusion_set.clone();
            let artist_id = *artist_id;

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok()?;
                let artist = {
                    let c = client.read().await;
                    c.get_artist_with_pagination_and_locale(
                        artist_id, true, Some(25), None, None,
                    )
                    .await
                    .ok()?
                };
                let albums = artist.albums?;
                let mut results: Vec<AlbumCardMeta> = Vec::new();
                for album in &albums.items {
                    if results.len() >= MAX_ALBUMS_PER_ARTIST {
                        break;
                    }
                    if !exclusion.contains(&album.id) {
                        let meta = album_to_card_meta(album);
                        {
                            let guard__ = reco_arc.lock().await;
                            if let Some(db) = guard__.as_ref() {
                                let _ = db.set_album_meta(&meta);
                            }
                        }
                        results.push(meta);
                    }
                }
                Some(results)
            }));
        }

        let mut all_albums: Vec<AlbumCardMeta> = Vec::new();
        for handle in handles {
            if let Ok(Some(albums)) = handle.await {
                all_albums.extend(albums);
            }
        }

        // Deduplicate, shuffle, and limit
        {
            let mut seen = std::collections::HashSet::new();
            all_albums.retain(|a| seen.insert(a.id.clone()));
        }
        all_albums.shuffle(&mut rand::thread_rng());
        all_albums.truncate(limit_favorites as usize);
        all_albums
    };

    // Step 6: Resolve artists
    let top_artists = resolve_artists(
        &all_artist_ids,
        &artist_play_counts,
        &reco_state,
        &app_state,
        &cache_state,
    )
    .await?;

    Ok(HomeResolved {
        recently_played_albums,
        continue_listening_tracks,
        top_artists,
        favorite_albums,
    })
}

/// Resolve album IDs → AlbumCardMeta with 3-tier cache
async fn resolve_albums(
    ids: &[String],
    reco_state: &State<'_, RecoState>,
    app_state: &State<'_, AppState>,
    cache_state: &State<'_, ApiCacheState>,
) -> Result<Vec<AlbumCardMeta>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    // Tier 1: reco meta cache (no TTL)
    let meta_hits = {
        let guard__ = reco_state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.get_album_metas(ids)?
    };
    let meta_map: HashMap<String, AlbumCardMeta> =
        meta_hits.into_iter().map(|m| (m.id.clone(), m)).collect();

    let missing_ids: Vec<String> = ids
        .iter()
        .filter(|id| !meta_map.contains_key(*id))
        .cloned()
        .collect();

    // Tier 2: API cache (24h TTL)
    let mut api_cache_resolved: HashMap<String, AlbumCardMeta> = HashMap::new();
    if !missing_ids.is_empty() {
        let cached = {
            let guard__ = cache_state.cache.lock().await;
            let cache = guard__
                .as_ref()
                .ok_or("No active session - please log in")?;
            cache.get_albums(&missing_ids, None)?
        };
        for (album_id, json_str) in cached {
            if let Ok(album) = serde_json::from_str::<Album>(&json_str) {
                let meta = album_to_card_meta(&album);
                // Write through to reco meta for future instant hits
                {
                    let guard__ = reco_state.db.lock().await;
                    if let Some(db) = guard__.as_ref() {
                        let _ = db.set_album_meta(&meta);
                    }
                }
                api_cache_resolved.insert(album_id, meta);
            }
        }
    }

    let still_missing: Vec<String> = missing_ids
        .iter()
        .filter(|id| !api_cache_resolved.contains_key(*id))
        .cloned()
        .collect();

    // Tier 3: Qobuz API (parallel with semaphore)
    let mut api_resolved: HashMap<String, AlbumCardMeta> = HashMap::new();
    if !still_missing.is_empty() {
        log::info!(
            "Home resolved: fetching {} albums from Qobuz API",
            still_missing.len()
        );
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
        let client = app_state.client.clone();
        let cache_arc = cache_state.cache.clone();
        let reco_arc = reco_state.db.clone();

        let mut handles = Vec::new();
        for album_id in &still_missing {
            let sem = sem.clone();
            let client = client.clone();
            let cache_arc = cache_arc.clone();
            let reco_arc = reco_arc.clone();
            let album_id = album_id.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok()?;
                let album = {
                    let c = client.read().await;
                    c.get_album(&album_id).await.ok()?
                };
                let meta = album_to_card_meta(&album);

                // Cache the full API response
                if let Ok(json) = serde_json::to_string(&album) {
                    let guard__ = cache_arc.lock().await;
                    if let Some(cache) = guard__.as_ref() {
                        let _ = cache.set_album(&album_id, &json);
                    }
                }
                // Write to reco meta
                {
                    let guard__ = reco_arc.lock().await;
                    if let Some(db) = guard__.as_ref() {
                        let _ = db.set_album_meta(&meta);
                    }
                }

                Some((album_id, meta))
            }));
        }

        for handle in handles {
            if let Ok(Some((id, meta))) = handle.await {
                api_resolved.insert(id, meta);
            }
        }
    }

    // Assemble in seed order
    let result: Vec<AlbumCardMeta> = ids
        .iter()
        .filter_map(|id| {
            meta_map
                .get(id)
                .or_else(|| api_cache_resolved.get(id))
                .or_else(|| api_resolved.get(id))
                .cloned()
        })
        .collect();

    Ok(result)
}

/// Resolve track IDs → TrackDisplayMeta with 3-tier cache
async fn resolve_tracks(
    ids: &[u64],
    reco_state: &State<'_, RecoState>,
    app_state: &State<'_, AppState>,
    cache_state: &State<'_, ApiCacheState>,
) -> Result<Vec<TrackDisplayMeta>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    // Tier 1: reco meta
    let meta_hits = {
        let guard__ = reco_state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.get_track_metas(ids)?
    };
    let meta_map: HashMap<u64, TrackDisplayMeta> =
        meta_hits.into_iter().map(|m| (m.id, m)).collect();

    let missing_ids: Vec<u64> = ids
        .iter()
        .filter(|id| !meta_map.contains_key(id))
        .copied()
        .collect();

    // Tier 2: API cache
    let mut api_cache_resolved: HashMap<u64, TrackDisplayMeta> = HashMap::new();
    if !missing_ids.is_empty() {
        let cached = {
            let guard__ = cache_state.cache.lock().await;
            let cache = guard__
                .as_ref()
                .ok_or("No active session - please log in")?;
            cache.get_tracks(&missing_ids, None)?
        };
        for (track_id, json_str) in cached {
            if let Ok(track) = serde_json::from_str::<Track>(&json_str) {
                let meta = track_to_display_meta(&track);
                {
                    let guard__ = reco_state.db.lock().await;
                    if let Some(db) = guard__.as_ref() {
                        let _ = db.set_track_meta(&meta);
                    }
                }
                api_cache_resolved.insert(track_id, meta);
            }
        }
    }

    let still_missing: Vec<u64> = missing_ids
        .iter()
        .filter(|id| !api_cache_resolved.contains_key(id))
        .copied()
        .collect();

    // Tier 3: Qobuz API
    let mut api_resolved: HashMap<u64, TrackDisplayMeta> = HashMap::new();
    if !still_missing.is_empty() {
        log::info!(
            "Home resolved: fetching {} tracks from Qobuz API",
            still_missing.len()
        );
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
        let client = app_state.client.clone();
        let cache_arc = cache_state.cache.clone();
        let reco_arc = reco_state.db.clone();

        let mut handles = Vec::new();
        for track_id in &still_missing {
            let sem = sem.clone();
            let client = client.clone();
            let cache_arc = cache_arc.clone();
            let reco_arc = reco_arc.clone();
            let track_id = *track_id;

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok()?;
                let track = {
                    let c = client.read().await;
                    c.get_track(track_id).await.ok()?
                };
                let meta = track_to_display_meta(&track);

                if let Ok(json) = serde_json::to_string(&track) {
                    let guard__ = cache_arc.lock().await;
                    if let Some(cache) = guard__.as_ref() {
                        let _ = cache.set_track(track_id, &json);
                    }
                }
                {
                    let guard__ = reco_arc.lock().await;
                    if let Some(db) = guard__.as_ref() {
                        let _ = db.set_track_meta(&meta);
                    }
                }

                Some((track_id, meta))
            }));
        }

        for handle in handles {
            if let Ok(Some((id, meta))) = handle.await {
                api_resolved.insert(id, meta);
            }
        }
    }

    let result: Vec<TrackDisplayMeta> = ids
        .iter()
        .filter_map(|id| {
            meta_map
                .get(id)
                .or_else(|| api_cache_resolved.get(id))
                .or_else(|| api_resolved.get(id))
                .cloned()
        })
        .collect();

    Ok(result)
}

/// Resolve artist IDs → ArtistCardMeta with 3-tier cache
async fn resolve_artists(
    ids: &[u64],
    play_counts: &HashMap<u64, u32>,
    reco_state: &State<'_, RecoState>,
    app_state: &State<'_, AppState>,
    cache_state: &State<'_, ApiCacheState>,
) -> Result<Vec<ArtistCardMeta>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    // Tier 1: reco meta
    let meta_hits = {
        let guard__ = reco_state.db.lock().await;
        let db = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        db.get_artist_metas(ids)?
    };
    let meta_map: HashMap<u64, ArtistCardMeta> = meta_hits.into_iter().map(|m| (m.id, m)).collect();

    let missing_ids: Vec<u64> = ids
        .iter()
        .filter(|id| !meta_map.contains_key(id))
        .copied()
        .collect();

    // Tier 2: API cache (locale-aware)
    let locale = {
        let client = app_state.client.read().await;
        client.get_locale().await
    };

    let mut api_cache_resolved: HashMap<u64, ArtistCardMeta> = HashMap::new();
    if !missing_ids.is_empty() {
        let cached = {
            let guard__ = cache_state.cache.lock().await;
            let cache = guard__
                .as_ref()
                .ok_or("No active session - please log in")?;
            cache.get_artists(&missing_ids, &locale, None)?
        };
        for (artist_id, json_str) in cached {
            if let Ok(artist) = serde_json::from_str::<Artist>(&json_str) {
                let meta = artist_to_card_meta(&artist, None);
                {
                    let guard__ = reco_state.db.lock().await;
                    if let Some(db) = guard__.as_ref() {
                        let _ = db.set_artist_meta(&meta);
                    }
                }
                api_cache_resolved.insert(artist_id, meta);
            }
        }
    }

    let still_missing: Vec<u64> = missing_ids
        .iter()
        .filter(|id| !api_cache_resolved.contains_key(id))
        .copied()
        .collect();

    // Tier 3: Qobuz API
    let mut api_resolved: HashMap<u64, ArtistCardMeta> = HashMap::new();
    if !still_missing.is_empty() {
        log::info!(
            "Home resolved: fetching {} artists from Qobuz API",
            still_missing.len()
        );
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
        let client = app_state.client.clone();
        let cache_arc = cache_state.cache.clone();
        let reco_arc = reco_state.db.clone();
        let locale_clone = locale.clone();

        let mut handles = Vec::new();
        for artist_id in &still_missing {
            let sem = sem.clone();
            let client = client.clone();
            let cache_arc = cache_arc.clone();
            let reco_arc = reco_arc.clone();
            let locale = locale_clone.clone();
            let artist_id = *artist_id;

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok()?;
                let artist = {
                    let c = client.read().await;
                    c.get_artist_basic(artist_id).await.ok()?
                };
                let meta = artist_to_card_meta(&artist, None);

                if let Ok(json) = serde_json::to_string(&artist) {
                    let guard__ = cache_arc.lock().await;
                    if let Some(cache) = guard__.as_ref() {
                        let _ = cache.set_artist(artist_id, &locale, &json);
                    }
                }
                {
                    let guard__ = reco_arc.lock().await;
                    if let Some(db) = guard__.as_ref() {
                        let _ = db.set_artist_meta(&meta);
                    }
                }

                Some((artist_id, meta))
            }));
        }

        for handle in handles {
            if let Ok(Some((id, meta))) = handle.await {
                api_resolved.insert(id, meta);
            }
        }
    }

    // Assemble in seed order, attaching play_counts
    let result: Vec<ArtistCardMeta> = ids
        .iter()
        .filter_map(|id| {
            let mut meta = meta_map
                .get(id)
                .or_else(|| api_cache_resolved.get(id))
                .or_else(|| api_resolved.get(id))
                .cloned()?;
            meta.play_count = play_counts.get(id).copied();
            Some(meta)
        })
        .collect();

    Ok(result)
}
