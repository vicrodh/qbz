// ==================== Runtime Contract Commands ====================

use std::sync::Arc;
use tauri::{Emitter, Manager, State};

use qbz_models::UserSession;

use crate::config::audio_settings::AudioSettingsState;
use crate::config::legal_settings::LegalSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::runtime::{
    DegradedReason, RuntimeError, RuntimeEvent, RuntimeManagerState, RuntimeStatus,
};
use crate::AppState;

use super::helpers::{accept_tos_best_effort, convert_to_qbz_audio_settings, rollback_auth_state};

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
    qconnect_cli_override: State<'_, crate::qconnect::startup::QconnectCliOverride>,
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

                // QConnect auto-connect-on-startup. Only fires here, after a successful
                // OAuth restore + session activation, because service.connect requires
                // a fully initialized client.
                crate::qconnect::startup::maybe_auto_connect_after_bootstrap(
                    &app,
                    qconnect_cli_override.0,
                )
                .await;
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
/// Steps: exchange code -> establish session -> inject into CoreBridge ->
/// activate per-user data -> persist token.
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

    // Convert api::models::UserSession -> qbz_models::UserSession for CoreBridge
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
