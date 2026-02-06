//! Authentication commands

use tauri::State;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::credentials;
use crate::AppState;
use crate::api::error::ApiError;
use crate::config::SubscriptionStateState;
use crate::offline_cache::OfflineCacheState;

#[derive(serde::Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub user_name: Option<String>,
    pub user_id: Option<u64>,
    pub subscription: Option<String>,
    pub subscription_valid_until: Option<String>,
    pub error: Option<String>,
    pub error_code: Option<String>,
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

async fn maybe_purge_offline_cache_for_invalid_subscription(
    now: i64,
    subscription_state: &SubscriptionStateState,
    cache_state: &OfflineCacheState,
    library_state: &crate::library::commands::LibraryState,
) {
    let should_purge = {
        let store = match subscription_state.lock() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Subscription state lock error: {}", e);
                return;
            }
        };
        match store.as_ref().and_then(|s| s.should_purge_offline_cache(now).ok()) {
            Some(v) => v,
            None => {
                log::warn!("Failed to evaluate purge condition");
                false
            }
        }
    };

    if !should_purge {
        return;
    }

    log::warn!("Subscription invalid for >3 days. Purging offline cache.");
    if let Err(e) = crate::offline_cache::commands::purge_all_cached_files(cache_state, library_state).await {
        log::error!("Failed to purge offline cache: {}", e);
        return;
    }

    if let Ok(guard) = subscription_state.lock() {
        if let Some(store) = guard.as_ref() {
            if let Err(e) = store.mark_offline_cache_purged(now) {
                log::warn!("Failed to persist purge timestamp: {}", e);
            }
        }
    }
}

#[tauri::command]
pub async fn login(
    email: String,
    password: String,
    state: State<'_, AppState>,
    subscription_state: State<'_, SubscriptionStateState>,
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
) -> Result<LoginResponse, String> {
    let client = state.client.lock().await;
    let now = now_unix_secs();

    match client.login(&email, &password).await {
        Ok(session) => {
            if let Ok(guard) = subscription_state.lock() {
                if let Some(store) = guard.as_ref() {
                    let _ = store.mark_valid(now);
                }
            }
            Ok(LoginResponse {
                success: true,
                user_name: Some(session.display_name),
                user_id: Some(session.user_id),
                subscription: Some(session.subscription_label),
                subscription_valid_until: session.subscription_valid_until,
                error: None,
                error_code: None,
            })
        }
        Err(ApiError::IneligibleUser) => {
            if let Ok(guard) = subscription_state.lock() {
                if let Some(store) = guard.as_ref() {
                    let _ = store.mark_invalid(now);
                }
            }
            maybe_purge_offline_cache_for_invalid_subscription(
                now,
                subscription_state.inner(),
                cache_state.inner(),
                library_state.inner(),
            )
            .await;
            Ok(LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some("No active subscription".to_string()),
                error_code: Some("ineligible_user".to_string()),
            })
        }
        Err(e) => Ok(LoginResponse {
            success: false,
            user_name: None,
            user_id: None,
            subscription: None,
            subscription_valid_until: None,
            error: Some(e.to_string()),
            error_code: None,
        }),
    }
}

#[tauri::command]
pub async fn is_logged_in(state: State<'_, AppState>) -> Result<bool, String> {
    let client = state.client.lock().await;
    Ok(client.is_logged_in().await)
}

#[tauri::command]
pub async fn init_client(state: State<'_, AppState>) -> Result<bool, String> {
    let client = state.client.lock().await;
    match client.init().await {
        Ok(_) => Ok(true),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    let client = state.client.lock().await;
    client.logout().await;
    Ok(())
}

#[derive(serde::Serialize)]
pub struct UserInfo {
    pub user_name: String,
    pub subscription: String,
    pub subscription_valid_until: Option<String>,
}

#[tauri::command]
pub async fn get_user_info(state: State<'_, AppState>) -> Result<Option<UserInfo>, String> {
    let client = state.client.lock().await;
    Ok(client.get_user_info().await.map(|(name, sub, until)| UserInfo {
        user_name: name,
        subscription: sub,
        subscription_valid_until: until,
    }))
}

// === Credential persistence commands ===

/// Check if saved credentials exist in system keyring
#[tauri::command]
pub fn has_saved_credentials() -> bool {
    credentials::has_saved_credentials()
}

/// Save credentials to system keyring
#[tauri::command]
pub fn save_credentials(email: String, password: String) -> Result<(), String> {
    credentials::save_qobuz_credentials(&email, &password)
}

/// Clear saved credentials from system keyring
#[tauri::command]
pub fn clear_saved_credentials() -> Result<(), String> {
    credentials::clear_qobuz_credentials()
}

/// Auto-login using saved credentials
/// Returns LoginResponse with success status
#[tauri::command]
pub async fn auto_login(
    state: State<'_, AppState>,
    subscription_state: State<'_, SubscriptionStateState>,
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
) -> Result<LoginResponse, String> {
    // Check for saved credentials
    let creds = match credentials::load_qobuz_credentials() {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Ok(LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some("No saved credentials".to_string()),
                error_code: None,
            });
        }
        Err(e) => {
            return Ok(LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some(e),
                error_code: None,
            });
        }
    };

    // Try to login with saved credentials
    let client = state.client.lock().await;
    let now = now_unix_secs();
    match client.login(&creds.email, &creds.password).await {
        Ok(session) => {
            if let Ok(guard) = subscription_state.lock() {
                if let Some(store) = guard.as_ref() {
                    let _ = store.mark_valid(now);
                }
            }
            Ok(LoginResponse {
                success: true,
                user_name: Some(session.display_name),
                user_id: Some(session.user_id),
                subscription: Some(session.subscription_label),
                subscription_valid_until: session.subscription_valid_until,
                error: None,
                error_code: None,
            })
        }
        Err(ApiError::IneligibleUser) => {
            if let Ok(guard) = subscription_state.lock() {
                if let Some(store) = guard.as_ref() {
                    let _ = store.mark_invalid(now);
                }
            }
            maybe_purge_offline_cache_for_invalid_subscription(
                now,
                subscription_state.inner(),
                cache_state.inner(),
                library_state.inner(),
            )
            .await;
            Ok(LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some("No active subscription".to_string()),
                error_code: Some("ineligible_user".to_string()),
            })
        }
        Err(e) => {
            // Credentials might be invalid, but don't clear them automatically
            // Let the user decide
            log::warn!("Auto-login failed: {}", e);
            Ok(LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some(e.to_string()),
                error_code: None,
            })
        }
    }
}

#[tauri::command]
pub async fn set_api_locale(locale: String, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: set_api_locale {}", locale);
    let client = state.client.lock().await;
    client.set_locale(locale).await;
    Ok(())
}
