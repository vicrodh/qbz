//! Casting errors

use thiserror::Error;

/// Chromecast casting errors
#[derive(Error, Debug)]
pub enum CastError {
    #[error("Discovery error: {0}")]
    Discovery(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Media error: {0}")]
    Media(String),

    #[error("Server error: {0}")]
    Server(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Not connected to a cast device")]
    NotConnected,

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}

/// AirPlay casting errors
#[derive(Error, Debug)]
pub enum AirPlayError {
    #[error("Discovery error: {0}")]
    Discovery(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Playback error: {0}")]
    Playback(String),

    #[error("Not connected")]
    NotConnected,

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),
}

/// DLNA/UPnP casting errors
#[derive(Error, Debug)]
pub enum DlnaError {
    #[error("Discovery error: {0}")]
    Discovery(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Playback error: {0}")]
    Playback(String),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Not connected")]
    NotConnected,

    #[error("Device not found: {0}")]
    DeviceNotFound(String),
}
