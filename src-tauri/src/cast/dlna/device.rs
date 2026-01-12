//! DLNA device connection and playback via AVTransport SOAP

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
    client: Client,
    // Service control URLs (discovered from device description)
    av_transport_url: Option<String>,
    rendering_control_url: Option<String>,
    // Current media URI
    current_uri: Option<String>,
    is_playing: bool,
}

impl DlnaConnection {
    /// Connect to a DLNA device and discover service URLs
    pub fn connect(device: DiscoveredDlnaDevice) -> Result<Self, DlnaError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| DlnaError::Connection(format!("Failed to create HTTP client: {}", e)))?;

        // For now, construct URLs based on device URL
        // The actual control URLs would come from parsing the device description XML
        let base_url = device.url.trim_end_matches('/');

        // Common UPnP control URL patterns
        let av_transport_url = if device.has_av_transport {
            Some(format!("{}/AVTransport/control", base_url))
        } else {
            None
        };

        let rendering_control_url = if device.has_rendering_control {
            Some(format!("{}/RenderingControl/control", base_url))
        } else {
            None
        };

        log::info!(
            "DLNA: Connected to {} (AVT: {:?}, RC: {:?})",
            device.name,
            av_transport_url.is_some(),
            rendering_control_url.is_some()
        );

        Ok(Self {
            device,
            connected: true,
            client,
            av_transport_url,
            rendering_control_url,
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

        let av_url = self.av_transport_url.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        // Build DIDL-Lite metadata
        let didl_metadata = build_didl_metadata(uri, metadata);

        // SetAVTransportURI SOAP action
        let soap_body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetAVTransportURI xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <InstanceID>0</InstanceID>
      <CurrentURI>{}</CurrentURI>
      <CurrentURIMetaData>{}</CurrentURIMetaData>
    </u:SetAVTransportURI>
  </s:Body>
</s:Envelope>"#,
            xml_escape(uri),
            xml_escape(&didl_metadata)
        );

        self.send_soap_action(
            av_url,
            "urn:schemas-upnp-org:service:AVTransport:1#SetAVTransportURI",
            &soap_body,
        ).await?;

        self.current_uri = Some(uri.to_string());
        log::info!("DLNA: Set URI to {}", uri);

        Ok(())
    }

    /// Start/resume playback
    pub async fn play(&mut self) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_url = self.av_transport_url.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        let soap_body = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Play xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <InstanceID>0</InstanceID>
      <Speed>1</Speed>
    </u:Play>
  </s:Body>
</s:Envelope>"#;

        self.send_soap_action(
            av_url,
            "urn:schemas-upnp-org:service:AVTransport:1#Play",
            soap_body,
        ).await?;

        self.is_playing = true;
        log::info!("DLNA: Play");
        Ok(())
    }

    /// Pause playback
    pub async fn pause(&mut self) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_url = self.av_transport_url.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        let soap_body = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Pause xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <InstanceID>0</InstanceID>
    </u:Pause>
  </s:Body>
</s:Envelope>"#;

        self.send_soap_action(
            av_url,
            "urn:schemas-upnp-org:service:AVTransport:1#Pause",
            soap_body,
        ).await?;

        self.is_playing = false;
        log::info!("DLNA: Pause");
        Ok(())
    }

    /// Stop playback
    pub async fn stop(&mut self) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_url = self.av_transport_url.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        let soap_body = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Stop xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <InstanceID>0</InstanceID>
    </u:Stop>
  </s:Body>
</s:Envelope>"#;

        self.send_soap_action(
            av_url,
            "urn:schemas-upnp-org:service:AVTransport:1#Stop",
            soap_body,
        ).await?;

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

        let av_url = self.av_transport_url.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        let hours = position_secs / 3600;
        let minutes = (position_secs % 3600) / 60;
        let seconds = position_secs % 60;
        let time_str = format!("{:02}:{:02}:{:02}", hours, minutes, seconds);

        let soap_body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Seek xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <InstanceID>0</InstanceID>
      <Unit>REL_TIME</Unit>
      <Target>{}</Target>
    </u:Seek>
  </s:Body>
</s:Envelope>"#,
            time_str
        );

        self.send_soap_action(
            av_url,
            "urn:schemas-upnp-org:service:AVTransport:1#Seek",
            &soap_body,
        ).await?;

        log::info!("DLNA: Seek to {}", time_str);
        Ok(())
    }

    /// Set volume (0.0 - 1.0)
    pub async fn set_volume(&mut self, volume: f32) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let rc_url = self.rendering_control_url.as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no RenderingControl service".to_string()))?;

        // DLNA volume is typically 0-100
        let dlna_volume = ((volume.clamp(0.0, 1.0) * 100.0) as u32).min(100);

        let soap_body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetVolume xmlns:u="urn:schemas-upnp-org:service:RenderingControl:1">
      <InstanceID>0</InstanceID>
      <Channel>Master</Channel>
      <DesiredVolume>{}</DesiredVolume>
    </u:SetVolume>
  </s:Body>
</s:Envelope>"#,
            dlna_volume
        );

        self.send_soap_action(
            rc_url,
            "urn:schemas-upnp-org:service:RenderingControl:1#SetVolume",
            &soap_body,
        ).await?;

        log::info!("DLNA: Set volume to {}", dlna_volume);
        Ok(())
    }

    /// Send a SOAP action request
    async fn send_soap_action(
        &self,
        url: &str,
        action: &str,
        body: &str,
    ) -> Result<String, DlnaError> {
        let response = self.client
            .post(url)
            .header("Content-Type", "text/xml; charset=utf-8")
            .header("SOAPAction", format!("\"{}\"", action))
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| DlnaError::Playback(format!("SOAP request failed: {}", e)))?;

        let status = response.status();
        let body = response.text().await
            .map_err(|e| DlnaError::Playback(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            log::error!("DLNA SOAP error ({}): {}", status, body);
            return Err(DlnaError::Playback(format!("SOAP error ({}): {}", status, parse_soap_error(&body))));
        }

        Ok(body)
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

/// Parse error message from SOAP fault response
fn parse_soap_error(body: &str) -> String {
    // Try to extract faultstring or errorDescription
    if let Some(start) = body.find("<faultstring>") {
        if let Some(end) = body.find("</faultstring>") {
            return body[start + 13..end].to_string();
        }
    }
    if let Some(start) = body.find("<errorDescription>") {
        if let Some(end) = body.find("</errorDescription>") {
            return body[start + 18..end].to_string();
        }
    }
    "Unknown error".to_string()
}
