//! Search commands

use tauri::State;

use crate::api::{
    endpoints, endpoints::paths, Album, Artist, ArtistAlbums, DiscoverAlbum, DiscoverData,
    DiscoverPlaylistsResponse, DiscoverResponse, LabelDetail, PageArtistResponse, Playlist,
    PlaylistTag, ReleasesGridResponse, SearchResultsPage, Track, TracksContainer,
};
use crate::api_cache::ApiCacheState;
use crate::artist_blacklist::BlacklistState;
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
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchResultsPage<Album>, String> {
    let mut results = {
        let client = state.client.read().await;
        client
            .search_albums(
                &query,
                limit.unwrap_or(20),
                offset.unwrap_or(0),
                search_type.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())?
    };

    // Filter out albums from blacklisted artists
    let original_count = results.items.len();
    results
        .items
        .retain(|album| !blacklist_state.is_blacklisted(album.artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!(
            "[Blacklist] Filtered {} albums from search results",
            filtered_count
        );
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

#[tauri::command]
pub async fn search_tracks(
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
    search_type: Option<String>,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchResultsPage<Track>, String> {
    let mut results = {
        let client = state.client.read().await;
        client
            .search_tracks(
                &query,
                limit.unwrap_or(20),
                offset.unwrap_or(0),
                search_type.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())?
    };

    // Filter out tracks from blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|track| {
        if let Some(ref performer) = track.performer {
            !blacklist_state.is_blacklisted(performer.id)
        } else {
            true // Keep tracks without performer info
        }
    });

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!(
            "[Blacklist] Filtered {} tracks from search results",
            filtered_count
        );
        // Adjust total to reflect filtered count (approximate)
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

#[tauri::command]
pub async fn search_artists(
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
    search_type: Option<String>,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchResultsPage<Artist>, String> {
    let mut results = {
        let client = state.client.read().await;
        client
            .search_artists(
                &query,
                limit.unwrap_or(20),
                offset.unwrap_or(0),
                search_type.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())?
    };

    // Filter out blacklisted artists
    let original_count = results.items.len();
    results
        .items
        .retain(|artist| !blacklist_state.is_blacklisted(artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!(
            "[Blacklist] Filtered {} artists from search results",
            filtered_count
        );
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

#[tauri::command]
pub async fn search_all(
    query: String,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchAllResults, String> {
    log::debug!("search_all called with query: {}", query);

    // Use catalog/search endpoint which returns everything including most_popular
    let url = endpoints::build_url(paths::CATALOG_SEARCH);

    // Acquire lock only for HTTP request, drop before parsing
    let response: Value = {
        let client = state.client.read().await;
        client
            .get_http()
            .get(&url)
            .header(
                "X-App-Id",
                client.app_id().await.map_err(|e| e.to_string())?,
            )
            .header(
                "X-User-Auth-Token",
                client.auth_token().await.map_err(|e| e.to_string())?,
            )
            .query(&[("query", query.as_str()), ("limit", "30"), ("offset", "0")])
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?
            .json()
            .await
            .map_err(|e| format!("JSON parse failed: {}", e))?
    };

    // Parse albums
    let albums: SearchResultsPage<Album> = response
        .get("albums")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage {
            items: vec![],
            total: 0,
            offset: 0,
            limit: 30,
        });

    // Parse tracks
    let mut tracks: SearchResultsPage<Track> = response
        .get("tracks")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage {
            items: vec![],
            total: 0,
            offset: 0,
            limit: 30,
        });

    // Parse artists
    let mut artists: SearchResultsPage<Artist> = response
        .get("artists")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage {
            items: vec![],
            total: 0,
            offset: 0,
            limit: 30,
        });

    // Parse playlists
    let playlists: SearchResultsPage<Playlist> = response
        .get("playlists")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage {
            items: vec![],
            total: 0,
            offset: 0,
            limit: 30,
        });

    // Parse most_popular - find the first non-blacklisted item
    let most_popular: Option<MostPopularItem> = response
        .get("most_popular")
        .and_then(|mp| mp.get("items"))
        .and_then(|items| items.as_array())
        .and_then(|arr| {
            // Iterate through all items to find the first non-blacklisted one
            for item in arr {
                let item_type = match item.get("type").and_then(|t| t.as_str()) {
                    Some(t) => t,
                    None => continue,
                };
                let content = match item.get("content") {
                    Some(c) => c,
                    None => continue,
                };

                log::debug!("most_popular type: {}, content keys: {:?}",
                    item_type,
                    content.as_object().map(|o| o.keys().collect::<Vec<_>>())
                );

                match item_type {
                    "tracks" => {
                        if let Ok(track) = serde_json::from_value::<Track>(content.clone()) {
                            // Check if track's performer is blacklisted
                            if let Some(ref performer) = track.performer {
                                if blacklist_state.is_blacklisted(performer.id) {
                                    log::debug!("[Blacklist] Skipping most_popular track from blacklisted artist: {}", performer.name);
                                    continue;
                                }
                            }
                            return Some(MostPopularItem::Tracks(track));
                        }
                    }
                    "albums" => {
                        if let Ok(album) = serde_json::from_value::<Album>(content.clone()) {
                            // Check if album's artist is blacklisted
                            if blacklist_state.is_blacklisted(album.artist.id) {
                                log::debug!("[Blacklist] Skipping most_popular album from blacklisted artist: {}", album.artist.name);
                                continue;
                            }
                            return Some(MostPopularItem::Albums(album));
                        }
                    }
                    "artists" => {
                        if let Ok(artist) = serde_json::from_value::<Artist>(content.clone()) {
                            // Check if artist is blacklisted
                            if blacklist_state.is_blacklisted(artist.id) {
                                log::debug!("[Blacklist] Skipping most_popular blacklisted artist: {}", artist.name);
                                continue;
                            }
                            return Some(MostPopularItem::Artists(artist));
                        }
                    }
                    _ => continue,
                }
            }
            None
        });

    // Apply blacklist filtering
    // Filter albums from blacklisted artists
    let original_album_count = albums.items.len();
    let mut albums = albums;
    albums
        .items
        .retain(|album| !blacklist_state.is_blacklisted(album.artist.id));
    let filtered_albums = original_album_count - albums.items.len();
    if filtered_albums > 0 {
        albums.total = albums.total.saturating_sub(filtered_albums as u32);
    }

    // Filter tracks from blacklisted artists
    let original_track_count = tracks.items.len();
    tracks.items.retain(|track| {
        if let Some(ref performer) = track.performer {
            !blacklist_state.is_blacklisted(performer.id)
        } else {
            true
        }
    });
    let filtered_tracks = original_track_count - tracks.items.len();
    if filtered_tracks > 0 {
        tracks.total = tracks.total.saturating_sub(filtered_tracks as u32);
    }

    // Filter blacklisted artists
    let original_artist_count = artists.items.len();
    artists
        .items
        .retain(|artist| !blacklist_state.is_blacklisted(artist.id));
    let filtered_artists = original_artist_count - artists.items.len();
    if filtered_artists > 0 {
        artists.total = artists.total.saturating_sub(filtered_artists as u32);
    }

    // Note: most_popular is already filtered during parsing (iteration finds first non-blacklisted)

    if filtered_tracks > 0 || filtered_artists > 0 || filtered_albums > 0 {
        log::debug!(
            "[Blacklist] search_all filtered: {} tracks, {} artists, {} albums",
            filtered_tracks,
            filtered_artists,
            filtered_albums
        );
    }

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
        let guard__ = cache_state.cache.lock().await;
        let cache = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        match cache.get_album(&album_id, None) {
            Ok(Some(cached_data)) => {
                log::debug!("Cache hit for album {}", album_id);
                return serde_json::from_str(&cached_data)
                    .map_err(|e| format!("Failed to parse cached album: {}", e));
            }
            Ok(None) => {} // Cache miss, continue to API fetch
            Err(_) => {
                // Continue to API fetch on cache error
            }
        }
    }

    // Cache miss - fetch from API
    log::debug!("Cache miss for album {}, fetching from API", album_id);
    let album = {
        let client = state.client.read().await;
        client
            .get_album(&album_id)
            .await
            .map_err(|e| e.to_string())?
    };

    // Cache the result
    {
        let guard__ = cache_state.cache.lock().await;
        let cache = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        let json = serde_json::to_string(&album)
            .map_err(|e| format!("Failed to serialize album: {}", e))?;
        let _ = cache.set_album(&album_id, &json);
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
    let result = {
        let client = state.client.read().await;
        client
            .get_featured_albums(
                &featured_type,
                limit.unwrap_or(12),
                offset.unwrap_or(0),
                genre_id,
            )
            .await
            .map_err(|e| e.to_string())?
    };
    Ok(result)
}

#[tauri::command]
pub async fn get_track(
    track_id: u64,
    state: State<'_, AppState>,
    cache_state: State<'_, ApiCacheState>,
) -> Result<Track, String> {
    // Check cache first
    {
        let guard__ = cache_state.cache.lock().await;
        let cache = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        match cache.get_track(track_id, None) {
            Ok(Some(cached_data)) => {
                log::debug!("Cache hit for track {}", track_id);
                return serde_json::from_str(&cached_data)
                    .map_err(|e| format!("Failed to parse cached track: {}", e));
            }
            Ok(None) => {} // Cache miss, continue to API fetch
            Err(_) => {
                // Continue to API fetch on cache error
            }
        }
    }

    // Cache miss - fetch from API
    log::debug!("Cache miss for track {}, fetching from API", track_id);
    let track = {
        let client = state.client.read().await;
        client
            .get_track(track_id)
            .await
            .map_err(|e| e.to_string())?
    };

    // Cache the result
    {
        let guard__ = cache_state.cache.lock().await;
        let cache = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        let json = serde_json::to_string(&track)
            .map_err(|e| format!("Failed to serialize track: {}", e))?;
        let _ = cache.set_track(track_id, &json);
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
    log::debug!("Command: get_artist {}", artist_id);

    // Get current locale
    let locale = {
        let client = state.client.read().await;
        client.get_locale().await
    };

    // Check cache first
    {
        let guard__ = cache_state.cache.lock().await;
        let cache = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        if let Some(cached_data) = cache.get_artist(artist_id, &locale, None)? {
            log::debug!("Cache hit for artist {} (locale: {})", artist_id, locale);
            return serde_json::from_str(&cached_data)
                .map_err(|e| format!("Failed to parse cached artist: {}", e));
        }
    }

    // Cache miss - fetch from API
    log::debug!(
        "Cache miss for artist {} (locale: {}), fetching from API",
        artist_id,
        locale
    );
    let client = state.client.read().await;
    let artist = client
        .get_artist(artist_id, true)
        .await
        .map_err(|e| e.to_string())?;

    // Cache the result
    {
        let guard__ = cache_state.cache.lock().await;
        let cache = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        let json = serde_json::to_string(&artist)
            .map_err(|e| format!("Failed to serialize artist: {}", e))?;
        cache.set_artist(artist_id, &locale, &json)?;
    }

    Ok(artist)
}

/// Get artist basic info only (no albums - faster for lists/cards)
#[tauri::command]
pub async fn get_artist_basic(
    artist_id: u64,
    state: State<'_, AppState>,
    cache_state: State<'_, ApiCacheState>,
) -> Result<Artist, String> {
    // Get current locale
    let locale = {
        let client = state.client.read().await;
        client.get_locale().await
    };

    // Check cache first (reuse same cache - basic response works for both)
    {
        let guard__ = cache_state.cache.lock().await;
        let cache = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
        if let Some(cached_data) = cache.get_artist(artist_id, &locale, None)? {
            log::debug!(
                "Cache hit for artist_basic {} (locale: {})",
                artist_id,
                locale
            );
            return serde_json::from_str(&cached_data)
                .map_err(|e| format!("Failed to parse cached artist: {}", e));
        }
    }

    // Cache miss - fetch from API (without albums - much faster)
    log::debug!(
        "Cache miss for artist_basic {} (locale: {}), fetching from API",
        artist_id,
        locale
    );
    let artist = {
        let client = state.client.read().await;
        client
            .get_artist_basic(artist_id)
            .await
            .map_err(|e| e.to_string())?
    };

    // Cache the result
    {
        let guard__ = cache_state.cache.lock().await;
        let cache = guard__
            .as_ref()
            .ok_or("No active session - please log in")?;
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
    log::debug!("Command: get_artist_detail {}", artist_id);

    let client = state.client.read().await;
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
    log::debug!(
        "Command: get_artist_tracks {} limit={:?} offset={:?}",
        artist_id,
        limit,
        offset
    );

    let client = state.client.read().await;
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
    log::debug!(
        "Command: get_artist_albums {} limit={:?} offset={:?}",
        artist_id,
        limit,
        offset
    );

    let client = state.client.read().await;
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
    blacklist_state: State<'_, BlacklistState>,
) -> Result<SearchResultsPage<Artist>, String> {
    log::info!(
        "Command: get_similar_artists {} limit={:?} offset={:?}",
        artist_id,
        limit,
        offset
    );

    let client = state.client.read().await;
    let mut results = client
        .get_similar_artists(artist_id, limit.unwrap_or(5), offset.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())?;

    // Filter out blacklisted artists
    let original_count = results.items.len();
    results
        .items
        .retain(|artist| !blacklist_state.is_blacklisted(artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[Blacklist] Filtered {} similar artists", filtered_count);
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
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

    let client = state.client.read().await;
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
    cache_state: State<'_, ApiCacheState>,
) -> Result<Vec<crate::api::models::GenreInfo>, String> {
    log::debug!("Command: get_genres parent_id={:?}", parent_id);

    // Check cache first
    {
        let guard = cache_state.cache.lock().await;
        if let Some(cache) = guard.as_ref() {
            match cache.get_genres(parent_id) {
                Ok(Some(cached_data)) => {
                    log::debug!("Cache hit for genres parent_id={:?}", parent_id);
                    return serde_json::from_str(&cached_data)
                        .map_err(|e| format!("Failed to parse cached genres: {}", e));
                }
                Ok(None) => {} // Cache miss
                Err(e) => {
                    log::warn!("Genre cache read error: {}", e);
                }
            }
        }
    }

    // Cache miss - fetch from API
    log::debug!(
        "Cache miss for genres parent_id={:?}, fetching from API",
        parent_id
    );
    let client = state.client.read().await;
    let genres = client
        .get_genres(parent_id)
        .await
        .map_err(|e| e.to_string())?;

    // Cache the result
    {
        let guard = cache_state.cache.lock().await;
        if let Some(cache) = guard.as_ref() {
            let json = serde_json::to_string(&genres)
                .map_err(|e| format!("Failed to serialize genres: {}", e))?;
            if let Err(e) = cache.set_genres(parent_id, &json) {
                log::warn!("Genre cache write error: {}", e);
            }
        }
    }

    Ok(genres)
}

/// Get discover index (home page content: playlists, ideal discography, etc.)
#[tauri::command]
pub async fn get_discover_index(
    genre_ids: Option<Vec<u64>>,
    state: State<'_, AppState>,
) -> Result<DiscoverResponse, String> {
    let result = {
        let client = state.client.read().await;
        client
            .get_discover_index(genre_ids)
            .await
            .map_err(|e| e.to_string())?
    };
    Ok(result)
}

/// Get discover playlists with optional tag and genre filters
#[tauri::command]
pub async fn get_discover_playlists(
    tag: Option<String>,
    genre_ids: Option<Vec<u64>>,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<DiscoverPlaylistsResponse, String> {
    log::debug!(
        "Command: get_discover_playlists tag={:?} genre_ids={:?} limit={:?} offset={:?}",
        tag,
        genre_ids,
        limit,
        offset
    );

    let client = state.client.read().await;
    client
        .get_discover_playlists(tag, genre_ids, limit, offset)
        .await
        .map_err(|e| e.to_string())
}

/// Get playlist tags (localized names from /playlist/getTags)
#[tauri::command]
pub async fn get_playlist_tags(state: State<'_, AppState>) -> Result<Vec<PlaylistTag>, String> {
    let client = state.client.read().await;
    client.get_playlist_tags().await.map_err(|e| e.to_string())
}

/// Get discover albums from a browse endpoint (newReleases, idealDiscography, mostStreamed)
#[tauri::command]
pub async fn get_discover_albums(
    endpoint_type: String,
    genre_ids: Option<Vec<u64>>,
    offset: Option<u32>,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<DiscoverData<DiscoverAlbum>, String> {
    let endpoint = match endpoint_type.as_str() {
        "newReleases" => paths::DISCOVER_NEW_RELEASES,
        "idealDiscography" => paths::DISCOVER_IDEAL_DISCOGRAPHY,
        "mostStreamed" => paths::DISCOVER_MOST_STREAMED,
        "qobuzissimes" => paths::DISCOVER_QOBUZISSIMS,
        "albumOfTheWeek" => paths::DISCOVER_ALBUM_OF_THE_WEEK,
        "pressAward" => paths::DISCOVER_PRESS_AWARD,
        _ => return Err(format!("Unknown discover endpoint type: {}", endpoint_type)),
    };

    log::debug!(
        "Command: get_discover_albums type={} offset={:?} limit={:?}",
        endpoint_type,
        offset,
        limit
    );

    let client = state.client.read().await;
    client
        .get_discover_albums(
            endpoint,
            genre_ids,
            offset.unwrap_or(0),
            limit.unwrap_or(50),
        )
        .await
        .map_err(|e| e.to_string())
}

/// Get artist page (aggregated: bio, top tracks, releases, similar, playlists)
#[tauri::command]
pub async fn get_artist_page(
    artist_id: u64,
    sort: Option<String>,
    state: State<'_, AppState>,
) -> Result<PageArtistResponse, String> {
    log::debug!("Command: get_artist_page {} sort={:?}", artist_id, sort);

    let client = state.client.read().await;
    client
        .get_artist_page(artist_id, sort.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Get artist releases grid (paginated by release_type)
#[tauri::command]
pub async fn get_releases_grid(
    artist_id: u64,
    release_type: String,
    limit: Option<u32>,
    offset: Option<u32>,
    sort: Option<String>,
    state: State<'_, AppState>,
) -> Result<ReleasesGridResponse, String> {
    log::debug!(
        "Command: get_releases_grid {} type={} limit={:?} offset={:?}",
        artist_id,
        release_type,
        limit,
        offset
    );

    let client = state.client.read().await;
    client
        .get_releases_grid(
            artist_id,
            &release_type,
            limit.unwrap_or(25),
            offset.unwrap_or(0),
            sort.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}
