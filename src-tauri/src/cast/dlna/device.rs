//! DLNA device connection and playback via AVTransport SOAP

use serde::{Deserialize, Serialize};
use rupnp::{Device, Service};
use rupnp::http::Uri;
use rupnp::ssdp::URN;

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

/// DLNA device status
#[derive(Debug, Clone, Serialize)]
pub struct DlnaStatus {
    pub device_id: String,
    pub device_name: String,
    pub is_connected: bool,
    pub is_playing: bool,
    pub current_uri: Option<String>,
}

/// DLNA connection with AVTransport and RenderingControl support
pub struct DlnaConnection {
    device: DiscoveredDlnaDevice,
    connected: bool,
    device_url: Uri,
    av_transport_service: Option<Service>,
    rendering_control_service: Option<Service>,
    // Current media URI
    current_uri: Option<String>,
    is_playing: bool,
}

impl DlnaConnection {
    /// Connect to a DLNA device and discover service URLs
    pub async fn connect(device: DiscoveredDlnaDevice) -> Result<Self, DlnaError> {
        let device_url: Uri = device
            .url
            .parse()
            .map_err(|e| DlnaError::Connection(format!("Invalid device URL: {}", e)))?;

        let parsed_device = Device::from_url(device_url.clone())
            .await
            .map_err(|e| DlnaError::Connection(format!("Failed to load device description: {}", e)))?;

        let av_transport_service = parsed_device
            .find_service(&av_transport_urn())
            .cloned();
        let rendering_control_service = parsed_device
            .find_service(&rendering_control_urn())
            .cloned();

        log::info!(
            "DLNA: Connected to {} (AVT: {:?}, RC: {:?})",
            device.name,
            av_transport_service.is_some(),
            rendering_control_service.is_some()
        );

        Ok(Self {
            device,
            connected: true,
            device_url,
            av_transport_service,
            rendering_control_service,
            current_uri: None,
            is_playing: false,
        })
    }

    /// Disconnect from the device
    pub fn disconnect(&mut self) -> Result<(), DlnaError> {
        self.connected = false;
        self.current_uri = None;
        self.is_playing = false;
        log::info!("DLNA: Disconnected from {}", self.device.name);
        Ok(())
    }

    /// Current connection status
    pub fn get_status(&self) -> DlnaStatus {
        DlnaStatus {
            device_id: self.device.id.clone(),
            device_name: self.device.name.clone(),
            is_connected: self.connected,
            is_playing: self.is_playing,
            current_uri: self.current_uri.clone(),
        }
    }

    pub fn device_ip(&self) -> &str {
        &self.device.ip
    }

    /// Set the media URI and start playback
    pub async fn load_media(&mut self, uri: &str, metadata: &DlnaMetadata) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_service = self.av_transport_service.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        // Build DIDL-Lite metadata
        let didl_metadata = build_didl_metadata(uri, metadata);

        let payload = format!(
            "<InstanceID>0</InstanceID><CurrentURI>{}</CurrentURI><CurrentURIMetaData>{}</CurrentURIMetaData>",
            xml_escape(uri),
            xml_escape(&didl_metadata)
        );

        av_service
            .action(&self.device_url, "SetAVTransportURI", &payload)
            .await
            .map_err(|e| DlnaError::Playback(e.to_string()))?;

        self.current_uri = Some(uri.to_string());
        log::info!("DLNA: Set URI to {}", uri);

        Ok(())
    }

    /// Start/resume playback
    pub async fn play(&mut self) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_service = self.av_transport_service.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        av_service
            .action(&self.device_url, "Play", "<InstanceID>0</InstanceID><Speed>1</Speed>")
            .await
            .map_err(|e| DlnaError::Playback(e.to_string()))?;

        self.is_playing = true;
        log::info!("DLNA: Play");
        Ok(())
    }

    /// Pause playback
    pub async fn pause(&mut self) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_service = self.av_transport_service.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        av_service
            .action(&self.device_url, "Pause", "<InstanceID>0</InstanceID>")
            .await
            .map_err(|e| DlnaError::Playback(e.to_string()))?;

        self.is_playing = false;
        log::info!("DLNA: Pause");
        Ok(())
    }

    /// Stop playback
    pub async fn stop(&mut self) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_service = self.av_transport_service.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        av_service
            .action(&self.device_url, "Stop", "<InstanceID>0</InstanceID>")
            .await
            .map_err(|e| DlnaError::Playback(e.to_string()))?;

        self.is_playing = false;
        self.current_uri = None;
        log::info!("DLNA: Stop");
        Ok(())
    }

    /// Seek to position
    pub async fn seek(&mut self, position_secs: u64) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let hours = position_secs / 3600;
        let minutes = (position_secs % 3600) / 60;
        let seconds = position_secs % 60;
        let time_str = format!("{:02}:{:02}:{:02}", hours, minutes, seconds);

        let av_service = self.av_transport_service.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        let payload = format!(
            "<InstanceID>0</InstanceID><Unit>REL_TIME</Unit><Target>{}</Target>",
            time_str
        );

        av_service
            .action(&self.device_url, "Seek", &payload)
            .await
            .map_err(|e| DlnaError::Playback(e.to_string()))?;

        log::info!("DLNA: Seek to {}", time_str);
        Ok(())
    }

    /// Set volume (0.0 - 1.0)
    pub async fn set_volume(&mut self, volume: f32) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let rc_service = self.rendering_control_service.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no RenderingControl service".to_string()))?;

        // DLNA volume is typically 0-100
        let dlna_volume = ((volume.clamp(0.0, 1.0) * 100.0) as u32).min(100);

        let payload = format!(
            "<InstanceID>0</InstanceID><Channel>Master</Channel><DesiredVolume>{}</DesiredVolume>",
            dlna_volume
        );

        rc_service
            .action(&self.device_url, "SetVolume", &payload)
            .await
            .map_err(|e| DlnaError::Playback(e.to_string()))?;

        log::info!("DLNA: Set volume to {}", dlna_volume);
        Ok(())
    }

}

/// Build DIDL-Lite metadata for a track
fn build_didl_metadata(uri: &str, metadata: &DlnaMetadata) -> String {
    let duration = metadata.duration_secs.map(|d| {
        let hours = d / 3600;
        let minutes = (d % 3600) / 60;
        let seconds = d % 60;
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }).unwrap_or_else(|| "00:00:00".to_string());

    let artwork = metadata.artwork_url.as_ref()
        .map(|url| format!(r#"<upnp:albumArtURI>{}</upnp:albumArtURI>"#, xml_escape(url)))
        .unwrap_or_default();

    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">
  <item id="0" parentID="-1" restricted="1">
    <dc:title>{}</dc:title>
    <dc:creator>{}</dc:creator>
    <upnp:album>{}</upnp:album>
    <upnp:artist>{}</upnp:artist>
    {}
    <res duration="{}" protocolInfo="http-get:*:audio/flac:*">{}</res>
    <upnp:class>object.item.audioItem.musicTrack</upnp:class>
  </item>
</DIDL-Lite>"#,
        xml_escape(&metadata.title),
        xml_escape(&metadata.artist),
        xml_escape(&metadata.album),
        xml_escape(&metadata.artist),
        artwork,
        duration,
        xml_escape(uri)
    )
}

/// Escape special XML characters
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn av_transport_urn() -> URN {
    URN::Service("schemas-upnp-org".into(), "AVTransport".into(), 1)
}

fn rendering_control_urn() -> URN {
    URN::Service("schemas-upnp-org".into(), "RenderingControl".into(), 1)
}
