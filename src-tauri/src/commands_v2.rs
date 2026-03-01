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
    PlaylistTag, Quality, QueueState,
    QueueTrack as CoreQueueTrack, RepeatMode, SearchResultsPage, Track, UserSession,
};

use crate::api::models::{
    DynamicSuggestRequest, DynamicSuggestResponse, DynamicTrackToAnalyse, PlaylistDuplicateResult,
    PlaylistWithTrackIds, PurchaseAlbum, PurchaseIdsResponse, PurchaseResponse, PurchaseTrack,
    SearchResultsPage as ApiSearchResultsPage,
};
use crate::api_cache::ApiCacheState;
use crate::artist_blacklist::BlacklistState;
use crate::artist_vectors::ArtistVectorStoreState;
use crate::audio::{AlsaPlugin, AudioBackendType, AudioDevice, BackendManager};
use crate::cache::{AudioCache, CacheStats};
use crate::cast::{
    AirPlayMetadata, AirPlayState, CastState, DlnaMetadata, DlnaState, MediaMetadata,
};
use crate::config::audio_settings::{AudioSettings, AudioSettingsState};
use crate::config::developer_settings::{DeveloperSettings, DeveloperSettingsState};
use crate::config::download_settings::DownloadSettingsState;
use crate::config::favorites_preferences::FavoritesPreferences;
use crate::config::graphics_settings::{
    GraphicsSettings, GraphicsSettingsState, GraphicsStartupStatus,
};
use crate::config::legal_settings::LegalSettingsState;
use crate::config::playback_preferences::{
    AutoplayMode, PlaybackPreferences, PlaybackPreferencesState,
};
use crate::config::tray_settings::TraySettings;
use crate::config::tray_settings::TraySettingsState;
use crate::config::window_settings::WindowSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::library::{
    get_artwork_cache_dir, thumbnails, LibraryState, LocalAlbum, LocalTrack, MetadataExtractor,
    PlaylistLocalTrack, PlaylistSettings, PlaylistStats, ScanProgress,
};
use crate::lyrics::LyricsState;
use crate::musicbrainz::{CacheStats as MusicBrainzCacheStats, MusicBrainzSharedState};
use crate::offline::OfflineState;
use crate::offline_cache::OfflineCacheState;
use crate::playback_context::{ContentSource, ContextType, PlaybackContext};
use crate::plex::{PlexMusicSection, PlexPlayResult, PlexServerInfo, PlexTrack};
use crate::reco_store::{HomeResolved, HomeSeeds, RecoEventInput, RecoState};
use crate::runtime::{
    CommandRequirement, DegradedReason, RuntimeError, RuntimeEvent, RuntimeManagerState,
    RuntimeStatus,
};
use crate::AppState;
use md5::{Digest, Md5};
use notify_rust::Notification;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

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
pub struct HardwareAudioStatus {
    pub hardware_sample_rate: Option<u32>,
    pub hardware_format: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DacCapabilities {
    pub node_name: String,
    pub sample_rates: Vec<u32>,
    pub formats: Vec<String>,
    pub channels: Option<u32>,
    pub description: Option<String>,
    pub error: Option<String>,
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

/// Convert config AudioSettings to qbz_audio::AudioSettings.
/// Used by runtime_bootstrap (once at startup) and v2_reinit_audio_device
/// to ensure the Player has fresh settings from the database.
fn convert_to_qbz_audio_settings(settings: &AudioSettings) -> qbz_audio::AudioSettings {
    qbz_audio::AudioSettings {
        output_device: settings.output_device.clone(),
        exclusive_mode: settings.exclusive_mode,
        dac_passthrough: settings.dac_passthrough,
        preferred_sample_rate: settings.preferred_sample_rate,
        limit_quality_to_device: settings.limit_quality_to_device,
        device_max_sample_rate: settings.device_max_sample_rate,
        device_sample_rate_limits: settings.device_sample_rate_limits.clone(),
        backend_type: settings.backend_type.clone(),
        alsa_plugin: settings.alsa_plugin.clone(),
        alsa_hardware_volume: false,
        stream_first_track: settings.stream_first_track,
        stream_buffer_seconds: settings.stream_buffer_seconds,
        streaming_only: settings.streaming_only,
        normalization_enabled: settings.normalization_enabled,
        normalization_target_lufs: settings.normalization_target_lufs,
        gapless_enabled: settings.gapless_enabled,
        pw_force_bitperfect: settings.pw_force_bitperfect,
    }
}

/// Persist ToS acceptance and remove the backend gate for login commands.
///
/// Calling any login command IS the user's ToS acceptance (they had to check
/// the checkbox on the frontend to enable the button).  We persist the value
/// best-effort, re-initializing the store if it was torn down (e.g. after a
/// factory reset), so subsequent bootstrap auto-logins work correctly.
fn accept_tos_best_effort(legal_state: &LegalSettingsState) {
    use crate::config::legal_settings::LegalSettingsStore;
    if let Ok(mut guard) = legal_state.lock() {
        // Re-initialize the store if it was torn down (e.g. after factory reset).
        if guard.is_none() {
            if let Ok(new_store) = LegalSettingsStore::new() {
                *guard = Some(new_store);
            }
        }
        if let Some(store) = guard.as_ref() {
            let _ = store.set_qobuz_tos_accepted(true);
        }
    }
}

/// Rollback runtime auth state after a partial login failure.
///
/// This MUST be called when:
/// - Legacy auth succeeded but CoreBridge auth failed
/// - Legacy + CoreBridge auth succeeded but session activation failed
///
/// Ensures runtime_get_status never reports a half-authenticated state.
async fn rollback_auth_state(manager: &crate::runtime::RuntimeManager, app: &tauri::AppHandle) {
    log::warn!("[V2] Rolling back auth state after partial login failure");
    manager.set_legacy_auth(false, None).await;
    manager.set_corebridge_auth(false).await;
    manager.set_session_activated(false, 0).await;
    let _ = app.emit(
        "runtime:event",
        RuntimeEvent::AuthChanged {
            logged_in: false,
            user_id: None,
        },
    );
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
                    max_rate,
                    quality.label()
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

/// Probe sample rate from FLAC audio data by reading the STREAMINFO header.
/// Returns None for non-FLAC data or data too short to parse.
fn probe_flac_sample_rate(data: &[u8]) -> Option<u32> {
    // FLAC format: "fLaC" magic + metadata blocks
    // First block is always STREAMINFO (34 bytes)
    // Bytes 18-20 of STREAMINFO contain: sample_rate (20 bits) | channels (3 bits) | bps (5 bits) | ...
    if data.len() < 22 || &data[0..4] != b"fLaC" {
        return None;
    }
    let sr = ((data[18] as u32) << 12) | ((data[19] as u32) << 4) | ((data[20] as u32) >> 4);
    if sr > 0 { Some(sr) } else { None }
}

/// Check if cached audio data has a sample rate that the current ALSA hardware
/// doesn't support. Returns true if the audio should NOT be played from cache
/// (needs re-fetch at lower quality).
#[cfg(target_os = "linux")]
fn cached_audio_incompatible_with_hw(
    audio_data: &[u8],
    audio_settings: &AudioSettingsState,
) -> bool {
    let sample_rate = match probe_flac_sample_rate(audio_data) {
        Some(rate) => rate,
        None => return false, // Can't determine format, assume compatible
    };

    let guard = match audio_settings.store.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let store = match guard.as_ref() {
        Some(s) => s,
        None => return false,
    };
    let settings = match store.get_settings() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let is_alsa = matches!(
        settings.backend_type,
        Some(qbz_audio::AudioBackendType::Alsa)
    );
    if !is_alsa {
        return false;
    }

    if let Some(ref device_id) = settings.output_device {
        match qbz_audio::device_supports_sample_rate(device_id, sample_rate) {
            Some(false) => {
                log::info!(
                    "[V2/Quality] Cached audio at {}Hz incompatible with hardware, will re-fetch at lower quality",
                    sample_rate
                );
                true
            }
            _ => false,
        }
    } else {
        false
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
        return Err(format!(
            "Failed to download artwork: HTTP {}",
            response.status()
        ));
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

    // Step 3: Check for saved credentials and attempt auto-login.
    // NOTE: user_id hint is optional; credentials are the source of truth.
    // This keeps bootstrap robust even if last_user_id is missing/corrupt.
    let creds = crate::credentials::load_qobuz_credentials();
    let last_user_id_hint = crate::user_data::UserDataPaths::load_last_user_id();

    if let Ok(Some(creds)) = creds {
        if last_user_id_hint.is_some() {
            log::info!("[Runtime] Found saved credentials, attempting auto-login");
        } else {
            log::info!("[Runtime] Found saved credentials, attempting auto-login (no hint)");
        }

        // Login to legacy client
        let client = app_state.client.read().await;
        match client.login(&creds.email, &creds.password).await {
            Ok(session) => {
                log::info!("[Runtime] Legacy auth successful");
                manager.set_legacy_auth(true, Some(session.user_id)).await;
                let _ = app.emit(
                    "runtime:event",
                    RuntimeEvent::AuthChanged {
                        logged_in: true,
                        user_id: Some(session.user_id),
                    },
                );

                // Step 4: Wait for CoreBridge init, then authenticate V2 - REQUIRED per ADR
                let cb_start = std::time::Instant::now();
                let cb_timeout = std::time::Duration::from_secs(10);
                let cb_poll = std::time::Duration::from_millis(50);

                loop {
                    if core_bridge.try_get().await.is_some() {
                        break;
                    }
                    if cb_start.elapsed() > cb_timeout {
                        log::error!("[Runtime] CoreBridge not available after 10s");
                        manager.set_bootstrap_in_progress(false).await;
                        return Err(RuntimeError::V2NotInitialized);
                    }
                    tokio::time::sleep(cb_poll).await;
                }
                log::info!("[Runtime] CoreBridge ready after {:?}", cb_start.elapsed());

                if let Some(bridge) = core_bridge.try_get().await {
                    match bridge.login(&creds.email, &creds.password).await {
                        Ok(_) => {
                            log::info!("[Runtime] CoreBridge auth successful");
                            manager.set_corebridge_auth(true).await;
                        }
                        Err(e) => {
                            log::error!("[Runtime] CoreBridge auth failed: {}", e);
                            let _ = app.emit(
                                "runtime:event",
                                RuntimeEvent::CoreBridgeAuthFailed {
                                    error: e.to_string(),
                                },
                            );
                            manager.set_bootstrap_in_progress(false).await;
                            return Err(RuntimeError::V2AuthFailed(e));
                        }
                    }
                } else {
                    log::error!("[Runtime] CoreBridge disappeared after ready check");
                    manager.set_bootstrap_in_progress(false).await;
                    return Err(RuntimeError::V2NotInitialized);
                }

                // Step 5: Activate per-user session - REQUIRED (FATAL if fails)
                // This initializes all per-user stores and sets runtime state
                // Session activation failure is FATAL per parity with v2_auto_login/v2_manual_login
                if let Err(e) =
                    crate::session_lifecycle::activate_session(&app, session.user_id).await
                {
                    log::error!("[Runtime] Session activation failed: {}", e);
                    // Rollback auth state since session is not usable
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

                // Step 6: Optionally sync audio settings after session activation.
                // Only runs if user has enabled sync_audio_on_startup (opt-in).
                // Useful when Player::new() may hold stale settings (e.g., Flatpak updates).
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
                                    log::info!("[Runtime] Syncing audio settings to player (sync_audio_on_startup=true)");
                                    let _ = player.reload_settings(convert_to_qbz_audio_settings(&fresh));
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("[Runtime] Auto-login failed: {}", e);
                // Not a fatal error - user can login manually
            }
        }
    } else if let Ok(Some(oauth_token)) = crate::credentials::load_oauth_token() {
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
                let cb_timeout = std::time::Duration::from_secs(10);
                loop {
                    if core_bridge.try_get().await.is_some() {
                        break;
                    }
                    if cb_start.elapsed() > cb_timeout {
                        log::error!("[Runtime] CoreBridge not available after 10s (OAuth restore)");
                        manager.set_bootstrap_in_progress(false).await;
                        return Err(RuntimeError::V2NotInitialized);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }

                if let Some(bridge) = core_bridge.try_get().await {
                    let core_session: qbz_models::UserSession = match serde_json::to_value(&session)
                        .and_then(serde_json::from_value)
                    {
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
                                    let _ = player.reload_settings(convert_to_qbz_audio_settings(&fresh));
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

    log::info!("[V2] Legacy auth successful");
    manager.set_legacy_auth(true, Some(session.user_id)).await;
    let _ = app.emit(
        "runtime:event",
        RuntimeEvent::AuthChanged {
            logged_in: true,
            user_id: Some(session.user_id),
        },
    );

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
                let _ = app.emit(
                    "runtime:event",
                    RuntimeEvent::CoreBridgeAuthFailed {
                        error: e.to_string(),
                    },
                );
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

    // Persist ToS acceptance now that login succeeded.
    // The frontend checkbox already gated the UI; we store the value so
    // subsequent bootstrap auto-logins pass without requiring the user to
    // re-accept (e.g. after a factory reset that wiped the DB).
    accept_tos_best_effort(&legal_state);

    // Emit ready event
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

    log::info!("[V2] Legacy auth successful");
    manager.set_legacy_auth(true, Some(session.user_id)).await;
    let _ = app.emit(
        "runtime:event",
        RuntimeEvent::AuthChanged {
            logged_in: true,
            user_id: Some(session.user_id),
        },
    );

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
                let _ = app.emit(
                    "runtime:event",
                    RuntimeEvent::CoreBridgeAuthFailed {
                        error: e.to_string(),
                    },
                );
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

    // Persist ToS acceptance now that login succeeded.
    accept_tos_best_effort(&legal_state);

    // Emit ready event
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
    let manager = runtime.manager();
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
    let code_holder: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(None));
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
        log::info!("[OAuth] New popup window requested: {} (label={})", url, label);

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

    // Wait up to 5 minutes for the user to complete login
    let timed_out = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        notify.notified(),
    )
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

    log::info!("[V2] OAuth code received, exchanging for session...");

    // Exchange code for UserSession via legacy client
    let session = {
        let client = app_state.client.read().await;
        match client.login_with_oauth_code(&code).await {
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

    log::info!("[V2] OAuth session established");
    manager.set_legacy_auth(true, Some(session.user_id)).await;
    let _ = app.emit(
        "runtime:event",
        RuntimeEvent::AuthChanged {
            logged_in: true,
            user_id: Some(session.user_id),
        },
    );

    // Convert api::models::UserSession → qbz_models::UserSession for CoreBridge
    // Both types are structurally identical; serde round-trip is safe.
    let core_session: UserSession = match serde_json::to_value(&session)
        .and_then(serde_json::from_value)
    {
        Ok(s) => s,
        Err(e) => {
            log::error!("[V2] Failed to convert session for CoreBridge: {}", e);
            rollback_auth_state(&manager, &app).await;
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

    // CoreBridge auth — inject session directly (OAuth has no email/password)
    if let Some(bridge) = core_bridge.try_get().await {
        match bridge.login_with_session(core_session).await {
            Ok(_) => {
                log::info!("[V2] CoreBridge session injected for OAuth user");
                manager.set_corebridge_auth(true).await;
            }
            Err(e) => {
                log::error!("[V2] CoreBridge session injection failed: {}", e);
                rollback_auth_state(&manager, &app).await;
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
        log::error!("[V2] CoreBridge not initialized for OAuth login");
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

    // Activate per-user session (same as manual login)
    if let Err(e) = crate::session_lifecycle::activate_session(&app, session.user_id).await {
        log::error!("[V2] Session activation failed after OAuth: {}", e);
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

    // Persist the OAuth token so bootstrap can restore the session on next launch.
    // Non-fatal: if saving fails, the user just has to re-login via OAuth.
    if let Err(e) = crate::credentials::save_oauth_token(&session.user_auth_token) {
        log::warn!("[V2] Failed to persist OAuth token: {}", e);
    }

    // Persist ToS acceptance now that login succeeded.
    accept_tos_best_effort(&legal_state);

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
    spawn_v2_prefetch_with_hw_check(bridge, cache, upcoming_tracks, quality, streaming_only, None);
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

            let result = async {
                let bridge_guard = bridge_clone.read().await;
                let bridge = bridge_guard.as_ref().ok_or("CoreBridge not initialized")?;
                let mut effective_quality = quality;

                // Smart quality downgrade: check if hardware supports the track's sample rate
                #[cfg(target_os = "linux")]
                if let Some(ref device_id) = hw_device_clone {
                    if quality == Quality::UltraHiRes {
                        let stream_url = bridge.get_stream_url(track_id, quality).await?;
                        let track_rate = (stream_url.sampling_rate * 1000.0) as u32;
                        if qbz_audio::device_supports_sample_rate(device_id, track_rate) == Some(false) {
                            log::info!(
                                "[V2/PREFETCH] Track {} at {}Hz incompatible with hardware, prefetching at Hi-Res",
                                track_id, track_rate
                            );
                            effective_quality = Quality::HiRes;
                        } else {
                            // Rate is supported, use the URL we already got
                            drop(bridge_guard);
                            let data = download_audio(&stream_url.url).await?;
                            return Ok::<Vec<u8>, String>(data);
                        }
                    }
                }

                let stream_url = bridge.get_stream_url(track_id, effective_quality).await?;
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
    let (sample_rate, bit_depth, is_playing) =
        if let Some(bridge) = core_bridge.try_get().await {
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
    let mut capabilities = DacCapabilities {
        node_name: nodeName.clone(),
        sample_rates: vec![44100, 48000, 88200, 96000, 176400, 192000],
        formats: vec![
            "S16LE".to_string(),
            "S24LE".to_string(),
            "F32LE".to_string(),
        ],
        channels: Some(2),
        description: None,
        error: None,
    };

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

// ==================== Link Resolver ====================

/// Result of resolving a cross-platform music link.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub enum MusicLinkResult {
    /// Successfully resolved to a Qobuz entity.
    Resolved {
        link: qbz_qobuz::ResolvedLink,
        provider: Option<String>,
    },
    /// The URL is a playlist — redirect to the Playlist Importer.
    PlaylistDetected {
        provider: String,
    },
    /// The content exists on the source platform but is not available on Qobuz.
    NotOnQobuz {
        provider: Option<String>,
    },
}

/// Resolve a cross-platform music link to a Qobuz navigation action.
///
/// Accepts URLs from Qobuz, Spotify, Apple Music, Tidal, Deezer, song.link, and album.link.
/// For non-Qobuz tracks/albums, uses the Odesli API to identify the content, then searches
/// Qobuz by title+artist to find the equivalent album.
/// For playlists, returns `PlaylistDetected` so the frontend can redirect to the importer.
#[tauri::command]
pub async fn v2_resolve_music_link(
    url: String,
    state: State<'_, AppState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<MusicLinkResult, RuntimeError> {
    use crate::playlist_import::providers::{detect_music_resource, MusicResource};

    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(RuntimeError::Internal("Empty URL".to_string()));
    }

    // 1. Try Qobuz native resolve first (sync, no network)
    if let Ok(resolved) = qbz_qobuz::resolve_link(&url) {
        return Ok(MusicLinkResult::Resolved {
            link: resolved,
            provider: None,
        });
    }

    // 2. Detect what kind of resource this is
    let resource = detect_music_resource(&url).ok_or_else(|| {
        RuntimeError::Internal("Unsupported or invalid music link".to_string())
    })?;

    match resource {
        MusicResource::Qobuz => {
            // Already handled above, but just in case
            let resolved = qbz_qobuz::resolve_link(&url)
                .map_err(|e| RuntimeError::Internal(e.to_string()))?;
            Ok(MusicLinkResult::Resolved {
                link: resolved,
                provider: None,
            })
        }

        MusicResource::Playlist { provider } => Ok(MusicLinkResult::PlaylistDetected {
            provider: format!("{:?}", provider),
        }),

        MusicResource::Track { provider, url: source_url } => {
            resolve_via_odesli_and_search(
                &state.songlink, &source_url, Some(&provider), true, &bridge, &runtime,
            ).await
        }

        MusicResource::Album { provider, url: source_url } => {
            resolve_via_odesli_and_search(
                &state.songlink, &source_url, Some(&provider), false, &bridge, &runtime,
            ).await
        }

        MusicResource::SongLink { url: source_url } => {
            // song.link URLs: try to detect track vs album from the URL format
            let is_track_hint = source_url.contains("song.link/");
            resolve_via_odesli_and_search(
                &state.songlink, &source_url, None, is_track_hint, &bridge, &runtime,
            ).await
        }
    }
}

/// Identify a cross-platform music URL and search Qobuz for the equivalent.
///
/// Fast path: for Spotify/Tidal/Deezer, calls the platform API directly to get
/// title+artist (~300ms). Fallback: uses Odesli API (~2-3s).
/// Then searches Qobuz with progressively simpler queries.
async fn resolve_via_odesli_and_search(
    songlink: &crate::share::SongLinkClient,
    url: &str,
    provider: Option<&crate::playlist_import::providers::MusicProvider>,
    is_track: bool,
    bridge: &State<'_, CoreBridgeState>,
    runtime: &State<'_, RuntimeManagerState>,
) -> Result<MusicLinkResult, RuntimeError> {
    let provider_name = provider.map(|p| format!("{:?}", p));

    // 1. Get title + artist: try direct platform API first (fast), fall back to Odesli
    let (title, artist) = if let Some(prov) = provider {
        match try_direct_platform_metadata(url, prov, is_track).await {
            Some(meta) => {
                log::info!("Link resolver: direct API resolved '{}' by '{}'", meta.0, meta.1);
                meta
            }
            None => {
                log::info!("Link resolver: direct API failed, falling back to Odesli");
                fetch_metadata_via_odesli(songlink, url).await?
            }
        }
    } else {
        // No provider (song.link URLs) — use Odesli
        fetch_metadata_via_odesli(songlink, url).await?
    };

    if title.is_empty() {
        return Ok(MusicLinkResult::NotOnQobuz {
            provider: provider_name,
        });
    }

    // 2. Search Qobuz with progressively simpler queries
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let bridge_guard = bridge.get().await;

    if let Some(result) = search_qobuz_smart(
        &*bridge_guard, &title, &artist, is_track, &provider_name,
    ).await? {
        return Ok(result);
    }

    log::info!("Link resolver: '{}' by '{}' not found on Qobuz", title, artist);
    Ok(MusicLinkResult::NotOnQobuz {
        provider: provider_name,
    })
}

/// Fetch metadata from Odesli API (with one retry for transient errors).
async fn fetch_metadata_via_odesli(
    songlink: &crate::share::SongLinkClient,
    url: &str,
) -> Result<(String, String), RuntimeError> {
    let response = match songlink
        .get_by_url(url, crate::share::ContentType::Track)
        .await
    {
        Ok(r) => r,
        Err(first_err) => {
            log::warn!("Link resolver: Odesli first attempt failed: {}, retrying...", first_err);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            songlink
                .get_by_url(url, crate::share::ContentType::Track)
                .await
                .map_err(|e| RuntimeError::Internal(format!("Odesli API error: {}", e)))?
        }
    };

    let title = response.title.unwrap_or_default().trim().to_string();
    let artist = response.artist.unwrap_or_default().trim().to_string();
    Ok((title, artist))
}

/// Search Qobuz with progressively simpler queries until a match is found.
///
/// Strategy:
/// 1. "title artist" (exact)
/// 2. "cleaned_title artist" (remove parenthetical/bracket suffixes)
/// 3. "artist" only with album search (broad)
async fn search_qobuz_smart(
    bridge: &crate::core_bridge::CoreBridge,
    title: &str,
    artist: &str,
    is_track: bool,
    provider_name: &Option<String>,
) -> Result<Option<MusicLinkResult>, RuntimeError> {
    let full_query = if artist.is_empty() {
        title.to_string()
    } else {
        format!("{} {}", title, artist)
    };

    // Attempt 1: full query
    if is_track {
        let results = bridge.search_tracks(&full_query, 5, 0, None).await.map_err(RuntimeError::Internal)?;
        if let Some(track) = results.items.first() {
            log::info!("Link resolver: found Qobuz track id={} (full query)", track.id);
            return Ok(Some(MusicLinkResult::Resolved {
                link: qbz_qobuz::ResolvedLink::OpenTrack(track.id),
                provider: provider_name.clone(),
            }));
        }
    }

    let results = bridge.search_albums(&full_query, 5, 0, None).await.map_err(RuntimeError::Internal)?;
    if let Some(album) = results.items.first() {
        log::info!("Link resolver: found Qobuz album id={} (full query)", album.id);
        return Ok(Some(MusicLinkResult::Resolved {
            link: qbz_qobuz::ResolvedLink::OpenAlbum(album.id.clone()),
            provider: provider_name.clone(),
        }));
    }

    // Attempt 2: clean title (remove parenthetical/bracket suffixes like "Remastered", "Deluxe")
    let cleaned = clean_title(title);
    if cleaned != title && !cleaned.is_empty() {
        let clean_query = if artist.is_empty() {
            cleaned.clone()
        } else {
            format!("{} {}", cleaned, artist)
        };

        log::info!("Link resolver: retrying with cleaned query '{}'", clean_query);
        let results = bridge.search_albums(&clean_query, 5, 0, None).await.map_err(RuntimeError::Internal)?;
        if let Some(album) = results.items.first() {
            log::info!("Link resolver: found Qobuz album id={} (cleaned query)", album.id);
            return Ok(Some(MusicLinkResult::Resolved {
                link: qbz_qobuz::ResolvedLink::OpenAlbum(album.id.clone()),
                provider: provider_name.clone(),
            }));
        }
    }

    // Attempt 3: search by artist name only (broad)
    if !artist.is_empty() && artist != title {
        log::info!("Link resolver: retrying with artist-only query '{}'", artist);
        let results = bridge.search_albums(artist, 10, 0, None).await.map_err(RuntimeError::Internal)?;
        let title_lower = title.to_ascii_lowercase();
        let cleaned_lower = clean_title(title).to_ascii_lowercase();
        for album in &results.items {
            let album_title_lower = album.title.to_ascii_lowercase();
            if album_title_lower.contains(&cleaned_lower)
                || cleaned_lower.contains(&album_title_lower)
                || album_title_lower.contains(&title_lower)
            {
                log::info!("Link resolver: found Qobuz album id={} (artist-only + title match)", album.id);
                return Ok(Some(MusicLinkResult::Resolved {
                    link: qbz_qobuz::ResolvedLink::OpenAlbum(album.id.clone()),
                    provider: provider_name.clone(),
                }));
            }
        }
    }

    Ok(None)
}

/// Remove parenthetical/bracket suffixes from a title.
/// "Senjutsu (2021 Remaster)" → "Senjutsu"
/// "The Number of the Beast [Deluxe Edition]" → "The Number of the Beast"
fn clean_title(title: &str) -> String {
    let mut result = title.to_string();
    // Remove trailing (...) and [...]
    while let Some(pos) = result.rfind('(') {
        if result[pos..].contains(')') {
            result = result[..pos].trim_end().to_string();
        } else {
            break;
        }
    }
    while let Some(pos) = result.rfind('[') {
        if result[pos..].contains(']') {
            result = result[..pos].trim_end().to_string();
        } else {
            break;
        }
    }
    result.trim().to_string()
}

// ── Direct platform metadata (bypass Odesli for speed) ──

const QBZ_PROXY_BASE: &str = "https://qbz-api-proxy.blitzkriegfc.workers.dev";

/// Try to get title+artist directly from the platform API.
/// Returns None if the platform isn't supported or the request fails.
async fn try_direct_platform_metadata(
    url: &str,
    provider: &crate::playlist_import::providers::MusicProvider,
    is_track: bool,
) -> Option<(String, String)> {
    use crate::playlist_import::providers::MusicProvider;

    match provider {
        MusicProvider::Deezer => try_deezer_metadata(url, is_track).await,
        MusicProvider::Spotify => try_spotify_metadata(url, is_track).await,
        MusicProvider::Tidal => try_tidal_metadata(url, is_track).await,
        MusicProvider::AppleMusic => None, // No direct API available
    }
}

/// Extract a numeric or alphanumeric ID after /track/ or /album/ in a URL.
fn extract_entity_id(url: &str, entity_type: &str) -> Option<String> {
    let pattern = format!("/{}/", entity_type);
    let idx = url.find(&pattern)?;
    let rest = &url[idx + pattern.len()..];
    let id = rest.split(['?', '/', '#']).next()?;
    if id.is_empty() { None } else { Some(id.to_string()) }
}

/// Extract Spotify ID from URL or URI.
fn extract_spotify_entity_id(url: &str, entity_type: &str) -> Option<String> {
    // URI format: spotify:track:abc123
    let uri_pattern = format!("spotify:{}:", entity_type);
    if let Some(rest) = url.strip_prefix(&uri_pattern) {
        let id = rest.split(['?', '/']).next()?;
        if !id.is_empty() { return Some(id.to_string()); }
    }
    extract_entity_id(url, entity_type)
}

async fn try_deezer_metadata(url: &str, is_track: bool) -> Option<(String, String)> {
    let entity = if is_track { "track" } else { "album" };
    let id = extract_entity_id(url, entity)
        .or_else(|| if is_track { None } else { extract_entity_id(url, "track") })?;
    let api_url = format!("https://api.deezer.com/{}/{}", entity, id);

    log::debug!("Link resolver: Deezer direct API: {}", api_url);
    let data: serde_json::Value = reqwest::get(&api_url).await.ok()?.json().await.ok()?;
    if data.get("error").is_some() { return None; }

    let title = data.get("title")?.as_str()?.to_string();
    let artist = data.get("artist")
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some((title, artist))
}

async fn try_spotify_metadata(url: &str, is_track: bool) -> Option<(String, String)> {
    let entity = if is_track { "track" } else { "album" };
    let id = extract_spotify_entity_id(url, entity)?;
    let token = get_proxy_token("spotify").await?;
    let api_url = format!("https://api.spotify.com/v1/{}s/{}", entity, id);

    log::debug!("Link resolver: Spotify direct API: {}", api_url);
    let data: serde_json::Value = reqwest::Client::new()
        .get(&api_url)
        .header("Authorization", format!("Bearer {}", token))
        .send().await.ok()?
        .json().await.ok()?;

    let title = data.get("name")?.as_str()?.to_string();
    let artist = data.get("artists")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some((title, artist))
}

async fn try_tidal_metadata(url: &str, is_track: bool) -> Option<(String, String)> {
    let entity = if is_track { "track" } else { "album" };
    let id = extract_entity_id(url, entity)
        // Also try /browse/track/ pattern
        .or_else(|| extract_entity_id(url, &format!("browse/{}", entity)))?;
    let token = get_proxy_token("tidal").await?;
    let api_url = format!(
        "https://openapi.tidal.com/v2/{}s/{}?countryCode=US&include=artists",
        entity, id
    );

    log::debug!("Link resolver: Tidal direct API: {}", api_url);
    let data: serde_json::Value = reqwest::Client::new()
        .get(&api_url)
        .header("Authorization", format!("Bearer {}", token))
        .send().await.ok()?
        .json().await.ok()?;

    let title = data.get("data")
        .and_then(|d| d.get("attributes"))
        .and_then(|a| a.get("title"))
        .and_then(|v| v.as_str())?
        .to_string();

    // Artist name is in the "included" array
    let artist = data.get("included")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|item|
            item.get("type").and_then(|v| v.as_str()) == Some("artists")
        ))
        .and_then(|item| item.get("attributes"))
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some((title, artist))
}

/// Get an OAuth token from the QBZ proxy for the given platform.
async fn get_proxy_token(platform: &str) -> Option<String> {
    let url = format!("{}/{}/token", QBZ_PROXY_BASE, platform);
    let data: serde_json::Value = reqwest::Client::builder()
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(reqwest::header::USER_AGENT, reqwest::header::HeaderValue::from_static("QBZ/1.0.0"));
            h
        })
        .build().ok()?
        .get(&url)
        .send().await.ok()?
        .json().await.ok()?;
    data.get("access_token")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
}

#[tauri::command]
pub fn v2_resolve_qobuz_link(url: String) -> Result<qbz_qobuz::ResolvedLink, RuntimeError> {
    qbz_qobuz::resolve_link(&url).map_err(|e| RuntimeError::Internal(e.to_string()))
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_get_qobuz_track_url(trackId: u64) -> Result<String, RuntimeError> {
    Ok(format!("https://play.qobuz.com/track/{}", trackId))
}

/// Known .desktop filenames across packaging formats.
const QBZ_DESKTOP_CANDIDATES: &[&str] = &[
    "com.blitzfc.qbz.desktop", // Tauri deb, Flatpak
    "qbz.desktop",             // Arch, AUR, Snap
    "qbz-nix.desktop",         // Possible alternative
];

/// Search standard directories for the installed QBZ .desktop file.
fn find_qbz_desktop_file() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let search_dirs = [
        "/usr/share/applications".to_string(),
        "/usr/local/share/applications".to_string(),
        format!("{}/.local/share/applications", home),
        "/var/lib/flatpak/exports/share/applications".to_string(),
        format!("{}/.local/share/flatpak/exports/share/applications", home),
    ];

    for candidate in QBZ_DESKTOP_CANDIDATES {
        for dir in &search_dirs {
            let path = format!("{}/{}", dir, candidate);
            if std::path::Path::new(&path).exists() {
                log::info!("[URI Handler] Found desktop file: {}", path);
                return candidate.to_string();
            }
        }
    }

    log::warn!("[URI Handler] No desktop file found in standard dirs, using default");
    "com.blitzfc.qbz.desktop".to_string()
}

/// Refresh the desktop MIME database so xdg-open picks up changes.
fn refresh_desktop_database() {
    // User-level applications dir
    if let Some(data_dir) = dirs::data_dir() {
        let user_apps = data_dir.join("applications");
        if user_apps.exists() {
            let _ = std::process::Command::new("update-desktop-database")
                .arg(&user_apps)
                .status();
        }
    }
    // System-level (may fail without root, that's OK)
    let _ = std::process::Command::new("update-desktop-database")
        .arg("/usr/share/applications")
        .status();
}

/// Check if QBZ is the default handler for qobuzapp:// links.
#[tauri::command]
pub fn v2_check_qobuzapp_handler() -> Result<bool, RuntimeError> {
    let output = std::process::Command::new("xdg-mime")
        .args(["query", "default", "x-scheme-handler/qobuzapp"])
        .output()
        .map_err(|e| RuntimeError::Internal(format!("Failed to run xdg-mime: {}", e)))?;

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(QBZ_DESKTOP_CANDIDATES.iter().any(|c| *c == result))
}

/// Register QBZ as the default handler for qobuzapp:// links.
#[tauri::command]
pub fn v2_register_qobuzapp_handler() -> Result<bool, RuntimeError> {
    let desktop_file = find_qbz_desktop_file();
    log::info!("[URI Handler] Registering {} for x-scheme-handler/qobuzapp", desktop_file);

    let status = std::process::Command::new("xdg-mime")
        .args(["default", &desktop_file, "x-scheme-handler/qobuzapp"])
        .status()
        .map_err(|e| RuntimeError::Internal(format!("Failed to run xdg-mime: {}", e)))?;

    if !status.success() {
        log::error!("[URI Handler] xdg-mime default failed");
        return Ok(false);
    }

    refresh_desktop_database();
    log::info!("[URI Handler] Registration complete, desktop database refreshed");
    Ok(true)
}

/// Remove QBZ as the default handler for qobuzapp:// links.
#[tauri::command]
pub fn v2_deregister_qobuzapp_handler() -> Result<bool, RuntimeError> {
    let mimeapps = dirs::config_dir()
        .ok_or_else(|| RuntimeError::Internal("No config dir found".to_string()))?
        .join("mimeapps.list");

    if !mimeapps.exists() {
        return Ok(true); // Nothing to remove
    }

    let content = std::fs::read_to_string(&mimeapps)
        .map_err(|e| RuntimeError::Internal(format!("Failed to read mimeapps.list: {}", e)))?;

    let filtered: String = content
        .lines()
        .filter(|line| !line.starts_with("x-scheme-handler/qobuzapp="))
        .collect::<Vec<_>>()
        .join("\n");

    // Preserve trailing newline if original had one
    let filtered = if content.ends_with('\n') && !filtered.ends_with('\n') {
        format!("{}\n", filtered)
    } else {
        filtered
    };

    std::fs::write(&mimeapps, filtered)
        .map_err(|e| RuntimeError::Internal(format!("Failed to write mimeapps.list: {}", e)))?;

    refresh_desktop_database();
    Ok(true)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_ping(baseUrl: String, token: String) -> Result<PlexServerInfo, String> {
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
pub fn v2_set_visualizer_enabled(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
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
pub fn v2_set_gsk_renderer(
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
    store.set_gsk_renderer(value)
}

#[tauri::command]
pub fn v2_clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    state.audio_cache.clear();
    Ok(())
}

#[tauri::command]
pub async fn v2_clear_artist_cache(cache_state: State<'_, ApiCacheState>) -> Result<usize, String> {
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
pub async fn v2_get_playlist_suggestions(
    input: V2PlaylistSuggestionsInput,
    store_state: State<'_, ArtistVectorStoreState>,
    musicbrainz: State<'_, crate::musicbrainz::MusicBrainzSharedState>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<crate::artist_vectors::SuggestionResult, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    if input.artists.is_empty() {
        return Ok(crate::artist_vectors::SuggestionResult {
            tracks: Vec::new(),
            source_artists: Vec::new(),
            playlist_artists_count: 0,
            similar_artists_count: 0,
        });
    }

    let mut resolved_artists: Vec<(String, String)> = Vec::new();
    let mut seen_mbids = std::collections::HashSet::new();

    for artist in &input.artists {
        let mbid_from_qobuz = if let Some(qobuz_id) = artist.qobuz_id {
            let qobuz_artist_name = {
                let client = app_state.client.read().await;
                match client.get_artist(qobuz_id, false).await {
                    Ok(qobuz_artist) => Some(qobuz_artist.name),
                    Err(err) => {
                        log::warn!(
                            "[V2/Suggestions] Failed to fetch Qobuz artist {} for MBID resolution: {}",
                            qobuz_id,
                            err
                        );
                        None
                    }
                }
            };

            if let Some(artist_name) = qobuz_artist_name {
                match musicbrainz.client.search_artist(&artist_name).await {
                    Ok(search) => search
                        .artists
                        .into_iter()
                        .find(|candidate| candidate.score.unwrap_or(0) >= 80)
                        .map(|candidate| candidate.id),
                    Err(err) => {
                        log::warn!(
                            "[V2/Suggestions] Failed Qobuz->MBID search for {} ({}): {}",
                            artist.name,
                            qobuz_id,
                            err
                        );
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        let resolved_mbid = if mbid_from_qobuz.is_some() {
            mbid_from_qobuz
        } else {
            match musicbrainz.client.search_artist(&artist.name).await {
                Ok(search) => search
                    .artists
                    .into_iter()
                    .find(|candidate| candidate.score.unwrap_or(0) >= 80)
                    .map(|candidate| candidate.id),
                Err(err) => {
                    log::warn!(
                        "[V2/Suggestions] Failed name->MBID resolution for {}: {}",
                        artist.name,
                        err
                    );
                    None
                }
            }
        };

        if let Some(mbid) = resolved_mbid {
            if seen_mbids.insert(mbid.clone()) {
                resolved_artists.push((mbid, artist.name.clone()));
            }
        }
    }

    if resolved_artists.is_empty() {
        log::warn!("[V2/Suggestions] No artists could be resolved to MusicBrainz IDs");
        return Ok(crate::artist_vectors::SuggestionResult {
            tracks: Vec::new(),
            source_artists: Vec::new(),
            playlist_artists_count: input.artists.len(),
            similar_artists_count: 0,
        });
    }

    let config = input.config.unwrap_or_default();
    let builder = std::sync::Arc::new(crate::artist_vectors::ArtistVectorBuilder::new(
        store_state.store.clone(),
        musicbrainz.client.clone(),
        musicbrainz.cache.clone(),
        app_state.client.clone(),
        crate::artist_vectors::RelationshipWeights::default(),
    ));

    let engine = crate::artist_vectors::SuggestionsEngine::new(
        store_state.store.clone(),
        builder,
        app_state.client.clone(),
        config,
    );

    let exclude_track_ids: std::collections::HashSet<u64> =
        input.exclude_track_ids.into_iter().collect();

    engine
        .generate_suggestions(&resolved_artists, &exclude_track_ids, input.include_reasons)
        .await
        .map_err(RuntimeError::Internal)
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
pub fn v2_clear_artist_blacklist(state: State<'_, BlacklistState>) -> Result<(), String> {
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
    crate::credentials::clear_qobuz_credentials()?;
    crate::credentials::clear_oauth_token()
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
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
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
        let server = server_guard
            .as_mut()
            .ok_or("Media server not initialized")?;
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
    state
        .chromecast
        .seek(position_secs)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_get_position(
    state: State<'_, CastState>,
) -> Result<crate::cast::CastPositionInfo, String> {
    state
        .chromecast
        .get_media_position()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_set_volume(volume: f32, state: State<'_, CastState>) -> Result<(), String> {
    state
        .chromecast
        .set_volume(volume)
        .map_err(|e| e.to_string())
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
        let server = server_guard
            .as_mut()
            .ok_or("Media server not initialized")?;
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
pub async fn v2_dlna_get_position(
    state: State<'_, DlnaState>,
) -> Result<crate::cast::DlnaPositionInfo, String> {
    let connection = state.connection.lock().await;
    let conn = connection
        .as_ref()
        .ok_or_else(|| "Not connected".to_string())?;
    conn.get_position_info().await.map_err(|e| e.to_string())
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
pub async fn v2_airplay_set_volume(
    volume: f32,
    state: State<'_, AirPlayState>,
) -> Result<(), String> {
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
    db.delete_tracks_in_folder(&path)
        .map_err(|e| e.to_string())?;
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
        tokio::task::spawn_blocking(move || {
            std::fs::read_dir(std::path::Path::new(&path_clone)).is_ok()
        }),
    )
    .await;

    match check_result {
        Ok(Ok(accessible)) => Ok(accessible),
        Ok(Err(_)) => {
            log::warn!(
                "[V2] Failed to spawn blocking task for folder check: {}",
                path
            );
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
    db.reorder_playlists(&playlist_ids)
        .map_err(|e| e.to_string())
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
    let cache = cache_opt
        .as_ref()
        .ok_or("No active session - please log in")?;
    cache.get_stats()
}

#[tauri::command]
pub async fn v2_musicbrainz_clear_cache(
    state: State<'_, MusicBrainzSharedState>,
) -> Result<(), String> {
    let cache_opt = state.cache.lock().await;
    let cache = cache_opt
        .as_ref()
        .ok_or("No active session - please log in")?;
    cache.clear_all()
}

#[tauri::command]
pub fn v2_set_show_partial_playlists(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_show_partial_playlists(enabled)
}

#[tauri::command]
pub fn v2_set_allow_cast_while_offline(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_allow_cast_while_offline(enabled)
}

#[tauri::command]
pub fn v2_set_allow_immediate_scrobbling(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_allow_immediate_scrobbling(enabled)
}

#[tauri::command]
pub fn v2_set_allow_accumulated_scrobbling(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_allow_accumulated_scrobbling(enabled)
}

#[tauri::command]
pub fn v2_set_show_network_folders_in_manual_offline(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
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
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.add_tracks_to_pending_playlist(pending_id, &qobuz_track_ids, &local_track_paths)
}

#[tauri::command]
pub fn v2_update_pending_playlist_qobuz_id(
    pending_id: i64,
    qobuz_playlist_id: u64,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.update_qobuz_playlist_id(pending_id, qobuz_playlist_id)
}

#[tauri::command]
pub fn v2_mark_pending_playlist_synced(
    pending_id: i64,
    qobuz_playlist_id: u64,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.mark_playlist_synced(pending_id, qobuz_playlist_id)
}

#[tauri::command]
pub fn v2_delete_pending_playlist(
    pending_id: i64,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.delete_pending_playlist(pending_id)
}

#[tauri::command]
pub fn v2_mark_scrobbles_sent(ids: Vec<i64>, state: State<'_, OfflineState>) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
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
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create cache directory: {}", e))?;
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
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
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
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_playlist_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_get_favorites(state: State<'_, LibraryState>) -> Result<Vec<u64>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_favorite_playlist_ids().map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_local_tracks_with_position(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistLocalTrack>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_playlist_settings(playlistId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_stats(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<Option<PlaylistStats>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_playlist_stats(playlistId).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_increment_play_count(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<PlaylistStats, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.increment_playlist_play_count(playlistId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_get_all_stats(
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistStats>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_playlist_stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_get_all_local_track_counts(
    state: State<'_, LibraryState>,
) -> Result<std::collections::HashMap<u64, u32>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_playlist_local_track_counts()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_get_playlist_folders(
    state: State<'_, LibraryState>,
) -> Result<Vec<crate::library::PlaylistFolder>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
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
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

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
pub async fn v2_library_get_folders(state: State<'_, LibraryState>) -> Result<Vec<String>, String> {
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
    crate::library::library_get_artists(exclude_network_folders, state, download_settings_state)
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
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresClientInit)
        .await
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
    db.update_folder_path(id, &new_path)
        .map_err(|e| e.to_string())?;

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

#[derive(Debug, Clone, serde::Serialize)]
pub struct CustomArtistImageResult {
    pub image_path: String,
    pub thumbnail_path: String,
}

#[tauri::command]
pub async fn v2_library_set_custom_artist_image(
    artist_name: String,
    custom_image_path: String,
    state: State<'_, LibraryState>,
) -> Result<CustomArtistImageResult, String> {
    let artwork_dir = get_artwork_cache_dir();
    let source = std::path::Path::new(&custom_image_path);
    if !source.exists() {
        return Err(format!(
            "Source image does not exist: {}",
            custom_image_path
        ));
    }

    // Validate extension
    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if !["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()) {
        return Err(format!(
            "Unsupported image format: {}. Use png, jpg, jpeg, or webp.",
            extension
        ));
    }

    // Generate filename using MD5 hash of artist name
    let mut hasher = Md5::new();
    hasher.update(artist_name.as_bytes());
    let artist_hash = format!("{:x}", hasher.finalize());
    let timestamp = chrono::Utc::now().timestamp();
    let filename = format!("artist_custom_{}_{}.jpg", artist_hash, timestamp);
    let dest_path = artwork_dir.join(&filename);

    // Decode, resize to max 1000x1000, save as JPEG
    let img = image::ImageReader::open(source)
        .map_err(|e| format!("Failed to open image: {}", e))?
        .decode()
        .map_err(|e| format!("Failed to decode image: {}", e))?;
    let resized = img.resize(1000, 1000, image::imageops::FilterType::Lanczos3);
    resized
        .save(&dest_path)
        .map_err(|e| format!("Failed to save resized image: {}", e))?;

    // Generate 500x500 thumbnail using qbz-library
    let thumbnail_path = thumbnails::generate_thumbnail(&dest_path)
        .map_err(|e| format!("Failed to generate thumbnail: {}", e))?;

    // Update database
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.cache_artist_image(
        &artist_name,
        None,
        "custom",
        Some(&dest_path.to_string_lossy()),
    )
    .map_err(|e| e.to_string())?;

    Ok(CustomArtistImageResult {
        image_path: dest_path.to_string_lossy().into_owned(),
        thumbnail_path: thumbnail_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn v2_library_remove_custom_artist_image(
    artist_name: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    // Get current info to find file paths to delete
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let info = db.get_artist_image(&artist_name).map_err(|e| e.to_string())?;

    if let Some(info) = info {
        // Delete custom image file if it exists
        if let Some(ref path) = info.custom_image_path {
            let p = std::path::Path::new(path);
            if p.exists() {
                // Also remove thumbnail
                if let Ok(thumb) = thumbnails::get_thumbnail_path(p) {
                    let _ = std::fs::remove_file(thumb);
                }
                let _ = std::fs::remove_file(p);
            }
        }

        // Reset to original image (clear custom_image_path, keep image_url)
        db.cache_artist_image(
            &artist_name,
            info.image_url.as_deref(),
            info.source.as_deref().unwrap_or("qobuz"),
            None,
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn v2_library_get_artist_image(
    artist_name: String,
    state: State<'_, LibraryState>,
) -> Result<Option<crate::library::ArtistImageInfo>, String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_artist_image(&artist_name).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_library_get_all_custom_artist_images(
    state: State<'_, LibraryState>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_all_custom_artist_images().map_err(|e| e.to_string())
}

// === Custom Album Covers ===

#[derive(Debug, Clone, serde::Serialize)]
pub struct CustomAlbumCoverResult {
    pub image_path: String,
    pub thumbnail_path: String,
}

#[tauri::command]
pub async fn v2_library_set_custom_album_cover(
    album_id: String,
    custom_image_path: String,
    state: State<'_, LibraryState>,
) -> Result<CustomAlbumCoverResult, String> {
    let artwork_dir = get_artwork_cache_dir();
    let source = std::path::Path::new(&custom_image_path);
    if !source.exists() {
        return Err(format!(
            "Source image does not exist: {}",
            custom_image_path
        ));
    }

    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if !["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()) {
        return Err(format!(
            "Unsupported image format: {}. Use png, jpg, jpeg, or webp.",
            extension
        ));
    }

    let mut hasher = Md5::new();
    hasher.update(album_id.as_bytes());
    let album_hash = format!("{:x}", hasher.finalize());
    let timestamp = chrono::Utc::now().timestamp();
    let filename = format!("album_custom_{}_{}.jpg", album_hash, timestamp);
    let dest_path = artwork_dir.join(&filename);

    let img = image::ImageReader::open(source)
        .map_err(|e| format!("Failed to open image: {}", e))?
        .decode()
        .map_err(|e| format!("Failed to decode image: {}", e))?;
    let resized = img.resize(1000, 1000, image::imageops::FilterType::Lanczos3);
    resized
        .save(&dest_path)
        .map_err(|e| format!("Failed to save resized image: {}", e))?;

    let thumbnail_path = thumbnails::generate_thumbnail(&dest_path)
        .map_err(|e| format!("Failed to generate thumbnail: {}", e))?;

    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.set_custom_album_cover(&album_id, &dest_path.to_string_lossy())
        .map_err(|e| e.to_string())?;

    Ok(CustomAlbumCoverResult {
        image_path: dest_path.to_string_lossy().into_owned(),
        thumbnail_path: thumbnail_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn v2_library_remove_custom_album_cover(
    album_id: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;

    let existing = db.get_custom_album_cover(&album_id).map_err(|e| e.to_string())?;
    if let Some(path) = existing {
        let p = std::path::Path::new(&path);
        if p.exists() {
            if let Ok(thumb) = thumbnails::get_thumbnail_path(p) {
                let _ = std::fs::remove_file(thumb);
            }
            let _ = std::fs::remove_file(p);
        }
    }

    db.remove_custom_album_cover(&album_id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn v2_library_get_all_custom_album_covers(
    state: State<'_, LibraryState>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_all_custom_album_covers().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_save_image_url_to_file(
    url: String,
    dest_path: String,
) -> Result<(), String> {
    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to download image: {}", e))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read image data: {}", e))?;
    std::fs::write(&dest_path, &bytes)
        .map_err(|e| format!("Failed to save image: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn v2_create_artist_radio(
    artist_id: u64,
    artist_name: String,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<String, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    let client = state.client.read().await.clone();

    let session_id = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
        let builder = crate::radio_engine::RadioPoolBuilder::new(
            &radio_db,
            &client,
            crate::radio_engine::BuildRadioOptions::default(),
        );
        let rt = tokio::runtime::Handle::current();
        let session = rt.block_on(builder.create_artist_radio(artist_id))?;
        Ok(session.id)
    })
    .await
    .map_err(|e| format!("Radio task failed: {}", e))??;

    let client = state.client.read().await;
    let track_ids = tokio::task::spawn_blocking({
        let session_id = session_id.clone();
        move || -> Result<Vec<u64>, String> {
            let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
            let radio_engine = crate::radio_engine::RadioEngine::new(radio_db);
            let mut ids = Vec::new();
            for _ in 0..60 {
                match radio_engine.next_track(&session_id) {
                    Ok(radio_track) => ids.push(radio_track.track_id),
                    Err(_) => break,
                }
            }
            Ok(ids.into_iter().take(50).collect())
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

    let queue_tracks: Vec<CoreQueueTrack> =
        tracks.iter().map(track_to_queue_track_from_api).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(0)).await;

    let queue_track_ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
    let context = PlaybackContext::new(
        ContextType::Radio,
        session_id.clone(),
        artist_name,
        ContentSource::Qobuz,
        queue_track_ids,
        0,
    );
    state.context.set_context(context);

    Ok(session_id)
}

#[tauri::command]
pub async fn v2_create_track_radio(
    track_id: u64,
    track_name: String,
    artist_id: u64,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<String, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

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

    let queue_tracks: Vec<CoreQueueTrack> =
        tracks.iter().map(track_to_queue_track_from_api).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(0)).await;

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

/// Create album radio using the Qobuz `/radio/album` API endpoint.
///
/// Unlike artist/track radio (which uses RadioPoolBuilder + RadioDb),
/// album radio calls the Qobuz API directly — the endpoint returns
/// recommended tracks in a single GET response.
#[tauri::command]
pub async fn v2_create_album_radio(
    album_id: String,
    album_name: String,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<String, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    let client = state.client.read().await.clone();
    let radio_response = client
        .get_radio_album(&album_id)
        .await
        .map_err(|e| format!("Failed to fetch album radio: {}", e))?;

    // The radio endpoint returns partial track objects (missing performer, etc.).
    // Extract IDs and fetch full track data individually, same as artist/track radio.
    let track_ids: Vec<u64> = radio_response.tracks.items.iter().map(|t| t.id).collect();

    if track_ids.is_empty() {
        return Err("No radio tracks returned for this album".to_string());
    }

    let mut tracks = Vec::new();
    for track_id in track_ids {
        if let Ok(track) = client.get_track(track_id).await {
            if let Some(ref performer) = track.performer {
                if blacklist_state.is_blacklisted(performer.id) {
                    continue;
                }
            }
            tracks.push(track);
        }
    }

    if tracks.is_empty() {
        return Err("Failed to fetch any radio tracks".to_string());
    }

    let queue_tracks: Vec<CoreQueueTrack> =
        tracks.iter().map(track_to_queue_track_from_api).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(0)).await;

    let queue_track_ids: Vec<u64> = tracks.iter().map(|track| track.id).collect();
    let context_id = format!("album_radio_{}_{}", album_id, chrono::Utc::now().timestamp());
    let context = PlaybackContext::new(
        ContextType::Radio,
        context_id.clone(),
        album_name,
        ContentSource::Qobuz,
        queue_track_ids,
        0,
    );
    state.context.set_context(context);

    Ok(context_id)
}

/// Create artist radio using the Qobuz `/radio/artist` API endpoint.
///
/// Like album radio, this calls the Qobuz API directly — the endpoint returns
/// recommended tracks in a single GET response.
#[tauri::command]
pub async fn v2_create_qobuz_artist_radio(
    artist_id: u64,
    artist_name: String,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<String, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    let client = state.client.read().await.clone();
    let radio_response = client
        .get_radio_artist(&artist_id.to_string())
        .await
        .map_err(|e| format!("Failed to fetch artist radio: {}", e))?;

    let track_ids: Vec<u64> = radio_response.tracks.items.iter().map(|track| track.id).collect();

    if track_ids.is_empty() {
        return Err("No radio tracks returned for this artist".to_string());
    }

    let mut tracks = Vec::new();
    for track_id in track_ids {
        if let Ok(track) = client.get_track(track_id).await {
            if let Some(ref performer) = track.performer {
                if blacklist_state.is_blacklisted(performer.id) {
                    continue;
                }
            }
            tracks.push(track);
        }
    }

    if tracks.is_empty() {
        return Err("Failed to fetch any radio tracks".to_string());
    }

    let queue_tracks: Vec<CoreQueueTrack> =
        tracks.iter().map(track_to_queue_track_from_api).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(0)).await;

    let queue_track_ids: Vec<u64> = tracks.iter().map(|track| track.id).collect();
    let context_id = format!("qobuz_artist_radio_{}_{}", artist_id, chrono::Utc::now().timestamp());
    let context = PlaybackContext::new(
        ContextType::Radio,
        context_id.clone(),
        artist_name,
        ContentSource::Qobuz,
        queue_track_ids,
        0,
    );
    state.context.set_context(context);

    Ok(context_id)
}

/// Create track radio using the Qobuz `/radio/track` API endpoint.
///
/// Like album radio, this calls the Qobuz API directly — the endpoint returns
/// recommended tracks in a single GET response.
#[tauri::command]
pub async fn v2_create_qobuz_track_radio(
    track_id: u64,
    track_name: String,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<String, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    let client = state.client.read().await.clone();
    let radio_response = client
        .get_radio_track(&track_id.to_string())
        .await
        .map_err(|e| format!("Failed to fetch track radio: {}", e))?;

    let fetched_ids: Vec<u64> = radio_response.tracks.items.iter().map(|track| track.id).collect();

    if fetched_ids.is_empty() {
        return Err("No radio tracks returned for this track".to_string());
    }

    let mut tracks = Vec::new();
    for next_id in fetched_ids {
        if let Ok(track) = client.get_track(next_id).await {
            if let Some(ref performer) = track.performer {
                if blacklist_state.is_blacklisted(performer.id) {
                    continue;
                }
            }
            tracks.push(track);
        }
    }

    if tracks.is_empty() {
        return Err("Failed to fetch any radio tracks".to_string());
    }

    let queue_tracks: Vec<CoreQueueTrack> =
        tracks.iter().map(track_to_queue_track_from_api).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(0)).await;

    let queue_track_ids: Vec<u64> = tracks.iter().map(|track| track.id).collect();
    let context_id = format!("qobuz_track_radio_{}_{}", track_id, chrono::Utc::now().timestamp());
    let context = PlaybackContext::new(
        ContextType::Radio,
        context_id.clone(),
        track_name,
        ContentSource::Qobuz,
        queue_track_ids,
        0,
    );
    state.context.set_context(context);

    Ok(context_id)
}

fn track_to_queue_track_from_api(track: &crate::api::Track) -> CoreQueueTrack {
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|a| a.image.large.clone())
        .or_else(|| track.album.as_ref().and_then(|a| a.image.thumbnail.clone()))
        .or_else(|| track.album.as_ref().and_then(|a| a.image.small.clone()));
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

    CoreQueueTrack {
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
    Ok(AllQueueTracksResponse { tracks, current_index })
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
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
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
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
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
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
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
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
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
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
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
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
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
    runtime: State<'_, RuntimeManagerState>,
) -> Result<bool, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
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
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<V2SearchAllResults, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

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
        .check_requirements(CommandRequirement::RequiresClientInit)
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
        .check_requirements(CommandRequirement::RequiresClientInit)
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
        .check_requirements(CommandRequirement::RequiresClientInit)
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
        .check_requirements(CommandRequirement::RequiresClientInit)
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
#[tauri::command]
pub async fn v2_set_volume(
    volume: f32,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresClientInit)
        .await?;
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
) -> Result<bool, RuntimeError> {
    log::info!("[V2] Command: play_next_gapless for track {}", track_id);

    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();
    let current_track_id = player.state.current_track_id();

    // Defensive guard: never queue the currently playing track as "next".
    // This avoids infinite one-track loops when frontend queue state is stale.
    if current_track_id != 0 && current_track_id == track_id {
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

    // Check streaming_only setting
    let streaming_only = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
        guard
            .as_ref()
            .and_then(|s| s.get_settings().ok())
            .map(|s| s.streaming_only)
            .unwrap_or(false)
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

                // Check if cached audio is compatible with hardware
                #[cfg(target_os = "linux")]
                if cached_audio_incompatible_with_hw(&audio_data, &audio_settings) {
                    log::info!(
                        "[V2/Quality] Skipping OFFLINE cache for track {} - incompatible sample rate",
                        track_id
                    );
                    // Fall through to network path which will request compatible quality
                } else {
                    player
                        .play_data(audio_data, track_id)
                        .map_err(RuntimeError::Internal)?;

                    // Prefetch next tracks in background (using CoreBridge queue)
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

                #[cfg(not(target_os = "linux"))]
                {
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

    // Check memory cache (L1) - using AppState's audio_cache for now
    // TODO: Move cache to qbz-core in future refactor
    let cache = app_state.audio_cache.clone();
    if let Some(cached) = cache.get(track_id) {
        log::info!(
            "[V2/CACHE HIT] Track {} from MEMORY cache ({} bytes)",
            track_id,
            cached.size_bytes
        );

        // Check if cached audio is compatible with hardware
        #[cfg(target_os = "linux")]
        let skip_cache = cached_audio_incompatible_with_hw(&cached.data, &audio_settings);
        #[cfg(not(target_os = "linux"))]
        let skip_cache = false;

        if skip_cache {
            log::info!(
                "[V2/Quality] Skipping MEMORY cache for track {} - incompatible sample rate",
                track_id
            );
            // Fall through to network path which will request compatible quality
        } else {
            player
                .play_data(cached.data, track_id)
                .map_err(RuntimeError::Internal)?;

            // Prefetch next tracks in background (using CoreBridge queue)
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

    // Check playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!(
                "[V2/CACHE HIT] Track {} from DISK cache ({} bytes)",
                track_id,
                audio_data.len()
            );

            // Check if cached audio is compatible with hardware
            #[cfg(target_os = "linux")]
            let skip_cache = cached_audio_incompatible_with_hw(&audio_data, &audio_settings);
            #[cfg(not(target_os = "linux"))]
            let skip_cache = false;

            if skip_cache {
                log::info!(
                    "[V2/Quality] Skipping DISK cache for track {} - incompatible sample rate",
                    track_id
                );
                // Fall through to network path which will request compatible quality
            } else {
                cache.insert(track_id, audio_data.clone());
                player
                    .play_data(audio_data, track_id)
                    .map_err(RuntimeError::Internal)?;

                // Prefetch next tracks in background (using CoreBridge queue)
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

    // Not in any cache - get stream URL from Qobuz via CoreBridge
    log::info!(
        "[V2] Track {} not in cache, fetching from network...",
        track_id
    );

    let mut stream_url = bridge_guard
        .get_stream_url(track_id, final_quality)
        .await
        .map_err(RuntimeError::Internal)?;
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

    // Download the audio
    let audio_data = download_audio(&stream_url.url)
        .await
        .map_err(RuntimeError::Internal)?;
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

    log::info!("[V2] get_playlist: {}", playlistId);
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
pub fn v2_set_audio_output_device(
    device: Option<String>,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    let normalized_device = device
        .as_ref()
        .map(|d| crate::audio::normalize_device_id_to_stable(d));
    log::info!(
        "[V2] set_audio_output_device {:?} -> {:?} (normalized)",
        device,
        normalized_device
    );
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_output_device(normalized_device.as_deref())
        .map_err(RuntimeError::Internal)
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
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_exclusive_mode(enabled)
        .map_err(RuntimeError::Internal)
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
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_dac_passthrough(enabled)
        .map_err(RuntimeError::Internal)
}

/// Set PipeWire force bit-perfect mode (V2)
#[tauri::command]
pub fn v2_set_audio_pw_force_bitperfect(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_pw_force_bitperfect: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_pw_force_bitperfect(enabled)
        .map_err(RuntimeError::Internal)
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
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
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
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_backend_type(backendType)
        .map_err(RuntimeError::Internal)
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
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_alsa_plugin(plugin)
        .map_err(RuntimeError::Internal)
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
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_gapless_enabled(enabled)
        .map_err(RuntimeError::Internal)
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
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_normalization_enabled(enabled)
        .map_err(RuntimeError::Internal)
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
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_normalization_target_lufs(target)
        .map_err(RuntimeError::Internal)
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
pub fn v2_reset_audio_settings(state: State<'_, AudioSettingsState>) -> Result<(), RuntimeError> {
    log::info!("[V2] reset_audio_settings");
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
        .map_err(RuntimeError::Internal)
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
pub fn v2_set_audio_alsa_hardware_volume(
    enabled: bool,
    state: State<'_, AudioSettingsState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] set_audio_alsa_hardware_volume: {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or(RuntimeError::UserSessionNotActivated)?;
    store
        .set_alsa_hardware_volume(enabled)
        .map_err(RuntimeError::Internal)
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

// ==================== Integrations V2 Commands ====================
//
// These commands use the qbz-integrations crate which is Tauri-independent.
// They can work without Tauri for TUI/headless clients.

use crate::integrations_v2::{LastFmV2State, ListenBrainzV2State, MusicBrainzV2State};

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
    legacy: State<'_, crate::listenbrainz::ListenBrainzSharedState>,
) -> Result<qbz_integrations::listenbrainz::UserInfo, RuntimeError> {
    log::info!("[V2] listenbrainz_connect");
    let client = state.client.lock().await;
    let user_info = client
        .set_token(&token)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))?;

    // Save credentials for persistence (in-memory V2 state)
    drop(client);
    state
        .save_credentials(token.clone(), user_info.user_name.clone())
        .await;

    // Persist to legacy SQLite cache so credentials survive restarts
    {
        let cache_guard = legacy.cache.lock().await;
        if let Some(cache) = cache_guard.as_ref() {
            if let Err(err) = cache.set_credentials(Some(&token), Some(&user_info.user_name)) {
                log::warn!("Failed to persist ListenBrainz credentials: {}", err);
            }
        }
        // Also update legacy in-memory client
        let legacy_client = legacy.client.lock().await;
        legacy_client.restore_token(token, user_info.user_name.clone()).await;
    }

    Ok(user_info)
}

/// Disconnect from ListenBrainz (V2)
#[tauri::command]
pub async fn v2_listenbrainz_disconnect(
    state: State<'_, ListenBrainzV2State>,
    legacy: State<'_, crate::listenbrainz::ListenBrainzSharedState>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] listenbrainz_disconnect");
    let client = state.client.lock().await;
    client.disconnect().await;
    drop(client);
    state.clear_credentials().await;

    // Clear from legacy SQLite cache
    {
        let cache_guard = legacy.cache.lock().await;
        if let Some(cache) = cache_guard.as_ref() {
            if let Err(err) = cache.clear_credentials() {
                log::warn!("Failed to clear ListenBrainz credentials: {}", err);
            }
        }
        legacy.client.lock().await.disconnect().await;
    }

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
    let additional_info = if recording_mbid.is_some()
        || release_mbid.is_some()
        || artist_mbids.is_some()
        || isrc.is_some()
        || duration_ms.is_some()
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
    client
        .submit_playing_now(&artist, &track, album.as_deref(), additional_info)
        .await
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
    let additional_info = if recording_mbid.is_some()
        || release_mbid.is_some()
        || artist_mbids.is_some()
        || isrc.is_some()
        || duration_ms.is_some()
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
    client
        .submit_listen(
            &artist,
            &track,
            album.as_deref(),
            timestamp,
            additional_info,
        )
        .await
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
    client
        .resolve_track(&artist, &title, isrc.as_deref())
        .await
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
    client
        .resolve_artist(&name)
        .await
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
pub async fn v2_remote_metadata_search(
    provider: String,
    query: String,
    artist: Option<String>,
    limit: Option<usize>,
    musicbrainz_state: State<'_, MusicBrainzSharedState>,
) -> Result<Vec<crate::library::remote_metadata::RemoteAlbumSearchResult>, RuntimeError> {
    use crate::library::remote_metadata::{
        discogs_extended_to_search_result, musicbrainz_release_to_search_result, RemoteProvider,
    };
    let provider = provider
        .parse::<RemoteProvider>()
        .map_err(RuntimeError::Internal)?;
    let max = limit.unwrap_or(10).clamp(1, 25);

    match provider {
        RemoteProvider::MusicBrainz => {
            let response = musicbrainz_state
                .client
                .search_releases_extended(&query, artist.as_deref().unwrap_or(""), None, max)
                .await
                .map_err(RuntimeError::Internal)?;
            Ok(response
                .releases
                .iter()
                .map(musicbrainz_release_to_search_result)
                .collect())
        }
        RemoteProvider::Discogs => {
            let client = crate::discogs::DiscogsClient::new();
            let results = client
                .search_releases(artist.as_deref().unwrap_or(""), &query, None, max)
                .await
                .map_err(RuntimeError::Internal)?;
            Ok(results
                .iter()
                .map(discogs_extended_to_search_result)
                .collect())
        }
    }
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_remote_metadata_get_album(
    provider: String,
    providerId: String,
    musicbrainz_state: State<'_, MusicBrainzSharedState>,
) -> Result<crate::library::remote_metadata::RemoteAlbumMetadata, RuntimeError> {
    use crate::library::remote_metadata::{
        discogs_full_to_metadata, musicbrainz_full_to_metadata, RemoteProvider,
    };

    let provider = provider
        .parse::<RemoteProvider>()
        .map_err(RuntimeError::Internal)?;

    match provider {
        RemoteProvider::MusicBrainz => {
            let full = musicbrainz_state
                .client
                .get_release_with_tracks(&providerId)
                .await
                .map_err(RuntimeError::Internal)?;
            Ok(musicbrainz_full_to_metadata(&full))
        }
        RemoteProvider::Discogs => {
            let id = providerId.parse::<u64>().map_err(|e| {
                RuntimeError::Internal(format!("Invalid Discogs release id: {}", e))
            })?;
            let client = crate::discogs::DiscogsClient::new();
            let full = client
                .get_release_metadata(id)
                .await
                .map_err(RuntimeError::Internal)?;
            Ok(discogs_full_to_metadata(&full))
        }
    }
}

#[tauri::command]
pub async fn v2_musicbrainz_get_artist_relationships(
    mbid: String,
    state: State<'_, MusicBrainzSharedState>,
) -> Result<crate::musicbrainz::ArtistRelationships, String> {
    // Check cache first
    {
        let cache_opt = state.cache.lock().await;
        let cache = cache_opt
            .as_ref()
            .ok_or("No active session - please log in")?;
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
                role: relation
                    .attributes
                    .as_ref()
                    .and_then(|a| a.first().cloned()),
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
        let cache = cache_opt
            .as_ref()
            .ok_or("No active session - please log in")?;
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
    let (token, auth_url) = client
        .get_token()
        .await
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

    let token = state
        .take_pending_token()
        .await
        .ok_or_else(|| RuntimeError::Internal("No pending auth token".to_string()))?;

    let mut client = state.client.lock().await;
    let session = client
        .get_session(&token)
        .await
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
pub async fn v2_lastfm_disconnect(state: State<'_, LastFmV2State>) -> Result<(), RuntimeError> {
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
    client
        .update_now_playing(&artist, &track, album.as_deref())
        .await
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
    client
        .scrobble(&artist, &track, album.as_deref(), timestamp)
        .await
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
    let cache = cache_guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;

    cache
        .queue_listen(
            timestamp,
            &artist,
            &track,
            album.as_deref(),
            recording_mbid.as_deref(),
            release_mbid.as_deref(),
            artist_mbids.as_deref(),
            isrc.as_deref(),
            duration_ms,
        )
        .map_err(|e| RuntimeError::Internal(e))
}

#[tauri::command]
pub async fn v2_listenbrainz_flush_queue(
    state: State<'_, ListenBrainzV2State>,
    legacy_state: State<'_, crate::listenbrainz::ListenBrainzSharedState>,
) -> Result<u32, RuntimeError> {
    let queued = {
        let cache_guard = legacy_state.cache.lock().await;
        let cache = cache_guard.as_ref().ok_or_else(|| {
            RuntimeError::Internal("No active session - please log in".to_string())
        })?;
        cache
            .get_queued_listens(500)
            .map_err(RuntimeError::Internal)?
    };

    if queued.is_empty() {
        return Ok(0);
    }

    let client = state.client.lock().await;
    let mut sent_ids = Vec::new();

    for listen in &queued {
        let additional_info = qbz_integrations::listenbrainz::AdditionalInfo {
            recording_mbid: listen.recording_mbid.clone(),
            release_mbid: listen.release_mbid.clone(),
            artist_mbids: listen.artist_mbids.clone(),
            isrc: listen.isrc.clone(),
            duration_ms: listen.duration_ms,
            tracknumber: None,
            media_player: "QBZ".to_string(),
            media_player_version: env!("CARGO_PKG_VERSION").to_string(),
            submission_client: "QBZ".to_string(),
            submission_client_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        if client
            .submit_listen(
                &listen.artist_name,
                &listen.track_name,
                listen.release_name.as_deref(),
                listen.listened_at,
                Some(additional_info),
            )
            .await
            .is_ok()
        {
            sent_ids.push(listen.id);
        }
    }
    drop(client);

    if sent_ids.is_empty() {
        return Ok(0);
    }

    let cache_guard = legacy_state.cache.lock().await;
    let cache = cache_guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
    cache
        .mark_listens_sent(&sent_ids)
        .map_err(RuntimeError::Internal)?;

    Ok(sent_ids.len() as u32)
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
    log::info!(
        "Command: v2_show_track_notification - {} by {}",
        title,
        artist
    );

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
    notification
        .summary(&title)
        .body(&lines.join("\n"))
        .appname("QBZ")
        .timeout(4000);

    if let Some(url) = artwork_url {
        if let Ok(path) = v2_cache_notification_artwork(&url) {
            if let Some(path_str) = path.to_str() {
                notification.image_path(path_str);
            }
        }
    }

    if let Err(e) = notification.show() {
        log::warn!(
            "Could not show notification (notification system may be unavailable): {}",
            e
        );
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

    let attribution = format!(
        "\n\n---\nOriginally curated by {} on Qobuz",
        source.owner.name
    );
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
#[allow(non_snake_case)]
pub async fn v2_share_track_songlink(
    isrc: Option<String>,
    url: String,
    trackId: Option<u64>,
    state: State<'_, AppState>,
) -> Result<crate::share::SongLinkResponse, RuntimeError> {
    if let Some(code) = isrc.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        return state
            .songlink
            .get_by_isrc(code)
            .await
            .map_err(|e| RuntimeError::Internal(e.to_string()));
    }

    let fallback_url = if let Some(id) = trackId {
        format!("https://play.qobuz.com/track/{}", id)
    } else {
        url
    };
    state
        .songlink
        .get_by_url(&fallback_url, crate::share::ContentType::Track)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_share_album_songlink(
    upc: Option<String>,
    albumId: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    state: State<'_, AppState>,
) -> Result<crate::share::SongLinkResponse, RuntimeError> {
    let _ = (title, artist);
    if let Some(code) = upc.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        return state
            .songlink
            .get_by_upc(code)
            .await
            .map_err(|e| RuntimeError::Internal(e.to_string()));
    }

    let fallback_url = albumId
        .map(|id| format!("https://play.qobuz.com/album/{}", id))
        .ok_or_else(|| {
            RuntimeError::Internal("Missing UPC/albumId for song.link album lookup".to_string())
        })?;
    state
        .songlink
        .get_by_url(&fallback_url, crate::share::ContentType::Album)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn v2_library_backfill_downloads(
    state: State<'_, LibraryState>,
    offline_cache_state: State<'_, crate::offline_cache::OfflineCacheState>,
) -> Result<crate::library::commands::BackfillReport, RuntimeError> {
    crate::library::commands::library_backfill_downloads(state, offline_cache_state)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_lyrics_get(
    trackId: Option<u64>,
    title: String,
    artist: String,
    album: Option<String>,
    durationSecs: Option<u64>,
    state: State<'_, LyricsState>,
) -> Result<Option<crate::lyrics::LyricsPayload>, RuntimeError> {
    crate::lyrics::commands::lyrics_get(trackId, title, artist, album, durationSecs, state)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_create_pending_playlist(
    name: String,
    description: Option<String>,
    isPublic: bool,
    trackIds: Vec<u64>,
    localTrackPaths: Vec<String>,
    state: State<'_, OfflineState>,
) -> Result<i64, RuntimeError> {
    crate::offline::commands::create_pending_playlist(
        name,
        description,
        isPublic,
        trackIds,
        localTrackPaths,
        state,
    )
    .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub fn v2_get_pending_playlist_count(state: State<'_, OfflineState>) -> Result<u32, RuntimeError> {
    crate::offline::commands::get_pending_playlist_count(state).map_err(RuntimeError::Internal)
}

#[tauri::command]
pub fn v2_queue_scrobble(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: i64,
    state: State<'_, OfflineState>,
) -> Result<i64, RuntimeError> {
    crate::offline::commands::queue_scrobble(artist, track, album, timestamp, state)
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_get_queued_scrobbles(
    limit: Option<u32>,
    state: State<'_, OfflineState>,
) -> Result<Vec<crate::offline::QueuedScrobble>, RuntimeError> {
    crate::offline::commands::get_queued_scrobbles(limit, state).map_err(RuntimeError::Internal)
}

#[tauri::command]
pub fn v2_get_queued_scrobble_count(state: State<'_, OfflineState>) -> Result<u32, RuntimeError> {
    crate::offline::commands::get_queued_scrobble_count(state).map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_cleanup_sent_scrobbles(
    olderThanDays: Option<u32>,
    state: State<'_, OfflineState>,
) -> Result<u32, RuntimeError> {
    crate::offline::commands::cleanup_sent_scrobbles(olderThanDays, state)
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_track_by_path(
    filePath: String,
    state: State<'_, LibraryState>,
) -> Result<Option<LocalTrack>, RuntimeError> {
    crate::library::commands::get_track_by_path(filePath, state)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_check_network_path(
    path: String,
) -> Result<crate::network::NetworkPathInfo, RuntimeError> {
    Ok(crate::network::commands::check_network_path(path))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_library_update_folder_settings(
    id: i64,
    alias: Option<String>,
    enabled: bool,
    isNetwork: bool,
    networkFsType: Option<String>,
    userOverrideNetwork: bool,
    state: State<'_, LibraryState>,
) -> Result<crate::library::LibraryFolder, RuntimeError> {
    crate::library::commands::library_update_folder_settings(
        id,
        alias,
        enabled,
        isNetwork,
        networkFsType,
        userOverrideNetwork,
        state,
    )
    .await
    .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_discogs_has_credentials() -> Result<bool, RuntimeError> {
    crate::library::commands::discogs_has_credentials()
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_discogs_search_artwork(
    artist: String,
    album: String,
    catalogNumber: Option<String>,
) -> Result<Vec<crate::discogs::DiscogsImageOption>, RuntimeError> {
    crate::library::commands::discogs_search_artwork(artist, album, catalogNumber)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_discogs_download_artwork(
    imageUrl: String,
    artist: String,
    album: String,
) -> Result<String, RuntimeError> {
    crate::library::commands::discogs_download_artwork(imageUrl, artist, album)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_check_album_fully_cached(
    albumId: String,
    cache_state: State<'_, crate::offline_cache::OfflineCacheState>,
) -> Result<bool, RuntimeError> {
    crate::offline_cache::commands::check_album_fully_cached(albumId, cache_state)
        .await
        .map_err(RuntimeError::Internal)
}

/// Shared helper: spawn the download task for a single track.
/// Used by both v2_cache_track_for_offline (single) and v2_cache_tracks_batch_for_offline (batch).
fn spawn_track_cache_download(
    track_id: u64,
    file_path: std::path::PathBuf,
    client: std::sync::Arc<tokio::sync::RwLock<crate::api::QobuzClient>>,
    fetcher: std::sync::Arc<crate::offline_cache::StreamFetcher>,
    db: std::sync::Arc<tokio::sync::Mutex<Option<crate::offline_cache::OfflineCacheDb>>>,
    offline_root: String,
    library_db: std::sync::Arc<tokio::sync::Mutex<Option<qbz_library::LibraryDatabase>>>,
    app: tauri::AppHandle,
    semaphore: std::sync::Arc<tokio::sync::Semaphore>,
) {
    tokio::spawn(async move {
        let _permit = match semaphore.acquire_owned().await {
            Ok(permit) => permit,
            Err(err) => {
                log::error!(
                    "Failed to acquire cache slot for track {}: {}",
                    track_id,
                    err
                );
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(
                        track_id,
                        crate::offline_cache::OfflineCacheStatus::Failed,
                        Some("Failed to start caching"),
                    );
                }
                let _ = app.emit(
                    "offline:caching_failed",
                    serde_json::json!({
                        "trackId": track_id,
                        "error": "Failed to acquire cache slot"
                    }),
                );
                return;
            }
        };

        if let Some(db_guard) = db.lock().await.as_ref() {
            let _ = db_guard.update_status(
                track_id,
                crate::offline_cache::OfflineCacheStatus::Downloading,
                None,
            );
        }
        let _ = app.emit(
            "offline:caching_started",
            serde_json::json!({ "trackId": track_id }),
        );

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
                let _ = app.emit(
                    "offline:caching_failed",
                    serde_json::json!({
                        "trackId": track_id,
                        "error": e.to_string()
                    }),
                );
                return;
            }
        };

        match fetcher
            .fetch_to_file(&url, &file_path, track_id, Some(&app))
            .await
        {
            Ok(size) => {
                log::info!("Caching complete for track {}: {} bytes", track_id, size);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.mark_complete(track_id, size);
                }
                let _ = app.emit(
                    "offline:caching_completed",
                    serde_json::json!({
                        "trackId": track_id,
                        "size": size
                    }),
                );

                // Post-processing kept in V2 to avoid command->command delegation.
                let file_path_str = file_path.to_string_lossy().to_string();
                let qobuz_client = client.read().await;
                let metadata = match crate::offline_cache::metadata::fetch_complete_metadata(
                    track_id,
                    &*qobuz_client,
                )
                .await
                {
                    Ok(m) => m,
                    Err(e) => {
                        log::warn!(
                            "Post-processing metadata fetch failed for {}: {}",
                            track_id,
                            e
                        );
                        return;
                    }
                };

                if let Err(e) =
                    crate::offline_cache::metadata::write_flac_tags(&file_path_str, &metadata)
                {
                    log::warn!("Failed to write tags for {}: {}", track_id, e);
                }
                if let Some(artwork_url) = &metadata.artwork_url {
                    if let Err(e) =
                        crate::offline_cache::metadata::embed_artwork(&file_path_str, artwork_url)
                            .await
                    {
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
                        let _ = crate::offline_cache::metadata::save_album_artwork(
                            parent_dir,
                            artwork_url,
                        )
                        .await;
                    }
                }

                let (bit_depth_detected, sample_rate_detected) =
                    match lofty::read_from_path(&new_path) {
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

                let _ = app.emit(
                    "offline:caching_processed",
                    serde_json::json!({
                        "trackId": track_id,
                        "path": new_path
                    }),
                );
            }
            Err(e) => {
                log::error!("Caching failed for track {}: {}", track_id, e);
                if let Some(db_guard) = db.lock().await.as_ref() {
                    let _ = db_guard.update_status(
                        track_id,
                        crate::offline_cache::OfflineCacheStatus::Failed,
                        Some(&e),
                    );
                }
                let _ = app.emit(
                    "offline:caching_failed",
                    serde_json::json!({
                        "trackId": track_id,
                        "error": e
                    }),
                );
            }
        }
    });
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
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    log::info!(
        "Command: v2_cache_track_for_offline {} - {} by {}",
        track_id,
        title,
        artist
    );

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

    spawn_track_cache_download(
        track_id,
        file_path,
        state.client.clone(),
        cache_state.fetcher.clone(),
        cache_state.db.clone(),
        cache_state.get_cache_path(),
        cache_state.library_db.clone(),
        app_handle.clone(),
        cache_state.cache_semaphore.clone(),
    );

    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTrackInfo {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub duration_secs: u64,
    pub quality: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
}

#[tauri::command]
pub async fn v2_cache_tracks_batch_for_offline(
    tracks: Vec<BatchTrackInfo>,
    state: State<'_, AppState>,
    cache_state: State<'_, OfflineCacheState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    log::info!(
        "Command: v2_cache_tracks_batch_for_offline ({} tracks)",
        tracks.len()
    );

    // Build TrackCacheInfo + file_path pairs for batch insert
    let mut batch: Vec<(crate::offline_cache::TrackCacheInfo, String)> =
        Vec::with_capacity(tracks.len());
    for track in &tracks {
        let file_path = cache_state.track_file_path(track.id, "flac");
        let file_path_str = file_path.to_string_lossy().to_string();
        batch.push((
            crate::offline_cache::TrackCacheInfo {
                track_id: track.id,
                title: track.title.clone(),
                artist: track.artist.clone(),
                album: track.album.clone(),
                album_id: track.album_id.clone(),
                duration_secs: track.duration_secs,
                quality: track.quality.clone(),
                bit_depth: track.bit_depth,
                sample_rate: track.sample_rate,
            },
            file_path_str,
        ));
    }

    // Single transactional batch insert
    {
        let guard = cache_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session - please log in")?;
        let refs: Vec<(&crate::offline_cache::TrackCacheInfo, String)> = batch
            .iter()
            .map(|(info, path)| (info, path.clone()))
            .collect();
        db.insert_tracks_batch(&refs)?;
    }

    // Spawn download tasks for each track
    for track in &tracks {
        let file_path = cache_state.track_file_path(track.id, "flac");
        spawn_track_cache_download(
            track.id,
            file_path,
            state.client.clone(),
            cache_state.fetcher.clone(),
            cache_state.db.clone(),
            cache_state.get_cache_path(),
            cache_state.library_db.clone(),
            app_handle.clone(),
            cache_state.cache_semaphore.clone(),
        );
    }

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
pub async fn v2_library_scan(state: State<'_, crate::library::LibraryState>) -> Result<(), String> {
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
    remote_control_settings: State<
        '_,
        crate::config::remote_control_settings::RemoteControlSettingsState,
    >,
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
pub fn v2_get_current_version(state: State<'_, crate::updates::UpdatesState>) -> String {
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

// ── Purchases (Qobuz) ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct V2PurchaseFormatOption {
    pub id: u32,
    pub label: String,
    pub bit_depth: Option<u32>,
    pub sampling_rate: Option<f64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(non_snake_case)]
pub struct V2DynamicTrackToAnalyseInput {
    pub trackId: u64,
    pub artistId: u64,
    pub genreId: u64,
    pub labelId: u64,
}

fn v2_purchase_extension(format_id: u32, mime_type: &str) -> &'static str {
    if format_id == 5 || mime_type.contains("mpeg") {
        "mp3"
    } else {
        "flac"
    }
}

fn v2_purchase_target_path(
    destination: &str,
    artist_name: &str,
    album_title: &str,
    quality_dir: &str,
    track_number: u32,
    track_title: &str,
    ext: &str,
) -> PathBuf {
    let artist_dir = crate::offline_cache::metadata::sanitize_filename(artist_name);
    let album_dir = crate::offline_cache::metadata::sanitize_filename(album_title);
    let title_clean = crate::offline_cache::metadata::sanitize_filename(track_title);

    let file_name = if track_number > 0 {
        format!("{:02} - {}.{}", track_number, title_clean, ext)
    } else {
        format!("{}.{}", title_clean, ext)
    };

    let mut path = PathBuf::from(destination)
        .join(artist_dir)
        .join(album_dir);
    if !quality_dir.is_empty() {
        path = path.join(crate::offline_cache::metadata::sanitize_filename(quality_dir));
    }
    path.join(file_name)
}

fn v2_apply_purchase_download_flags(
    response: &mut PurchaseResponse,
    downloaded_ids: &HashSet<i64>,
) {
    for track in &mut response.tracks.items {
        track.downloaded = downloaded_ids.contains(&(track.id as i64));
    }

    for album in &mut response.albums.items {
        let album_track_ids: Vec<i64> = response
            .tracks
            .items
            .iter()
            .filter(|track| {
                track
                    .album
                    .as_ref()
                    .map(|album_ref| album_ref.id == album.id)
                    .unwrap_or(false)
            })
            .map(|track| track.id as i64)
            .collect();

        album.downloaded = !album_track_ids.is_empty()
            && album_track_ids
                .iter()
                .all(|track_id| downloaded_ids.contains(track_id));
    }
}

fn v2_filter_purchase_response(mut response: PurchaseResponse, query: &str) -> PurchaseResponse {
    let q = query.to_lowercase();

    response.albums.items.retain(|album| {
        album.title.to_lowercase().contains(&q) || album.artist.name.to_lowercase().contains(&q)
    });
    response.albums.total = response.albums.items.len() as u32;
    response.albums.offset = 0;

    response.tracks.items.retain(|track| {
        track.title.to_lowercase().contains(&q)
            || track.performer.name.to_lowercase().contains(&q)
            || track
                .album
                .as_ref()
                .map(|album_ref| album_ref.title.to_lowercase().contains(&q))
                .unwrap_or(false)
    });
    response.tracks.total = response.tracks.items.len() as u32;
    response.tracks.offset = 0;

    response
}

async fn v2_download_purchase_track_impl(
    track_id: u64,
    format_id: u32,
    destination: &str,
    quality_dir: &str,
    app_state: &AppState,
) -> Result<String, String> {
    let client = app_state.client.read().await;
    let track = client
        .get_track(track_id)
        .await
        .map_err(|e| format!("Failed to fetch track {}: {}", track_id, e))?;
    let stream = client
        .get_track_file_url_by_format(track_id, format_id)
        .await
        .map_err(|e| format!("Failed to get download URL for track {}: {}", track_id, e))?;
    drop(client);

    let data = download_audio(&stream.url).await?;

    let artist_name = track
        .performer
        .as_ref()
        .map(|artist| artist.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album_title = track
        .album
        .as_ref()
        .map(|album| album.title.clone())
        .unwrap_or_else(|| "Singles".to_string());
    let extension = v2_purchase_extension(stream.format_id, &stream.mime_type);
    let target_path = v2_purchase_target_path(
        destination,
        &artist_name,
        &album_title,
        quality_dir,
        track.track_number,
        &track.title,
        extension,
    );

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create destination folder: {}", e))?;
    }

    let temp_path = target_path.with_extension(format!("{}.part", extension));
    fs::write(&temp_path, &data).map_err(|e| format!("Failed to write temporary file: {}", e))?;
    fs::rename(&temp_path, &target_path).map_err(|e| format!("Failed to finalize file: {}", e))?;

    Ok(target_path.to_string_lossy().to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_get_all(
    limit: Option<u32>,
    offset: Option<u32>,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<PurchaseResponse, String> {
    let client = app_state.client.read().await;
    let mut response = if let (Some(lim), Some(off)) = (limit, offset) {
        client
            .get_user_purchases_page(lim, off)
            .await
            .map_err(|e| format!("Failed to fetch purchases page: {}", e))?
    } else {
        client
            .get_user_purchases_all()
            .await
            .map_err(|e| format!("Failed to fetch purchases: {}", e))?
    };
    drop(client);

    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let downloaded_ids: HashSet<i64> = db
        .get_downloaded_purchase_track_ids()
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();

    v2_apply_purchase_download_flags(&mut response, &downloaded_ids);
    Ok(response)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_get_ids(
    limit: Option<u32>,
    offset: Option<u32>,
    purchaseType: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<PurchaseIdsResponse, String> {
    let lim = limit.unwrap_or(500);
    let off = offset.unwrap_or(0);
    let type_ref = purchaseType.as_deref();

    let client = app_state.client.read().await;
    let response = client
        .get_user_purchases_ids_page_typed(type_ref, lim, off)
        .await
        .map_err(|e| format!("Failed to fetch purchase IDs page: {}", e))?;
    drop(client);

    Ok(response)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_get_by_type(
    purchaseType: String,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<PurchaseResponse, String> {
    if purchaseType != "albums" && purchaseType != "tracks" {
        return Err(format!(
            "Invalid purchase type '{}'. Expected 'albums' or 'tracks'.",
            purchaseType
        ));
    }

    let client = app_state.client.read().await;
    let mut response = client
        .get_user_purchases_all_typed(&purchaseType)
        .await
        .map_err(|e| format!("Failed to fetch {} purchases: {}", purchaseType, e))?;
    drop(client);

    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let downloaded_ids: HashSet<i64> = db
        .get_downloaded_purchase_track_ids()
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();

    if purchaseType == "tracks" {
        for track in &mut response.tracks.items {
            track.downloaded = downloaded_ids.contains(&(track.id as i64));
        }
    } else {
        for album in &mut response.albums.items {
            let album_track_ids: Vec<i64> = album
                .tracks
                .as_ref()
                .map(|tracks_page| {
                    tracks_page
                        .items
                        .iter()
                        .map(|track| track.id as i64)
                        .collect::<Vec<i64>>()
                })
                .unwrap_or_default();

            album.downloaded = !album_track_ids.is_empty()
                && album_track_ids
                    .iter()
                    .all(|track_id| downloaded_ids.contains(track_id));
        }
    }

    Ok(response)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_search(
    query: String,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<PurchaseResponse, String> {
    let mut response = v2_purchases_get_all(None, None, app_state, library_state).await?;
    if query.trim().is_empty() {
        return Ok(response);
    }
    response = v2_filter_purchase_response(response, query.trim());
    Ok(response)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_get_album(
    albumId: String,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<PurchaseAlbum, String> {
    let client = app_state.client.read().await;
    let album = client
        .get_album(&albumId)
        .await
        .map_err(|e| format!("Failed to fetch album {}: {}", albumId, e))?;
    let purchases = client
        .get_user_purchases_all()
        .await
        .map_err(|e| format!("Failed to fetch purchases: {}", e))?;
    drop(client);

    let purchase_meta = purchases
        .albums
        .items
        .iter()
        .find(|item| item.id == albumId);

    let tracks_items: Vec<PurchaseTrack> = album
        .tracks
        .as_ref()
        .map(|tracks| {
            tracks
                .items
                .iter()
                .map(|track| PurchaseTrack {
                    id: track.id,
                    title: track.title.clone(),
                    track_number: track.track_number,
                    media_number: track.media_number,
                    duration: track.duration,
                    performer: track.performer.clone().unwrap_or_default(),
                    album: track.album.clone(),
                    hires: track.hires,
                    maximum_sampling_rate: track.maximum_sampling_rate,
                    maximum_bit_depth: track.maximum_bit_depth,
                    streamable: track.streamable,
                    downloaded: false,
                    downloaded_format_ids: Vec::new(),
                    purchased_at: purchase_meta.and_then(|item| item.purchased_at),
                })
                .collect()
        })
        .unwrap_or_default();

    let mut result = PurchaseAlbum {
        id: album.id.clone(),
        title: album.title.clone(),
        artist: album.artist.clone(),
        image: album.image.clone(),
        release_date_original: album.release_date_original.clone(),
        label: album.label.clone(),
        genre: album.genre.clone(),
        tracks_count: album.tracks_count,
        duration: album.duration,
        hires: album.hires,
        maximum_sampling_rate: album.maximum_sampling_rate,
        maximum_bit_depth: album.maximum_bit_depth,
        downloadable: purchase_meta.map(|item| item.downloadable).unwrap_or(true),
        downloaded: false,
        purchased_at: purchase_meta.and_then(|item| item.purchased_at),
        tracks: Some(ApiSearchResultsPage {
            offset: 0,
            limit: tracks_items.len() as u32,
            total: tracks_items.len() as u32,
            items: tracks_items,
        }),
    };

    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let downloaded_formats = db
        .get_downloaded_purchase_formats()
        .map_err(|e| e.to_string())?;

    // Build per-track format lookup: track_id -> Vec<format_id>
    let mut format_map: std::collections::HashMap<i64, Vec<u32>> = std::collections::HashMap::new();
    for (track_id, format_id) in &downloaded_formats {
        format_map.entry(*track_id).or_default().push(*format_id as u32);
    }
    let downloaded_ids: HashSet<i64> = downloaded_formats.iter().map(|(tid, _)| *tid).collect();

    if let Some(tracks) = &mut result.tracks {
        for track in &mut tracks.items {
            let tid = track.id as i64;
            track.downloaded = downloaded_ids.contains(&tid);
            track.downloaded_format_ids = format_map.get(&tid).cloned().unwrap_or_default();
        }
        result.downloaded = !tracks.items.is_empty()
            && tracks
                .items
                .iter()
                .all(|track| downloaded_ids.contains(&(track.id as i64)));
    }

    Ok(result)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_get_formats(
    albumId: String,
    app_state: State<'_, AppState>,
) -> Result<Vec<V2PurchaseFormatOption>, String> {
    let client = app_state.client.read().await;
    let album = client
        .get_album(&albumId)
        .await
        .map_err(|e| format!("Failed to fetch album {}: {}", albumId, e))?;
    drop(client);

    let mut formats = Vec::new();

    if album.hires && album.maximum_sampling_rate.unwrap_or(0.0) > 96.0 {
        formats.push(V2PurchaseFormatOption {
            id: 27,
            label: "FLAC 24-bit / 192 kHz".to_string(),
            bit_depth: Some(24),
            sampling_rate: Some(192.0),
        });
    }

    if album.hires {
        formats.push(V2PurchaseFormatOption {
            id: 7,
            label: "FLAC 24-bit / 96 kHz".to_string(),
            bit_depth: Some(24),
            sampling_rate: Some(96.0),
        });
    }

    formats.push(V2PurchaseFormatOption {
        id: 6,
        label: "FLAC 16-bit / 44.1 kHz".to_string(),
        bit_depth: Some(16),
        sampling_rate: Some(44.1),
    });

    formats.push(V2PurchaseFormatOption {
        id: 5,
        label: "MP3 320 kbps".to_string(),
        bit_depth: None,
        sampling_rate: None,
    });

    Ok(formats)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_download_track(
    trackId: u64,
    formatId: u32,
    destination: String,
    qualityDir: String,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<String, String> {
    let file_path =
        v2_download_purchase_track_impl(trackId, formatId, &destination, &qualityDir, &app_state).await?;

    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.mark_purchase_downloaded(trackId as i64, None, &file_path, formatId as i64)
        .map_err(|e| e.to_string())?;

    Ok(file_path)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_download_album(
    albumId: String,
    formatId: u32,
    destination: String,
    qualityDir: String,
    app_state: State<'_, AppState>,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    let client = app_state.client.read().await;
    let album = client
        .get_album(&albumId)
        .await
        .map_err(|e| format!("Failed to fetch album {}: {}", albumId, e))?;
    drop(client);

    let tracks = album
        .tracks
        .as_ref()
        .map(|list| list.items.clone())
        .unwrap_or_default();

    let mut failures: Vec<String> = Vec::new();
    for track in tracks {
        match v2_download_purchase_track_impl(track.id, formatId, &destination, &qualityDir, &app_state).await {
            Ok(file_path) => {
                let guard = library_state.db.lock().await;
                let db = guard.as_ref().ok_or("No active session - please log in")?;
                if let Err(err) =
                    db.mark_purchase_downloaded(track.id as i64, Some(albumId.as_str()), &file_path, formatId as i64)
                {
                    failures.push(format!("track {} registry error: {}", track.id, err));
                }
            }
            Err(err) => failures.push(format!("track {}: {}", track.id, err)),
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Album download completed with errors: {}",
            failures.join(" | ")
        ))
    }
}

// ── Downloaded Purchases Registry ──

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_mark_downloaded(
    trackId: i64,
    albumId: Option<String>,
    filePath: String,
    formatId: i64,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.mark_purchase_downloaded(trackId, albumId.as_deref(), &filePath, formatId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_purchases_remove_downloaded(
    trackId: i64,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.remove_downloaded_purchase(trackId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_purchases_get_downloaded_track_ids(
    library_state: State<'_, LibraryState>,
) -> Result<Vec<i64>, String> {
    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_downloaded_purchase_track_ids()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_dynamic_suggest(
    limit: Option<u32>,
    listenedTrackIds: Option<Vec<u64>>,
    tracksToAnalyse: Option<Vec<V2DynamicTrackToAnalyseInput>>,
    app_state: State<'_, AppState>,
) -> Result<DynamicSuggestResponse, String> {
    let request = DynamicSuggestRequest {
        limit: limit.unwrap_or(50).clamp(1, 200),
        listened_tracks_ids: listenedTrackIds.unwrap_or_default(),
        track_to_analysed: tracksToAnalyse
            .unwrap_or_default()
            .into_iter()
            .map(|item| DynamicTrackToAnalyse {
                track_id: item.trackId,
                artist_id: item.artistId,
                genre_id: item.genreId,
                label_id: item.labelId,
            })
            .collect(),
    };

    let client = app_state.client.read().await;
    client
        .get_dynamic_suggest(&request)
        .await
        .map_err(|e| format!("Failed to fetch dynamic suggestions: {}", e))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_dynamic_suggest_raw(
    limit: Option<u32>,
    listenedTrackIds: Option<Vec<u64>>,
    tracksToAnalyse: Option<Vec<V2DynamicTrackToAnalyseInput>>,
    app_state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let request = DynamicSuggestRequest {
        limit: limit.unwrap_or(50).clamp(1, 200),
        listened_tracks_ids: listenedTrackIds.unwrap_or_default(),
        track_to_analysed: tracksToAnalyse
            .unwrap_or_default()
            .into_iter()
            .map(|item| DynamicTrackToAnalyse {
                track_id: item.trackId,
                artist_id: item.artistId,
                genre_id: item.genreId,
                label_id: item.labelId,
            })
            .collect(),
    };

    let client = app_state.client.read().await;
    client
        .get_dynamic_suggest_raw(&request)
        .await
        .map_err(|e| format!("Failed to fetch raw dynamic suggestions: {}", e))
}

// ── Auto-Theme commands ─────────────────────────────────────────────────────
// Image processing (decode + k-means) is CPU-bound, so heavy commands use
// spawn_blocking to avoid freezing the main thread / UI spinner.

#[tauri::command]
pub fn v2_detect_desktop_environment() -> crate::auto_theme::system::DesktopEnvironment {
    crate::auto_theme::system::detect_desktop_environment()
}

#[tauri::command]
pub fn v2_get_system_wallpaper() -> Result<String, String> {
    crate::auto_theme::system::get_system_wallpaper()
}

#[tauri::command]
pub fn v2_get_system_accent_color() -> Result<crate::auto_theme::PaletteColor, String> {
    crate::auto_theme::system::get_system_accent_color()
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_generate_theme_from_image(
    imagePath: String,
) -> Result<crate::auto_theme::GeneratedTheme, String> {
    tokio::task::spawn_blocking(move || {
        let palette = crate::auto_theme::palette::extract_palette(&imagePath)?;
        Ok(crate::auto_theme::generator::generate_theme(
            &palette,
            &imagePath,
        ))
    })
    .await
    .map_err(|e| format!("Theme generation task failed: {}", e))?
}

#[tauri::command]
pub async fn v2_generate_theme_from_wallpaper() -> Result<crate::auto_theme::GeneratedTheme, String> {
    tokio::task::spawn_blocking(|| {
        let wallpaper = crate::auto_theme::system::get_system_wallpaper()?;
        let palette = crate::auto_theme::palette::extract_palette(&wallpaper)?;
        Ok(crate::auto_theme::generator::generate_theme(
            &palette,
            &wallpaper,
        ))
    })
    .await
    .map_err(|e| format!("Theme generation task failed: {}", e))?
}

#[tauri::command]
pub fn v2_generate_theme_from_system_colors() -> Result<crate::auto_theme::GeneratedTheme, String> {
    let scheme = crate::auto_theme::system::get_system_color_scheme()?;
    Ok(crate::auto_theme::generator::generate_theme_from_scheme(&scheme))
}

#[tauri::command]
pub fn v2_get_system_color_scheme() -> Result<crate::auto_theme::SystemColorScheme, String> {
    crate::auto_theme::system::get_system_color_scheme()
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_extract_palette(
    imagePath: String,
) -> Result<crate::auto_theme::ThemePalette, String> {
    tokio::task::spawn_blocking(move || {
        crate::auto_theme::palette::extract_palette(&imagePath)
    })
    .await
    .map_err(|e| format!("Palette extraction task failed: {}", e))?
}

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
