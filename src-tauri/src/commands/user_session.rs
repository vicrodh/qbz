//! Per-user session activation and teardown
//!
//! After login, the frontend calls `activate_user_session` with the Qobuz user_id.
//! This initializes all per-user database stores at the user-scoped directory.
//! On logout, `deactivate_user_session` tears everything down.

use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::State;

use crate::api_cache::ApiCacheState;
use crate::artist_blacklist::BlacklistState;
use crate::artist_vectors::ArtistVectorStoreState;
use crate::config::{
    audio_settings::AudioSettingsState,
    download_settings::{DownloadSettingsState, DownloadSettingsStore},
    favorites_cache::FavoritesCacheState,
    favorites_preferences::FavoritesPreferencesState,
    legal_settings::{LegalSettingsState, LegalSettingsStore},
    playback_preferences::PlaybackPreferencesState,
    remote_control_settings::{AllowedOriginsState, RemoteControlSettingsState},
    subscription_state::{SubscriptionStateState, SubscriptionStateStore},
    tray_settings::TraySettingsState,
};
use crate::library::commands::LibraryState;
use crate::listenbrainz::ListenBrainzSharedState;
use crate::lyrics::LyricsState;
use crate::musicbrainz::MusicBrainzSharedState;
use crate::offline::OfflineState;
use crate::offline_cache::OfflineCacheState;
use crate::reco_store::RecoState;
use crate::session_store::SessionStoreState;
use crate::updates::UpdatesState;
use crate::user_data::UserDataPaths;

/// Helper to init a type-alias state (Arc<Mutex<Option<Store>>>) at a path
fn init_type_alias_state<S, F>(
    state: &Arc<Mutex<Option<S>>>,
    base_dir: &Path,
    constructor: F,
) -> Result<(), String>
where
    F: FnOnce(&Path) -> Result<S, String>,
{
    let store = constructor(base_dir)?;
    let mut guard = state.lock().map_err(|e| format!("Lock error: {}", e))?;
    *guard = Some(store);
    Ok(())
}

/// Helper to teardown a type-alias state
fn teardown_type_alias_state<S>(state: &Arc<Mutex<Option<S>>>) {
    if let Ok(mut guard) = state.lock() {
        *guard = None;
    }
}

/// Activate the per-user session after login.
///
/// This runs the one-time migration (if needed) and initializes all
/// per-user database stores at `~/.local/share/qbz/users/{user_id}/`
/// and cache stores at `~/.cache/qbz/users/{user_id}/`.
#[tauri::command]
pub async fn activate_user_session(
    user_id: u64,
    user_paths: State<'_, UserDataPaths>,
    session_store: State<'_, SessionStoreState>,
    favorites_cache: State<'_, FavoritesCacheState>,
    subscription_state: State<'_, SubscriptionStateState>,
    playback_prefs: State<'_, PlaybackPreferencesState>,
    favorites_prefs: State<'_, FavoritesPreferencesState>,
    download_settings: State<'_, DownloadSettingsState>,
    audio_settings: State<'_, AudioSettingsState>,
    tray_settings: State<'_, TraySettingsState>,
    remote_control_settings: State<'_, RemoteControlSettingsState>,
    allowed_origins: State<'_, AllowedOriginsState>,
    legal_settings: State<'_, LegalSettingsState>,
    updates: State<'_, UpdatesState>,
    library: State<'_, LibraryState>,
    reco: State<'_, RecoState>,
    api_cache: State<'_, ApiCacheState>,
    artist_vectors: State<'_, ArtistVectorStoreState>,
    blacklist: State<'_, BlacklistState>,
    offline: State<'_, OfflineState>,
    offline_cache: State<'_, OfflineCacheState>,
    lyrics: State<'_, LyricsState>,
    musicbrainz: State<'_, MusicBrainzSharedState>,
    listenbrainz: State<'_, ListenBrainzSharedState>,
) -> Result<(), String> {
    log::info!("Activating user session for user_id={}", user_id);

    // Set the active user for path resolution
    user_paths.set_user(user_id);

    // Run one-time flat-to-user migration if needed
    if let Err(e) = crate::migration::migrate_flat_to_user(user_id) {
        log::error!("Migration failed: {}", e);
        // Non-fatal: user gets a fresh slate if migration fails
    }

    // Resolve user-scoped directories
    let data_dir = user_paths.user_data_dir()?;
    let cache_dir = user_paths.user_cache_dir()?;

    // Ensure directories exist
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create user data dir: {}", e))?;
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create user cache dir: {}", e))?;

    log::info!("User data dir: {}", data_dir.display());
    log::info!("User cache dir: {}", cache_dir.display());

    // Initialize all per-user states at the user directory.
    // Data-dir stores:
    session_store.init_at(&data_dir)?;
    favorites_cache.init_at(&data_dir)?;
    playback_prefs.init_at(&data_dir)?;
    favorites_prefs.init_at(&data_dir)?;
    audio_settings.init_at(&data_dir)?;
    tray_settings.init_at(&data_dir)?;
    remote_control_settings.init_at(&data_dir)?;
    allowed_origins.init_at(&data_dir)?;
    updates.init_at(&data_dir)?;
    library.init_at(&data_dir).await?;
    reco.init_at(&data_dir).await?;
    api_cache.init_at(&data_dir).await?;
    artist_vectors.init_at(&data_dir).await?;
    blacklist.init_at(&data_dir)?;
    offline.init_at(&data_dir)?;
    musicbrainz.init_at(&data_dir).await?;
    listenbrainz.init_at(&data_dir).await?;

    // Type-alias states (no init_at method — init inline)
    init_type_alias_state(&*subscription_state, &data_dir, SubscriptionStateStore::new_at)?;
    init_type_alias_state(&*download_settings, &data_dir, DownloadSettingsStore::new_at)?;
    init_type_alias_state(&*legal_settings, &data_dir, LegalSettingsStore::new_at)?;

    // Cache-dir stores:
    offline_cache.init_at(&cache_dir).await?;
    lyrics.init_at(&cache_dir).await?;

    // Run deferred subscription purge check (was removed from startup)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let should_purge = {
        let guard = subscription_state.lock().map_err(|e| format!("Lock error: {}", e))?;
        guard.as_ref()
            .and_then(|s| s.should_purge_offline_cache(now).ok())
            .unwrap_or(false)
    };

    if should_purge {
        log::warn!("Subscription invalid for >3 days. Purging offline cache.");
        if let Err(e) = crate::offline_cache::commands::purge_all_cached_files(
            offline_cache.inner(),
            library.inner(),
        ).await {
            log::error!("Failed to purge offline cache: {}", e);
        } else {
            let guard = subscription_state.lock().map_err(|e| format!("Lock error: {}", e))?;
            if let Some(store) = guard.as_ref() {
                let _ = store.mark_offline_cache_purged(now);
            }
        }
    }

    log::info!("User session activated for user_id={}", user_id);
    Ok(())
}

/// Deactivate the per-user session on logout.
///
/// Tears down all per-user stores, closing database connections.
#[tauri::command]
pub async fn deactivate_user_session(
    user_paths: State<'_, UserDataPaths>,
    session_store: State<'_, SessionStoreState>,
    favorites_cache: State<'_, FavoritesCacheState>,
    subscription_state: State<'_, SubscriptionStateState>,
    playback_prefs: State<'_, PlaybackPreferencesState>,
    favorites_prefs: State<'_, FavoritesPreferencesState>,
    download_settings: State<'_, DownloadSettingsState>,
    audio_settings: State<'_, AudioSettingsState>,
    tray_settings: State<'_, TraySettingsState>,
    remote_control_settings: State<'_, RemoteControlSettingsState>,
    allowed_origins: State<'_, AllowedOriginsState>,
    legal_settings: State<'_, LegalSettingsState>,
    updates: State<'_, UpdatesState>,
    library: State<'_, LibraryState>,
    reco: State<'_, RecoState>,
    api_cache: State<'_, ApiCacheState>,
    artist_vectors: State<'_, ArtistVectorStoreState>,
    blacklist: State<'_, BlacklistState>,
    offline: State<'_, OfflineState>,
    offline_cache: State<'_, OfflineCacheState>,
    lyrics: State<'_, LyricsState>,
    musicbrainz: State<'_, MusicBrainzSharedState>,
    listenbrainz: State<'_, ListenBrainzSharedState>,
) -> Result<(), String> {
    log::info!("Deactivating user session");

    // Teardown all per-user stores (closes DB connections)
    session_store.teardown();
    favorites_cache.teardown()?;
    playback_prefs.teardown()?;
    favorites_prefs.teardown()?;
    audio_settings.teardown()?;
    tray_settings.teardown()?;
    remote_control_settings.teardown()?;
    allowed_origins.teardown()?;
    updates.teardown();
    library.teardown().await;
    reco.teardown().await;
    api_cache.teardown().await;
    artist_vectors.teardown().await;
    blacklist.teardown();
    offline.teardown();
    offline_cache.teardown().await;
    lyrics.teardown().await;
    musicbrainz.teardown().await;
    listenbrainz.teardown().await;

    // Type-alias states
    teardown_type_alias_state(&*subscription_state);
    teardown_type_alias_state(&*download_settings);
    teardown_type_alias_state(&*legal_settings);

    // Clear the active user
    user_paths.clear_user();

    log::info!("User session deactivated");
    Ok(())
}
