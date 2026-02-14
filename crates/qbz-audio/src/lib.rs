//! QBZ Audio - Bit-perfect audio backend system
//!
//! This crate provides the audio backend abstraction for QBZ:
//! - AudioBackend trait
//! - PipeWire backend (Linux)
//! - ALSA backend (Linux)
//! - ALSA Direct backend (Linux, bit-perfect)
//!
//! # CRITICAL
//!
//! The audio backends in this crate are IMMUTABLE.
//! They have been extensively debugged and tested for bit-perfect playback.
//! DO NOT modify the logic - only imports may change.

// TODO: Phase 2 - Copy audio modules from qbz-nix
// pub mod backend;
// pub mod pipewire_backend;
// pub mod alsa_backend;
// pub mod alsa_direct;
// pub mod loudness;
// pub mod loudness_analyzer;
// pub mod loudness_cache;
// pub mod dynamic_amplify;
// pub mod analyzer_tap;
// pub mod diagnostic;
