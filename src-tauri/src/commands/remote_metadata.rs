//! Tauri commands for remote metadata fetching (MusicBrainz, Discogs)
//!
//! These commands allow the Tag Editor to search for and fetch
//! album metadata from external services.

use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::library::remote_metadata::{
    RemoteAlbumMetadata, RemoteAlbumSearchResult, RemoteMetadataState, RemoteProvider,
};

/// State wrapper for Tauri
pub struct RemoteMetadataSharedState {
    pub inner: Arc<Mutex<RemoteMetadataState>>,
}

/// Search for albums on a remote provider
///
/// # Arguments
/// * `provider` - "musicbrainz" or "discogs"
/// * `query` - Album title to search for
/// * `artist` - Optional artist name to narrow search
/// * `limit` - Maximum results (1-25)
#[tauri::command]
pub async fn remote_metadata_search(
    provider: String,
    query: String,
    artist: Option<String>,
    limit: Option<usize>,
    state: State<'_, RemoteMetadataSharedState>,
) -> Result<Vec<RemoteAlbumSearchResult>, String> {
    log::info!(
        "Command: remote_metadata_search provider={} query={} artist={:?}",
        provider,
        query,
        artist
    );

    let provider = parse_provider(&provider)?;
    let limit = limit.unwrap_or(10).min(25);

    let state_guard = state.inner.lock().await;
    state_guard
        .search_albums(provider, &query, artist.as_deref(), limit)
        .await
        .map_err(|e| format!("{:?}", e))
}

/// Get full album metadata from a remote provider
///
/// # Arguments
/// * `provider` - "musicbrainz" or "discogs"
/// * `provider_id` - The provider-specific ID (MBID or Discogs release ID)
#[tauri::command]
pub async fn remote_metadata_get_album(
    provider: String,
    provider_id: String,
    state: State<'_, RemoteMetadataSharedState>,
) -> Result<RemoteAlbumMetadata, String> {
    log::info!(
        "Command: remote_metadata_get_album provider={} id={}",
        provider,
        provider_id
    );

    let provider = parse_provider(&provider)?;

    let state_guard = state.inner.lock().await;
    state_guard
        .get_album_metadata(provider, &provider_id)
        .await
        .map_err(|e| format!("{:?}", e))
}

/// Get cache statistics
#[tauri::command]
pub async fn remote_metadata_cache_stats(
    state: State<'_, RemoteMetadataSharedState>,
) -> Result<crate::library::remote_metadata::CacheStats, String> {
    let state_guard = state.inner.lock().await;
    Ok(state_guard.cache.stats().await)
}

/// Clear the remote metadata cache
#[tauri::command]
pub async fn remote_metadata_clear_cache(
    state: State<'_, RemoteMetadataSharedState>,
) -> Result<(), String> {
    log::info!("Command: remote_metadata_clear_cache");
    let state_guard = state.inner.lock().await;
    state_guard.cache.clear_all().await;
    Ok(())
}

/// Parse provider string to enum
fn parse_provider(provider: &str) -> Result<RemoteProvider, String> {
    match provider.to_lowercase().as_str() {
        "musicbrainz" | "mb" => Ok(RemoteProvider::MusicBrainz),
        "discogs" => Ok(RemoteProvider::Discogs),
        _ => Err(format!("Unknown provider: {}", provider)),
    }
}
