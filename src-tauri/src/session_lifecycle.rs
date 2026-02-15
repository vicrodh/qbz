//! Session Lifecycle Management
//!
//! Centralized functions for activating and deactivating user sessions.
//! These can be called from runtime_bootstrap, v2_login, v2_logout, etc.
//!
//! This module exists to solve the problem of needing session activation
//! from multiple places (runtime_bootstrap, commands, etc.) without
//! duplicating the complex state initialization logic.

use tauri::{Emitter, Manager};

use crate::runtime::{RuntimeEvent, RuntimeManagerState};
use crate::user_data::UserDataPaths;

/// Activate a user session from anywhere given an AppHandle.
///
/// This performs the full session activation:
/// 1. Sets user paths
/// 2. Runs migration
/// 3. Initializes all per-user stores
/// 4. Updates runtime state
/// 5. Emits UserSessionActivated event
///
/// Note: This is the core logic extracted from `activate_user_session` command.
/// It can be called from runtime_bootstrap, v2_login, or the command itself.
pub async fn activate_session(app: &tauri::AppHandle, user_id: u64) -> Result<(), String> {
    log::info!("[SessionLifecycle] Activating session for user_id={}", user_id);

    // Get all required states from AppHandle
    let user_paths = app.state::<UserDataPaths>();
    let session_store = app.state::<crate::session_store::SessionStoreState>();
    let favorites_cache = app.state::<crate::config::favorites_cache::FavoritesCacheState>();
    let subscription_state = app.state::<crate::config::subscription_state::SubscriptionStateState>();
    let playback_prefs = app.state::<crate::config::playback_preferences::PlaybackPreferencesState>();
    let favorites_prefs = app.state::<crate::config::favorites_preferences::FavoritesPreferencesState>();
    let download_settings = app.state::<crate::config::download_settings::DownloadSettingsState>();
    let audio_settings = app.state::<crate::config::audio_settings::AudioSettingsState>();
    let tray_settings = app.state::<crate::config::tray_settings::TraySettingsState>();
    let remote_control_settings = app.state::<crate::config::remote_control_settings::RemoteControlSettingsState>();
    let allowed_origins = app.state::<crate::config::remote_control_settings::AllowedOriginsState>();
    // NOTE: legal_settings is GLOBAL - not per-user, not initialized/torn down here
    let updates = app.state::<crate::updates::UpdatesState>();
    let library = app.state::<crate::library::commands::LibraryState>();
    let reco = app.state::<crate::reco_store::RecoState>();
    let api_cache = app.state::<crate::api_cache::ApiCacheState>();
    let artist_vectors = app.state::<crate::artist_vectors::ArtistVectorStoreState>();
    let blacklist = app.state::<crate::artist_blacklist::BlacklistState>();
    let offline = app.state::<crate::offline::OfflineState>();
    let offline_cache = app.state::<crate::offline_cache::OfflineCacheState>();
    let lyrics = app.state::<crate::lyrics::LyricsState>();
    let musicbrainz = app.state::<crate::musicbrainz::MusicBrainzSharedState>();
    let listenbrainz = app.state::<crate::listenbrainz::ListenBrainzSharedState>();
    let runtime_manager = app.state::<RuntimeManagerState>();

    // Set the active user for path resolution
    user_paths.set_user(user_id);

    // Run one-time flat-to-user migration if needed
    if let Err(e) = crate::migration::migrate_flat_to_user(user_id) {
        log::error!("[SessionLifecycle] Migration failed: {}", e);
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

    log::info!("[SessionLifecycle] User data dir: {}", data_dir.display());
    log::info!("[SessionLifecycle] User cache dir: {}", cache_dir.display());

    // Initialize all per-user states at the user directory
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

    // Type-alias states (per-user settings)
    // NOTE: LegalSettingsState is GLOBAL (not per-user) - initialized at app startup
    use crate::config::{
        subscription_state::SubscriptionStateStore,
        download_settings::DownloadSettingsStore,
    };
    crate::commands::user_session::init_type_alias_state(&*subscription_state, &data_dir, SubscriptionStateStore::new_at)?;
    crate::commands::user_session::init_type_alias_state(&*download_settings, &data_dir, DownloadSettingsStore::new_at)?;

    // Cache-dir stores
    offline_cache.init_at(&cache_dir).await?;
    lyrics.init_at(&cache_dir).await?;

    // Run deferred subscription purge check
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
        log::warn!("[SessionLifecycle] Subscription invalid for >3 days. Purging offline cache.");
        if let Err(e) = crate::offline_cache::commands::purge_all_cached_files(
            offline_cache.inner(),
            library.inner(),
        ).await {
            log::error!("[SessionLifecycle] Failed to purge offline cache: {}", e);
        } else {
            let guard = subscription_state.lock().map_err(|e| format!("Lock error: {}", e))?;
            if let Some(store) = guard.as_ref() {
                let _ = store.mark_offline_cache_purged(now);
            }
        }
    }

    // Persist last user_id for session restore on next launch
    if let Err(e) = UserDataPaths::save_last_user_id(user_id) {
        log::warn!("[SessionLifecycle] Failed to save last_user_id: {}", e);
    }

    // Start visualizer FFT thread (idempotent)
    app.state::<crate::AppState>()
        .visualizer
        .start(app.clone());

    // Start remote control API server if enabled
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::api_server::sync_server(&app_clone).await {
            log::error!("[SessionLifecycle] Remote control API init failed: {}", e);
        }
    });

    // Update runtime state to reflect session activation
    runtime_manager.manager().set_session_activated(true, user_id).await;

    // Emit event for clients
    let _ = app.emit("runtime:event", RuntimeEvent::UserSessionActivated { user_id });

    log::info!("[SessionLifecycle] Session activated for user_id={}", user_id);
    Ok(())
}

/// Deactivate the current user session.
///
/// Tears down all per-user stores and updates runtime state.
pub async fn deactivate_session(app: &tauri::AppHandle) -> Result<(), String> {
    log::info!("[SessionLifecycle] Deactivating session");

    // Get all required states
    let user_paths = app.state::<UserDataPaths>();
    let session_store = app.state::<crate::session_store::SessionStoreState>();
    let favorites_cache = app.state::<crate::config::favorites_cache::FavoritesCacheState>();
    let subscription_state = app.state::<crate::config::subscription_state::SubscriptionStateState>();
    let playback_prefs = app.state::<crate::config::playback_preferences::PlaybackPreferencesState>();
    let favorites_prefs = app.state::<crate::config::favorites_preferences::FavoritesPreferencesState>();
    let download_settings = app.state::<crate::config::download_settings::DownloadSettingsState>();
    let audio_settings = app.state::<crate::config::audio_settings::AudioSettingsState>();
    let tray_settings = app.state::<crate::config::tray_settings::TraySettingsState>();
    let remote_control_settings = app.state::<crate::config::remote_control_settings::RemoteControlSettingsState>();
    let allowed_origins = app.state::<crate::config::remote_control_settings::AllowedOriginsState>();
    // NOTE: legal_settings is GLOBAL - not per-user, not initialized/torn down here
    let updates = app.state::<crate::updates::UpdatesState>();
    let library = app.state::<crate::library::commands::LibraryState>();
    let reco = app.state::<crate::reco_store::RecoState>();
    let api_cache = app.state::<crate::api_cache::ApiCacheState>();
    let artist_vectors = app.state::<crate::artist_vectors::ArtistVectorStoreState>();
    let blacklist = app.state::<crate::artist_blacklist::BlacklistState>();
    let offline = app.state::<crate::offline::OfflineState>();
    let offline_cache = app.state::<crate::offline_cache::OfflineCacheState>();
    let lyrics = app.state::<crate::lyrics::LyricsState>();
    let musicbrainz = app.state::<crate::musicbrainz::MusicBrainzSharedState>();
    let listenbrainz = app.state::<crate::listenbrainz::ListenBrainzSharedState>();
    let runtime_manager = app.state::<RuntimeManagerState>();

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

    // Type-alias states (per-user settings)
    // NOTE: LegalSettingsState is GLOBAL (not per-user) - NOT torn down here
    crate::commands::user_session::teardown_type_alias_state(&*subscription_state);
    crate::commands::user_session::teardown_type_alias_state(&*download_settings);

    // Clear the active user and persisted last_user_id
    user_paths.clear_user();
    UserDataPaths::clear_last_user_id();

    // Update runtime state - clear BOTH auth and session
    runtime_manager.manager().set_legacy_auth(false, None).await;
    runtime_manager.manager().set_session_activated(false, 0).await;
    runtime_manager.manager().set_corebridge_auth(false).await;

    // Emit event for clients
    let _ = app.emit("runtime:event", RuntimeEvent::UserSessionDeactivated);

    log::info!("[SessionLifecycle] Session deactivated");
    Ok(())
}

/// Activate an offline-only session (no remote auth required).
///
/// This creates a minimal session for offline/local library use.
/// Uses user_id = 0 as a special "offline user" marker.
pub async fn activate_offline_session(app: &tauri::AppHandle) -> Result<(), String> {
    log::info!("[SessionLifecycle] Activating offline session");

    // For offline mode, we use a special user_id of 0
    // This is distinct from authenticated users
    const OFFLINE_USER_ID: u64 = 0;

    let user_paths = app.state::<UserDataPaths>();
    let runtime_manager = app.state::<RuntimeManagerState>();

    // Set user to offline user for path resolution
    user_paths.set_user(OFFLINE_USER_ID);

    // Resolve directories (same as normal user but at user_id=0)
    let data_dir = user_paths.user_data_dir()?;
    let cache_dir = user_paths.user_cache_dir()?;

    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create offline data dir: {}", e))?;
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create offline cache dir: {}", e))?;

    // Initialize only the stores needed for offline operation
    let library = app.state::<crate::library::commands::LibraryState>();
    let offline = app.state::<crate::offline::OfflineState>();
    let offline_cache = app.state::<crate::offline_cache::OfflineCacheState>();
    let audio_settings = app.state::<crate::config::audio_settings::AudioSettingsState>();
    let playback_prefs = app.state::<crate::config::playback_preferences::PlaybackPreferencesState>();

    library.init_at(&data_dir).await?;
    offline.init_at(&data_dir)?;
    offline_cache.init_at(&cache_dir).await?;
    audio_settings.init_at(&data_dir)?;
    playback_prefs.init_at(&data_dir)?;

    // Mark session as activated for offline use
    // Note: legacy_auth remains false, corebridge_auth remains false
    // But session_activated is true so queue commands work
    runtime_manager.manager().set_session_activated(true, OFFLINE_USER_ID).await;

    // Emit event
    let _ = app.emit("runtime:event", RuntimeEvent::UserSessionActivated { user_id: OFFLINE_USER_ID });

    log::info!("[SessionLifecycle] Offline session activated");
    Ok(())
}
