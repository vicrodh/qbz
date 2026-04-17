//! Casting module (Chromecast-first, designed for future DLNA expansion).
//!
//! This module provides Tauri commands for casting functionality.
//! The core casting logic is in the qbz-cast crate.

pub mod chromecast_thread;
pub mod commands;
pub mod device;
pub mod discovery;
pub mod dlna;
pub mod errors;
pub mod media_server;

// Re-export from qbz-cast for internal use
pub use qbz_cast::{
    CastCommand, CastDevice, CastDeviceConnection, CastError, CastPositionInfo, CastStatus,
    ChromecastHandle, DeviceDiscovery, DiscoveredDevice, DiscoveredDlnaDevice, DlnaConnection,
    DlnaDiscovery, DlnaError, DlnaMetadata, DlnaPositionInfo, DlnaStatus, MediaMetadata,
    MediaServer,
};

// Re-export Tauri command states
pub use commands::CastState;
pub use dlna::commands::DlnaState;
