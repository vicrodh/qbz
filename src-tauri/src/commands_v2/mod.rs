//! V2 Commands - Using the new multi-crate architecture
//!
//! These commands use QbzCore via CoreBridge instead of the old AppState.
//! Runtime contract ensures proper lifecycle (see ADR_RUNTIME_SESSION_CONTRACT.md).
//!
//! Playback flows through CoreBridge -> QbzCore -> Player (qbz-player crate).

use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tokio::sync::RwLock;

use qbz_models::{
    Album, Artist, DiscoverAlbum, DiscoverData, DiscoverPlaylistsResponse, DiscoverResponse,
    GenreInfo, LabelDetail, LabelExploreResponse, LabelPageData, PageArtistResponse, Playlist,
    PlaylistTag, Quality, QueueState, QueueTrack as CoreQueueTrack, RepeatMode, SearchResultsPage,
    Track, UserSession,
};
use qconnect_app::QueueCommandType;
use qconnect_app::{QConnectQueueState, QConnectRendererState};

use crate::api::models::{
    PlaylistDuplicateResult, PlaylistWithTrackIds,
};
use crate::artist_blacklist::BlacklistState;
use crate::audio::{AlsaPlugin, AudioBackendType, AudioDevice, BackendManager};
use crate::cache::{AudioCache, CacheStats};
use crate::config::audio_settings::{AudioSettings, AudioSettingsState};
use crate::config::developer_settings::DeveloperSettingsState;
use crate::config::favorites_preferences::FavoritesPreferences;
use crate::config::graphics_settings::GraphicsSettingsState;
use crate::config::legal_settings::LegalSettingsState;
use crate::config::playback_preferences::{
    AutoplayMode, PlaybackPreferences, PlaybackPreferencesState,
};
use crate::config::tray_settings::TraySettings;
use crate::config::tray_settings::TraySettingsState;
use crate::config::window_settings::WindowSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::library::LibraryState;
use crate::offline_cache::OfflineCacheState;
use crate::qconnect_service::{QconnectServiceState, QconnectVisibleQueueProjection};
use crate::reco_store::RecoState;
use crate::runtime::{
    CommandRequirement, DegradedReason, RuntimeError, RuntimeEvent, RuntimeManagerState,
    RuntimeStatus,
};
use crate::AppState;
use crate::integrations_v2::MusicBrainzV2State;
use std::collections::HashSet;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct V2SuggestionArtistInput {
    pub name: String,
    pub qobuz_id: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct V2PlaylistSuggestionsInput {
    pub artists: Vec<V2SuggestionArtistInput>,
    pub exclude_track_ids: Vec<u64>,
    #[serde(default)]
    pub include_reasons: bool,
    pub config: Option<crate::artist_vectors::SuggestionConfig>,
}

mod helpers;
pub use helpers::*;

// ==================== Runtime Contract Commands ====================

/// Get current runtime status
/// Use this to check if the runtime is ready before calling other commands
#[tauri::command]
pub async fn runtime_get_status(
    runtime: State<'_, RuntimeManagerState>,
) -> Result<RuntimeStatus, RuntimeError> {
    Ok(runtime.manager().get_status().await)
}

/// Bootstrap the runtime - single entrypoint for initialization
///
/// This command:
/// 1. Initializes the API client (extracts bundle tokens)
/// 2. Checks for saved credentials and auto-logs in if available
/// 3. Activates per-user session if user is logged in
/// 4. Authenticates CoreBridge/V2
///
/// Returns RuntimeStatus with full state information.
/// Clients should call this once at startup and react to the returned state.
#[tauri::command]
pub async fn runtime_bootstrap(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
    app_state: State<'_, AppState>,
    core_bridge: State<'_, CoreBridgeState>,
    audio_settings: State<'_, AudioSettingsState>,
) -> Result<RuntimeStatus, RuntimeError> {
    let manager = runtime.manager();

    // Check if bootstrap is already in progress
    if manager.is_bootstrap_in_progress().await {
        return Err(RuntimeError::BootstrapInProgress);
    }
    manager.set_bootstrap_in_progress(true).await;

    log::info!("[Runtime] Bootstrap starting...");

    // Step 1: Initialize API client (bundle tokens)
    {
        let client = app_state.client.read().await;
        match client.init().await {
            Ok(_) => {
                log::info!("[Runtime] API client initialized (bundle tokens extracted)");
                manager.set_client_initialized(true).await;
                let _ = app.emit("runtime:event", RuntimeEvent::RuntimeInitialized);
            }
            Err(e) => {
                let reason = DegradedReason::BundleExtractionFailed(e.to_string());
                log::error!("[Runtime] Bundle extraction failed: {}", e);
                manager.set_degraded(reason.clone()).await;
                manager.set_bootstrap_in_progress(false).await;
                let _ = app.emit(
                    "runtime:event",
                    RuntimeEvent::RuntimeDegraded {
                        reason: reason.clone(),
                    },
                );
                return Err(RuntimeError::RuntimeDegraded(reason));
            }
        }
    }

    // Step 2: Check ToS acceptance. Fail open if the DB is unavailable
    // (e.g. after a factory reset that deleted legal_settings.db but left
    // credentials intact). ToS is now stored AFTER a successful login, so
    // the very first bootstrap after factory reset will not have it yet.
    let tos_accepted: bool = {
        let legal_state = app.state::<crate::config::legal_settings::LegalSettingsState>();
        let guard = legal_state.lock();
        match guard {
            Ok(ref g) => {
                if let Some(store) = g.as_ref() {
                    store
                        .get_settings()
                        .map(|s| s.qobuz_tos_accepted)
                        .unwrap_or(true) // fail open when DB read fails
                } else {
                    true // fail open when store is torn down (e.g. factory reset)
                }
            }
            Err(_) => true, // fail open on lock error
        }
    };

    if !tos_accepted {
        log::info!("[Runtime] ToS not accepted, skipping auto-login. User must accept ToS first.");
        manager.set_bootstrap_in_progress(false).await;
        let status = manager.get_status().await;
        log::info!(
            "[Runtime] Bootstrap complete (ToS gate): {:?}",
            status.state
        );
        return Ok(status);
    }

    // Step 3: Attempt auto-login via saved OAuth token.
    // Basic auth (email/password) is no longer supported by Qobuz — any saved
    // credentials from older versions are ignored. Users will see the login
    // screen and authenticate via OAuth.
    if let Ok(Some(oauth_token)) = crate::credentials::load_oauth_token() {
        // No email/password credentials but there IS a saved OAuth token.
        // Try to restore the session by calling /user/login with the stored token.
        log::info!("[Runtime] No email/password credentials, trying saved OAuth token...");

        let client = app_state.client.read().await;
        match client.login_with_token(&oauth_token).await {
            Ok(session) => {
                log::info!("[Runtime] OAuth token re-auth successful");
                manager.set_legacy_auth(true, Some(session.user_id)).await;
                let _ = app.emit(
                    "runtime:event",
                    RuntimeEvent::AuthChanged {
                        logged_in: true,
                        user_id: Some(session.user_id),
                    },
                );

                // Wait for CoreBridge, then inject session
                let cb_start = std::time::Instant::now();
                let cb_timeout = std::time::Duration::from_secs(30);
                loop {
                    if core_bridge.try_get().await.is_some() {
                        break;
                    }
                    if cb_start.elapsed() > cb_timeout {
                        log::error!("[Runtime] CoreBridge not available after 30s (OAuth restore)");
                        manager.set_bootstrap_in_progress(false).await;
                        return Err(RuntimeError::V2NotInitialized);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }

                if let Some(bridge) = core_bridge.try_get().await {
                    let core_session: qbz_models::UserSession =
                        match serde_json::to_value(&session).and_then(serde_json::from_value) {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("[Runtime] OAuth session conversion failed: {}", e);
                                manager.set_bootstrap_in_progress(false).await;
                                return Err(RuntimeError::Internal(e.to_string()));
                            }
                        };
                    match bridge.login_with_session(core_session).await {
                        Ok(_) => {
                            log::info!("[Runtime] CoreBridge session restored via OAuth token");
                            manager.set_corebridge_auth(true).await;
                        }
                        Err(e) => {
                            log::error!("[Runtime] CoreBridge OAuth restore failed: {}", e);
                            manager.set_bootstrap_in_progress(false).await;
                            return Err(RuntimeError::V2AuthFailed(e));
                        }
                    }
                }

                if let Err(e) =
                    crate::session_lifecycle::activate_session(&app, session.user_id).await
                {
                    log::error!("[Runtime] Session activation failed (OAuth restore): {}", e);
                    manager.set_legacy_auth(false, None).await;
                    manager.set_corebridge_auth(false).await;
                    let reason = DegradedReason::SessionActivationFailed(e.clone());
                    manager.set_degraded(reason.clone()).await;
                    manager.set_bootstrap_in_progress(false).await;
                    let _ = app.emit(
                        "runtime:event",
                        RuntimeEvent::RuntimeDegraded {
                            reason: reason.clone(),
                        },
                    );
                    return Err(RuntimeError::RuntimeDegraded(reason));
                }

                // Sync audio settings after OAuth session activation.
                // Same logic as email/password path — without this, backend_type
                // stays None and the player falls back to legacy CPAL path.
                let should_sync = audio_settings
                    .store
                    .lock()
                    .ok()
                    .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()))
                    .map(|s| s.sync_audio_on_startup)
                    .unwrap_or(false);

                if should_sync {
                    if let Some(bridge) = core_bridge.try_get().await {
                        let player = bridge.player();
                        if let Ok(guard) = audio_settings.store.lock() {
                            if let Some(store) = guard.as_ref() {
                                if let Ok(fresh) = store.get_settings() {
                                    log::info!("[Runtime] Syncing audio settings to player after OAuth login (sync_audio_on_startup=true)");
                                    let _ = player
                                        .reload_settings(convert_to_qbz_audio_settings(&fresh));
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                // Token expired — clear it and let user re-login via OAuth
                log::warn!("[Runtime] OAuth token expired, clearing: {}", e);
                let _ = crate::credentials::clear_oauth_token();
            }
        }
    } else {
        log::info!("[Runtime] No saved credentials, staying in InitializedNoAuth");
    }

    manager.set_bootstrap_in_progress(false).await;
    let status = manager.get_status().await;
    log::info!("[Runtime] Bootstrap complete: {:?}", status.state);

    Ok(status)
}

/// Initialize the API client only (Phase 1 of runtime initialization)
///
/// This command:
/// 1. Extracts bundle tokens from Qobuz
/// 2. Sets RuntimeManager state to InitializedNoAuth
///
/// Call this first. ToS gate is enforced in backend by all login commands.
#[tauri::command]
pub async fn v2_init_client(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
    app_state: State<'_, AppState>,
) -> Result<RuntimeStatus, RuntimeError> {
    let manager = runtime.manager();

    log::info!("[V2] v2_init_client starting...");

    // Initialize API client (bundle tokens)
    {
        let client = app_state.client.read().await;
        match client.init().await {
            Ok(_) => {
                log::info!("[V2] API client initialized (bundle tokens extracted)");
                manager.set_client_initialized(true).await;
                let _ = app.emit("runtime:event", RuntimeEvent::RuntimeInitialized);
            }
            Err(e) => {
                let reason = DegradedReason::BundleExtractionFailed(e.to_string());
                log::error!("[V2] Bundle extraction failed: {}", e);
                manager.set_degraded(reason.clone()).await;
                let _ = app.emit(
                    "runtime:event",
                    RuntimeEvent::RuntimeDegraded {
                        reason: reason.clone(),
                    },
                );
                return Err(RuntimeError::RuntimeDegraded(reason));
            }
        }
    }

    Ok(manager.get_status().await)
}

/// Shared cancel signal for system browser OAuth flow.
/// Managed as Tauri state so the frontend can cancel the wait.
pub struct OAuthCancelState {
    pub cancel: Arc<tokio::sync::Notify>,
}

impl OAuthCancelState {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

/// Auto-login response matching legacy LoginResponse
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct V2LoginResponse {
    pub success: bool,
    pub user_name: Option<String>,
    pub user_id: Option<u64>,
    pub subscription: Option<String>,
    pub subscription_valid_until: Option<String>,
    pub error: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct V2UserInfo {
    pub user_name: String,
    pub subscription: String,
    pub subscription_valid_until: Option<String>,
}

#[tauri::command]
pub async fn v2_get_user_info(
    app_state: State<'_, AppState>,
) -> Result<Option<V2UserInfo>, RuntimeError> {
    let client = app_state.client.read().await;
    Ok(client
        .get_user_info()
        .await
        .map(|(name, subscription, valid_until)| V2UserInfo {
            user_name: name,
            subscription,
            subscription_valid_until: valid_until,
        }))
}

/// Auto-login using saved credentials.
///
/// Basic auth (email/password) is no longer supported by Qobuz.
/// This command now always returns no_credentials. Session restore
/// is handled by the OAuth token path in runtime_bootstrap.
#[tauri::command]
pub async fn v2_auto_login(
    _app: tauri::AppHandle,
    _runtime: State<'_, RuntimeManagerState>,
    _app_state: State<'_, AppState>,
    _core_bridge: State<'_, CoreBridgeState>,
    _legal_state: State<'_, LegalSettingsState>,
) -> Result<V2LoginResponse, String> {
    log::info!("[V2] v2_auto_login: basic auth no longer supported, returning no_credentials");
    Ok(V2LoginResponse {
        success: false,
        user_name: None,
        user_id: None,
        subscription: None,
        subscription_valid_until: None,
        error: Some("Basic auth no longer supported. Please use OAuth.".to_string()),
        error_code: Some("no_credentials".to_string()),
    })
}

/// Manual login with email and password.
///
/// Basic auth is no longer supported by Qobuz. This command is kept for
/// backward compatibility but always returns an error directing users to OAuth.
#[tauri::command]
pub async fn v2_manual_login(
    _email: String,
    _password: String,
    _app: tauri::AppHandle,
    _runtime: State<'_, RuntimeManagerState>,
    _app_state: State<'_, AppState>,
    _core_bridge: State<'_, CoreBridgeState>,
    _legal_state: State<'_, LegalSettingsState>,
) -> Result<V2LoginResponse, String> {
    log::info!("[V2] v2_manual_login: basic auth no longer supported");
    Ok(V2LoginResponse {
        success: false,
        user_name: None,
        user_id: None,
        subscription: None,
        subscription_valid_until: None,
        error: Some("Basic auth no longer supported. Please use OAuth.".to_string()),
        error_code: Some("basic_auth_deprecated".to_string()),
    })
}

/// OAuth login via embedded browser window.
///
/// Opens a Qobuz sign-in page in a new WebView window.  When Qobuz redirects
/// to play.qobuz.com/discover?code=..., the code is intercepted and exchanged
/// for a full user session without loading that page.
///
/// Both the legacy client and CoreBridge are authenticated so all subsystems
/// work identically to a normal email/password login.
///
/// Returns the same V2LoginResponse as v2_manual_login.
#[tauri::command]
pub async fn v2_start_oauth_login(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
    app_state: State<'_, AppState>,
    core_bridge: State<'_, CoreBridgeState>,
    legal_state: State<'_, LegalSettingsState>,
) -> Result<V2LoginResponse, String> {
    log::info!("[V2] v2_start_oauth_login starting...");

    // Get app_id from initialized client
    let app_id = {
        let client = app_state.client.read().await;
        client.app_id().await.map_err(|e| e.to_string())?
    };

    // Redirect URL that Qobuz will send the code to (play.qobuz.com/discover)
    let redirect_url = "https://play.qobuz.com/discover";
    let oauth_url = format!(
        "https://www.qobuz.com/signin/oauth?ext_app_id={}&redirect_url={}",
        app_id,
        urlencoding::encode(redirect_url),
    );

    // Shared state: code captured by the navigation callback
    let code_holder: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    let notify = Arc::new(tokio::sync::Notify::new());

    let code_holder_nav = Arc::clone(&code_holder);
    let notify_nav = Arc::clone(&notify);
    let app_for_nav = app.clone();

    // Clones for the on_new_window handler (Google/Apple/Facebook OAuth popups)
    let code_holder_popup = Arc::clone(&code_holder);
    let notify_popup = Arc::clone(&notify);
    let app_for_popup = app.clone();
    let popup_counter = Arc::new(std::sync::atomic::AtomicU32::new(0));

    // Build and open the OAuth WebView window
    let parsed_url: tauri::Url = oauth_url
        .parse()
        .map_err(|e| format!("Invalid OAuth URL: {}", e))?;

    let _oauth_window = tauri::WebviewWindowBuilder::new(
        &app,
        "qobuz-oauth",
        tauri::WebviewUrl::External(parsed_url),
    )
    .title("Qobuz Login")
    .inner_size(520.0, 720.0)
    .resizable(true)
    .on_navigation(move |url| {
        log::info!("[OAuth] on_navigation: {}", url);
        // Intercept redirect to play.qobuz.com/discover?code_autorisation=...
        if url.host_str() == Some("play.qobuz.com") {
            for (key, value) in url.query_pairs() {
                if key == "code_autorisation" || key == "code" {
                    let mut holder = code_holder_nav.lock().unwrap();
                    // Only act on the first code (idempotent)
                    if holder.is_none() {
                        log::info!("[OAuth] Intercepted OAuth code from navigation");
                        *holder = Some(value.to_string());
                        drop(holder);
                        notify_nav.notify_one();
                        // Close from within the navigation callback — more reliable on WebKitGTK
                        if let Some(win) = app_for_nav.get_webview_window("qobuz-oauth") {
                            let _ = win.close();
                            log::info!("[OAuth] OAuth window closed from navigation callback");
                        }
                    }
                    break;
                }
            }
        }
        true // Always allow navigation — never block (blocking is unreliable on WebKitGTK)
    })
    .on_new_window(move |url, features| {
        // Handle window.open() calls from Google/Apple/Facebook OAuth flows.
        // On Linux, .window_features(features) sets the required related_view automatically.
        let popup_id = popup_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let label = format!("qobuz-oauth-popup-{}", popup_id);
        log::info!(
            "[OAuth] New popup window requested: {} (label={})",
            url,
            label
        );

        let code_holder_p = Arc::clone(&code_holder_popup);
        let notify_p = Arc::clone(&notify_popup);
        let app_p = app_for_popup.clone();
        let label_p = label.clone();

        let builder = tauri::WebviewWindowBuilder::new(
            &app_for_popup,
            &label,
            tauri::WebviewUrl::External(url),
        )
        .window_features(features) // sets related_view on Linux, webview config on macOS
        .title("Qobuz Login")
        .inner_size(520.0, 720.0)
        .resizable(true)
        .on_navigation(move |popup_url| {
            log::info!("[OAuth] popup({}) on_navigation: {}", label_p, popup_url);
            if popup_url.host_str() == Some("play.qobuz.com") {
                for (key, value) in popup_url.query_pairs() {
                    if key == "code_autorisation" || key == "code" {
                        let mut holder = code_holder_p.lock().unwrap();
                        if holder.is_none() {
                            log::info!("[OAuth] Code captured from popup navigation");
                            *holder = Some(value.to_string());
                            drop(holder);
                            notify_p.notify_one();
                            // Close main OAuth window and this popup
                            for win_label in ["qobuz-oauth", label_p.as_str()] {
                                if let Some(win) = app_p.get_webview_window(win_label) {
                                    let _ = win.close();
                                }
                            }
                        }
                        break;
                    }
                }
            }
            true
        });

        match builder.build() {
            Ok(window) => tauri::webview::NewWindowResponse::Create { window },
            Err(e) => {
                log::error!("[OAuth] Failed to create popup window: {}", e);
                tauri::webview::NewWindowResponse::Deny
            }
        }
    })
    .build()
    .map_err(|e| format!("Failed to open OAuth window: {}", e))?;

    // Detect when user closes the OAuth window manually
    _oauth_window.on_window_event({
        let notify_close = Arc::clone(&notify);
        move |event| {
            if let tauri::WindowEvent::Destroyed = event {
                log::info!("[OAuth] WebView window destroyed, unblocking wait");
                notify_close.notify_one();
            }
        }
    });

    // Wait up to 5 minutes for the user to complete login (or window close)
    let timed_out = tokio::time::timeout(std::time::Duration::from_secs(300), notify.notified())
        .await
        .is_err();

    // Best-effort close of main window and any open popups
    for win in app.webview_windows().values() {
        let label = win.label().to_string();
        if label == "qobuz-oauth" || label.starts_with("qobuz-oauth-popup-") {
            let _ = win.close();
        }
    }

    if timed_out {
        return Ok(V2LoginResponse {
            success: false,
            user_name: None,
            user_id: None,
            subscription: None,
            subscription_valid_until: None,
            error: Some("OAuth login timed out after 5 minutes".to_string()),
            error_code: Some("oauth_timeout".to_string()),
        });
    }

    // Extract code from shared state
    let code = code_holder.lock().unwrap().clone();
    let code = match code {
        Some(c) => c,
        None => {
            return Ok(V2LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some("OAuth login cancelled".to_string()),
                error_code: Some("oauth_cancelled".to_string()),
            });
        }
    };

    finalize_oauth_login(&code, &app, &runtime, &app_state, &core_bridge, &legal_state).await
}

/// Complete the OAuth login flow after obtaining an authorization code.
///
/// Cancel the system browser OAuth flow.
/// Triggers the cancel signal so v2_start_system_browser_oauth returns immediately.
#[tauri::command]
pub async fn v2_cancel_system_browser_oauth(
    cancel_state: State<'_, OAuthCancelState>,
) -> Result<(), String> {
    log::info!("[OAuth] System browser OAuth cancelled by user");
    cancel_state.cancel.notify_one();
    Ok(())
}

/// Cancel the WebView OAuth flow by closing all OAuth windows.
#[tauri::command]
pub async fn v2_cancel_oauth_login(app: tauri::AppHandle) -> Result<(), String> {
    log::info!("[OAuth] WebView OAuth cancelled by user");
    for win in app.webview_windows().values() {
        let label = win.label().to_string();
        if label == "qobuz-oauth" || label.starts_with("qobuz-oauth-popup-") {
            let _ = win.close();
        }
    }
    Ok(())
}

/// Frontend-agnostic: does not depend on how the code was obtained (WebView,
/// system browser, manual paste, etc.). Suitable for GUI, TUI, and headless.
///
/// Steps: exchange code → establish session → inject into CoreBridge →
/// activate per-user data → persist token.
async fn finalize_oauth_login(
    code: &str,
    app: &tauri::AppHandle,
    runtime: &State<'_, RuntimeManagerState>,
    app_state: &State<'_, AppState>,
    core_bridge: &State<'_, CoreBridgeState>,
    legal_state: &State<'_, LegalSettingsState>,
) -> Result<V2LoginResponse, String> {
    let manager = runtime.manager();
    log::info!("[OAuth] Exchanging authorization code for session...");

    // Exchange code for UserSession
    let session = {
        let client = app_state.client.read().await;
        match client.login_with_oauth_code(code).await {
            Ok(s) => s,
            Err(e) => {
                return Ok(V2LoginResponse {
                    success: false,
                    user_name: None,
                    user_id: None,
                    subscription: None,
                    subscription_valid_until: None,
                    error: Some(e.to_string()),
                    error_code: Some("oauth_exchange_failed".to_string()),
                });
            }
        }
    };

    log::info!("[OAuth] Session established, activating...");
    manager.set_legacy_auth(true, Some(session.user_id)).await;
    let _ = app.emit(
        "runtime:event",
        RuntimeEvent::AuthChanged {
            logged_in: true,
            user_id: Some(session.user_id),
        },
    );

    // Convert api::models::UserSession → qbz_models::UserSession for CoreBridge
    let core_session: UserSession =
        match serde_json::to_value(&session).and_then(serde_json::from_value) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[OAuth] Failed to convert session for CoreBridge: {}", e);
                rollback_auth_state(&manager, app).await;
                return Ok(V2LoginResponse {
                    success: false,
                    user_name: None,
                    user_id: None,
                    subscription: None,
                    subscription_valid_until: None,
                    error: Some(format!("Session conversion error: {}", e)),
                    error_code: Some("internal_error".to_string()),
                });
            }
        };

    // Inject session into CoreBridge
    if let Some(bridge) = core_bridge.try_get().await {
        match bridge.login_with_session(core_session).await {
            Ok(_) => {
                log::info!("[OAuth] CoreBridge session injected");
                manager.set_corebridge_auth(true).await;
            }
            Err(e) => {
                log::error!("[OAuth] CoreBridge session injection failed: {}", e);
                rollback_auth_state(&manager, app).await;
                let _ = app.emit(
                    "runtime:event",
                    RuntimeEvent::CoreBridgeAuthFailed {
                        error: e.to_string(),
                    },
                );
                return Ok(V2LoginResponse {
                    success: false,
                    user_name: Some(session.display_name),
                    user_id: Some(session.user_id),
                    subscription: Some(session.subscription_label),
                    subscription_valid_until: session.subscription_valid_until,
                    error: Some(format!("V2 authentication failed: {}", e)),
                    error_code: Some("v2_auth_failed".to_string()),
                });
            }
        }
    } else {
        log::error!("[OAuth] CoreBridge not initialized");
        rollback_auth_state(&manager, app).await;
        return Ok(V2LoginResponse {
            success: false,
            user_name: Some(session.display_name),
            user_id: Some(session.user_id),
            subscription: Some(session.subscription_label),
            subscription_valid_until: session.subscription_valid_until,
            error: Some("V2 CoreBridge not initialized".to_string()),
            error_code: Some("v2_not_initialized".to_string()),
        });
    }

    // Activate per-user session
    if let Err(e) = crate::session_lifecycle::activate_session(app, session.user_id).await {
        log::error!("[OAuth] Session activation failed: {}", e);
        rollback_auth_state(&manager, app).await;
        return Ok(V2LoginResponse {
            success: false,
            user_name: Some(session.display_name.clone()),
            user_id: Some(session.user_id),
            subscription: Some(session.subscription_label.clone()),
            subscription_valid_until: session.subscription_valid_until.clone(),
            error: Some(format!("Session activation failed: {}", e)),
            error_code: Some("session_activation_failed".to_string()),
        });
    }

    // Persist OAuth token for session restore on next launch
    if let Err(e) = crate::credentials::save_oauth_token(&session.user_auth_token) {
        log::warn!("[OAuth] Failed to persist token: {}", e);
    }

    accept_tos_best_effort(legal_state);

    let _ = app.emit(
        "runtime:event",
        RuntimeEvent::RuntimeReady {
            user_id: session.user_id,
        },
    );

    Ok(V2LoginResponse {
        success: true,
        user_name: Some(session.display_name),
        user_id: Some(session.user_id),
        subscription: Some(session.subscription_label),
        subscription_valid_until: session.subscription_valid_until,
        error: None,
        error_code: None,
    })
}

/// OAuth login via the user's system browser.
///
/// Spawns a temporary local HTTP server, opens the system browser to the Qobuz
/// OAuth page with a localhost redirect, captures the authorization code when
/// Qobuz redirects back, then completes the login via `finalize_oauth_login`.
#[tauri::command]
pub async fn v2_start_system_browser_oauth(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
    app_state: State<'_, AppState>,
    core_bridge: State<'_, CoreBridgeState>,
    legal_state: State<'_, LegalSettingsState>,
    cancel_state: State<'_, OAuthCancelState>,
) -> Result<V2LoginResponse, String> {
    log::info!("[V2] System browser OAuth starting...");

    // Get app_id from initialized client
    let app_id = {
        let client = app_state.client.read().await;
        client.app_id().await.map_err(|e| e.to_string())?
    };

    // Bind to a random available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind local OAuth listener: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get listener address: {}", e))?
        .port();

    let oauth_url = format!(
        "https://www.qobuz.com/signin/oauth?ext_app_id={}&redirect_url={}",
        app_id,
        urlencoding::encode(&format!("http://localhost:{}", port)),
    );

    // Channel for the authorization code
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);

    // Local HTTP handler: serves a success page and forwards the code
    let oauth_handler = axum::Router::new().route(
        "/",
        axum::routing::get(move |axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>| {
            let tx = tx.clone();
            async move {
                if let Some(code) = params.get("code_autorisation").or_else(|| params.get("code")) {
                    let _ = tx.send(code.clone()).await;
                    axum::response::Html(
                        "<html><body style=\"font-family:system-ui;text-align:center;padding:60px\">\
                         <h2>Login successful</h2>\
                         <p>You can close this tab and return to QBZ.</p>\
                         </body></html>"
                    )
                } else {
                    axum::response::Html(
                        "<html><body style=\"font-family:system-ui;text-align:center;padding:60px\">\
                         <h2>Login failed</h2>\
                         <p>No authorization code received. Please try again.</p>\
                         </body></html>"
                    )
                }
            }
        }),
    );

    // Spawn the server in the background
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, oauth_handler).await.ok();
    });

    // Open the user's default browser
    log::info!("[OAuth] Opening system browser to Qobuz login (port {})", port);
    if let Err(e) = open::that(&oauth_url) {
        log::error!("[OAuth] Failed to open system browser: {}", e);
        server_handle.abort();
        return Ok(V2LoginResponse {
            success: false,
            user_name: None,
            user_id: None,
            subscription: None,
            subscription_valid_until: None,
            error: Some(format!("Failed to open browser: {}", e)),
            error_code: Some("browser_open_failed".to_string()),
        });
    }

    // Race: code received vs timeout vs user cancel
    let cancel = cancel_state.cancel.clone();
    enum OAuthResult {
        Code(Option<String>),
        Timeout,
        Cancelled,
    }
    let result = tokio::select! {
        code = rx.recv() => OAuthResult::Code(code),
        _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => OAuthResult::Timeout,
        _ = cancel.notified() => OAuthResult::Cancelled,
    };

    server_handle.abort();

    let code = match result {
        OAuthResult::Code(Some(c)) => c,
        OAuthResult::Code(None) | OAuthResult::Cancelled => {
            log::info!("[OAuth] System browser login cancelled");
            return Ok(V2LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some("OAuth login cancelled".to_string()),
                error_code: Some("oauth_cancelled".to_string()),
            });
        }
        OAuthResult::Timeout => {
            log::info!("[OAuth] System browser login timed out");
            return Ok(V2LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some("OAuth login timed out after 2 minutes".to_string()),
                error_code: Some("oauth_timeout".to_string()),
            });
        }
    };

    finalize_oauth_login(&code, &app, &runtime, &app_state, &core_bridge, &legal_state).await
}

// ==================== Prefetch (V2) ====================

/// Number of Qobuz tracks to prefetch (not total tracks, just Qobuz).
/// Higher values keep more upcoming tracks in cache for instant playback.
/// CMAF segment downloads are ~24s per Hi-Res track, so 5 tracks means
/// the cache stays ~2 minutes ahead of playback.
const V2_PREFETCH_COUNT: usize = 5;

/// How far ahead to look for tracks to prefetch (to handle mixed playlists
/// with local/offline tracks interspersed with Qobuz tracks)
const V2_PREFETCH_LOOKAHEAD: usize = 15;

/// Maximum concurrent prefetch downloads (track-level, not segment-level).
/// 2 tracks downloading simultaneously keeps the cache filling while
/// one download is in its cooldown/decryption phase.
const V2_MAX_CONCURRENT_PREFETCH: usize = 2;

lazy_static::lazy_static! {
    /// Semaphore to limit concurrent prefetch operations
    static ref V2_PREFETCH_SEMAPHORE: tokio::sync::Semaphore =
        tokio::sync::Semaphore::new(V2_MAX_CONCURRENT_PREFETCH);
}

/// Spawn background tasks to prefetch upcoming Qobuz tracks (V2)
/// Takes upcoming tracks directly from CoreBridge (not legacy AppState queue)
fn spawn_v2_prefetch(
    bridge: Arc<RwLock<Option<crate::core_bridge::CoreBridge>>>,
    cache: Arc<AudioCache>,
    upcoming_tracks: Vec<CoreQueueTrack>,
    quality: Quality,
    streaming_only: bool,
) {
    spawn_v2_prefetch_with_hw_check(
        bridge,
        cache,
        upcoming_tracks,
        quality,
        streaming_only,
        None,
    );
}

/// Prefetch with optional hardware rate checking.
/// If `hw_device_id` is Some, checks each track's sample rate against hardware
/// and downgrades quality if needed.
fn spawn_v2_prefetch_with_hw_check(
    bridge: Arc<RwLock<Option<crate::core_bridge::CoreBridge>>>,
    cache: Arc<AudioCache>,
    upcoming_tracks: Vec<CoreQueueTrack>,
    quality: Quality,
    streaming_only: bool,
    hw_device_id: Option<String>,
) {
    // Skip prefetch entirely in streaming_only mode
    if streaming_only {
        log::debug!("[V2/PREFETCH] Skipped - streaming_only mode active");
        return;
    }

    // upcoming_tracks already provided by caller from CoreBridge
    let upcoming_tracks = upcoming_tracks;

    if upcoming_tracks.is_empty() {
        log::debug!("[V2/PREFETCH] No upcoming tracks to prefetch");
        return;
    }

    let mut qobuz_prefetched = 0;

    for track in upcoming_tracks {
        // Stop once we've prefetched enough Qobuz tracks
        if qobuz_prefetched >= V2_PREFETCH_COUNT {
            break;
        }

        let track_id = track.id;
        let track_title = track.title.clone();

        // Skip local tracks - they don't need prefetching from Qobuz
        if track.is_local {
            log::debug!(
                "[V2/PREFETCH] Skipping local track: {} - {}",
                track_id,
                track_title
            );
            continue;
        }

        // Check if already cached or being fetched
        if cache.contains(track_id) {
            log::debug!("[V2/PREFETCH] Track {} already cached", track_id);
            qobuz_prefetched += 1;
            continue;
        }

        if cache.is_fetching(track_id) {
            log::debug!("[V2/PREFETCH] Track {} already being fetched", track_id);
            qobuz_prefetched += 1;
            continue;
        }

        // Mark as fetching
        cache.mark_fetching(track_id);
        qobuz_prefetched += 1;

        let bridge_clone = bridge.clone();
        let cache_clone = cache.clone();
        let hw_device_clone = hw_device_id.clone();

        log::info!(
            "[V2/PREFETCH] Prefetching track: {} - {}",
            track_id,
            track_title
        );

        // Spawn background task for each track (with semaphore to limit concurrency)
        tokio::spawn(async move {
            // Acquire semaphore permit to limit concurrent prefetches
            let _permit = match V2_PREFETCH_SEMAPHORE.acquire().await {
                Ok(permit) => permit,
                Err(_) => {
                    log::warn!(
                        "[V2/PREFETCH] Semaphore closed, skipping track {}",
                        track_id
                    );
                    cache_clone.unmark_fetching(track_id);
                    return;
                }
            };

            // Determine effective quality (may be downgraded for hardware compatibility)
            let effective_quality = {
                let mut eq = quality;
                #[cfg(target_os = "linux")]
                if let Some(ref device_id) = hw_device_clone {
                    if quality == Quality::UltraHiRes {
                        let bridge_guard = bridge_clone.read().await;
                        if let Some(bridge) = bridge_guard.as_ref() {
                            if let Ok(stream_url) = bridge.get_stream_url(track_id, quality).await {
                                let track_rate = (stream_url.sampling_rate * 1000.0) as u32;
                                if qbz_audio::device_supports_sample_rate(device_id, track_rate)
                                    == Some(false)
                                {
                                    log::info!(
                                        "[V2/PREFETCH] Track {} at {}Hz incompatible with hardware, prefetching at Hi-Res",
                                        track_id, track_rate
                                    );
                                    eq = Quality::HiRes;
                                }
                            }
                        }
                    }
                }
                eq
            };

            let result = async {
                let bridge_guard = bridge_clone.read().await;
                let bridge = bridge_guard.as_ref().ok_or("CoreBridge not initialized")?;

                // Try CMAF first (Akamai CDN), fall back to legacy (nginx CDN)
                match try_cmaf_full_download(bridge, track_id, effective_quality).await {
                    Ok(data) => return Ok::<Vec<u8>, String>(data),
                    Err(e) => {
                        log::warn!("[V2/PREFETCH] CMAF failed for track {}: {}, trying legacy", track_id, e);
                    }
                }

                let stream_url = bridge.get_stream_url(track_id, effective_quality).await?;
                let (data, _url) = download_with_backoff(&stream_url.url, track_id, effective_quality, bridge).await?;
                Ok(data)
            }
            .await;

            match result {
                Ok(data) => {
                    // Small delay before cache insertion to avoid potential race with audio thread
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    cache_clone.insert(track_id, data);
                    log::info!("[V2/PREFETCH] Complete for track {}", track_id);
                }
                Err(e) => {
                    log::warn!(
                        "[V2/PREFETCH] Failed for track {} after all retries: {}",
                        track_id,
                        e
                    );
                }
            }

            cache_clone.unmark_fetching(track_id);
        });
    }
}

// ==================== Auth Commands (V2) ====================

/// Check if user is logged in (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_is_logged_in(bridge: State<'_, CoreBridgeState>) -> Result<bool, RuntimeError> {
    let bridge = bridge.get().await;
    Ok(bridge.is_logged_in().await)
}

/// Login with email and password (V2 - uses QbzCore)
///
/// This performs the full login flow:
/// 0. ToS gate (REQUIRED - enforced in backend)
/// 1. Legacy auth (Qobuz API client)
/// 2. CoreBridge auth (V2)
/// 3. Session activation (per-user stores)
/// 4. Runtime state update
#[tauri::command]
pub async fn v2_login(
    app: tauri::AppHandle,
    email: String,
    password: String,
    app_state: State<'_, AppState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
    legal_state: State<'_, LegalSettingsState>,
) -> Result<UserSession, RuntimeError> {
    let manager = runtime.manager();

    // Step 1: Legacy auth
    let session = {
        let client = app_state.client.read().await;
        client
            .login(&email, &password)
            .await
            .map_err(|e| RuntimeError::Internal(e.to_string()))?
    };
    manager.set_legacy_auth(true, Some(session.user_id)).await;
    log::info!("[v2_login] Legacy auth successful");

    // Step 2: CoreBridge auth
    let bridge_guard = bridge.get().await;
    if let Err(e) = bridge_guard.login(&email, &password).await {
        log::error!("[v2_login] CoreBridge auth failed: {}", e);
        rollback_auth_state(&manager, &app).await;
        return Err(RuntimeError::Internal(e));
    }
    manager.set_corebridge_auth(true).await;
    log::info!("[v2_login] CoreBridge auth successful");

    // Step 3: Activate session
    if let Err(e) = crate::session_lifecycle::activate_session(&app, session.user_id).await {
        log::error!("[v2_login] Session activation failed: {}", e);
        rollback_auth_state(&manager, &app).await;
        return Err(RuntimeError::Internal(e));
    }
    log::info!("[v2_login] Session activated");

    // Persist ToS acceptance now that login succeeded.
    accept_tos_best_effort(&legal_state);

    // Convert api::models::UserSession to qbz_models::UserSession
    Ok(UserSession {
        user_auth_token: session.user_auth_token,
        user_id: session.user_id,
        email: session.email,
        display_name: session.display_name,
        subscription_label: session.subscription_label,
        subscription_valid_until: session.subscription_valid_until,
    })
}

/// Logout current user (V2 - uses QbzCore)
///
/// This performs the full logout flow:
/// 1. Deactivate session (teardown per-user stores)
/// 2. CoreBridge logout
/// 3. Legacy logout
/// 4. Runtime state cleanup
#[tauri::command]
pub async fn v2_logout(
    app: tauri::AppHandle,
    app_state: State<'_, AppState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[v2_logout] Starting logout");

    // Step 0: Stop any active playback
    {
        let bridge_guard = bridge.get().await;
        if let Err(e) = bridge_guard.stop() {
            log::debug!("[v2_logout] Stop playback: {}", e);
        }
        app_state.media_controls.set_stopped();
    }

    // Step 1: Deactivate session (teardown stores, clear runtime state)
    crate::session_lifecycle::deactivate_session(&app)
        .await
        .map_err(RuntimeError::Internal)?;
    log::info!("[v2_logout] Session deactivated");

    // Step 2: CoreBridge logout
    let bridge_guard = bridge.get().await;
    bridge_guard
        .logout()
        .await
        .map_err(RuntimeError::Internal)?;
    log::info!("[v2_logout] CoreBridge logged out");

    // Step 3: Legacy logout
    {
        let client = app_state.client.read().await;
        client.logout().await;
    }
    log::info!("[v2_logout] Legacy client logged out");

    // Step 4: Clear stored credentials (keyring + encrypted files)
    if let Err(e) = crate::credentials::clear_oauth_token() {
        log::warn!("[v2_logout] Failed to clear OAuth token: {}", e);
    }
    if let Err(e) = crate::credentials::clear_qobuz_credentials() {
        log::warn!("[v2_logout] Failed to clear credentials: {}", e);
    }
    log::info!("[v2_logout] Stored credentials cleared");

    Ok(())
}

/// Activate offline-only session (no remote auth required)
///
/// This creates a minimal session for offline/local library use.
/// Uses user_id = 0 as a special "offline user" marker.
/// Queue commands will work because session_activated is set.
#[tauri::command]
pub async fn v2_activate_offline_session(app: tauri::AppHandle) -> Result<(), RuntimeError> {
    crate::session_lifecycle::activate_offline_session(&app)
        .await
        .map_err(RuntimeError::Internal)
}

// ==================== UX / Settings Commands (V2 Native) ====================

#[tauri::command]
pub async fn v2_set_api_locale(locale: String, state: State<'_, AppState>) -> Result<(), String> {
    let client = state.client.read().await;
    client.set_locale(locale).await;
    Ok(())
}

#[tauri::command]
pub fn v2_set_use_system_titlebar(
    value: bool,
    state: State<'_, WindowSettingsState>,
) -> Result<(), String> {
    state.set_use_system_titlebar(value)
}

#[tauri::command]
pub fn v2_set_enable_tray(value: bool, state: State<'_, TraySettingsState>) -> Result<(), String> {
    state.set_enable_tray(value)?;
    // Mirror to global startup store so tray visibility on next launch
    // is consistent even before session activation/runtime bootstrap.
    if let Ok(global_store) = crate::config::tray_settings::TraySettingsStore::new() {
        let _ = global_store.set_enable_tray(value);
    }
    Ok(())
}

#[tauri::command]
pub fn v2_set_minimize_to_tray(
    value: bool,
    state: State<'_, TraySettingsState>,
) -> Result<(), String> {
    state.set_minimize_to_tray(value)
}

#[tauri::command]
pub fn v2_set_close_to_tray(
    value: bool,
    state: State<'_, TraySettingsState>,
) -> Result<(), String> {
    state.set_close_to_tray(value)
}

#[tauri::command]
pub fn v2_set_autoplay_mode(
    mode: AutoplayMode,
    state: State<'_, PlaybackPreferencesState>,
) -> Result<(), String> {
    state.set_autoplay_mode(mode)
}

#[tauri::command]
pub fn v2_set_show_context_icon(
    show: bool,
    state: State<'_, PlaybackPreferencesState>,
) -> Result<(), String> {
    state.set_show_context_icon(show)
}

#[tauri::command]
pub fn v2_set_persist_session(
    persist: bool,
    state: State<'_, PlaybackPreferencesState>,
) -> Result<(), String> {
    state.set_persist_session(persist)
}

#[tauri::command]
pub fn v2_get_playback_preferences(
    state: State<'_, PlaybackPreferencesState>,
) -> Result<PlaybackPreferences, String> {
    state.get_preferences()
}

#[tauri::command]
pub fn v2_get_tray_settings(state: State<'_, TraySettingsState>) -> Result<TraySettings, String> {
    state.get_settings()
}

#[tauri::command]
pub fn v2_get_favorites_preferences(
    state: State<'_, crate::config::favorites_preferences::FavoritesPreferencesState>,
) -> Result<FavoritesPreferences, String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites preferences store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_preferences()
}

#[tauri::command]
pub fn v2_save_favorites_preferences(
    prefs: FavoritesPreferences,
    state: State<'_, crate::config::favorites_preferences::FavoritesPreferencesState>,
) -> Result<FavoritesPreferences, String> {
    crate::config::favorites_preferences::save_favorites_preferences(prefs, state)
}

#[tauri::command]
pub fn v2_get_cache_stats(state: State<'_, AppState>) -> CacheStats {
    state.audio_cache.stats()
}

#[tauri::command]
pub fn v2_get_available_backends() -> Result<Vec<BackendInfo>, String> {
    log::info!("Command: v2_get_available_backends");

    let backends = BackendManager::available_backends();
    let backend_infos: Vec<BackendInfo> = backends
        .into_iter()
        .map(|backend_type| {
            let backend = BackendManager::create_backend(backend_type);
            let (is_available, description) = match backend {
                Ok(b) => (b.is_available(), b.description().to_string()),
                Err(_) => (false, "Not available".to_string()),
            };

            let name = match backend_type {
                AudioBackendType::PipeWire => "PipeWire",
                AudioBackendType::Alsa => "ALSA Direct",
                AudioBackendType::Pulse => "PulseAudio",
                AudioBackendType::SystemDefault => "System Audio",
            };

            BackendInfo {
                backend_type,
                name: name.to_string(),
                description,
                is_available,
            }
        })
        .collect();

    Ok(backend_infos)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_get_devices_for_backend(
    backendType: AudioBackendType,
) -> Result<Vec<AudioDevice>, String> {
    log::info!("Command: v2_get_devices_for_backend({:?})", backendType);
    let backend = BackendManager::create_backend(backendType)?;
    backend.enumerate_devices()
}

#[tauri::command]
pub async fn v2_get_hardware_audio_status(
    state: State<'_, AppState>,
    core_bridge: State<'_, CoreBridgeState>,
) -> Result<HardwareAudioStatus, String> {
    // Try V2 player first (CoreBridge), fall back to legacy player
    let (sample_rate, bit_depth, is_playing) = if let Some(bridge) = core_bridge.try_get().await {
        let player = bridge.player();
        (
            player.state.get_sample_rate(),
            player.state.get_bit_depth(),
            player.state.is_playing(),
        )
    } else {
        (
            state.player.state.get_sample_rate(),
            state.player.state.get_bit_depth(),
            state.player.state.is_playing(),
        )
    };

    let active = is_playing && sample_rate > 0;

    let hardware_sample_rate = if sample_rate > 0 {
        Some(sample_rate)
    } else {
        None
    };
    let hardware_format = if sample_rate > 0 && bit_depth > 0 {
        Some(format!(
            "{}-bit / {:.1}kHz",
            bit_depth,
            sample_rate as f64 / 1000.0
        ))
    } else {
        None
    };

    Ok(HardwareAudioStatus {
        hardware_sample_rate,
        hardware_format,
        is_active: active,
    })
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_get_default_device_name(backendType: AudioBackendType) -> Result<Option<String>, String> {
    let backend = BackendManager::create_backend(backendType)?;
    let devices = backend.enumerate_devices()?;
    Ok(devices.into_iter().find(|d| d.is_default).map(|d| d.name))
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_query_dac_capabilities(nodeName: String) -> Result<DacCapabilities, String> {
    // Default fallback — only used if all detection methods fail
    let fallback_rates = vec![44100, 48000, 88200, 96000, 176400, 192000];

    let mut capabilities = DacCapabilities {
        node_name: nodeName.clone(),
        sample_rates: fallback_rates.clone(),
        formats: vec![
            "S16LE".to_string(),
            "S24LE".to_string(),
            "F32LE".to_string(),
        ],
        channels: Some(2),
        description: None,
        error: None,
    };

    // Try PipeWire backend: get device description and ALSA card for rate detection
    if let Ok(backend) = BackendManager::create_backend(AudioBackendType::PipeWire) {
        if let Ok(devices) = backend.enumerate_devices() {
            if let Some(device) = devices
                .iter()
                .find(|d| d.id == nodeName || d.name == nodeName)
            {
                capabilities.description = device
                    .description
                    .clone()
                    .or_else(|| Some(device.name.clone()));
            }
        }
    }

    // Detect real sample rates from /proc/asound via PipeWire sink -> ALSA card mapping
    #[cfg(target_os = "linux")]
    {
        if let Some(rates) =
            crate::audio::pipewire_backend::PipeWireBackend::get_sink_supported_rates(&nodeName)
        {
            log::info!(
                "[HiFi Wizard] Detected sample rates for {}: {:?}",
                nodeName,
                rates
            );
            capabilities.sample_rates = rates;
        } else {
            // Fallback: try ALSA device ID directly (for ALSA Direct backend)
            if let Some(rates) = qbz_audio::get_device_supported_rates(&nodeName) {
                log::info!(
                    "[HiFi Wizard] Detected sample rates via ALSA for {}: {:?}",
                    nodeName,
                    rates
                );
                capabilities.sample_rates = rates;
            } else {
                log::warn!(
                    "[HiFi Wizard] Could not detect sample rates for {}, using defaults",
                    nodeName
                );
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        log::info!(
            "[HiFi Wizard] Hardware sample rate detection not yet implemented on this platform for {}",
            nodeName
        );
    }

    Ok(capabilities)
}

#[tauri::command]
pub fn v2_get_alsa_plugins() -> Result<Vec<AlsaPluginInfo>, String> {
    Ok(vec![
        AlsaPluginInfo {
            plugin: AlsaPlugin::Hw,
            name: "hw (Direct Hardware)".to_string(),
            description: "Bit-perfect, exclusive access, blocks device for other apps".to_string(),
        },
        AlsaPluginInfo {
            plugin: AlsaPlugin::PlugHw,
            name: "plughw (Plugin Hardware)".to_string(),
            description: "Automatic format conversion, still relatively direct".to_string(),
        },
        AlsaPluginInfo {
            plugin: AlsaPlugin::Pcm,
            name: "pcm (Default)".to_string(),
            description: "Generic ALSA device, most compatible".to_string(),
        },
    ])
}

mod library;
pub use library::*;

mod link_resolver;
pub use link_resolver::*;

// ==================== Queue Commands (V2) ====================

/// Get current queue state (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_queue_state(
    bridge: State<'_, CoreBridgeState>,
) -> Result<QueueState, RuntimeError> {
    let bridge = bridge.get().await;
    Ok(bridge.get_queue_state().await)
}

/// Full queue snapshot for session persistence (no caps on track count)
#[derive(serde::Serialize)]
pub struct AllQueueTracksResponse {
    pub tracks: Vec<CoreQueueTrack>,
    pub current_index: Option<usize>,
}

/// Get all queue tracks and current index (for session persistence, no caps)
#[tauri::command]
pub async fn v2_get_all_queue_tracks(
    bridge: State<'_, CoreBridgeState>,
) -> Result<AllQueueTracksResponse, RuntimeError> {
    let bridge = bridge.get().await;
    let (tracks, current_index) = bridge.get_all_queue_tracks().await;
    Ok(AllQueueTracksResponse {
        tracks,
        current_index,
    })
}

/// Get currently selected queue track (V2)
#[tauri::command]
pub async fn v2_get_current_queue_track(
    bridge: State<'_, CoreBridgeState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    let bridge = bridge.get().await;
    let state = bridge.get_queue_state().await;
    Ok(state.current_track.map(Into::into))
}

/// Set repeat mode (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_set_repeat_mode(
    mode: RepeatMode,
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    if qconnect.status().await.transport_connected {
        qconnect
            .send_command(
                QueueCommandType::CtrlSrvrSetLoopMode,
                serde_json::json!({
                    "loop_mode": qconnect_loop_mode_from_repeat_mode(mode),
                }),
            )
            .await
            .map_err(RuntimeError::Internal)?;
        return Ok(());
    }

    let bridge = bridge.get().await;
    bridge.set_repeat_mode(mode).await;
    Ok(())
}

/// Toggle shuffle (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_toggle_shuffle(
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<bool, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    if qconnect.status().await.transport_connected {
        let queue = qconnect
            .queue_snapshot()
            .await
            .map_err(RuntimeError::Internal)?;
        let next_enabled = !queue.shuffle_mode;
        apply_qconnect_shuffle_mode(qconnect.inner(), &queue, next_enabled).await?;
        return Ok(next_enabled);
    }

    let bridge = bridge.get().await;
    Ok(bridge.toggle_shuffle().await)
}

/// Set shuffle mode directly (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_set_shuffle(
    enabled: bool,
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] set_shuffle: {}", enabled);

    if qconnect.status().await.transport_connected {
        let queue = qconnect
            .queue_snapshot()
            .await
            .map_err(RuntimeError::Internal)?;
        apply_qconnect_shuffle_mode(qconnect.inner(), &queue, enabled).await?;
        return Ok(());
    }

    let bridge = bridge.get().await;
    bridge.set_shuffle(enabled).await;
    Ok(())
}

/// Clear the queue (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_clear_queue(
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    if qconnect.status().await.transport_connected {
        qconnect
            .send_command(QueueCommandType::CtrlSrvrClearQueue, serde_json::json!({}))
            .await
            .map_err(RuntimeError::Internal)?;
        return Ok(());
    }

    let bridge = bridge.get().await;
    bridge.clear_queue().await;
    Ok(())
}

async fn apply_qconnect_shuffle_mode(
    qconnect: &QconnectServiceState,
    queue: &QConnectQueueState,
    enabled: bool,
) -> Result<(), RuntimeError> {
    let renderer = qconnect.renderer_snapshot().await.unwrap_or_default();
    let shuffle_seed = enabled.then(|| rand::random::<u32>() & (i32::MAX as u32));
    let pivot_queue_item_id = resolve_qconnect_shuffle_pivot(queue, &renderer);

    qconnect
        .send_command(
            QueueCommandType::CtrlSrvrSetShuffleMode,
            serde_json::json!({
                "shuffle_mode": enabled,
                "shuffle_seed": shuffle_seed.map(i64::from),
                "shuffle_pivot_queue_item_id": pivot_queue_item_id
                    .and_then(|value| i32::try_from(value).ok())
                    .map(i64::from),
                "autoplay_reset": false,
                "autoplay_loading": false,
            }),
        )
        .await
        .map_err(RuntimeError::Internal)?;
    Ok(())
}

fn qconnect_queue_item_id_to_wire_value(queue_item_id: u64) -> Result<i64, RuntimeError> {
    i64::try_from(queue_item_id)
        .map_err(|_| RuntimeError::Internal("queue_item_id out of range".to_string()))
}

fn build_qconnect_remove_upcoming_payload(
    projection: &QconnectVisibleQueueProjection,
    upcoming_index: usize,
) -> Result<Option<serde_json::Value>, RuntimeError> {
    let Some(queue_item) = projection.upcoming_tracks.get(upcoming_index) else {
        return Ok(None);
    };

    Ok(Some(serde_json::json!({
        "queue_item_ids": [qconnect_queue_item_id_to_wire_value(queue_item.queue_item_id)?],
        "autoplay_reset": false,
        "autoplay_loading": false,
    })))
}

fn build_qconnect_reorder_payload(
    projection: &QconnectVisibleQueueProjection,
    from_index: usize,
    to_index: usize,
) -> Result<Option<serde_json::Value>, RuntimeError> {
    let upcoming_len = projection.upcoming_tracks.len();
    if from_index >= upcoming_len || to_index >= upcoming_len {
        return Ok(None);
    }
    if from_index == to_index {
        return Ok(Some(serde_json::json!({})));
    }

    let mut remaining_queue_item_ids: Vec<u64> = projection
        .upcoming_tracks
        .iter()
        .map(|item| item.queue_item_id)
        .collect();
    let moved_queue_item_id = remaining_queue_item_ids.remove(from_index);
    let insert_position = if from_index < to_index {
        to_index.saturating_sub(1)
    } else {
        to_index
    };
    let insert_after = if insert_position == 0 {
        projection
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id)
    } else {
        remaining_queue_item_ids.get(insert_position - 1).copied()
    };

    Ok(Some(serde_json::json!({
        "queue_item_ids": [qconnect_queue_item_id_to_wire_value(moved_queue_item_id)?],
        "insert_after": insert_after
            .map(qconnect_queue_item_id_to_wire_value)
            .transpose()?,
        "autoplay_reset": false,
        "autoplay_loading": false,
    })))
}

fn qconnect_loop_mode_from_repeat_mode(mode: RepeatMode) -> i32 {
    // QConnect protocol loop mode values:
    // 1 = off, 2 = repeat one, 3 = repeat all.
    match mode {
        RepeatMode::Off => 1,
        RepeatMode::All => 3,
        RepeatMode::One => 2,
    }
}

fn resolve_qconnect_shuffle_pivot(
    queue: &QConnectQueueState,
    renderer: &QConnectRendererState,
) -> Option<u64> {
    let Some(current_track) = renderer.current_track.as_ref() else {
        return None;
    };

    if queue
        .queue_items
        .iter()
        .position(|item| item.queue_item_id == current_track.queue_item_id)
        .is_some()
    {
        return Some(current_track.queue_item_id);
    }

    if let Some((_, item)) = queue
        .queue_items
        .iter()
        .enumerate()
        .find(|(_, item)| item.track_id == current_track.track_id)
    {
        return Some(item.queue_item_id);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        build_qconnect_remove_upcoming_payload, build_qconnect_reorder_payload,
        qconnect_loop_mode_from_repeat_mode, resolve_qconnect_shuffle_pivot,
    };
    use crate::qconnect_service::QconnectVisibleQueueProjection;
    use qbz_models::RepeatMode;
    use qconnect_app::{QConnectQueueState, QConnectRendererState};
    use qconnect_core::QueueItem;
    use serde_json::json;

    fn item(queue_item_id: u64, track_id: u64) -> QueueItem {
        QueueItem {
            track_context_uuid: "ctx".to_string(),
            track_id,
            queue_item_id,
        }
    }

    #[test]
    fn maps_repeat_mode_to_qconnect_loop_mode() {
        assert_eq!(qconnect_loop_mode_from_repeat_mode(RepeatMode::Off), 1);
        assert_eq!(qconnect_loop_mode_from_repeat_mode(RepeatMode::All), 3);
        assert_eq!(qconnect_loop_mode_from_repeat_mode(RepeatMode::One), 2);
    }

    #[test]
    fn resolves_shuffle_pivot_from_renderer_queue_item_id() {
        let queue = QConnectQueueState {
            queue_items: vec![item(10, 100), item(11, 101), item(12, 102)],
            ..Default::default()
        };
        let renderer = QConnectRendererState {
            current_track: Some(item(11, 101)),
            ..Default::default()
        };

        let queue_item_id = resolve_qconnect_shuffle_pivot(&queue, &renderer);
        assert_eq!(queue_item_id, Some(11));
    }

    #[test]
    fn resolves_shuffle_pivot_by_track_id_when_renderer_qid_is_placeholder() {
        let queue = QConnectQueueState {
            queue_items: vec![item(20, 200), item(21, 201), item(22, 202)],
            ..Default::default()
        };
        let renderer = QConnectRendererState {
            current_track: Some(item(0, 202)),
            ..Default::default()
        };

        let queue_item_id = resolve_qconnect_shuffle_pivot(&queue, &renderer);
        assert_eq!(queue_item_id, Some(22));
    }

    #[test]
    fn remove_upcoming_payload_uses_queue_item_id_from_projection() {
        let projection = QconnectVisibleQueueProjection {
            current_track: Some(item(0, 100)),
            upcoming_tracks: vec![item(7, 107), item(8, 108)],
        };

        let payload =
            build_qconnect_remove_upcoming_payload(&projection, 1).expect("payload build");

        assert_eq!(
            payload,
            Some(json!({
                "queue_item_ids": [8],
                "autoplay_reset": false,
                "autoplay_loading": false,
            })),
        );
    }

    #[test]
    fn reorder_payload_moves_track_before_drop_target_using_current_anchor() {
        let projection = QconnectVisibleQueueProjection {
            current_track: Some(item(0, 100)),
            upcoming_tracks: vec![item(1, 101), item(2, 102), item(3, 103), item(4, 104)],
        };

        let payload = build_qconnect_reorder_payload(&projection, 0, 3).expect("payload build");

        assert_eq!(
            payload,
            Some(json!({
                "queue_item_ids": [1],
                "insert_after": 3,
                "autoplay_reset": false,
                "autoplay_loading": false,
            })),
        );
    }

    #[test]
    fn reorder_payload_can_move_track_to_first_upcoming_slot() {
        let projection = QconnectVisibleQueueProjection {
            current_track: Some(item(0, 100)),
            upcoming_tracks: vec![item(1, 101), item(2, 102), item(3, 103), item(4, 104)],
        };

        let payload = build_qconnect_reorder_payload(&projection, 3, 0).expect("payload build");

        assert_eq!(
            payload,
            Some(json!({
                "queue_item_ids": [4],
                "insert_after": 0,
                "autoplay_reset": false,
                "autoplay_loading": false,
            })),
        );
    }
}

/// Queue track representation for V2 commands
/// Maps to internal QueueTrack format
/// Field names match frontend BackendQueueTrack interface exactly
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct V2QueueTrack {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub hires: bool,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
    #[serde(default)]
    pub is_local: bool,
    pub album_id: Option<String>,
    pub artist_id: Option<u64>,
    #[serde(default = "default_streamable")]
    pub streamable: bool,
    /// Source type: "qobuz", "local", "plex"
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub parental_warning: bool,
}

fn default_streamable() -> bool {
    true
}

impl From<V2QueueTrack> for crate::queue::QueueTrack {
    fn from(t: V2QueueTrack) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_secs: t.duration_secs,
            artwork_url: t.artwork_url,
            hires: t.hires,
            bit_depth: t.bit_depth,
            sample_rate: t.sample_rate,
            is_local: t.is_local,
            album_id: t.album_id,
            artist_id: t.artist_id,
            streamable: t.streamable,
            source: t.source,
            parental_warning: t.parental_warning,
        }
    }
}

impl From<crate::queue::QueueTrack> for V2QueueTrack {
    fn from(t: crate::queue::QueueTrack) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_secs: t.duration_secs,
            artwork_url: t.artwork_url,
            hires: t.hires,
            bit_depth: t.bit_depth,
            sample_rate: t.sample_rate,
            is_local: t.is_local,
            album_id: t.album_id,
            artist_id: t.artist_id,
            streamable: t.streamable,
            source: t.source,
            parental_warning: t.parental_warning,
        }
    }
}

// V2 queue track <-> qbz_models::QueueTrack (CoreQueueTrack)
impl From<V2QueueTrack> for CoreQueueTrack {
    fn from(t: V2QueueTrack) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_secs: t.duration_secs,
            artwork_url: t.artwork_url,
            hires: t.hires,
            bit_depth: t.bit_depth,
            sample_rate: t.sample_rate,
            is_local: t.is_local,
            album_id: t.album_id,
            artist_id: t.artist_id,
            streamable: t.streamable,
            source: t.source,
            parental_warning: t.parental_warning,
        }
    }
}

impl From<CoreQueueTrack> for V2QueueTrack {
    fn from(t: CoreQueueTrack) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_secs: t.duration_secs,
            artwork_url: t.artwork_url,
            hires: t.hires,
            bit_depth: t.bit_depth,
            sample_rate: t.sample_rate,
            is_local: t.is_local,
            album_id: t.album_id,
            artist_id: t.artist_id,
            streamable: t.streamable,
            source: t.source,
            parental_warning: t.parental_warning,
        }
    }
}

/// Add track to the end of the queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_add_to_queue(
    track: V2QueueTrack,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] add_to_queue: {} - {}", track.id, track.title);
    let bridge = bridge.get().await;
    bridge.add_track(track.into()).await;
    Ok(())
}

/// Add track to play next (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_add_to_queue_next(
    track: V2QueueTrack,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] add_to_queue_next: {} - {}", track.id, track.title);
    let bridge = bridge.get().await;
    bridge.add_track_next(track.into()).await;
    Ok(())
}

/// Add multiple tracks to end of queue (V2 - bulk)
#[tauri::command]
pub async fn v2_bulk_add_to_queue(
    tracks: Vec<V2QueueTrack>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] bulk_add_to_queue: {} tracks", tracks.len());
    let bridge = bridge.get().await;
    for track in tracks {
        bridge.add_track(track.into()).await;
    }
    Ok(())
}

/// Add multiple tracks as play next (V2 - bulk, reversed to preserve order)
#[tauri::command]
pub async fn v2_bulk_add_to_queue_next(
    tracks: Vec<V2QueueTrack>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] bulk_add_to_queue_next: {} tracks", tracks.len());
    let bridge = bridge.get().await;
    // Reverse so the first track in the selection ends up as "next"
    for track in tracks.into_iter().rev() {
        bridge.add_track_next(track.into()).await;
    }
    Ok(())
}

/// Set the entire queue and start playing from index (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_set_queue(
    tracks: Vec<V2QueueTrack>,
    start_index: usize,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!(
        "[V2] set_queue: {} tracks, start at {}",
        tracks.len(),
        start_index
    );
    let queue_tracks: Vec<CoreQueueTrack> = tracks.into_iter().map(Into::into).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(start_index)).await;
    Ok(())
}

/// Remove a track from the queue by index (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_remove_from_queue(
    index: usize,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] remove_from_queue: index {}", index);
    let bridge = bridge.get().await;
    bridge.remove_track(index).await;
    Ok(())
}

/// Remove a track from the upcoming queue by its position (V2 - uses CoreBridge)
/// (0 = first upcoming track, handles shuffle mode correctly)
#[tauri::command]
pub async fn v2_remove_upcoming_track(
    upcoming_index: usize,
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!(
        "[V2] remove_upcoming_track: upcoming_index {}",
        upcoming_index
    );

    if qconnect.status().await.transport_connected {
        let projection = qconnect
            .visible_queue_projection()
            .await
            .map_err(RuntimeError::Internal)?;
        let Some(payload) = build_qconnect_remove_upcoming_payload(&projection, upcoming_index)?
        else {
            return Ok(None);
        };

        qconnect
            .send_command(QueueCommandType::CtrlSrvrQueueRemoveTracks, payload)
            .await
            .map_err(RuntimeError::Internal)?;
        return Ok(None);
    }

    let bridge = bridge.get().await;
    Ok(bridge
        .remove_upcoming_track(upcoming_index)
        .await
        .map(Into::into))
}

/// Skip to next track in queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_next_track(
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    app_handle: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] next_track");
    if qconnect
        .skip_next_if_remote(&app_handle)
        .await
        .map_err(RuntimeError::Internal)?
    {
        return Ok(None);
    }
    let bridge = bridge.get().await;
    let track = bridge.next_track().await;
    Ok(track.map(Into::into))
}

/// Go to previous track in queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_previous_track(
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    app_handle: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] previous_track");
    if qconnect
        .skip_previous_if_remote(&app_handle)
        .await
        .map_err(RuntimeError::Internal)?
    {
        return Ok(None);
    }
    let bridge = bridge.get().await;
    let track = bridge.previous_track().await;
    Ok(track.map(Into::into))
}

/// Play a specific track in the queue by index (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_play_queue_index(
    index: usize,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] play_queue_index: {}", index);
    let bridge = bridge.get().await;
    let track = bridge.play_index(index).await;
    Ok(track.map(Into::into))
}

/// Move a track within the queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_move_queue_track(
    from_index: usize,
    to_index: usize,
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<bool, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] move_queue_track: {} -> {}", from_index, to_index);

    if qconnect.status().await.transport_connected {
        let projection = qconnect
            .visible_queue_projection()
            .await
            .map_err(RuntimeError::Internal)?;
        let Some(payload) = build_qconnect_reorder_payload(&projection, from_index, to_index)?
        else {
            return Ok(false);
        };
        if from_index == to_index {
            return Ok(true);
        }

        qconnect
            .send_command(QueueCommandType::CtrlSrvrQueueReorderTracks, payload)
            .await
            .map_err(RuntimeError::Internal)?;
        return Ok(true);
    }

    let bridge = bridge.get().await;
    Ok(bridge.move_track(from_index, to_index).await)
}

/// Add multiple tracks to queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_add_tracks_to_queue(
    tracks: Vec<V2QueueTrack>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] add_tracks_to_queue: {} tracks", tracks.len());
    let queue_tracks: Vec<CoreQueueTrack> = tracks.into_iter().map(Into::into).collect();
    let bridge = bridge.get().await;
    bridge.add_tracks(queue_tracks).await;
    Ok(())
}

/// Add multiple tracks to play next (V2 - uses CoreBridge)
/// Tracks are added in reverse order so they play in the order provided
#[tauri::command]
pub async fn v2_add_tracks_to_queue_next(
    tracks: Vec<V2QueueTrack>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] add_tracks_to_queue_next: {} tracks", tracks.len());
    let bridge = bridge.get().await;
    // Add in reverse order so they end up in the correct order
    for track in tracks.into_iter().rev() {
        bridge.add_track_next(track.into()).await;
    }
    Ok(())
}

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

// ==================== Playback Commands (V2) ====================
//
// These commands use CoreBridge.player (qbz-player crate) for playback.
// This is the V2 architecture - playback flows through QbzCore.

/// Pause playback (V2)
#[tauri::command]
pub async fn v2_pause_playback(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] Command: pause_playback");
    app_state.media_controls.set_playback(false);
    let bridge = bridge.get().await;
    bridge.pause().map_err(RuntimeError::Internal)
}

/// Resume playback (V2)
#[tauri::command]
pub async fn v2_resume_playback(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] Command: resume_playback");
    app_state.media_controls.set_playback(true);
    let bridge = bridge.get().await;
    bridge.resume().map_err(RuntimeError::Internal)
}

/// Stop playback (V2)
#[tauri::command]
pub async fn v2_stop_playback(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] Command: stop_playback");
    app_state.media_controls.set_stopped();
    let bridge = bridge.get().await;
    bridge.stop().map_err(RuntimeError::Internal)
}

/// Seek to position in seconds (V2)
#[tauri::command]
pub async fn v2_seek(
    position: u64,
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] Command: seek {}", position);
    let bridge_guard = bridge.get().await;
    bridge_guard
        .seek(position)
        .map_err(RuntimeError::Internal)?;

    // Update MPRIS with effective playback state only after successful seek.
    let playback_state = bridge_guard.get_playback_state();
    app_state
        .media_controls
        .set_playback_with_progress(playback_state.is_playing, playback_state.position);

    Ok(())
}

/// Set volume (0.0 - 1.0) (V2)
///
/// When ALSA Direct hw: is active, volume is forced to 1.0 (100%)
/// because hw: bypasses all software mixing — volume must be
/// controlled at the DAC/hardware level.
#[tauri::command]
pub async fn v2_set_volume(
    volume: f32,
    bridge: State<'_, CoreBridgeState>,
    audio_state: State<'_, AudioSettingsState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    // Force 100% volume when ALSA Direct hw: is active
    let effective_volume = {
        let is_alsa_hw = audio_state
            .store
            .lock()
            .ok()
            .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()))
            .map(|s| {
                s.backend_type == Some(AudioBackendType::Alsa)
                    && s.alsa_plugin == Some(AlsaPlugin::Hw)
            })
            .unwrap_or(false);
        if is_alsa_hw { 1.0 } else { volume }
    };

    let bridge = bridge.get().await;
    bridge
        .set_volume(effective_volume)
        .map_err(RuntimeError::Internal)
}

/// Get current playback state (V2) - also updates MPRIS progress
#[tauri::command]
pub async fn v2_get_playback_state(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
) -> Result<qbz_player::PlaybackState, RuntimeError> {
    let bridge = bridge.get().await;
    let playback_state = bridge.get_playback_state();

    // Update MPRIS with current progress (called every ~500ms from frontend)
    app_state
        .media_controls
        .set_playback_with_progress(playback_state.is_playing, playback_state.position);

    Ok(playback_state)
}

/// Set media controls metadata (V2 - for MPRIS integration)
#[tauri::command]
pub async fn v2_set_media_metadata(
    title: String,
    artist: String,
    album: String,
    duration_secs: Option<u64>,
    cover_url: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Command: set_media_metadata - {} by {}", title, artist);
    crate::update_media_controls_metadata(
        &app_state.media_controls,
        &title,
        &artist,
        &album,
        duration_secs,
        cover_url,
    );
    Ok(())
}

/// Queue next track for gapless playback (V2 - cache-only, no download)
/// Returns true if gapless was queued, false if track not cached or ineligible
#[tauri::command]
pub async fn v2_play_next_gapless(
    track_id: u64,
    bridge: State<'_, CoreBridgeState>,
    offline_cache: State<'_, OfflineCacheState>,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<bool, RuntimeError> {
    log::info!("[V2] Command: play_next_gapless for track {}", track_id);

    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();
    let current_track_id = player.state.current_track_id();
    let repeat_mode = bridge_guard.get_queue_state().await.repeat;

    // Defensive guard: never queue the currently playing track as "next".
    // This avoids infinite one-track loops when frontend queue state is stale.
    if current_track_id != 0 && repeat_mode != RepeatMode::One && current_track_id == track_id {
        log::warn!(
            "[V2/GAPLESS] Ignoring play_next_gapless for current track {}",
            track_id
        );
        return Ok(false);
    }

    // Check offline cache (persistent disk cache)
    {
        let cached_path = {
            let db_opt = offline_cache.db.lock().await;
            if let Some(db) = db_opt.as_ref() {
                if let Ok(Some(file_path)) = db.get_file_path(track_id) {
                    Some(file_path)
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(file_path) = cached_path {
            let path = std::path::Path::new(&file_path);
            if path.exists() {
                log::info!("[V2/GAPLESS] Track {} from OFFLINE cache", track_id);
                let audio_data = std::fs::read(path).map_err(|e| {
                    RuntimeError::Internal(format!("Failed to read cached file: {}", e))
                })?;
                player
                    .play_next(audio_data, track_id)
                    .map_err(RuntimeError::Internal)?;
                return Ok(true);
            }
        }
    }

    // Check memory cache (L1)
    let cache = app_state.audio_cache.clone();
    if let Some(cached) = cache.get(track_id) {
        log::info!(
            "[V2/GAPLESS] Track {} from MEMORY cache ({} bytes)",
            track_id,
            cached.size_bytes
        );
        player
            .play_next(cached.data, track_id)
            .map_err(RuntimeError::Internal)?;
        return Ok(true);
    }

    // Check playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!(
                "[V2/GAPLESS] Track {} from DISK cache ({} bytes)",
                track_id,
                audio_data.len()
            );
            player
                .play_next(audio_data, track_id)
                .map_err(RuntimeError::Internal)?;
            return Ok(true);
        }
    }

    // Check local library
    if let Ok(track_id_i64) = i64::try_from(track_id) {
        let local_path = v2_library_get_tracks_by_ids(vec![track_id_i64], library_state.clone())
            .await
            .ok()
            .and_then(|mut tracks| tracks.pop())
            .map(|track| std::path::PathBuf::from(track.file_path))
            .filter(|p| p.exists());

        if let Some(path) = local_path {
            log::info!("[V2/GAPLESS] Track {} from LOCAL library", track_id);
            let audio_data = std::fs::read(&path)
                .map_err(|e| RuntimeError::Internal(format!("Failed to read local file: {}", e)))?;
            bridge.get().await.player()
                .play_next(audio_data, track_id)
                .map_err(RuntimeError::Internal)?;
            return Ok(true);
        }
    }

    log::info!(
        "[V2/GAPLESS] Track {} not in any cache, gapless not possible",
        track_id
    );
    Ok(false)
}

/// Prefetch a track into the in-memory cache without starting playback (V2)
#[tauri::command]
pub async fn v2_prefetch_track(
    track_id: u64,
    quality: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    offline_cache: State<'_, OfflineCacheState>,
    audio_settings: State<'_, AudioSettingsState>,
    app_state: State<'_, AppState>,
) -> Result<(), RuntimeError> {
    let preferred_quality = parse_quality(quality.as_deref());

    // Apply per-device sample rate limit if enabled
    let final_quality = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        if let Some(store) = guard.as_ref() {
            if let Ok(settings) = store.get_settings() {
                if settings.limit_quality_to_device {
                    let device_id = settings.output_device.as_deref().unwrap_or("default");
                    let max_rate = settings
                        .device_sample_rate_limits
                        .get(device_id)
                        .copied()
                        .or(settings.device_max_sample_rate);
                    limit_quality_for_device(preferred_quality, max_rate)
                } else {
                    preferred_quality
                }
            } else {
                preferred_quality
            }
        } else {
            preferred_quality
        }
    };

    log::info!(
        "[V2] Command: prefetch_track {} (quality_str={:?}, parsed={:?}, final={:?})",
        track_id,
        quality,
        preferred_quality,
        final_quality
    );

    let cache = app_state.audio_cache.clone();

    if cache.contains(track_id) {
        log::info!("[V2] Track {} already in memory cache", track_id);
        return Ok(());
    }

    if cache.is_fetching(track_id) {
        log::info!("[V2] Track {} already being fetched", track_id);
        return Ok(());
    }

    cache.mark_fetching(track_id);
    let result: Result<(), RuntimeError> = async {
        // Check persistent offline cache first
        {
            let cached_path = {
                let db_opt = offline_cache.db.lock().await;
                if let Some(db) = db_opt.as_ref() {
                    db.get_file_path(track_id).ok().flatten()
                } else {
                    None
                }
            };
            if let Some(file_path) = cached_path {
                let path = std::path::Path::new(&file_path);
                if path.exists() {
                    log::info!("[V2] Prefetching track {} from offline cache", track_id);
                    let audio_data = std::fs::read(path).map_err(|e| {
                        RuntimeError::Internal(format!("Failed to read cached file: {}", e))
                    })?;
                    cache.insert(track_id, audio_data);
                    return Ok(());
                }
            }
        }

        let bridge_guard = bridge.get().await;

        // Try CMAF first (Akamai CDN), fall back to legacy
        match try_cmaf_full_download(&*bridge_guard, track_id, final_quality).await {
            Ok(data) => {
                log::info!("[V2/CMAF] Prefetch succeeded for track {}", track_id);
                drop(bridge_guard);
                cache.insert(track_id, data);
                return Ok(());
            }
            Err(e) => {
                log::warn!("[V2/CMAF] Prefetch CMAF failed for track {}: {}, trying legacy", track_id, e);
            }
        }

        let stream_url = bridge_guard
            .get_stream_url(track_id, final_quality)
            .await
            .map_err(RuntimeError::Internal)?;
        drop(bridge_guard);

        let audio_data = download_audio(&stream_url.url)
            .await
            .map_err(RuntimeError::Internal)?;
        cache.insert(track_id, audio_data);
        Ok(())
    }
    .await;

    cache.unmark_fetching(track_id);
    result
}

/// Result from play_track command with format info
#[derive(serde::Serialize)]
pub struct V2PlayTrackResult {
    /// The actual format_id returned by Qobuz (5=MP3, 6=FLAC 16-bit, 7=24-bit, 27=Hi-Res)
    /// None when playing from cache (format unknown)
    pub format_id: Option<u32>,
}

/// Play a track by ID (V2 - uses CoreBridge for API and playback)
///
/// This is the core playback command that:
/// 1. Checks caches (offline, memory, disk)
/// 2. Gets stream URL from Qobuz via CoreBridge
/// 3. Downloads audio
/// 4. Plays via CoreBridge.player() (qbz-player crate)
/// 5. Caches for future playback
#[tauri::command]
pub async fn v2_play_track(
    track_id: u64,
    quality: Option<String>,
    force_lowest_quality: Option<bool>,
    duration_secs: Option<u64>,
    bridge: State<'_, CoreBridgeState>,
    offline_cache: State<'_, OfflineCacheState>,
    audio_settings: State<'_, AudioSettingsState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<V2PlayTrackResult, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 playback
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let preferred_quality = parse_quality(quality.as_deref());

    // Apply per-device sample rate limit if enabled
    let final_quality = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        if let Some(store) = guard.as_ref() {
            if let Ok(settings) = store.get_settings() {
                if settings.limit_quality_to_device {
                    let device_id = settings.output_device.as_deref().unwrap_or("default");
                    let max_rate = settings
                        .device_sample_rate_limits
                        .get(device_id)
                        .copied()
                        .or(settings.device_max_sample_rate);
                    limit_quality_for_device(preferred_quality, max_rate)
                } else {
                    preferred_quality
                }
            } else {
                preferred_quality
            }
        } else {
            preferred_quality
        }
    };

    // Override quality to Mp3 if force_lowest_quality is set (used by
    // QualityFallbackModal "always_fallback" preference)
    let final_quality = if force_lowest_quality.unwrap_or(false) {
        log::info!("[V2] force_lowest_quality=true, using Mp3");
        Quality::Mp3
    } else {
        final_quality
    };

    // Check streaming settings
    let (stream_first_enabled, streaming_only) = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        match guard.as_ref().and_then(|s| s.get_settings().ok()) {
            Some(s) => (s.stream_first_track, s.streaming_only),
            None => (false, false),
        }
    };

    // Determine hardware device ID for sample rate compatibility checks (ALSA only)
    #[cfg(target_os = "linux")]
    let hw_device_id: Option<String> = {
        let guard = audio_settings.store.lock().ok();
        guard
            .as_ref()
            .and_then(|g| g.as_ref())
            .and_then(|store| store.get_settings().ok())
            .and_then(|settings| {
                let is_alsa = matches!(
                    settings.backend_type,
                    Some(qbz_audio::AudioBackendType::Alsa)
                );
                if is_alsa {
                    settings.output_device.clone()
                } else {
                    None
                }
            })
    };
    #[cfg(not(target_os = "linux"))]
    let hw_device_id: Option<String> = None;

    log::info!(
        "[V2] play_track {} (quality_str={:?}, parsed={:?}, final={:?}, format_id={})",
        track_id,
        quality,
        preferred_quality,
        final_quality,
        final_quality.id()
    );

    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();

    // Fallback: if cache has lower quality than requested but network fails,
    // play the cached version rather than failing entirely.
    let mut low_quality_fallback: Option<Vec<u8>> = None;

    // Check offline cache (persistent disk cache)
    {
        let cached_path = {
            let db_opt = offline_cache.db.lock().await;
            if let Some(db) = db_opt.as_ref() {
                if let Ok(Some(file_path)) = db.get_file_path(track_id) {
                    let _ = db.touch(track_id);
                    Some(file_path)
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(file_path) = cached_path {
            let path = std::path::Path::new(&file_path);
            if path.exists() {
                log::info!(
                    "[V2/CACHE HIT] Track {} from OFFLINE cache: {:?}",
                    track_id,
                    path
                );
                let audio_data = std::fs::read(path).map_err(|e| {
                    RuntimeError::Internal(format!("Failed to read cached file: {}", e))
                })?;

                // Check hardware compatibility (ALSA only)
                #[cfg(target_os = "linux")]
                let hw_incompatible =
                    cached_audio_incompatible_with_hw(&audio_data, &audio_settings);
                #[cfg(not(target_os = "linux"))]
                let hw_incompatible = false;

                // Check quality mismatch (all platforms)
                let quality_mismatch = cached_quality_below_requested(&audio_data, final_quality);

                if hw_incompatible {
                    log::info!(
                        "[V2/Quality] Skipping OFFLINE cache for track {} - incompatible sample rate",
                        track_id
                    );
                } else if quality_mismatch {
                    // Keep as fallback — don't discard, network might fail
                    low_quality_fallback = Some(audio_data);
                } else {
                    player
                        .play_data(audio_data, track_id)
                        .map_err(RuntimeError::Internal)?;

                    let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                    drop(bridge_guard);
                    spawn_v2_prefetch(
                        bridge.0.clone(),
                        app_state.audio_cache.clone(),
                        upcoming_tracks,
                        final_quality,
                        streaming_only,
                    );
                    return Ok(V2PlayTrackResult { format_id: None });
                }
            }
        }
    }

    // Check memory cache (L1)
    let cache = app_state.audio_cache.clone();
    if low_quality_fallback.is_none() {
        if let Some(cached) = cache.get(track_id) {
            log::info!(
                "[V2/CACHE HIT] Track {} from MEMORY cache ({} bytes)",
                track_id,
                cached.size_bytes
            );

            #[cfg(target_os = "linux")]
            let hw_incompatible = cached_audio_incompatible_with_hw(&cached.data, &audio_settings);
            #[cfg(not(target_os = "linux"))]
            let hw_incompatible = false;

            let quality_mismatch = cached_quality_below_requested(&cached.data, final_quality);

            if hw_incompatible {
                log::info!(
                    "[V2/Quality] Skipping MEMORY cache for track {} - incompatible sample rate",
                    track_id
                );
            } else if quality_mismatch {
                low_quality_fallback = Some(cached.data);
            } else {
                player
                    .play_data(cached.data, track_id)
                    .map_err(RuntimeError::Internal)?;

                let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                drop(bridge_guard);
                spawn_v2_prefetch(
                    bridge.0.clone(),
                    cache.clone(),
                    upcoming_tracks,
                    final_quality,
                    streaming_only,
                );
                return Ok(V2PlayTrackResult { format_id: None });
            }
        }
    }

    // Check playback cache (L2 - disk)
    if low_quality_fallback.is_none() {
        if let Some(playback_cache) = cache.get_playback_cache() {
            if let Some(audio_data) = playback_cache.get(track_id) {
                log::info!(
                    "[V2/CACHE HIT] Track {} from DISK cache ({} bytes)",
                    track_id,
                    audio_data.len()
                );

                #[cfg(target_os = "linux")]
                let hw_incompatible =
                    cached_audio_incompatible_with_hw(&audio_data, &audio_settings);
                #[cfg(not(target_os = "linux"))]
                let hw_incompatible = false;

                let quality_mismatch =
                    cached_quality_below_requested(&audio_data, final_quality);

                if hw_incompatible {
                    log::info!(
                        "[V2/Quality] Skipping DISK cache for track {} - incompatible sample rate",
                        track_id
                    );
                } else if quality_mismatch {
                    low_quality_fallback = Some(audio_data);
                } else {
                    cache.insert(track_id, audio_data.clone());
                    player
                        .play_data(audio_data, track_id)
                        .map_err(RuntimeError::Internal)?;

                    let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                    drop(bridge_guard);
                    spawn_v2_prefetch(
                        bridge.0.clone(),
                        cache.clone(),
                        upcoming_tracks,
                        final_quality,
                        streaming_only,
                    );
                    return Ok(V2PlayTrackResult { format_id: None });
                }
            }
        }
    }

    // Not in cache (or cached at lower quality) — fetch from network
    if low_quality_fallback.is_some() {
        log::info!(
            "[V2] Track {} cached at lower quality, re-downloading at {:?}...",
            track_id, final_quality
        );
    } else {
        log::info!(
            "[V2] Track {} not in cache, fetching from network...",
            track_id
        );
    }

    // Try CMAF streaming pipeline (Akamai CDN, encrypted segments)
    // Only the init segment is fetched synchronously; audio segments stream in background.
    log::info!("[V2/CMAF] Attempting CMAF streaming for track {}", track_id);
    match try_cmaf_streaming_setup(&*bridge_guard, track_id, final_quality).await {
        Ok(cmaf_info) => {
            let format_id = cmaf_info.format_id;

            // Derive stream parameters from init segment and file URL metadata
            let sample_rate = cmaf_info.sampling_rate.unwrap_or(44100);
            let channels = 2u16; // FLAC from Qobuz is always stereo
            let bit_depth = cmaf_info.bit_depth.unwrap_or(16);
            let total_flac_size = cmaf_info.flac_header.len() as u64
                + cmaf_info
                    .segment_table
                    .iter()
                    .map(|s| s.byte_len as u64)
                    .sum::<u64>();

            // Estimate speed from init segment fetch (conservative: assume ~10 MB/s if
            // init was too fast to measure reliably)
            let speed_mbps = if cmaf_info.init_fetch_ms > 0 {
                let init_bytes = cmaf_info.flac_header.len() as f64 + 4096.0; // rough init size
                (init_bytes / (cmaf_info.init_fetch_ms as f64 / 1000.0)) / (1024.0 * 1024.0)
            } else {
                10.0
            };

            log::info!(
                "[V2/CMAF] Streaming setup: {}Hz, {}-bit, {:.2} MB total, {:.1} MB/s est, {} segments",
                sample_rate,
                bit_depth,
                total_flac_size as f64 / (1024.0 * 1024.0),
                speed_mbps,
                cmaf_info.n_segments
            );

            // Create streaming buffer and start playback immediately
            let buffer_writer = player
                .play_streaming_dynamic(
                    track_id,
                    sample_rate,
                    channels,
                    bit_depth,
                    total_flac_size,
                    speed_mbps,
                    duration_secs.unwrap_or(0),
                )
                .map_err(RuntimeError::Internal)?;

            // Spawn background task to fetch + decrypt + push audio segments
            let url_template = cmaf_info.url_template.clone();
            let content_key = cmaf_info.content_key;
            let flac_header = cmaf_info.flac_header;
            let n_segments = cmaf_info.n_segments;
            let cache_clone = cache.clone();
            let skip_cache = streaming_only;

            tokio::spawn(async move {
                match v2_cmaf_stream(
                    &url_template,
                    n_segments,
                    content_key,
                    flac_header,
                    buffer_writer,
                    track_id,
                    cache_clone,
                    skip_cache,
                )
                .await
                {
                    Ok(()) => {
                        if skip_cache {
                            log::info!(
                                "[V2/CMAF-STREAM COMPLETE] Track {} - NOT cached (streaming_only)",
                                track_id
                            );
                        } else {
                            log::info!(
                                "[V2/CMAF-STREAM COMPLETE] Track {} - cached for instant replay",
                                track_id
                            );
                        }
                    }
                    Err(e) => log::error!("[V2/CMAF-STREAM ERROR] Track {}: {}", track_id, e),
                }
            });

            // Prefetch next tracks in background
            let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
            drop(bridge_guard);
            spawn_v2_prefetch_with_hw_check(
                bridge.0.clone(),
                cache,
                upcoming_tracks,
                final_quality,
                streaming_only,
                hw_device_id,
            );

            return Ok(V2PlayTrackResult {
                format_id: Some(format_id),
            });
        }
        Err(e) => {
            log::warn!("[V2/CMAF] Streaming setup failed: {}, falling back to legacy download", e);
            // Fall through to existing legacy path
        }
    }

    let stream_url_result = bridge_guard
        .get_stream_url(track_id, final_quality)
        .await;

    let mut stream_url = match stream_url_result {
        Ok(url) => url,
        Err(e) => {
            // Network failed — use lower-quality fallback if available
            if let Some(fallback_data) = low_quality_fallback {
                log::warn!(
                    "[V2] Network failed for track {}: {}. Playing cached lower-quality version.",
                    track_id, e
                );
                player
                    .play_data(fallback_data, track_id)
                    .map_err(RuntimeError::Internal)?;
                let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                drop(bridge_guard);
                spawn_v2_prefetch(
                    bridge.0.clone(),
                    cache.clone(),
                    upcoming_tracks,
                    final_quality,
                    streaming_only,
                );
                return Ok(V2PlayTrackResult { format_id: None });
            }
            return Err(RuntimeError::Internal(e));
        }
    };
    log::info!(
        "[V2] Got stream URL for track {} (format_id={})",
        track_id,
        stream_url.format_id
    );

    // Smart quality downgrade for ALSA Direct: if the hardware doesn't support
    // the track's sample rate, re-request at a lower quality that IS supported.
    // This avoids resampling and keeps bit-perfect playback.
    if let Some(ref device_id) = hw_device_id {
        if final_quality == Quality::UltraHiRes {
            let track_rate = (stream_url.sampling_rate * 1000.0) as u32;
            if qbz_audio::device_supports_sample_rate(device_id, track_rate) == Some(false) {
                log::info!(
                    "[V2/Quality] Hardware doesn't support {}Hz, downgrading to Hi-Res",
                    track_rate
                );
                stream_url = bridge_guard
                    .get_stream_url(track_id, Quality::HiRes)
                    .await
                    .map_err(RuntimeError::Internal)?;
                log::info!(
                    "[V2/Quality] Got fallback stream URL (format_id={}, rate={}kHz)",
                    stream_url.format_id,
                    stream_url.sampling_rate
                );
            }
        }
    }

    if stream_first_enabled {
        // Streaming path: start playback before full download completes
        log::info!(
            "[V2/STREAMING] Track {} - streaming from network (cache_after: {})",
            track_id,
            !streaming_only
        );

        let stream_info = match v2_get_stream_info(&stream_url.url).await {
            Ok(info) => info,
            Err(e) => {
                // Probe failed (CDN EOF, etc.) — fall through to full download with backoff
                log::warn!(
                    "[V2/STREAMING] Probe failed for track {}: {}. Falling back to full download with backoff.",
                    track_id,
                    e
                );
                let effective_quality = Quality::from_id(stream_url.format_id)
                    .unwrap_or(final_quality);

                // Try CMAF full download first (Akamai CDN), fall back to legacy
                let audio_data = match try_cmaf_full_download(&*bridge_guard, track_id, effective_quality).await {
                    Ok(data) => {
                        log::info!("[V2/CMAF] Streaming-fallback download succeeded for track {}", track_id);
                        data
                    }
                    Err(cmaf_err) => {
                        log::warn!("[V2/CMAF] Streaming-fallback CMAF failed for track {}: {}, trying legacy", track_id, cmaf_err);
                        let (data, worked) = download_with_backoff(
                            &stream_url.url,
                            track_id,
                            effective_quality,
                            &*bridge_guard,
                        )
                        .await
                        .map_err(RuntimeError::Internal)?;
                        stream_url = worked;
                        data
                    }
                };

                let data_size = audio_data.len();
                if !streaming_only {
                    cache.insert(track_id, audio_data.clone());
                }
                player
                    .play_data(audio_data, track_id)
                    .map_err(RuntimeError::Internal)?;
                log::info!(
                    "[V2] Playing track {} ({} bytes, fallback from streaming)",
                    track_id,
                    data_size
                );
                let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                drop(bridge_guard);
                spawn_v2_prefetch_with_hw_check(
                    bridge.0.clone(),
                    cache,
                    upcoming_tracks,
                    final_quality,
                    streaming_only,
                    hw_device_id,
                );
                return Ok(V2PlayTrackResult {
                    format_id: Some(stream_url.format_id),
                });
            }
        };

        log::info!(
            "[V2/STREAMING] Info: {:.2} MB, {}Hz, {} ch, {}-bit, {:.1} MB/s",
            stream_info.content_length as f64 / (1024.0 * 1024.0),
            stream_info.sample_rate,
            stream_info.channels,
            stream_info.bit_depth,
            stream_info.speed_mbps
        );

        let buffer_writer = player
            .play_streaming_dynamic(
                track_id,
                stream_info.sample_rate,
                stream_info.channels,
                stream_info.bit_depth,
                stream_info.content_length,
                stream_info.speed_mbps,
                duration_secs.unwrap_or(0),
            )
            .map_err(RuntimeError::Internal)?;

        // Capture format_id before spawning background task
        let actual_format_id = stream_url.format_id;
        let url = stream_url.url.clone();
        let cache_clone = cache.clone();
        let content_len = stream_info.content_length;
        let skip_cache = streaming_only;

        // Spawn background download that feeds chunks to the player buffer
        tokio::spawn(async move {
            match v2_download_and_stream(
                &url,
                buffer_writer,
                track_id,
                cache_clone,
                content_len,
                skip_cache,
            )
            .await
            {
                Ok(()) => {
                    if skip_cache {
                        log::info!(
                            "[V2/STREAMING COMPLETE] Track {} - NOT cached (streaming_only)",
                            track_id
                        );
                    } else {
                        log::info!(
                            "[V2/STREAMING COMPLETE] Track {} - cached for instant replay",
                            track_id
                        );
                    }
                }
                Err(e) => log::error!("[V2/STREAMING ERROR] Track {}: {}", track_id, e),
            }
        });

        // Prefetch next tracks in background
        let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
        drop(bridge_guard);
        spawn_v2_prefetch_with_hw_check(
            bridge.0.clone(),
            cache,
            upcoming_tracks,
            final_quality,
            streaming_only,
            hw_device_id,
        );

        return Ok(V2PlayTrackResult {
            format_id: Some(actual_format_id),
        });
    }

    // Standard download path (streaming disabled) - full download before playback
    log::info!(
        "[V2/DOWNLOAD] Track {} - full download before playback (cache_after: {})",
        track_id,
        !streaming_only
    );
    let effective_quality = Quality::from_id(stream_url.format_id)
        .unwrap_or(final_quality);

    // Try CMAF full download first (Akamai CDN), fall back to legacy
    let audio_data = match try_cmaf_full_download(&*bridge_guard, track_id, effective_quality).await {
        Ok(data) => {
            log::info!("[V2/CMAF] Standard download succeeded for track {}", track_id);
            data
        }
        Err(cmaf_err) => {
            log::warn!("[V2/CMAF] Standard download CMAF failed for track {}: {}, trying legacy", track_id, cmaf_err);
            let (data, worked) = download_with_backoff(
                &stream_url.url,
                track_id,
                effective_quality,
                &*bridge_guard,
            )
            .await
            .map_err(RuntimeError::Internal)?;
            stream_url = worked;
            data
        }
    };

    let data_size = audio_data.len();

    // Cache it (unless streaming_only mode)
    if !streaming_only {
        cache.insert(track_id, audio_data.clone());
        log::info!("[V2/CACHED] Track {} stored in memory cache", track_id);
    } else {
        log::info!(
            "[V2/NOT CACHED] Track {} - streaming_only mode active",
            track_id
        );
    }

    // Play it via qbz-player
    player
        .play_data(audio_data, track_id)
        .map_err(RuntimeError::Internal)?;
    log::info!("[V2] Playing track {} ({} bytes)", track_id, data_size);

    // Prefetch next tracks in background (using CoreBridge queue)
    let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
    drop(bridge_guard);
    spawn_v2_prefetch_with_hw_check(
        bridge.0.clone(),
        cache,
        upcoming_tracks,
        final_quality,
        streaming_only,
        hw_device_id,
    );

    Ok(V2PlayTrackResult {
        format_id: Some(stream_url.format_id),
    })
}

// ==================== Audio Device Commands (V2) ====================

/// Reinitialize audio device (V2 - uses CoreBridge.player)
/// Call this when changing audio settings like exclusive mode or output device
#[tauri::command]
pub async fn v2_reinit_audio_device(
    device: Option<String>,
    bridge: State<'_, CoreBridgeState>,
    audio_settings: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Command: reinit_audio_device {:?}", device);

    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();

    // Reload settings from database to ensure Player has latest config
    if let Ok(guard) = audio_settings.store.lock() {
        if let Some(store) = guard.as_ref() {
            if let Ok(fresh_settings) = store.get_settings() {
                log::info!(
                    "[V2] Reloading audio settings before reinit (backend_type: {:?})",
                    fresh_settings.backend_type
                );
                let _ = player.reload_settings(convert_to_qbz_audio_settings(&fresh_settings));
            }
        }
    }

    player.reinit_device(device).map_err(RuntimeError::Internal)
}

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

// ==================== Audio Settings Commands (V2) ====================

/// Get current audio settings (V2)
#[tauri::command]
pub fn v2_get_audio_settings(
    state: State<'_, AudioSettingsState>,
) -> Result<AudioSettings, RuntimeError> {
    log::info!("[V2] get_audio_settings");
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store.get_settings().map_err(RuntimeError::Internal)
}

/// Set audio output device (V2)
#[tauri::command]
pub async fn v2_set_audio_output_device(
    device: Option<String>,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    let normalized_device = device
        .as_ref()
        .map(|d| crate::audio::normalize_device_id_to_stable(d));
    log::info!(
        "[V2] set_audio_output_device {:?} -> {:?} (normalized)",
        device,
        normalized_device
    );
    {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .set_output_device(normalized_device.as_deref())
            .map_err(RuntimeError::Internal)?;
    }
    sync_audio_settings_to_player(&state, &bridge).await;
    Ok(())
}

/// Set audio exclusive mode (V2)
#[tauri::command]
pub async fn v2_set_audio_exclusive_mode(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_exclusive_mode: {}", enabled);
    {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .set_exclusive_mode(enabled)
            .map_err(RuntimeError::Internal)?;
    }
    sync_audio_settings_to_player(&state, &bridge).await;
    Ok(())
}

/// Set DAC passthrough mode (V2)
#[tauri::command]
pub async fn v2_set_audio_dac_passthrough(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_dac_passthrough: {}", enabled);
    {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .set_dac_passthrough(enabled)
            .map_err(RuntimeError::Internal)?;
    }
    sync_audio_settings_to_player(&state, &bridge).await;
    Ok(())
}

/// Set PipeWire force bit-perfect mode (V2)
#[tauri::command]
pub async fn v2_set_audio_pw_force_bitperfect(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_pw_force_bitperfect: {}", enabled);
    {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .set_pw_force_bitperfect(enabled)
            .map_err(RuntimeError::Internal)?;
    }
    sync_audio_settings_to_player(&state, &bridge).await;
    Ok(())
}

/// Set sync audio settings on startup (V2)
#[tauri::command]
pub fn v2_set_sync_audio_on_startup(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_sync_audio_on_startup: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_sync_audio_on_startup(enabled)
        .map_err(RuntimeError::Internal)
}

/// Get quality fallback behavior (V2)
#[tauri::command]
pub fn v2_get_quality_fallback_behavior(
    audio_settings: State<'_, AudioSettingsState>,
) -> Result<String, RuntimeError> {
    let guard = audio_settings
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .get_quality_fallback_behavior()
        .map_err(RuntimeError::Internal)
}

/// Set quality fallback behavior (V2)
#[tauri::command]
pub fn v2_set_quality_fallback_behavior(
    behavior: String,
    audio_settings: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("Command: v2_set_quality_fallback_behavior {}", behavior);
    let guard = audio_settings
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_quality_fallback_behavior(&behavior)
        .map_err(RuntimeError::Internal)
}

/// Set preferred sample rate (V2)
#[tauri::command]
pub async fn v2_set_audio_sample_rate(
    rate: Option<u32>,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_sample_rate: {:?}", rate);
    {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store.set_sample_rate(rate).map_err(RuntimeError::Internal)?;
    }
    sync_audio_settings_to_player(&state, &bridge).await;
    Ok(())
}

/// Set audio backend type (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_set_audio_backend_type(
    backendType: Option<AudioBackendType>,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_backend_type: {:?}", backendType);
    {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .set_backend_type(backendType)
            .map_err(RuntimeError::Internal)?;
    }
    sync_audio_settings_to_player(&state, &bridge).await;
    Ok(())
}

/// Set ALSA plugin (V2)
#[tauri::command]
pub async fn v2_set_audio_alsa_plugin(
    plugin: Option<AlsaPlugin>,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_alsa_plugin: {:?}", plugin);
    {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .set_alsa_plugin(plugin)
            .map_err(RuntimeError::Internal)?;
    }
    sync_audio_settings_to_player(&state, &bridge).await;
    Ok(())
}

/// Set gapless playback enabled (V2)
#[tauri::command]
pub async fn v2_set_audio_gapless_enabled(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_gapless_enabled: {}", enabled);
    let fresh_settings = {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .set_gapless_enabled(enabled)
            .map_err(RuntimeError::Internal)?;
        store.get_settings().ok()
    }; // guard dropped here before .await

    // Sync to player immediately so gapless takes effect without restart
    if let Some(fresh) = fresh_settings {
        if let Some(b) = bridge.try_get().await {
            let _ = b
                .player()
                .reload_settings(convert_to_qbz_audio_settings(&fresh));
        }
    }
    Ok(())
}

/// Set allow quality fallback (V2)
#[tauri::command]
pub async fn v2_set_audio_allow_quality_fallback(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_allow_quality_fallback: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_allow_quality_fallback(enabled)
        .map_err(RuntimeError::Internal)?;
    Ok(())
}

/// Set skip sink switch (V2) — preserves JACK/qjackctl routing
#[tauri::command]
pub async fn v2_set_audio_skip_sink_switch(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_skip_sink_switch: {}", enabled);
    let fresh_settings = {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;

        // Constraint: cannot enable when dac_passthrough is on
        if enabled {
            let current = store.get_settings().map_err(RuntimeError::Internal)?;
            if current.dac_passthrough {
                return Err(RuntimeError::Internal(
                    "Cannot enable skip sink switch while DAC passthrough is active".to_string(),
                ));
            }
        }

        store
            .set_skip_sink_switch(enabled)
            .map_err(RuntimeError::Internal)?;
        store.get_settings().ok()
    };

    if let Some(fresh) = fresh_settings {
        if let Some(b) = bridge.try_get().await {
            let _ = b
                .player()
                .reload_settings(convert_to_qbz_audio_settings(&fresh));
        }
    }
    Ok(())
}

/// Set normalization enabled (V2)
#[tauri::command]
pub async fn v2_set_audio_normalization_enabled(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_normalization_enabled: {}", enabled);
    let fresh_settings = {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .set_normalization_enabled(enabled)
            .map_err(RuntimeError::Internal)?;
        store.get_settings().ok()
    };

    if let Some(fresh) = fresh_settings {
        if let Some(b) = bridge.try_get().await {
            let _ = b
                .player()
                .reload_settings(convert_to_qbz_audio_settings(&fresh));
        }
    }
    Ok(())
}

/// Set normalization target LUFS (V2)
#[tauri::command]
pub async fn v2_set_audio_normalization_target(
    target: f32,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_normalization_target: {}", target);
    let fresh_settings = {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .set_normalization_target_lufs(target)
            .map_err(RuntimeError::Internal)?;
        store.get_settings().ok()
    };

    if let Some(fresh) = fresh_settings {
        if let Some(b) = bridge.try_get().await {
            let _ = b
                .player()
                .reload_settings(convert_to_qbz_audio_settings(&fresh));
        }
    }
    Ok(())
}

/// Set device max sample rate (V2)
#[tauri::command]
pub fn v2_set_audio_device_max_sample_rate(
    rate: Option<u32>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_device_max_sample_rate: {:?}", rate);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_device_max_sample_rate(rate)
        .map_err(RuntimeError::Internal)
}

/// Set limit quality to device capability (V2)
#[tauri::command]
pub fn v2_set_audio_limit_quality_to_device(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_limit_quality_to_device: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_limit_quality_to_device(enabled)
        .map_err(RuntimeError::Internal)
}

/// Set streaming only mode (V2)
#[tauri::command]
pub fn v2_set_audio_streaming_only(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_streaming_only: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_streaming_only(enabled)
        .map_err(RuntimeError::Internal)
}

/// Reset audio settings to defaults (V2)
#[tauri::command]
pub async fn v2_reset_audio_settings(
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] reset_audio_settings");
    {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .reset_all()
            .map(|_| ())
            .map_err(RuntimeError::Internal)?;
    }
    sync_audio_settings_to_player(&state, &bridge).await;
    Ok(())
}

/// Set stream first track enabled (V2)
#[tauri::command]
pub fn v2_set_audio_stream_first_track(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_stream_first_track: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_stream_first_track(enabled)
        .map_err(RuntimeError::Internal)
}

/// Set stream buffer seconds (V2)
#[tauri::command]
pub fn v2_set_audio_stream_buffer_seconds(
    seconds: u8,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_stream_buffer_seconds: {}", seconds);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_stream_buffer_seconds(seconds)
        .map_err(RuntimeError::Internal)
}

/// Set ALSA hardware volume control (V2)
#[tauri::command]
pub async fn v2_set_audio_alsa_hardware_volume(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_alsa_hardware_volume: {}", enabled);
    {
        let guard = state
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        let store = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        store
            .set_alsa_hardware_volume(enabled)
            .map_err(RuntimeError::Internal)?;
    }
    sync_audio_settings_to_player(&state, &bridge).await;
    Ok(())
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

mod integrations;
pub use integrations::*;
// ==================== Playback Context Commands (V2) ====================

/// Get current playback context (V2)
#[tauri::command]
pub async fn v2_get_playback_context(
    app_state: State<'_, AppState>,
) -> Result<Option<crate::playback_context::PlaybackContext>, RuntimeError> {
    Ok(app_state.context.get_context())
}

/// Set playback context (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_set_playback_context(
    contextType: String,
    id: String,
    label: String,
    source: String,
    trackIds: Vec<u64>,
    startPosition: usize,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    use crate::playback_context::{ContentSource, ContextType, PlaybackContext};

    let ctx_type = match contextType.as_str() {
        "album" => ContextType::Album,
        "playlist" => ContextType::Playlist,
        "artist_top" => ContextType::ArtistTop,
        "label_top" => ContextType::LabelTop,
        "home_list" => ContextType::HomeList,
        "daily_q" => ContextType::DailyQ,
        "weekly_q" => ContextType::WeeklyQ,
        "fav_q" => ContextType::FavQ,
        "top_q" => ContextType::TopQ,
        "favorites" => ContextType::Favorites,
        "local_library" => ContextType::LocalLibrary,
        "radio" => ContextType::Radio,
        "search" => ContextType::Search,
        _ => {
            return Err(RuntimeError::Internal(format!(
                "Invalid context type: {}",
                contextType
            )))
        }
    };

    let content_source = match source.as_str() {
        "qobuz" => ContentSource::Qobuz,
        "local" => ContentSource::Local,
        "plex" => ContentSource::Plex,
        _ => {
            return Err(RuntimeError::Internal(format!(
                "Invalid source: {}",
                source
            )))
        }
    };

    let context =
        PlaybackContext::new(ctx_type, id, label, content_source, trackIds, startPosition);

    app_state.context.set_context(context);
    log::info!("[V2] set_playback_context: type={}", contextType);
    Ok(())
}

/// Clear playback context (V2)
#[tauri::command]
pub async fn v2_clear_playback_context(app_state: State<'_, AppState>) -> Result<(), RuntimeError> {
    app_state.context.clear_context();
    log::info!("[V2] clear_playback_context");
    Ok(())
}

/// Check if playback context is active (V2)
#[tauri::command]
pub async fn v2_has_playback_context(app_state: State<'_, AppState>) -> Result<bool, RuntimeError> {
    Ok(app_state.context.has_context())
}

// ==================== Session Persistence Commands (V2) ====================

/// Save session position (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_save_session_position(
    positionSecs: u64,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .save_position(positionSecs)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Save session volume (V2)
#[tauri::command]
pub async fn v2_save_session_volume(
    volume: f32,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .save_volume(volume)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Save session playback mode (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_save_session_playback_mode(
    shuffle: bool,
    repeatMode: String,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .save_playback_mode(shuffle, &repeatMode)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Save session state - full state (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_save_session_state(
    queueTracks: Vec<crate::session_store::PersistedQueueTrack>,
    currentIndex: Option<usize>,
    currentPositionSecs: u64,
    volume: f32,
    shuffleEnabled: bool,
    repeatMode: String,
    wasPlaying: bool,
    lastView: Option<String>,
    viewContextId: Option<String>,
    viewContextType: Option<String>,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let session = crate::session_store::PersistedSession {
        queue_tracks: queueTracks,
        current_index: currentIndex,
        current_position_secs: currentPositionSecs,
        volume,
        shuffle_enabled: shuffleEnabled,
        repeat_mode: repeatMode,
        was_playing: wasPlaying,
        saved_at: 0, // Will be set by save_session
        last_view: lastView.unwrap_or_else(|| "home".to_string()),
        view_context_id: viewContextId,
        view_context_type: viewContextType,
    };

    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .save_session(&session)
        .map_err(|e| RuntimeError::Internal(e))?;
    log::debug!(
        "[V2] save_session_state: index={:?} pos={}",
        currentIndex,
        currentPositionSecs
    );
    Ok(())
}

/// Load session state (V2)
#[tauri::command]
pub async fn v2_load_session_state(
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<crate::session_store::PersistedSession, RuntimeError> {
    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.load_session().map_err(|e| RuntimeError::Internal(e))
}

/// Clear session (V2)
#[tauri::command]
pub async fn v2_clear_session(
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .clear_session()
        .map_err(|e| RuntimeError::Internal(e))?;
    log::info!("[V2] clear_session");
    Ok(())
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


mod legacy_compat;
pub use legacy_compat::*;

// ==================== Utility Commands ====================

/// Fetch a remote URL as bytes (bypasses WebView CORS restrictions).
/// Used for loading PDF booklets from Qobuz CDN.
#[tauri::command]
pub async fn v2_fetch_url_bytes(url: String) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch URL: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}: {}", response.status(), url));
    }

    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("Failed to read response: {}", e))
}

// ============ Image Cache Commands ============

/// Download an image via reqwest (rustls) and write to a temp file.
/// Returns a file:// URL that WebKit can load without needing system TLS.
/// Used as fallback when the image cache service is unavailable.
async fn download_image_to_temp(url: &str) -> Result<String, String> {
    let url_owned = url.to_string();
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let response = reqwest::blocking::Client::new()
            .get(&url_owned)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .map_err(|e| format!("Failed to download image: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }

        response
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read image bytes: {}", e))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    // Write to temp dir with a hash-based filename to avoid duplicates
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        hasher.finish()
    };
    let tmp_dir = std::env::temp_dir().join("qbz-img-proxy");
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let tmp_path = tmp_dir.join(format!("{:x}.img", hash));
    std::fs::write(&tmp_path, &bytes)
        .map_err(|e| format!("Failed to write temp image: {}", e))?;

    Ok(format!("file://{}", tmp_path.display()))
}

#[tauri::command]
pub async fn v2_get_cached_image(
    url: String,
    cache_state: State<'_, crate::image_cache::ImageCacheState>,
    settings_state: State<'_, crate::config::ImageCacheSettingsState>,
) -> Result<String, String> {
    // Check if caching is enabled
    let settings = {
        let lock = settings_state
            .store
            .lock()
            .map_err(|e| format!("Settings lock error: {}", e))?;
        match lock.as_ref() {
            Some(store) => store.get_settings()?,
            None => crate::config::ImageCacheSettings::default(),
        }
    };

    if !settings.enabled {
        // Cache disabled — still proxy through reqwest so WebKit never
        // needs to resolve HTTPS (fixes AppImage TLS on some distros)
        return download_image_to_temp(&url).await;
    }

    // Check cache first
    {
        let lock = cache_state
            .service
            .lock()
            .map_err(|e| format!("Cache lock error: {}", e))?;
        if let Some(service) = lock.as_ref() {
            if let Some(path) = service.get(&url) {
                return Ok(format!("file://{}", path.display()));
            }
        }
    }

    // Download the image via reqwest (uses rustls — own CA bundle)
    let url_clone = url.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let response = reqwest::blocking::Client::new()
            .get(&url_clone)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .map_err(|e| format!("Failed to download image: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }

        response
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read image bytes: {}", e))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    // Store in cache and evict if needed
    let store_result = {
        let max_bytes = (settings.max_size_mb as u64) * 1024 * 1024;
        let lock = cache_state
            .service
            .lock()
            .map_err(|e| format!("Cache lock error: {}", e))?;
        if let Some(service) = lock.as_ref() {
            let path = service.store(&url, &bytes)?;
            let _ = service.evict(max_bytes);
            Some(format!("file://{}", path.display()))
        } else {
            None
        }
    }; // lock dropped here, before any .await

    match store_result {
        Some(path) => Ok(path),
        // Service not initialized — use temp file fallback
        None => download_image_to_temp(&url).await,
    }
}

#[tauri::command]
pub async fn v2_get_image_cache_settings(
    state: State<'_, crate::config::ImageCacheSettingsState>,
) -> Result<crate::config::ImageCacheSettings, String> {
    let lock = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    match lock.as_ref() {
        Some(store) => store.get_settings(),
        None => Ok(crate::config::ImageCacheSettings::default()),
    }
}

#[tauri::command]
pub async fn v2_set_image_cache_enabled(
    enabled: bool,
    state: State<'_, crate::config::ImageCacheSettingsState>,
) -> Result<(), String> {
    let lock = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    match lock.as_ref() {
        Some(store) => store.set_enabled(enabled),
        None => Err("Image cache settings not initialized".to_string()),
    }
}

#[tauri::command]
pub async fn v2_set_image_cache_max_size(
    max_size_mb: u32,
    state: State<'_, crate::config::ImageCacheSettingsState>,
    cache_state: State<'_, crate::image_cache::ImageCacheState>,
) -> Result<(), String> {
    {
        let lock = state
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        match lock.as_ref() {
            Some(store) => store.set_max_size_mb(max_size_mb)?,
            None => return Err("Image cache settings not initialized".to_string()),
        }
    }
    // Trigger eviction with new limit
    let max_bytes = (max_size_mb as u64) * 1024 * 1024;
    let lock = cache_state
        .service
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    if let Some(service) = lock.as_ref() {
        let _ = service.evict(max_bytes);
    }
    Ok(())
}

#[tauri::command]
pub async fn v2_get_image_cache_stats(
    state: State<'_, crate::image_cache::ImageCacheState>,
) -> Result<crate::image_cache::ImageCacheStats, String> {
    let lock = state
        .service
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    match lock.as_ref() {
        Some(service) => service.stats(),
        None => Ok(crate::image_cache::ImageCacheStats {
            total_bytes: 0,
            file_count: 0,
        }),
    }
}

#[tauri::command]
pub async fn v2_clear_image_cache(
    state: State<'_, crate::image_cache::ImageCacheState>,
    reco_state: State<'_, crate::reco_store::RecoState>,
) -> Result<u64, String> {
    let freed = {
        let lock = state
            .service
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        match lock.as_ref() {
            Some(service) => service.clear()?,
            None => 0,
        }
    };

    // Also clear reco meta image URLs so they re-resolve with correct sizes
    {
        let guard__ = reco_state.db.lock().await;
        if let Some(db) = guard__.as_ref() {
            let _ = db.clear_meta_caches();
        }
    }

    Ok(freed)
}

// ==================== ListenBrainz Discovery ====================

/// Normalize an artist name for dedup: trim, lowercase, collapse whitespace
pub(crate) fn normalize_artist_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Discover new artists via MusicBrainz tag-based search.
///
/// Pipeline: "Listeners also enjoy"
///
/// Uses MusicBrainz tag search to find artists that share the seed artist's
/// primary genre tag. This gives genre-accurate results (e.g., searching
/// "thrash metal" for Metallica returns Megadeth, Slayer, Anthrax — not
/// mainstream crossover like Led Zeppelin).
///
/// Pipeline:
/// 1. Fetch seed artist's tags from MusicBrainz (sorted by vote count)
/// 2. Search MB for artists tagged with the primary genre tag
/// 3. Filter: seed artist, known similar artists, local listening history
/// 4. Resolve on Qobuz (verify exact name match to avoid homonyms)
/// 5. Return top 8, minimum 5 (frontend shows 6, keeps 2 reserves)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryArtist {
    pub mbid: String,
    pub name: String,
    pub normalized_name: String,
    pub affinity_score: f64,
    pub similarity_percent: f64,
    pub qobuz_id: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResponse {
    pub artists: Vec<DiscoveryArtist>,
    pub primary_tag: String,
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_discovery_artists(
    seedMbid: String,
    seedArtistName: String,
    similarArtistNames: Vec<String>,
    musicbrainz: State<'_, MusicBrainzV2State>,
    reco_state: State<'_, RecoState>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<DiscoveryResponse, String> {
    log::info!(
        "[Discovery] Starting pipeline for {} (MBID: {})",
        seedArtistName,
        seedMbid
    );

    // Step 1: Check MB is enabled
    {
        let client = musicbrainz.client.lock().await;
        if !client.is_enabled().await {
            log::warn!("[Discovery] MusicBrainz is disabled, returning empty");
            return Ok(DiscoveryResponse {
                artists: Vec::new(),
                primary_tag: String::new(),
            });
        }
    }

    // Step 2: Get seed artist's primary genre tag
    let seed_tags = {
        let client = musicbrainz.client.lock().await;
        client.get_artist_tags(&seedMbid).await.unwrap_or_default()
    };

    if seed_tags.is_empty() {
        log::warn!("[Discovery] No tags found for seed artist, returning empty");
        return Ok(DiscoveryResponse {
            artists: Vec::new(),
            primary_tag: String::new(),
        });
    }

    let primary_tag = &seed_tags[0];
    log::info!(
        "[Discovery] Seed primary tag: '{}' (from {} tags total)",
        primary_tag,
        seed_tags.len()
    );

    // Step 3: Search MB for artists with the same primary tag
    // Request more than we need to account for filtering
    let mb_results = {
        let client = musicbrainz.client.lock().await;
        client
            .search_artists_by_tag(primary_tag, 50)
            .await
            .map_err(|e| format!("Tag search failed: {}", e))?
    };

    log::info!(
        "[Discovery] MB tag search returned {} artists for '{}'",
        mb_results.artists.len(),
        primary_tag
    );

    if mb_results.artists.is_empty() {
        return Ok(DiscoveryResponse {
            artists: Vec::new(),
            primary_tag: primary_tag.to_string(),
        });
    }

    // Step 4: Build exclusion sets
    let seed_name_normalized = normalize_artist_name(&seedArtistName);

    let similar_names_set: HashSet<String> = similarArtistNames
        .iter()
        .map(|name| normalize_artist_name(name))
        .collect();

    // Exclude any artist listened more than 2 times (user already knows them)
    let listen_threshold: u32 = 2;
    let (local_known_qobuz_ids, local_known_names): (HashSet<u64>, HashSet<String>) = {
        let guard = reco_state.db.lock().await;
        if let Some(db) = guard.as_ref() {
            let top_artists = db.get_top_artist_ids(500).unwrap_or_default();
            let qobuz_ids: HashSet<u64> = top_artists
                .iter()
                .filter(|a| a.play_count > listen_threshold)
                .map(|a| a.artist_id)
                .collect();

            let known_artists = db.get_known_artist_names(1000).unwrap_or_default();
            let known_ids: HashSet<u64> = qobuz_ids.clone();
            let names: HashSet<String> = known_artists
                .iter()
                .filter(|(id, _)| known_ids.contains(id))
                .map(|(_, name)| normalize_artist_name(name))
                .collect();

            log::debug!(
                "[Discovery] Exclusion: {} known artists (>{} plays)",
                qobuz_ids.len(),
                listen_threshold
            );

            (qobuz_ids, names)
        } else {
            (HashSet::new(), HashSet::new())
        }
    };

    // Step 4b: Load dismissed artists for this tag
    let dismissed_names: HashSet<String> = {
        let guard = reco_state.db.lock().await;
        if let Some(db) = guard.as_ref() {
            db.get_dismissed_artists_for_tag(&primary_tag.to_lowercase())
                .unwrap_or_default()
                .into_iter()
                .collect()
        } else {
            HashSet::new()
        }
    };

    if !dismissed_names.is_empty() {
        log::debug!(
            "[Discovery] {} dismissed artists for tag '{}'",
            dismissed_names.len(),
            primary_tag
        );
    }

    // Step 5: Filter MB results
    let mut candidates: Vec<(String, String)> = Vec::new(); // (mbid, name)

    for artist in &mb_results.artists {
        let normalized = normalize_artist_name(&artist.name);

        // Skip seed artist
        if normalized == seed_name_normalized || artist.id.to_lowercase() == seedMbid.to_lowercase()
        {
            continue;
        }
        // Skip artists already shown in the similar section
        if similar_names_set.contains(&normalized) {
            continue;
        }
        // Skip locally known artists
        if local_known_names.contains(&normalized) {
            continue;
        }
        // Skip dismissed artists for this tag
        if dismissed_names.contains(&normalized) {
            continue;
        }
        candidates.push((artist.id.clone(), artist.name.clone()));
    }

    // Step 6: Shuffle deterministically using seed MBID
    // This ensures: same artist page = same results, different artist = different results
    {
        use rand::seq::SliceRandom;
        use rand::SeedableRng;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        seedMbid.hash(&mut hasher);
        let hash = hasher.finish();
        let mut rng = rand::rngs::StdRng::seed_from_u64(hash);
        candidates.shuffle(&mut rng);
    }

    log::info!(
        "[Discovery] {} candidates after filtering + shuffle (from {} MB results)",
        candidates.len(),
        mb_results.artists.len()
    );

    // Step 7: Resolve on Qobuz
    let bridge_guard = bridge.try_get().await;
    let mut results: Vec<DiscoveryArtist> = Vec::new();
    let min_results = 5;
    let max_results = 8;

    if let Some(ref core_bridge) = bridge_guard {
        for (mbid, name) in &candidates {
            if results.len() >= max_results {
                break;
            }

            let qobuz_artist = match core_bridge.search_artists(name, 1, 0, None).await {
                Ok(search_results) => {
                    if let Some(artist) = search_results.items.first() {
                        let qobuz_norm = normalize_artist_name(&artist.name);
                        let cand_norm = normalize_artist_name(name);
                        if qobuz_norm == cand_norm
                            && !local_known_qobuz_ids.contains(&artist.id)
                            && !blacklist_state.is_blacklisted(artist.id)
                        {
                            Some((artist.id, artist.name.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                Err(_) => None,
            };

            if let Some((qobuz_id, qobuz_name)) = qobuz_artist {
                results.push(DiscoveryArtist {
                    mbid: mbid.to_string(),
                    name: qobuz_name.clone(),
                    normalized_name: normalize_artist_name(&qobuz_name),
                    affinity_score: 0.0,
                    similarity_percent: 0.0,
                    qobuz_id: Some(qobuz_id),
                });
            }
        }
    } else {
        log::warn!("[Discovery] CoreBridge not available");
        return Ok(DiscoveryResponse {
            artists: Vec::new(),
            primary_tag: primary_tag.to_string(),
        });
    }

    // Step 7: If not enough results with primary tag, try secondary tag
    if results.len() < min_results && seed_tags.len() > 1 {
        let secondary_tag = &seed_tags[1];
        log::info!(
            "[Discovery] Only {} results, trying secondary tag: '{}'",
            results.len(),
            secondary_tag
        );

        // Load dismissals for secondary tag too
        let secondary_dismissed: HashSet<String> = {
            let guard = reco_state.db.lock().await;
            if let Some(db) = guard.as_ref() {
                db.get_dismissed_artists_for_tag(&secondary_tag.to_lowercase())
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            } else {
                HashSet::new()
            }
        };

        let secondary_search = {
            let client = musicbrainz.client.lock().await;
            client.search_artists_by_tag(secondary_tag, 30).await
        };
        if let Ok(secondary_results) = secondary_search {
            let existing_mbids: HashSet<String> = results.iter().map(|r| r.mbid.clone()).collect();

            // Filter and shuffle secondary candidates too
            let mut secondary_candidates: Vec<(String, String)> = Vec::new();
            for artist in &secondary_results.artists {
                let normalized = normalize_artist_name(&artist.name);
                if normalized == seed_name_normalized
                    || artist.id.to_lowercase() == seedMbid.to_lowercase()
                {
                    continue;
                }
                if similar_names_set.contains(&normalized)
                    || local_known_names.contains(&normalized)
                    || dismissed_names.contains(&normalized)
                    || secondary_dismissed.contains(&normalized)
                {
                    continue;
                }
                if existing_mbids.contains(&artist.id) {
                    continue;
                }
                secondary_candidates.push((artist.id.clone(), artist.name.clone()));
            }

            {
                use rand::seq::SliceRandom;
                use rand::SeedableRng;
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};

                let mut hasher = DefaultHasher::new();
                seedMbid.hash(&mut hasher);
                secondary_tag.hash(&mut hasher);
                let hash = hasher.finish();
                let mut rng = rand::rngs::StdRng::seed_from_u64(hash);
                secondary_candidates.shuffle(&mut rng);
            }

            if let Some(ref core_bridge) = bridge_guard {
                for (mbid, name) in &secondary_candidates {
                    if results.len() >= max_results {
                        break;
                    }

                    let qobuz_artist = match core_bridge.search_artists(name, 1, 0, None).await {
                        Ok(sr) => {
                            if let Some(qa) = sr.items.first() {
                                let qobuz_norm = normalize_artist_name(&qa.name);
                                let cand_norm = normalize_artist_name(name);
                                if qobuz_norm == cand_norm
                                    && !local_known_qobuz_ids.contains(&qa.id)
                                    && !blacklist_state.is_blacklisted(qa.id)
                                {
                                    Some((qa.id, qa.name.clone()))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    };

                    if let Some((qobuz_id, qobuz_name)) = qobuz_artist {
                        results.push(DiscoveryArtist {
                            mbid: mbid.clone(),
                            name: qobuz_name.clone(),
                            normalized_name: normalize_artist_name(&qobuz_name),
                            affinity_score: 0.0,
                            similarity_percent: 0.0,
                            qobuz_id: Some(qobuz_id),
                        });
                    }
                }
            }
        }
    }

    log::info!("[Discovery] Returning {} discovery artists", results.len());
    Ok(DiscoveryResponse {
        artists: results,
        primary_tag: primary_tag.to_string(),
    })
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_dismiss_discovery_artist(
    tag: String,
    artistName: String,
    reco_state: State<'_, RecoState>,
) -> Result<(), String> {
    let normalized = normalize_artist_name(&artistName);
    let tag_lower = tag.to_lowercase();

    log::info!(
        "[Discovery] Dismissing '{}' for tag '{}'",
        normalized,
        tag_lower
    );

    let guard = reco_state.db.lock().await;
    if let Some(db) = guard.as_ref() {
        db.dismiss_discovery_artist(&tag_lower, &normalized)?;
    }
    Ok(())
}

// ==================== Runtime Diagnostics ====================

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostics {
    // Audio: saved settings
    pub audio_output_device: Option<String>,
    pub audio_backend_type: Option<String>,
    pub audio_exclusive_mode: bool,
    pub audio_dac_passthrough: bool,
    pub audio_preferred_sample_rate: Option<u32>,
    pub audio_alsa_plugin: Option<String>,
    pub audio_alsa_hardware_volume: bool,
    pub audio_normalization_enabled: bool,
    pub audio_normalization_target_lufs: f32,
    pub audio_gapless_enabled: bool,
    pub audio_pw_force_bitperfect: bool,
    pub audio_stream_buffer_seconds: u8,
    pub audio_streaming_only: bool,

    // Graphics: saved settings
    pub gfx_hardware_acceleration: bool,
    pub gfx_force_x11: bool,
    pub gfx_gdk_scale: Option<String>,
    pub gfx_gdk_dpi_scale: Option<String>,
    pub gfx_gsk_renderer: Option<String>,

    // Graphics: runtime (what actually applied at startup)
    pub runtime_using_fallback: bool,
    pub runtime_is_wayland: bool,
    pub runtime_has_nvidia: bool,
    pub runtime_has_amd: bool,
    pub runtime_has_intel: bool,
    pub runtime_is_vm: bool,
    pub runtime_hw_accel_enabled: bool,
    pub runtime_force_x11_active: bool,

    // Developer settings
    pub dev_force_dmabuf: bool,

    // Environment variables (what WebKit actually sees)
    pub env_webkit_disable_dmabuf: Option<String>,
    pub env_webkit_disable_compositing: Option<String>,
    pub env_gdk_backend: Option<String>,
    pub env_gsk_renderer: Option<String>,
    pub env_libgl_always_software: Option<String>,
    pub env_wayland_display: Option<String>,
    pub env_xdg_session_type: Option<String>,

    // App info
    pub app_version: String,
}

#[tauri::command]
pub fn v2_get_runtime_diagnostics(
    audio_state: State<'_, AudioSettingsState>,
    graphics_state: State<'_, GraphicsSettingsState>,
    developer_state: State<'_, DeveloperSettingsState>,
) -> Result<RuntimeDiagnostics, RuntimeError> {
    // Audio settings (may not be available before login)
    let audio = audio_state
        .store
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()));

    // Graphics settings
    let gfx = graphics_state
        .store
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()));

    // Graphics runtime status (static atomics — always available)
    let gfx_status = crate::config::graphics_settings::get_graphics_startup_status();

    // Developer settings
    let dev = developer_state
        .store
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()));

    let env_var = |name: &str| std::env::var(name).ok();

    let audio_defaults = crate::config::audio_settings::AudioSettings::default();
    let audio = audio.unwrap_or(audio_defaults);
    let gfx = gfx.unwrap_or_default();
    let dev = dev.unwrap_or_default();

    Ok(RuntimeDiagnostics {
        audio_output_device: audio.output_device,
        audio_backend_type: audio.backend_type.map(|b| format!("{:?}", b)),
        audio_exclusive_mode: audio.exclusive_mode,
        audio_dac_passthrough: audio.dac_passthrough,
        audio_preferred_sample_rate: audio.preferred_sample_rate,
        audio_alsa_plugin: audio.alsa_plugin.map(|p| format!("{:?}", p)),
        audio_alsa_hardware_volume: audio.alsa_hardware_volume,
        audio_normalization_enabled: audio.normalization_enabled,
        audio_normalization_target_lufs: audio.normalization_target_lufs,
        audio_gapless_enabled: audio.gapless_enabled,
        audio_pw_force_bitperfect: audio.pw_force_bitperfect,
        audio_stream_buffer_seconds: audio.stream_buffer_seconds,
        audio_streaming_only: audio.streaming_only,

        gfx_hardware_acceleration: gfx.hardware_acceleration,
        gfx_force_x11: gfx.force_x11,
        gfx_gdk_scale: gfx.gdk_scale,
        gfx_gdk_dpi_scale: gfx.gdk_dpi_scale,
        gfx_gsk_renderer: gfx.gsk_renderer,

        runtime_using_fallback: gfx_status.using_fallback,
        runtime_is_wayland: gfx_status.is_wayland,
        runtime_has_nvidia: gfx_status.has_nvidia,
        runtime_has_amd: gfx_status.has_amd,
        runtime_has_intel: gfx_status.has_intel,
        runtime_is_vm: gfx_status.is_vm,
        runtime_hw_accel_enabled: gfx_status.hardware_accel_enabled,
        runtime_force_x11_active: gfx_status.force_x11_active,

        dev_force_dmabuf: dev.force_dmabuf,

        env_webkit_disable_dmabuf: env_var("WEBKIT_DISABLE_DMABUF_RENDERER"),
        env_webkit_disable_compositing: env_var("WEBKIT_DISABLE_COMPOSITING_MODE"),
        env_gdk_backend: env_var("GDK_BACKEND"),
        env_gsk_renderer: env_var("GSK_RENDERER"),
        env_libgl_always_software: env_var("LIBGL_ALWAYS_SOFTWARE"),
        env_wayland_display: env_var("WAYLAND_DISPLAY"),
        env_xdg_session_type: env_var("XDG_SESSION_TYPE"),

        app_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
