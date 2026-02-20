//! Tauri commands for AirPlay casting (discovery + connection scaffolding)

use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::cast::airplay::{
    AirPlayConnection, AirPlayDiscovery, AirPlayError, AirPlayMetadata, AirPlayStatus,
    DiscoveredAirPlayDevice,
};

/// AirPlay state shared across commands
pub struct AirPlayState {
    pub discovery: Arc<Mutex<AirPlayDiscovery>>,
    pub connection: Arc<Mutex<Option<AirPlayConnection>>>,
}

impl AirPlayState {
    pub fn new() -> Result<Self, AirPlayError> {
        Ok(Self {
            discovery: Arc::new(Mutex::new(AirPlayDiscovery::new())),
            connection: Arc::new(Mutex::new(None)),
        })
    }
}

// === Discovery ===

#[tauri::command]
pub async fn airplay_start_discovery(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.start_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn airplay_stop_discovery(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.stop_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn airplay_get_devices(
    state: State<'_, AirPlayState>,
) -> Result<Vec<DiscoveredAirPlayDevice>, String> {
    let discovery = state.discovery.lock().await;
    Ok(discovery.get_discovered_devices())
}

// === Connection ===

#[tauri::command]
pub async fn airplay_connect(
    device_id: String,
    state: State<'_, AirPlayState>,
) -> Result<(), String> {
    let device = {
        let discovery = state.discovery.lock().await;
        discovery
            .get_device(&device_id)
            .ok_or_else(|| AirPlayError::DeviceNotFound(device_id.clone()))
            .map_err(|e| e.to_string())?
    };

    let connection = AirPlayConnection::connect(device).map_err(|e| e.to_string())?;
    let mut state_connection = state.connection.lock().await;
    *state_connection = Some(connection);
    Ok(())
}

#[tauri::command]
pub async fn airplay_disconnect(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    if let Some(conn) = connection.as_mut() {
        conn.disconnect().map_err(|e| e.to_string())?;
    }
    *connection = None;
    Ok(())
}

#[tauri::command]
pub async fn airplay_get_status(state: State<'_, AirPlayState>) -> Result<AirPlayStatus, String> {
    let connection = state.connection.lock().await;
    let conn = connection
        .as_ref()
        .ok_or_else(|| "Not connected".to_string())?;
    Ok(conn.get_status())
}

// === Playback (stubs) ===

#[tauri::command]
pub async fn airplay_load_media(
    metadata: AirPlayMetadata,
    state: State<'_, AirPlayState>,
) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection
        .as_mut()
        .ok_or_else(|| "Not connected".to_string())?;
    conn.load_media(metadata).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn airplay_play(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection
        .as_mut()
        .ok_or_else(|| "Not connected".to_string())?;
    conn.play().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn airplay_pause(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection
        .as_mut()
        .ok_or_else(|| "Not connected".to_string())?;
    conn.pause().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn airplay_stop(state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection
        .as_mut()
        .ok_or_else(|| "Not connected".to_string())?;
    conn.stop().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn airplay_set_volume(volume: f32, state: State<'_, AirPlayState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection
        .as_mut()
        .ok_or_else(|| "Not connected".to_string())?;
    conn.set_volume(volume).map_err(|e| e.to_string())
}
