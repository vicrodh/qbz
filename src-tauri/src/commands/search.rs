//! Search commands

use tauri::State;

use crate::api::{Album, Artist, ArtistAlbums, LabelDetail, SearchResultsPage, Track, TracksContainer};
use crate::api_cache::ApiCacheState;
use crate::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchAllResults {
    pub albums: SearchResultsPage<Album>,
    pub tracks: SearchResultsPage<Track>,
    pub artists: SearchResultsPage<Artist>,
}

#[tauri::command]
pub async fn search_albums(
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
    search_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Album>, String> {
    let client = state.client.lock().await;
    client
        .search_albums(&query, limit.unwrap_or(20), offset.unwrap_or(0), search_type.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_tracks(
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
    search_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Track>, String> {
    let client = state.client.lock().await;
    client
        .search_tracks(&query, limit.unwrap_or(20), offset.unwrap_or(0), search_type.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_artists(
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
    search_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Artist>, String> {
    let client = state.client.lock().await;
    client
        .search_artists(&query, limit.unwrap_or(20), offset.unwrap_or(0), search_type.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_all(
    query: String,
    search_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<SearchAllResults, String> {
    let client = state.client.lock().await;
    let st = search_type.as_deref();

    let (albums_result, tracks_result, artists_result) = tokio::join!(
        client.search_albums(&query, 30, 0, st),
        client.search_tracks(&query, 8, 0, st),
        client.search_artists(&query, 12, 0, st)
    );

    Ok(SearchAllResults {
        albums: albums_result.map_err(|e| e.to_string())?,
        tracks: tracks_result.map_err(|e| e.to_string())?,
        artists: artists_result.map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
pub async fn get_album(
    album_id: String,
    state: State<'_, AppState>,
    cache_state: State<'_, ApiCacheState>,
) -> Result<Album, String> {
    // Check cache first
    {
        let cache = cache_state.cache.lock().await;
        if let Some(cached_data) = cache.get_album(&album_id, None)? {
            log::debug!("Cache hit for album {}", album_id);
            return serde_json::from_str(&cached_data)
                .map_err(|e| format!("Failed to parse cached album: {}", e));
        }
    }

    // Cache miss - fetch from API
    log::debug!("Cache miss for album {}, fetching from API", album_id);
    let client = state.client.lock().await;
    let album = client.get_album(&album_id).await.map_err(|e| e.to_string())?;

    // Cache the result
    {
        let cache = cache_state.cache.lock().await;
        let json = serde_json::to_string(&album)
            .map_err(|e| format!("Failed to serialize album: {}", e))?;
        cache.set_album(&album_id, &json)?;
        log::debug!("Cached album {}", album_id);
    }

    Ok(album)
}

/// Get featured albums by type (new-releases, press-awards)
#[tauri::command]
pub async fn get_featured_albums(
    featured_type: String,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Album>, String> {
    let client = state.client.lock().await;
    client
        .get_featured_albums(&featured_type, limit.unwrap_or(12), offset.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_track(
    track_id: u64,
    state: State<'_, AppState>,
    cache_state: State<'_, ApiCacheState>,
) -> Result<Track, String> {
    // Check cache first
    {
        let cache = cache_state.cache.lock().await;
        if let Some(cached_data) = cache.get_track(track_id, None)? {
            log::debug!("Cache hit for track {}", track_id);
            return serde_json::from_str(&cached_data)
                .map_err(|e| format!("Failed to parse cached track: {}", e));
        }
    }

    // Cache miss - fetch from API
    log::debug!("Cache miss for track {}, fetching from API", track_id);
    let client = state.client.lock().await;
    let track = client.get_track(track_id).await.map_err(|e| e.to_string())?;

    // Cache the result
    {
        let cache = cache_state.cache.lock().await;
        let json = serde_json::to_string(&track)
            .map_err(|e| format!("Failed to serialize track: {}", e))?;
        cache.set_track(track_id, &json)?;
    }

    Ok(track)
}

/// Get artist with albums
#[tauri::command]
pub async fn get_artist(
    artist_id: u64,
    state: State<'_, AppState>,
    cache_state: State<'_, ApiCacheState>,
) -> Result<Artist, String> {
    log::info!("Command: get_artist {}", artist_id);

    // Get current locale
    let locale = {
        let client = state.client.lock().await;
        client.get_locale().await
    };

    // Check cache first
    {
        let cache = cache_state.cache.lock().await;
        if let Some(cached_data) = cache.get_artist(artist_id, &locale, None)? {
            log::debug!("Cache hit for artist {} (locale: {})", artist_id, locale);
            return serde_json::from_str(&cached_data)
                .map_err(|e| format!("Failed to parse cached artist: {}", e));
        }
    }

    // Cache miss - fetch from API
    log::debug!("Cache miss for artist {} (locale: {}), fetching from API", artist_id, locale);
    let client = state.client.lock().await;
    let artist = client
        .get_artist(artist_id, true)
        .await
        .map_err(|e| e.to_string())?;

    // Cache the result
    {
        let cache = cache_state.cache.lock().await;
        let json = serde_json::to_string(&artist)
            .map_err(|e| format!("Failed to serialize artist: {}", e))?;
        cache.set_artist(artist_id, &locale, &json)?;
    }

    Ok(artist)
}

/// Get artist detail with albums, playlists, and appears-on tracks
/// Fetches 1000 albums initially to ensure each album type section
/// (Discography, EPs, Live, etc.) has enough albums loaded
#[tauri::command]
pub async fn get_artist_detail(
    artist_id: u64,
    state: State<'_, AppState>,
) -> Result<Artist, String> {
    log::info!("Command: get_artist_detail {}", artist_id);

    let client = state.client.lock().await;
    client
        .get_artist_detail(artist_id, Some(1000), Some(0))
        .await
        .map_err(|e| e.to_string())
}

/// Get artist's popular/top tracks
#[tauri::command]
pub async fn get_artist_tracks(
    artist_id: u64,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<TracksContainer, String> {
    log::info!(
        "Command: get_artist_tracks {} limit={:?} offset={:?}",
        artist_id,
        limit,
        offset
    );

    let client = state.client.lock().await;
    client
        .get_artist_tracks(artist_id, limit.unwrap_or(50), offset.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())
}

/// Get artist albums with pagination (for load more)
#[tauri::command]
pub async fn get_artist_albums(
    artist_id: u64,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<ArtistAlbums, String> {
    log::info!(
        "Command: get_artist_albums {} limit={:?} offset={:?}",
        artist_id,
        limit,
        offset
    );

    let client = state.client.lock().await;
    let artist = client
        .get_artist_with_pagination(artist_id, true, limit, offset)
        .await
        .map_err(|e| e.to_string())?;

    artist
        .albums
        .ok_or_else(|| "No albums in response".to_string())
}

/// Get similar artists for an artist ID
#[tauri::command]
pub async fn get_similar_artists(
    artist_id: u64,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Artist>, String> {
    log::info!(
        "Command: get_similar_artists {} limit={:?} offset={:?}",
        artist_id,
        limit,
        offset
    );

    let client = state.client.lock().await;
    client
        .get_similar_artists(artist_id, limit.unwrap_or(5), offset.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())
}

/// Get label detail with albums
#[tauri::command]
pub async fn get_label(
    label_id: u64,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<LabelDetail, String> {
    log::info!(
        "Command: get_label {} limit={:?} offset={:?}",
        label_id,
        limit,
        offset
    );

    let client = state.client.lock().await;
    client
        .get_label(label_id, limit.unwrap_or(100), offset.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())
}
