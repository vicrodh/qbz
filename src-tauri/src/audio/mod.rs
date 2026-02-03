//! Audio backend system
//!
//! Provides abstraction over different audio backends (PipeWire, ALSA, PulseAudio)
//! allowing users to choose their preferred audio stack.

pub mod backend;
pub mod pipewire_backend;
pub mod alsa_backend;
pub mod pulse_backend;
pub mod alsa_direct;

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
