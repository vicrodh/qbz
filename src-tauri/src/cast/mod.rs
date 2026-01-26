//! Casting module (Chromecast-first, designed for future AirPlay/DLNA expansion).

pub mod chromecast_thread;
pub mod commands;
pub mod device;
pub mod discovery;
pub mod errors;
pub mod media_server;
pub mod airplay;
pub mod dlna;

pub use commands::CastState;
pub use device::{CastDeviceConnection, CastStatus, MediaMetadata, CastPositionInfo};
pub use discovery::{DeviceDiscovery, DiscoveredDevice};
pub use errors::CastError;
pub use media_server::MediaServer;
pub use airplay::{
    AirPlayConnection,
    AirPlayDiscovery,
    AirPlayError,
    AirPlayMetadata,
    AirPlayState,
    AirPlayStatus,
    DiscoveredAirPlayDevice,
};
pub use dlna::{
    DiscoveredDlnaDevice,
    DlnaConnection,
    DlnaDiscovery,
    DlnaError,
    DlnaMetadata,
    DlnaState,
    DlnaStatus,
};

pub type CastDevice = CastDeviceConnection;
