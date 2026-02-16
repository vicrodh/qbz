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
    GenreInfo, LabelDetail, PageArtistResponse, Playlist, PlaylistTag, Quality, QueueState,
    QueueTrack as CoreQueueTrack, RepeatMode, SearchResultsPage, Track, UserSession,
};

use crate::artist_blacklist::BlacklistState;
use crate::cache::AudioCache;
use crate::audio::{AlsaPlugin, AudioBackendType};
use crate::config::audio_settings::{AudioSettings, AudioSettingsState};
use crate::config::legal_settings::LegalSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::offline_cache::OfflineCacheState;
use crate::runtime::{RuntimeManagerState, RuntimeStatus, RuntimeError, RuntimeEvent, DegradedReason, CommandRequirement};
use crate::AppState;

// ==================== Helper Functions ====================

/// Check that Qobuz ToS has been accepted before allowing login.
///
/// This is the single enforcement point for ToS gate in backend.
/// All login commands MUST call this before authenticating.
///
/// Returns Ok(()) if ToS accepted, Err with specific error code if not.
fn require_tos_accepted(legal_state: &LegalSettingsState) -> Result<(), (String, String)> {
    let guard = legal_state.lock().map_err(|e| {
        ("tos_check_failed".to_string(), format!("Lock error: {}", e))
    })?;

    let tos_accepted = guard
        .as_ref()
        .and_then(|store| store.get_settings().ok())
        .map(|s| s.qobuz_tos_accepted)
        .unwrap_or(false);

    if !tos_accepted {
        return Err((
            "tos_not_accepted".to_string(),
            "Qobuz Terms of Service must be accepted before login".to_string(),
        ));
    }

    Ok(())
}

/// Rollback runtime auth state after a partial login failure.
///
/// This MUST be called when:
/// - Legacy auth succeeded but CoreBridge auth failed
/// - Legacy + CoreBridge auth succeeded but session activation failed
///
/// Ensures runtime_get_status never reports a half-authenticated state.
async fn rollback_auth_state(
    manager: &crate::runtime::RuntimeManager,
    app: &tauri::AppHandle,
) {
    log::warn!("[V2] Rolling back auth state after partial login failure");
    manager.set_legacy_auth(false, None).await;
    manager.set_corebridge_auth(false).await;
    manager.set_session_activated(false, 0).await;
    let _ = app.emit("runtime:event", RuntimeEvent::AuthChanged {
        logged_in: false,
        user_id: None,
    });
}

/// Convert quality string from frontend to Quality enum
fn parse_quality(quality_str: Option<&str>) -> Quality {
    match quality_str {
        Some("MP3") => Quality::Mp3,
        Some("CD Quality") => Quality::Lossless,
        Some("Hi-Res") => Quality::HiRes,
        Some("Hi-Res+") => Quality::UltraHiRes,
        _ => Quality::UltraHiRes, // Default to highest
    }
}

/// Limit quality based on device's max sample rate
fn limit_quality_for_device(quality: Quality, max_sample_rate: Option<u32>) -> Quality {
    let Some(max_rate) = max_sample_rate else {
        return quality;
    };

    if max_rate <= 48000 {
        match quality {
            Quality::UltraHiRes | Quality::HiRes => {
                log::info!(
                    "[V2/Quality Limit] Device max {}Hz, limiting {} to Lossless (44.1kHz)",
                    max_rate, quality.label()
                );
                Quality::Lossless
            }
            _ => quality,
        }
    } else if max_rate <= 96000 {
        match quality {
            Quality::UltraHiRes => {
                log::info!(
                    "[V2/Quality Limit] Device max {}Hz, limiting Hi-Res+ to Hi-Res (96kHz)",
                    max_rate
                );
                Quality::HiRes
            }
            _ => quality,
        }
    } else {
        quality
    }
}

/// Download audio from URL
async fn download_audio(url: &str) -> Result<Vec<u8>, String> {
    use std::time::Duration;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    log::info!("[V2] Downloading audio...");

    let response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch audio: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read audio bytes: {}", e))?;

    log::info!("[V2] Downloaded {} bytes", bytes.len());
    Ok(bytes.to_vec())
}

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
                let _ = app.emit("runtime:event", RuntimeEvent::RuntimeDegraded { reason: reason.clone() });
                return Err(RuntimeError::RuntimeDegraded(reason));
            }
        }
    }

    // Step 2: Check ToS acceptance BEFORE attempting auto-login
    // ToS gate is enforced: if not accepted, skip auto-login entirely
    let tos_accepted: bool = {
        let legal_state = app.state::<crate::config::legal_settings::LegalSettingsState>();
        let guard = legal_state.lock();
        match guard {
            Ok(ref g) => {
                if let Some(store) = g.as_ref() {
                    store.get_settings().map(|s| s.qobuz_tos_accepted).unwrap_or(false)
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    };

    if !tos_accepted {
        log::info!("[Runtime] ToS not accepted, skipping auto-login. User must accept ToS first.");
        manager.set_bootstrap_in_progress(false).await;
        let status = manager.get_status().await;
        log::info!("[Runtime] Bootstrap complete (ToS gate): {:?}", status.state);
        return Ok(status);
    }

    // Step 3: Check for saved credentials and attempt auto-login
    let creds = crate::credentials::load_qobuz_credentials();
    let last_user_id = crate::user_data::UserDataPaths::load_last_user_id();

    if let (Ok(Some(creds)), Some(user_id)) = (creds, last_user_id) {
        log::info!("[Runtime] Found saved credentials, attempting auto-login for user {}", user_id);

        // Login to legacy client
        let client = app_state.client.read().await;
        match client.login(&creds.email, &creds.password).await {
            Ok(session) => {
                log::info!("[Runtime] Legacy auth successful for user {}", session.user_id);
                manager.set_legacy_auth(true, Some(session.user_id)).await;
                let _ = app.emit("runtime:event", RuntimeEvent::AuthChanged {
                    logged_in: true,
                    user_id: Some(session.user_id),
                });

                // Step 4: Authenticate CoreBridge/V2 - REQUIRED per ADR
                if let Some(bridge) = core_bridge.try_get().await {
                    match bridge.login(&creds.email, &creds.password).await {
                        Ok(_) => {
                            log::info!("[Runtime] CoreBridge auth successful");
                            manager.set_corebridge_auth(true).await;
                        }
                        Err(e) => {
                            log::error!("[Runtime] CoreBridge auth failed: {}", e);
                            let _ = app.emit("runtime:event", RuntimeEvent::CoreBridgeAuthFailed {
                                error: e.to_string(),
                            });
                            manager.set_bootstrap_in_progress(false).await;
                            return Err(RuntimeError::V2AuthFailed(e));
                        }
                    }
                } else {
                    log::error!("[Runtime] CoreBridge not initialized - cannot complete bootstrap");
                    manager.set_bootstrap_in_progress(false).await;
                    return Err(RuntimeError::V2NotInitialized);
                }

                // Step 5: Activate per-user session - REQUIRED (FATAL if fails)
                // This initializes all per-user stores and sets runtime state
                // Session activation failure is FATAL per parity with v2_auto_login/v2_manual_login
                if let Err(e) = crate::session_lifecycle::activate_session(&app, session.user_id).await {
                    log::error!("[Runtime] Session activation failed: {}", e);
                    // Rollback auth state since session is not usable
                    manager.set_legacy_auth(false, None).await;
                    manager.set_corebridge_auth(false).await;
                    let reason = DegradedReason::SessionActivationFailed(e.clone());
                    manager.set_degraded(reason.clone()).await;
                    manager.set_bootstrap_in_progress(false).await;
                    let _ = app.emit("runtime:event", RuntimeEvent::RuntimeDegraded { reason: reason.clone() });
                    return Err(RuntimeError::RuntimeDegraded(reason));
                }
            }
            Err(e) => {
                log::warn!("[Runtime] Auto-login failed: {}", e);
                // Not a fatal error - user can login manually
            }
        }
    } else {
        log::info!("[Runtime] No saved credentials or last user ID, staying in InitializedNoAuth");
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
                let _ = app.emit("runtime:event", RuntimeEvent::RuntimeDegraded { reason: reason.clone() });
                return Err(RuntimeError::RuntimeDegraded(reason));
            }
        }
    }

    Ok(manager.get_status().await)
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

/// Auto-login using saved credentials (Phase 2 of runtime initialization)
///
/// This command:
/// 1. Loads saved credentials from keyring
/// 2. Authenticates with legacy client
/// 3. Authenticates with CoreBridge/V2 (BLOCKING - required per ADR)
/// 4. Updates RuntimeManager state
///
/// V2 auth is REQUIRED - if it fails, the whole login fails.
/// ToS acceptance is REQUIRED - checked in backend before any auth attempt.
#[tauri::command]
pub async fn v2_auto_login(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
    app_state: State<'_, AppState>,
    core_bridge: State<'_, CoreBridgeState>,
    legal_state: State<'_, LegalSettingsState>,
) -> Result<V2LoginResponse, String> {
    let manager = runtime.manager();

    log::info!("[V2] v2_auto_login starting...");

    // ToS gate - must be accepted before any login attempt
    if let Err((error_code, error)) = require_tos_accepted(&legal_state) {
        return Ok(V2LoginResponse {
            success: false,
            user_name: None,
            user_id: None,
            subscription: None,
            subscription_valid_until: None,
            error: Some(error),
            error_code: Some(error_code),
        });
    }

    // Load saved credentials
    let creds = match crate::credentials::load_qobuz_credentials() {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Ok(V2LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some("No saved credentials".to_string()),
                error_code: Some("no_credentials".to_string()),
            });
        }
        Err(e) => {
            return Ok(V2LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some(e),
                error_code: Some("credentials_error".to_string()),
            });
        }
    };

    // Legacy auth
    let client = app_state.client.read().await;
    let session = match client.login(&creds.email, &creds.password).await {
        Ok(s) => s,
        Err(e) => {
            let error_code = if matches!(e, crate::api::error::ApiError::IneligibleUser) {
                Some("ineligible_user".to_string())
            } else {
                None
            };
            return Ok(V2LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some(e.to_string()),
                error_code,
            });
        }
    };
    drop(client);

    log::info!("[V2] Legacy auth successful for user {}", session.user_id);
    manager.set_legacy_auth(true, Some(session.user_id)).await;
    let _ = app.emit("runtime:event", RuntimeEvent::AuthChanged {
        logged_in: true,
        user_id: Some(session.user_id),
    });

    // V2 CoreBridge auth - REQUIRED per ADR Runtime Session Contract
    if let Some(bridge) = core_bridge.try_get().await {
        match bridge.login(&creds.email, &creds.password).await {
            Ok(_) => {
                log::info!("[V2] CoreBridge auth successful");
                manager.set_corebridge_auth(true).await;
            }
            Err(e) => {
                log::error!("[V2] CoreBridge auth failed: {}", e);
                rollback_auth_state(&manager, &app).await;
                let _ = app.emit("runtime:event", RuntimeEvent::CoreBridgeAuthFailed {
                    error: e.to_string(),
                });
                // V2 auth failed - return failure per ADR
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
        log::error!("[V2] CoreBridge not initialized - cannot complete auth");
        rollback_auth_state(&manager, &app).await;
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

    // Activate per-user session (initializes all per-user stores)
    // This is REQUIRED - without it, user has auth but no stores, causing UserSessionNotActivated errors
    if let Err(e) = crate::session_lifecycle::activate_session(&app, session.user_id).await {
        log::error!("[V2] Session activation failed: {}", e);
        rollback_auth_state(&manager, &app).await;
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

    // Emit ready event
    let _ = app.emit("runtime:event", RuntimeEvent::RuntimeReady {
        user_id: session.user_id,
    });

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

/// Manual login with email and password (V2 - with blocking CoreBridge auth)
///
/// This command:
/// 1. Authenticates with legacy client
/// 2. Authenticates with CoreBridge/V2 (BLOCKING - required per ADR)
/// 3. Updates RuntimeManager state
///
/// V2 auth is REQUIRED - if it fails, the whole login fails.
/// ToS acceptance is REQUIRED - checked in backend before any auth attempt.
#[tauri::command]
pub async fn v2_manual_login(
    email: String,
    password: String,
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
    app_state: State<'_, AppState>,
    core_bridge: State<'_, CoreBridgeState>,
    legal_state: State<'_, LegalSettingsState>,
) -> Result<V2LoginResponse, String> {
    let manager = runtime.manager();

    log::info!("[V2] v2_manual_login starting...");

    // ToS gate - must be accepted before any login attempt
    if let Err((error_code, error)) = require_tos_accepted(&legal_state) {
        return Ok(V2LoginResponse {
            success: false,
            user_name: None,
            user_id: None,
            subscription: None,
            subscription_valid_until: None,
            error: Some(error),
            error_code: Some(error_code),
        });
    }

    // Legacy auth
    let client = app_state.client.read().await;
    let session = match client.login(&email, &password).await {
        Ok(s) => s,
        Err(e) => {
            let error_code = if matches!(e, crate::api::error::ApiError::IneligibleUser) {
                Some("ineligible_user".to_string())
            } else {
                None
            };
            return Ok(V2LoginResponse {
                success: false,
                user_name: None,
                user_id: None,
                subscription: None,
                subscription_valid_until: None,
                error: Some(e.to_string()),
                error_code,
            });
        }
    };
    drop(client);

    log::info!("[V2] Legacy auth successful for user {}", session.user_id);
    manager.set_legacy_auth(true, Some(session.user_id)).await;
    let _ = app.emit("runtime:event", RuntimeEvent::AuthChanged {
        logged_in: true,
        user_id: Some(session.user_id),
    });

    // V2 CoreBridge auth - REQUIRED per ADR Runtime Session Contract
    if let Some(bridge) = core_bridge.try_get().await {
        match bridge.login(&email, &password).await {
            Ok(_) => {
                log::info!("[V2] CoreBridge auth successful");
                manager.set_corebridge_auth(true).await;
            }
            Err(e) => {
                log::error!("[V2] CoreBridge auth failed: {}", e);
                rollback_auth_state(&manager, &app).await;
                let _ = app.emit("runtime:event", RuntimeEvent::CoreBridgeAuthFailed {
                    error: e.to_string(),
                });
                // V2 auth failed - return failure per ADR
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
        log::error!("[V2] CoreBridge not initialized - cannot complete auth");
        rollback_auth_state(&manager, &app).await;
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

    // Activate per-user session (initializes all per-user stores)
    // This is REQUIRED - without it, user has auth but no stores, causing UserSessionNotActivated errors
    if let Err(e) = crate::session_lifecycle::activate_session(&app, session.user_id).await {
        log::error!("[V2] Session activation failed: {}", e);
        rollback_auth_state(&manager, &app).await;
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

    // Emit ready event
    let _ = app.emit("runtime:event", RuntimeEvent::RuntimeReady {
        user_id: session.user_id,
    });

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

// ==================== Prefetch (V2) ====================

/// Number of Qobuz tracks to prefetch (not total tracks, just Qobuz)
const V2_PREFETCH_COUNT: usize = 2;

/// How far ahead to look for tracks to prefetch (to handle mixed playlists)
const V2_PREFETCH_LOOKAHEAD: usize = 10;

/// Maximum concurrent prefetch downloads
const V2_MAX_CONCURRENT_PREFETCH: usize = 1;

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
            log::debug!("[V2/PREFETCH] Skipping local track: {} - {}", track_id, track_title);
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

        log::info!("[V2/PREFETCH] Prefetching track: {} - {}", track_id, track_title);

        // Spawn background task for each track (with semaphore to limit concurrency)
        tokio::spawn(async move {
            // Acquire semaphore permit to limit concurrent prefetches
            let _permit = match V2_PREFETCH_SEMAPHORE.acquire().await {
                Ok(permit) => permit,
                Err(_) => {
                    log::warn!("[V2/PREFETCH] Semaphore closed, skipping track {}", track_id);
                    cache_clone.unmark_fetching(track_id);
                    return;
                }
            };

            let result = async {
                let bridge_guard = bridge_clone.read().await;
                let bridge = bridge_guard.as_ref().ok_or("CoreBridge not initialized")?;
                let stream_url = bridge.get_stream_url(track_id, quality).await?;
                drop(bridge_guard);

                let data = download_audio(&stream_url.url).await?;
                Ok::<Vec<u8>, String>(data)
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
                    log::warn!("[V2/PREFETCH] Failed for track {}: {}", track_id, e);
                }
            }

            cache_clone.unmark_fetching(track_id);
        });
    }
}

// ==================== Auth Commands (V2) ====================

/// Check if user is logged in (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_is_logged_in(
    bridge: State<'_, CoreBridgeState>,
) -> Result<bool, RuntimeError> {
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

    // Step 0: ToS gate - must be accepted before any login attempt
    if let Err((_, error)) = require_tos_accepted(&legal_state) {
        return Err(RuntimeError::Internal(error));
    }

    // Step 1: Legacy auth
    let session = {
        let client = app_state.client.read().await;
        client.login(&email, &password).await
            .map_err(|e| RuntimeError::Internal(e.to_string()))?
    };
    manager.set_legacy_auth(true, Some(session.user_id)).await;
    log::info!("[v2_login] Legacy auth successful for user {}", session.user_id);

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
    log::info!("[v2_login] Session activated for user {}", session.user_id);

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

    // Step 1: Deactivate session (teardown stores, clear runtime state)
    crate::session_lifecycle::deactivate_session(&app).await
        .map_err(RuntimeError::Internal)?;
    log::info!("[v2_logout] Session deactivated");

    // Step 2: CoreBridge logout
    let bridge_guard = bridge.get().await;
    bridge_guard.logout().await
        .map_err(RuntimeError::Internal)?;
    log::info!("[v2_logout] CoreBridge logged out");

    // Step 3: Legacy logout
    {
        let client = app_state.client.read().await;
        client.logout().await;
    }
    log::info!("[v2_logout] Legacy client logged out");

    Ok(())
}

/// Activate offline-only session (no remote auth required)
///
/// This creates a minimal session for offline/local library use.
/// Uses user_id = 0 as a special "offline user" marker.
/// Queue commands will work because session_activated is set.
#[tauri::command]
pub async fn v2_activate_offline_session(
    app: tauri::AppHandle,
) -> Result<(), RuntimeError> {
    crate::session_lifecycle::activate_offline_session(&app).await
        .map_err(RuntimeError::Internal)
}

// ==================== Queue Commands (V2) ====================

/// Get current queue state (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_queue_state(
    bridge: State<'_, CoreBridgeState>,
) -> Result<QueueState, RuntimeError> {
    let bridge = bridge.get().await;
    Ok(bridge.get_queue_state().await)
}

/// Set repeat mode (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_set_repeat_mode(
    mode: RepeatMode,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
    let bridge = bridge.get().await;
    bridge.set_repeat_mode(mode).await;
    Ok(())
}

/// Toggle shuffle (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_toggle_shuffle(
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<bool, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
    let bridge = bridge.get().await;
    Ok(bridge.toggle_shuffle().await)
}

/// Set shuffle mode directly (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_set_shuffle(
    enabled: bool,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
    log::info!("[V2] set_shuffle: {}", enabled);
    let bridge = bridge.get().await;
    bridge.set_shuffle(enabled).await;
    Ok(())
}

/// Clear the queue (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_clear_queue(
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
    let bridge = bridge.get().await;
    bridge.clear_queue().await;
    Ok(())
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
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
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
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
    log::info!("[V2] add_to_queue_next: {} - {}", track.id, track.title);
    let bridge = bridge.get().await;
    bridge.add_track_next(track.into()).await;
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
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
    log::info!("[V2] set_queue: {} tracks, start at {}", tracks.len(), start_index);
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
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
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
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
    log::info!("[V2] remove_upcoming_track: upcoming_index {}", upcoming_index);
    let bridge = bridge.get().await;
    Ok(bridge.remove_upcoming_track(upcoming_index).await.map(Into::into))
}

/// Skip to next track in queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_next_track(
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
    log::info!("[V2] next_track");
    let bridge = bridge.get().await;
    let track = bridge.next_track().await;
    Ok(track.map(Into::into))
}

/// Go to previous track in queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_previous_track(
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
    log::info!("[V2] previous_track");
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
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
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
    runtime: State<'_, RuntimeManagerState>,
) -> Result<bool, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
    log::info!("[V2] move_queue_track: {} -> {}", from_index, to_index);
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
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
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
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    let bridge = bridge.get().await;
    let mut results = bridge.search_albums(&query, limit, offset, searchType.as_deref()).await
        .map_err(RuntimeError::Internal)?;

    // Filter out albums from blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|album| !blacklist_state.is_blacklisted(album.artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} albums from search results", filtered_count);
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    let bridge = bridge.get().await;
    let mut results = bridge.search_tracks(&query, limit, offset, searchType.as_deref()).await
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
        log::debug!("[V2/Blacklist] Filtered {} tracks from search results", filtered_count);
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    let bridge = bridge.get().await;
    let mut results = bridge.search_artists(&query, limit, offset, searchType.as_deref()).await
        .map_err(RuntimeError::Internal)?;

    // Filter out blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|artist| !blacklist_state.is_blacklisted(artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} artists from search results", filtered_count);
        results.total = results.total.saturating_sub(filtered_count as u32);
    }

    Ok(results)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;
    let bridge = bridge.get().await;
    bridge.get_album(&albumId).await.map_err(RuntimeError::Internal)
}

/// Get track by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_track(
    trackId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Track, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;
    let bridge = bridge.get().await;
    bridge.get_track(trackId).await.map_err(RuntimeError::Internal)
}

/// Get artist by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_artist(
    artistId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Artist, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;
    let bridge = bridge.get().await;
    bridge.get_artist(artistId).await.map_err(RuntimeError::Internal)
}

// ==================== Favorites Commands (V2) ====================

/// Get favorites (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_favorites(
    favType: String,
    limit: u32,
    offset: u32,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<serde_json::Value, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 favorites
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    let bridge = bridge.get().await;
    bridge.get_favorites(&favType, limit, offset).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] add_favorite type={} id={}", favType, itemId);
    let bridge = bridge.get().await;
    bridge.add_favorite(&favType, &itemId).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] remove_favorite type={} id={}", favType, itemId);
    let bridge = bridge.get().await;
    bridge.remove_favorite(&favType, &itemId).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await?;
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
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await?;
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
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await?;
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
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await?;
    log::info!("[V2] Command: seek {}", position);
    let bridge_guard = bridge.get().await;
    let result = bridge_guard.seek(position).map_err(RuntimeError::Internal);

    // Update MPRIS with new position
    let playback_state = bridge_guard.get_playback_state();
    app_state.media_controls.set_playback_with_progress(
        playback_state.is_playing,
        position,
    );

    result
}

/// Set volume (0.0 - 1.0) (V2)
#[tauri::command]
pub async fn v2_set_volume(
    volume: f32,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await?;
    let bridge = bridge.get().await;
    bridge.set_volume(volume).map_err(RuntimeError::Internal)
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
    app_state.media_controls.set_playback_with_progress(
        playback_state.is_playing,
        playback_state.position,
    );

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
    app_state.media_controls.set_playback_with_progress(true, 0);
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
) -> Result<bool, RuntimeError> {
    log::info!("[V2] Command: play_next_gapless for track {}", track_id);

    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();

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
                let audio_data = std::fs::read(path)
                    .map_err(|e| RuntimeError::Internal(format!("Failed to read cached file: {}", e)))?;
                player.play_next(audio_data, track_id).map_err(RuntimeError::Internal)?;
                return Ok(true);
            }
        }
    }

    // Check memory cache (L1)
    let cache = app_state.audio_cache.clone();
    if let Some(cached) = cache.get(track_id) {
        log::info!("[V2/GAPLESS] Track {} from MEMORY cache ({} bytes)", track_id, cached.size_bytes);
        player.play_next(cached.data, track_id).map_err(RuntimeError::Internal)?;
        return Ok(true);
    }

    // Check playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!("[V2/GAPLESS] Track {} from DISK cache ({} bytes)", track_id, audio_data.len());
            player.play_next(audio_data, track_id).map_err(RuntimeError::Internal)?;
            return Ok(true);
        }
    }

    log::info!("[V2/GAPLESS] Track {} not in any cache, gapless not possible", track_id);
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
        track_id, quality, preferred_quality, final_quality
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
                    let audio_data = std::fs::read(path)
                        .map_err(|e| RuntimeError::Internal(format!("Failed to read cached file: {}", e)))?;
                    cache.insert(track_id, audio_data);
                    return Ok(());
                }
            }
        }

        let bridge_guard = bridge.get().await;
        let stream_url = bridge_guard.get_stream_url(track_id, final_quality).await
            .map_err(RuntimeError::Internal)?;
        drop(bridge_guard);

        let audio_data = download_audio(&stream_url.url).await
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
    bridge: State<'_, CoreBridgeState>,
    offline_cache: State<'_, OfflineCacheState>,
    audio_settings: State<'_, AudioSettingsState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<V2PlayTrackResult, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 playback
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

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

    // Check streaming_only setting
    let streaming_only = {
        let guard = audio_settings.store.lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        guard.as_ref().and_then(|s| s.get_settings().ok()).map(|s| s.streaming_only).unwrap_or(false)
    };

    log::info!(
        "[V2] play_track {} (quality_str={:?}, parsed={:?}, final={:?}, format_id={})",
        track_id, quality, preferred_quality, final_quality, final_quality.id()
    );

    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();

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
                log::info!("[V2/CACHE HIT] Track {} from OFFLINE cache: {:?}", track_id, path);
                let audio_data = std::fs::read(path)
                    .map_err(|e| RuntimeError::Internal(format!("Failed to read cached file: {}", e)))?;
                player.play_data(audio_data, track_id)
                    .map_err(RuntimeError::Internal)?;

                // Prefetch next tracks in background (using CoreBridge queue)
                let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
                drop(bridge_guard);
                spawn_v2_prefetch(bridge.0.clone(), app_state.audio_cache.clone(), upcoming_tracks, final_quality, streaming_only);
                return Ok(V2PlayTrackResult { format_id: None });
            }
        }
    }

    // Check memory cache (L1) - using AppState's audio_cache for now
    // TODO: Move cache to qbz-core in future refactor
    let cache = app_state.audio_cache.clone();
    if let Some(cached) = cache.get(track_id) {
        log::info!("[V2/CACHE HIT] Track {} from MEMORY cache ({} bytes)", track_id, cached.size_bytes);
        player.play_data(cached.data, track_id)
            .map_err(RuntimeError::Internal)?;

        // Prefetch next tracks in background (using CoreBridge queue)
        let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
        drop(bridge_guard);
        spawn_v2_prefetch(bridge.0.clone(), cache.clone(), upcoming_tracks, final_quality, streaming_only);
        return Ok(V2PlayTrackResult { format_id: None });
    }

    // Check playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!("[V2/CACHE HIT] Track {} from DISK cache ({} bytes)", track_id, audio_data.len());
            cache.insert(track_id, audio_data.clone());
            player.play_data(audio_data, track_id)
                .map_err(RuntimeError::Internal)?;

            // Prefetch next tracks in background (using CoreBridge queue)
            let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
            drop(bridge_guard);
            spawn_v2_prefetch(bridge.0.clone(), cache.clone(), upcoming_tracks, final_quality, streaming_only);
            return Ok(V2PlayTrackResult { format_id: None });
        }
    }

    // Not in any cache - get stream URL from Qobuz via CoreBridge
    log::info!("[V2] Track {} not in cache, fetching from network...", track_id);

    let stream_url = bridge_guard.get_stream_url(track_id, final_quality).await
        .map_err(RuntimeError::Internal)?;
    log::info!("[V2] Got stream URL for track {} (format_id={})", track_id, stream_url.format_id);

    // Download the audio
    let audio_data = download_audio(&stream_url.url).await
        .map_err(RuntimeError::Internal)?;
    let data_size = audio_data.len();

    // Cache it (unless streaming_only mode)
    if !streaming_only {
        cache.insert(track_id, audio_data.clone());
        log::info!("[V2/CACHED] Track {} stored in memory cache", track_id);
    } else {
        log::info!("[V2/NOT CACHED] Track {} - streaming_only mode active", track_id);
    }

    // Play it via qbz-player
    player.play_data(audio_data, track_id)
        .map_err(RuntimeError::Internal)?;
    log::info!("[V2] Playing track {} ({} bytes)", track_id, data_size);

    // Prefetch next tracks in background (using CoreBridge queue)
    let upcoming_tracks = bridge_guard.peek_upcoming(V2_PREFETCH_LOOKAHEAD).await;
    drop(bridge_guard);
    spawn_v2_prefetch(bridge.0.clone(), cache, upcoming_tracks, final_quality, streaming_only);

    Ok(V2PlayTrackResult { format_id: Some(stream_url.format_id) })
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
                // Convert to qbz_audio::AudioSettings
                let qbz_settings = qbz_audio::AudioSettings {
                    output_device: fresh_settings.output_device.clone(),
                    exclusive_mode: fresh_settings.exclusive_mode,
                    dac_passthrough: fresh_settings.dac_passthrough,
                    preferred_sample_rate: fresh_settings.preferred_sample_rate,
                    limit_quality_to_device: fresh_settings.limit_quality_to_device,
                    device_max_sample_rate: fresh_settings.device_max_sample_rate,
                    device_sample_rate_limits: fresh_settings.device_sample_rate_limits.clone(),
                    backend_type: fresh_settings.backend_type.clone(),
                    alsa_plugin: fresh_settings.alsa_plugin.clone(),
                    alsa_hardware_volume: false, // Not exposed in legacy UI
                    stream_first_track: fresh_settings.stream_first_track,
                    stream_buffer_seconds: fresh_settings.stream_buffer_seconds,
                    streaming_only: fresh_settings.streaming_only,
                    normalization_enabled: fresh_settings.normalization_enabled,
                    normalization_target_lufs: fresh_settings.normalization_target_lufs,
                    gapless_enabled: fresh_settings.gapless_enabled,
                };
                let _ = player.reload_settings(qbz_settings);
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_user_playlists");
    let bridge = bridge.get().await;
    bridge.get_user_playlists().await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_playlist: {}", playlistId);
    let bridge = bridge.get().await;
    bridge.get_playlist(playlistId).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] add_tracks_to_playlist: playlist {} <- {} tracks", playlistId, trackIds.len());
    let bridge = bridge.get().await;
    bridge.add_tracks_to_playlist(playlistId, &trackIds).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    let ptids = playlistTrackIds.unwrap_or_default();
    let tids = trackIds.unwrap_or_default();
    log::info!(
        "[V2] remove_tracks_from_playlist: playlist {} (playlistTrackIds={}, trackIds={})",
        playlistId, ptids.len(), tids.len()
    );

    let bridge = bridge.get().await;

    // If we have direct playlist_track_ids, use them
    if !ptids.is_empty() {
        return bridge.remove_tracks_from_playlist(playlistId, &ptids).await.map_err(RuntimeError::Internal);
    }

    // Otherwise resolve track_ids → playlist_track_ids via full playlist fetch
    if !tids.is_empty() {
        let playlist = bridge.get_playlist(playlistId).await.map_err(RuntimeError::Internal)?;

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
            return Err(RuntimeError::Internal("Could not resolve any track IDs to playlist track IDs".to_string()));
        }

        return bridge.remove_tracks_from_playlist(playlistId, &resolved_ptids).await.map_err(RuntimeError::Internal);
    }

    Err(RuntimeError::Internal("Either playlistTrackIds or trackIds must be provided".to_string()))
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
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.get_settings().map_err(RuntimeError::Internal)
}

/// Set audio output device (V2)
#[tauri::command]
pub fn v2_set_audio_output_device(
    device: Option<String>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    let normalized_device = device
        .as_ref()
        .map(|d| crate::audio::normalize_device_id_to_stable(d));
    log::info!(
        "[V2] set_audio_output_device {:?} -> {:?} (normalized)",
        device, normalized_device
    );
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_output_device(normalized_device.as_deref()).map_err(RuntimeError::Internal)
}

/// Set audio exclusive mode (V2)
#[tauri::command]
pub fn v2_set_audio_exclusive_mode(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_exclusive_mode: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_exclusive_mode(enabled).map_err(RuntimeError::Internal)
}

/// Set DAC passthrough mode (V2)
#[tauri::command]
pub fn v2_set_audio_dac_passthrough(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_dac_passthrough: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_dac_passthrough(enabled).map_err(RuntimeError::Internal)
}

/// Set preferred sample rate (V2)
#[tauri::command]
pub fn v2_set_audio_sample_rate(
    rate: Option<u32>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_sample_rate: {:?}", rate);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_sample_rate(rate).map_err(RuntimeError::Internal)
}

/// Set audio backend type (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_set_audio_backend_type(
    backendType: Option<AudioBackendType>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_backend_type: {:?}", backendType);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_backend_type(backendType).map_err(RuntimeError::Internal)
}

/// Set ALSA plugin (V2)
#[tauri::command]
pub fn v2_set_audio_alsa_plugin(
    plugin: Option<AlsaPlugin>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_alsa_plugin: {:?}", plugin);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_alsa_plugin(plugin).map_err(RuntimeError::Internal)
}

/// Set gapless playback enabled (V2)
#[tauri::command]
pub fn v2_set_audio_gapless_enabled(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_gapless_enabled: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_gapless_enabled(enabled).map_err(RuntimeError::Internal)
}

/// Set normalization enabled (V2)
#[tauri::command]
pub fn v2_set_audio_normalization_enabled(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_normalization_enabled: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_normalization_enabled(enabled).map_err(RuntimeError::Internal)
}

/// Set normalization target LUFS (V2)
#[tauri::command]
pub fn v2_set_audio_normalization_target(
    target: f32,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_normalization_target: {}", target);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_normalization_target_lufs(target).map_err(RuntimeError::Internal)
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
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_device_max_sample_rate(rate).map_err(RuntimeError::Internal)
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
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_limit_quality_to_device(enabled).map_err(RuntimeError::Internal)
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
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_streaming_only(enabled).map_err(RuntimeError::Internal)
}

/// Reset audio settings to defaults (V2)
#[tauri::command]
pub fn v2_reset_audio_settings(
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] reset_audio_settings");
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.reset_all().map(|_| ()).map_err(RuntimeError::Internal)
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
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_stream_first_track(enabled).map_err(RuntimeError::Internal)
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
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_stream_buffer_seconds(seconds).map_err(RuntimeError::Internal)
}

/// Set ALSA hardware volume control (V2)
#[tauri::command]
pub fn v2_set_audio_alsa_hardware_volume(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_alsa_hardware_volume: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    store.set_alsa_hardware_volume(enabled).map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] create_playlist: {}", name);
    let bridge = bridge.get().await;
    bridge.create_playlist(&name, description.as_deref(), isPublic).await.map_err(RuntimeError::Internal)
}

/// Delete a playlist (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_delete_playlist(
    playlistId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] delete_playlist: {}", playlistId);
    let bridge = bridge.get().await;
    bridge.delete_playlist(playlistId).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] update_playlist: {}", playlistId);
    let bridge = bridge.get().await;
    bridge.update_playlist(playlistId, name.as_deref(), description.as_deref(), isPublic).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] search_playlists: {}", query);
    let bridge = bridge.get().await;
    bridge.search_playlists(&query, limit, offset).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_tracks_batch: {} tracks", trackIds.len());
    let bridge = bridge.get().await;
    bridge.get_tracks_batch(&trackIds).await.map_err(RuntimeError::Internal)
}

/// Get genres (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_genres(
    parentId: Option<u64>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<GenreInfo>, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_genres: parent={:?}", parentId);
    let bridge = bridge.get().await;
    bridge.get_genres(parentId).await.map_err(RuntimeError::Internal)
}

/// Get discover index (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_discover_index(
    genreIds: Option<Vec<u64>>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<DiscoverResponse, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_discover_index: genres={:?}", genreIds);
    let bridge = bridge.get().await;
    bridge.get_discover_index(genreIds).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_discover_playlists: tag={:?}", tag);
    let bridge = bridge.get().await;
    bridge.get_discover_playlists(tag, genreIds, limit, offset).await.map_err(RuntimeError::Internal)
}

/// Get playlist tags (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_playlist_tags(
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<PlaylistTag>, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_playlist_tags");
    let bridge = bridge.get().await;
    bridge.get_playlist_tags().await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    // Map endpoint type to actual path
    let endpoint = match endpointType.as_str() {
        "newReleases" => "/discover/newReleases",
        "idealDiscography" => "/discover/idealDiscography",
        "mostStreamed" => "/discover/mostStreamed",
        "qobuzissimes" => "/discover/qobuzissims",
        "albumOfTheWeek" => "/discover/albumOfTheWeek",
        "pressAward" => "/discover/pressAward",
        _ => return Err(RuntimeError::Internal(format!("Unknown discover endpoint type: {}", endpointType))),
    };

    log::info!("[V2] get_discover_albums: type={}", endpointType);
    let bridge = bridge.get().await;
    let mut results = bridge.get_discover_albums(
        endpoint,
        genreIds,
        offset.unwrap_or(0),
        limit.unwrap_or(50),
    ).await
        .map_err(RuntimeError::Internal)?;

    // Filter out albums from blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|album| {
        // Check if any of the album's artists are blacklisted
        !album.artists.iter().any(|artist| blacklist_state.is_blacklisted(artist.id))
    });

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} albums from discover results", filtered_count);
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_featured_albums: type={}, genre={:?}", featuredType, genreId);
    let bridge = bridge.get().await;
    let mut results = bridge.get_featured_albums(&featuredType, limit, offset, genreId).await
        .map_err(RuntimeError::Internal)?;

    // Filter out albums from blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|album| !blacklist_state.is_blacklisted(album.artist.id));

    let filtered_count = original_count - results.items.len();
    if filtered_count > 0 {
        log::debug!("[V2/Blacklist] Filtered {} albums from featured results", filtered_count);
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_artist_page: {} sort={:?}", artistId, sort);
    let bridge = bridge.get().await;
    bridge.get_artist_page(artistId, sort.as_deref()).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_similar_artists: {}", artistId);
    let bridge = bridge.get().await;
    let mut results = bridge.get_similar_artists(artistId, limit, offset).await
        .map_err(RuntimeError::Internal)?;

    // Filter out blacklisted artists
    let original_count = results.items.len();
    results.items.retain(|artist| !blacklist_state.is_blacklisted(artist.id));

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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_artist_with_albums: {} limit={:?} offset={:?}", artistId, limit, offset);
    let bridge = bridge.get().await;
    bridge.get_artist_with_albums(artistId, limit, offset).await.map_err(RuntimeError::Internal)
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
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

    log::info!("[V2] get_label: {}", labelId);
    let bridge = bridge.get().await;
    bridge.get_label(labelId, limit, offset).await.map_err(RuntimeError::Internal)
}

// ==================== Integrations V2 Commands ====================
//
// These commands use the qbz-integrations crate which is Tauri-independent.
// They can work without Tauri for TUI/headless clients.

use crate::integrations_v2::{ListenBrainzV2State, MusicBrainzV2State, LastFmV2State};

// --- ListenBrainz V2 ---

/// Get ListenBrainz status (V2)
#[tauri::command]
pub async fn v2_listenbrainz_get_status(
    state: State<'_, ListenBrainzV2State>,
) -> Result<qbz_integrations::listenbrainz::ListenBrainzStatus, RuntimeError> {
    log::info!("[V2] listenbrainz_get_status");
    let client = state.client.lock().await;
    Ok(client.get_status().await)
}

/// Check if ListenBrainz is enabled (V2)
#[tauri::command]
pub async fn v2_listenbrainz_is_enabled(
    state: State<'_, ListenBrainzV2State>,
) -> Result<bool, RuntimeError> {
    let client = state.client.lock().await;
    Ok(client.is_enabled().await)
}

/// Enable or disable ListenBrainz (V2)
#[tauri::command]
pub async fn v2_listenbrainz_set_enabled(
    enabled: bool,
    state: State<'_, ListenBrainzV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] listenbrainz_set_enabled: {}", enabled);
    let client = state.client.lock().await;
    client.set_enabled(enabled).await;
    Ok(())
}

/// Connect to ListenBrainz with token (V2)
#[tauri::command]
pub async fn v2_listenbrainz_connect(
    token: String,
    state: State<'_, ListenBrainzV2State>,
) -> Result<qbz_integrations::listenbrainz::UserInfo, RuntimeError> {
    log::info!("[V2] listenbrainz_connect");
    let client = state.client.lock().await;
    let user_info = client.set_token(&token).await
        .map_err(|e| RuntimeError::Internal(e.to_string()))?;

    // Save credentials for persistence
    drop(client);
    state.save_credentials(token, user_info.user_name.clone()).await;

    Ok(user_info)
}

/// Disconnect from ListenBrainz (V2)
#[tauri::command]
pub async fn v2_listenbrainz_disconnect(
    state: State<'_, ListenBrainzV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] listenbrainz_disconnect");
    let client = state.client.lock().await;
    client.disconnect().await;
    drop(client);
    state.clear_credentials().await;
    Ok(())
}

/// Submit now playing to ListenBrainz (V2)
#[tauri::command]
pub async fn v2_listenbrainz_now_playing(
    artist: String,
    track: String,
    album: Option<String>,
    recording_mbid: Option<String>,
    release_mbid: Option<String>,
    artist_mbids: Option<Vec<String>>,
    isrc: Option<String>,
    duration_ms: Option<u64>,
    state: State<'_, ListenBrainzV2State>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] listenbrainz_now_playing: {} - {}", artist, track);

    // Build additional info if any MusicBrainz data provided
    let additional_info = if recording_mbid.is_some() || release_mbid.is_some()
        || artist_mbids.is_some() || isrc.is_some() || duration_ms.is_some()
    {
        Some(qbz_integrations::listenbrainz::AdditionalInfo {
            recording_mbid,
            release_mbid,
            artist_mbids,
            isrc,
            duration_ms,
            tracknumber: None,
            media_player: "QBZ".to_string(),
            media_player_version: env!("CARGO_PKG_VERSION").to_string(),
            submission_client: "QBZ".to_string(),
            submission_client_version: env!("CARGO_PKG_VERSION").to_string(),
        })
    } else {
        None
    };

    let client = state.client.lock().await;
    client.submit_playing_now(&artist, &track, album.as_deref(), additional_info).await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

/// Submit scrobble to ListenBrainz (V2)
#[tauri::command]
pub async fn v2_listenbrainz_scrobble(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: i64,
    recording_mbid: Option<String>,
    release_mbid: Option<String>,
    artist_mbids: Option<Vec<String>>,
    isrc: Option<String>,
    duration_ms: Option<u64>,
    state: State<'_, ListenBrainzV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] listenbrainz_scrobble: {} - {}", artist, track);

    // Build additional info if any MusicBrainz data provided
    let additional_info = if recording_mbid.is_some() || release_mbid.is_some()
        || artist_mbids.is_some() || isrc.is_some() || duration_ms.is_some()
    {
        Some(qbz_integrations::listenbrainz::AdditionalInfo {
            recording_mbid,
            release_mbid,
            artist_mbids,
            isrc,
            duration_ms,
            tracknumber: None,
            media_player: "QBZ".to_string(),
            media_player_version: env!("CARGO_PKG_VERSION").to_string(),
            submission_client: "QBZ".to_string(),
            submission_client_version: env!("CARGO_PKG_VERSION").to_string(),
        })
    } else {
        None
    };

    let client = state.client.lock().await;
    client.submit_listen(&artist, &track, album.as_deref(), timestamp, additional_info).await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

// --- MusicBrainz V2 ---

/// Check if MusicBrainz is enabled (V2)
#[tauri::command]
pub async fn v2_musicbrainz_is_enabled(
    state: State<'_, MusicBrainzV2State>,
) -> Result<bool, RuntimeError> {
    let client = state.client.lock().await;
    Ok(client.is_enabled().await)
}

/// Enable or disable MusicBrainz (V2)
#[tauri::command]
pub async fn v2_musicbrainz_set_enabled(
    enabled: bool,
    state: State<'_, MusicBrainzV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] musicbrainz_set_enabled: {}", enabled);
    let client = state.client.lock().await;
    client.set_enabled(enabled).await;
    Ok(())
}

/// Resolve track to MusicBrainz IDs (V2)
#[tauri::command]
pub async fn v2_musicbrainz_resolve_track(
    artist: String,
    title: String,
    isrc: Option<String>,
    state: State<'_, MusicBrainzV2State>,
) -> Result<Option<qbz_integrations::musicbrainz::ResolvedTrack>, RuntimeError> {
    log::debug!("[V2] musicbrainz_resolve_track: {} - {}", artist, title);
    let client = state.client.lock().await;
    client.resolve_track(&artist, &title, isrc.as_deref()).await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

/// Resolve artist to MusicBrainz ID (V2)
#[tauri::command]
pub async fn v2_musicbrainz_resolve_artist(
    name: String,
    state: State<'_, MusicBrainzV2State>,
) -> Result<Option<qbz_integrations::musicbrainz::ResolvedArtist>, RuntimeError> {
    log::debug!("[V2] musicbrainz_resolve_artist: {}", name);
    let client = state.client.lock().await;
    client.resolve_artist(&name).await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

// --- Last.fm V2 ---

/// Get Last.fm auth token and URL (V2)
#[tauri::command]
pub async fn v2_lastfm_get_auth_url(
    state: State<'_, LastFmV2State>,
) -> Result<String, RuntimeError> {
    log::info!("[V2] lastfm_get_auth_url");
    let client = state.client.lock().await;
    let (token, auth_url) = client.get_token().await
        .map_err(|e| RuntimeError::Internal(e.to_string()))?;

    // Store pending token for later session retrieval
    drop(client);
    state.set_pending_token(token).await;

    Ok(auth_url)
}

/// Complete Last.fm authentication (V2)
#[tauri::command]
pub async fn v2_lastfm_complete_auth(
    state: State<'_, LastFmV2State>,
) -> Result<qbz_integrations::LastFmSession, RuntimeError> {
    log::info!("[V2] lastfm_complete_auth");

    let token = state.take_pending_token().await
        .ok_or_else(|| RuntimeError::Internal("No pending auth token".to_string()))?;

    let mut client = state.client.lock().await;
    let session = client.get_session(&token).await
        .map_err(|e| RuntimeError::Internal(e.to_string()))?;

    Ok(session)
}

/// Check if Last.fm is authenticated (V2)
#[tauri::command]
pub async fn v2_lastfm_is_authenticated(
    state: State<'_, LastFmV2State>,
) -> Result<bool, RuntimeError> {
    let client = state.client.lock().await;
    Ok(client.is_authenticated())
}

/// Disconnect from Last.fm (V2)
#[tauri::command]
pub async fn v2_lastfm_disconnect(
    state: State<'_, LastFmV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] lastfm_disconnect");
    let mut client = state.client.lock().await;
    client.clear_session();
    Ok(())
}

/// Submit now playing to Last.fm (V2)
#[tauri::command]
pub async fn v2_lastfm_now_playing(
    artist: String,
    track: String,
    album: Option<String>,
    state: State<'_, LastFmV2State>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] lastfm_now_playing: {} - {}", artist, track);
    let client = state.client.lock().await;
    client.update_now_playing(&artist, &track, album.as_deref()).await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

/// Scrobble to Last.fm (V2)
#[tauri::command]
pub async fn v2_lastfm_scrobble(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: u64,
    state: State<'_, LastFmV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] lastfm_scrobble: {} - {}", artist, track);
    let client = state.client.lock().await;
    client.scrobble(&artist, &track, album.as_deref(), timestamp).await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

/// Set Last.fm session key (V2)
///
/// Used to restore a previously saved session key.
#[tauri::command]
pub async fn v2_lastfm_set_session(
    session_key: String,
    state: State<'_, LastFmV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] lastfm_set_session");
    let mut client = state.client.lock().await;
    client.set_session_key(session_key);
    Ok(())
}

/// Queue a listen for offline submission (V2)
///
/// Uses legacy cache for persistence until qbz-integrations has its own cache.
#[tauri::command]
pub async fn v2_listenbrainz_queue_listen(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: i64,
    recording_mbid: Option<String>,
    release_mbid: Option<String>,
    artist_mbids: Option<Vec<String>>,
    isrc: Option<String>,
    duration_ms: Option<u64>,
    legacy_state: State<'_, crate::listenbrainz::ListenBrainzSharedState>,
) -> Result<i64, RuntimeError> {
    log::info!("[V2] listenbrainz_queue_listen: {} - {}", artist, track);

    let cache_guard = legacy_state.cache.lock().await;
    let cache = cache_guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;

    cache.queue_listen(
        timestamp,
        &artist,
        &track,
        album.as_deref(),
        recording_mbid.as_deref(),
        release_mbid.as_deref(),
        artist_mbids.as_deref(),
        isrc.as_deref(),
        duration_ms,
    ).map_err(|e| RuntimeError::Internal(e))
}

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
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;

    use crate::playback_context::{PlaybackContext, ContextType, ContentSource};

    let ctx_type = match contextType.as_str() {
        "album" => ContextType::Album,
        "playlist" => ContextType::Playlist,
        "artist_top" => ContextType::ArtistTop,
        "home_list" => ContextType::HomeList,
        "favorites" => ContextType::Favorites,
        "local_library" => ContextType::LocalLibrary,
        "radio" => ContextType::Radio,
        "search" => ContextType::Search,
        _ => return Err(RuntimeError::Internal(format!("Invalid context type: {}", contextType))),
    };

    let content_source = match source.as_str() {
        "qobuz" => ContentSource::Qobuz,
        "local" => ContentSource::Local,
        "plex" => ContentSource::Plex,
        _ => return Err(RuntimeError::Internal(format!("Invalid source: {}", source))),
    };

    let context = PlaybackContext::new(
        ctx_type,
        id,
        label,
        content_source,
        trackIds,
        startPosition,
    );

    app_state.context.set_context(context);
    log::info!("[V2] set_playback_context: type={}", contextType);
    Ok(())
}

/// Clear playback context (V2)
#[tauri::command]
pub async fn v2_clear_playback_context(
    app_state: State<'_, AppState>,
) -> Result<(), RuntimeError> {
    app_state.context.clear_context();
    log::info!("[V2] clear_playback_context");
    Ok(())
}

/// Check if playback context is active (V2)
#[tauri::command]
pub async fn v2_has_playback_context(
    app_state: State<'_, AppState>,
) -> Result<bool, RuntimeError> {
    Ok(app_state.context.has_context())
}

// ==================== Session Persistence Commands (V2) ====================

/// Save session position (V2)
#[tauri::command]
pub async fn v2_save_session_position(
    positionSecs: u64,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state.store.lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.save_position(positionSecs)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Save session volume (V2)
#[tauri::command]
pub async fn v2_save_session_volume(
    volume: f32,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state.store.lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.save_volume(volume)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Save session playback mode (V2)
#[tauri::command]
pub async fn v2_save_session_playback_mode(
    shuffle: bool,
    repeatMode: String,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state.store.lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.save_playback_mode(shuffle, &repeatMode)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Save session state - full state (V2)
#[tauri::command]
pub async fn v2_save_session_state(
    queueTracks: Vec<crate::session_store::PersistedQueueTrack>,
    currentIndex: Option<usize>,
    currentPositionSecs: u64,
    volume: f32,
    shuffleEnabled: bool,
    repeatMode: String,
    wasPlaying: bool,
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
    };

    let guard = session_state.store.lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.save_session(&session)
        .map_err(|e| RuntimeError::Internal(e))?;
    log::debug!("[V2] save_session_state: index={:?} pos={}", currentIndex, currentPositionSecs);
    Ok(())
}

/// Load session state (V2)
#[tauri::command]
pub async fn v2_load_session_state(
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<crate::session_store::PersistedSession, RuntimeError> {
    let guard = session_state.store.lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.load_session()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Clear session (V2)
#[tauri::command]
pub async fn v2_clear_session(
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state.store.lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.clear_session()
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
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.get_favorite_track_ids()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Sync cached favorite tracks (V2)
#[tauri::command]
pub async fn v2_sync_cached_favorite_tracks(
    trackIds: Vec<i64>,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.sync_favorite_tracks(&trackIds)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Cache a favorite track (V2)
#[tauri::command]
pub async fn v2_cache_favorite_track(
    trackId: i64,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.add_favorite_track(trackId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Uncache a favorite track (V2)
#[tauri::command]
pub async fn v2_uncache_favorite_track(
    trackId: i64,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.remove_favorite_track(trackId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Clear favorites cache (V2)
#[tauri::command]
pub async fn v2_clear_favorites_cache(
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.clear_all()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get cached favorite albums (V2)
#[tauri::command]
pub async fn v2_get_cached_favorite_albums(
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<Vec<String>, RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.get_favorite_album_ids()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Sync cached favorite albums (V2)
#[tauri::command]
pub async fn v2_sync_cached_favorite_albums(
    albumIds: Vec<String>,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.sync_favorite_albums(&albumIds)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Cache a favorite album (V2)
#[tauri::command]
pub async fn v2_cache_favorite_album(
    albumId: String,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.add_favorite_album(&albumId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Uncache a favorite album (V2)
#[tauri::command]
pub async fn v2_uncache_favorite_album(
    albumId: String,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.remove_favorite_album(&albumId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get cached favorite artists (V2)
#[tauri::command]
pub async fn v2_get_cached_favorite_artists(
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<Vec<i64>, RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.get_favorite_artist_ids()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Sync cached favorite artists (V2)
#[tauri::command]
pub async fn v2_sync_cached_favorite_artists(
    artistIds: Vec<i64>,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.sync_favorite_artists(&artistIds)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Cache a favorite artist (V2)
#[tauri::command]
pub async fn v2_cache_favorite_artist(
    artistId: i64,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.add_favorite_artist(artistId)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Uncache a favorite artist (V2)
#[tauri::command]
pub async fn v2_uncache_favorite_artist(
    artistId: i64,
    cache_state: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
) -> Result<(), RuntimeError> {
    let guard = cache_state.store.lock()
        .map_err(|_| RuntimeError::Internal("Failed to lock favorites cache".to_string()))?;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.remove_favorite_artist(artistId)
        .map_err(|e| RuntimeError::Internal(e))
}

// ==================== Settings Commands (V2) ====================

// -- Tray Settings --

/// Get tray settings (V2)
#[tauri::command]
pub async fn v2_get_tray_settings(
    state: State<'_, crate::config::tray_settings::TraySettingsState>,
) -> Result<crate::config::tray_settings::TraySettings, RuntimeError> {
    state.get_settings().map_err(|e| RuntimeError::Internal(e))
}

/// Set enable tray (V2)
#[tauri::command]
pub async fn v2_set_enable_tray(
    value: bool,
    state: State<'_, crate::config::tray_settings::TraySettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting enable_tray to {} (restart required)", value);
    state.set_enable_tray(value).map_err(|e| RuntimeError::Internal(e))
}

/// Set minimize to tray (V2)
#[tauri::command]
pub async fn v2_set_minimize_to_tray(
    value: bool,
    state: State<'_, crate::config::tray_settings::TraySettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting minimize_to_tray to {}", value);
    state.set_minimize_to_tray(value).map_err(|e| RuntimeError::Internal(e))
}

/// Set close to tray (V2)
#[tauri::command]
pub async fn v2_set_close_to_tray(
    value: bool,
    state: State<'_, crate::config::tray_settings::TraySettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting close_to_tray to {}", value);
    state.set_close_to_tray(value).map_err(|e| RuntimeError::Internal(e))
}

// -- Graphics Settings --

/// Get graphics settings (V2)
#[tauri::command]
pub async fn v2_get_graphics_settings(
    state: State<'_, crate::config::graphics_settings::GraphicsSettingsState>,
) -> Result<crate::config::graphics_settings::GraphicsSettings, RuntimeError> {
    crate::config::graphics_settings::get_graphics_settings(state)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set hardware acceleration (V2)
#[tauri::command]
pub async fn v2_set_hardware_acceleration(
    enabled: bool,
    state: State<'_, crate::config::graphics_settings::GraphicsSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting hardware_acceleration to {}", enabled);
    crate::config::graphics_settings::set_hardware_acceleration(state, enabled)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set GDK scale (V2)
#[tauri::command]
pub async fn v2_set_gdk_scale(
    scale: Option<String>,
    state: State<'_, crate::config::graphics_settings::GraphicsSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting gdk_scale to {:?}", scale);
    crate::config::graphics_settings::set_gdk_scale(state, scale)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set GDK DPI scale (V2)
#[tauri::command]
pub async fn v2_set_gdk_dpi_scale(
    scale: Option<String>,
    state: State<'_, crate::config::graphics_settings::GraphicsSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting gdk_dpi_scale to {:?}", scale);
    crate::config::graphics_settings::set_gdk_dpi_scale(state, scale)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get graphics startup status (V2)
#[tauri::command]
pub async fn v2_get_graphics_startup_status() -> Result<crate::config::graphics_settings::GraphicsStartupStatus, RuntimeError> {
    Ok(crate::config::graphics_settings::get_graphics_startup_status())
}

// -- Window Settings --

/// Get window settings (V2)
#[tauri::command]
pub async fn v2_get_window_settings(
    state: State<'_, crate::config::window_settings::WindowSettingsState>,
) -> Result<crate::config::window_settings::WindowSettings, RuntimeError> {
    crate::config::window_settings::get_window_settings(state)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set use system titlebar (V2)
#[tauri::command]
pub async fn v2_set_use_system_titlebar(
    value: bool,
    state: State<'_, crate::config::window_settings::WindowSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting use_system_titlebar to {}", value);
    crate::config::window_settings::set_use_system_titlebar(value, state)
        .map_err(|e| RuntimeError::Internal(e))
}

// -- Developer Settings --

/// Get developer settings (V2)
#[tauri::command]
pub async fn v2_get_developer_settings(
    state: State<'_, crate::config::developer_settings::DeveloperSettingsState>,
) -> Result<crate::config::developer_settings::DeveloperSettings, RuntimeError> {
    crate::config::developer_settings::get_developer_settings(state)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set developer force DMABuf (V2)
#[tauri::command]
pub async fn v2_set_developer_force_dmabuf(
    value: bool,
    state: State<'_, crate::config::developer_settings::DeveloperSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting force_dmabuf to {}", value);
    crate::config::developer_settings::set_developer_force_dmabuf(state, value)
        .map_err(|e| RuntimeError::Internal(e))
}

// -- Playback Preferences --

/// Get playback preferences (V2)
#[tauri::command]
pub async fn v2_get_playback_preferences(
    state: State<'_, crate::config::playback_preferences::PlaybackPreferencesState>,
) -> Result<crate::config::playback_preferences::PlaybackPreferences, RuntimeError> {
    crate::config::playback_preferences::get_playback_preferences(state)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set autoplay mode (V2)
#[tauri::command]
pub async fn v2_set_autoplay_mode(
    mode: String,
    state: State<'_, crate::config::playback_preferences::PlaybackPreferencesState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting autoplay_mode to {}", mode);
    crate::config::playback_preferences::set_autoplay_mode(mode, state)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set show context icon (V2)
#[tauri::command]
pub async fn v2_set_show_context_icon(
    value: bool,
    state: State<'_, crate::config::playback_preferences::PlaybackPreferencesState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting show_context_icon to {}", value);
    crate::config::playback_preferences::set_show_context_icon(value, state)
        .map_err(|e| RuntimeError::Internal(e))
}

// -- Favorites Preferences --

/// Get favorites preferences (V2)
#[tauri::command]
pub async fn v2_get_favorites_preferences(
    state: State<'_, crate::config::favorites_preferences::FavoritesPreferencesState>,
) -> Result<crate::config::favorites_preferences::FavoritesPreferences, RuntimeError> {
    crate::config::favorites_preferences::get_favorites_preferences(state)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Save favorites preferences (V2)
#[tauri::command]
pub async fn v2_save_favorites_preferences(
    prefs: crate::config::favorites_preferences::FavoritesPreferences,
    state: State<'_, crate::config::favorites_preferences::FavoritesPreferencesState>,
) -> Result<crate::config::favorites_preferences::FavoritesPreferences, RuntimeError> {
    log::info!("[V2] Saving favorites preferences");
    crate::config::favorites_preferences::save_favorites_preferences(prefs, state)
        .map_err(|e| RuntimeError::Internal(e))
}

// -- Visualizer --

/// Set visualizer enabled (V2)
#[tauri::command]
pub async fn v2_set_visualizer_enabled(
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting visualizer_enabled to {}", enabled);
    crate::commands::visualizer::set_visualizer_enabled(enabled, app_state)
        .map_err(|e| RuntimeError::Internal(e))
}

// -- Blacklist --

/// Get artist blacklist (V2)
#[tauri::command]
pub async fn v2_get_artist_blacklist(
    state: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<Vec<crate::artist_blacklist::BlacklistedArtist>, RuntimeError> {
    state.get_all().map_err(|e| RuntimeError::Internal(e))
}

/// Add to artist blacklist (V2)
#[tauri::command]
pub async fn v2_add_to_artist_blacklist(
    artistId: u64,
    artistName: String,
    notes: Option<String>,
    state: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Adding artist {} to blacklist", artistId);
    state.add(artistId, &artistName, notes.as_deref())
        .map_err(|e| RuntimeError::Internal(e))
}

/// Remove from artist blacklist (V2)
#[tauri::command]
pub async fn v2_remove_from_artist_blacklist(
    artistId: u64,
    state: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Removing artist {} from blacklist", artistId);
    state.remove(artistId).map_err(|e| RuntimeError::Internal(e))
}

/// Clear artist blacklist (V2)
#[tauri::command]
pub async fn v2_clear_artist_blacklist(
    state: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Clearing artist blacklist");
    state.clear_all().map_err(|e| RuntimeError::Internal(e))
}

/// Set blacklist enabled (V2)
#[tauri::command]
pub async fn v2_set_blacklist_enabled(
    enabled: bool,
    state: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting blacklist_enabled to {}", enabled);
    state.set_enabled(enabled).map_err(|e| RuntimeError::Internal(e))
}

/// Get blacklist settings (V2)
#[tauri::command]
pub async fn v2_get_blacklist_settings(
    state: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<crate::artist_blacklist::BlacklistSettings, RuntimeError> {
    state.get_settings().map_err(|e| RuntimeError::Internal(e))
}

/// Get blacklist count (V2)
#[tauri::command]
pub async fn v2_get_blacklist_count(
    state: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<usize, RuntimeError> {
    Ok(state.count())
}

// -- API Locale --

/// Set API locale (V2)
#[tauri::command]
pub async fn v2_set_api_locale(
    locale: String,
    app_state: State<'_, AppState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] Setting API locale to {}", locale);
    crate::commands::auth::set_api_locale(locale, app_state).await
        .map_err(|e| RuntimeError::Internal(e))
}

// ==================== Cache Management Commands (V2) ====================

/// Get cache stats (V2)
#[tauri::command]
pub async fn v2_get_cache_stats(
    app_state: State<'_, AppState>,
) -> Result<crate::cache::CacheStats, RuntimeError> {
    log::info!("[V2] get_cache_stats");
    Ok(app_state.audio_cache.stats())
}

/// Clear audio cache (V2)
#[tauri::command]
pub async fn v2_clear_cache(
    app_state: State<'_, AppState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] clear_cache");
    app_state.audio_cache.clear();
    Ok(())
}

/// Clear artist cache (V2)
#[tauri::command]
pub async fn v2_clear_artist_cache(
    cache_state: State<'_, crate::api_cache::ApiCacheState>,
) -> Result<usize, RuntimeError> {
    log::info!("[V2] clear_artist_cache");
    let guard = cache_state.cache.lock().await;
    let cache = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    cache.clear_all_artists()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Clear vector store (V2)
#[tauri::command]
pub async fn v2_clear_vector_store(
    store_state: State<'_, crate::artist_vectors::ArtistVectorStoreState>,
) -> Result<usize, RuntimeError> {
    log::info!("[V2] clear_vector_store");
    let mut guard = store_state.store.lock().await;
    let store = guard.as_mut()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.clear_all()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get vector store stats (V2)
#[tauri::command]
pub async fn v2_get_vector_store_stats(
    store_state: State<'_, crate::artist_vectors::ArtistVectorStoreState>,
) -> Result<crate::artist_vectors::StoreStats, RuntimeError> {
    log::info!("[V2] get_vector_store_stats");
    let guard = store_state.store.lock().await;
    let store = guard.as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.get_stats()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Clear offline cache (V2)
#[tauri::command]
pub async fn v2_clear_offline_cache(
    cache_state: State<'_, crate::offline_cache::OfflineCacheState>,
    library_state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] clear_offline_cache");
    crate::offline_cache::commands::clear_offline_cache(cache_state, library_state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Clear saved credentials (V2)
#[tauri::command]
pub async fn v2_clear_saved_credentials() -> Result<(), RuntimeError> {
    log::info!("[V2] clear_saved_credentials");
    crate::commands::auth::clear_saved_credentials()
        .map_err(|e| RuntimeError::Internal(e))
}

// ==================== Playlist Custom Commands (V2) ====================

/// Set playlist sort (V2)
#[tauri::command]
pub async fn v2_playlist_set_sort(
    playlistId: u64,
    sortBy: String,
    sortOrder: String,
    state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] playlist_set_sort {} {} {}", playlistId, sortBy, sortOrder);
    crate::library::commands::playlist_set_sort(playlistId, sortBy, sortOrder, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set playlist artwork (V2)
#[tauri::command]
pub async fn v2_playlist_set_artwork(
    playlistId: u64,
    artworkPath: Option<String>,
    state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] playlist_set_artwork {} {:?}", playlistId, artworkPath);
    crate::library::commands::playlist_set_artwork(playlistId, artworkPath, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Add local track to playlist (V2)
#[tauri::command]
pub async fn v2_playlist_add_local_track(
    playlistId: u64,
    trackId: i64,
    position: i32,
    state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] playlist_add_local_track {} {} pos={}", playlistId, trackId, position);
    crate::library::commands::playlist_add_local_track(playlistId, trackId, position, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Remove local track from playlist (V2)
#[tauri::command]
pub async fn v2_playlist_remove_local_track(
    playlistId: u64,
    trackId: i64,
    state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] playlist_remove_local_track {} {}", playlistId, trackId);
    crate::library::commands::playlist_remove_local_track(playlistId, trackId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set playlist hidden (V2)
#[tauri::command]
pub async fn v2_playlist_set_hidden(
    playlistId: u64,
    hidden: bool,
    state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] playlist_set_hidden {} {}", playlistId, hidden);
    crate::library::commands::playlist_set_hidden(playlistId, hidden, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set playlist favorite (V2)
#[tauri::command]
pub async fn v2_playlist_set_favorite(
    playlistId: u64,
    favorite: bool,
    state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] playlist_set_favorite {} {}", playlistId, favorite);
    crate::library::commands::playlist_set_favorite(playlistId, favorite, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Reorder playlists (V2) - reorder playlists in sidebar
#[tauri::command]
pub async fn v2_playlist_reorder(
    playlistIds: Vec<u64>,
    state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] playlist_reorder {} playlists", playlistIds.len());
    crate::library::commands::playlist_reorder(playlistIds, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Init playlist custom order (V2) - initialize custom track order
/// trackIds is Vec<(track_id, is_local)>
#[tauri::command]
pub async fn v2_playlist_init_custom_order(
    playlistId: u64,
    trackIds: Vec<(i64, bool)>,
    state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] playlist_init_custom_order {} tracks={}", playlistId, trackIds.len());
    crate::library::commands::playlist_init_custom_order(playlistId, trackIds, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set playlist custom order (V2) - set track positions
/// orders is Vec<(track_id, is_local, position)>
#[tauri::command]
pub async fn v2_playlist_set_custom_order(
    playlistId: u64,
    orders: Vec<(i64, bool, i32)>,
    state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] playlist_set_custom_order {} tracks={}", playlistId, orders.len());
    crate::library::commands::playlist_set_custom_order(playlistId, orders, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Move track in playlist (V2)
#[tauri::command]
pub async fn v2_playlist_move_track(
    playlistId: u64,
    trackId: i64,
    isLocal: bool,
    newPosition: i32,
    state: State<'_, crate::library::commands::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] playlist_move_track {} {} local={} -> {}", playlistId, trackId, isLocal, newPosition);
    crate::library::commands::playlist_move_track(playlistId, trackId, isLocal, newPosition, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

// ============================================================================
// Cast / DLNA / AirPlay V2 Commands
// ============================================================================

// === Chromecast V2 ===

/// Start Chromecast discovery (V2)
#[tauri::command]
pub async fn v2_cast_start_discovery(
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_start_discovery");
    crate::cast::commands::cast_start_discovery(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Stop Chromecast discovery (V2)
#[tauri::command]
pub async fn v2_cast_stop_discovery(
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_stop_discovery");
    crate::cast::commands::cast_stop_discovery(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get discovered Chromecast devices (V2)
#[tauri::command]
pub async fn v2_cast_get_devices(
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<Vec<crate::cast::DiscoveredDevice>, RuntimeError> {
    log::info!("[V2] cast_get_devices");
    crate::cast::commands::cast_get_devices(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Connect to Chromecast device (V2)
#[tauri::command]
pub async fn v2_cast_connect(
    deviceId: String,
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_connect {}", deviceId);
    crate::cast::commands::cast_connect(deviceId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Disconnect from Chromecast (V2)
#[tauri::command]
pub async fn v2_cast_disconnect(
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_disconnect");
    crate::cast::commands::cast_disconnect(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get Chromecast status (V2)
#[tauri::command]
pub async fn v2_cast_get_status(
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<crate::cast::CastStatus, RuntimeError> {
    crate::cast::commands::cast_get_status(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get Chromecast position (V2)
#[tauri::command]
pub async fn v2_cast_get_position(
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<crate::cast::CastPositionInfo, RuntimeError> {
    crate::cast::commands::cast_get_position(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Play track on Chromecast (V2)
#[tauri::command]
pub async fn v2_cast_play_track(
    trackId: u64,
    metadata: crate::cast::MediaMetadata,
    state: State<'_, crate::cast::commands::CastState>,
    appState: State<'_, crate::AppState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_play_track {}", trackId);
    crate::cast::commands::cast_play_track(trackId, metadata, state, appState).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Play local track on Chromecast (V2)
#[tauri::command]
pub async fn v2_cast_play_local_track(
    trackId: i64,
    state: State<'_, crate::cast::commands::CastState>,
    libraryState: State<'_, crate::library::LibraryState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_play_local_track {}", trackId);
    crate::cast::commands::cast_play_local_track(trackId, state, libraryState).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Play on Chromecast (V2)
#[tauri::command]
pub async fn v2_cast_play(
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_play");
    crate::cast::commands::cast_play(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Pause Chromecast (V2)
#[tauri::command]
pub async fn v2_cast_pause(
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_pause");
    crate::cast::commands::cast_pause(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Stop Chromecast (V2)
#[tauri::command]
pub async fn v2_cast_stop(
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_stop");
    crate::cast::commands::cast_stop(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Seek on Chromecast (V2)
#[tauri::command]
pub async fn v2_cast_seek(
    positionSecs: f64,
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_seek {}", positionSecs);
    crate::cast::commands::cast_seek(positionSecs, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set Chromecast volume (V2)
#[tauri::command]
pub async fn v2_cast_set_volume(
    volume: f32,
    state: State<'_, crate::cast::commands::CastState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] cast_set_volume {}", volume);
    crate::cast::commands::cast_set_volume(volume, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

// === DLNA V2 ===

/// Start DLNA discovery (V2)
#[tauri::command]
pub async fn v2_dlna_start_discovery(
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] dlna_start_discovery");
    crate::cast::dlna::commands::dlna_start_discovery(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Stop DLNA discovery (V2)
#[tauri::command]
pub async fn v2_dlna_stop_discovery(
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] dlna_stop_discovery");
    crate::cast::dlna::commands::dlna_stop_discovery(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get discovered DLNA devices (V2)
#[tauri::command]
pub async fn v2_dlna_get_devices(
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<Vec<crate::cast::dlna::DiscoveredDlnaDevice>, RuntimeError> {
    log::info!("[V2] dlna_get_devices");
    crate::cast::dlna::commands::dlna_get_devices(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Connect to DLNA device (V2)
#[tauri::command]
pub async fn v2_dlna_connect(
    deviceId: String,
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] dlna_connect {}", deviceId);
    crate::cast::dlna::commands::dlna_connect(deviceId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Disconnect from DLNA device (V2)
#[tauri::command]
pub async fn v2_dlna_disconnect(
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] dlna_disconnect");
    crate::cast::dlna::commands::dlna_disconnect(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get DLNA status (V2)
#[tauri::command]
pub async fn v2_dlna_get_status(
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<crate::cast::dlna::DlnaStatus, RuntimeError> {
    crate::cast::dlna::commands::dlna_get_status(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get DLNA position (V2)
#[tauri::command]
pub async fn v2_dlna_get_position(
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<crate::cast::dlna::DlnaPositionInfo, RuntimeError> {
    crate::cast::dlna::commands::dlna_get_position(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Play track on DLNA device (V2)
#[tauri::command]
pub async fn v2_dlna_play_track(
    trackId: u64,
    metadata: crate::cast::dlna::DlnaMetadata,
    dlnaState: State<'_, crate::cast::dlna::commands::DlnaState>,
    appState: State<'_, crate::AppState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] dlna_play_track {}", trackId);
    crate::cast::dlna::commands::dlna_play_track(trackId, metadata, dlnaState, appState).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Load media on DLNA (stub) (V2)
#[tauri::command]
pub async fn v2_dlna_load_media(
    metadata: crate::cast::dlna::DlnaMetadata,
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<(), RuntimeError> {
    crate::cast::dlna::commands::dlna_load_media(metadata, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Play on DLNA (V2)
#[tauri::command]
pub async fn v2_dlna_play(
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] dlna_play");
    crate::cast::dlna::commands::dlna_play(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Pause DLNA (V2)
#[tauri::command]
pub async fn v2_dlna_pause(
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] dlna_pause");
    crate::cast::dlna::commands::dlna_pause(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Stop DLNA (V2)
#[tauri::command]
pub async fn v2_dlna_stop(
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] dlna_stop");
    crate::cast::dlna::commands::dlna_stop(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Seek DLNA (V2)
#[tauri::command]
pub async fn v2_dlna_seek(
    positionSecs: u64,
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] dlna_seek {}", positionSecs);
    crate::cast::dlna::commands::dlna_seek(positionSecs, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set DLNA volume (V2)
#[tauri::command]
pub async fn v2_dlna_set_volume(
    volume: f32,
    state: State<'_, crate::cast::dlna::commands::DlnaState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] dlna_set_volume {}", volume);
    crate::cast::dlna::commands::dlna_set_volume(volume, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

// === AirPlay V2 ===

/// Start AirPlay discovery (V2)
#[tauri::command]
pub async fn v2_airplay_start_discovery(
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] airplay_start_discovery");
    crate::cast::airplay::commands::airplay_start_discovery(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Stop AirPlay discovery (V2)
#[tauri::command]
pub async fn v2_airplay_stop_discovery(
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] airplay_stop_discovery");
    crate::cast::airplay::commands::airplay_stop_discovery(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get discovered AirPlay devices (V2)
#[tauri::command]
pub async fn v2_airplay_get_devices(
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<Vec<crate::cast::airplay::DiscoveredAirPlayDevice>, RuntimeError> {
    log::info!("[V2] airplay_get_devices");
    crate::cast::airplay::commands::airplay_get_devices(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Connect to AirPlay device (V2)
#[tauri::command]
pub async fn v2_airplay_connect(
    deviceId: String,
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] airplay_connect {}", deviceId);
    crate::cast::airplay::commands::airplay_connect(deviceId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Disconnect from AirPlay device (V2)
#[tauri::command]
pub async fn v2_airplay_disconnect(
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] airplay_disconnect");
    crate::cast::airplay::commands::airplay_disconnect(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get AirPlay status (V2)
#[tauri::command]
pub async fn v2_airplay_get_status(
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<crate::cast::airplay::AirPlayStatus, RuntimeError> {
    crate::cast::airplay::commands::airplay_get_status(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Load media on AirPlay (V2)
#[tauri::command]
pub async fn v2_airplay_load_media(
    metadata: crate::cast::airplay::AirPlayMetadata,
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] airplay_load_media");
    crate::cast::airplay::commands::airplay_load_media(metadata, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Play on AirPlay (V2)
#[tauri::command]
pub async fn v2_airplay_play(
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] airplay_play");
    crate::cast::airplay::commands::airplay_play(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Pause AirPlay (V2)
#[tauri::command]
pub async fn v2_airplay_pause(
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] airplay_pause");
    crate::cast::airplay::commands::airplay_pause(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Stop AirPlay (V2)
#[tauri::command]
pub async fn v2_airplay_stop(
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] airplay_stop");
    crate::cast::airplay::commands::airplay_stop(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Set AirPlay volume (V2)
#[tauri::command]
pub async fn v2_airplay_set_volume(
    volume: f32,
    state: State<'_, crate::cast::airplay::commands::AirPlayState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] airplay_set_volume {}", volume);
    crate::cast::airplay::commands::airplay_set_volume(volume, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

// ============================================================================
// Audio Backends V2 Commands
// ============================================================================

/// Get available audio backends (V2)
#[tauri::command]
pub fn v2_get_available_backends() -> Result<Vec<crate::commands::audio_backends::BackendInfo>, RuntimeError> {
    log::info!("[V2] get_available_backends");
    crate::commands::audio_backends::get_available_backends()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get devices for a specific backend (V2)
#[tauri::command]
pub fn v2_get_devices_for_backend(
    backendType: crate::audio::AudioBackendType,
) -> Result<Vec<crate::audio::AudioDevice>, RuntimeError> {
    log::info!("[V2] get_devices_for_backend {:?}", backendType);
    crate::commands::audio_backends::get_devices_for_backend(backendType)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get ALSA plugins (V2)
#[tauri::command]
pub fn v2_get_alsa_plugins() -> Result<Vec<crate::commands::audio_backends::AlsaPluginInfo>, RuntimeError> {
    log::info!("[V2] get_alsa_plugins");
    crate::commands::audio_backends::get_alsa_plugins()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Check if ALSA utils are installed (V2)
#[tauri::command]
pub fn v2_check_alsa_utils_installed() -> bool {
    crate::commands::audio_backends::check_alsa_utils_installed()
}

/// Get default device name for backend (V2)
#[tauri::command]
pub fn v2_get_default_device_name(
    backendType: crate::audio::AudioBackendType,
) -> Option<String> {
    log::info!("[V2] get_default_device_name {:?}", backendType);
    crate::commands::audio_backends::get_default_device_name(backendType)
}

/// Get Linux distro info (V2)
#[tauri::command]
pub fn v2_get_linux_distro() -> crate::commands::audio_backends::LinuxDistroInfo {
    log::info!("[V2] get_linux_distro");
    crate::commands::audio_backends::get_linux_distro()
}

/// Query DAC capabilities (V2)
#[tauri::command]
pub fn v2_query_dac_capabilities(
    nodeName: String,
) -> crate::commands::audio_backends::DacCapabilities {
    log::info!("[V2] query_dac_capabilities {}", nodeName);
    crate::commands::audio_backends::query_dac_capabilities(nodeName)
}

// ============================================================================
// Audio Diagnostics V2 Commands
// ============================================================================

/// Get hardware audio status (V2)
#[tauri::command]
pub fn v2_get_hardware_audio_status() -> Result<crate::commands::audio_diagnostics::HardwareAudioStatus, RuntimeError> {
    log::info!("[V2] get_hardware_audio_status");
    crate::commands::audio_diagnostics::get_hardware_audio_status()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Start bit-depth capture (V2)
#[tauri::command]
pub fn v2_start_bitdepth_capture(
    app: tauri::AppHandle,
) -> Result<String, RuntimeError> {
    log::info!("[V2] start_bitdepth_capture");
    crate::commands::audio_diagnostics::start_bitdepth_capture(app)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Stop bit-depth capture and analyze (V2)
#[tauri::command]
pub fn v2_stop_bitdepth_capture(
    app: tauri::AppHandle,
) -> Result<crate::audio::BitDepthResult, RuntimeError> {
    log::info!("[V2] stop_bitdepth_capture");
    crate::commands::audio_diagnostics::stop_bitdepth_capture(app)
        .map_err(|e| RuntimeError::Internal(e))
}

// ============================================================================
// Radio V2 Commands
// ============================================================================

/// Create artist radio (V2)
#[tauri::command]
pub async fn v2_create_artist_radio(
    artistId: u64,
    artistName: String,
    state: State<'_, crate::AppState>,
    blacklistState: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<String, RuntimeError> {
    log::info!("[V2] create_artist_radio {} {}", artistId, artistName);
    crate::commands::radio::create_artist_radio(artistId, artistName, state, blacklistState).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Create track radio (V2)
#[tauri::command]
pub async fn v2_create_track_radio(
    trackId: u64,
    trackName: String,
    artistId: u64,
    state: State<'_, crate::AppState>,
    blacklistState: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<String, RuntimeError> {
    log::info!("[V2] create_track_radio {} {} {}", trackId, trackName, artistId);
    crate::commands::radio::create_track_radio(trackId, trackName, artistId, state, blacklistState).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Refill radio queue (V2)
#[tauri::command]
pub async fn v2_refill_radio_queue(
    sessionId: String,
    state: State<'_, crate::AppState>,
    blacklistState: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<u32, RuntimeError> {
    log::info!("[V2] refill_radio_queue {}", sessionId);
    crate::commands::radio::refill_radio_queue(sessionId, state, blacklistState).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get queue remaining count (V2)
#[tauri::command]
pub fn v2_get_queue_remaining(
    state: State<'_, crate::AppState>,
) -> u32 {
    crate::commands::radio::get_queue_remaining(state)
}

/// Create infinite radio from recent tracks (V2)
#[tauri::command]
pub async fn v2_create_infinite_radio(
    recentTrackIds: Vec<u64>,
    state: State<'_, crate::AppState>,
    blacklistState: State<'_, crate::artist_blacklist::BlacklistState>,
) -> Result<String, RuntimeError> {
    log::info!("[V2] create_infinite_radio {:?}", recentTrackIds);
    crate::commands::radio::create_infinite_radio(recentTrackIds, state, blacklistState).await
        .map_err(|e| RuntimeError::Internal(e))
}

// ============================================================================
// Share V2 Commands
// ============================================================================

/// Get Qobuz track URL (V2)
#[tauri::command]
pub fn v2_get_qobuz_track_url(trackId: u64) -> String {
    crate::commands::share::get_qobuz_track_url(trackId)
}

/// Get Qobuz album URL (V2)
#[tauri::command]
pub fn v2_get_qobuz_album_url(albumId: String) -> String {
    crate::commands::share::get_qobuz_album_url(albumId)
}

/// Get Qobuz artist URL (V2)
#[tauri::command]
pub fn v2_get_qobuz_artist_url(artistId: u64) -> String {
    crate::commands::share::get_qobuz_artist_url(artistId)
}

// ============================================================================
// Visualizer V2 Commands
// ============================================================================

/// Check if visualizer is enabled (V2)
#[tauri::command]
pub fn v2_is_visualizer_enabled(
    state: State<'_, crate::AppState>,
) -> bool {
    crate::commands::visualizer::is_visualizer_enabled(state)
}

// ============================================================================
// Queue V2 Additional Commands
// ============================================================================

/// Get current queue track (V2)
#[tauri::command]
pub fn v2_get_current_queue_track(
    state: State<'_, crate::AppState>,
) -> Result<Option<crate::queue::QueueTrack>, RuntimeError> {
    crate::commands::queue::get_current_queue_track(state)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Peek next track (V2)
#[tauri::command]
pub fn v2_peek_next_track(
    state: State<'_, crate::AppState>,
) -> Result<Option<crate::queue::QueueTrack>, RuntimeError> {
    crate::commands::queue::peek_next_track(state)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get shuffle state (V2)
#[tauri::command]
pub fn v2_get_shuffle(
    state: State<'_, crate::AppState>,
) -> Result<bool, RuntimeError> {
    crate::commands::queue::get_shuffle(state)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get repeat mode (V2)
#[tauri::command]
pub fn v2_get_repeat(
    state: State<'_, crate::AppState>,
) -> Result<String, RuntimeError> {
    crate::commands::queue::get_repeat(state)
        .map_err(|e| RuntimeError::Internal(e))
}

// ============================================================================
// Playlist Additional V2 Commands
// ============================================================================

/// Get current user ID (V2)
#[tauri::command]
pub async fn v2_get_current_user_id(
    state: State<'_, crate::AppState>,
) -> Result<Option<u64>, RuntimeError> {
    crate::commands::playlist::get_current_user_id(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

// ============================================================================
// Auth V2 Additional Commands
// ============================================================================

/// Has saved credentials (V2)
#[tauri::command]
pub fn v2_has_saved_credentials() -> bool {
    crate::commands::auth::has_saved_credentials()
}

/// Save credentials (V2)
#[tauri::command]
pub fn v2_save_credentials(
    email: String,
    password: String,
) -> Result<(), RuntimeError> {
    log::info!("[V2] save_credentials");
    crate::commands::auth::save_credentials(email, password)
        .map_err(|e| RuntimeError::Internal(e))
}

// ============================================================================
// Plex V2 Commands
// ============================================================================

/// Plex ping (V2)
#[tauri::command]
pub async fn v2_plex_ping(
    baseUrl: String,
    token: String,
) -> Result<crate::plex::PlexServerInfo, RuntimeError> {
    log::info!("[V2] plex_ping");
    crate::plex::plex_ping(baseUrl, token).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex get music sections (V2)
#[tauri::command]
pub async fn v2_plex_get_music_sections(
    baseUrl: String,
    token: String,
) -> Result<Vec<crate::plex::PlexMusicSection>, RuntimeError> {
    log::info!("[V2] plex_get_music_sections");
    crate::plex::plex_get_music_sections(baseUrl, token).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex get section tracks (V2)
#[tauri::command]
pub async fn v2_plex_get_section_tracks(
    baseUrl: String,
    token: String,
    sectionKey: String,
    limit: Option<u32>,
) -> Result<Vec<crate::plex::PlexTrack>, RuntimeError> {
    log::info!("[V2] plex_get_section_tracks section={}", sectionKey);
    crate::plex::plex_get_section_tracks(baseUrl, token, sectionKey, limit).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex get track metadata (V2)
#[tauri::command]
pub async fn v2_plex_get_track_metadata(
    baseUrl: String,
    token: String,
    ratingKey: String,
) -> Result<crate::plex::PlexTrack, RuntimeError> {
    log::info!("[V2] plex_get_track_metadata rating_key={}", ratingKey);
    crate::plex::plex_get_track_metadata(baseUrl, token, ratingKey).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex auth pin start (V2)
#[tauri::command]
pub async fn v2_plex_auth_pin_start(
    clientIdentifier: String,
) -> Result<crate::plex::PlexPinStartResult, RuntimeError> {
    log::info!("[V2] plex_auth_pin_start");
    crate::plex::plex_auth_pin_start(clientIdentifier).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex auth pin check (V2)
#[tauri::command]
pub async fn v2_plex_auth_pin_check(
    clientIdentifier: String,
    pinId: u64,
    code: Option<String>,
) -> Result<crate::plex::PlexPinCheckResult, RuntimeError> {
    log::info!("[V2] plex_auth_pin_check pin_id={}", pinId);
    crate::plex::plex_auth_pin_check(clientIdentifier, pinId, code).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex open auth URL (V2)
#[tauri::command]
pub async fn v2_plex_open_auth_url(
    url: String,
) -> Result<(), RuntimeError> {
    log::info!("[V2] plex_open_auth_url");
    crate::plex::plex_open_auth_url(url).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex cache get sections (V2)
#[tauri::command]
pub fn v2_plex_cache_get_sections() -> Result<Vec<crate::plex::PlexMusicSection>, RuntimeError> {
    log::info!("[V2] plex_cache_get_sections");
    crate::plex::plex_cache_get_sections()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex cache save sections (V2)
#[tauri::command]
pub fn v2_plex_cache_save_sections(
    serverId: Option<String>,
    sections: Vec<crate::plex::PlexMusicSection>,
) -> Result<usize, RuntimeError> {
    log::info!("[V2] plex_cache_save_sections count={}", sections.len());
    crate::plex::plex_cache_save_sections(serverId, sections)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex cache get tracks (V2)
#[tauri::command]
pub fn v2_plex_cache_get_tracks(
    sectionKey: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<crate::plex::PlexTrack>, RuntimeError> {
    log::info!("[V2] plex_cache_get_tracks section={:?}", sectionKey);
    crate::plex::plex_cache_get_tracks(sectionKey, limit)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex cache get albums (V2)
#[tauri::command]
pub fn v2_plex_cache_get_albums() -> Result<Vec<crate::plex::PlexCachedAlbum>, RuntimeError> {
    log::info!("[V2] plex_cache_get_albums");
    crate::plex::plex_cache_get_albums()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex cache get album tracks (V2)
#[tauri::command]
pub fn v2_plex_cache_get_album_tracks(
    albumKey: String,
) -> Result<Vec<crate::plex::PlexCachedTrack>, RuntimeError> {
    log::info!("[V2] plex_cache_get_album_tracks album={}", albumKey);
    crate::plex::plex_cache_get_album_tracks(albumKey)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex cache search tracks (V2)
#[tauri::command]
pub fn v2_plex_cache_search_tracks(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<crate::plex::PlexCachedTrack>, RuntimeError> {
    log::info!("[V2] plex_cache_search_tracks query={}", query);
    crate::plex::plex_cache_search_tracks(query, limit)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex cache save tracks (V2)
#[tauri::command]
pub fn v2_plex_cache_save_tracks(
    serverId: Option<String>,
    sectionKey: String,
    tracks: Vec<crate::plex::PlexTrack>,
) -> Result<usize, RuntimeError> {
    log::info!("[V2] plex_cache_save_tracks section={} count={}", sectionKey, tracks.len());
    crate::plex::plex_cache_save_tracks(serverId, sectionKey, tracks)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex cache update track quality (V2)
#[tauri::command]
pub fn v2_plex_cache_update_track_quality(
    updates: Vec<crate::plex::PlexTrackQualityUpdate>,
) -> Result<usize, RuntimeError> {
    log::info!("[V2] plex_cache_update_track_quality count={}", updates.len());
    crate::plex::plex_cache_update_track_quality(updates)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex cache get tracks needing hydration (V2)
#[tauri::command]
pub fn v2_plex_cache_get_tracks_needing_hydration(
    limit: Option<u32>,
) -> Result<Vec<String>, RuntimeError> {
    log::info!("[V2] plex_cache_get_tracks_needing_hydration");
    crate::plex::plex_cache_get_tracks_needing_hydration(limit)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex cache clear (V2)
#[tauri::command]
pub fn v2_plex_cache_clear() -> Result<(), RuntimeError> {
    log::info!("[V2] plex_cache_clear");
    crate::plex::plex_cache_clear()
        .map_err(|e| RuntimeError::Internal(e))
}

/// Plex play track (V2)
#[tauri::command]
pub async fn v2_plex_play_track(
    baseUrl: String,
    token: String,
    ratingKey: String,
    appState: tauri::State<'_, crate::AppState>,
) -> Result<crate::plex::PlexPlayResult, RuntimeError> {
    log::info!("[V2] plex_play_track rating_key={}", ratingKey);
    crate::plex::plex_play_track(baseUrl, token, ratingKey, appState).await
        .map_err(|e| RuntimeError::Internal(e))
}

// ============================================================================
// Playlist Additional V2 Commands
// ============================================================================

/// Get playlist settings (V2)
#[tauri::command]
pub async fn v2_playlist_get_settings(
    playlistId: u64,
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<Option<crate::library::PlaylistSettings>, RuntimeError> {
    log::info!("[V2] playlist_get_settings {}", playlistId);
    crate::library::commands::playlist_get_settings(playlistId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get all playlist settings (V2)
#[tauri::command]
pub async fn v2_playlist_get_all_settings(
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<Vec<crate::library::PlaylistSettings>, RuntimeError> {
    log::info!("[V2] playlist_get_all_settings");
    crate::library::commands::playlist_get_all_settings(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get playlist stats (V2)
#[tauri::command]
pub async fn v2_playlist_get_stats(
    playlistId: u64,
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<Option<crate::library::PlaylistStats>, RuntimeError> {
    log::info!("[V2] playlist_get_stats {}", playlistId);
    crate::library::commands::playlist_get_stats(playlistId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get all playlist stats (V2)
#[tauri::command]
pub async fn v2_playlist_get_all_stats(
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<Vec<crate::library::PlaylistStats>, RuntimeError> {
    log::info!("[V2] playlist_get_all_stats");
    crate::library::commands::playlist_get_all_stats(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Increment playlist play count (V2)
#[tauri::command]
pub async fn v2_playlist_increment_play_count(
    playlistId: u64,
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<crate::library::PlaylistStats, RuntimeError> {
    log::info!("[V2] playlist_increment_play_count {}", playlistId);
    crate::library::commands::playlist_increment_play_count(playlistId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get favorite playlist IDs (V2)
#[tauri::command]
pub async fn v2_playlist_get_favorites(
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<Vec<u64>, RuntimeError> {
    log::info!("[V2] playlist_get_favorites");
    crate::library::commands::playlist_get_favorites(state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get local tracks in playlist (V2)
#[tauri::command]
pub async fn v2_playlist_get_local_tracks(
    playlistId: u64,
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<Vec<crate::library::LocalTrack>, RuntimeError> {
    log::info!("[V2] playlist_get_local_tracks {}", playlistId);
    crate::library::commands::playlist_get_local_tracks(playlistId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get local tracks in playlist with positions (V2)
#[tauri::command]
pub async fn v2_playlist_get_local_tracks_with_position(
    playlistId: u64,
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<Vec<crate::library::PlaylistLocalTrack>, RuntimeError> {
    log::info!("[V2] playlist_get_local_tracks_with_position {}", playlistId);
    crate::library::commands::playlist_get_local_tracks_with_position(playlistId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get playlist custom order (V2)
#[tauri::command]
pub async fn v2_playlist_get_custom_order(
    playlistId: u64,
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<Vec<(i64, bool, i32)>, RuntimeError> {
    log::info!("[V2] playlist_get_custom_order {}", playlistId);
    crate::library::commands::playlist_get_custom_order(playlistId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Check if playlist has custom order (V2)
#[tauri::command]
pub async fn v2_playlist_has_custom_order(
    playlistId: u64,
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<bool, RuntimeError> {
    log::info!("[V2] playlist_has_custom_order {}", playlistId);
    crate::library::commands::playlist_has_custom_order(playlistId, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Get tracks with local copies (V2)
#[tauri::command]
pub async fn v2_playlist_get_tracks_with_local_copies(
    trackIds: Vec<u64>,
    state: tauri::State<'_, crate::library::commands::LibraryState>,
) -> Result<Vec<u64>, RuntimeError> {
    log::info!("[V2] playlist_get_tracks_with_local_copies count={}", trackIds.len());
    crate::library::commands::playlist_get_tracks_with_local_copies(trackIds, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Playlist import preview (V2)
#[tauri::command]
pub async fn v2_playlist_import_preview(
    url: String,
) -> Result<crate::playlist_import::ImportPlaylist, RuntimeError> {
    log::info!("[V2] playlist_import_preview {}", url);
    crate::commands::playlist_import::playlist_import_preview(url).await
        .map_err(|e| RuntimeError::Internal(e))
}

/// Playlist import execute (V2)
#[tauri::command]
pub async fn v2_playlist_import_execute(
    app: tauri::AppHandle,
    url: String,
    nameOverride: Option<String>,
    isPublic: Option<bool>,
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::playlist_import::ImportSummary, RuntimeError> {
    log::info!("[V2] playlist_import_execute {}", url);
    crate::commands::playlist_import::playlist_import_execute(app, url, nameOverride, isPublic, state).await
        .map_err(|e| RuntimeError::Internal(e))
}

