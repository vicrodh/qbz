//! DLNA device connection and playback (placeholder)

use serde::{Deserialize, Serialize};

use crate::cast::dlna::{DiscoveredDlnaDevice, DlnaError};

/// Metadata for DLNA playback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlnaMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork_url: Option<String>,
    pub duration_secs: Option<u64>,
}

/// DLNA device status (minimal for now)
#[derive(Debug, Clone, Serialize)]
pub struct DlnaStatus {
    pub device_id: String,
    pub device_name: String,
    pub is_connected: bool,
}

/// DLNA connection stub (AVTransport integration pending)
pub struct DlnaConnection {
    device: DiscoveredDlnaDevice,
    connected: bool,
}

impl DlnaConnection {
    /// Connect to a DLNA device
    pub fn connect(device: DiscoveredDlnaDevice) -> Result<Self, DlnaError> {
        Ok(Self {
            device,
            connected: true,
        })
    }

    /// Disconnect from the device
    pub fn disconnect(&mut self) -> Result<(), DlnaError> {
        self.connected = false;
        Ok(())
    }

    /// Current connection status
    pub fn get_status(&self) -> DlnaStatus {
        DlnaStatus {
            device_id: self.device.id.clone(),
            device_name: self.device.name.clone(),
            is_connected: self.connected,
        }
    }

    /// Load media (requires AVTransport integration)
    pub fn load_media(&mut self, _metadata: DlnaMetadata) -> Result<(), DlnaError> {
        Err(DlnaError::NotImplemented(
            "AVTransport integration not implemented".to_string(),
        ))
    }

    /// Play/pause/stop controls
    pub fn play(&mut self) -> Result<(), DlnaError> {
        Err(DlnaError::NotImplemented(
            "AVTransport integration not implemented".to_string(),
        ))
    }

    pub fn pause(&mut self) -> Result<(), DlnaError> {
        Err(DlnaError::NotImplemented(
            "AVTransport integration not implemented".to_string(),
        ))
    }

    pub fn stop(&mut self) -> Result<(), DlnaError> {
        Err(DlnaError::NotImplemented(
            "AVTransport integration not implemented".to_string(),
        ))
    }

    /// Volume control (0.0 - 1.0)
    pub fn set_volume(&mut self, _volume: f32) -> Result<(), DlnaError> {
        Err(DlnaError::NotImplemented(
            "RenderingControl integration not implemented".to_string(),
        ))
    }
}
