//! Tauri commands for DLNA/UPnP casting

use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::api::models::Quality;
use crate::AppState;
use crate::cast::dlna::{
    DiscoveredDlnaDevice, DlnaConnection, DlnaDiscovery, DlnaError, DlnaMetadata, DlnaStatus,
};
use crate::cast::MediaServer;

/// DLNA state shared across commands
pub struct DlnaState {
    pub discovery: Arc<Mutex<DlnaDiscovery>>,
    pub connection: Arc<Mutex<Option<DlnaConnection>>>,
    pub media_server: Arc<Mutex<MediaServer>>,
}

impl DlnaState {
    pub fn new(media_server: Arc<Mutex<MediaServer>>) -> Result<Self, DlnaError> {
        Ok(Self {
            discovery: Arc::new(Mutex::new(DlnaDiscovery::new())),
            connection: Arc::new(Mutex::new(None)),
            media_server,
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

// === Playback ===

/// Play a Qobuz track on the DLNA device
#[tauri::command]
pub async fn dlna_play_track(
    track_id: u64,
    metadata: DlnaMetadata,
    dlna_state: State<'_, DlnaState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    // Get stream URL from Qobuz
    let stream_url = {
        let client = app_state.client.lock().await;
        client
            .get_stream_url_with_fallback(track_id, Quality::HiRes)
            .await
            .map_err(|e| format!("Failed to get stream URL: {}", e))?
    };

    let content_type = stream_url.mime_type.clone();
    let cache = app_state.audio_cache.clone();

    // Download audio data
    let audio_data = if let Some(cached) = cache.get(track_id) {
        cached.data
    } else {
        let data = download_audio(&stream_url.url).await?;
        cache.insert(track_id, data.clone());
        data
    };

    // Register with media server to get a URL the DLNA device can access
    let target_ip = {
        let connection = dlna_state.connection.lock().await;
        connection.as_ref().map(|conn| conn.device_ip().to_string())
    };

    let url = {
        let mut server = dlna_state.media_server.lock().await;
        server.register_audio(track_id, audio_data, &content_type);
        let url = match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(track_id, ip),
            None => server.get_audio_url(track_id),
        };
        url.ok_or_else(|| "Failed to build media URL".to_string())?
    };

    // Load media on DLNA device
    {
        let mut connection = dlna_state.connection.lock().await;
        let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
        conn.load_media(&url, &metadata).await.map_err(|e| e.to_string())?;
    }

    // Start playback
    {
        let mut connection = dlna_state.connection.lock().await;
        let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
        conn.play().await.map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Load media without starting playback (for compatibility with existing castStore)
#[tauri::command]
pub async fn dlna_load_media(
    _metadata: DlnaMetadata,
    _state: State<'_, DlnaState>,
) -> Result<(), String> {
    // This is a stub - actual loading happens in dlna_play_track
    // The castStore calls this but we need the track_id for actual loading
    log::warn!("dlna_load_media called without track_id - use dlna_play_track instead");
    Ok(())
}

#[tauri::command]
pub async fn dlna_play(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.play().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dlna_pause(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.pause().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dlna_stop(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.stop().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dlna_seek(position_secs: u64, state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.seek(position_secs).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dlna_set_volume(volume: f32, state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.set_volume(volume).await.map_err(|e| e.to_string())
}

async fn download_audio(url: &str) -> Result<Vec<u8>, String> {
    use std::time::Duration;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

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

    Ok(bytes.to_vec())
}
