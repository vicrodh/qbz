//! Audio backend system
//!
//! Re-exports from qbz-audio crate for unified type system.
//!
//! CRITICAL: This is the single source of truth for audio types.
//! All audio types are defined in qbz-audio and re-exported here.

// Re-export everything from qbz-audio
pub use qbz_audio::*;

// On non-Linux platforms, ALSA device ID normalization is a no-op.
// OSS device IDs (/dev/dsp0) and others are already stable.
#[cfg(not(target_os = "linux"))]
pub fn normalize_device_id_to_stable(device_id: &str) -> String {
    device_id.to_string()
}
