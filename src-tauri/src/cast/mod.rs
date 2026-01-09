//! Casting module (Chromecast-first, designed for future AirPlay/DLNA expansion).

pub mod commands;
pub mod device;
pub mod discovery;
pub mod errors;
pub mod media_server;

pub use commands::CastState;
pub use device::{CastDeviceConnection, CastStatus, MediaMetadata};
pub use discovery::{DeviceDiscovery, DiscoveredDevice};
pub use errors::CastError;
pub use media_server::MediaServer;

pub type CastDevice = CastDeviceConnection;
