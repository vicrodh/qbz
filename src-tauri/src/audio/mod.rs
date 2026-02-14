//! Audio backend system
//!
//! Re-exports from qbz-audio crate for unified type system.
//!
//! CRITICAL: This is the single source of truth for audio types.
//! All audio types are defined in qbz-audio and re-exported here.

// Re-export everything from qbz-audio
pub use qbz_audio::*;
