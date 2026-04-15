use tauri::State;

use qbz_models::UserSession;

use crate::config::legal_settings::LegalSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::runtime::{RuntimeError, RuntimeManagerState};
use crate::AppState;

use super::helpers::{accept_tos_best_effort, rollback_auth_state};

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
