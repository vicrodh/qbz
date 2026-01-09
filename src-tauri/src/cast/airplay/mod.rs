//! AirPlay casting module (discovery + scaffolding)

pub mod commands;
pub mod device;
pub mod discovery;
pub mod errors;

pub use commands::AirPlayState;
pub use device::{AirPlayConnection, AirPlayMetadata, AirPlayStatus};
pub use discovery::{AirPlayDiscovery, DiscoveredAirPlayDevice};
pub use errors::AirPlayError;
