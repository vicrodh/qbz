//! Audio settings persistence
//!
//! Tauri command adapters for the canonical qbz-audio settings store.

use crate::audio::{AlsaPlugin, AudioBackendType};

pub use qbz_audio::settings::{AudioSettings, AudioSettingsState, AudioSettingsStore};

// Tauri commands
#[tauri::command]
pub fn get_audio_settings(
    state: tauri::State<'_, AudioSettingsState>,
) -> Result<AudioSettings, String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_settings()
}

#[tauri::command]
pub fn set_audio_output_device(
    state: tauri::State<'_, AudioSettingsState>,
    device: Option<String>,
) -> Result<(), String> {
    // Normalize hw:X,0 to stable front:CARD=name,DEV=0 format
    // This ensures the saved device ID survives reboots and USB reconnections
    let normalized_device = device
        .as_ref()
        .map(|d| crate::audio::normalize_device_id_to_stable(d));

    log::info!(
        "Command: set_audio_output_device {:?} -> {:?} (normalized)",
        device,
        normalized_device
    );

    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_output_device(normalized_device.as_deref())
}

#[tauri::command]
pub fn set_audio_exclusive_mode(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_exclusive_mode(enabled)
}

#[tauri::command]
pub fn set_audio_dac_passthrough(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_dac_passthrough(enabled)
}

#[tauri::command]
pub fn set_audio_sample_rate(
    state: tauri::State<'_, AudioSettingsState>,
    rate: Option<u32>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_sample_rate(rate)
}

#[tauri::command]
pub fn set_audio_backend_type(
    state: tauri::State<'_, AudioSettingsState>,
    backend_type: Option<AudioBackendType>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_backend_type(backend_type)
}

#[tauri::command]
pub fn set_audio_alsa_plugin(
    state: tauri::State<'_, AudioSettingsState>,
    plugin: Option<AlsaPlugin>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_alsa_plugin(plugin)
}

#[tauri::command]
pub fn set_audio_alsa_hardware_volume(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_alsa_hardware_volume(enabled)
}

#[tauri::command]
pub fn set_audio_stream_first_track(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_stream_first_track(enabled)
}

#[tauri::command]
pub fn set_audio_stream_buffer_seconds(
    state: tauri::State<'_, AudioSettingsState>,
    seconds: u8,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_stream_buffer_seconds(seconds)
}

#[tauri::command]
pub fn set_audio_streaming_only(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_streaming_only(enabled)
}

#[tauri::command]
pub fn set_audio_limit_quality_to_device(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_limit_quality_to_device(enabled)
}

#[tauri::command]
pub fn set_audio_device_max_sample_rate(
    state: tauri::State<'_, AudioSettingsState>,
    rate: Option<u32>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_device_max_sample_rate(rate)
}

/// Set the sample rate limit for a specific device
/// If rate is None, removes the limit for that device
#[tauri::command]
pub fn set_device_sample_rate_limit(
    state: tauri::State<'_, AudioSettingsState>,
    device_id: String,
    rate: Option<u32>,
) -> Result<(), String> {
    log::info!(
        "Command: set_device_sample_rate_limit device={} rate={:?}",
        device_id,
        rate
    );
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_device_sample_rate_limit(&device_id, rate)
}

/// Get the sample rate limit for a specific device
/// Returns None if no limit is set
#[tauri::command]
pub fn get_device_sample_rate_limit(
    state: tauri::State<'_, AudioSettingsState>,
    device_id: String,
) -> Result<Option<u32>, String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_device_sample_rate_limit(&device_id)
}

#[tauri::command]
pub fn set_audio_normalization_enabled(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    log::info!("Command: set_audio_normalization_enabled {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_normalization_enabled(enabled)
}

#[tauri::command]
pub fn set_audio_normalization_target(
    state: tauri::State<'_, AudioSettingsState>,
    target_lufs: f32,
) -> Result<(), String> {
    log::info!(
        "Command: set_audio_normalization_target {} LUFS",
        target_lufs
    );
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_normalization_target_lufs(target_lufs)
}

#[tauri::command]
pub fn set_audio_skip_sink_switch(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    log::info!("Command: set_audio_skip_sink_switch {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;

    // Constraint: cannot enable skip_sink_switch when dac_passthrough is on
    if enabled {
        let settings = store.get_settings()?;
        if settings.dac_passthrough {
            return Err(
                "Cannot enable skip sink switch while DAC passthrough is active".to_string(),
            );
        }
    }

    store.set_skip_sink_switch(enabled)
}

#[tauri::command]
pub fn set_audio_gapless_enabled(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    log::info!("Command: set_audio_gapless_enabled {}", enabled);
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_gapless_enabled(enabled)
}

#[tauri::command]
pub fn reset_audio_settings(
    audio_state: tauri::State<'_, AudioSettingsState>,
    playback_state: tauri::State<'_, crate::config::playback_preferences::PlaybackPreferencesState>,
) -> Result<AudioSettings, String> {
    log::info!("Command: reset_audio_settings (resetting audio + playback to defaults)");

    // Reset audio settings
    let guard = audio_state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    let defaults = store.reset_all()?;

    // Reset playback preferences
    let pb_guard = playback_state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let pb_store = pb_guard
        .as_ref()
        .ok_or("No active session - please log in")?;
    pb_store.reset_all()?;

    Ok(defaults)
}

#[cfg(test)]
mod reserve_dac_tests {
    use super::*;

    #[test]
    fn deserializes_legacy_json_without_field() {
        // Legacy installs predate `reserve_dac_while_running`. Their persisted
        // JSON does not contain the key; serde must default it to false rather
        // than failing the round-trip.
        let legacy = r#"{
            "output_device": null,
            "exclusive_mode": false,
            "dac_passthrough": false,
            "preferred_sample_rate": null,
            "backend_type": null,
            "alsa_plugin": null,
            "alsa_hardware_volume": false,
            "stream_first_track": false,
            "stream_buffer_seconds": 3,
            "streaming_only": false,
            "limit_quality_to_device": false,
            "device_max_sample_rate": null,
            "normalization_enabled": false,
            "normalization_target_lufs": -14.0,
            "gapless_enabled": true,
            "pw_force_bitperfect": false,
            "sync_audio_on_startup": false,
            "quality_fallback_behavior": "ask",
            "skip_sink_switch": false,
            "allow_quality_fallback": false
        }"#;
        let settings: AudioSettings =
            serde_json::from_str(legacy).expect("legacy JSON should deserialize");
        assert!(!settings.reserve_dac_while_running);
    }

    #[test]
    fn round_trip_with_field_set() {
        let mut settings = AudioSettings::default();
        settings.reserve_dac_while_running = true;
        let json = serde_json::to_string(&settings).expect("serialize");
        let parsed: AudioSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(parsed.reserve_dac_while_running);
    }

    #[test]
    fn default_is_false() {
        let settings = AudioSettings::default();
        assert!(!settings.reserve_dac_while_running);
    }
}
