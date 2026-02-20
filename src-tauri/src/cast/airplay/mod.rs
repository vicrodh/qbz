//! AirPlay casting module (discovery + scaffolding)
//!
//! Core functionality is in qbz-cast crate, this module provides Tauri commands.

pub mod commands;

// Re-export from qbz-cast
pub use qbz_cast::{
    AirPlayConnection, AirPlayDiscovery, AirPlayError, AirPlayMetadata, AirPlayStatus,
    DiscoveredAirPlayDevice,
};

// Re-export Tauri command state
pub use commands::AirPlayState;
