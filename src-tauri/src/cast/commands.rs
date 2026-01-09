//! Tauri commands for Chromecast casting

use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::api::models::Quality;
use crate::AppState;
use crate::cast::{
    CastDeviceConnection, CastError, CastStatus, DeviceDiscovery, DiscoveredDevice, MediaMetadata,
    MediaServer,
};
use crate::library::{AudioFormat, LibraryState};

/// Cast state shared across commands
pub struct CastState {
    pub discovery: Arc<Mutex<DeviceDiscovery>>,
    pub connection: Arc<Mutex<Option<CastDeviceConnection>>>,
    pub media_server: Arc<Mutex<MediaServer>>,
}

impl CastState {
    pub fn new() -> Result<Self, CastError> {
        Ok(Self {
            discovery: Arc::new(Mutex::new(DeviceDiscovery::new())),
            connection: Arc::new(Mutex::new(None)),
            media_server: Arc::new(Mutex::new(MediaServer::start()?)),
        })
    }
}

// === Discovery ===

#[tauri::command]
pub async fn cast_start_discovery(state: State<'_, CastState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery
        .start_discovery()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_stop_discovery(state: State<'_, CastState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.stop_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_get_devices(
    state: State<'_, CastState>,
) -> Result<Vec<DiscoveredDevice>, String> {
    let discovery = state.discovery.lock().await;
    Ok(discovery.get_discovered_devices())
}

// === Connection ===

#[tauri::command]
pub async fn cast_connect(device_id: String, state: State<'_, CastState>) -> Result<(), String> {
    let device = {
        let discovery = state.discovery.lock().await;
        discovery
            .get_device(&device_id)
            .ok_or_else(|| CastError::DeviceNotFound(device_id.clone()))
            .map_err(|e| e.to_string())?
    };

    let connection = CastDeviceConnection::connect(&device.ip, device.port)
        .map_err(|e| e.to_string())?;

    let mut state_connection = state.connection.lock().await;
    *state_connection = Some(connection);
    Ok(())
}

#[tauri::command]
pub async fn cast_disconnect(state: State<'_, CastState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    if let Some(conn) = connection.as_mut() {
        conn.disconnect().map_err(|e| e.to_string())?;
    }
    *connection = None;
    Ok(())
}

#[tauri::command]
pub async fn cast_get_status(state: State<'_, CastState>) -> Result<CastStatus, String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.get_status().map_err(|e| e.to_string())
}

// === Playback ===

#[tauri::command]
pub async fn cast_play_track(
    track_id: u64,
    metadata: MediaMetadata,
    state: State<'_, CastState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let stream_url = {
        let client = app_state.client.lock().await;
        client
            .get_stream_url_with_fallback(track_id, Quality::HiRes)
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

    let url = {
        let mut server = state.media_server.lock().await;
        server.register_audio(track_id, audio_data, &content_type);
        server
            .get_audio_url(track_id)
            .ok_or_else(|| "Failed to build media URL".to_string())?
    };

    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.load_media(&url, &content_type, metadata)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_play_local_track(
    track_id: i64,
    state: State<'_, CastState>,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    let track = {
        let db = library_state.db.lock().await;
        db.get_track(track_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Track not found".to_string())?
    };

    let url = {
        let mut server = state.media_server.lock().await;
        server
            .register_file(track_id as u64, &track.file_path)
            .map_err(|e| e.to_string())?;
        server
            .get_audio_url(track_id as u64)
            .ok_or_else(|| "Failed to build media URL".to_string())?
    };

    let metadata = MediaMetadata {
        title: track.title,
        artist: track.artist,
        album: track.album,
        artwork_url: None,
        duration_secs: Some(track.duration_secs),
    };

    let content_type = content_type_from_format(&track.format);

    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.load_media(&url, content_type, metadata)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_play(state: State<'_, CastState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.play().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_pause(state: State<'_, CastState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.pause().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_stop(state: State<'_, CastState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.stop().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_seek(position_secs: f64, state: State<'_, CastState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.seek(position_secs).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_set_volume(volume: f32, state: State<'_, CastState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or_else(|| "Not connected".to_string())?;
    conn.set_volume(volume).map_err(|e| e.to_string())
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

fn content_type_from_format(format: &AudioFormat) -> &'static str {
    match format {
        AudioFormat::Flac => "audio/flac",
        AudioFormat::Alac => "audio/mp4",
        AudioFormat::Wav => "audio/wav",
        AudioFormat::Aiff => "audio/aiff",
        AudioFormat::Ape => "audio/ape",
        AudioFormat::Unknown => "application/octet-stream",
    }
}
