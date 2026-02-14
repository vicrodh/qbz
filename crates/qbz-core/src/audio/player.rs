//! Audio player implementation using rodio
//!
//! Simple streaming player for POC - downloads and plays audio.

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::io::Cursor;
use std::sync::{Arc, Mutex};

/// Audio player state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
}

/// Simple audio player for POC
/// Note: Not Send because OutputStream contains raw pointers
pub struct AudioPlayer {
    /// Output stream (must be kept alive)
    _stream: OutputStream,
    /// Stream handle for creating sinks
    stream_handle: OutputStreamHandle,
    /// Current playback sink
    sink: Arc<Mutex<Option<Sink>>>,
    /// Current state
    state: Arc<Mutex<PlayerState>>,
    /// Volume (0.0 - 1.0)
    volume: Arc<Mutex<f32>>,
}

impl AudioPlayer {
    /// Create a new audio player with default output device
    pub fn new() -> Result<Self, String> {
        let (stream, stream_handle) = OutputStream::try_default()
            .map_err(|e| format!("Failed to create output stream: {}", e))?;

        Ok(Self {
            _stream: stream,
            stream_handle,
            sink: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(PlayerState::Stopped)),
            volume: Arc::new(Mutex::new(1.0)),
        })
    }

    /// Play audio from bytes (downloaded audio data) - sync version
    pub fn play_bytes_sync(&self, data: Vec<u8>) -> Result<(), String> {
        log::info!("Playing audio ({} bytes)", data.len());

        // Stop any current playback
        self.stop_sync();

        // Create decoder from bytes
        let cursor = Cursor::new(data);
        let source = Decoder::new(cursor)
            .map_err(|e| format!("Failed to decode audio: {}", e))?;

        // Create new sink
        let sink = Sink::try_new(&self.stream_handle)
            .map_err(|e| format!("Failed to create sink: {}", e))?;

        // Set volume
        let vol = *self.volume.lock().unwrap();
        sink.set_volume(vol);

        // Append source and play
        sink.append(source);

        // Store sink
        *self.sink.lock().unwrap() = Some(sink);
        *self.state.lock().unwrap() = PlayerState::Playing;

        log::info!("Playback started");
        Ok(())
    }

    /// Pause playback
    pub fn pause_sync(&self) {
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink.pause();
            *self.state.lock().unwrap() = PlayerState::Paused;
            log::info!("Playback paused");
        }
    }

    /// Resume playback
    pub fn resume_sync(&self) {
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink.play();
            *self.state.lock().unwrap() = PlayerState::Playing;
            log::info!("Playback resumed");
        }
    }

    /// Toggle play/pause
    pub fn toggle_sync(&self) {
        let state = *self.state.lock().unwrap();
        match state {
            PlayerState::Playing => self.pause_sync(),
            PlayerState::Paused => self.resume_sync(),
            PlayerState::Stopped => {}
        }
    }

    /// Stop playback
    pub fn stop_sync(&self) {
        if let Some(sink) = self.sink.lock().unwrap().take() {
            sink.stop();
        }
        *self.state.lock().unwrap() = PlayerState::Stopped;
        log::info!("Playback stopped");
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume_sync(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        *self.volume.lock().unwrap() = volume;
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink.set_volume(volume);
        }
        log::debug!("Volume set to {:.2}", volume);
    }

    /// Check if playing
    pub fn is_playing_sync(&self) -> bool {
        *self.state.lock().unwrap() == PlayerState::Playing
    }

    /// Check if current track has finished
    pub fn is_finished_sync(&self) -> bool {
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink.empty()
        } else {
            true
        }
    }
}
