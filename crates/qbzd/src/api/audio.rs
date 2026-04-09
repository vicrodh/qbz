use std::sync::Arc;
use axum::Json;
use serde::Deserialize;

use crate::daemon::DaemonCore;

pub async fn get_settings(_daemon: Arc<DaemonCore>) -> Json<serde_json::Value> {
    let result = qbz_audio::settings::AudioSettingsStore::new()
        .ok()
        .and_then(|s| s.get_settings().ok());
    match result {
        Some(settings) => Json(serde_json::to_value(settings).unwrap_or_default()),
        None => Json(serde_json::json!({"error": "no audio settings"})),
    }
}

pub async fn get_backends(_daemon: Arc<DaemonCore>) -> Json<serde_json::Value> {
    use qbz_audio::{AudioBackendType, BackendManager};

    let backends: Vec<serde_json::Value> = BackendManager::available_backends()
        .into_iter()
        .map(|bt| {
            let backend = BackendManager::create_backend(bt);
            let (available, description) = match backend {
                Ok(b) => (b.is_available(), b.description().to_string()),
                Err(_) => (false, "Not available".to_string()),
            };
            let name = match bt {
                AudioBackendType::PipeWire => "PipeWire",
                AudioBackendType::Alsa => "ALSA Direct",
                AudioBackendType::Pulse => "PulseAudio",
                AudioBackendType::SystemDefault => "System Audio",
            };
            serde_json::json!({
                "backend_type": bt,
                "name": name,
                "description": description,
                "is_available": available,
            })
        })
        .collect();

    Json(serde_json::json!(backends))
}

#[derive(Deserialize)]
pub struct DevicesQuery {
    pub backend: Option<String>,
}

pub async fn get_devices(
    _daemon: Arc<DaemonCore>,
    axum::extract::Query(q): axum::extract::Query<DevicesQuery>,
) -> Result<Json<serde_json::Value>, String> {
    use qbz_audio::{AudioBackendType, BackendManager};

    let bt = match q.backend.as_deref() {
        Some("pipewire") | Some("PipeWire") => AudioBackendType::PipeWire,
        Some("alsa") | Some("Alsa") => AudioBackendType::Alsa,
        Some("pulse") | Some("PulseAudio") => AudioBackendType::Pulse,
        _ => AudioBackendType::SystemDefault,
    };

    let backend = BackendManager::create_backend(bt).map_err(|e| e.to_string())?;
    let devices = backend.enumerate_devices().map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(devices).unwrap_or_default()))
}

pub async fn get_hardware_status(daemon: Arc<DaemonCore>) -> Json<serde_json::Value> {
    let player = daemon.core.player();
    let state = &player.state;
    let sr = state.get_sample_rate();
    let bd = state.get_bit_depth();
    let playing = state.is_playing();

    Json(serde_json::json!({
        "is_active": playing && sr > 0,
        "hardware_sample_rate": if sr > 0 { Some(sr) } else { None::<u32> },
        "hardware_format": if sr > 0 && bd > 0 {
            Some(format!("{}-bit / {:.1}kHz", bd, sr as f64 / 1000.0))
        } else {
            None::<String>
        },
    }))
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub backend_type: Option<String>,
    pub output_device: Option<String>,
    pub exclusive_mode: Option<bool>,
    pub dac_passthrough: Option<bool>,
    pub volume_normalization: Option<bool>,
}

pub async fn update_settings(
    _daemon: Arc<DaemonCore>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<serde_json::Value>, String> {
    let store = qbz_audio::settings::AudioSettingsStore::new()
        .map_err(|e| format!("Failed to open settings: {}", e))?;

    if let Some(ref bt) = req.backend_type {
        let backend = match bt.as_str() {
            "PipeWire" | "pipewire" => Some(qbz_audio::AudioBackendType::PipeWire),
            "Alsa" | "alsa" => Some(qbz_audio::AudioBackendType::Alsa),
            "Pulse" | "pulse" => Some(qbz_audio::AudioBackendType::Pulse),
            _ => Some(qbz_audio::AudioBackendType::SystemDefault),
        };
        store.set_backend_type(backend).map_err(|e| e.to_string())?;
    }
    if let Some(ref device) = req.output_device {
        let d = if device.is_empty() { None } else { Some(device.as_str()) };
        store.set_output_device(d).map_err(|e| e.to_string())?;
    }
    if let Some(exc) = req.exclusive_mode {
        store.set_exclusive_mode(exc).map_err(|e| e.to_string())?;
    }
    if let Some(dac) = req.dac_passthrough {
        store.set_dac_passthrough(dac).map_err(|e| e.to_string())?;
    }
    // volume_normalization: no individual setter yet, skip for now

    // Return updated settings
    let updated = store.get_settings().unwrap_or_default();
    Ok(Json(serde_json::to_value(updated).unwrap_or_default()))
}
