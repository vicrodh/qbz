//! Playback-related types for QBZ
//!
//! This module contains types related to audio playback:
//! - Queue track representation
//! - Repeat mode
//! - Queue state snapshots
//! - Playback state

use serde::{Deserialize, Serialize};

// ============ Queue Types ============

/// Track info stored in the queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueTrack {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub hires: bool,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
    /// Whether this is a local library track (not from streaming service)
    #[serde(default)]
    pub is_local: bool,
    /// Album ID for navigation
    pub album_id: Option<String>,
    /// Artist ID for navigation
    pub artist_id: Option<u64>,
    /// Whether the track is streamable (false = removed/unavailable)
    #[serde(default = "default_streamable")]
    pub streamable: bool,
    /// Source identifier (e.g., "qobuz", "local", "plex")
    #[serde(default)]
    pub source: Option<String>,
}

fn default_streamable() -> bool {
    true
}

/// Repeat mode options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl Default for RepeatMode {
    fn default() -> Self {
        Self::Off
    }
}

/// Queue state snapshot for frontend
#[derive(Debug, Clone, Serialize)]
pub struct QueueState {
    pub current_track: Option<QueueTrack>,
    pub current_index: Option<usize>,
    pub upcoming: Vec<QueueTrack>,
    pub history: Vec<QueueTrack>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub total_tracks: usize,
}

// ============ Playback State ============

/// Current playback state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackState {
    /// No track loaded
    Stopped,
    /// Track loaded and playing
    Playing,
    /// Track loaded but paused
    Paused,
    /// Loading/buffering track
    Loading,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self::Stopped
    }
}

/// Detailed playback status with position and duration
#[derive(Debug, Clone, Serialize)]
pub struct PlaybackStatus {
    pub state: PlaybackState,
    pub track_id: Option<u64>,
    pub position_secs: u64,
    pub duration_secs: u64,
    pub volume: f32,
    /// Sample rate of currently playing track (Hz)
    pub sample_rate: Option<u32>,
    /// Bit depth of currently playing track
    pub bit_depth: Option<u32>,
}

impl Default for PlaybackStatus {
    fn default() -> Self {
        Self {
            state: PlaybackState::Stopped,
            track_id: None,
            position_secs: 0,
            duration_secs: 0,
            volume: 1.0,
            sample_rate: None,
            bit_depth: None,
        }
    }
}

// ============ Audio Backend Types ============

/// Audio backend type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioBackendType {
    /// PipeWire/PulseAudio via CPAL (default, most compatible)
    PipeWire,
    /// Direct ALSA for bit-perfect playback
    AlsaDirect,
}

impl Default for AudioBackendType {
    fn default() -> Self {
        Self::PipeWire
    }
}

/// Audio device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}
