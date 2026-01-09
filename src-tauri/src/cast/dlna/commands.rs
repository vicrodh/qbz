//! Tauri commands for DLNA/UPnP casting (discovery + scaffolding)

use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::cast::dlna::{
    DiscoveredDlnaDevice, DlnaConnection, DlnaDiscovery, DlnaError, DlnaMetadata, DlnaStatus,
};

/// DLNA state shared across commands
pub struct DlnaState {
    pub discovery: Arc<Mutex<DlnaDiscovery>>,
    pub connection: Arc<Mutex<Option<DlnaConnection>>>,
}

impl DlnaState {
    pub fn new() -> Result<Self, DlnaError> {
        Ok(Self {
            discovery: Arc::new(Mutex::new(DlnaDiscovery::new())),
            connection: Arc::new(Mutex::new(None)),
        })
    }
}

// === Discovery ===

#[tauri::command]
pub async fn dlna_start_discovery(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery
        .start_discovery()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dlna_stop_discovery(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.stop_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dlna_get_devices(
    state: State<'_, DlnaState>,
) -> Result<Vec<DiscoveredDlnaDevice>, String> {
    let discovery = state.discovery.lock().await;
    Ok(discovery.get_discovered_devices())
}

// === Connection ===

#[tauri::command]
pub async fn dlna_connect(device_id: String, state: State<'_, DlnaState>) -> Result<(), String> {
    let device = {
        let discovery = state.discovery.lock().await;
        discovery
            .get_device(&device_id)
            .ok_or_else(|| DlnaError::DeviceNotFound(device_id.clone()))
            .map_err(|e| e.to_string())?
    };

    let connection = DlnaConnection::connect(device).map_err(|e| e.to_string())?;
    let mut state_connection = state.connection.lock().await;
    *state_connection = Some(connection);
    Ok(())
}

#[tauri::command]
pub async fn dlna_disconnect(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    if let Some(conn) = connection.as_mut() {
        conn.disconnect().map_err(|e| e.to_string())?;
    }
    *connection = None;
    Ok(())
}

#[tauri::command]
pub async fn dlna_get_status(state: State<'_, DlnaState>) -> Result<DlnaStatus, String> {
    let connection = state.connection.lock().await;
    let conn = connection.as_ref().ok_or_else(|| "Not connected".to_string())?;
    Ok(conn.get_status())
}

// === Playback (stubs) ===

#[tauri::command]
pub async fn dlna_load_media(
    metadata: DlnaMetadata,
    state: State<'_, DlnaState>,
) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.load_media(metadata).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dlna_play(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.play().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dlna_pause(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.pause().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dlna_stop(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.stop().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dlna_set_volume(volume: f32, state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.set_volume(volume).map_err(|e| e.to_string())
}
