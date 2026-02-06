//! Cache management commands

use tauri::State;

use crate::api_cache::ApiCacheState;
use crate::cache::CacheStats;
use crate::AppState;

/// Get cache statistics
#[tauri::command]
pub fn get_cache_stats(state: State<'_, AppState>) -> CacheStats {
    log::info!("Command: get_cache_stats");
    state.audio_cache.stats()
}

/// Clear the audio cache
#[tauri::command]
pub fn clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: clear_cache");
    state.audio_cache.clear();
    Ok(())
}

/// Clear all cached artists (useful when changing language settings)
#[tauri::command]
pub async fn clear_artist_cache(cache_state: State<'_, ApiCacheState>) -> Result<usize, String> {
    log::info!("Command: clear_artist_cache");
    let guard = cache_state.cache.lock().await;
    let cache = guard.as_ref().ok_or("No active session - please log in")?;
    cache.clear_all_artists()
}
