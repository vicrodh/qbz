//! Tauri commands for vector-based playlist suggestions
//!
//! These commands expose the playlist suggestions engine to the frontend.

use std::collections::HashSet;
use std::sync::Arc;
use tauri::State;

use crate::artist_blacklist::BlacklistState;
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

/// Genres that are typically incompatible with rock/metal playlists
/// Used to filter out false positives from name-only matching
const INCOMPATIBLE_GENRE_KEYWORDS: &[&str] = &[
    "merengue", "reggaeton", "salsa", "bachata", "cumbia", "vallenato",
    "k-pop", "kpop", "j-pop", "jpop", "mandopop", "cantopop",
    "schlager", "volksmusi", "chanson",
    "country singer", "country music",
    "gospel singer", "christian music",
    "children", "nursery",
];

/// Check if an artist's disambiguation suggests an incompatible genre
fn has_incompatible_disambiguation(disambiguation: Option<&str>) -> bool {
    match disambiguation {
        Some(d) => {
            let lower = d.to_lowercase();
            INCOMPATIBLE_GENRE_KEYWORDS.iter().any(|kw| lower.contains(kw))
        }
        None => false,
    }
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
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SuggestionResult, String> {
    use std::time::Instant;
    let total_start = Instant::now();

    log::info!(
        "[Suggestions] Starting for {} artists, exclude {} tracks",
        input.artists.len(),
        input.exclude_track_ids.len()
    );

    // Get config early to check skip_vector_build
    let config = input.config.unwrap_or_default();
    let skip_network = config.skip_vector_build;

    // Resolve artist names to MBIDs (with caching)
    // Store (mbid, name) tuples so we can search for tracks by name
    let resolve_start = Instant::now();
    let mut artist_info: Vec<(String, String)> = Vec::new(); // (mbid, name)
    let mut cached_count = 0;
    let mut resolved_count = 0;
    let mut skipped_count = 0;

    for artist in &input.artists {
        // Check cache first
        let cached = {
            let cache_opt__ = mb_state.cache.lock().await;
            let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
            cache.get_artist(&artist.name).ok().flatten()
        };

        if let Some(resolved) = cached {
            if let Some(mbid) = resolved.mbid {
                if resolved.confidence != MatchConfidence::None
                    && resolved.confidence != MatchConfidence::Low
                {
                    artist_info.push((mbid, artist.name.clone()));
                    cached_count += 1;
                    continue;
                }
            }
            // Cached as "no match" - skip
            skipped_count += 1;
            continue;
        }

        // If skip_network is true, don't make network calls for uncached artists
        if skip_network {
            skipped_count += 1;
            continue;
        }

        // Try to resolve via MusicBrainz search
        if mb_state.client.is_enabled().await {
            match mb_state.client.search_artist(&artist.name).await {
                Ok(response) => {
                    // Find best match: high score AND compatible genre
                    // This prevents "Martín Méndez" (Opeth bassist) matching "Martin Mendez" (merengue singer)
                    if let Some(mb_artist) = response
                        .artists
                        .iter()
                        .filter(|a| a.score.unwrap_or(0) >= 90)
                        .find(|a| !has_incompatible_disambiguation(a.disambiguation.as_deref()))
                    {
                        let mbid = mb_artist.id.clone();
                        log::debug!(
                            "[Suggestions] Resolved '{}' -> {} (score={}, disambiguation={:?})",
                            artist.name,
                            mb_artist.name,
                            mb_artist.score.unwrap_or(0),
                            mb_artist.disambiguation
                        );
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
                            let cache_opt__ = mb_state.cache.lock().await;
                            let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
                            let _ = cache.set_artist(&artist.name, &resolved);
                        }
                        artist_info.push((mbid, artist.name.clone()));
                        resolved_count += 1;
                        continue;
                    } else if let Some(skipped) = response.artists.iter().find(|a| {
                        a.score.unwrap_or(0) >= 90 && has_incompatible_disambiguation(a.disambiguation.as_deref())
                    }) {
                        log::info!(
                            "[Suggestions] Skipped incompatible artist for '{}': {} (disambiguation: {:?})",
                            artist.name,
                            skipped.name,
                            skipped.disambiguation
                        );
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
            let cache_opt__ = mb_state.cache.lock().await;
            let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
            let _ = cache.set_artist(&artist.name, &ResolvedArtist::empty());
        }
        skipped_count += 1;
    }

    let resolve_elapsed = resolve_start.elapsed();
    log::debug!(
        "[Suggestions] MBID resolution took {:?}: {} cached, {} resolved, {} skipped (skip_network={})",
        resolve_elapsed,
        cached_count,
        resolved_count,
        skipped_count,
        skip_network
    );

    if artist_info.is_empty() {
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

    // Log config (already extracted above for skip_network check)
    log::debug!("[Suggestions] Config: max_artists={}, tracks_per={}, pool_size={}, skip_build={}",
        config.max_artists, config.tracks_per_artist, config.max_pool_size, config.skip_vector_build);

    let engine = SuggestionsEngine::new(
        store_state.store.clone(),
        builder,
        app_state.client.clone(),
        config,
    );

    // Convert exclude list to HashSet
    let exclude_set: HashSet<u64> = input.exclude_track_ids.into_iter().collect();

    // Generate suggestions
    log::debug!("[Suggestions] Calling engine.generate_suggestions with {} artists", artist_info.len());
    let engine_start = std::time::Instant::now();

    let result = engine
        .generate_suggestions(&artist_info, &exclude_set, input.include_reasons)
        .await;

    let engine_elapsed = engine_start.elapsed();
    let total_elapsed = total_start.elapsed();

    match result {
        Ok(mut r) => {
            log::info!(
                "[Suggestions] Engine took {:?}, total {:?}. Result: {} tracks from {} similar artists",
                engine_elapsed,
                total_elapsed,
                r.tracks.len(),
                r.similar_artists_count
            );

            // Filter out tracks from blacklisted artists
            let original_count = r.tracks.len();
            r.tracks.retain(|track| {
                if let Some(artist_id) = track.artist_id {
                    !blacklist_state.is_blacklisted(artist_id)
                } else {
                    true // Keep tracks without artist ID
                }
            });

            let filtered_count = original_count - r.tracks.len();
            if filtered_count > 0 {
                log::info!(
                    "[Suggestions] Filtered {} tracks from blacklisted artists",
                    filtered_count
                );
            }

            Ok(r)
        }
        Err(e) => {
            log::error!("[Suggestions] Engine failed after {:?}: {}", engine_elapsed, e);
            Err(e)
        }
    }
}

/// Get store statistics for debugging
#[tauri::command]
pub async fn get_vector_store_stats(
    store_state: State<'_, ArtistVectorStoreState>,
) -> Result<StoreStats, String> {
    let guard = store_state.store.lock().await;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_stats()
}

/// Clean up expired vectors
#[tauri::command]
pub async fn cleanup_vector_store(
    max_age_days: Option<i64>,
    store_state: State<'_, ArtistVectorStoreState>,
) -> Result<usize, String> {
    let max_age_secs = max_age_days.unwrap_or(30) * 24 * 60 * 60;
    let mut guard = store_state.store.lock().await;
    let store = guard.as_mut().ok_or("No active session - please log in")?;
    store.cleanup_expired(max_age_secs)
}

/// Clear all vector store data
#[tauri::command]
pub async fn clear_vector_store(
    store_state: State<'_, ArtistVectorStoreState>,
) -> Result<usize, String> {
    let mut guard = store_state.store.lock().await;
    let store = guard.as_mut().ok_or("No active session - please log in")?;
    store.clear_all()
}
