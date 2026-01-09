//! DLNA/UPnP casting errors

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DlnaError {
    #[error("Discovery error: {0}")]
    Discovery(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Playback error: {0}")]
    Playback(String),

    #[error("Not connected to a DLNA device")]
    NotConnected,

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),
}
