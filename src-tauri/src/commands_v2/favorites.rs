use tauri::State;

use crate::core_bridge::CoreBridgeState;
use crate::runtime::{CommandRequirement, RuntimeError, RuntimeManagerState};

// ==================== Favorites Commands (V2) ====================

/// Get favorites (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_favorites(
    favType: String,
    limit: Option<u32>,
    offset: Option<u32>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<serde_json::Value, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 favorites
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let bridge = bridge.get().await;
    let resolved_limit = limit.unwrap_or(500);
    let resolved_offset = offset.unwrap_or(0);
    bridge
        .get_favorites(&favType, resolved_limit, resolved_offset)
        .await
        .map_err(RuntimeError::Internal)
}

/// Add item to favorites (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_add_favorite(
    favType: String,
    itemId: String,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 favorites
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] add_favorite type={} id={}", favType, itemId);
    let bridge = bridge.get().await;
    bridge
        .add_favorite(&favType, &itemId)
        .await
        .map_err(RuntimeError::Internal)
}

/// Remove item from favorites (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_remove_favorite(
    favType: String,
    itemId: String,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 favorites
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    log::info!("[V2] remove_favorite type={} id={}", favType, itemId);
    let bridge = bridge.get().await;
    bridge
        .remove_favorite(&favType, &itemId)
        .await
        .map_err(RuntimeError::Internal)
}

// ==================== Favorites Cache Commands (V2) ====================

/// Get cached favorite tracks (V2)
#[tauri::command]
pub async fn v2_get_cached_favorite_tracks(
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<Vec<i64>, RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .get_favorite_track_ids()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Sync cached favorite tracks (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_sync_cached_favorite_tracks(
    trackIds: Vec<i64>,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .sync_favorite_tracks(&trackIds)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Cache a favorite track (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_cache_favorite_track(
    trackId: i64,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .add_favorite_track(trackId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Uncache a favorite track (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_uncache_favorite_track(
    trackId: i64,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .remove_favorite_track(trackId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Bulk add tracks to favorites (V2) — adds via API then updates local cache
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_bulk_add_favorites(
    trackIds: Vec<i64>,
    bridge: State<'_, CoreBridgeState>,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;
    log::info!("[V2] bulk_add_favorites: {} tracks", trackIds.len());
    let bridge = bridge.get().await;
    // Phase 1: API calls (async — no lock held across awaits)
    for id in &trackIds {
        bridge
            .add_favorite("track", &id.to_string())
            .await
            .map_err(RuntimeError::Internal)?;
    }
    // Phase 2: cache update (sync, lock acquired and released atomically)
    {
        let guard = cache_state
            .store
            .lock()
            .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
        if let Some(store) = guard.as_ref() {
            for id in &trackIds {
                let _ = store.add_favorite_track(*id);
            }
        }
    }
    Ok(())
}

/// Clear favorites cache (V2)
#[tauri::command]
pub async fn v2_clear_favorites_cache(
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.clear_all().map_err(|e| RuntimeError::Internal(e))
}

/// Get cached favorite albums (V2)
#[tauri::command]
pub async fn v2_get_cached_favorite_albums(
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<Vec<String>, RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .get_favorite_album_ids()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Sync cached favorite albums (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_sync_cached_favorite_albums(
    albumIds: Vec<String>,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .sync_favorite_albums(&albumIds)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Cache a favorite album (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_cache_favorite_album(
    albumId: String,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .add_favorite_album(&albumId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Uncache a favorite album (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_uncache_favorite_album(
    albumId: String,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .remove_favorite_album(&albumId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get cached favorite artists (V2)
#[tauri::command]
pub async fn v2_get_cached_favorite_artists(
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<Vec<i64>, RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .get_favorite_artist_ids()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Sync cached favorite artists (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_sync_cached_favorite_artists(
    artistIds: Vec<i64>,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .sync_favorite_artists(&artistIds)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Cache a favorite artist (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_cache_favorite_artist(
    artistId: i64,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .add_favorite_artist(artistId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Uncache a favorite artist (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_uncache_favorite_artist(
    artistId: i64,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .remove_favorite_artist(artistId)
        .map_err(|e| RuntimeError::Internal(e))
}

// ============ Label favorites cache (mirrors artist) ============

/// Get cached favorite labels (V2)
#[tauri::command]
pub async fn v2_get_cached_favorite_labels(
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<Vec<i64>, RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .get_favorite_label_ids()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Sync cached favorite labels (V2) — replaces the entire cached set
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_sync_cached_favorite_labels(
    labelIds: Vec<i64>,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .sync_favorite_labels(&labelIds)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Cache a single favorite label (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_cache_favorite_label(
    labelId: i64,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .add_favorite_label(labelId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Uncache a favorite label (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_uncache_favorite_label(
    labelId: i64,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .remove_favorite_label(labelId)
        .map_err(|e| RuntimeError::Internal(e))
}

// ============ Award favorites cache (mirrors label) ============

#[tauri::command]
pub async fn v2_get_cached_favorite_awards(
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<Vec<String>, RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .get_favorite_award_ids()
        .map_err(|e| RuntimeError::Internal(e))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_sync_cached_favorite_awards(
    awardIds: Vec<String>,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .sync_favorite_awards(&awardIds)
        .map_err(|e| RuntimeError::Internal(e))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_cache_favorite_award(
    awardId: String,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .add_favorite_award(&awardId)
        .map_err(|e| RuntimeError::Internal(e))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_uncache_favorite_award(
    awardId: String,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state
        .store
        .lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .remove_favorite_award(&awardId)
        .map_err(|e| RuntimeError::Internal(e))
}
