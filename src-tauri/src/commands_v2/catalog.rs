use tauri::State;

use qbz_models::{
    Album, Artist, DiscoverAlbum, DiscoverData, DiscoverPlaylistsResponse, DiscoverResponse,
    GenreInfo, LabelDetail, LabelExploreResponse, LabelPageData, PageArtistResponse,
    PlaylistTag, SearchResultsPage, Track,
};

use crate::artist_blacklist::BlacklistState;
use crate::core_bridge::CoreBridgeState;
use crate::runtime::{CommandRequirement, RuntimeError, RuntimeManagerState};

// ==================== Extended Catalog Commands (V2) ====================

/// Get tracks batch by IDs (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_tracks_batch(
    trackIds: Vec<u64>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<Track>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_tracks_batch: {} tracks", trackIds.len());
    let bridge = bridge.get().await;
    bridge
        .get_tracks_batch(&trackIds)
        .await
        .map_err(RuntimeError::Internal)
}

/// Get genres (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_genres(
    parentId: Option<u64>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<GenreInfo>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_genres: parent={:?}", parentId);
    let bridge = bridge.get().await;
    bridge
        .get_genres(parentId)
        .await
        .map_err(RuntimeError::Internal)
}

/// Get discover index (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_discover_index(
    genreIds: Option<Vec<u64>>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<DiscoverResponse, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_discover_index: genres={:?}", genreIds);
    let bridge = bridge.get().await;
    let mut response = bridge
        .get_discover_index(genreIds)
        .await
        .map_err(RuntimeError::Internal)?;

    let mut filtered_count: usize = 0;
    let mut filter_container =
        |container: &mut Option<qbz_models::DiscoverContainer<DiscoverAlbum>>| {
            if let Some(section) = container.as_mut() {
                let before = section.data.items.len();
                section.data.items.retain(|album| {
                    !album
                        .artists
                        .iter()
                        .any(|artist| blacklist_state.is_blacklisted(artist.id))
                });
                filtered_count += before.saturating_sub(section.data.items.len());
            }
        };

    filter_container(&mut response.containers.ideal_discography);
    filter_container(&mut response.containers.new_releases);
    filter_container(&mut response.containers.qobuzissims);
    filter_container(&mut response.containers.most_streamed);
    filter_container(&mut response.containers.press_awards);
    filter_container(&mut response.containers.album_of_the_week);

    if filtered_count > 0 {
        log::debug!(
            "[V2/Blacklist] Filtered {} discover index albums from home containers",
            filtered_count
        );
    }

    Ok(response)
}

/// Get discover playlists (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_discover_playlists(
    tag: Option<String>,
    genreIds: Option<Vec<u64>>,
    limit: Option<u32>,
    offset: Option<u32>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<DiscoverPlaylistsResponse, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_discover_playlists: tag={:?}", tag);
    let bridge = bridge.get().await;
    bridge
        .get_discover_playlists(tag, genreIds, limit, offset)
        .await
        .map_err(RuntimeError::Internal)
}

/// Get playlist tags (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_playlist_tags(
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<PlaylistTag>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_playlist_tags");
    let bridge = bridge.get().await;
    bridge
        .get_playlist_tags()
        .await
        .map_err(RuntimeError::Internal)
}

/// Get discover albums from a browse endpoint (V2 - uses QbzCore)
/// Supports: newReleases, idealDiscography, mostStreamed, qobuzissimes, albumOfTheWeek, pressAward
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_discover_albums(
    endpointType: String,
    genreIds: Option<Vec<u64>>,
    offset: Option<u32>,
    limit: Option<u32>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<DiscoverData<DiscoverAlbum>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    // Map endpoint type to actual path
    let endpoint = match endpointType.as_str() {
        "newReleases" => "/discover/newReleases",
        "idealDiscography" => "/discover/idealDiscography",
        "mostStreamed" => "/discover/mostStreamed",
        "qobuzissimes" => "/discover/qobuzissims",
        "albumOfTheWeek" => "/discover/albumOfTheWeek",
        "pressAward" => "/discover/pressAward",
        _ => {
            return Err(RuntimeError::Internal(format!(
                "Unknown discover endpoint type: {}",
                endpointType
            )))
        }
    };

    log::info!("[V2] get_discover_albums: type={}", endpointType);
    let bridge = bridge.get().await;
    let mut results = bridge
        .get_discover_albums(endpoint, genreIds, offset.unwrap_or(0), limit.unwrap_or(50))
        .await
        .map_err(RuntimeError::Internal)?;

    // Filter out albums from blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|album| {
        // Check if any of the album's artists are blacklisted
        !album
            .artists
            .iter()
            .any(|artist| blacklist_state.is_blacklisted(artist.id))
    });

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!(
            "[V2/Blacklist] Filtered {} albums from discover results",
            filtered_count
        );
    }

    Ok(results)
}

/// Get featured albums (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_featured_albums(
    featuredType: String,
    limit: u32,
    offset: u32,
    genreId: Option<u64>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<SearchResultsPage<Album>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!(
        "[V2] get_featured_albums: type={}, genre={:?}",
        featuredType,
        genreId
    );
    let bridge = bridge.get().await;
    let mut results = bridge
        .get_featured_albums(&featuredType, limit, offset, genreId)
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
            "[V2/Blacklist] Filtered {} albums from featured results",
            filtered_count
        );
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

/// Get artist page (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_artist_page(
    artistId: u64,
    sort: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<PageArtistResponse, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_artist_page: {} sort={:?}", artistId, sort);
    let bridge = bridge.get().await;
    bridge
        .get_artist_page(artistId, sort.as_deref())
        .await
        .map_err(RuntimeError::Internal)
}

/// Get similar artists (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_similar_artists(
    artistId: u64,
    limit: u32,
    offset: u32,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<SearchResultsPage<Artist>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_similar_artists: {}", artistId);
    let bridge = bridge.get().await;
    let mut results = bridge
        .get_similar_artists(artistId, limit, offset)
        .await
        .map_err(RuntimeError::Internal)?;

    // Filter out blacklisted artists
    let original_count = results.items.len();
    results
        .items
        .retain(|artist| !blacklist_state.is_blacklisted(artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} similar artists", filtered_count);
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
}

/// Get artist with albums (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_artist_with_albums(
    artistId: u64,
    limit: Option<u32>,
    offset: Option<u32>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Artist, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!(
        "[V2] get_artist_with_albums: {} limit={:?} offset={:?}",
        artistId,
        limit,
        offset
    );
    let bridge = bridge.get().await;
    bridge
        .get_artist_with_albums(artistId, limit, offset)
        .await
        .map_err(RuntimeError::Internal)
}

/// Get label details (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_label(
    labelId: u64,
    limit: u32,
    offset: u32,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<LabelDetail, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_label: {}", labelId);
    let bridge = bridge.get().await;
    bridge
        .get_label(labelId, limit, offset)
        .await
        .map_err(RuntimeError::Internal)
}

/// Get label page (aggregated: top tracks, releases, playlists, artists)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_label_page(
    labelId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<LabelPageData, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_label_page: {}", labelId);
    let bridge = bridge.get().await;
    bridge
        .get_label_page(labelId)
        .await
        .map_err(RuntimeError::Internal)
}

/// Get label explore (discover more labels)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_label_explore(
    limit: u32,
    offset: u32,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<LabelExploreResponse, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_label_explore: limit={} offset={}", limit, offset);
    let bridge = bridge.get().await;
    bridge
        .get_label_explore(limit, offset)
        .await
        .map_err(RuntimeError::Internal)
}
