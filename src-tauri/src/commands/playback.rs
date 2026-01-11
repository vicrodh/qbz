//! Playback-related Tauri commands

use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::api::client::QobuzClient;
use crate::api::models::Quality;
use crate::cache::AudioCache;
use crate::download_cache::DownloadCacheState;
use crate::player::PlaybackState;
use crate::queue::QueueManager;
use crate::AppState;

/// Play a track by ID (with caching support)
#[tauri::command]
pub async fn play_track(
    track_id: u64,
    state: State<'_, AppState>,
    download_cache: State<'_, DownloadCacheState>,
) -> Result<(), String> {
    log::info!("Command: play_track {}", track_id);

    // First check download cache (persistent disk cache)
    {
        let db = download_cache.db.lock().await;
        if let Ok(Some(file_path)) = db.get_file_path(track_id) {
            let path = std::path::Path::new(&file_path);
            if path.exists() {
                log::info!("Playing track {} from download cache: {:?}", track_id, path);

                // Update last accessed time
                let _ = db.touch(track_id);
                drop(db);  // Release lock before reading file

                // Read file and play
                let audio_data = std::fs::read(path)
                    .map_err(|e| format!("Failed to read cached file: {}", e))?;

                state.player.play_data(audio_data, track_id)?;

                // Prefetch next track in background
                spawn_prefetch(
                    state.client.clone(),
                    state.audio_cache.clone(),
                    &state.queue,
                );

                return Ok(());
            }
        }
    }

    let cache = state.audio_cache.clone();

    // Check if track is in memory cache
    if let Some(cached) = cache.get(track_id) {
        log::info!("Playing track {} from memory cache ({} bytes)", track_id, cached.size_bytes);
        state.player.play_data(cached.data, track_id)?;

        // Prefetch next track in background
        spawn_prefetch(
            state.client.clone(),
            state.audio_cache.clone(),
            &state.queue,
        );

        return Ok(());
    }

    // Not in any cache - download and cache in memory
    log::info!("Track {} not in cache, streaming...", track_id);

    let client = state.client.lock().await;

    // Get the stream URL
    let stream_url = client
        .get_stream_url_with_fallback(track_id, Quality::HiRes)
        .await
        .map_err(|e| format!("Failed to get stream URL: {}", e))?;

    log::info!("Got stream URL for track {}", track_id);

    // Download the audio
    let audio_data = download_audio(&stream_url.url).await?;
    let data_size = audio_data.len();

    // Cache it
    cache.insert(track_id, audio_data.clone());

    // Play it
    state.player.play_data(audio_data, track_id)?;

    log::info!("Playing track {} ({} bytes)", track_id, data_size);

    // Release client lock before prefetching
    drop(client);

    // Prefetch next track in background
    spawn_prefetch(
        state.client.clone(),
        state.audio_cache.clone(),
        &state.queue,
    );

    Ok(())
}

/// Download audio from URL
async fn download_audio(url: &str) -> Result<Vec<u8>, String> {
    use std::time::Duration;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    log::info!("Downloading audio...");

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

    log::info!("Downloaded {} bytes", bytes.len());
    Ok(bytes.to_vec())
}

/// Number of tracks to prefetch ahead
const PREFETCH_COUNT: usize = 3;

/// Spawn background tasks to prefetch upcoming tracks
fn spawn_prefetch(
    client: Arc<Mutex<QobuzClient>>,
    cache: Arc<AudioCache>,
    queue: &QueueManager,
) {
    // Get upcoming tracks from queue
    let upcoming_tracks = queue.peek_upcoming(PREFETCH_COUNT);

    if upcoming_tracks.is_empty() {
        log::debug!("No upcoming tracks to prefetch");
        return;
    }

    for track in upcoming_tracks {
        let track_id = track.id;
        let track_title = track.title.clone();

        // Check if already cached or being fetched
        if cache.contains(track_id) {
            log::debug!("Track {} already cached", track_id);
            continue;
        }

        if cache.is_fetching(track_id) {
            log::debug!("Track {} already being fetched", track_id);
            continue;
        }

        // Mark as fetching
        cache.mark_fetching(track_id);

        let client_clone = client.clone();
        let cache_clone = cache.clone();

        log::info!("Prefetching track: {} - {}", track_id, track_title);

        // Spawn background task for each track
        tokio::spawn(async move {
            let result = async {
                let client_guard = client_clone.lock().await;
                let stream_url = client_guard
                    .get_stream_url_with_fallback(track_id, Quality::HiRes)
                    .await
                    .map_err(|e| format!("Failed to get stream URL: {}", e))?;
                drop(client_guard);

                let data = download_audio(&stream_url.url).await?;
                Ok::<Vec<u8>, String>(data)
            }
            .await;

            match result {
                Ok(data) => {
                    cache_clone.insert(track_id, data);
                    log::info!("Prefetch complete for track {}", track_id);
                }
                Err(e) => {
                    log::warn!("Prefetch failed for track {}: {}", track_id, e);
                }
            }

            cache_clone.unmark_fetching(track_id);
        });
    }
}

/// Pause playback
#[tauri::command]
pub fn pause_playback(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: pause_playback");
    state.media_controls.set_playback(false);
    state.player.pause()
}

/// Resume playback
#[tauri::command]
pub fn resume_playback(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: resume_playback");
    state.media_controls.set_playback(true);
    state.player.resume()
}

/// Stop playback
#[tauri::command]
pub fn stop_playback(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: stop_playback");
    state.media_controls.set_stopped();
    state.player.stop()
}

/// Set media controls metadata (for MPRIS integration)
#[tauri::command]
pub fn set_media_metadata(
    title: String,
    artist: String,
    album: String,
    duration_secs: Option<u64>,
    cover_url: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: set_media_metadata - {} by {}", title, artist);
    crate::update_media_controls_metadata(
        &state.media_controls,
        &title,
        &artist,
        &album,
        duration_secs,
        cover_url,
    );
    state.media_controls.set_playback(true);
    Ok(())
}

/// Set volume (0.0 - 1.0)
#[tauri::command]
pub fn set_volume(volume: f32, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: set_volume {}", volume);
    state.player.set_volume(volume)
}

/// Seek to position in seconds
#[tauri::command]
pub fn seek(position: u64, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: seek {}", position);
    let result = state.player.seek(position);

    // Update MPRIS with new position
    let playback_state = state.player.get_state().unwrap_or_default();
    state.media_controls.set_playback_with_progress(
        playback_state.is_playing,
        position,
    );

    result
}

/// Get current playback state (also updates MPRIS progress)
#[tauri::command]
pub fn get_playback_state(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    let playback_state = state.player.get_state()?;

    // Update MPRIS with current progress (called every ~500ms from frontend)
    state.media_controls.set_playback_with_progress(
        playback_state.is_playing,
        playback_state.position,
    );

    Ok(playback_state)
}

/// Audio device information
#[derive(serde::Serialize)]
pub struct AudioDevice {
    pub name: String,
    pub is_default: bool,
}

/// Get available audio output devices
#[tauri::command]
pub fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    log::info!("Command: get_audio_devices");

    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    let host = rodio::cpal::default_host();

    let default_device_name = host
        .default_output_device()
        .and_then(|d| d.name().ok());

    let devices: Vec<AudioDevice> = host
        .output_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?
        .filter_map(|device| {
            device.name().ok().map(|name| {
                let is_default = default_device_name.as_ref().map(|d| d == &name).unwrap_or(false);
                AudioDevice { name, is_default }
            })
        })
        .collect();

    log::info!("Found {} audio output devices", devices.len());
    Ok(devices)
}

/// Current audio output status
#[derive(serde::Serialize)]
pub struct AudioOutputStatus {
    pub device_name: Option<String>,
    pub is_playing: bool,
}

/// Get current audio output status (what device is actually being used)
#[tauri::command]
pub fn get_audio_output_status(
    state: tauri::State<'_, crate::AppState>,
) -> Result<AudioOutputStatus, String> {
    let device_name = state.player.state.current_device();
    let is_playing = state.player.state.is_playing();

    Ok(AudioOutputStatus {
        device_name,
        is_playing,
    })
}
