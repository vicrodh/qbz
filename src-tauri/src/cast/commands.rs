//! Tauri commands for Chromecast casting

use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::api::models::Quality;
use crate::AppState;
use crate::cast::{
    CastError, CastStatus, DeviceDiscovery, DiscoveredDevice, MediaMetadata, MediaServer,
};
use crate::cast::chromecast_thread::ChromecastHandle;
use crate::library::{AudioFormat, LibraryState};

/// Cast state shared across commands
/// Uses a dedicated thread for Chromecast operations since rust_cast is not thread-safe
pub struct CastState {
    pub discovery: Arc<Mutex<DeviceDiscovery>>,
    pub chromecast: ChromecastHandle,
    pub media_server: Arc<Mutex<MediaServer>>,
    pub connected_device_ip: Arc<Mutex<Option<String>>>,
}

impl CastState {
    pub fn new() -> Result<Self, CastError> {
        Ok(Self {
            discovery: Arc::new(Mutex::new(DeviceDiscovery::new())),
            chromecast: ChromecastHandle::new(),
            media_server: Arc::new(Mutex::new(MediaServer::start()?)),
            connected_device_ip: Arc::new(Mutex::new(None)),
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

    state
        .chromecast
        .connect(device.ip.clone(), device.port)
        .map_err(|e| e.to_string())?;

    {
        let mut connected = state.connected_device_ip.lock().await;
        *connected = Some(device.ip.clone());
    }

    Ok(())
}

#[tauri::command]
pub async fn cast_disconnect(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.disconnect().map_err(|e| e.to_string())?;

    {
        let mut connected = state.connected_device_ip.lock().await;
        *connected = None;
    }

    Ok(())
}

#[tauri::command]
pub async fn cast_get_status(state: State<'_, CastState>) -> Result<CastStatus, String> {
    state.chromecast.get_status().map_err(|e| e.to_string())
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

    let target_ip = {
        let connected = state.connected_device_ip.lock().await;
        connected.clone()
    };

    let url = {
        let mut server = state.media_server.lock().await;
        server.register_audio(track_id, audio_data, &content_type);
        let url = match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(track_id, ip),
            None => server.get_audio_url(track_id),
        };
        url.ok_or_else(|| "Failed to build media URL".to_string())?
    };

    state
        .chromecast
        .load_media(url, content_type, metadata)
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

    let target_ip = {
        let connected = state.connected_device_ip.lock().await;
        connected.clone()
    };

    let url = {
        let mut server = state.media_server.lock().await;
        server
            .register_file(track_id as u64, &track.file_path)
            .map_err(|e| e.to_string())?;
        let url = match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(track_id as u64, ip),
            None => server.get_audio_url(track_id as u64),
        };
        url.ok_or_else(|| "Failed to build media URL".to_string())?
    };

    let metadata = MediaMetadata {
        title: track.title,
        artist: track.artist,
        album: track.album,
        artwork_url: None,
        duration_secs: Some(track.duration_secs),
    };

    let content_type = content_type_from_format(&track.format).to_string();

    state
        .chromecast
        .load_media(url, content_type, metadata)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_play(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.play().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_pause(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.pause().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_stop(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.stop().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_seek(position_secs: f64, state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.seek(position_secs).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cast_set_volume(volume: f32, state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.set_volume(volume).map_err(|e| e.to_string())
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
