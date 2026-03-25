//! QBZ Audio - Audio backend system for bit-perfect playback
//!
//! This crate provides the audio backend abstraction layer:
//! - Backend trait and implementations (PipeWire/ALSA on Linux, OSS on FreeBSD)
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

// Linux-only audio backends
#[cfg(target_os = "linux")]
pub mod alsa_backend;
#[cfg(target_os = "linux")]
pub mod alsa_direct;
#[cfg(target_os = "linux")]
pub mod pipewire_backend;
#[cfg(target_os = "linux")]
pub mod pulse_backend;

// FreeBSD-only audio backends
#[cfg(target_os = "freebsd")]
pub mod oss_backend;
#[cfg(target_os = "freebsd")]
pub mod oss_direct;

// Platform-agnostic modules
pub mod analysis;
pub mod analyzer_tap;
pub mod backend;
pub mod diagnostic;
pub mod dynamic_amplify;
pub mod loudness;
pub mod loudness_analyzer;
pub mod loudness_cache;
pub mod settings;
pub mod visualizer;

// Linux-only re-exports
#[cfg(target_os = "linux")]
pub use alsa_backend::{
    device_supports_sample_rate, get_device_supported_rates, normalize_device_id_to_stable,
    resolve_stable_to_current_hw,
};
#[cfg(target_os = "linux")]
pub use alsa_direct::AlsaDirectStream;

// FreeBSD-only re-exports
#[cfg(target_os = "freebsd")]
pub use oss_direct::OssDirectStream;

// Platform-agnostic re-exports
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
