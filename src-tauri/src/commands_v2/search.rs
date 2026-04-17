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

/// Parse a `SearchResultsPage<T>` from a JSON search response, falling back to
/// an empty page (with the given `limit`) on missing or malformed data.
fn parse_results_page<T>(
    response: &serde_json::Value,
    key: &str,
    limit: u32,
) -> SearchResultsPage<T>
where
    T: serde::de::DeserializeOwned,
{
    response
        .get(key)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| SearchResultsPage {
            items: vec![],
            total: 0,
            offset: 0,
            limit,
        })
}

/// Remove entries from `page.items` that fail `keep`, adjusting `page.total`
/// by the number actually removed.
fn filter_page<T, F>(page: &mut SearchResultsPage<T>, keep: F)
where
    F: FnMut(&T) -> bool,
{
    let before = page.items.len();
    page.items.retain(keep);
    let removed = before - page.items.len();
    if removed > 0 {
        page.total = page.total.saturating_sub(removed as u32);
    }
}

/// Try to convert a single `most_popular` items-array entry into a typed
/// `V2MostPopularItem`, honoring the blacklist. Returns `None` when the
/// entry is malformed, of an unknown type, or filtered out.
fn most_popular_from_item(
    item: &serde_json::Value,
    blacklist: &BlacklistState,
) -> Option<V2MostPopularItem> {
    let item_type = item.get("type")?.as_str()?;
    let content = item.get("content")?;

    match item_type {
        "tracks" => {
            let track: Track = serde_json::from_value(content.clone()).ok()?;
            let skip = track
                .performer
                .as_ref()
                .is_some_and(|p| blacklist.is_blacklisted(p.id));
            if skip {
                return None;
            }
            Some(V2MostPopularItem::Tracks(track))
        }
        "albums" => {
            let album: Album = serde_json::from_value(content.clone()).ok()?;
            if blacklist.is_blacklisted(album.artist.id) {
                return None;
            }
            Some(V2MostPopularItem::Albums(album))
        }
        "artists" => {
            let artist: Artist = serde_json::from_value(content.clone()).ok()?;
            if blacklist.is_blacklisted(artist.id) {
                return None;
            }
            Some(V2MostPopularItem::Artists(artist))
        }
        _ => None,
    }
}

/// Walk the `most_popular.items` array and return the first entry that
/// passes the blacklist (or `None` if every entry was skipped / absent).
fn pick_most_popular(
    response: &serde_json::Value,
    blacklist: &BlacklistState,
) -> Option<V2MostPopularItem> {
    response
        .get("most_popular")?
        .get("items")?
        .as_array()?
        .iter()
        .find_map(|item| most_popular_from_item(item, blacklist))
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

    let mut albums: SearchResultsPage<Album> = parse_results_page(&response, "albums", 30);
    let mut tracks: SearchResultsPage<Track> = parse_results_page(&response, "tracks", 30);
    let mut artists: SearchResultsPage<Artist> = parse_results_page(&response, "artists", 30);
    let playlists: SearchResultsPage<Playlist> = parse_results_page(&response, "playlists", 30);

    let most_popular = pick_most_popular(&response, &blacklist_state);

    filter_page(&mut albums, |a| {
        !blacklist_state.is_blacklisted(a.artist.id)
    });
    filter_page(&mut tracks, |t| {
        t.performer
            .as_ref()
            .is_none_or(|p| !blacklist_state.is_blacklisted(p.id))
    });
    filter_page(&mut artists, |a| !blacklist_state.is_blacklisted(a.id));

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
