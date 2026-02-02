//! Search commands

use tauri::State;

use crate::api::{endpoints, endpoints::paths, Album, Artist, ArtistAlbums, LabelDetail, Playlist, SearchResultsPage, Track, TracksContainer};
use crate::api_cache::ApiCacheState;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Most popular item from catalog search - can be a track, album, or artist
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "content", rename_all = "lowercase")]
pub enum MostPopularItem {
    Tracks(Track),
    Albums(Album),
    Artists(Artist),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchAllResults {
    pub albums: SearchResultsPage<Album>,
    pub tracks: SearchResultsPage<Track>,
    pub artists: SearchResultsPage<Artist>,
    pub playlists: SearchResultsPage<Playlist>,
    pub most_popular: Option<MostPopularItem>,
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
    state: State<'_, AppState>,
) -> Result<SearchAllResults, String> {
    log::debug!("search_all called with query: {}", query);
    let client = state.client.lock().await;

    // Use catalog/search endpoint which returns everything including most_popular
    let url = endpoints::build_url(paths::CATALOG_SEARCH);

    let response: Value = client
        .get_http()
        .get(&url)
        .header("X-App-Id", client.app_id().await.map_err(|e| e.to_string())?)
        .header("X-User-Auth-Token", client.auth_token().await.map_err(|e| e.to_string())?)
        .query(&[
            ("query", query.as_str()),
            ("limit", "30"),
            ("offset", "0"),
        ])
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("JSON parse failed: {}", e))?;

    // Parse albums
    let albums: SearchResultsPage<Album> = response
        .get("albums")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage { items: vec![], total: 0, offset: 0, limit: 30 });

    // Parse tracks
    let tracks: SearchResultsPage<Track> = response
        .get("tracks")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage { items: vec![], total: 0, offset: 0, limit: 30 });

    // Parse artists
    let artists: SearchResultsPage<Artist> = response
        .get("artists")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage { items: vec![], total: 0, offset: 0, limit: 30 });

    // Parse playlists
    let playlists: SearchResultsPage<Playlist> = response
        .get("playlists")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage { items: vec![], total: 0, offset: 0, limit: 30 });

    // Parse most_popular - get the first item
    let most_popular: Option<MostPopularItem> = response
        .get("most_popular")
        .and_then(|mp| mp.get("items"))
        .and_then(|items| items.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| {
            let item_type = item.get("type")?.as_str()?;
            let content = item.get("content")?;

            log::debug!("most_popular type: {}, content keys: {:?}",
                item_type,
                content.as_object().map(|o| o.keys().collect::<Vec<_>>())
            );

            match item_type {
                "tracks" => serde_json::from_value::<Track>(content.clone())
                    .ok()
                    .map(MostPopularItem::Tracks),
                "albums" => serde_json::from_value::<Album>(content.clone())
                    .ok()
                    .map(MostPopularItem::Albums),
                "artists" => serde_json::from_value::<Artist>(content.clone())
                    .ok()
                    .map(MostPopularItem::Artists),
                _ => None,
            }
        });

    Ok(SearchAllResults {
        albums,
        tracks,
        artists,
        playlists,
        most_popular,
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
    genre_id: Option<u64>,
    state: State<'_, AppState>,
) -> Result<SearchResultsPage<Album>, String> {
    log::info!(
        "Command: get_featured_albums type={} limit={:?} offset={:?} genre_id={:?}",
        featured_type,
        limit,
        offset,
        genre_id
    );
    let client = state.client.lock().await;
    client
        .get_featured_albums(&featured_type, limit.unwrap_or(12), offset.unwrap_or(0), genre_id)
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

/// Get list of genres for filtering
#[tauri::command]
pub async fn get_genres(
    parent_id: Option<u64>,
    state: State<'_, AppState>,
) -> Result<Vec<crate::api::models::GenreInfo>, String> {
    log::info!("Command: get_genres parent_id={:?}", parent_id);

    let client = state.client.lock().await;
    client
        .get_genres(parent_id)
        .await
        .map_err(|e| e.to_string())
}
