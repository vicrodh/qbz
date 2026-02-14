//! Events emitted by QBZ Core to frontend adapters
//!
//! These events represent state changes that the UI needs to reflect.

use crate::types::*;

/// Events emitted by QbzCore to the frontend adapter.
///
/// The frontend should handle these events to keep the UI in sync
/// with the core state.
#[derive(Debug, Clone)]
pub enum CoreEvent {
    // ─── Authentication ─────────────────────────────────────────────────────

    /// User needs to log in (no saved session or session expired)
    LoginRequired,

    /// Login succeeded
    LoginSuccess(UserInfo),

    /// Login failed with error message
    LoginFailed(String),

    /// User logged out
    LoggedOut,

    // ─── Playback ───────────────────────────────────────────────────────────

    /// Playback state changed (playing/paused, position, volume)
    PlaybackStateChanged(PlaybackState),

    /// Current track changed
    TrackChanged(Track),

    /// Position updated (emitted periodically during playback)
    PositionChanged {
        /// Current position in milliseconds
        position: u64,
        /// Total duration in milliseconds
        duration: u64,
    },

    /// Volume changed
    VolumeChanged(f32),

    /// Playback stopped (no track loaded)
    PlaybackStopped,

    // ─── Queue ──────────────────────────────────────────────────────────────

    /// Queue contents changed
    QueueChanged(Vec<QueueTrack>),

    /// Current queue index changed
    QueueIndexChanged(usize),

    /// Shuffle mode changed
    ShuffleChanged(bool),

    /// Repeat mode changed
    RepeatChanged(RepeatMode),

    // ─── Data Loading ───────────────────────────────────────────────────────

    /// Started loading data (show spinner)
    LoadingStarted(String),

    /// Finished loading data (hide spinner)
    LoadingFinished(String),

    /// Loading failed
    LoadingFailed {
        /// What was being loaded
        description: String,
        /// Error message
        error: String,
    },

    // ─── Errors ─────────────────────────────────────────────────────────────

    /// General error occurred
    Error(AppError),

    /// Network error (offline, timeout, etc.)
    NetworkError(String),

    /// Audio error (device not found, format not supported, etc.)
    AudioError(String),
}

/// Current playback state
#[derive(Debug, Clone, Default)]
pub struct PlaybackState {
    /// Whether audio is currently playing
    pub is_playing: bool,

    /// Current position in milliseconds
    pub position: u64,

    /// Total duration in milliseconds
    pub duration: u64,

    /// Current volume (0.0 - 1.0)
    pub volume: f32,

    /// Sample rate in Hz (e.g., 44100, 96000, 192000)
    pub sample_rate: Option<u32>,

    /// Bit depth (e.g., 16, 24, 32)
    pub bit_depth: Option<u8>,
}

/// Repeat mode for queue playback
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    /// No repeat - stop after last track
    #[default]
    Off,

    /// Repeat entire queue
    All,

    /// Repeat current track
    One,
}

/// Application error with context
#[derive(Debug, Clone)]
pub struct AppError {
    /// Error code for programmatic handling
    pub code: String,

    /// Human-readable error message
    pub message: String,

    /// Whether the error is recoverable (user can retry)
    pub recoverable: bool,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, recoverable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recoverable,
        }
    }
}
