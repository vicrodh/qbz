//! End-to-end volume round trip against a simulated UPnP MediaRenderer.
//!
//! Covers the actual failure the slider had: the app never asked the renderer
//! what its volume was, and assumed every renderer counts 0-100. The fake
//! device here deliberately declares a 0..31 scale so a percentage sent (or
//! read) verbatim is unmistakably wrong.

use std::sync::mpsc;

use qbz_cast::{DiscoveredDlnaDevice, DlnaConnection};

/// The renderer's declared scale — NOT 0-100, on purpose.
const RC_MAX: i64 = 31;
/// Where the fake renderer's volume sits when the app connects.
const CURRENT_VOLUME: i64 = 8;

fn device_description(port: u16) -> String {
    // Only RenderingControl: `DlnaConnection::connect` sends an AVTransport
    // Stop when that service exists, which this test has no use for.
    format!(
        r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <device>
    <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
    <friendlyName>Fake Renderer</friendlyName>
    <manufacturer>QBZ</manufacturer>
    <modelName>Test</modelName>
    <UDN>uuid:fake-renderer-{port}</UDN>
    <serviceList>
      <service>
        <serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>
        <serviceId>urn:upnp-org:serviceId:RenderingControl</serviceId>
        <SCPDURL>/rc.xml</SCPDURL>
        <controlURL>/rc/control</controlURL>
        <eventSubURL>/rc/event</eventSubURL>
      </service>
    </serviceList>
  </device>
</root>"#
    )
}

fn rendering_control_scpd() -> String {
    format!(
        r#"<?xml version="1.0"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <actionList>
    <action>
      <name>GetVolume</name>
      <argumentList>
        <argument>
          <name>CurrentVolume</name>
          <direction>out</direction>
          <relatedStateVariable>Volume</relatedStateVariable>
        </argument>
      </argumentList>
    </action>
    <action>
      <name>SetVolume</name>
      <argumentList>
        <argument>
          <name>DesiredVolume</name>
          <direction>in</direction>
          <relatedStateVariable>Volume</relatedStateVariable>
        </argument>
      </argumentList>
    </action>
  </actionList>
  <serviceStateTable>
    <stateVariable sendEvents="yes">
      <name>Volume</name>
      <dataType>ui2</dataType>
      <allowedValueRange>
        <minimum>0</minimum>
        <maximum>{RC_MAX}</maximum>
        <step>1</step>
      </allowedValueRange>
    </stateVariable>
  </serviceStateTable>
</scpd>"#
    )
}

fn soap_envelope(action: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:{action}Response xmlns:u="urn:schemas-upnp-org:service:RenderingControl:1">{body}</u:{action}Response>
  </s:Body>
</s:Envelope>"#
    )
}

/// Pull `<DesiredVolume>N</DesiredVolume>` out of a SetVolume request body.
fn desired_volume(body: &str) -> Option<i64> {
    let start = body.find("<DesiredVolume>")? + "<DesiredVolume>".len();
    let end = body[start..].find("</DesiredVolume>")? + start;
    body[start..end].trim().parse().ok()
}

/// A minimal UPnP renderer: serves its description + RenderingControl SCPD and
/// answers GetVolume/SetVolume. Every SetVolume value it receives is reported
/// back over `set_volumes`.
fn spawn_fake_renderer() -> (String, mpsc::Receiver<i64>) {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("bind fake renderer");
    let port = server.server_addr().to_ip().expect("ip addr").port();
    let (set_tx, set_rx) = mpsc::channel();

    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let url = request.url().to_string();
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);

            let response = if url.starts_with("/rc.xml") {
                rendering_control_scpd()
            } else if url.starts_with("/rc/control") {
                if body.contains("SetVolume") {
                    if let Some(v) = desired_volume(&body) {
                        let _ = set_tx.send(v);
                    }
                    soap_envelope("SetVolume", "")
                } else {
                    soap_envelope(
                        "GetVolume",
                        &format!("<CurrentVolume>{CURRENT_VOLUME}</CurrentVolume>"),
                    )
                }
            } else {
                device_description(port)
            };

            let header = "Content-Type: text/xml; charset=\"utf-8\""
                .parse::<tiny_http::Header>()
                .expect("valid header");
            let _ = request.respond(tiny_http::Response::from_string(response).with_header(header));
        }
    });

    (format!("http://127.0.0.1:{port}/desc.xml"), set_rx)
}

fn fake_device(url: String) -> DiscoveredDlnaDevice {
    DiscoveredDlnaDevice {
        id: "uuid:fake-renderer".to_string(),
        name: "Fake Renderer".to_string(),
        manufacturer: "QBZ".to_string(),
        model: "Test".to_string(),
        ip: "127.0.0.1".to_string(),
        url,
        has_av_transport: false,
        has_rendering_control: true,
    }
}

#[tokio::test]
async fn reads_and_writes_volume_on_the_devices_own_scale() {
    let (url, set_volumes) = spawn_fake_renderer();
    let mut conn = DlnaConnection::connect(fake_device(url))
        .await
        .expect("connect to fake renderer");

    // READ: 8 of 31 is ~26% — not "8%", which is what a hard-coded percent
    // scale would have reported, and not the app's own volume either.
    let volume = conn.get_volume().await.expect("GetVolume");
    let expected = CURRENT_VOLUME as f32 / RC_MAX as f32;
    assert!(
        (volume - expected).abs() < 1e-3,
        "expected ~{expected:.3}, got {volume:.3}"
    );

    // WRITE: half way up the slider must land half way up the DEVICE's scale
    // (16 of 31), not at "50" — which this renderer would have clamped to full.
    conn.set_volume(0.5).await.expect("SetVolume");
    let sent = set_volumes.recv().expect("renderer received SetVolume");
    assert_eq!(sent, 16, "0.5 on a 0..{RC_MAX} renderer");

    // The extremes must reach the real endpoints.
    conn.set_volume(1.0).await.expect("SetVolume max");
    assert_eq!(set_volumes.recv().expect("SetVolume max"), RC_MAX);
    conn.set_volume(0.0).await.expect("SetVolume min");
    assert_eq!(set_volumes.recv().expect("SetVolume min"), 0);
}
