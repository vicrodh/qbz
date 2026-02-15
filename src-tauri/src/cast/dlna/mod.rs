//! DLNA/UPnP casting module
//!
//! Core functionality is in qbz-cast crate, this module provides Tauri commands.

pub mod commands;

// Re-export from qbz-cast
pub use qbz_cast::{
    DlnaConnection, DlnaMetadata, DlnaPositionInfo, DlnaStatus, DlnaDiscovery,
    DiscoveredDlnaDevice, DlnaError,
};

// Re-export Tauri command state
pub use commands::DlnaState;
