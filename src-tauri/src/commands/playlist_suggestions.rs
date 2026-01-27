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
use crate::musicbrainz::MusicBrainzSharedState;
use crate::AppState;

/// Input for generating suggestions
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SuggestionsInput {
    /// Artist MBIDs from the playlist
    pub artist_mbids: Vec<String>,
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
    let engine = SuggestionsEngine::new(
        store_state.store.clone(),
        builder,
        app_state.client.clone(),
        config,
    );

    // Convert exclude list to HashSet
    let exclude_set: HashSet<u64> = input.exclude_track_ids.into_iter().collect();

    // Generate suggestions
    engine
        .generate_suggestions(&input.artist_mbids, &exclude_set, input.include_reasons)
        .await
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
