//! QBZ Audio - Audio backend system for bit-perfect playback
//!
//! This crate provides the audio backend abstraction layer:
//! - Backend trait and implementations (PipeWire/ALSA on Linux, OSS on FreeBSD, CoreAudio on macOS)
//! - Audio device enumeration and selection
//! - Loudness analysis and normalization
//! - Diagnostic tools
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

// Linux-only audio backends
#[cfg(target_os = "linux")]
pub mod alsa_backend;
#[cfg(target_os = "linux")]
pub mod alsa_direct;
#[cfg(target_os = "linux")]
pub mod pipewire_backend;
#[cfg(target_os = "linux")]
pub mod pulse_backend;

// macOS-only audio backends
#[cfg(target_os = "macos")]
pub mod coreaudio_direct;

// FreeBSD-only audio backends
#[cfg(target_os = "freebsd")]
pub mod oss_direct;

// Platform-agnostic modules
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
#[cfg(target_os = "linux")]
pub use alsa_direct::AlsaDirectStream;
#[cfg(target_os = "freebsd")]
pub use oss_direct::OssDirectStream;
pub use analysis::SpectralAnalyzer;
pub use analyzer_tap::{AnalyzerMessage, AnalyzerTap};
pub use backend::{
    AlsaDirectError, AlsaPlugin, AudioBackend, AudioBackendType, AudioDevice, BackendConfig,
    BackendManager, BackendResult, BitPerfectMode, DirectAudioStream,
};
pub use diagnostic::{AudioDiagnostic, BitDepthResult, DiagnosticSource};
pub use dynamic_amplify::DynamicAmplify;
pub use loudness::{calculate_gain_factor, db_to_linear, extract_replaygain, ReplayGainData};
pub use loudness_analyzer::LoudnessAnalyzer;
pub use loudness_cache::LoudnessCache;
pub use settings::AudioSettings;
pub use visualizer::{RingBuffer, TappedSource, VisualizerTap};

/// Stub: returns the ID unchanged on non-Linux (no ALSA normalization needed).
#[cfg(not(target_os = "linux"))]
pub fn normalize_device_id_to_stable(id: &str) -> String { id.to_string() }

/// Stub: no ALSA device resolution on non-Linux.
#[cfg(not(target_os = "linux"))]
pub fn resolve_stable_to_current_hw(_stable: &str) -> Option<String> { None }

/// Stub: no ALSA sample rate probing on non-Linux.
#[cfg(not(target_os = "linux"))]
pub fn device_supports_sample_rate(_device_id: &str, _sample_rate: u32) -> Option<bool> { None }

/// Stub: no ALSA rate enumeration on non-Linux.
#[cfg(not(target_os = "linux"))]
pub fn get_device_supported_rates(_device_id: &str) -> Option<Vec<u32>> { None }
