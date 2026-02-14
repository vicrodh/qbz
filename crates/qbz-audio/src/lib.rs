//! QBZ Audio - Audio backend system for bit-perfect playback
//!
//! This crate provides the audio backend abstraction layer:
//! - Backend trait and implementations (PipeWire, ALSA, PulseAudio)
//! - Audio device enumeration and selection
//! - Loudness analysis and normalization
//! - Diagnostic tools
//!
//! # CRITICAL: This code is IMMUTABLE
//!
//! The audio backend system was carefully designed for bit-perfect playback.
//! Do NOT modify the logic in these files without understanding the full
//! architecture. See `qbz-nix-docs/AUDIO_BACKENDS.md` for details.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     qbz-audio (Tier 1)                      │
//! │  Audio backends, device management, loudness analysis       │
//! └─────────────────────────────────────────────────────────────┘
//!                              ↑
//!                      ┌───────┴───────┐
//!                      │  qbz-models   │
//!                      │   (Tier 0)    │
//!                      └───────────────┘
//! ```

pub mod backend;
pub mod pipewire_backend;
pub mod alsa_backend;
pub mod pulse_backend;
pub mod alsa_direct;
pub mod diagnostic;
pub mod loudness;
pub mod dynamic_amplify;
pub mod analyzer_tap;
pub mod loudness_cache;
pub mod loudness_analyzer;
pub mod settings;
pub mod visualizer;

// Re-export commonly used types
pub use backend::{
    AlsaPlugin,
    AudioBackend,
    AudioBackendType,
    AudioDevice,
    BackendConfig,
    BackendManager,
    BackendResult,
    AlsaDirectError,
    BitPerfectMode,
};
pub use alsa_direct::AlsaDirectStream;
pub use alsa_backend::{normalize_device_id_to_stable, resolve_stable_to_current_hw};
pub use diagnostic::{AudioDiagnostic, DiagnosticSource, BitDepthResult};
pub use loudness::{ReplayGainData, extract_replaygain, calculate_gain_factor, db_to_linear};
pub use dynamic_amplify::DynamicAmplify;
pub use analyzer_tap::{AnalyzerTap, AnalyzerMessage};
pub use loudness_cache::LoudnessCache;
pub use loudness_analyzer::LoudnessAnalyzer;
pub use settings::AudioSettings;
pub use visualizer::{VisualizerTap, TappedSource, RingBuffer};
