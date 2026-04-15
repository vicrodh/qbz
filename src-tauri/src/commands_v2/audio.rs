use tauri::State;

use crate::audio::{AlsaPlugin, AudioBackendType};
use crate::audio_device_watch::{check_selected_device_presence, DevicePresence};
use crate::config::audio_settings::{AudioSettings, AudioSettingsState};
use crate::core_bridge::CoreBridgeState;
use crate::runtime::RuntimeError;

use super::{convert_to_qbz_audio_settings, sync_audio_settings_to_player};

/// Frontend-facing shape: serializable snapshot of the presence check.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AudioDevicePresence {
    UsingDefault,
    Present { wanted: String },
    Missing { wanted: String, available: Vec<String> },
    Inconclusive,
}

/// Snapshot the presence of the user's currently-selected output
/// device. Frontend calls this on demand (e.g. when a
/// `audio:device-missing` toast button fires Retry).
#[tauri::command]
pub fn v2_check_audio_device_presence(
    audio_settings: State<'_, AudioSettingsState>,
) -> Result<AudioDevicePresence, RuntimeError> {
    let presence = check_selected_device_presence(&audio_settings);
    Ok(match presence {
        DevicePresence::UsingDefault => AudioDevicePresence::UsingDefault,
        DevicePresence::Present => {
            // Re-read the wanted name so we can echo it back.
            let wanted = audio_settings
                .store
                .lock()
                .ok()
                .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()))
                .and_then(|s| s.output_device)
                .unwrap_or_default();
            AudioDevicePresence::Present { wanted }
        }
        DevicePresence::Missing { wanted, available } => {
            AudioDevicePresence::Missing { wanted, available }
        }
        DevicePresence::Inconclusive => AudioDevicePresence::Inconclusive,
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
