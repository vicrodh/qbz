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
#[cfg(target_os = "linux")]
pub mod pipewire_backend;
#[cfg(target_os = "linux")]
pub mod alsa_backend;
#[cfg(target_os = "linux")]
pub mod pulse_backend;
pub mod alsa_direct;
pub mod analysis;
pub mod analyzer_tap;
pub mod diagnostic;
pub mod dynamic_amplify;
pub mod loudness;
pub mod loudness_analyzer;
pub mod loudness_cache;
pub mod settings;
pub mod visualizer;

// Re-export commonly used types
#[cfg(target_os = "linux")]
pub use alsa_backend::{
    device_supports_sample_rate, get_device_supported_rates, normalize_device_id_to_stable,
    resolve_stable_to_current_hw,
};
pub use alsa_direct::AlsaDirectStream;
pub use analysis::SpectralAnalyzer;
pub use analyzer_tap::{AnalyzerMessage, AnalyzerTap};
pub use backend::{
    AlsaDirectError, AlsaPlugin, AudioBackend, AudioBackendType, AudioDevice, BackendConfig,
    BackendManager, BackendResult, BitPerfectMode,
};
pub use diagnostic::{AudioDiagnostic, BitDepthResult, DiagnosticSource};
pub use dynamic_amplify::DynamicAmplify;
pub use loudness::{calculate_gain_factor, db_to_linear, extract_replaygain, ReplayGainData};
pub use loudness_analyzer::LoudnessAnalyzer;
pub use loudness_cache::LoudnessCache;
pub use settings::AudioSettings;
pub use visualizer::{RingBuffer, TappedSource, VisualizerTap};

// Stub implementations for non-Linux platforms
#[cfg(not(target_os = "linux"))]
pub fn normalize_device_id_to_stable(_id: &str) -> String { _id.to_string() }
#[cfg(not(target_os = "linux"))]
pub fn resolve_stable_to_current_hw(_stable: &str) -> Option<String> { None }
#[cfg(not(target_os = "linux"))]
pub fn device_supports_sample_rate(_device_id: &str, _sample_rate: u32) -> Option<bool> { None }
#[cfg(not(target_os = "linux"))]
pub fn get_device_supported_rates(_device_id: &str) -> Option<Vec<u32>> { None }
