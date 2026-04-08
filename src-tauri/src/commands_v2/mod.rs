//! V2 Commands - Using the new multi-crate architecture
//!
//! These commands use QbzCore via CoreBridge instead of the old AppState.
//! Runtime contract ensures proper lifecycle (see ADR_RUNTIME_SESSION_CONTRACT.md).
//!
//! Playback flows through CoreBridge -> QbzCore -> Player (qbz-player crate).

use tauri::State;

use qbz_models::{
    Album, Artist, DiscoverAlbum, DiscoverData, DiscoverPlaylistsResponse, DiscoverResponse,
    GenreInfo, LabelDetail, LabelExploreResponse, LabelPageData, PageArtistResponse, Playlist,
    PlaylistTag, QueueState, QueueTrack as CoreQueueTrack, RepeatMode, SearchResultsPage,
    Track, UserSession,
};
use qconnect_app::QueueCommandType;
use qconnect_app::{QConnectQueueState, QConnectRendererState};

use crate::api::models::{
    PlaylistDuplicateResult, PlaylistWithTrackIds,
};
use crate::artist_blacklist::BlacklistState;
use crate::audio::{AlsaPlugin, AudioBackendType, AudioDevice, BackendManager};
use crate::cache::CacheStats;
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
use crate::qconnect_service::{QconnectServiceState, QconnectVisibleQueueProjection};
use crate::reco_store::RecoState;
use crate::runtime::{
    CommandRequirement, RuntimeError, RuntimeManagerState,
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

mod runtime;
pub use runtime::*;

mod playback;
pub use playback::*;

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
