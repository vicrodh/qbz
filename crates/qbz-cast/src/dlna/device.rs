//! DLNA device connection and playback via AVTransport SOAP

use rupnp::http::Uri;
use rupnp::{Device, Service};
use serde::{Deserialize, Serialize};

use crate::dlna::DiscoveredDlnaDevice;
use crate::DlnaError;

/// Metadata for DLNA playback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlnaMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork_url: Option<String>,
    pub duration_secs: Option<u64>,
}

/// DLNA playback position info
#[derive(Debug, Clone, Serialize)]
pub struct DlnaPositionInfo {
    pub position_secs: u64,
    pub duration_secs: u64,
    pub transport_state: String, // PLAYING, PAUSED_PLAYBACK, STOPPED, etc.
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
    // Last SetAVTransportURI payload (URI + DIDL). Kept so `play()` can
    // re-assert the content on a 702 "no contents" fault — HQPlayer6 clears
    // CurrentURI when a track ends, which would otherwise make a bare Play
    // (the manual play button, or a late auto-advance) fail permanently.
    last_set_uri_payload: Option<String>,
    // Set by `load_media`, cleared by `play`. When true the URI was just set, so
    // `play` skips its idle pre-check (the content is already current) and avoids
    // a redundant SetAVTransportURI. A bare play (manual button / resume) leaves
    // it false so the pre-check runs.
    uri_freshly_set: bool,
    is_playing: bool,
}

impl DlnaConnection {
    /// Connect to a DLNA device and discover service URLs
    pub async fn connect(device: DiscoveredDlnaDevice) -> Result<Self, DlnaError> {
        // Defensive: the device-description fetch may go over TLS in some
        // setups; ensure a rustls CryptoProvider is installed (idempotent).
        crate::ensure_crypto_provider();
        let device_url: Uri = device
            .url
            .parse()
            .map_err(|e| DlnaError::Connection(format!("Invalid device URL: {}", e)))?;

        let parsed_device = Device::from_url(device_url.clone()).await.map_err(|e| {
            DlnaError::Connection(format!("Failed to load device description: {}", e))
        })?;

        let av_transport_service = find_service_any_version(&parsed_device, "AVTransport");
        let rendering_control_service =
            find_service_any_version(&parsed_device, "RenderingControl");

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
            last_set_uri_payload: None,
            uri_freshly_set: false,
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
    pub async fn load_media(
        &mut self,
        uri: &str,
        metadata: &DlnaMetadata,
        content_type: &str,
    ) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_service = self
            .av_transport_service
            .as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        // Build DIDL-Lite metadata with actual content type
        let didl_metadata = build_didl_metadata(uri, metadata, content_type);

        log::info!("DLNA: Loading media URI: {}", uri);
        log::info!("DLNA: Content-Type: {}", content_type);
        log::info!("DLNA: DIDL Metadata:\n{}", didl_metadata);

        let payload = format!(
            "<InstanceID>0</InstanceID><CurrentURI>{}</CurrentURI><CurrentURIMetaData>{}</CurrentURIMetaData>",
            xml_escape(uri),
            xml_escape(&didl_metadata)
        );

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            av_service.action(&self.device_url, "SetAVTransportURI", &payload),
        )
        .await
        .map_err(|_| {
            log::error!("DLNA: SetAVTransportURI timed out after 10s");
            DlnaError::Playback("SetAVTransportURI timed out".to_string())
        })?
        .map_err(|e| {
            log::error!("DLNA: SetAVTransportURI failed: {}", e);
            DlnaError::Playback(e.to_string())
        })?;

        log::info!("DLNA: SetAVTransportURI response: {:?}", response);
        self.current_uri = Some(uri.to_string());
        // Remember the exact payload so play() can re-assert it if the renderer
        // later reports 702 "no contents" (see the retry loop in `play`).
        self.last_set_uri_payload = Some(payload);
        self.uri_freshly_set = true;
        log::info!("DLNA: Set URI to {}", uri);

        Ok(())
    }

    /// Run a SOAP action with a timeout. A hung renderer maps to
    /// `DlnaError::Timeout` instead of blocking the caller forever — closes the
    /// gap where pause/stop/seek/set_volume had no timeout at all.
    async fn run_action(
        service: &Service,
        device_url: &Uri,
        name: &str,
        payload: &str,
        timeout_secs: u64,
    ) -> Result<std::collections::HashMap<String, String>, DlnaError> {
        tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            service.action(device_url, name, payload),
        )
        .await
        .map_err(|_| {
            log::error!("DLNA: {name} action timed out after {timeout_secs}s");
            DlnaError::Timeout(format!("{name} action timed out"))
        })?
        .map_err(|e| {
            log::error!("DLNA: {name} action failed: {e}");
            DlnaError::Playback(e.to_string())
        })
    }

    /// Start/resume playback
    pub async fn play(&mut self) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        // Clone the service handle so the retry loop below doesn't hold an
        // immutable `&self` borrow across the `&mut self` write of
        // `self.is_playing` on success. `rupnp::Service` is `Clone`.
        let av_service = self
            .av_transport_service
            .as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?
            .clone();

        // Snapshot the last SetAVTransportURI payload up front so we can (re-)assert
        // it without holding a `&self` borrow across the loop below.
        let set_uri_payload = self.last_set_uri_payload.clone();

        // PRE-CHECK — the core of the strict-renderer fix. When a track reaches its
        // natural end HQPlayer6 finalises/clears the transport, so a *bare* Play at
        // that point (the manual play button, or a resume after a stop) either
        // faults 702 "No contents" or 200-OKs into a silent no-op (the "it just
        // stops, no error, won't play" symptom). Both are cured the way the manual
        // recovery works: re-assert the current URI so Play acts on fresh content
        // (SetAVTransportURI + Play — the sequence that reliably starts a track).
        // Only do this when the URI was NOT just set (skip the redundant SetURI on
        // the auto-advance path, whose settle-race 702 the retry net below still
        // covers) and the transport is idle (STOPPED / NO_MEDIA_PRESENT); a PAUSED
        // transport is left untouched so pause→resume keeps its position.
        let uri_freshly_set = std::mem::replace(&mut self.uri_freshly_set, false);
        if !uri_freshly_set {
            if let Some(payload) = set_uri_payload.as_deref() {
                if Self::transport_is_idle(&av_service, &self.device_url).await {
                    log::info!("DLNA: transport idle before Play; re-asserting SetAVTransportURI");
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        av_service.action(&self.device_url, "SetAVTransportURI", payload),
                    )
                    .await;
                }
            }
        }

        // Retry net for the residual settle race. AVTransport faults at a boundary:
        //   701 "Transition not available" — pure settle race; wait and retry Play.
        //   702 "No contents" — empty transport; re-assert the URI, then retry.
        // HQPlayer6 returns these as a raw HTTP status line (e.g. "702"), which
        // rupnp surfaces as `HttpErrorCode` before it parses a SOAP body;
        // spec-compliant renderers return a SOAP Fault parsed into `UPnPError`.
        // Match both. A renderer that accepts Play immediately sees a single attempt.
        const PLAY_SETTLE_CODE: u16 = 701;
        const PLAY_NO_CONTENT_CODE: u16 = 702;
        const PLAY_MAX_ATTEMPTS: u32 = 6;

        for attempt in 1..=PLAY_MAX_ATTEMPTS {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                av_service.action(
                    &self.device_url,
                    "Play",
                    "<InstanceID>0</InstanceID><Speed>1</Speed>",
                ),
            )
            .await
            .map_err(|_| {
                log::error!("DLNA: Play action timed out after 10s");
                DlnaError::Playback("Play action timed out".to_string())
            })?;

            let e = match result {
                Ok(response) => {
                    log::info!("DLNA: Play response: {:?}", response);
                    self.is_playing = true;
                    log::info!("DLNA: Play started successfully");
                    return Ok(());
                }
                Err(e) => e,
            };

            let code = match &e {
                rupnp::Error::UPnPError(u) => Some(u.err_code()),
                rupnp::Error::HttpErrorCode(c) => Some(c.as_u16()),
                _ => None,
            };
            let reassert_uri = match code {
                Some(PLAY_NO_CONTENT_CODE) => true,
                Some(PLAY_SETTLE_CODE) => false,
                _ => {
                    log::error!("DLNA: Play action failed: {}", e);
                    return Err(DlnaError::Playback(e.to_string()));
                }
            };

            if attempt == PLAY_MAX_ATTEMPTS {
                log::error!("DLNA: Play still faulting after {PLAY_MAX_ATTEMPTS} attempts: {e}");
                return Err(DlnaError::Playback(e.to_string()));
            }

            if reassert_uri {
                if let Some(payload) = set_uri_payload.as_deref() {
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        av_service.action(&self.device_url, "SetAVTransportURI", payload),
                    )
                    .await;
                }
            }

            // Linear backoff: ~300 ms * attempt, so playback typically recovers
            // within ~1–2 s and gives up after ~4.5 s worst case.
            let backoff = std::time::Duration::from_millis(300 * attempt as u64);
            log::warn!(
                "DLNA: Play returned transient UPnP fault ({e}); \
                 retry {attempt}/{PLAY_MAX_ATTEMPTS} after {backoff:?}"
            );
            tokio::time::sleep(backoff).await;
        }

        // Unreachable: the loop returns on success or on the final attempt.
        unreachable!("play retry loop exited without returning")
    }

    /// True only if the renderer's transport is idle (STOPPED / NO_MEDIA_PRESENT).
    /// On any query error/timeout returns `false` — "not known to be idle" — so we
    /// never re-assert the URI under uncertainty and disturb a paused resume.
    async fn transport_is_idle(av_service: &Service, device_url: &Uri) -> bool {
        match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            av_service.action(device_url, "GetTransportInfo", "<InstanceID>0</InstanceID>"),
        )
        .await
        {
            Ok(Ok(resp)) => matches!(
                resp.get("CurrentTransportState")
                    .map(|s| s.trim().to_ascii_uppercase())
                    .as_deref(),
                Some("STOPPED") | Some("NO_MEDIA_PRESENT")
            ),
            _ => false,
        }
    }

    /// Pause playback
    pub async fn pause(&mut self) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_service = self
            .av_transport_service
            .as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        Self::run_action(
            av_service,
            &self.device_url,
            "Pause",
            "<InstanceID>0</InstanceID>",
            10,
        )
        .await?;

        self.is_playing = false;
        log::info!("DLNA: Pause");
        Ok(())
    }

    /// Stop playback
    pub async fn stop(&mut self) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_service = self
            .av_transport_service
            .as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        Self::run_action(
            av_service,
            &self.device_url,
            "Stop",
            "<InstanceID>0</InstanceID>",
            10,
        )
        .await?;

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

        let av_service = self
            .av_transport_service
            .as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        let payload = format!(
            "<InstanceID>0</InstanceID><Unit>REL_TIME</Unit><Target>{}</Target>",
            time_str
        );

        Self::run_action(av_service, &self.device_url, "Seek", &payload, 10).await?;

        log::info!("DLNA: Seek to {}", time_str);
        Ok(())
    }

    /// Set volume (0.0 - 1.0)
    pub async fn set_volume(&mut self, volume: f32) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let rc_service = self.rendering_control_service.as_ref().ok_or_else(|| {
            DlnaError::Playback("Device has no RenderingControl service".to_string())
        })?;

        // DLNA volume is typically 0-100
        let dlna_volume = ((volume.clamp(0.0, 1.0) * 100.0) as u32).min(100);

        let payload = format!(
            "<InstanceID>0</InstanceID><Channel>Master</Channel><DesiredVolume>{}</DesiredVolume>",
            dlna_volume
        );

        Self::run_action(rc_service, &self.device_url, "SetVolume", &payload, 10).await?;

        log::info!("DLNA: Set volume to {}", dlna_volume);
        Ok(())
    }

    /// Set mute on/off (RenderingControl SetMute, Master channel). Companion to
    /// `set_volume` — was missing from the crate.
    pub async fn set_mute(&mut self, mute: bool) -> Result<(), DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let rc_service = self.rendering_control_service.as_ref().ok_or_else(|| {
            DlnaError::Playback("Device has no RenderingControl service".to_string())
        })?;

        let payload = format!(
            "<InstanceID>0</InstanceID><Channel>Master</Channel><DesiredMute>{}</DesiredMute>",
            if mute { 1 } else { 0 }
        );

        Self::run_action(rc_service, &self.device_url, "SetMute", &payload, 10).await?;

        log::info!("DLNA: Set mute to {}", mute);
        Ok(())
    }

    /// Query current mute state (RenderingControl GetMute, Master channel).
    pub async fn get_mute(&self) -> Result<bool, DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let rc_service = self.rendering_control_service.as_ref().ok_or_else(|| {
            DlnaError::Playback("Device has no RenderingControl service".to_string())
        })?;

        let response = Self::run_action(
            rc_service,
            &self.device_url,
            "GetMute",
            "<InstanceID>0</InstanceID><Channel>Master</Channel>",
            5,
        )
        .await?;

        let muted = response
            .get("CurrentMute")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Ok(muted)
    }

    /// Get current playback position and transport state
    pub async fn get_position_info(&self) -> Result<DlnaPositionInfo, DlnaError> {
        if !self.connected {
            return Err(DlnaError::NotConnected);
        }

        let av_service = self
            .av_transport_service
            .as_ref()
            .ok_or_else(|| DlnaError::Playback("Device has no AVTransport service".to_string()))?;

        // Get position info
        let position_response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            av_service.action(
                &self.device_url,
                "GetPositionInfo",
                "<InstanceID>0</InstanceID>",
            ),
        )
        .await
        .map_err(|_| DlnaError::Playback("GetPositionInfo timed out".to_string()))?
        .map_err(|e| DlnaError::Playback(e.to_string()))?;

        // Get transport state
        let transport_response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            av_service.action(
                &self.device_url,
                "GetTransportInfo",
                "<InstanceID>0</InstanceID>",
            ),
        )
        .await
        .map_err(|_| DlnaError::Playback("GetTransportInfo timed out".to_string()))?
        .map_err(|e| DlnaError::Playback(e.to_string()))?;

        // Parse RelTime (position) - format: "HH:MM:SS" or "H:MM:SS"
        let rel_time = position_response
            .get("RelTime")
            .map(|s| s.as_str())
            .unwrap_or("0:00:00");
        let position_secs = parse_time_string(rel_time);

        // Parse TrackDuration - format: "HH:MM:SS"
        let track_duration = position_response
            .get("TrackDuration")
            .map(|s| s.as_str())
            .unwrap_or("0:00:00");
        let duration_secs = parse_time_string(track_duration);

        // Get transport state (PLAYING, PAUSED_PLAYBACK, STOPPED, etc.)
        let transport_state = transport_response
            .get("CurrentTransportState")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "UNKNOWN".to_string());

        Ok(DlnaPositionInfo {
            position_secs,
            duration_secs,
            transport_state,
        })
    }
}

/// Parse time string "HH:MM:SS" or "H:MM:SS" to seconds
fn parse_time_string(time: &str) -> u64 {
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() != 3 {
        return 0;
    }

    let hours: u64 = parts[0].parse().unwrap_or(0);
    let minutes: u64 = parts[1].parse().unwrap_or(0);
    let seconds: u64 = parts[2]
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    hours * 3600 + minutes * 60 + seconds
}

/// Build DIDL-Lite metadata for a track
fn build_didl_metadata(uri: &str, metadata: &DlnaMetadata, content_type: &str) -> String {
    let duration = metadata
        .duration_secs
        .map(|d| {
            let hours = d / 3600;
            let minutes = (d % 3600) / 60;
            let seconds = d % 60;
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        })
        .unwrap_or_else(|| "00:00:00".to_string());

    let artwork = metadata
        .artwork_url
        .as_ref()
        .map(|url| {
            format!(
                r#"<upnp:albumArtURI>{}</upnp:albumArtURI>"#,
                xml_escape(url)
            )
        })
        .unwrap_or_default();

    // Use actual content type for protocolInfo - critical for DLNA compatibility
    // Many devices reject content if protocolInfo doesn't match actual MIME type.
    // The 4th field advertises the same DLNA content features the media server
    // sends on GET/HEAD (see `media_server::DLNA_CONTENT_FEATURES`); strict
    // renderers cross-check these against the response headers.
    let protocol_info = format!(
        "http-get:*:{}:{}",
        content_type,
        crate::media_server::DLNA_CONTENT_FEATURES
    );

    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">
  <item id="0" parentID="-1" restricted="1">
    <dc:title>{}</dc:title>
    <dc:creator>{}</dc:creator>
    <upnp:album>{}</upnp:album>
    <upnp:artist>{}</upnp:artist>
    {}
    <res duration="{}" protocolInfo="{}">{}</res>
    <upnp:class>object.item.audioItem.musicTrack</upnp:class>
  </item>
</DIDL-Lite>"#,
        xml_escape(&metadata.title),
        xml_escape(&metadata.artist),
        xml_escape(&metadata.album),
        xml_escape(&metadata.artist),
        artwork,
        duration,
        protocol_info,
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

/// Version-agnostic service-type match: `true` when `service_type` names
/// `service` at any UPnP version. Shared by [`find_service_any_version`] so the
/// rule can be unit-tested without constructing a real rupnp `Device`.
fn service_type_matches(service_type: &str, service: &str) -> bool {
    service_type.contains(&format!(":service:{}:", service))
}

/// Find a service by name regardless of its UPnP version (`:1`/`:2`/`:3`),
/// matching discovery's substring logic so a `:2`/`:3`-only renderer connects.
fn find_service_any_version(device: &Device, service: &str) -> Option<Service> {
    device
        .services_iter()
        .find(|s| service_type_matches(&s.service_type().to_string(), service))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::service_type_matches;

    #[test]
    fn matches_any_upnp_version() {
        for st in [
            "urn:schemas-upnp-org:service:AVTransport:1",
            "urn:schemas-upnp-org:service:AVTransport:2",
            "urn:schemas-upnp-org:service:AVTransport:3",
        ] {
            assert!(
                service_type_matches(st, "AVTransport"),
                "expected {st} to match AVTransport"
            );
        }
    }

    #[test]
    fn rejects_unrelated_service() {
        assert!(!service_type_matches(
            "urn:schemas-upnp-org:service:ConnectionManager:1",
            "AVTransport"
        ));
    }
}
