//! Casting module (Chromecast-first, designed for future AirPlay/DLNA expansion).
//!
//! This module provides Tauri commands for casting functionality.
//! The core casting logic is in the qbz-cast crate.

pub mod chromecast_thread;
pub mod commands;
pub mod device;
pub mod discovery;
pub mod errors;
pub mod media_server;
pub mod airplay;
pub mod dlna;

// Re-export from qbz-cast for internal use
pub use qbz_cast::{
    CastError, CastDevice, CastDeviceConnection, CastStatus, MediaMetadata, CastPositionInfo,
    DeviceDiscovery, DiscoveredDevice, MediaServer, ChromecastHandle, CastCommand,
    AirPlayConnection, AirPlayDiscovery, AirPlayError, AirPlayMetadata, AirPlayStatus,
    DiscoveredAirPlayDevice,
    DlnaConnection, DlnaDiscovery, DlnaError, DlnaMetadata, DlnaStatus, DlnaPositionInfo,
    DiscoveredDlnaDevice,
};

// Re-export Tauri command states
pub use commands::CastState;
pub use airplay::commands::AirPlayState;
pub use dlna::commands::DlnaState;
