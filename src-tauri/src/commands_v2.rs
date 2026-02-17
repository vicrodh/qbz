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
use crate::artist_vectors::ArtistVectorStoreState;
use crate::cache::{AudioCache, CacheStats};
use crate::api::models::PlaylistWithTrackIds;
use crate::api_cache::ApiCacheState;
use crate::audio::{AlsaPlugin, AudioBackendType, AudioDevice, BackendManager};
use crate::cast::{AirPlayMetadata, AirPlayState, CastState, DlnaMetadata, DlnaState, MediaMetadata};
use crate::config::audio_settings::{AudioSettings, AudioSettingsState};
use crate::config::developer_settings::{DeveloperSettings, DeveloperSettingsState};
use crate::config::download_settings::DownloadSettingsState;
use crate::config::favorites_preferences::FavoritesPreferences;
use crate::config::graphics_settings::{GraphicsSettings, GraphicsSettingsState, GraphicsStartupStatus};
use crate::config::legal_settings::LegalSettingsState;
use crate::config::playback_preferences::{AutoplayMode, PlaybackPreferences, PlaybackPreferencesState};
use crate::config::tray_settings::TraySettings;
use crate::config::tray_settings::TraySettingsState;
use crate::config::window_settings::WindowSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::library::{thumbnails, LibraryState, LocalAlbum, LocalTrack, MetadataExtractor, PlaylistLocalTrack, PlaylistSettings, PlaylistStats, ScanProgress, get_artwork_cache_dir};
use crate::lyrics::LyricsState;
use crate::musicbrainz::{CacheStats as MusicBrainzCacheStats, MusicBrainzSharedState};
use crate::offline::OfflineState;
use crate::offline_cache::OfflineCacheState;
use crate::playback_context::{ContentSource, ContextType, PlaybackContext};
use crate::plex::{PlexMusicSection, PlexPlayResult, PlexServerInfo, PlexTrack};
use crate::queue::QueueTrack;
use crate::reco_store::{HomeResolved, HomeSeeds, RecoEventInput, RecoState};
use crate::runtime::{RuntimeManagerState, RuntimeStatus, RuntimeError, RuntimeEvent, DegradedReason, CommandRequirement};
use crate::AppState;
use md5::{Digest, Md5};
use notify_rust::Notification;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

// ==================== Helper Functions ====================

/// Backend information for UI display
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackendInfo {
    pub backend_type: AudioBackendType,
    pub name: String,
    pub description: String,
    pub is_available: bool,
}

/// ALSA plugin information for UI display
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlsaPluginInfo {
    pub plugin: AlsaPlugin,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "content", rename_all = "lowercase")]
pub enum V2MostPopularItem {
    Tracks(Track),
    Albums(Album),
    Artists(Artist),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct V2SearchAllResults {
    pub albums: SearchResultsPage<Album>,
    pub tracks: SearchResultsPage<Track>,
    pub artists: SearchResultsPage<Artist>,
    pub playlists: SearchResultsPage<Playlist>,
    pub most_popular: Option<V2MostPopularItem>,
}

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

fn v2_teardown_type_alias_state<S>(state: &Arc<Mutex<Option<S>>>) {
    if let Ok(mut guard) = state.lock() {
        *guard = None;
    }
}

fn v2_get_notification_artwork_cache_dir() -> Result<PathBuf, String> {
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| "Could not find cache directory".to_string())?
        .join("qbz")
        .join("artwork");

    fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create artwork cache dir: {}", e))?;
    Ok(cache_dir)
}

fn v2_resolve_local_artwork(url: &str) -> Option<PathBuf> {
    if let Some(path) = url.strip_prefix("file://") {
        return Some(PathBuf::from(path));
    }
    if let Some(path) = url.strip_prefix("asset://localhost/") {
        let decoded = urlencoding::decode(path).ok()?;
        return Some(PathBuf::from(decoded.into_owned()));
    }
    None
}

fn v2_cache_notification_artwork(url: &str) -> Result<PathBuf, String> {
    if let Some(local_path) = v2_resolve_local_artwork(url) {
        if local_path.exists() {
            return Ok(local_path);
        }
    }

    let mut hasher = Md5::new();
    hasher.update(url.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let cache_dir = v2_get_notification_artwork_cache_dir()?;
    let cache_path = cache_dir.join(format!("{}.jpg", hash));
    if cache_path.exists() {
        return Ok(cache_path);
    }

    let response = reqwest::blocking::Client::new()
        .get(url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .map_err(|e| format!("Failed to download artwork: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Failed to download artwork: HTTP {}", response.status()));
    }

    let bytes = response
        .bytes()
        .map_err(|e| format!("Failed to read artwork bytes: {}", e))?;

    let mut file = fs::File::create(&cache_path)
        .map_err(|e| format!("Failed to create artwork cache file: {}", e))?;
    file.write_all(&bytes)
        .map_err(|e| format!("Failed to write artwork cache: {}", e))?;
    Ok(cache_path)
}

fn v2_format_notification_quality(bit_depth: Option<u32>, sample_rate: Option<f64>) -> String {
    match (bit_depth, sample_rate) {
        (Some(bits), Some(rate)) if bits >= 24 || rate > 48.0 => {
            let rate_str = if rate.fract() == 0.0 {
                format!("{}", rate as u32)
            } else {
                format!("{}", rate)
            };
            format!("Hi-Res - {}-bit/{}kHz", bits, rate_str)
        }
        (Some(bits), Some(rate)) => {
            let rate_str = if rate.fract() == 0.0 {
                format!("{}", rate as u32)
            } else {
                format!("{}", rate)
            };
            format!("CD Quality - {}-bit/{}kHz", bits, rate_str)
        }
        _ => String::new(),
    }
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

    // Step 3: Check for saved credentials and attempt auto-login.
    // NOTE: user_id hint is optional; credentials are the source of truth.
    // This keeps bootstrap robust even if last_user_id is missing/corrupt.
    let creds = crate::credentials::load_qobuz_credentials();
    let last_user_id_hint = crate::user_data::UserDataPaths::load_last_user_id();

    if let Ok(Some(creds)) = creds {
        if let Some(uid) = last_user_id_hint {
            log::info!(
                "[Runtime] Found saved credentials, attempting auto-login (user hint: {})",
                uid
            );
        } else {
            log::info!("[Runtime] Found saved credentials, attempting auto-login (no user hint)");
        }

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
    Ok(client.get_user_info().await.map(|(name, subscription, valid_until)| V2UserInfo {
        user_name: name,
        subscription,
        subscription_valid_until: valid_until,
    }))
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

// ==================== UX / Settings Commands (V2 Native) ====================

#[tauri::command]
pub async fn v2_set_api_locale(
    locale: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
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
pub fn v2_set_enable_tray(
    value: bool,
    state: State<'_, TraySettingsState>,
) -> Result<(), String> {
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
pub fn v2_get_playback_preferences(
    state: State<'_, PlaybackPreferencesState>,
) -> Result<PlaybackPreferences, String> {
    state.get_preferences()
}

#[tauri::command]
pub fn v2_get_tray_settings(
    state: State<'_, TraySettingsState>,
) -> Result<TraySettings, String> {
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

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_ping(
    baseUrl: String,
    token: String,
) -> Result<PlexServerInfo, String> {
    crate::plex::plex_ping(baseUrl, token).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_get_track_metadata(
    baseUrl: String,
    token: String,
    ratingKey: String,
) -> Result<crate::plex::PlexTrack, String> {
    crate::plex::plex_get_track_metadata(baseUrl, token, ratingKey).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_auth_pin_start(
    clientIdentifier: String,
) -> Result<crate::plex::PlexPinStartResult, String> {
    crate::plex::plex_auth_pin_start(clientIdentifier).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_auth_pin_check(
    clientIdentifier: String,
    pinId: u64,
    code: Option<String>,
) -> Result<crate::plex::PlexPinCheckResult, String> {
    crate::plex::plex_auth_pin_check(clientIdentifier, pinId, code).await
}

#[tauri::command]
pub fn v2_set_visualizer_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.visualizer.set_enabled(enabled);
    Ok(())
}

#[tauri::command]
pub fn v2_get_developer_settings(
    state: State<'_, DeveloperSettingsState>,
) -> Result<DeveloperSettings, String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Developer settings store not initialized")?;
    store.get_settings()
}

#[tauri::command]
pub fn v2_set_developer_force_dmabuf(
    enabled: bool,
    state: State<'_, DeveloperSettingsState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Developer settings store not initialized")?;
    store.set_force_dmabuf(enabled)
}

#[tauri::command]
pub fn v2_get_graphics_settings(
    state: State<'_, GraphicsSettingsState>,
) -> Result<GraphicsSettings, String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.get_settings()
}

#[tauri::command]
pub fn v2_get_graphics_startup_status() -> GraphicsStartupStatus {
    crate::config::graphics_settings::get_graphics_startup_status()
}

#[tauri::command]
pub fn v2_set_hardware_acceleration(
    enabled: bool,
    state: State<'_, GraphicsSettingsState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_hardware_acceleration(enabled)
}

#[tauri::command]
pub fn v2_set_gdk_scale(
    value: Option<String>,
    state: State<'_, GraphicsSettingsState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_gdk_scale(value)
}

#[tauri::command]
pub fn v2_set_gdk_dpi_scale(
    value: Option<String>,
    state: State<'_, GraphicsSettingsState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_gdk_dpi_scale(value)
}

#[tauri::command]
pub fn v2_clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    state.audio_cache.clear();
    Ok(())
}

#[tauri::command]
pub async fn v2_clear_artist_cache(
    cache_state: State<'_, ApiCacheState>,
) -> Result<usize, String> {
    let guard = cache_state.cache.lock().await;
    let cache = guard.as_ref().ok_or("No active session - please log in")?;
    cache.clear_all_artists()
}

#[tauri::command]
pub async fn v2_get_vector_store_stats(
    store_state: State<'_, ArtistVectorStoreState>,
) -> Result<crate::artist_vectors::StoreStats, String> {
    let guard = store_state.store.lock().await;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_stats()
}

#[tauri::command]
pub async fn v2_clear_vector_store(
    store_state: State<'_, ArtistVectorStoreState>,
) -> Result<usize, String> {
    let mut guard = store_state.store.lock().await;
    let store = guard.as_mut().ok_or("No active session - please log in")?;
    store.clear_all()
}

#[tauri::command]
pub fn v2_add_to_artist_blacklist(
    artist_id: u64,
    artist_name: String,
    notes: Option<String>,
    state: State<'_, BlacklistState>,
) -> Result<(), String> {
    state.add(artist_id, &artist_name, notes.as_deref())
}

#[tauri::command]
pub fn v2_remove_from_artist_blacklist(
    artist_id: u64,
    state: State<'_, BlacklistState>,
) -> Result<(), String> {
    state.remove(artist_id)
}

#[tauri::command]
pub fn v2_set_blacklist_enabled(
    enabled: bool,
    state: State<'_, BlacklistState>,
) -> Result<(), String> {
    state.set_enabled(enabled)
}

#[tauri::command]
pub fn v2_clear_artist_blacklist(
    state: State<'_, BlacklistState>,
) -> Result<(), String> {
    state.clear_all()
}

#[tauri::command]
pub fn v2_get_artist_blacklist(
    state: State<'_, BlacklistState>,
) -> Result<Vec<crate::artist_blacklist::BlacklistedArtist>, String> {
    state.get_all()
}

#[tauri::command]
pub fn v2_get_blacklist_settings(
    state: State<'_, BlacklistState>,
) -> Result<crate::artist_blacklist::BlacklistSettings, String> {
    state.get_settings()
}

#[tauri::command]
pub fn v2_save_credentials(email: String, password: String) -> Result<(), String> {
    crate::credentials::save_qobuz_credentials(&email, &password)
}

#[tauri::command]
pub fn v2_clear_saved_credentials() -> Result<(), String> {
    crate::credentials::clear_qobuz_credentials()
}

#[tauri::command]
pub async fn v2_plex_open_auth_url(url: String) -> Result<(), String> {
    crate::plex::plex_open_auth_url(url).await
}

#[tauri::command]
pub fn v2_plex_cache_save_sections(
    server_id: Option<String>,
    sections: Vec<crate::plex::PlexMusicSection>,
) -> Result<usize, String> {
    crate::plex::plex_cache_save_sections(server_id, sections)
}

#[tauri::command]
pub fn v2_plex_cache_get_sections() -> Result<Vec<crate::plex::PlexMusicSection>, String> {
    crate::plex::plex_cache_get_sections()
}

#[tauri::command]
pub fn v2_plex_cache_save_tracks(
    server_id: Option<String>,
    section_key: String,
    tracks: Vec<crate::plex::PlexTrack>,
) -> Result<usize, String> {
    crate::plex::plex_cache_save_tracks(server_id, section_key, tracks)
}

#[tauri::command]
pub fn v2_plex_cache_clear() -> Result<(), String> {
    crate::plex::plex_cache_clear()
}

#[tauri::command]
pub fn v2_plex_cache_get_tracks(
    sectionKey: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<crate::plex::PlexTrack>, String> {
    crate::plex::plex_cache_get_tracks(sectionKey, limit)
}

#[tauri::command]
pub fn v2_plex_cache_get_albums() -> Result<Vec<crate::plex::PlexCachedAlbum>, String> {
    crate::plex::plex_cache_get_albums()
}

#[tauri::command]
pub fn v2_plex_cache_search_tracks(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<crate::plex::PlexCachedTrack>, String> {
    crate::plex::plex_cache_search_tracks(query, limit)
}

#[tauri::command]
pub fn v2_plex_cache_get_album_tracks(
    albumKey: String,
) -> Result<Vec<crate::plex::PlexCachedTrack>, String> {
    crate::plex::plex_cache_get_album_tracks(albumKey)
}

#[tauri::command]
pub fn v2_plex_cache_update_track_quality(
    updates: Vec<crate::plex::PlexTrackQualityUpdate>,
) -> Result<usize, String> {
    crate::plex::plex_cache_update_track_quality(updates)
}

#[tauri::command]
pub fn v2_plex_cache_get_tracks_needing_hydration(
    limit: Option<u32>,
) -> Result<Vec<String>, String> {
    crate::plex::plex_cache_get_tracks_needing_hydration(limit)
}

// ==================== Casting / Local Library Commands (V2 Native) ====================

#[tauri::command]
pub async fn v2_cast_start_discovery(state: State<'_, CastState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.start_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_stop_discovery(state: State<'_, CastState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.stop_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_get_devices(
    state: State<'_, CastState>,
) -> Result<Vec<crate::cast::DiscoveredDevice>, String> {
    let discovery = state.discovery.lock().await;
    Ok(discovery.get_discovered_devices())
}

#[tauri::command]
pub async fn v2_cast_connect(device_id: String, state: State<'_, CastState>) -> Result<(), String> {
    let device = {
        let discovery = state.discovery.lock().await;
        discovery
            .get_device(&device_id)
            .ok_or_else(|| format!("Device not found: {}", device_id))?
    };
    state
        .chromecast
        .connect(device.ip.clone(), device.port)
        .map_err(|e| e.to_string())?;
    let mut connected = state.connected_device_ip.lock().await;
    *connected = Some(device.ip);
    Ok(())
}

#[tauri::command]
pub async fn v2_cast_disconnect(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.disconnect().map_err(|e| e.to_string())?;
    let mut connected = state.connected_device_ip.lock().await;
    *connected = None;
    Ok(())
}

#[tauri::command]
pub async fn v2_cast_play_track(
    track_id: u64,
    metadata: MediaMetadata,
    cast_state: State<'_, CastState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let stream_url = {
        let client = app_state.client.read().await;
        client
            .get_stream_url_with_fallback(track_id, crate::api::models::Quality::HiRes)
            .await
            .map_err(|e| format!("Failed to get stream URL: {}", e))?
    };

    let content_type = stream_url.mime_type.clone();
    let cache = app_state.audio_cache.clone();
    let audio_data = if let Some(cached) = cache.get(track_id) {
        cached.data
    } else {
        let data = download_audio(&stream_url.url).await?;
        cache.insert(track_id, data.clone());
        data
    };

    let target_ip = {
        let connected = cast_state.connected_device_ip.lock().await;
        connected.clone()
    };

    cast_state
        .get_or_create_media_server()
        .await
        .map_err(|e| e.to_string())?;

    let url = {
        let mut server_guard = cast_state.media_server.lock().await;
        let server = server_guard.as_mut().ok_or("Media server not initialized")?;
        server.register_audio(track_id, audio_data, &content_type);
        match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(track_id, ip),
            None => server.get_audio_url(track_id),
        }
        .ok_or_else(|| "Failed to build media URL".to_string())?
    };

    cast_state
        .chromecast
        .load_media(url, content_type, metadata)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_play(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.play().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_pause(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.pause().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_stop(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.stop().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_seek(position_secs: f64, state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.seek(position_secs).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_set_volume(volume: f32, state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.set_volume(volume).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_start_discovery(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.start_discovery().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_stop_discovery(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.stop_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_get_devices(
    state: State<'_, DlnaState>,
) -> Result<Vec<crate::cast::DiscoveredDlnaDevice>, String> {
    let discovery = state.discovery.lock().await;
    Ok(discovery.get_discovered_devices())
}

#[tauri::command]
pub async fn v2_dlna_connect(device_id: String, state: State<'_, DlnaState>) -> Result<(), String> {
    let device = {
        let discovery = state.discovery.lock().await;
        discovery
            .get_device(&device_id)
            .ok_or_else(|| format!("Device not found: {}", device_id))?
    };
    let connection = crate::cast::DlnaConnection::connect(device)
        .await
        .map_err(|e| e.to_string())?;
    let mut state_connection = state.connection.lock().await;
    *state_connection = Some(connection);
    Ok(())
}

#[tauri::command]
pub async fn v2_dlna_disconnect(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    if let Some(conn) = connection.as_mut() {
        conn.disconnect().map_err(|e| e.to_string())?;
    }
    *connection = None;
    Ok(())
}

#[tauri::command]
pub async fn v2_dlna_play_track(
    track_id: u64,
    metadata: DlnaMetadata,
    dlna_state: State<'_, DlnaState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let stream_url = {
        let client = app_state.client.read().await;
        client
            .get_stream_url_with_fallback(track_id, crate::api::models::Quality::HiRes)
            .await
            .map_err(|e| format!("Failed to get stream URL: {}", e))?
    };

    let content_type = stream_url.mime_type.clone();
    let cache = app_state.audio_cache.clone();
    let audio_data = if let Some(cached) = cache.get(track_id) {
        cached.data
    } else {
        let data = download_audio(&stream_url.url).await?;
        cache.insert(track_id, data.clone());
        data
    };

    let target_ip = {
        let connection = dlna_state.connection.lock().await;
        connection.as_ref().map(|conn| conn.device_ip().to_string())
    };

    dlna_state
        .ensure_media_server()
        .await
        .map_err(|e| e.to_string())?;

    let url = {
        let mut server_guard = dlna_state.media_server.lock().await;
        let server = server_guard.as_mut().ok_or("Media server not initialized")?;
        server.register_audio(track_id, audio_data, &content_type);
        match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(track_id, ip),
            None => server.get_audio_url(track_id),
        }
        .ok_or_else(|| "Failed to build media URL".to_string())?
    };

    {
        let mut connection = dlna_state.connection.lock().await;
        let conn = connection.as_mut().ok_or("Not connected")?;
        conn.load_media(&url, &metadata, &content_type)
            .await
            .map_err(|e| e.to_string())?;
    }
    {
        let mut connection = dlna_state.connection.lock().await;
        let conn = connection.as_mut().ok_or("Not connected")?;
        conn.play().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn v2_dlna_play(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.play().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_pause(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.pause().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_stop(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.stop().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_seek(position_secs: u64, state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.seek(position_secs).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_set_volume(volume: f32, state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.set_volume(volume).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_airplay_start_discovery(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.start_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_airplay_stop_discovery(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.stop_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_airplay_get_devices(
    state: State<'_, AirPlayState>,
) -> Result<Vec<crate::cast::DiscoveredAirPlayDevice>, String> {
    let discovery = state.discovery.lock().await;
    Ok(discovery.get_discovered_devices())
}

#[tauri::command]
pub async fn v2_airplay_connect(
    device_id: String,
    state: State<'_, AirPlayState>,
) -> Result<(), String> {
    let device = {
        let discovery = state.discovery.lock().await;
        discovery
            .get_device(&device_id)
            .ok_or_else(|| format!("Device not found: {}", device_id))?
    };
    let connection = crate::cast::AirPlayConnection::connect(device).map_err(|e| e.to_string())?;
    let mut state_connection = state.connection.lock().await;
    *state_connection = Some(connection);
    Ok(())
}

#[tauri::command]
pub async fn v2_airplay_disconnect(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    if let Some(conn) = connection.as_mut() {
        conn.disconnect().map_err(|e| e.to_string())?;
    }
    *connection = None;
    Ok(())
}

#[tauri::command]
pub async fn v2_airplay_load_media(
    metadata: AirPlayMetadata,
    state: State<'_, AirPlayState>,
) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.load_media(metadata).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_airplay_play(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.play().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_airplay_pause(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.pause().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_airplay_stop(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.stop().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_airplay_set_volume(volume: f32, state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.set_volume(volume).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_clear_offline_cache(
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    let paths = {
        let guard = cache_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session - please log in")?;
        db.clear_all()?
    };
    for path in paths {
        let p = std::path::Path::new(&path);
        if p.exists() {
            let _ = std::fs::remove_file(p);
        }
    }

    let cache_dir = cache_state.cache_dir.read().unwrap().clone();
    let tracks_dir = cache_dir.join("tracks");
    if tracks_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&tracks_dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    if let Ok(entries) = std::fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "tracks" && !name.ends_with(".db") && !name.ends_with(".db-journal") {
                    let _ = std::fs::remove_dir_all(&path);
                }
            }
        }
    }

    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.remove_all_qobuz_cached_tracks()
        .map_err(|e| format!("Failed to remove cached tracks from library: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn v2_library_remove_folder(
    path: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.remove_folder(&path).map_err(|e| e.to_string())?;
    db.delete_tracks_in_folder(&path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn v2_library_check_folder_accessible(path: String) -> Result<bool, String> {
    log::info!("[V2] library_check_folder_accessible {}", path);

    let path_ref = std::path::Path::new(&path);
    if !path_ref.exists() {
        return Ok(false);
    }

    // Avoid UI stalls on slow/unresponsive mounts.
    let path_clone = path.clone();
    let check_result = tokio::time::timeout(
        std::time::Duration::from_secs(6),
        tokio::task::spawn_blocking(move || std::fs::read_dir(std::path::Path::new(&path_clone)).is_ok()),
    )
    .await;

    match check_result {
        Ok(Ok(accessible)) => Ok(accessible),
        Ok(Err(_)) => {
            log::warn!("[V2] Failed to spawn blocking task for folder check: {}", path);
            Ok(false)
        }
        Err(_) => {
            // Mounted-but-slow network shares can timeout but still be usable.
            let exists = std::path::Path::new(&path).exists();
            log::warn!(
                "[V2] Timeout checking folder accessibility: {} (exists={})",
                path,
                exists
            );
            Ok(exists)
        }
    }
}

#[tauri::command]
pub async fn v2_library_clear_artwork_cache() -> Result<u64, String> {
    let artwork_dir = get_artwork_cache_dir();
    if !artwork_dir.exists() {
        return Ok(0);
    }
    let mut cleared = 0u64;
    if let Ok(entries) = std::fs::read_dir(&artwork_dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    cleared += meta.len();
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    Ok(cleared)
}

#[tauri::command]
pub async fn v2_library_clear_thumbnails_cache() -> Result<u64, String> {
    let size_before = thumbnails::get_cache_size().unwrap_or(0);
    thumbnails::clear_thumbnails().map_err(|e| e.to_string())?;
    Ok(size_before)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_library_get_thumbnail(artworkPath: String) -> Result<String, String> {
    crate::library::library_get_thumbnail(artworkPath).await
}

#[tauri::command]
pub async fn v2_library_get_thumbnails_cache_size() -> Result<u64, String> {
    crate::library::library_get_thumbnails_cache_size().await
}

#[tauri::command]
pub async fn v2_library_get_scan_progress(
    library_state: State<'_, LibraryState>,
) -> Result<ScanProgress, String> {
    crate::library::library_get_scan_progress(library_state).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_library_get_tracks_by_ids(
    trackIds: Vec<i64>,
    library_state: State<'_, LibraryState>,
) -> Result<Vec<LocalTrack>, String> {
    crate::library::library_get_tracks_by_ids(trackIds, library_state).await
}

#[tauri::command]
pub async fn v2_library_play_track(
    track_id: i64,
    library_state: State<'_, LibraryState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await
        .map_err(|e| e.to_string())?;

    let track = {
        let guard = library_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session - please log in")?;
        db.get_track(track_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Track not found".to_string())?
    };
    let file_path = std::path::Path::new(&track.file_path);
    if !file_path.exists() {
        return Err(format!("File not found: {}", track.file_path));
    }
    let audio_data = std::fs::read(file_path).map_err(|e| format!("Failed to read file: {}", e))?;
    let bridge = bridge.get().await;
    bridge
        .player()
        .play_data(audio_data, track_id as u64)
        .map_err(|e| format!("Failed to play: {}", e))?;
    if let Some(start_secs) = track.cue_start_secs {
        let start_pos = start_secs as u64;
        if start_pos > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            bridge
                .player()
                .seek(start_pos)
                .map_err(|e| format!("Failed to seek: {}", e))?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn v2_playlist_set_sort(
    playlist_id: u64,
    sort_by: String,
    sort_order: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.update_playlist_sort(playlist_id, &sort_by, &sort_order)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_set_artwork(
    playlist_id: u64,
    artwork_path: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let final_path = if let Some(source_path) = artwork_path {
        let artwork_dir = get_artwork_cache_dir();
        let source = std::path::Path::new(&source_path);
        if !source.exists() {
            return Err(format!("Source image does not exist: {}", source_path));
        }
        let extension = source.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
        let filename = format!(
            "playlist_{}_{}.{}",
            playlist_id,
            chrono::Utc::now().timestamp(),
            extension
        );
        let dest_path = artwork_dir.join(filename);
        std::fs::copy(source, &dest_path).map_err(|e| format!("Failed to copy artwork: {}", e))?;
        Some(dest_path.to_string_lossy().to_string())
    } else {
        None
    };
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.update_playlist_artwork(playlist_id, final_path.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_add_local_track(
    playlist_id: u64,
    local_track_id: i64,
    position: i32,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.add_local_track_to_playlist(playlist_id, local_track_id, position)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_remove_local_track(
    playlist_id: u64,
    local_track_id: i64,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.remove_local_track_from_playlist(playlist_id, local_track_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_set_hidden(
    playlist_id: u64,
    hidden: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.set_playlist_hidden(playlist_id, hidden)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_set_favorite(
    playlist_id: u64,
    favorite: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.set_playlist_favorite(playlist_id, favorite)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_reorder(
    playlist_ids: Vec<u64>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.reorder_playlists(&playlist_ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_init_custom_order(
    playlist_id: u64,
    track_ids: Vec<(i64, bool)>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.init_playlist_custom_order(playlist_id, &track_ids)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_set_custom_order(
    playlist_id: u64,
    orders: Vec<(i64, bool, i32)>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.set_playlist_custom_order(playlist_id, &orders)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_move_track(
    playlist_id: u64,
    track_id: i64,
    is_local: bool,
    new_position: i32,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.move_playlist_track(playlist_id, track_id, is_local, new_position)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_library_set_album_artwork(
    album_group_key: String,
    artwork_path: String,
    state: State<'_, LibraryState>,
) -> Result<String, String> {
    if album_group_key.is_empty() {
        return Err("Album group key is required".to_string());
    }
    let source_path = std::path::Path::new(&artwork_path);
    if !source_path.is_file() {
        return Err("Artwork file not found".to_string());
    }
    let artwork_cache = get_artwork_cache_dir();
    let cached_path = MetadataExtractor::cache_artwork_file(source_path, &artwork_cache)
        .ok_or_else(|| "Failed to cache artwork file".to_string())?;
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.update_album_group_artwork(&album_group_key, &cached_path)
        .map_err(|e| e.to_string())?;
    Ok(cached_path)
}

#[tauri::command]
pub async fn v2_library_set_album_hidden(
    album_group_key: String,
    hidden: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.set_album_hidden(&album_group_key, hidden)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_delete_playlist_folder(
    id: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.delete_playlist_folder(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_reorder_playlist_folders(
    folder_ids: Vec<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.reorder_playlist_folders(&folder_ids)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_move_playlist_to_folder(
    playlist_id: u64,
    folder_id: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.move_playlist_to_folder(playlist_id, folder_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_lyrics_clear_cache(state: State<'_, LyricsState>) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.clear().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_musicbrainz_get_cache_stats(
    state: State<'_, MusicBrainzSharedState>,
) -> Result<MusicBrainzCacheStats, String> {
    let cache_opt = state.cache.lock().await;
    let cache = cache_opt.as_ref().ok_or("No active session - please log in")?;
    cache.get_stats()
}

#[tauri::command]
pub async fn v2_musicbrainz_clear_cache(
    state: State<'_, MusicBrainzSharedState>,
) -> Result<(), String> {
    let cache_opt = state.cache.lock().await;
    let cache = cache_opt.as_ref().ok_or("No active session - please log in")?;
    cache.clear_all()
}

#[tauri::command]
pub fn v2_set_show_partial_playlists(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_show_partial_playlists(enabled)
}

#[tauri::command]
pub fn v2_set_allow_cast_while_offline(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_allow_cast_while_offline(enabled)
}

#[tauri::command]
pub fn v2_set_allow_immediate_scrobbling(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_allow_immediate_scrobbling(enabled)
}

#[tauri::command]
pub fn v2_set_allow_accumulated_scrobbling(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_allow_accumulated_scrobbling(enabled)
}

#[tauri::command]
pub fn v2_set_show_network_folders_in_manual_offline(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_show_network_folders_in_manual_offline(enabled)
}

#[tauri::command]
pub async fn v2_get_offline_status(
    state: State<'_, OfflineState>,
) -> Result<crate::offline::OfflineStatus, String> {
    crate::offline::commands::get_offline_status(state).await
}

#[tauri::command]
pub fn v2_get_offline_settings(
    state: State<'_, OfflineState>,
) -> Result<crate::offline::OfflineSettings, String> {
    crate::offline::commands::get_offline_settings(state)
}

#[tauri::command]
pub async fn v2_set_manual_offline(
    enabled: bool,
    state: State<'_, OfflineState>,
    app_handle: tauri::AppHandle,
) -> Result<crate::offline::OfflineStatus, String> {
    crate::offline::commands::set_manual_offline(enabled, state, app_handle).await
}

#[tauri::command]
pub async fn v2_check_network() -> bool {
    crate::offline::commands::check_network().await
}

#[tauri::command]
pub fn v2_add_tracks_to_pending_playlist(
    pending_id: i64,
    qobuz_track_ids: Vec<u64>,
    local_track_paths: Vec<String>,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.add_tracks_to_pending_playlist(pending_id, &qobuz_track_ids, &local_track_paths)
}

#[tauri::command]
pub fn v2_update_pending_playlist_qobuz_id(
    pending_id: i64,
    qobuz_playlist_id: u64,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.update_qobuz_playlist_id(pending_id, qobuz_playlist_id)
}

#[tauri::command]
pub fn v2_mark_pending_playlist_synced(
    pending_id: i64,
    qobuz_playlist_id: u64,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.mark_playlist_synced(pending_id, qobuz_playlist_id)
}

#[tauri::command]
pub fn v2_delete_pending_playlist(
    pending_id: i64,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.delete_pending_playlist(pending_id)
}

#[tauri::command]
pub fn v2_mark_scrobbles_sent(
    ids: Vec<i64>,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.mark_scrobbles_sent(&ids)
}

#[tauri::command]
pub fn v2_get_pending_playlists(
    state: State<'_, OfflineState>,
) -> Result<Vec<crate::offline::PendingPlaylist>, String> {
    crate::offline::commands::get_pending_playlists(state)
}

#[tauri::command]
pub async fn v2_remove_cached_track(
    track_id: u64,
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    {
        let guard = cache_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session - please log in")?;
        if let Some(file_path) = db.delete_track(track_id)? {
            let path = std::path::Path::new(&file_path);
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let _ = db.remove_qobuz_cached_track(track_id);
    Ok(())
}

#[tauri::command]
pub async fn v2_get_cached_tracks(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<Vec<crate::offline_cache::CachedTrackInfo>, String> {
    let guard = cache_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_all_tracks()
}

#[tauri::command]
pub async fn v2_get_offline_cache_stats(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<crate::offline_cache::OfflineCacheStats, String> {
    let limit = *cache_state.limit_bytes.lock().await;
    let guard = cache_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_stats(&cache_state.get_cache_path(), limit)
}

#[tauri::command]
pub async fn v2_set_offline_cache_limit(
    limit_mb: Option<u64>,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    let limit_bytes = limit_mb.map(|mb| mb * 1024 * 1024);
    let mut limit = cache_state.limit_bytes.lock().await;
    *limit = limit_bytes;
    Ok(())
}

#[tauri::command]
pub async fn v2_open_offline_cache_folder(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    let path = cache_state.cache_dir.read().unwrap().clone();
    std::fs::create_dir_all(&path).map_err(|e| format!("Failed to create cache directory: {}", e))?;
    open::that(&path).map_err(|e| format!("Failed to open folder: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn v2_open_album_folder(
    album_id: String,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    let guard = cache_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let tracks = db.get_all_tracks()?;
    let album_tracks: Vec<_> = tracks
        .into_iter()
        .filter(|t| t.album_id.as_deref() == Some(&album_id))
        .collect();
    if album_tracks.is_empty() {
        return Err("No cached tracks found for this album".to_string());
    }
    let file_path = db
        .get_file_path(album_tracks[0].track_id)?
        .ok_or_else(|| "Track file path not found".to_string())?;
    let album_dir = std::path::Path::new(&file_path)
        .parent()
        .ok_or_else(|| "Could not determine album folder".to_string())?;
    open::that(album_dir).map_err(|e| format!("Failed to open folder: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn v2_open_track_folder(
    track_id: u64,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    let guard = cache_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let file_path = db
        .get_file_path(track_id)?
        .ok_or_else(|| "Track file path not found - track may not be cached".to_string())?;
    let track_dir = std::path::Path::new(&file_path)
        .parent()
        .ok_or_else(|| "Could not determine track folder".to_string())?;
    open::that(track_dir).map_err(|e| format!("Failed to open folder: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn v2_lastfm_open_auth_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| format!("Failed to open browser: {}", e))
}

#[tauri::command]
pub async fn v2_lastfm_set_credentials(
    api_key: String,
    api_secret: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut client = state.lastfm.lock().await;
    client.set_credentials(api_key, api_secret);
    Ok(())
}

#[tauri::command]
pub async fn v2_reco_log_event(
    event: RecoEventInput,
    state: State<'_, RecoState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.insert_event(&event)
}

#[tauri::command]
pub async fn v2_reco_train_scores(
    lookback_days: Option<i64>,
    half_life_days: Option<f64>,
    max_events: Option<u32>,
    max_per_type: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<(), String> {
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    let lookback_days = lookback_days.unwrap_or(90);
    let half_life_days = half_life_days.unwrap_or(21.0);
    let max_events = max_events.unwrap_or(5000);
    let max_per_type = max_per_type.unwrap_or(200);

    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let since_ts = now_ts.saturating_sub(lookback_days * 86_400);

    let mut guard = state.db.lock().await;
    let db = guard.as_mut().ok_or("No active session - please log in")?;
    let events = db.get_events_since(since_ts, Some(max_events))?;

    let decay_factor = |age_secs: i64| -> f64 {
        if half_life_days <= 0.0 {
            return 1.0;
        }
        let half_life_secs = half_life_days * 86_400.0;
        let exponent = age_secs as f64 / half_life_secs;
        0.5_f64.powf(exponent)
    };

    let event_weight = |event_type: &str| -> f64 {
        match event_type {
            "play" => 1.0,
            "favorite" => 3.0,
            "playlist_add" => 1.2,
            _ => 1.0,
        }
    };

    let item_weight = |item_type: &str, primary: bool| -> f64 {
        if primary {
            return 1.0;
        }
        match item_type {
            "album" => 0.7,
            "artist" => 0.5,
            "track" => 0.85,
            _ => 0.6,
        }
    };

    let build_scores = |favorites_only: bool| {
        let mut tracks: HashMap<u64, f64> = HashMap::new();
        let mut albums: HashMap<String, f64> = HashMap::new();
        let mut artists: HashMap<u64, f64> = HashMap::new();

        for event in &events {
            if favorites_only && event.event_type != "favorite" {
                continue;
            }

            let age_secs = (now_ts - event.created_at).max(0);
            let base_weight = event_weight(&event.event_type) * decay_factor(age_secs);

            if let Some(track_id) = event.track_id {
                let weight = base_weight * item_weight("track", event.item_type == "track");
                *tracks.entry(track_id).or_insert(0.0) += weight;
            }
            if let Some(album_id) = event.album_id.as_ref() {
                let weight = base_weight * item_weight("album", event.item_type == "album");
                *albums.entry(album_id.clone()).or_insert(0.0) += weight;
            }
            if let Some(artist_id) = event.artist_id {
                let weight = base_weight * item_weight("artist", event.item_type == "artist");
                *artists.entry(artist_id).or_insert(0.0) += weight;
            }
        }

        (tracks, albums, artists)
    };

    let build_track_entries = |scores: HashMap<u64, f64>| {
        let mut entries: Vec<(u64, f64)> = scores.into_iter().collect();
        entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        entries
            .into_iter()
            .take(max_per_type as usize)
            .map(|(track_id, score)| crate::reco_store::db::RecoScoreEntry {
                track_id: Some(track_id),
                album_id: None,
                artist_id: None,
                score,
            })
            .collect::<Vec<_>>()
    };
    let build_album_entries = |scores: HashMap<String, f64>| {
        let mut entries: Vec<(String, f64)> = scores.into_iter().collect();
        entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        entries
            .into_iter()
            .take(max_per_type as usize)
            .map(|(album_id, score)| crate::reco_store::db::RecoScoreEntry {
                track_id: None,
                album_id: Some(album_id),
                artist_id: None,
                score,
            })
            .collect::<Vec<_>>()
    };
    let build_artist_entries = |scores: HashMap<u64, f64>| {
        let mut entries: Vec<(u64, f64)> = scores.into_iter().collect();
        entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        entries
            .into_iter()
            .take(max_per_type as usize)
            .map(|(artist_id, score)| crate::reco_store::db::RecoScoreEntry {
                track_id: None,
                album_id: None,
                artist_id: Some(artist_id),
                score,
            })
            .collect::<Vec<_>>()
    };

    let (all_tracks, all_albums, all_artists) = build_scores(false);
    let (fav_tracks, fav_albums, fav_artists) = build_scores(true);

    db.replace_scores("all", "track", &build_track_entries(all_tracks))?;
    db.replace_scores("all", "album", &build_album_entries(all_albums))?;
    db.replace_scores("all", "artist", &build_artist_entries(all_artists))?;
    db.replace_scores("favorite", "track", &build_track_entries(fav_tracks))?;
    db.replace_scores("favorite", "album", &build_album_entries(fav_albums))?;
    db.replace_scores("favorite", "artist", &build_artist_entries(fav_artists))?;

    Ok(())
}

#[tauri::command]
pub async fn v2_reco_get_home(
    limitRecentAlbums: Option<u32>,
    limitContinueTracks: Option<u32>,
    limitTopArtists: Option<u32>,
    limitFavorites: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<HomeSeeds, String> {
    crate::reco_store::commands::reco_get_home(
        limitRecentAlbums,
        limitContinueTracks,
        limitTopArtists,
        limitFavorites,
        state,
    )
    .await
}

#[tauri::command]
pub async fn v2_reco_get_home_ml(
    limitRecentAlbums: Option<u32>,
    limitContinueTracks: Option<u32>,
    limitTopArtists: Option<u32>,
    limitFavorites: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<HomeSeeds, String> {
    crate::reco_store::commands::reco_get_home_ml(
        limitRecentAlbums,
        limitContinueTracks,
        limitTopArtists,
        limitFavorites,
        state,
    )
    .await
}

#[tauri::command]
pub async fn v2_reco_get_home_resolved(
    limitRecentAlbums: Option<u32>,
    limitContinueTracks: Option<u32>,
    limitTopArtists: Option<u32>,
    limitFavorites: Option<u32>,
    reco_state: State<'_, RecoState>,
    app_state: State<'_, AppState>,
    cache_state: State<'_, ApiCacheState>,
) -> Result<HomeResolved, String> {
    crate::reco_store::commands::reco_get_home_resolved(
        limitRecentAlbums,
        limitContinueTracks,
        limitTopArtists,
        limitFavorites,
        reco_state,
        app_state,
        cache_state,
    )
    .await
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct V2LibraryCacheStats {
    pub artwork_cache_bytes: u64,
    pub thumbnails_cache_bytes: u64,
    pub artwork_file_count: usize,
    pub thumbnail_file_count: usize,
}

#[tauri::command]
pub async fn v2_library_get_cache_stats() -> Result<V2LibraryCacheStats, String> {
    let artwork_dir = get_artwork_cache_dir();
    let (artwork_bytes, artwork_count) = if artwork_dir.exists() {
        let mut size = 0u64;
        let mut count = 0usize;
        if let Ok(entries) = std::fs::read_dir(&artwork_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        size += meta.len();
                        count += 1;
                    }
                }
            }
        }
        (size, count)
    } else {
        (0, 0)
    };
    let thumbnails_bytes = thumbnails::get_cache_size().unwrap_or(0);
    let thumbnail_count = if let Ok(dir) = thumbnails::get_thumbnails_dir() {
        std::fs::read_dir(&dir).map(|e| e.count()).unwrap_or(0)
    } else {
        0
    };
    Ok(V2LibraryCacheStats {
        artwork_cache_bytes: artwork_bytes,
        thumbnails_cache_bytes: thumbnails_bytes,
        artwork_file_count: artwork_count,
        thumbnail_file_count: thumbnail_count,
    })
}

#[tauri::command]
pub async fn v2_playlist_get_all_settings(
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistSettings>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.get_all_playlist_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_get_favorites(
    state: State<'_, LibraryState>,
) -> Result<Vec<u64>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.get_favorite_playlist_ids().map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_local_tracks_with_position(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistLocalTrack>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.get_playlist_local_tracks_with_position(playlistId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_settings(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<Option<PlaylistSettings>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.get_playlist_settings(playlistId).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_stats(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<Option<PlaylistStats>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.get_playlist_stats(playlistId).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_increment_play_count(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<PlaylistStats, String> {
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.increment_playlist_play_count(playlistId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_get_all_stats(
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistStats>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.get_all_playlist_stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_get_all_local_track_counts(
    state: State<'_, LibraryState>,
) -> Result<std::collections::HashMap<u64, u32>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.get_all_playlist_local_track_counts()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_get_playlist_folders(
    state: State<'_, LibraryState>,
) -> Result<Vec<crate::library::PlaylistFolder>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.get_all_playlist_folders().map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_create_playlist_folder(
    name: String,
    iconType: Option<String>,
    iconPreset: Option<String>,
    iconColor: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<crate::library::PlaylistFolder, String> {
    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;
    db.create_playlist_folder(
        &name,
        iconType.as_deref(),
        iconPreset.as_deref(),
        iconColor.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_update_playlist_folder(
    id: String,
    name: Option<String>,
    iconType: Option<String>,
    iconPreset: Option<String>,
    iconColor: Option<String>,
    customImagePath: Option<String>,
    isHidden: Option<bool>,
    state: State<'_, LibraryState>,
) -> Result<crate::library::PlaylistFolder, String> {
    crate::library::update_playlist_folder(
        id,
        name,
        iconType,
        iconPreset,
        iconColor,
        customImagePath,
        isHidden,
        state,
    )
    .await
}

#[tauri::command]
pub async fn v2_library_get_albums(
    include_hidden: Option<bool>,
    exclude_network_folders: Option<bool>,
    state: State<'_, LibraryState>,
    download_settings_state: State<'_, DownloadSettingsState>,
) -> Result<Vec<LocalAlbum>, String> {
    let include_qobuz = download_settings_state
        .lock()
        .map_err(|e| format!("Failed to lock download settings: {}", e))?
        .as_ref()
        .and_then(|s| s.get_settings().ok())
        .map(|s| s.show_in_library)
        .unwrap_or(false);

    let guard__ = state.db.lock().await;
    let db = guard__.as_ref().ok_or("No active session - please log in")?;

    db.get_albums_with_full_filter(
        include_hidden.unwrap_or(false),
        include_qobuz,
        exclude_network_folders.unwrap_or(false),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_library_get_stats(
    state: State<'_, LibraryState>,
    download_settings_state: State<'_, DownloadSettingsState>,
) -> Result<crate::library::LibraryStats, String> {
    crate::library::library_get_stats(state, download_settings_state).await
}

#[tauri::command]
pub async fn v2_library_get_folders(
    state: State<'_, LibraryState>,
) -> Result<Vec<String>, String> {
    crate::library::library_get_folders(state).await
}

#[tauri::command]
pub async fn v2_library_get_folders_with_metadata(
    state: State<'_, LibraryState>,
) -> Result<Vec<crate::library::LibraryFolder>, String> {
    crate::library::library_get_folders_with_metadata(state).await
}

#[tauri::command]
pub async fn v2_library_add_folder(
    path: String,
    state: State<'_, LibraryState>,
) -> Result<crate::library::LibraryFolder, String> {
    crate::library::library_add_folder(path, state).await
}

#[tauri::command]
pub async fn v2_library_cleanup_missing_files(
    state: State<'_, LibraryState>,
) -> Result<crate::library::CleanupResult, String> {
    crate::library::library_cleanup_missing_files(state).await
}

#[tauri::command]
pub async fn v2_library_fetch_missing_artwork(
    state: State<'_, LibraryState>,
) -> Result<u32, String> {
    crate::library::library_fetch_missing_artwork(state).await
}

#[tauri::command]
pub async fn v2_library_get_artists(
    exclude_network_folders: Option<bool>,
    state: State<'_, LibraryState>,
    download_settings_state: State<'_, DownloadSettingsState>,
) -> Result<Vec<crate::library::LocalArtist>, String> {
    crate::library::library_get_artists(
        exclude_network_folders,
        state,
        download_settings_state,
    )
    .await
}

#[tauri::command]
pub async fn v2_library_search(
    query: String,
    limit: Option<u32>,
    exclude_network_folders: Option<bool>,
    state: State<'_, LibraryState>,
    download_settings_state: State<'_, DownloadSettingsState>,
) -> Result<Vec<crate::library::LocalTrack>, String> {
    crate::library::library_search(
        query,
        limit,
        exclude_network_folders,
        state,
        download_settings_state,
    )
    .await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_library_get_album_tracks(
    albumGroupKey: String,
    state: State<'_, LibraryState>,
) -> Result<Vec<crate::library::LocalTrack>, String> {
    crate::library::library_get_album_tracks(albumGroupKey, state).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_get_music_sections(
    baseUrl: String,
    token: String,
) -> Result<Vec<PlexMusicSection>, String> {
    crate::plex::plex_get_music_sections(baseUrl, token).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_get_section_tracks(
    baseUrl: String,
    token: String,
    sectionKey: String,
    limit: Option<u32>,
) -> Result<Vec<PlexTrack>, String> {
    crate::plex::plex_get_section_tracks(baseUrl, token, sectionKey, limit).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_play_track(
    baseUrl: String,
    token: String,
    ratingKey: String,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<PlexPlayResult, String> {
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await
        .map_err(|e| e.to_string())?;

    let resolved = crate::plex::plex_resolve_track_media(baseUrl, token, ratingKey).await?;
    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();

    player
        .play_data(resolved.bytes.clone(), resolved.playback_id)
        .map_err(|e| format!("Failed to play Plex track via V2 player: {}", e))?;

    Ok(PlexPlayResult {
        rating_key: resolved.rating_key,
        part_key: resolved.part_key,
        part_url: resolved.part_url,
        bytes: resolved.bytes.len(),
        direct_play_confirmed: resolved.direct_play_confirmed,
        content_type: resolved.content_type,
        sampling_rate_hz: resolved.sampling_rate_hz,
        bit_depth: resolved.bit_depth,
    })
}

#[tauri::command]
pub async fn v2_library_update_folder_path(
    id: i64,
    new_path: String,
    state: State<'_, LibraryState>,
) -> Result<crate::library::LibraryFolder, String> {
    let path_ref = std::path::Path::new(&new_path);
    if !path_ref.exists() {
        return Err("The selected folder does not exist".to_string());
    }
    if !path_ref.is_dir() {
        return Err("The selected path is not a folder".to_string());
    }

    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.update_folder_path(id, &new_path).map_err(|e| e.to_string())?;

    let network_info = crate::network::is_network_path(path_ref);
    if network_info.is_network {
        let fs_type = network_info.mount_info.as_ref().and_then(|mi| {
            if let crate::network::MountKind::Network(nfs) = &mi.kind {
                Some(format!("{:?}", nfs).to_lowercase())
            } else {
                None
            }
        });
        if let Some(folder) = db.get_folder_by_id(id).map_err(|e| e.to_string())? {
            db.update_folder_settings(
                id,
                folder.alias.as_deref(),
                folder.enabled,
                true,
                fs_type.as_deref(),
                false,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    db.get_folder_by_id(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Folder not found after update".to_string())
}

#[tauri::command]
pub async fn v2_library_cache_artist_image(
    artist_name: String,
    image_url: String,
    source: String,
    canonical_name: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.cache_artist_image_with_canonical(
        &artist_name,
        Some(&image_url),
        &source,
        None,
        canonical_name.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_library_set_custom_artist_image(
    artist_name: String,
    custom_image_path: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let artwork_dir = get_artwork_cache_dir();
    let source = std::path::Path::new(&custom_image_path);
    if !source.exists() {
        return Err(format!("Source image does not exist: {}", custom_image_path));
    }
    let extension = source.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(artist_name.as_bytes());
    let artist_hash = format!("{:x}", hasher.finalize());
    let filename = format!(
        "artist_custom_{}_{}.{}",
        artist_hash,
        chrono::Utc::now().timestamp(),
        extension
    );
    let dest_path = artwork_dir.join(filename);
    std::fs::copy(source, &dest_path).map_err(|e| format!("Failed to copy artwork: {}", e))?;

    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.cache_artist_image(&artist_name, None, "custom", Some(&dest_path.to_string_lossy()))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_create_track_radio(
    track_id: u64,
    track_name: String,
    artist_id: u64,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<String, String> {
    let client = state.client.read().await.clone();

    let session_id = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
        let builder = crate::radio_engine::RadioPoolBuilder::new(
            &radio_db,
            &client,
            crate::radio_engine::BuildRadioOptions::default(),
        );
        let rt = tokio::runtime::Handle::current();
        let session = rt.block_on(builder.create_track_radio(track_id, artist_id))?;
        Ok(session.id)
    })
    .await
    .map_err(|e| format!("Radio task failed: {}", e))??;

    let client = state.client.read().await;
    let track_ids = tokio::task::spawn_blocking({
        let session_id = session_id.clone();
        let seed_track_id = track_id;
        move || -> Result<Vec<u64>, String> {
            let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
            let radio_engine = crate::radio_engine::RadioEngine::new(radio_db);
            let mut tracks_with_source = Vec::new();
            for _ in 0..60 {
                match radio_engine.next_track(&session_id) {
                    Ok(radio_track) => {
                        tracks_with_source.push((radio_track.track_id, radio_track.source.clone()));
                    }
                    Err(_) => break,
                }
            }
            if let Some(seed_idx) = tracks_with_source
                .iter()
                .position(|(id, source)| *id == seed_track_id && source == "seed_track")
            {
                if seed_idx != 0 {
                    tracks_with_source.swap(0, seed_idx);
                }
            }
            Ok(tracks_with_source
                .into_iter()
                .take(50)
                .map(|(id, _)| id)
                .collect())
        }
    })
    .await
    .map_err(|e| format!("Track generation task failed: {}", e))??;

    let mut tracks = Vec::new();
    for next_track_id in track_ids {
        if let Ok(track) = client.get_track(next_track_id).await {
            if let Some(ref performer) = track.performer {
                if blacklist_state.is_blacklisted(performer.id) {
                    continue;
                }
            }
            tracks.push(track);
        }
    }

    if tracks.is_empty() {
        return Err("Failed to generate any radio tracks".to_string());
    }

    let queue_tracks: Vec<QueueTrack> = tracks.iter().map(track_to_queue_track_from_api).collect();
    state.queue.set_queue(queue_tracks, Some(0));

    let queue_track_ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
    let context = PlaybackContext::new(
        ContextType::Radio,
        session_id.clone(),
        track_name,
        ContentSource::Qobuz,
        queue_track_ids,
        0,
    );
    state.context.set_context(context);

    Ok(session_id)
}

fn track_to_queue_track_from_api(track: &crate::api::Track) -> QueueTrack {
    let artwork_url = track.album.as_ref().and_then(|a| a.image.large.clone());
    let artist = track
        .performer
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album = track
        .album
        .as_ref()
        .map(|a| a.title.clone())
        .unwrap_or_else(|| "Unknown Album".to_string());
    let album_id = track.album.as_ref().map(|a| a.id.clone());
    let artist_id = track.performer.as_ref().map(|p| p.id);

    QueueTrack {
        id: track.id,
        title: track.title.clone(),
        artist,
        album,
        duration_secs: track.duration as u64,
        artwork_url,
        hires: track.hires,
        bit_depth: track.maximum_bit_depth,
        sample_rate: track.maximum_sampling_rate,
        is_local: false,
        album_id,
        artist_id,
        streamable: track.streamable,
        source: Some("qobuz".to_string()),
    }
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

/// Search all categories in one call (albums/tracks/artists/playlists + most_popular)
#[tauri::command]
pub async fn v2_search_all(
    query: String,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<V2SearchAllResults, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;

    let url = crate::api::endpoints::build_url(crate::api::endpoints::paths::CATALOG_SEARCH);
    let response: serde_json::Value = {
        let client = state.client.read().await;
        client
            .get_http()
            .get(&url)
            .header(
                "X-App-Id",
                client
                    .app_id()
                    .await
                    .map_err(|e| RuntimeError::Internal(e.to_string()))?,
            )
            .header(
                "X-User-Auth-Token",
                client
                    .auth_token()
                    .await
                    .map_err(|e| RuntimeError::Internal(e.to_string()))?,
            )
            .query(&[("query", query.as_str()), ("limit", "30"), ("offset", "0")])
            .send()
            .await
            .map_err(|e| RuntimeError::Internal(format!("Request failed: {}", e)))?
            .json()
            .await
            .map_err(|e| RuntimeError::Internal(format!("JSON parse failed: {}", e)))?
    };

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
    limit: Option<u32>,
    offset: Option<u32>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<serde_json::Value, RuntimeError> {
    // Runtime contract: require CoreBridge auth for V2 favorites
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await?;

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

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_custom_order(
    playlistId: u64,
    library_state: State<'_, LibraryState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<(i64, bool, i32)>, RuntimeError> {
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
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
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
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
    runtime.manager().check_requirements(CommandRequirement::RequiresUserSession).await?;
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

#[tauri::command]
pub async fn v2_resolve_musician(
    name: String,
    role: String,
    mb_state: State<'_, MusicBrainzV2State>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<crate::musicbrainz::ResolvedMusician, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let resolved_artist = {
        let client = mb_state.client.lock().await;
        client
            .resolve_artist(&name)
            .await
            .map_err(|e| RuntimeError::Internal(e.to_string()))?
    };

    let normalized_target = name.trim().to_lowercase();
    let bridge = bridge.get().await;
    let artist_results = bridge
        .search_artists(&name, 10, 0, None)
        .await
        .map_err(RuntimeError::Internal)?;
    let exact = artist_results
        .items
        .iter()
        .find(|artist| artist.name.trim().to_lowercase() == normalized_target);

    if let Some(artist) = exact {
        let qobuz_artist_id = i64::try_from(artist.id).ok();
        return Ok(crate::musicbrainz::ResolvedMusician {
            name,
            role,
            mbid: None,
            qobuz_artist_id,
            confidence: crate::musicbrainz::MusicianConfidence::Confirmed,
            bands: Vec::new(),
            appears_on_count: 0,
        });
    }

    let album_results = bridge
        .search_albums(&name, 20, 0, None)
        .await
        .map_err(RuntimeError::Internal)?;
    let appears_on_count = album_results.total as usize;

    let confidence = if appears_on_count > 0 {
        crate::musicbrainz::MusicianConfidence::Contextual
    } else if resolved_artist.is_some() {
        crate::musicbrainz::MusicianConfidence::Weak
    } else {
        crate::musicbrainz::MusicianConfidence::None
    };

    Ok(crate::musicbrainz::ResolvedMusician {
        name,
        role,
        mbid: resolved_artist.as_ref().map(|a| a.mbid.clone()),
        qobuz_artist_id: None,
        confidence,
        bands: Vec::new(),
        appears_on_count,
    })
}

#[tauri::command]
pub async fn v2_get_musician_appearances(
    name: String,
    role: String,
    limit: Option<u32>,
    offset: Option<u32>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<crate::musicbrainz::MusicianAppearances, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let bridge = bridge.get().await;
    let results = bridge
        .search_albums(&name, limit.unwrap_or(20), offset.unwrap_or(0), None)
        .await
        .map_err(RuntimeError::Internal)?;

    let albums = results
        .items
        .into_iter()
        .map(|album| crate::musicbrainz::AlbumAppearance {
            album_id: album.id,
            album_title: album.title,
            album_artwork: album.image.large.or(album.image.small).unwrap_or_default(),
            artist_name: album.artist.name,
            year: album.release_date_original,
            role_on_album: role.clone(),
        })
        .collect::<Vec<_>>();

    Ok(crate::musicbrainz::MusicianAppearances {
        albums,
        total: results.total as usize,
    })
}

#[tauri::command]
pub async fn v2_musicbrainz_get_artist_relationships(
    mbid: String,
    state: State<'_, MusicBrainzSharedState>,
) -> Result<crate::musicbrainz::ArtistRelationships, String> {
    // Check cache first
    {
        let cache_opt = state.cache.lock().await;
        let cache = cache_opt.as_ref().ok_or("No active session - please log in")?;
        if let Some(cached) = cache.get_artist_relations(&mbid)? {
            return Ok(cached);
        }
    }

    let artist = state.client.get_artist_with_relations(&mbid).await?;

    let mut members = Vec::new();
    let mut past_members = Vec::new();
    let mut groups = Vec::new();
    let mut collaborators = Vec::new();

    if let Some(relations) = &artist.relations {
        for relation in relations {
            let Some(related_artist) = &relation.artist else {
                continue;
            };

            let related = crate::musicbrainz::RelatedArtist {
                mbid: related_artist.id.clone(),
                name: related_artist.name.clone(),
                role: relation.attributes.as_ref().and_then(|a| a.first().cloned()),
                period: Some(crate::musicbrainz::Period {
                    begin: relation.begin.clone(),
                    end: relation.end.clone(),
                }),
                ended: relation.ended.unwrap_or(false),
            };

            match relation.relation_type.as_str() {
                "member of band" => {
                    if relation.direction.as_deref() == Some("backward") {
                        if related.ended {
                            past_members.push(related);
                        } else {
                            members.push(related);
                        }
                    } else {
                        groups.push(related);
                    }
                }
                "collaboration" => {
                    collaborators.push(related);
                }
                _ => {}
            }
        }
    }

    let result = crate::musicbrainz::ArtistRelationships {
        members,
        past_members,
        groups,
        collaborators,
    };

    // Cache result
    {
        let cache_opt = state.cache.lock().await;
        let cache = cache_opt.as_ref().ok_or("No active session - please log in")?;
        cache.set_artist_relations(&mbid, &result)?;
    }

    Ok(result)
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

// ==================== Remaining Legacy-Equivalent V2 Commands ====================

#[tauri::command]
pub fn v2_show_track_notification(
    title: String,
    artist: String,
    album: String,
    artwork_url: Option<String>,
    bit_depth: Option<u32>,
    sample_rate: Option<f64>,
) -> Result<(), String> {
    log::info!("Command: v2_show_track_notification - {} by {}", title, artist);

    let mut lines = Vec::new();
    let mut line1_parts = Vec::new();
    if !artist.is_empty() {
        line1_parts.push(artist.clone());
    }
    if !album.is_empty() {
        line1_parts.push(album.clone());
    }
    if !line1_parts.is_empty() {
        lines.push(line1_parts.join(" • "));
    }

    let quality = v2_format_notification_quality(bit_depth, sample_rate);
    if !quality.is_empty() {
        lines.push(quality);
    }

    let mut notification = Notification::new();
    notification.summary(&title).body(&lines.join("\n")).appname("QBZ").timeout(4000);

    if let Some(url) = artwork_url {
        if let Ok(path) = v2_cache_notification_artwork(&url) {
            if let Some(path_str) = path.to_str() {
                notification.image_path(path_str);
            }
        }
    }

    if let Err(e) = notification.show() {
        log::warn!("Could not show notification (notification system may be unavailable): {}", e);
    }
    Ok(())
}

#[tauri::command]
pub async fn v2_subscribe_playlist(
    playlist_id: u64,
    state: State<'_, AppState>,
    library_state: State<'_, crate::library::LibraryState>,
) -> Result<crate::api::models::Playlist, String> {
    log::info!("Command: v2_subscribe_playlist {}", playlist_id);
    let client = state.client.read().await;

    let source = client
        .get_playlist(playlist_id)
        .await
        .map_err(|e| format!("Failed to get source playlist: {}", e))?;

    let track_ids: Vec<u64> = source
        .tracks
        .as_ref()
        .map(|t| t.items.iter().map(|track| track.id).collect())
        .unwrap_or_default();
    if track_ids.is_empty() {
        return Err("Source playlist has no tracks to copy".to_string());
    }

    let attribution = format!("\n\n---\nOriginally curated by {} on Qobuz", source.owner.name);
    let new_description = match source.description {
        Some(ref desc) if !desc.is_empty() => Some(format!("{}{}", desc, attribution)),
        _ => Some(attribution.trim_start().to_string()),
    };

    let new_playlist = client
        .create_playlist(&source.name, new_description.as_deref(), false)
        .await
        .map_err(|e| format!("Failed to create new playlist: {}", e))?;

    client
        .add_tracks_to_playlist(new_playlist.id, &track_ids)
        .await
        .map_err(|e| format!("Failed to add tracks to new playlist: {}", e))?;

    if let Some(ref images) = source.images {
        if let Some(image_url) = images.first() {
            let db_guard = library_state.db.lock().await;
            if let Some(ref db) = *db_guard {
                if let Err(e) = db.update_playlist_artwork(new_playlist.id, Some(image_url)) {
                    log::warn!("Failed to save original playlist artwork: {}", e);
                }
            }
        }
    }

    client
        .get_playlist(new_playlist.id)
        .await
        .map_err(|e| format!("Failed to fetch created playlist: {}", e))
}

#[tauri::command]
pub async fn v2_cache_track_for_offline(
    track_id: u64,
    title: String,
    artist: String,
    album: Option<String>,
    album_id: Option<String>,
    duration_secs: u64,
    quality: String,
    bit_depth: Option<u32>,
    sample_rate: Option<f64>,
    state: State<'_, AppState>,
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, crate::library::LibraryState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    log::info!("Command: v2_cache_track_for_offline {} - {} by {}", track_id, title, artist);

    let track_info = crate::offline_cache::TrackCacheInfo {
        track_id,
        title,
        artist,
        album,
        album_id,
        duration_secs,
        quality,
        bit_depth,
        sample_rate,
    };

    let file_path = cache_state.track_file_path(track_id, "flac");
    let file_path_str = file_path.to_string_lossy().to_string();
    {
        let guard = cache_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session - please log in")?;
        db.insert_track(&track_info, &file_path_str)?;
    }

    let client = state.client.clone();
    let fetcher = cache_state.fetcher.clone();
    let db = cache_state.db.clone();
    let offline_root = cache_state.get_cache_path();
    let library_db = library_state.db.clone();
    let app = app_handle.clone();
    let semaphore = cache_state.cache_semaphore.clone();

    tokio::spawn(async move {
        let _permit = match semaphore.acquire_owned().await {
            Ok(permit) => permit,
            Err(err) => {
                log::error!("Failed to acquire cache slot for track {}: {}", track_id, err);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(
                        track_id,
                        crate::offline_cache::OfflineCacheStatus::Failed,
                        Some("Failed to start caching"),
                    );
                }
                let _ = app.emit("offline:caching_failed", serde_json::json!({
                    "trackId": track_id,
                    "error": "Failed to acquire cache slot"
                }));
                return;
            }
        };

        if let Some(db_guard) = db.lock().await.as_ref() {
            let _ = db_guard.update_status(track_id, crate::offline_cache::OfflineCacheStatus::Downloading, None);
        }
        let _ = app.emit("offline:caching_started", serde_json::json!({ "trackId": track_id }));

        let stream_url = {
            let client_guard = client.read().await;
            client_guard
                .get_stream_url_with_fallback(track_id, crate::api::models::Quality::UltraHiRes)
                .await
        };

        let url = match stream_url {
            Ok(s) => s.url,
            Err(e) => {
                log::error!("Failed to get stream URL for track {}: {}", track_id, e);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(
                        track_id,
                        crate::offline_cache::OfflineCacheStatus::Failed,
                        Some(&format!("Failed to get stream URL: {}", e)),
                    );
                }
                let _ = app.emit("offline:caching_failed", serde_json::json!({
                    "trackId": track_id,
                    "error": e.to_string()
                }));
                return;
            }
        };

        match fetcher.fetch_to_file(&url, &file_path, track_id, Some(&app)).await {
            Ok(size) => {
                log::info!("Caching complete for track {}: {} bytes", track_id, size);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.mark_complete(track_id, size);
                }
                let _ = app.emit("offline:caching_completed", serde_json::json!({
                    "trackId": track_id,
                    "size": size
                }));

                // Post-processing kept in V2 to avoid command->command delegation.
                let file_path_str = file_path.to_string_lossy().to_string();
                let qobuz_client = client.read().await;
                let metadata = match crate::offline_cache::metadata::fetch_complete_metadata(track_id, &*qobuz_client).await {
                    Ok(m) => m,
                    Err(e) => {
                        log::warn!("Post-processing metadata fetch failed for {}: {}", track_id, e);
                        return;
                    }
                };

                if let Err(e) = crate::offline_cache::metadata::write_flac_tags(&file_path_str, &metadata) {
                    log::warn!("Failed to write tags for {}: {}", track_id, e);
                }
                if let Some(artwork_url) = &metadata.artwork_url {
                    if let Err(e) = crate::offline_cache::metadata::embed_artwork(&file_path_str, artwork_url).await {
                        log::warn!("Failed to embed artwork for {}: {}", track_id, e);
                    }
                }

                let new_path = match crate::offline_cache::metadata::organize_cached_file(
                    track_id,
                    &file_path_str,
                    &offline_root,
                    &metadata,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        log::warn!("Failed to organize cached file {}: {}", track_id, e);
                        return;
                    }
                };

                if let Some(artwork_url) = &metadata.artwork_url {
                    if let Some(parent_dir) = std::path::Path::new(&new_path).parent() {
                        let _ = crate::offline_cache::metadata::save_album_artwork(parent_dir, artwork_url).await;
                    }
                }

                let (bit_depth_detected, sample_rate_detected) = match lofty::read_from_path(&new_path) {
                    Ok(tagged_file) => {
                        use lofty::AudioFile;
                        let properties = tagged_file.properties();
                        (
                            properties.bit_depth().map(|bd| bd as u32),
                            properties.sample_rate().map(|sr| sr as f64),
                        )
                    }
                    Err(_) => (None, None),
                };

                let album_artist = metadata.album_artist.as_ref().unwrap_or(&metadata.artist);
                let album_group_key = format!("{}|{}", metadata.album, album_artist);
                let lib_opt = library_db.lock().await;
                if let Some(lib_guard) = lib_opt.as_ref() {
                    let _ = lib_guard.insert_qobuz_cached_track_with_grouping(
                        track_id,
                        &metadata.title,
                        &metadata.artist,
                        Some(&metadata.album),
                        metadata.album_artist.as_deref(),
                        metadata.track_number,
                        metadata.disc_number,
                        metadata.year,
                        metadata.duration_secs,
                        &new_path,
                        &album_group_key,
                        &metadata.album,
                        bit_depth_detected,
                        sample_rate_detected,
                        None,
                    );
                }

                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_file_path(track_id, &new_path);
                }

                let _ = app.emit("offline:caching_processed", serde_json::json!({
                    "trackId": track_id,
                    "path": new_path
                }));
            }
            Err(e) => {
                log::error!("Caching failed for track {}: {}", track_id, e);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(track_id, crate::offline_cache::OfflineCacheStatus::Failed, Some(&e));
                }
                let _ = app.emit("offline:caching_failed", serde_json::json!({
                    "trackId": track_id,
                    "error": e
                }));
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn v2_start_legacy_migration(
    state: State<'_, AppState>,
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, crate::library::LibraryState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    log::info!("Command: v2_start_legacy_migration");
    let tracks_dir = cache_state.cache_dir.read().unwrap().join("tracks");
    let track_ids = crate::offline_cache::detect_legacy_cached_files(&tracks_dir)?;

    if track_ids.is_empty() {
        return Err("No legacy cached files found".to_string());
    }

    let offline_root = cache_state.get_cache_path();
    let qobuz_client = state.client.clone();
    let library_db = library_state.db.clone();
    let app_complete = app_handle.clone();

    tokio::spawn(async move {
        let status = crate::offline_cache::migrate_legacy_cached_files(
            track_ids,
            tracks_dir,
            offline_root,
            qobuz_client,
            library_db,
        )
        .await;
        let _ = app_complete.emit("migration:complete", status);
    });
    Ok(())
}

#[tauri::command]
pub async fn v2_library_scan(
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_scan_impl(state).await
}

#[tauri::command]
pub async fn v2_library_stop_scan(
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_stop_scan_impl(state).await
}

#[tauri::command]
pub async fn v2_library_scan_folder(
    folder_id: i64,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_scan_folder_impl(folder_id, state).await
}

#[tauri::command]
pub async fn v2_library_clear(
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_clear_impl(state).await
}

#[tauri::command]
pub async fn v2_library_update_album_metadata(
    request: crate::library::LibraryAlbumMetadataUpdateRequest,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_update_album_metadata_impl(request, state).await
}

#[tauri::command]
pub async fn v2_library_write_album_metadata_to_files(
    app: tauri::AppHandle,
    request: crate::library::LibraryAlbumMetadataUpdateRequest,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_write_album_metadata_to_files_impl(app, request, state).await
}

#[tauri::command]
pub async fn v2_library_refresh_album_metadata_from_files(
    album_group_key: String,
    state: State<'_, crate::library::LibraryState>,
) -> Result<(), String> {
    crate::library::library_refresh_album_metadata_from_files_impl(album_group_key, state).await
}

#[tauri::command]
pub async fn v2_factory_reset(
    app_state: State<'_, AppState>,
    user_paths: State<'_, crate::user_data::UserDataPaths>,
    session_store: State<'_, crate::session_store::SessionStoreState>,
    favorites_cache: State<'_, crate::config::favorites_cache::FavoritesCacheState>,
    subscription_state: State<'_, crate::config::subscription_state::SubscriptionStateState>,
    playback_prefs: State<'_, crate::config::playback_preferences::PlaybackPreferencesState>,
    favorites_prefs: State<'_, crate::config::favorites_preferences::FavoritesPreferencesState>,
    download_settings: State<'_, crate::config::download_settings::DownloadSettingsState>,
    audio_settings: State<'_, crate::config::audio_settings::AudioSettingsState>,
    tray_settings: State<'_, crate::config::tray_settings::TraySettingsState>,
    remote_control_settings: State<'_, crate::config::remote_control_settings::RemoteControlSettingsState>,
    allowed_origins: State<'_, crate::config::remote_control_settings::AllowedOriginsState>,
    legal_settings: State<'_, crate::config::legal_settings::LegalSettingsState>,
    updates: State<'_, crate::updates::UpdatesState>,
    library: State<'_, crate::library::LibraryState>,
    reco: State<'_, crate::reco_store::RecoState>,
    api_cache: State<'_, crate::api_cache::ApiCacheState>,
    artist_vectors: State<'_, crate::artist_vectors::ArtistVectorStoreState>,
    blacklist: State<'_, crate::artist_blacklist::BlacklistState>,
    offline: State<'_, crate::offline::OfflineState>,
    offline_cache: State<'_, crate::offline_cache::OfflineCacheState>,
    lyrics: State<'_, crate::lyrics::LyricsState>,
    musicbrainz: State<'_, crate::musicbrainz::MusicBrainzSharedState>,
    listenbrainz: State<'_, crate::listenbrainz::ListenBrainzSharedState>,
) -> Result<(), String> {
    log::warn!("FACTORY RESET: Starting - all application data will be deleted");

    let _ = app_state.player.stop();
    app_state.media_controls.set_stopped();

    session_store.teardown();
    let _ = favorites_cache.teardown();
    let _ = playback_prefs.teardown();
    let _ = favorites_prefs.teardown();
    let _ = audio_settings.teardown();
    let _ = tray_settings.teardown();
    let _ = remote_control_settings.teardown();
    let _ = allowed_origins.teardown();
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
    v2_teardown_type_alias_state(&*subscription_state);
    v2_teardown_type_alias_state(&*download_settings);
    v2_teardown_type_alias_state(&*legal_settings);

    user_paths.clear_user();
    crate::user_data::UserDataPaths::clear_last_user_id();

    if let Err(e) = crate::credentials::clear_qobuz_credentials() {
        log::error!("FACTORY RESET: Failed to clear credentials: {}", e);
    }

    if let Ok(data_dir) = crate::user_data::UserDataPaths::global_data_dir() {
        if data_dir.exists() {
            let _ = std::fs::remove_dir_all(&data_dir);
        }
    }
    if let Ok(cache_dir) = crate::user_data::UserDataPaths::global_cache_dir() {
        if cache_dir.exists() {
            let _ = std::fs::remove_dir_all(&cache_dir);
        }
    }
    if let Some(config_dir) = dirs::config_dir().map(|d| d.join("qbz")) {
        if config_dir.exists() {
            let _ = std::fs::remove_dir_all(&config_dir);
        }
    }

    log::warn!("FACTORY RESET: Complete - all application data deleted");
    Ok(())
}

#[tauri::command]
pub fn v2_set_qobuz_tos_accepted(
    state: State<'_, crate::config::legal_settings::LegalSettingsState>,
    accepted: bool,
) -> Result<(), String> {
    crate::config::legal_settings::set_qobuz_tos_accepted(state, accepted)
}

#[tauri::command]
pub fn v2_set_update_check_on_launch(
    enabled: bool,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::set_update_check_on_launch(enabled, state)
}

#[tauri::command]
pub fn v2_set_show_whats_new_on_launch(
    enabled: bool,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::set_show_whats_new_on_launch(enabled, state)
}

#[tauri::command]
pub fn v2_get_update_preferences(
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<crate::updates::UpdatePreferences, String> {
    crate::updates::get_update_preferences(state)
}

#[tauri::command]
pub fn v2_get_current_version(
    state: State<'_, crate::updates::UpdatesState>,
) -> String {
    crate::updates::get_current_version(state)
}

#[tauri::command]
pub async fn v2_check_for_updates(
    mode: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<crate::updates::UpdateCheckResult, String> {
    crate::updates::check_for_updates(mode, state).await
}

#[tauri::command]
pub async fn v2_fetch_release_for_version(
    version: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<Option<crate::updates::ReleaseInfo>, String> {
    crate::updates::fetch_release_for_version(version, state).await
}

#[tauri::command]
pub fn v2_acknowledge_release(
    version: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::acknowledge_release(version, state)
}

#[tauri::command]
pub fn v2_ignore_release(
    version: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::ignore_release(version, state)
}

#[tauri::command]
pub fn v2_mark_whats_new_shown(
    version: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::mark_whats_new_shown(version, state)
}

#[tauri::command]
pub fn v2_has_whats_new_been_shown(
    version: String,
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<bool, String> {
    crate::updates::has_whats_new_been_shown(version, state)
}

#[tauri::command]
pub fn v2_mark_flatpak_welcome_shown(
    state: State<'_, crate::updates::UpdatesState>,
) -> Result<(), String> {
    crate::updates::mark_flatpak_welcome_shown(state)
}

#[tauri::command]
pub fn v2_get_backend_logs() -> Vec<String> {
    crate::logging::get_backend_logs()
}

#[tauri::command]
pub async fn v2_upload_logs_to_paste(content: String) -> Result<String, String> {
    crate::logging::upload_logs_to_paste(content).await
}

#[tauri::command]
pub fn v2_set_show_downloads_in_library(
    show: bool,
    state: State<'_, crate::config::download_settings::DownloadSettingsState>,
) -> Result<(), String> {
    crate::config::download_settings::set_show_downloads_in_library(show, state)
}

#[tauri::command]
pub fn v2_get_download_settings(
    state: State<'_, crate::config::download_settings::DownloadSettingsState>,
) -> Result<crate::config::download_settings::DownloadSettings, String> {
    crate::config::download_settings::get_download_settings(state)
}

#[tauri::command]
pub async fn v2_lyrics_get_cache_stats(
    state: State<'_, crate::lyrics::LyricsState>,
) -> Result<crate::lyrics::commands::LyricsCacheStats, String> {
    crate::lyrics::commands::lyrics_get_cache_stats(state).await
}

#[tauri::command]
pub fn v2_lastfm_has_embedded_credentials() -> bool {
    crate::lastfm::LastFmClient::has_embedded_credentials()
}

#[tauri::command]
pub async fn v2_remote_control_get_status(
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlStatus, String> {
    crate::api_server::remote_control_get_status(app_handle).await
}

#[tauri::command]
pub async fn v2_remote_control_set_enabled(
    enabled: bool,
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlStatus, String> {
    crate::api_server::remote_control_set_enabled(enabled, app_handle).await
}

#[tauri::command]
pub async fn v2_remote_control_set_port(
    port: u16,
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlStatus, String> {
    crate::api_server::remote_control_set_port(port, app_handle).await
}

#[tauri::command]
pub async fn v2_remote_control_set_secure(
    secure: bool,
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlStatus, String> {
    crate::api_server::remote_control_set_secure(secure, app_handle).await
}

#[tauri::command]
pub async fn v2_remote_control_regenerate_token(
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlQr, String> {
    crate::api_server::remote_control_regenerate_token(app_handle).await
}

#[tauri::command]
pub async fn v2_remote_control_get_pairing_qr(
    app_handle: tauri::AppHandle,
) -> Result<crate::api_server::RemoteControlQr, String> {
    crate::api_server::remote_control_get_pairing_qr(app_handle).await
}

#[tauri::command]
pub fn v2_is_running_in_flatpak() -> bool {
    crate::flatpak::is_running_in_flatpak()
}

#[tauri::command]
pub async fn v2_detect_legacy_cached_files(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<crate::offline_cache::MigrationStatus, String> {
    let tracks_dir = cache_state.cache_dir.read().unwrap().join("tracks");
    let track_ids = crate::offline_cache::detect_legacy_cached_files(&tracks_dir)?;
    Ok(crate::offline_cache::MigrationStatus {
        has_legacy_files: !track_ids.is_empty(),
        total_tracks: track_ids.len(),
        ..Default::default()
    })
}

#[tauri::command]
pub fn v2_get_device_sample_rate_limit(
    state: State<'_, crate::config::audio_settings::AudioSettingsState>,
    device_id: String,
) -> Result<Option<u32>, String> {
    crate::config::audio_settings::get_device_sample_rate_limit(state, device_id)
}

#[tauri::command]
pub fn v2_set_device_sample_rate_limit(
    state: State<'_, crate::config::audio_settings::AudioSettingsState>,
    device_id: String,
    rate: Option<u32>,
) -> Result<(), String> {
    crate::config::audio_settings::set_device_sample_rate_limit(state, device_id, rate)
}

#[tauri::command]
pub fn v2_set_force_x11(
    state: State<'_, crate::config::graphics_settings::GraphicsSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    crate::config::graphics_settings::set_force_x11(state, enabled)
}

#[tauri::command]
pub fn v2_restart_app(app: tauri::AppHandle) {
    log::info!("[V2] App restart requested by user");
    app.restart();
}
