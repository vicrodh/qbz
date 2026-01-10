//! AirPlay device connection and streaming (placeholder)

use serde::{Deserialize, Serialize};

use crate::cast::airplay::{AirPlayError, DiscoveredAirPlayDevice};

/// Metadata for AirPlay playback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirPlayMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork_url: Option<String>,
    pub duration_secs: Option<u64>,
}

/// AirPlay device status (minimal for now)
#[derive(Debug, Clone, Serialize)]
pub struct AirPlayStatus {
    pub device_id: String,
    pub device_name: String,
    pub is_connected: bool,
}

/// AirPlay connection stub (RAOP sender integration pending)
pub struct AirPlayConnection {
    device: DiscoveredAirPlayDevice,
    connected: bool,
}

impl AirPlayConnection {
    /// Connect to an AirPlay device
    pub fn connect(device: DiscoveredAirPlayDevice) -> Result<Self, AirPlayError> {
        Ok(Self {
            device,
            connected: true,
        })
    }

    /// Disconnect from the device
    pub fn disconnect(&mut self) -> Result<(), AirPlayError> {
        self.connected = false;
        Ok(())
    }

    /// Current connection status
    pub fn get_status(&self) -> AirPlayStatus {
        AirPlayStatus {
            device_id: self.device.id.clone(),
            device_name: self.device.name.clone(),
            is_connected: self.connected,
        }
    }

    /// Load media (requires RAOP sender integration)
    pub fn load_media(&mut self, _metadata: AirPlayMetadata) -> Result<(), AirPlayError> {
        Err(AirPlayError::NotImplemented(
            "RAOP sender integration not implemented".to_string(),
        ))
    }

    /// Play/pause/stop controls
    pub fn play(&mut self) -> Result<(), AirPlayError> {
        Err(AirPlayError::NotImplemented(
            "RAOP sender integration not implemented".to_string(),
        ))
    }

    pub fn pause(&mut self) -> Result<(), AirPlayError> {
        Err(AirPlayError::NotImplemented(
            "RAOP sender integration not implemented".to_string(),
        ))
    }

    pub fn stop(&mut self) -> Result<(), AirPlayError> {
        Err(AirPlayError::NotImplemented(
            "RAOP sender integration not implemented".to_string(),
        ))
    }

    /// Volume control (0.0 - 1.0)
    pub fn set_volume(&mut self, _volume: f32) -> Result<(), AirPlayError> {
        Err(AirPlayError::NotImplemented(
            "RAOP sender integration not implemented".to_string(),
        ))
    }
}
