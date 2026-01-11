//! Audio player module
//!
//! Handles audio playback with support for:
//! - HTTP streaming from Qobuz
//! - FLAC, MP3 decoding via symphonia
//! - Gapless playback
//! - Volume control
//! - Real-time position tracking via events
//!
//! Uses a dedicated audio thread since rodio's OutputStream is not Send.

use std::io::{BufReader, Cursor};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use rodio::{Decoder, OutputStream, Sink, Source};
use rodio::cpal::traits::{DeviceTrait, HostTrait};

use crate::api::{client::QobuzClient, models::Quality};

/// Commands sent to the audio thread
enum AudioCommand {
    /// Play audio data with track ID and duration
    Play { data: Vec<u8>, track_id: u64, duration_secs: u64 },
    /// Pause playback
    Pause,
    /// Resume playback
    Resume,
    /// Stop playback
    Stop,
    /// Set volume (0.0 - 1.0)
    SetVolume(f32),
    /// Seek to position in seconds
    Seek(u64),
}

/// Event payload for playback state updates
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackEvent {
    pub is_playing: bool,
    pub position: u64,
    pub duration: u64,
    pub track_id: u64,
    pub volume: f32,
}

/// Shared state between main thread and audio thread
#[derive(Clone)]
pub struct SharedState {
    /// Is currently playing
    is_playing: Arc<AtomicBool>,
    /// Current position in seconds
    position: Arc<AtomicU64>,
    /// Total duration in seconds
    duration: Arc<AtomicU64>,
    /// Current track ID
    current_track_id: Arc<AtomicU64>,
    /// Volume (0.0 - 1.0 stored as 0-100)
    volume: Arc<AtomicU64>,
    /// Playback start time (Unix timestamp millis when started/resumed)
    playback_start_millis: Arc<AtomicU64>,
    /// Position when playback was started/resumed (in seconds)
    position_at_start: Arc<AtomicU64>,
    /// Current output device name
    current_device: Arc<std::sync::RwLock<Option<String>>>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            is_playing: Arc::new(AtomicBool::new(false)),
            position: Arc::new(AtomicU64::new(0)),
            duration: Arc::new(AtomicU64::new(0)),
            current_track_id: Arc::new(AtomicU64::new(0)),
            volume: Arc::new(AtomicU64::new(75)),
            playback_start_millis: Arc::new(AtomicU64::new(0)),
            position_at_start: Arc::new(AtomicU64::new(0)),
            current_device: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    pub fn set_current_device(&self, device: Option<String>) {
        if let Ok(mut d) = self.current_device.write() {
            *d = device;
        }
    }

    pub fn current_device(&self) -> Option<String> {
        self.current_device.read().ok().and_then(|d| d.clone())
    }

    /// Get current position based on elapsed time since playback started
    pub fn current_position(&self) -> u64 {
        if !self.is_playing.load(Ordering::SeqCst) {
            return self.position.load(Ordering::SeqCst);
        }

        let start_millis = self.playback_start_millis.load(Ordering::SeqCst);
        if start_millis == 0 {
            return self.position.load(Ordering::SeqCst);
        }

        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let elapsed_secs = (now_millis.saturating_sub(start_millis)) / 1000;
        let position_at_start = self.position_at_start.load(Ordering::SeqCst);
        let duration = self.duration.load(Ordering::SeqCst);

        // Clamp to duration
        (position_at_start + elapsed_secs).min(duration)
    }

    /// Mark playback as started/resumed at current position
    fn start_playback_timer(&self, position: u64) {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.playback_start_millis.store(now_millis, Ordering::SeqCst);
        self.position_at_start.store(position, Ordering::SeqCst);
    }

    /// Mark playback as paused, saving current position
    fn pause_playback_timer(&self) {
        let current_pos = self.current_position();
        self.position.store(current_pos, Ordering::SeqCst);
        self.playback_start_millis.store(0, Ordering::SeqCst);
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing.load(Ordering::SeqCst)
    }

    pub fn position(&self) -> u64 {
        self.position.load(Ordering::SeqCst)
    }

    pub fn duration(&self) -> u64 {
        self.duration.load(Ordering::SeqCst)
    }

    pub fn current_track_id(&self) -> u64 {
        self.current_track_id.load(Ordering::SeqCst)
    }

    pub fn volume(&self) -> f32 {
        self.volume.load(Ordering::SeqCst) as f32 / 100.0
    }
}

/// Audio player that handles streaming playback
/// Uses a dedicated thread for audio output
pub struct Player {
    /// Channel to send commands to the audio thread
    tx: Sender<AudioCommand>,
    /// Shared state accessible from any thread
    pub state: SharedState,
}

impl Default for Player {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Player {
    /// Create a new player with an optional specific output device
    /// If device_name is None, uses the system default device
    pub fn new(device_name: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel::<AudioCommand>();
        let state = SharedState::new();
        let thread_state = state.clone();

        // Spawn dedicated audio thread
        thread::spawn(move || {
            log::info!("Audio thread starting...");

            // Get the audio host
            let host = rodio::cpal::default_host();

            // Find the requested device or use default
            let device = if let Some(ref name) = device_name {
                log::info!("Looking for audio device: {}", name);
                host.output_devices()
                    .ok()
                    .and_then(|mut devices| {
                        devices.find(|d| d.name().ok().as_ref() == Some(name))
                    })
                    .or_else(|| {
                        log::warn!("Device '{}' not found, using default", name);
                        host.default_output_device()
                    })
            } else {
                log::info!("Using default audio device");
                host.default_output_device()
            };

            let device = match device {
                Some(d) => {
                    if let Ok(name) = d.name() {
                        log::info!("Using audio device: {}", name);
                        thread_state.set_current_device(Some(name));
                    }
                    d
                }
                None => {
                    log::error!("No audio output device available");
                    thread_state.set_current_device(None);
                    // Keep thread alive to receive commands
                    loop {
                        match rx.recv() {
                            Ok(_) => log::warn!("Audio command received but no device available"),
                            Err(_) => break,
                        }
                    }
                    return;
                }
            };

            // Create output stream on the selected device
            let (_stream, stream_handle) = match OutputStream::try_from_device(&device) {
                Ok(s) => {
                    log::info!("Audio output initialized successfully");
                    s
                }
                Err(e) => {
                    log::error!("Failed to create audio output on device: {}. Trying default...", e);
                    // Fallback to default
                    match OutputStream::try_default() {
                        Ok(s) => {
                            log::info!("Fallback to default audio output succeeded");
                            s
                        }
                        Err(e2) => {
                            log::error!("Failed to create default audio output: {}", e2);
                            loop {
                                match rx.recv() {
                                    Ok(_) => log::warn!("Audio command received but audio output is unavailable"),
                                    Err(_) => break,
                                }
                            }
                            return;
                        }
                    }
                }
            };

            let mut current_sink: Option<Sink> = None;
            // Store audio data for seeking (we need to re-decode from the beginning)
            let mut current_audio_data: Option<Vec<u8>> = None;

            log::info!("Audio thread ready and waiting for commands");

            loop {
                // Use recv_timeout so we can periodically check if sink is empty
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(AudioCommand::Play { data, track_id, duration_secs }) => {
                        log::info!("Audio thread: playing track {}", track_id);

                        // Stop existing playback
                        if let Some(sink) = current_sink.take() {
                            sink.stop();
                        }

                        // Store audio data for potential seeking
                        current_audio_data = Some(data.clone());

                        // Create new sink
                        let sink = match Sink::try_new(&stream_handle) {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("Failed to create sink: {}", e);
                                continue;
                            }
                        };

                        // Set volume
                        let volume = thread_state.volume.load(Ordering::SeqCst) as f32 / 100.0;
                        sink.set_volume(volume);

                        // Decode audio
                        let cursor = Cursor::new(data);
                        let source = match Decoder::new(BufReader::new(cursor)) {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("Failed to decode audio: {}", e);
                                continue;
                            }
                        };

                        // Get duration from source if available, otherwise use provided duration
                        let actual_duration = source.total_duration()
                            .map(|d| d.as_secs())
                            .unwrap_or(duration_secs);
                        thread_state.duration.store(actual_duration, Ordering::SeqCst);

                        sink.append(source);

                        // Update state
                        thread_state.is_playing.store(true, Ordering::SeqCst);
                        thread_state.position.store(0, Ordering::SeqCst);
                        thread_state.current_track_id.store(track_id, Ordering::SeqCst);
                        thread_state.start_playback_timer(0);

                        current_sink = Some(sink);
                        log::info!("Audio thread: playback started, duration: {}s", actual_duration);
                    }
                    Ok(AudioCommand::Pause) => {
                        if let Some(ref sink) = current_sink {
                            sink.pause();
                            thread_state.pause_playback_timer();
                            thread_state.is_playing.store(false, Ordering::SeqCst);
                            log::info!("Audio thread: paused at {}s", thread_state.position.load(Ordering::SeqCst));
                        }
                    }
                    Ok(AudioCommand::Resume) => {
                        if let Some(ref sink) = current_sink {
                            sink.play();
                            let current_pos = thread_state.position.load(Ordering::SeqCst);
                            thread_state.start_playback_timer(current_pos);
                            thread_state.is_playing.store(true, Ordering::SeqCst);
                            log::info!("Audio thread: resumed");
                        }
                    }
                    Ok(AudioCommand::Stop) => {
                        if let Some(sink) = current_sink.take() {
                            sink.stop();
                        }
                        current_audio_data = None;
                        thread_state.is_playing.store(false, Ordering::SeqCst);
                        thread_state.position.store(0, Ordering::SeqCst);
                        thread_state.playback_start_millis.store(0, Ordering::SeqCst);
                        thread_state.position_at_start.store(0, Ordering::SeqCst);
                        log::info!("Audio thread: stopped");
                    }
                    Ok(AudioCommand::SetVolume(volume)) => {
                        thread_state
                            .volume
                            .store((volume * 100.0) as u64, Ordering::SeqCst);
                        if let Some(ref sink) = current_sink {
                            sink.set_volume(volume);
                        }
                        log::info!("Audio thread: volume set to {}", volume);
                    }
                    Ok(AudioCommand::Seek(position_secs)) => {
                        let Some(ref audio_data) = current_audio_data else {
                            log::warn!("Audio thread: cannot seek - no audio data available");
                            continue;
                        };

                        log::info!("Audio thread: seeking to {}s", position_secs);

                        // Stop current sink
                        if let Some(sink) = current_sink.take() {
                            sink.stop();
                        }

                        // Create new sink
                        let sink = match Sink::try_new(&stream_handle) {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("Failed to create sink for seek: {}", e);
                                continue;
                            }
                        };

                        // Set volume
                        let volume = thread_state.volume.load(Ordering::SeqCst) as f32 / 100.0;
                        sink.set_volume(volume);

                        // Re-decode audio
                        let cursor = Cursor::new(audio_data.clone());
                        let source = match Decoder::new(BufReader::new(cursor)) {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("Failed to decode audio for seek: {}", e);
                                continue;
                            }
                        };

                        // Skip to the desired position
                        let skip_duration = Duration::from_secs(position_secs);
                        let skipped_source = source.skip_duration(skip_duration);

                        sink.append(skipped_source);

                        // Check if we were playing before seek
                        let was_playing = thread_state.is_playing.load(Ordering::SeqCst);
                        if !was_playing {
                            sink.pause();
                        }

                        // Update state
                        thread_state.position.store(position_secs, Ordering::SeqCst);
                        if was_playing {
                            thread_state.start_playback_timer(position_secs);
                        }

                        current_sink = Some(sink);
                        log::info!("Audio thread: seeked to {}s (was_playing: {})", position_secs, was_playing);
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // Check if sink is empty (playback finished)
                        if let Some(ref sink) = current_sink {
                            if sink.empty() && thread_state.is_playing.load(Ordering::SeqCst) {
                                // Track finished playing naturally
                                log::info!("Audio thread: track finished (sink empty)");
                                thread_state.is_playing.store(false, Ordering::SeqCst);
                                // Set position to duration to signal completion
                                let duration = thread_state.duration.load(Ordering::SeqCst);
                                thread_state.position.store(duration, Ordering::SeqCst);
                                thread_state.playback_start_millis.store(0, Ordering::SeqCst);
                            }
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        // Channel closed, exit thread
                        log::info!("Audio thread: channel closed, exiting");
                        break;
                    }
                }
            }
        });

        Self { tx, state }
    }

    /// Play a track by ID (downloads audio)
    pub async fn play_track(
        &self,
        client: &QobuzClient,
        track_id: u64,
        quality: Quality,
    ) -> Result<(), String> {
        log::info!("Player: Starting playback for track {} with quality {:?}", track_id, quality);

        // Get the stream URL
        log::info!("Player: Getting stream URL...");
        let stream_url = client
            .get_stream_url_with_fallback(track_id, quality)
            .await
            .map_err(|e| {
                log::error!("Player: Failed to get stream URL: {}", e);
                format!("Failed to get stream URL: {}", e)
            })?;

        log::info!("Player: Got stream URL: {} (format: {})", stream_url.url, stream_url.mime_type);

        // Download the audio data
        log::info!("Player: Starting audio download...");
        let audio_data = self.download_audio(&stream_url.url).await.map_err(|e| {
            log::error!("Player: Download failed: {}", e);
            e
        })?;
        log::info!("Player: Downloaded {} bytes of audio data", audio_data.len());

        // Send to audio thread
        self.play_data(audio_data, track_id)
    }

    /// Play from raw audio data (for cached tracks)
    pub fn play_data(&self, data: Vec<u8>, track_id: u64) -> Result<(), String> {
        log::info!("Player: Playing {} bytes of audio data for track {}", data.len(), track_id);

        self.tx
            .send(AudioCommand::Play {
                data,
                track_id,
                duration_secs: 0, // Will be determined by decoder
            })
            .map_err(|e| {
                log::error!("Player: Failed to send to audio thread: {}", e);
                format!("Failed to send play command (audio thread may have crashed): {}", e)
            })?;

        log::info!("Player: Playback initiated successfully");
        Ok(())
    }

    /// Download audio from URL with timeout
    async fn download_audio(&self, url: &str) -> Result<Vec<u8>, String> {
        use std::time::Duration;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        log::info!("Downloading audio from URL...");

        let response = client
            .get(url)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch audio: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        log::info!("Download response received, reading bytes...");

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read audio bytes: {}", e))?;

        log::info!("Downloaded {} bytes", bytes.len());
        Ok(bytes.to_vec())
    }

    /// Pause playback
    pub fn pause(&self) -> Result<(), String> {
        self.tx
            .send(AudioCommand::Pause)
            .map_err(|e| format!("Failed to send pause command: {}", e))
    }

    /// Resume playback
    pub fn resume(&self) -> Result<(), String> {
        self.tx
            .send(AudioCommand::Resume)
            .map_err(|e| format!("Failed to send resume command: {}", e))
    }

    /// Stop playback
    pub fn stop(&self) -> Result<(), String> {
        self.tx
            .send(AudioCommand::Stop)
            .map_err(|e| format!("Failed to send stop command: {}", e))
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) -> Result<(), String> {
        let clamped = volume.clamp(0.0, 1.0);
        self.tx
            .send(AudioCommand::SetVolume(clamped))
            .map_err(|e| format!("Failed to send volume command: {}", e))
    }

    /// Seek to position in seconds
    pub fn seek(&self, position: u64) -> Result<(), String> {
        // Clamp to duration if known
        let duration = self.state.duration();
        let clamped_position = if duration > 0 {
            position.min(duration)
        } else {
            position
        };

        self.tx
            .send(AudioCommand::Seek(clamped_position))
            .map_err(|e| format!("Failed to send seek command: {}", e))
    }

    /// Get current playback state with real-time position
    pub fn get_state(&self) -> Result<PlaybackState, String> {
        Ok(PlaybackState {
            is_playing: self.state.is_playing(),
            position: self.state.current_position(),
            duration: self.state.duration(),
            track_id: self.state.current_track_id(),
            volume: self.state.volume(),
        })
    }

    /// Get playback event for emitting to frontend
    pub fn get_playback_event(&self) -> PlaybackEvent {
        PlaybackEvent {
            is_playing: self.state.is_playing(),
            position: self.state.current_position(),
            duration: self.state.duration(),
            track_id: self.state.current_track_id(),
            volume: self.state.volume(),
        }
    }
}

/// Playback state snapshot
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PlaybackState {
    pub is_playing: bool,
    pub position: u64,
    pub duration: u64,
    pub track_id: u64,
    pub volume: f32,
}
