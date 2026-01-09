//! AirPlay casting errors

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AirPlayError {
    #[error("Discovery error: {0}")]
    Discovery(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Streaming error: {0}")]
    Streaming(String),

    #[error("Not connected to an AirPlay device")]
    NotConnected,

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),
}
