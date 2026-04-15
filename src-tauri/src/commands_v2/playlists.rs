use tauri::State;

use qbz_models::{Playlist, SearchResultsPage};

use crate::api::models::{PlaylistDuplicateResult, PlaylistWithTrackIds};
use crate::core_bridge::CoreBridgeState;
use crate::library::LibraryState;
use crate::runtime::{CommandRequirement, RuntimeError, RuntimeManagerState};
use crate::AppState;

// ==================== Playlist Commands (V2) ====================

/// Get user playlists (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_user_playlists(
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<Playlist>, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 playlists
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] get_user_playlists");
    let bridge = bridge.get().await;
    bridge
        .get_user_playlists()
        .await
        .map_err(RuntimeError::Internal)
}

/// Get playlist by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_playlist(
    playlistId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Playlist, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 playlists
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::debug!("[V2] get_playlist: {}", playlistId);
    let bridge = bridge.get().await;
    bridge
        .get_playlist(playlistId)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_playlist_import_preview(
    url: String,
) -> Result<crate::playlist_import::ImportPlaylist, RuntimeError> {
    crate::playlist_import::preview_public_playlist(&url)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_import_execute(
    url: String,
    nameOverride: Option<String>,
    isPublic: bool,
    app_state: State<'_, AppState>,
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<crate::playlist_import::ImportSummary, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    let client = app_state.client.read().await;
    crate::playlist_import::import_public_playlist(
        &url,
        &client,
        nameOverride.as_deref(),
        isPublic,
        &app,
    )
    .await
    .map_err(|e| RuntimeError::Internal(e.to_string()))
}

/// Get playlist metadata + track ids for progressive loading.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_playlist_track_ids(
    playlistId: u64,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<PlaylistWithTrackIds, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    let client = app_state.client.read().await;
    client
        .get_playlist_track_ids(playlistId)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

/// Check duplicates before adding tracks to a playlist (V2).
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_check_playlist_duplicates(
    playlistId: u64,
    trackIds: Vec<u64>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<PlaylistDuplicateResult, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    let client = app_state.client.read().await;
    let playlist = client
        .get_playlist_track_ids(playlistId)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))?;

    let existing_ids: std::collections::HashSet<u64> = playlist.track_ids.into_iter().collect();
    let duplicate_track_ids: std::collections::HashSet<u64> = trackIds
        .iter()
        .copied()
        .filter(|track_id| existing_ids.contains(track_id))
        .collect();

    Ok(PlaylistDuplicateResult {
        total_tracks: trackIds.len(),
        duplicate_count: duplicate_track_ids.len(),
        duplicate_track_ids,
    })
}

/// Add tracks to playlist (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_add_tracks_to_playlist(
    playlistId: u64,
    trackIds: Vec<u64>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 playlists
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!(
        "[V2] add_tracks_to_playlist: playlist {} <- {} tracks",
        playlistId,
        trackIds.len()
    );
    let bridge = bridge.get().await;
    bridge
        .add_tracks_to_playlist(playlistId, &trackIds)
        .await
        .map_err(RuntimeError::Internal)
}

/// Remove tracks from playlist (V2 - uses QbzCore)
/// Accepts either playlistTrackIds (direct) or trackIds (requires resolution)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_remove_tracks_from_playlist(
    playlistId: u64,
    playlistTrackIds: Option<Vec<u64>>,
    trackIds: Option<Vec<u64>>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 playlists
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let ptids = playlistTrackIds.unwrap_or_default();
    let tids = trackIds.unwrap_or_default();
    log::info!(
        "[V2] remove_tracks_from_playlist: playlist {} (playlistTrackIds={}, trackIds={})",
        playlistId,
        ptids.len(),
        tids.len()
    );

    let bridge = bridge.get().await;

    // If we have direct playlist_track_ids, use them
    if !ptids.is_empty() {
        return bridge
            .remove_tracks_from_playlist(playlistId, &ptids)
            .await
            .map_err(RuntimeError::Internal);
    }

    // Otherwise resolve track_ids → playlist_track_ids via full playlist fetch
    if !tids.is_empty() {
        let playlist = bridge
            .get_playlist(playlistId)
            .await
            .map_err(RuntimeError::Internal)?;

        let track_id_set: std::collections::HashSet<u64> = tids.into_iter().collect();
        let resolved_ptids: Vec<u64> = playlist
            .tracks
            .map(|tc| {
                tc.items
                    .into_iter()
                    .filter(|track| track_id_set.contains(&track.id))
                    .filter_map(|track| track.playlist_track_id)
                    .collect()
            })
            .unwrap_or_default();

        if resolved_ptids.is_empty() {
            return Err(RuntimeError::Internal(
                "Could not resolve any track IDs to playlist track IDs".to_string(),
            ));
        }

        return bridge
            .remove_tracks_from_playlist(playlistId, &resolved_ptids)
            .await
            .map_err(RuntimeError::Internal);
    }

    Err(RuntimeError::Internal(
        "Either playlistTrackIds or trackIds must be provided".to_string(),
    ))
}

// ==================== Extended Playlist Commands (V2) ====================

/// Create a new playlist (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_create_playlist(
    name: String,
    description: Option<String>,
    isPublic: bool,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Playlist, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] create_playlist: {}", name);
    let bridge = bridge.get().await;
    bridge
        .create_playlist(&name, description.as_deref(), isPublic)
        .await
        .map_err(RuntimeError::Internal)
}

/// Delete a playlist (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_delete_playlist(
    playlistId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] delete_playlist: {}", playlistId);
    let bridge = bridge.get().await;
    bridge
        .delete_playlist(playlistId)
        .await
        .map_err(RuntimeError::Internal)
}

/// Update a playlist (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_update_playlist(
    playlistId: u64,
    name: Option<String>,
    description: Option<String>,
    isPublic: Option<bool>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Playlist, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] update_playlist: {}", playlistId);
    let bridge = bridge.get().await;
    bridge
        .update_playlist(
            playlistId,
            name.as_deref(),
            description.as_deref(),
            isPublic,
        )
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_custom_order(
    playlistId: u64,
    library_state: State<'_, LibraryState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<(i64, bool, i32)>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    crate::library::playlist_get_custom_order(playlistId, library_state)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_has_custom_order(
    playlistId: u64,
    library_state: State<'_, LibraryState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<bool, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    crate::library::playlist_has_custom_order(playlistId, library_state)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_tracks_with_local_copies(
    trackIds: Vec<u64>,
    library_state: State<'_, LibraryState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<u64>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    crate::library::playlist_get_tracks_with_local_copies(trackIds, library_state)
        .await
        .map_err(RuntimeError::Internal)
}

/// Search playlists (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_search_playlists(
    query: String,
    limit: u32,
    offset: u32,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<SearchResultsPage<Playlist>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] search_playlists: {}", query);
    let bridge = bridge.get().await;
    bridge
        .search_playlists(&query, limit, offset)
        .await
        .map_err(RuntimeError::Internal)
}
