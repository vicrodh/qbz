//! Tauri commands for vector-based playlist suggestions
//!
//! These commands expose the playlist suggestions engine to the frontend.

use std::collections::HashSet;
use std::sync::Arc;
use tauri::State;

use crate::artist_vectors::{
    ArtistVectorBuilder, ArtistVectorStoreState, RelationshipWeights, StoreStats,
    SuggestionConfig, SuggestionResult, SuggestionsEngine,
};
use crate::musicbrainz::{MatchConfidence, MusicBrainzSharedState, ResolvedArtist};
use crate::AppState;

/// Artist info from the frontend
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PlaylistArtist {
    /// Artist name (required)
    pub name: String,
    /// Qobuz artist ID (optional, for better matching)
    #[serde(default)]
    pub qobuz_id: Option<u64>,
}

/// Input for generating suggestions
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SuggestionsInput {
    /// Artists from the playlist (names + optional Qobuz IDs)
    pub artists: Vec<PlaylistArtist>,
    /// Track IDs already in the playlist (to exclude)
    pub exclude_track_ids: Vec<u64>,
    /// Whether to include reason strings (dev mode)
    #[serde(default)]
    pub include_reasons: bool,
    /// Optional custom configuration
    pub config: Option<SuggestionConfig>,
}

/// Get suggestions for a playlist based on artist similarity vectors
#[tauri::command]
pub async fn get_playlist_suggestions_v2(
    input: SuggestionsInput,
    store_state: State<'_, ArtistVectorStoreState>,
    mb_state: State<'_, MusicBrainzSharedState>,
    app_state: State<'_, AppState>,
) -> Result<SuggestionResult, String> {
    use std::time::Instant;
    let total_start = Instant::now();

    log::info!(
        "[Suggestions] Starting for {} artists, exclude {} tracks",
        input.artists.len(),
        input.exclude_track_ids.len()
    );

    // Resolve artist names to MBIDs (with caching)
    let resolve_start = Instant::now();
    let mut artist_mbids = Vec::new();
    let mut cached_count = 0;
    let mut resolved_count = 0;
    let mut failed_count = 0;

    for artist in &input.artists {
        // Check cache first
        let cached = {
            let cache = mb_state.cache.lock().await;
            cache.get_artist(&artist.name).ok().flatten()
        };

        if let Some(resolved) = cached {
            if let Some(mbid) = resolved.mbid {
                if resolved.confidence != MatchConfidence::None
                    && resolved.confidence != MatchConfidence::Low
                {
                    artist_mbids.push(mbid);
                    cached_count += 1;
                    continue;
                }
            }
            // Cached as "no match"
            failed_count += 1;
            continue;
        }

        // Try to resolve via MusicBrainz search
        if mb_state.client.is_enabled().await {
            match mb_state.client.search_artist(&artist.name).await {
                Ok(response) => {
                    if let Some(mb_artist) = response
                        .artists
                        .iter()
                        .find(|a| a.score.unwrap_or(0) >= 90)
                    {
                        let mbid = mb_artist.id.clone();
                        // Cache the resolved artist
                        let resolved = ResolvedArtist {
                            mbid: Some(mbid.clone()),
                            name: Some(mb_artist.name.clone()),
                            sort_name: mb_artist.sort_name.clone(),
                            artist_type: None,
                            country: mb_artist.country.clone(),
                            disambiguation: mb_artist.disambiguation.clone(),
                            confidence: MatchConfidence::High,
                        };
                        {
                            let cache = mb_state.cache.lock().await;
                            let _ = cache.set_artist(&artist.name, &resolved);
                        }
                        artist_mbids.push(mbid);
                        resolved_count += 1;
                        continue;
                    }
                }
                Err(e) => {
                    log::warn!("[Suggestions] MusicBrainz search failed for '{}': {}", artist.name, e);
                }
            }
        } else {
            log::debug!("[Suggestions] MusicBrainz disabled, skipping '{}'", artist.name);
        }

        // Cache negative result
        {
            let cache = mb_state.cache.lock().await;
            let _ = cache.set_artist(&artist.name, &ResolvedArtist::empty());
        }
        failed_count += 1;
    }

    let resolve_elapsed = resolve_start.elapsed();
    log::info!(
        "[Suggestions] MBID resolution took {:?}: {} cached, {} resolved, {} failed",
        resolve_elapsed,
        cached_count,
        resolved_count,
        failed_count
    );

    if artist_mbids.is_empty() {
        log::warn!("[Suggestions] No MBIDs resolved, returning empty result");
        return Ok(SuggestionResult {
            tracks: Vec::new(),
            source_artists: Vec::new(),
            playlist_artists_count: input.artists.len(),
            similar_artists_count: 0,
        });
    }

    // Create the builder (just cloning Arcs, cheap)
    let builder = Arc::new(ArtistVectorBuilder::new(
        store_state.store.clone(),
        mb_state.client.clone(),
        mb_state.cache.clone(),
        app_state.client.clone(),
        RelationshipWeights::default(),
    ));

    // Create the engine with config
    let config = input.config.unwrap_or_default();
    log::info!("[Suggestions] Config: max_artists={}, tracks_per={}, pool_size={}",
        config.max_artists, config.tracks_per_artist, config.max_pool_size);

    let engine = SuggestionsEngine::new(
        store_state.store.clone(),
        builder,
        app_state.client.clone(),
        config,
    );

    // Convert exclude list to HashSet
    let exclude_set: HashSet<u64> = input.exclude_track_ids.into_iter().collect();

    // Generate suggestions
    log::info!("[Suggestions] Calling engine.generate_suggestions with {} MBIDs", artist_mbids.len());
    let engine_start = std::time::Instant::now();

    let result = engine
        .generate_suggestions(&artist_mbids, &exclude_set, input.include_reasons)
        .await;

    let engine_elapsed = engine_start.elapsed();
    let total_elapsed = total_start.elapsed();

    match &result {
        Ok(r) => {
            log::info!(
                "[Suggestions] Engine took {:?}, total {:?}. Result: {} tracks from {} similar artists",
                engine_elapsed,
                total_elapsed,
                r.tracks.len(),
                r.similar_artists_count
            );
        }
        Err(e) => {
            log::error!("[Suggestions] Engine failed after {:?}: {}", engine_elapsed, e);
        }
    }

    result
}

/// Get store statistics for debugging
#[tauri::command]
pub async fn get_vector_store_stats(
    store_state: State<'_, ArtistVectorStoreState>,
) -> Result<StoreStats, String> {
    let store = store_state.store.lock().await;
    store.get_stats()
}

/// Clean up expired vectors
#[tauri::command]
pub async fn cleanup_vector_store(
    max_age_days: Option<i64>,
    store_state: State<'_, ArtistVectorStoreState>,
) -> Result<usize, String> {
    let max_age_secs = max_age_days.unwrap_or(30) * 24 * 60 * 60;
    let mut store = store_state.store.lock().await;
    store.cleanup_expired(max_age_secs)
}
