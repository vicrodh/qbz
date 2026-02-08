//! Audio backend system
//!
//! Provides abstraction over different audio backends (PipeWire, ALSA, PulseAudio)
//! allowing users to choose their preferred audio stack.

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

// Re-export commonly used types
pub use backend::{
    AlsaPlugin,
    AudioBackend,
    AudioBackendType,
    AudioDevice,
    BackendConfig,
    BackendManager,
    BackendResult,
};
pub use alsa_direct::AlsaDirectStream;
pub use alsa_backend::{normalize_device_id_to_stable, resolve_stable_to_current_hw};
pub use diagnostic::{AudioDiagnostic, DiagnosticSource, BitDepthResult};
pub use loudness::{ReplayGainData, extract_replaygain, calculate_gain_factor, db_to_linear};
pub use dynamic_amplify::DynamicAmplify;
pub use analyzer_tap::{AnalyzerTap, AnalyzerMessage};
pub use loudness_cache::LoudnessCache;
pub use loudness_analyzer::LoudnessAnalyzer;
