//! Casting module (Chromecast-first, designed for future AirPlay/DLNA expansion).
//!
//! This module provides Tauri commands for casting functionality.
//! The core casting logic is in the qbz-cast crate.

pub mod airplay;
pub mod chromecast_thread;
pub mod commands;
pub mod device;
pub mod discovery;
pub mod dlna;
pub mod errors;
pub mod media_server;

// Re-export from qbz-cast for internal use
pub use qbz_cast::{
    AirPlayConnection, AirPlayDiscovery, AirPlayError, AirPlayMetadata, AirPlayStatus, CastCommand,
    CastDevice, CastDeviceConnection, CastError, CastPositionInfo, CastStatus, ChromecastHandle,
    DeviceDiscovery, DiscoveredAirPlayDevice, DiscoveredDevice, DiscoveredDlnaDevice,
    DlnaConnection, DlnaDiscovery, DlnaError, DlnaMetadata, DlnaPositionInfo, DlnaStatus,
    MediaMetadata, MediaServer,
};

// Re-export Tauri command states
pub use airplay::commands::AirPlayState;
pub use commands::CastState;
pub use dlna::commands::DlnaState;
