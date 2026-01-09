//! Chromecast casting errors

use thiserror::Error;

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
