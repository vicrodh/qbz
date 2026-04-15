use tauri::State;

use qbz_models::{Album, Artist, Playlist, SearchResultsPage, Track};

use crate::artist_blacklist::BlacklistState;
use crate::core_bridge::CoreBridgeState;
use crate::runtime::{CommandRequirement, RuntimeError, RuntimeManagerState};

use super::{V2MostPopularItem, V2SearchAllResults};

// ==================== Search Commands (V2) ====================

/// Search for albums (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_search_albums(
    query: String,
    limit: u32,
    offset: u32,
    searchType: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<SearchResultsPage<Album>, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 search
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let bridge = bridge.get().await;
    let mut results = bridge
        .search_albums(&query, limit, offset, searchType.as_deref())
        .await
        .map_err(RuntimeError::Internal)?;

    // Filter out albums from blacklisted artists
    let original_count = results.items.len();
    results
        .items
        .retain(|album| !blacklist_state.is_blacklisted(album.artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!(
            "[V2/Blacklist] Filtered {} albums from search results",
            filtered_count
        );
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

/// Search for tracks (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_search_tracks(
    query: String,
    limit: u32,
    offset: u32,
    searchType: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<SearchResultsPage<Track>, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 search
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let bridge = bridge.get().await;
    let mut results = bridge
        .search_tracks(&query, limit, offset, searchType.as_deref())
        .await
        .map_err(RuntimeError::Internal)?;

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
            "[V2/Blacklist] Filtered {} tracks from search results",
            filtered_count
        );
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

/// Search for artists (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_search_artists(
    query: String,
    limit: u32,
    offset: u32,
    searchType: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<SearchResultsPage<Artist>, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 search
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let bridge = bridge.get().await;
    let mut results = bridge
        .search_artists(&query, limit, offset, searchType.as_deref())
        .await
        .map_err(RuntimeError::Internal)?;

    // Filter out blacklisted artists
    let original_count = results.items.len();
    results
        .items
        .retain(|artist| !blacklist_state.is_blacklisted(artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!(
            "[V2/Blacklist] Filtered {} artists from search results",
            filtered_count
        );
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

/// Search all categories in one call (albums/tracks/artists/playlists + most_popular)
#[tauri::command]
pub async fn v2_search_all(
    query: String,
    core_bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<V2SearchAllResults, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let bridge = core_bridge.get().await;
    let response: serde_json::Value = bridge
        .catalog_search(&query, 30, 0)
        .await
        .map_err(RuntimeError::Internal)?;

    let mut albums: SearchResultsPage<Album> = response
        .get("albums")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage {
            items: vec![],
            total: 0,
            offset: 0,
            limit: 30,
        });
    let mut tracks: SearchResultsPage<Track> = response
        .get("tracks")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage {
            items: vec![],
            total: 0,
            offset: 0,
            limit: 30,
        });
    let mut artists: SearchResultsPage<Artist> = response
        .get("artists")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage {
            items: vec![],
            total: 0,
            offset: 0,
            limit: 30,
        });
    let playlists: SearchResultsPage<Playlist> = response
        .get("playlists")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage {
            items: vec![],
            total: 0,
            offset: 0,
            limit: 30,
        });

    let most_popular: Option<V2MostPopularItem> = response
        .get("most_popular")
        .and_then(|mp| mp.get("items"))
        .and_then(|items| items.as_array())
        .and_then(|arr| {
            for item in arr {
                let item_type = item.get("type").and_then(|t| t.as_str())?;
                let content = item.get("content")?;

                match item_type {
                    "tracks" => {
                        if let Ok(track) = serde_json::from_value::<Track>(content.clone()) {
                            if let Some(ref performer) = track.performer {
                                if blacklist_state.is_blacklisted(performer.id) {
                                    continue;
                                }
                            }
                            return Some(V2MostPopularItem::Tracks(track));
                        }
                    }
                    "albums" => {
                        if let Ok(album) = serde_json::from_value::<Album>(content.clone()) {
                            if blacklist_state.is_blacklisted(album.artist.id) {
                                continue;
                            }
                            return Some(V2MostPopularItem::Albums(album));
                        }
                    }
                    "artists" => {
                        if let Ok(artist) = serde_json::from_value::<Artist>(content.clone()) {
                            if blacklist_state.is_blacklisted(artist.id) {
                                continue;
                            }
                            return Some(V2MostPopularItem::Artists(artist));
                        }
                    }
                    _ => {}
                }
            }
            None
        });

    let original_album_count = albums.items.len();
    albums
        .items
        .retain(|album| !blacklist_state.is_blacklisted(album.artist.id));
    let filtered_albums = original_album_count - albums.items.len();
    if filtered_albums > 0 {
        albums.total = albums.total.saturating_sub(filtered_albums as u32);
    }

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

    let original_artist_count = artists.items.len();
    artists
        .items
        .retain(|artist| !blacklist_state.is_blacklisted(artist.id));
    let filtered_artists = original_artist_count - artists.items.len();
    if filtered_artists > 0 {
        artists.total = artists.total.saturating_sub(filtered_artists as u32);
    }

    Ok(V2SearchAllResults {
        albums,
        tracks,
        artists,
        playlists,
        most_popular,
    })
}

// ==================== Catalog Commands (V2) ====================

/// Get album by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_album(
    albumId: String,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Album, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;
    let bridge = bridge.get().await;
    bridge
        .get_album(&albumId)
        .await
        .map_err(RuntimeError::Internal)
}

/// Get track by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_track(
    trackId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Track, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;
    let bridge = bridge.get().await;
    bridge
        .get_track(trackId)
        .await
        .map_err(RuntimeError::Internal)
}

/// Get artist by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_artist(
    artistId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Artist, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;
    let bridge = bridge.get().await;
    bridge
        .get_artist(artistId)
        .await
        .map_err(RuntimeError::Internal)
}
