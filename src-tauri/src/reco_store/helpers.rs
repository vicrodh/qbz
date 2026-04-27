//! Helper functions for the recommendation store.
//!
//! These are non-command helpers consumed by V2 commands in
//! `commands_v2/library.rs` (specifically the home-recommendation paths).

use std::collections::HashMap;

use tauri::State;

use crate::api::models::{Album, Artist, ImageSet, Track};
use crate::api_cache::ApiCacheState;
use crate::reco_store::{
    AlbumCardMeta, ArtistCardMeta, HomeSeeds, RecoState, TopArtistSeed, TrackDisplayMeta,
};
use crate::AppState;

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
        .small
        .as_ref()
        .or(image.large.as_ref())
        .or(image.thumbnail.as_ref())
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn album_to_card_meta(album: &Album) -> AlbumCardMeta {
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
pub(crate) fn get_home_seeds_internal(
    db: &crate::reco_store::db::RecoStoreDb,
    limit_recent_albums: u32,
    limit_continue_tracks: u32,
    limit_top_artists: u32,
    limit_favorites: u32,
) -> Result<HomeSeeds, String> {
    let has_scores = db.has_scores("all")?;

    // When scores exist, merge scored results with recent items.
    // When no scores (or scored results empty), fall back to event-based queries.
    // Avoid duplicate queries: only call fallback if scored path was taken but empty.
    let recently_played_album_ids = if has_scores {
        let recent_fresh = db.get_recent_album_ids(4)?;
        let scored = db.get_scored_album_ids("all", limit_recent_albums + 4)?;
        let merged =
            merge_unique_preserve_order(recent_fresh, scored, limit_recent_albums as usize);
        if merged.is_empty() {
            db.get_recent_album_ids(limit_recent_albums)?
        } else {
            merged
        }
    } else {
        db.get_recent_album_ids(limit_recent_albums)?
    };

    let continue_listening_track_ids = if has_scores {
        let recent_fresh = db.get_recent_track_ids(4)?;
        let scored = db.get_scored_track_ids("all", limit_continue_tracks + 4)?;
        let merged =
            merge_unique_preserve_order(recent_fresh, scored, limit_continue_tracks as usize);
        if merged.is_empty() {
            db.get_recent_track_ids(limit_continue_tracks)?
        } else {
            merged
        }
    } else {
        db.get_recent_track_ids(limit_continue_tracks)?
    };

    let top_artist_ids = if has_scores {
        let scored: Vec<TopArtistSeed> = db
            .get_scored_artist_scores("all", limit_top_artists)?
            .into_iter()
            .map(|(artist_id, score)| TopArtistSeed {
                artist_id,
                play_count: score.round().max(1.0) as u32,
            })
            .collect();
        if scored.is_empty() {
            db.get_top_artist_ids(limit_top_artists)?
        } else {
            scored
        }
    } else {
        db.get_top_artist_ids(limit_top_artists)?
    };

    let favorite_album_ids = if has_scores {
        let scored = db.get_scored_album_ids("favorite", limit_favorites)?;
        if scored.is_empty() {
            db.get_favorite_album_ids(limit_favorites)?
        } else {
            scored
        }
    } else {
        db.get_favorite_album_ids(limit_favorites)?
    };

    let favorite_track_ids = if has_scores {
        let scored = db.get_scored_track_ids("favorite", limit_favorites)?;
        if scored.is_empty() {
            db.get_favorite_track_ids(limit_favorites)?
        } else {
            scored
        }
    } else {
        db.get_favorite_track_ids(limit_favorites)?
    };

    Ok(HomeSeeds {
        recently_played_album_ids,
        continue_listening_track_ids,
        top_artist_ids,
        favorite_album_ids,
        favorite_track_ids,
    })
}

/// Resolve album IDs → AlbumCardMeta with 3-tier cache
pub async fn resolve_albums(
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
    let mut tier2_album_metas: Vec<AlbumCardMeta> = Vec::new();
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
                tier2_album_metas.push(meta.clone());
                api_cache_resolved.insert(album_id, meta);
            }
        }
        // Batch write tier-2 hits to reco meta (single lock)
        if !tier2_album_metas.is_empty() {
            let guard__ = reco_state.db.lock().await;
            if let Some(db) = guard__.as_ref() {
                for meta in &tier2_album_metas {
                    let _ = db.set_album_meta(meta);
                }
            }
        }
    }

    let still_missing: Vec<String> = missing_ids
        .iter()
        .filter(|id| !api_cache_resolved.contains_key(*id))
        .cloned()
        .collect();

    // Tier 3: Qobuz API — fetch in parallel, defer cache writes to after all complete
    let mut api_resolved: HashMap<String, AlbumCardMeta> = HashMap::new();
    if !still_missing.is_empty() {
        log::info!(
            "Home resolved: fetching {} albums from Qobuz API",
            still_missing.len()
        );
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
        let client = app_state.client.clone();

        let mut handles = Vec::new();
        for album_id in &still_missing {
            let sem = sem.clone();
            let client = client.clone();
            let album_id = album_id.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok()?;
                let album = {
                    let c = client.read().await;
                    c.get_album(&album_id).await.ok()?
                };
                let meta = album_to_card_meta(&album);
                let json = serde_json::to_string(&album).ok();
                Some((album_id, meta, json))
            }));
        }

        // Collect results, then batch-write caches
        let mut api_albums_to_cache: Vec<(String, String)> = Vec::new();
        let mut api_album_metas: Vec<AlbumCardMeta> = Vec::new();
        for handle in handles {
            if let Ok(Some((id, meta, json))) = handle.await {
                if let Some(j) = json {
                    api_albums_to_cache.push((id.clone(), j));
                }
                api_album_metas.push(meta.clone());
                api_resolved.insert(id, meta);
            }
        }

        // Batch write to API cache (single lock)
        if !api_albums_to_cache.is_empty() {
            let guard__ = cache_state.cache.lock().await;
            if let Some(cache) = guard__.as_ref() {
                for (album_id, json) in &api_albums_to_cache {
                    let _ = cache.set_album(album_id, json);
                }
            }
        }
        // Batch write to reco meta (single lock)
        if !api_album_metas.is_empty() {
            let guard__ = reco_state.db.lock().await;
            if let Some(db) = guard__.as_ref() {
                for meta in &api_album_metas {
                    let _ = db.set_album_meta(meta);
                }
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
pub(crate) async fn resolve_tracks(
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
    let mut tier2_track_metas: Vec<TrackDisplayMeta> = Vec::new();
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
                tier2_track_metas.push(meta.clone());
                api_cache_resolved.insert(track_id, meta);
            }
        }
        // Batch write tier-2 hits to reco meta (single lock)
        if !tier2_track_metas.is_empty() {
            let guard__ = reco_state.db.lock().await;
            if let Some(db) = guard__.as_ref() {
                for meta in &tier2_track_metas {
                    let _ = db.set_track_meta(meta);
                }
            }
        }
    }

    let still_missing: Vec<u64> = missing_ids
        .iter()
        .filter(|id| !api_cache_resolved.contains_key(id))
        .copied()
        .collect();

    // Tier 3: Qobuz API — fetch in parallel, defer cache writes
    let mut api_resolved: HashMap<u64, TrackDisplayMeta> = HashMap::new();
    if !still_missing.is_empty() {
        log::info!(
            "Home resolved: fetching {} tracks from Qobuz API",
            still_missing.len()
        );
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
        let client = app_state.client.clone();

        let mut handles = Vec::new();
        for track_id in &still_missing {
            let sem = sem.clone();
            let client = client.clone();
            let track_id = *track_id;

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok()?;
                let track = {
                    let c = client.read().await;
                    c.get_track(track_id).await.ok()?
                };
                let meta = track_to_display_meta(&track);
                let json = serde_json::to_string(&track).ok();
                Some((track_id, meta, json))
            }));
        }

        // Collect results, then batch-write caches
        let mut api_tracks_to_cache: Vec<(u64, String)> = Vec::new();
        let mut api_track_metas: Vec<TrackDisplayMeta> = Vec::new();
        for handle in handles {
            if let Ok(Some((id, meta, json))) = handle.await {
                if let Some(j) = json {
                    api_tracks_to_cache.push((id, j));
                }
                api_track_metas.push(meta.clone());
                api_resolved.insert(id, meta);
            }
        }

        // Batch write to API cache (single lock)
        if !api_tracks_to_cache.is_empty() {
            let guard__ = cache_state.cache.lock().await;
            if let Some(cache) = guard__.as_ref() {
                for (track_id, json) in &api_tracks_to_cache {
                    let _ = cache.set_track(*track_id, json);
                }
            }
        }
        // Batch write to reco meta (single lock)
        if !api_track_metas.is_empty() {
            let guard__ = reco_state.db.lock().await;
            if let Some(db) = guard__.as_ref() {
                for meta in &api_track_metas {
                    let _ = db.set_track_meta(meta);
                }
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
pub(crate) async fn resolve_artists(
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
    let mut tier2_metas_to_write: Vec<ArtistCardMeta> = Vec::new();
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
                tier2_metas_to_write.push(meta.clone());
                api_cache_resolved.insert(artist_id, meta);
            }
        }
        // Batch write tier-2 hits to reco meta (single lock acquisition)
        if !tier2_metas_to_write.is_empty() {
            let guard__ = reco_state.db.lock().await;
            if let Some(db) = guard__.as_ref() {
                for meta in &tier2_metas_to_write {
                    let _ = db.set_artist_meta(meta);
                }
            }
        }
    }

    let still_missing: Vec<u64> = missing_ids
        .iter()
        .filter(|id| !api_cache_resolved.contains_key(id))
        .copied()
        .collect();

    // Tier 3: Qobuz API — fetch in parallel, defer cache writes to after all complete
    let mut api_resolved: HashMap<u64, ArtistCardMeta> = HashMap::new();
    if !still_missing.is_empty() {
        log::info!(
            "Home resolved: fetching {} artists from Qobuz API",
            still_missing.len()
        );
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
        let client = app_state.client.clone();

        let mut handles = Vec::new();
        for artist_id in &still_missing {
            let sem = sem.clone();
            let client = client.clone();
            let artist_id = *artist_id;

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok()?;
                let artist = {
                    let c = client.read().await;
                    c.get_artist_basic(artist_id).await.ok()?
                };
                let meta = artist_to_card_meta(&artist, None);
                let json = serde_json::to_string(&artist).ok();
                Some((artist_id, meta, json))
            }));
        }

        // Collect results without holding any locks
        let mut api_artists_to_cache: Vec<(u64, String)> = Vec::new();
        let mut api_metas_to_write: Vec<ArtistCardMeta> = Vec::new();
        for handle in handles {
            if let Ok(Some((id, meta, json))) = handle.await {
                if let Some(j) = json {
                    api_artists_to_cache.push((id, j));
                }
                api_metas_to_write.push(meta.clone());
                api_resolved.insert(id, meta);
            }
        }

        // Batch write to API cache (single lock)
        if !api_artists_to_cache.is_empty() {
            let guard__ = cache_state.cache.lock().await;
            if let Some(cache) = guard__.as_ref() {
                for (artist_id, json) in &api_artists_to_cache {
                    let _ = cache.set_artist(*artist_id, &locale, json);
                }
            }
        }
        // Batch write to reco meta (single lock)
        if !api_metas_to_write.is_empty() {
            let guard__ = reco_state.db.lock().await;
            if let Some(db) = guard__.as_ref() {
                for meta in &api_metas_to_write {
                    let _ = db.set_artist_meta(meta);
                }
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
