//! V2 Commands - Using the new multi-crate architecture
//!
//! These commands use QbzCore via CoreBridge instead of the old AppState.
//! Runtime contract ensures proper lifecycle (see ADR_RUNTIME_SESSION_CONTRACT.md).
//!
//! Playback flows through CoreBridge -> QbzCore -> Player (qbz-player crate).

use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::RwLock;

use qbz_models::{Album, Artist, Playlist, Quality, QueueState, RepeatMode, SearchResultsPage, Track, UserSession};

use crate::artist_blacklist::BlacklistState;
use crate::cache::AudioCache;
use crate::audio::{AlsaPlugin, AudioBackendType};
use crate::config::audio_settings::{AudioSettings, AudioSettingsState};
use crate::core_bridge::CoreBridgeState;
use crate::offline_cache::OfflineCacheState;
use crate::queue::QueueManager;
use crate::runtime::{RuntimeManagerState, RuntimeStatus, RuntimeError, RuntimeEvent, DegradedReason, CommandRequirement};
use crate::AppState;

// ==================== Helper Functions ====================

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
) -> Result<RuntimeStatus, String> {
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

    // Step 2: Check for saved credentials and attempt auto-login
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

                // Step 3: Activate per-user session
                // This is done by calling activate_user_session command
                // For now, we just mark the user_id and let the frontend handle it
                // In a full implementation, we'd activate here directly

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
/// Call this first, then let frontend handle ToS check before calling v2_auto_login.
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
#[tauri::command]
pub async fn v2_auto_login(
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
    app_state: State<'_, AppState>,
    core_bridge: State<'_, CoreBridgeState>,
) -> Result<V2LoginResponse, String> {
    let manager = runtime.manager();

    log::info!("[V2] v2_auto_login starting...");

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
#[tauri::command]
pub async fn v2_manual_login(
    email: String,
    password: String,
    app: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
    app_state: State<'_, AppState>,
    core_bridge: State<'_, CoreBridgeState>,
) -> Result<V2LoginResponse, String> {
    let manager = runtime.manager();

    log::info!("[V2] v2_manual_login starting...");

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
fn spawn_v2_prefetch(
    bridge: Arc<RwLock<Option<crate::core_bridge::CoreBridge>>>,
    cache: Arc<AudioCache>,
    queue: &QueueManager,
    quality: Quality,
    streaming_only: bool,
) {
    // Skip prefetch entirely in streaming_only mode
    if streaming_only {
        log::debug!("[V2/PREFETCH] Skipped - streaming_only mode active");
        return;
    }

    // Look further ahead to find Qobuz tracks in mixed playlists
    let upcoming_tracks = queue.peek_upcoming(V2_PREFETCH_LOOKAHEAD);

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
) -> Result<bool, String> {
    let bridge = bridge.get().await;
    Ok(bridge.is_logged_in().await)
}

/// Login with email and password (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_login(
    email: String,
    password: String,
    bridge: State<'_, CoreBridgeState>,
) -> Result<UserSession, String> {
    let bridge = bridge.get().await;
    bridge.login(&email, &password).await
}

/// Logout current user (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_logout(
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.logout().await
}

// ==================== Queue Commands (V2) ====================

/// Get current queue state (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_queue_state(
    bridge: State<'_, CoreBridgeState>,
) -> Result<QueueState, String> {
    let bridge = bridge.get().await;
    Ok(bridge.get_queue_state().await)
}

/// Set repeat mode (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_set_repeat_mode(
    mode: RepeatMode,
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.set_repeat_mode(mode).await;
    Ok(())
}

/// Toggle shuffle (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_toggle_shuffle(
    bridge: State<'_, CoreBridgeState>,
) -> Result<bool, String> {
    let bridge = bridge.get().await;
    Ok(bridge.toggle_shuffle().await)
}

/// Set shuffle mode directly (V2)
#[tauri::command]
pub async fn v2_set_shuffle(
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] set_shuffle: {}", enabled);
    app_state.queue.set_shuffle(enabled);
    Ok(())
}

/// Clear the queue (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_clear_queue(
    bridge: State<'_, CoreBridgeState>,
) -> Result<(), String> {
    let bridge = bridge.get().await;
    bridge.clear_queue().await;
    Ok(())
}

/// Queue track representation for V2 commands
/// Maps to internal QueueTrack format
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct V2QueueTrack {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    #[serde(rename = "duration")]
    pub duration_secs: u64,
    #[serde(rename = "artwork")]
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

/// Add track to the end of the queue (V2)
#[tauri::command]
pub async fn v2_add_to_queue(
    track: V2QueueTrack,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] add_to_queue: {} - {}", track.id, track.title);
    app_state.queue.add_track(track.into());
    Ok(())
}

/// Add track to play next (V2)
#[tauri::command]
pub async fn v2_add_to_queue_next(
    track: V2QueueTrack,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] add_to_queue_next: {} - {}", track.id, track.title);
    app_state.queue.add_track_next(track.into());
    Ok(())
}

/// Set the entire queue and start playing from index (V2)
#[tauri::command]
pub async fn v2_set_queue(
    tracks: Vec<V2QueueTrack>,
    start_index: usize,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] set_queue: {} tracks, start at {}", tracks.len(), start_index);
    let queue_tracks: Vec<crate::queue::QueueTrack> = tracks.into_iter().map(Into::into).collect();
    app_state.queue.set_queue(queue_tracks, Some(start_index));
    Ok(())
}

/// Remove a track from the queue by index (V2)
#[tauri::command]
pub async fn v2_remove_from_queue(
    index: usize,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] remove_from_queue: index {}", index);
    app_state.queue.remove_track(index);
    Ok(())
}

/// Remove a track from the upcoming queue by its position (V2)
/// (0 = first upcoming track, handles shuffle mode correctly)
#[tauri::command]
pub async fn v2_remove_upcoming_track(
    upcoming_index: usize,
    app_state: State<'_, AppState>,
) -> Result<Option<V2QueueTrack>, String> {
    log::info!("[V2] remove_upcoming_track: upcoming_index {}", upcoming_index);
    Ok(app_state.queue.remove_upcoming_track(upcoming_index).map(Into::into))
}

/// Skip to next track in queue (V2)
#[tauri::command]
pub async fn v2_next_track(
    app_state: State<'_, AppState>,
) -> Result<Option<V2QueueTrack>, String> {
    log::info!("[V2] next_track");
    let track = app_state.queue.next();
    Ok(track.map(Into::into))
}

/// Go to previous track in queue (V2)
#[tauri::command]
pub async fn v2_previous_track(
    app_state: State<'_, AppState>,
) -> Result<Option<V2QueueTrack>, String> {
    log::info!("[V2] previous_track");
    let track = app_state.queue.previous();
    Ok(track.map(Into::into))
}

/// Play a specific track in the queue by index (V2)
#[tauri::command]
pub async fn v2_play_queue_index(
    index: usize,
    app_state: State<'_, AppState>,
) -> Result<Option<V2QueueTrack>, String> {
    log::info!("[V2] play_queue_index: {}", index);
    let track = app_state.queue.play_index(index);
    Ok(track.map(Into::into))
}

/// Move a track within the queue (V2)
#[tauri::command]
pub async fn v2_move_queue_track(
    from_index: usize,
    to_index: usize,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    log::info!("[V2] move_queue_track: {} -> {}", from_index, to_index);
    Ok(app_state.queue.move_track(from_index, to_index))
}

/// Add multiple tracks to queue (V2)
#[tauri::command]
pub async fn v2_add_tracks_to_queue(
    tracks: Vec<V2QueueTrack>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] add_tracks_to_queue: {} tracks", tracks.len());
    let queue_tracks: Vec<crate::queue::QueueTrack> = tracks.into_iter().map(|t| t.into()).collect();
    app_state.queue.add_tracks(queue_tracks);
    Ok(())
}

/// Add multiple tracks to play next (V2)
/// Tracks are added in reverse order so they play in the order provided
#[tauri::command]
pub async fn v2_add_tracks_to_queue_next(
    tracks: Vec<V2QueueTrack>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[V2] add_tracks_to_queue_next: {} tracks", tracks.len());
    // Add in reverse order so they end up in the correct order
    for track in tracks.into_iter().rev() {
        let queue_track: crate::queue::QueueTrack = track.into();
        app_state.queue.add_track_next(queue_track);
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
) -> Result<SearchResultsPage<Album>, String> {
    // Runtime contract: require CoreBridge auth for V2 search
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    let bridge = bridge.get().await;
    let mut results = bridge.search_albums(&query, limit, offset, searchType.as_deref()).await?;

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
) -> Result<SearchResultsPage<Track>, String> {
    // Runtime contract: require CoreBridge auth for V2 search
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    let bridge = bridge.get().await;
    let mut results = bridge.search_tracks(&query, limit, offset, searchType.as_deref()).await?;

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
) -> Result<SearchResultsPage<Artist>, String> {
    // Runtime contract: require CoreBridge auth for V2 search
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    let bridge = bridge.get().await;
    let mut results = bridge.search_artists(&query, limit, offset, searchType.as_deref()).await?;

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
) -> Result<Album, String> {
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;
    let bridge = bridge.get().await;
    bridge.get_album(&albumId).await
}

/// Get track by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_track(
    trackId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Track, String> {
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;
    let bridge = bridge.get().await;
    bridge.get_track(trackId).await
}

/// Get artist by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_artist(
    artistId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Artist, String> {
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;
    let bridge = bridge.get().await;
    bridge.get_artist(artistId).await
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
) -> Result<serde_json::Value, String> {
    // Runtime contract: require CoreBridge auth for V2 favorites
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    let bridge = bridge.get().await;
    bridge.get_favorites(&favType, limit, offset).await
}

/// Add item to favorites (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_add_favorite(
    favType: String,
    itemId: String,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), String> {
    // Runtime contract: require CoreBridge auth for V2 favorites
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    log::info!("[V2] add_favorite type={} id={}", favType, itemId);
    let bridge = bridge.get().await;
    bridge.add_favorite(&favType, &itemId).await
}

/// Remove item from favorites (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_remove_favorite(
    favType: String,
    itemId: String,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), String> {
    // Runtime contract: require CoreBridge auth for V2 favorites
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    log::info!("[V2] remove_favorite type={} id={}", favType, itemId);
    let bridge = bridge.get().await;
    bridge.remove_favorite(&favType, &itemId).await
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
) -> Result<(), String> {
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await
        .map_err(|e| e.to_string())?;
    log::info!("[V2] Command: pause_playback");
    app_state.media_controls.set_playback(false);
    let bridge = bridge.get().await;
    bridge.pause()
}

/// Resume playback (V2)
#[tauri::command]
pub async fn v2_resume_playback(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), String> {
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await
        .map_err(|e| e.to_string())?;
    log::info!("[V2] Command: resume_playback");
    app_state.media_controls.set_playback(true);
    let bridge = bridge.get().await;
    bridge.resume()
}

/// Stop playback (V2)
#[tauri::command]
pub async fn v2_stop_playback(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), String> {
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await
        .map_err(|e| e.to_string())?;
    log::info!("[V2] Command: stop_playback");
    app_state.media_controls.set_stopped();
    let bridge = bridge.get().await;
    bridge.stop()
}

/// Seek to position in seconds (V2)
#[tauri::command]
pub async fn v2_seek(
    position: u64,
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), String> {
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await
        .map_err(|e| e.to_string())?;
    log::info!("[V2] Command: seek {}", position);
    let bridge_guard = bridge.get().await;
    let result = bridge_guard.seek(position);

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
) -> Result<(), String> {
    runtime.manager().check_requirements(CommandRequirement::RequiresClientInit).await
        .map_err(|e| e.to_string())?;
    let bridge = bridge.get().await;
    bridge.set_volume(volume)
}

/// Get current playback state (V2) - also updates MPRIS progress
#[tauri::command]
pub async fn v2_get_playback_state(
    bridge: State<'_, CoreBridgeState>,
    app_state: State<'_, AppState>,
) -> Result<qbz_player::PlaybackState, String> {
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
) -> Result<(), String> {
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
) -> Result<bool, String> {
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
                    .map_err(|e| format!("Failed to read cached file: {}", e))?;
                player.play_next(audio_data, track_id)?;
                return Ok(true);
            }
        }
    }

    // Check memory cache (L1)
    let cache = app_state.audio_cache.clone();
    if let Some(cached) = cache.get(track_id) {
        log::info!("[V2/GAPLESS] Track {} from MEMORY cache ({} bytes)", track_id, cached.size_bytes);
        player.play_next(cached.data, track_id)?;
        return Ok(true);
    }

    // Check playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!("[V2/GAPLESS] Track {} from DISK cache ({} bytes)", track_id, audio_data.len());
            player.play_next(audio_data, track_id)?;
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
) -> Result<(), String> {
    let preferred_quality = parse_quality(quality.as_deref());

    // Apply per-device sample rate limit if enabled
    let final_quality = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
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
    let result = async {
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
                        .map_err(|e| format!("Failed to read cached file: {}", e))?;
                    cache.insert(track_id, audio_data);
                    return Ok(());
                }
            }
        }

        let bridge_guard = bridge.get().await;
        let stream_url = bridge_guard.get_stream_url(track_id, final_quality).await?;
        drop(bridge_guard);

        let audio_data = download_audio(&stream_url.url).await?;
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
) -> Result<V2PlayTrackResult, String> {
    // Runtime contract: require CoreBridge auth for V2 playback
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    let preferred_quality = parse_quality(quality.as_deref());

    // Apply per-device sample rate limit if enabled
    let final_quality = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
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
        let guard = audio_settings.store.lock().map_err(|e| format!("Lock error: {}", e))?;
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
                    .map_err(|e| format!("Failed to read cached file: {}", e))?;
                player.play_data(audio_data, track_id)?;

                // Prefetch next tracks in background
                drop(bridge_guard);
                spawn_v2_prefetch(bridge.0.clone(), app_state.audio_cache.clone(), &app_state.queue, final_quality, streaming_only);
                return Ok(V2PlayTrackResult { format_id: None });
            }
        }
    }

    // Check memory cache (L1) - using AppState's audio_cache for now
    // TODO: Move cache to qbz-core in future refactor
    let cache = app_state.audio_cache.clone();
    if let Some(cached) = cache.get(track_id) {
        log::info!("[V2/CACHE HIT] Track {} from MEMORY cache ({} bytes)", track_id, cached.size_bytes);
        player.play_data(cached.data, track_id)?;

        // Prefetch next tracks in background
        drop(bridge_guard);
        spawn_v2_prefetch(bridge.0.clone(), cache.clone(), &app_state.queue, final_quality, streaming_only);
        return Ok(V2PlayTrackResult { format_id: None });
    }

    // Check playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!("[V2/CACHE HIT] Track {} from DISK cache ({} bytes)", track_id, audio_data.len());
            cache.insert(track_id, audio_data.clone());
            player.play_data(audio_data, track_id)?;

            // Prefetch next tracks in background
            drop(bridge_guard);
            spawn_v2_prefetch(bridge.0.clone(), cache.clone(), &app_state.queue, final_quality, streaming_only);
            return Ok(V2PlayTrackResult { format_id: None });
        }
    }

    // Not in any cache - get stream URL from Qobuz via CoreBridge
    log::info!("[V2] Track {} not in cache, fetching from network...", track_id);

    let stream_url = bridge_guard.get_stream_url(track_id, final_quality).await?;
    log::info!("[V2] Got stream URL for track {} (format_id={})", track_id, stream_url.format_id);

    // Download the audio
    let audio_data = download_audio(&stream_url.url).await?;
    let data_size = audio_data.len();

    // Cache it (unless streaming_only mode)
    if !streaming_only {
        cache.insert(track_id, audio_data.clone());
        log::info!("[V2/CACHED] Track {} stored in memory cache", track_id);
    } else {
        log::info!("[V2/NOT CACHED] Track {} - streaming_only mode active", track_id);
    }

    // Play it via qbz-player
    player.play_data(audio_data, track_id)?;
    log::info!("[V2] Playing track {} ({} bytes)", track_id, data_size);

    // Prefetch next tracks in background
    drop(bridge_guard);
    spawn_v2_prefetch(bridge.0.clone(), cache, &app_state.queue, final_quality, streaming_only);

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
) -> Result<(), String> {
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

    player.reinit_device(device)
}

// ==================== Playlist Commands (V2) ====================

/// Get user playlists (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_user_playlists(
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<Playlist>, String> {
    // Runtime contract: require CoreBridge auth for V2 playlists
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    log::info!("[V2] get_user_playlists");
    let bridge = bridge.get().await;
    bridge.get_user_playlists().await
}

/// Get playlist by ID (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_playlist(
    playlistId: u64,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Playlist, String> {
    // Runtime contract: require CoreBridge auth for V2 playlists
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    log::info!("[V2] get_playlist: {}", playlistId);
    let bridge = bridge.get().await;
    bridge.get_playlist(playlistId).await
}

/// Add tracks to playlist (V2 - uses QbzCore)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_add_tracks_to_playlist(
    playlistId: u64,
    trackIds: Vec<u64>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), String> {
    // Runtime contract: require CoreBridge auth for V2 playlists
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    log::info!("[V2] add_tracks_to_playlist: playlist {} <- {} tracks", playlistId, trackIds.len());
    let bridge = bridge.get().await;
    bridge.add_tracks_to_playlist(playlistId, &trackIds).await
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
) -> Result<(), String> {
    // Runtime contract: require CoreBridge auth for V2 playlists
    runtime.manager().check_requirements(CommandRequirement::RequiresCoreBridgeAuth).await
        .map_err(|e| e.to_string())?;

    let ptids = playlistTrackIds.unwrap_or_default();
    let tids = trackIds.unwrap_or_default();
    log::info!(
        "[V2] remove_tracks_from_playlist: playlist {} (playlistTrackIds={}, trackIds={})",
        playlistId, ptids.len(), tids.len()
    );

    let bridge = bridge.get().await;

    // If we have direct playlist_track_ids, use them
    if !ptids.is_empty() {
        return bridge.remove_tracks_from_playlist(playlistId, &ptids).await;
    }

    // Otherwise resolve track_ids → playlist_track_ids via full playlist fetch
    if !tids.is_empty() {
        let playlist = bridge.get_playlist(playlistId).await?;

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
            return Err("Could not resolve any track IDs to playlist track IDs".to_string());
        }

        return bridge.remove_tracks_from_playlist(playlistId, &resolved_ptids).await;
    }

    Err("Either playlistTrackIds or trackIds must be provided".to_string())
}

// ==================== Audio Settings Commands (V2) ====================

/// Get current audio settings (V2)
#[tauri::command]
pub fn v2_get_audio_settings(
    state: State<'_, AudioSettingsState>,
) -> Result<AudioSettings, String> {
    log::info!("[V2] get_audio_settings");
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_settings()
}

/// Set audio output device (V2)
#[tauri::command]
pub fn v2_set_audio_output_device(
    device: Option<String>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
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
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_output_device(normalized_device.as_deref())
}

/// Set audio exclusive mode (V2)
#[tauri::command]
pub fn v2_set_audio_exclusive_mode(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_exclusive_mode: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_exclusive_mode(enabled)
}

/// Set DAC passthrough mode (V2)
#[tauri::command]
pub fn v2_set_audio_dac_passthrough(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_dac_passthrough: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_dac_passthrough(enabled)
}

/// Set preferred sample rate (V2)
#[tauri::command]
pub fn v2_set_audio_sample_rate(
    rate: Option<u32>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_sample_rate: {:?}", rate);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_sample_rate(rate)
}

/// Set audio backend type (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_set_audio_backend_type(
    backendType: Option<AudioBackendType>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_backend_type: {:?}", backendType);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_backend_type(backendType)
}

/// Set ALSA plugin (V2)
#[tauri::command]
pub fn v2_set_audio_alsa_plugin(
    plugin: Option<AlsaPlugin>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_alsa_plugin: {:?}", plugin);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_alsa_plugin(plugin)
}

/// Set gapless playback enabled (V2)
#[tauri::command]
pub fn v2_set_audio_gapless_enabled(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_gapless_enabled: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_gapless_enabled(enabled)
}

/// Set normalization enabled (V2)
#[tauri::command]
pub fn v2_set_audio_normalization_enabled(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_normalization_enabled: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_normalization_enabled(enabled)
}

/// Set normalization target LUFS (V2)
#[tauri::command]
pub fn v2_set_audio_normalization_target(
    target: f32,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_normalization_target: {}", target);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_normalization_target_lufs(target)
}

/// Set device max sample rate (V2)
#[tauri::command]
pub fn v2_set_audio_device_max_sample_rate(
    rate: Option<u32>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_device_max_sample_rate: {:?}", rate);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_device_max_sample_rate(rate)
}

/// Set limit quality to device capability (V2)
#[tauri::command]
pub fn v2_set_audio_limit_quality_to_device(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_limit_quality_to_device: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_limit_quality_to_device(enabled)
}

/// Set streaming only mode (V2)
#[tauri::command]
pub fn v2_set_audio_streaming_only(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_streaming_only: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_streaming_only(enabled)
}

/// Reset audio settings to defaults (V2)
#[tauri::command]
pub fn v2_reset_audio_settings(
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] reset_audio_settings");
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.reset_all().map(|_| ())
}

/// Set stream first track enabled (V2)
#[tauri::command]
pub fn v2_set_audio_stream_first_track(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_stream_first_track: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_stream_first_track(enabled)
}

/// Set stream buffer seconds (V2)
#[tauri::command]
pub fn v2_set_audio_stream_buffer_seconds(
    seconds: u8,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_stream_buffer_seconds: {}", seconds);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_stream_buffer_seconds(seconds)
}

/// Set ALSA hardware volume control (V2)
#[tauri::command]
pub fn v2_set_audio_alsa_hardware_volume(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    log::info!("[V2] set_audio_alsa_hardware_volume: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_alsa_hardware_volume(enabled)
}
