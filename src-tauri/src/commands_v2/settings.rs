use tauri::State;

use crate::audio::{AlsaPlugin, AudioBackendType, AudioDevice, BackendManager};
use crate::cache::CacheStats;
use crate::config::favorites_preferences::FavoritesPreferences;
use crate::config::playback_preferences::{
    AutoplayMode, PlaybackPreferences, PlaybackPreferencesState,
};
use crate::config::tray_settings::TraySettings;
use crate::config::tray_settings::TraySettingsState;
use crate::config::window_settings::WindowSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::AppState;

use super::helpers::{AlsaPluginInfo, BackendInfo, DacCapabilities, HardwareAudioStatus};

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
pub fn v2_set_match_system_window_chrome(
    value: bool,
    state: State<'_, WindowSettingsState>,
) -> Result<(), String> {
    state.set_match_system_window_chrome(value)
}

/// Report whether the main window was built transparent for this session.
/// The frontend uses this to decide whether to apply the rounded-corner
/// CSS (which only looks right on a transparent window).
#[tauri::command]
pub fn v2_main_window_is_transparent() -> bool {
    crate::main_window_built_transparent()
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
pub fn v2_set_resume_playback_position(
    resume: bool,
    state: State<'_, PlaybackPreferencesState>,
) -> Result<(), String> {
    state.set_resume_playback_position(resume)
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
