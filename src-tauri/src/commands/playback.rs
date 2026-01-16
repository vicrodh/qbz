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

    // Check if track is in memory cache (L1)
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

    // Check if track is in playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!("Playing track {} from playback cache ({} bytes)", track_id, audio_data.len());

            // Promote back to memory cache
            cache.insert(track_id, audio_data.clone());

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

    // Not in any cache - download and cache in memory
    log::info!("Track {} not in any cache, streaming...", track_id);

    let client = state.client.lock().await;

    // Get the stream URL with highest quality available
    let stream_url = client
        .get_stream_url_with_fallback(track_id, Quality::UltraHiRes)
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

/// Prefetch a track into the in-memory cache without starting playback
#[tauri::command]
pub async fn prefetch_track(
    track_id: u64,
    state: State<'_, AppState>,
    download_cache: State<'_, DownloadCacheState>,
) -> Result<(), String> {
    log::info!("Command: prefetch_track {}", track_id);

    let cache = state.audio_cache.clone();

    if cache.contains(track_id) {
        log::info!("Track {} already in memory cache", track_id);
        return Ok(());
    }

    if cache.is_fetching(track_id) {
        log::info!("Track {} already being fetched", track_id);
        return Ok(());
    }

    cache.mark_fetching(track_id);
    let result = async {
        // Check persistent download cache first
        {
            let db = download_cache.db.lock().await;
            if let Ok(Some(file_path)) = db.get_file_path(track_id) {
                let path = std::path::Path::new(&file_path);
                if path.exists() {
                    log::info!("Prefetching track {} from download cache", track_id);
                    drop(db);
                    let audio_data = std::fs::read(path)
                        .map_err(|e| format!("Failed to read cached file: {}", e))?;
                    cache.insert(track_id, audio_data);
                    return Ok(());
                }
            }
        }

        let client = state.client.lock().await;
        let stream_url = client
            .get_stream_url_with_fallback(track_id, Quality::UltraHiRes)
            .await
            .map_err(|e| format!("Failed to get stream URL: {}", e))?;
        drop(client);

        let audio_data = download_audio(&stream_url.url).await?;
        cache.insert(track_id, audio_data);
        Ok(())
    }
    .await;

    cache.unmark_fetching(track_id);
    result
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

/// Number of Qobuz tracks to prefetch (not total tracks, just Qobuz)
const QOBUZ_PREFETCH_COUNT: usize = 3;

/// How far ahead to look for tracks to prefetch (to handle mixed playlists)
const PREFETCH_LOOKAHEAD: usize = 15;

/// Spawn background tasks to prefetch upcoming Qobuz tracks
/// For mixed playlists, we look further ahead to find Qobuz tracks past local ones
fn spawn_prefetch(
    client: Arc<Mutex<QobuzClient>>,
    cache: Arc<AudioCache>,
    queue: &QueueManager,
) {
    // Look further ahead to find Qobuz tracks in mixed playlists
    let upcoming_tracks = queue.peek_upcoming(PREFETCH_LOOKAHEAD);

    if upcoming_tracks.is_empty() {
        log::debug!("No upcoming tracks to prefetch");
        return;
    }

    let mut qobuz_prefetched = 0;

    for track in upcoming_tracks {
        // Stop once we've prefetched enough Qobuz tracks
        if qobuz_prefetched >= QOBUZ_PREFETCH_COUNT {
            break;
        }

        let track_id = track.id;
        let track_title = track.title.clone();

        // Skip local tracks - they don't need prefetching from Qobuz
        if track.is_local {
            log::debug!("Skipping prefetch for local track: {} - {}", track_id, track_title);
            continue;
        }

        // Check if already cached or being fetched
        if cache.contains(track_id) {
            log::debug!("Track {} already cached", track_id);
            qobuz_prefetched += 1; // Count as "handled"
            continue;
        }

        if cache.is_fetching(track_id) {
            log::debug!("Track {} already being fetched", track_id);
            qobuz_prefetched += 1; // Count as "handled"
            continue;
        }

        // Mark as fetching
        cache.mark_fetching(track_id);
        qobuz_prefetched += 1;

        let client_clone = client.clone();
        let cache_clone = cache.clone();

        log::info!("Prefetching track: {} - {}", track_id, track_title);

        // Spawn background task for each track
        tokio::spawn(async move {
            let result = async {
                let client_guard = client_clone.lock().await;
                let stream_url = client_guard
                    .get_stream_url_with_fallback(track_id, Quality::UltraHiRes)
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
    state.media_controls.set_playback_with_progress(true, 0);
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
    pub name: String,           // Technical name (internal use)
    pub display_name: String,   // Friendly name for UI
    pub is_default: bool,
    pub device_type: Option<String>,  // "USB", "PCI", "HDMI", etc.
}

/// Get friendly device names from PipeWire/PulseAudio
#[cfg(target_os = "linux")]
fn get_pipewire_device_descriptions() -> std::collections::HashMap<String, String> {
    use std::process::Command;

    let mut map = std::collections::HashMap::new();

    // Try pactl (PipeWire/PulseAudio)
    if let Ok(output) = Command::new("pactl").args(&["list", "sinks"]).output() {
        if let Ok(text) = String::from_utf8(output.stdout) {
            let mut current_name: Option<String> = None;

            for line in text.lines() {
                let line = line.trim();

                // Extract Name: field
                if let Some(name) = line.strip_prefix("Name: ") {
                    current_name = Some(name.to_string());
                }

                // Extract Description: field
                if let Some(desc) = line.strip_prefix("Description: ") {
                    if let Some(name) = &current_name {
                        map.insert(name.clone(), desc.to_string());
                        log::debug!("Device mapping: {} -> {}", name, desc);
                    }
                }
            }
        }
    }

    log::info!("Found {} device descriptions from PipeWire/PulseAudio", map.len());
    map
}

#[cfg(not(target_os = "linux"))]
fn get_pipewire_device_descriptions() -> std::collections::HashMap<String, String> {
    std::collections::HashMap::new()
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

    // Get friendly names from PipeWire/PulseAudio
    let friendly_names = get_pipewire_device_descriptions();

    let devices: Vec<AudioDevice> = host
        .output_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?
        .filter_map(|device| {
            device.name().ok().map(|name| {
                let is_default = default_device_name.as_ref().map(|d| d == &name).unwrap_or(false);

                // Try to get friendly name from PipeWire/PulseAudio
                let display_name = friendly_names
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| {
                        // Fallback: clean up technical name slightly
                        name.replace("alsa_output.", "")
                            .replace("_", " ")
                            .replace("pro-output", "")
                            .trim()
                            .to_string()
                    });

                // Detect device type from technical name
                let device_type = if name.contains("usb") || name.contains("USB") {
                    Some("USB".to_string())
                } else if name.contains("hdmi") || name.contains("HDMI") {
                    Some("HDMI".to_string())
                } else if name.contains("pci") || name.contains("PCI") {
                    Some("PCI".to_string())
                } else if name.contains("bluetooth") || name.contains("bluez") {
                    Some("Bluetooth".to_string())
                } else {
                    None
                };

                log::debug!(
                    "Device: {} -> {} (type: {:?}, default: {})",
                    name,
                    display_name,
                    device_type,
                    is_default
                );

                AudioDevice {
                    name,
                    display_name,
                    is_default,
                    device_type,
                }
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

/// Reinitialize audio device (releases and re-acquires)
/// Call this when changing audio settings like exclusive mode or output device
#[tauri::command]
pub fn reinit_audio_device(
    device: Option<String>,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    log::info!("Command: reinit_audio_device {:?}", device);
    state.player.reinit_device(device)
}

/// PipeWire/PulseAudio sink information (cross-platform compatible struct)
#[derive(serde::Serialize, Clone)]
pub struct PipewireSink {
    /// Internal name (e.g., "alsa_output.usb-XXX" on Linux, device name on Mac)
    pub name: String,
    /// User-friendly description (e.g., "AB13X Headset Adapter Analog Stereo")
    pub description: String,
    /// Current volume percentage (0-100) - only available on Linux
    pub volume: Option<u32>,
    /// Whether this is the default sink
    pub is_default: bool,
}

/// Get PipeWire/PulseAudio sink information with friendly names (Linux)
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn get_pipewire_sinks() -> Result<Vec<PipewireSink>, String> {
    log::info!("Command: get_pipewire_sinks (Linux)");

    use std::process::Command;

    // Get default sink name first
    let default_sink = Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        });

    // Get all sinks
    let output = Command::new("pactl")
        .args(["list", "sinks"])
        .output()
        .map_err(|e| format!("Failed to run pactl: {}. Is PulseAudio/PipeWire installed?", e))?;

    if !output.status.success() {
        return Err("pactl command failed".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sinks = Vec::new();

    // Parse pactl output
    let mut current_name: Option<String> = None;
    let mut current_description: Option<String> = None;
    let mut current_volume: Option<u32> = None;

    for line in stdout.lines() {
        let line = line.trim();

        // New sink starts with "Sink #"
        if line.starts_with("Sink #") {
            // Save previous sink if complete
            if let (Some(name), Some(desc)) = (current_name.take(), current_description.take()) {
                let is_default = default_sink.as_ref().map(|d| d == &name).unwrap_or(false);
                sinks.push(PipewireSink {
                    name,
                    description: desc,
                    volume: current_volume.take(),
                    is_default,
                });
            }
            current_volume = None;
        } else if line.starts_with("Name:") {
            current_name = Some(line.trim_start_matches("Name:").trim().to_string());
        } else if line.starts_with("Description:") {
            current_description = Some(line.trim_start_matches("Description:").trim().to_string());
        } else if line.starts_with("Volume:") {
            // Parse volume like "Volume: front-left: 65536 / 100% / 0.00 dB"
            if let Some(percent_pos) = line.find('%') {
                // Find the number before %
                let before_percent = &line[..percent_pos];
                if let Some(slash_pos) = before_percent.rfind('/') {
                    let vol_str = before_percent[slash_pos + 1..].trim();
                    if let Ok(vol) = vol_str.parse::<u32>() {
                        current_volume = Some(vol);
                    }
                }
            }
        }
    }

    // Don't forget the last sink
    if let (Some(name), Some(desc)) = (current_name, current_description) {
        let is_default = default_sink.as_ref().map(|d| d == &name).unwrap_or(false);
        sinks.push(PipewireSink {
            name,
            description: desc,
            volume: current_volume,
            is_default,
        });
    }

    log::info!("Found {} PipeWire sinks", sinks.len());
    Ok(sinks)
}

/// Get audio output devices using cpal (macOS/Windows fallback)
/// Returns devices in the same format as Linux for UI compatibility
#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub fn get_pipewire_sinks() -> Result<Vec<PipewireSink>, String> {
    log::info!("Command: get_pipewire_sinks (non-Linux, using cpal)");

    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    let host = rodio::cpal::default_host();

    let default_device_name = host
        .default_output_device()
        .and_then(|d| d.name().ok());

    let sinks: Vec<PipewireSink> = host
        .output_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?
        .filter_map(|device| {
            device.name().ok().map(|name| {
                let is_default = default_device_name.as_ref().map(|d| d == &name).unwrap_or(false);
                PipewireSink {
                    name: name.clone(),
                    description: name, // On Mac/Windows, name is usually descriptive enough
                    volume: None,      // Volume not available via cpal
                    is_default,
                }
            })
        })
        .collect();

    log::info!("Found {} audio output devices", sinks.len());
    Ok(sinks)
}
