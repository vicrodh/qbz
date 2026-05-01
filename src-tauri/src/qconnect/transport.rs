//! Transport setup, device identity, and credential discovery for the
//! QConnect WebSocket. Owns hostname resolution, the persisted custom
//! device name (in `qconnect_settings.db`), the qws/createToken
//! handshake, hex helpers used for inbound payload diagnostics, and the
//! once-initialized device UUID.

use std::sync::OnceLock;

use qconnect_transport_ws::WsTransportConfig;
use serde_json::Value;
use uuid::Uuid;

use crate::AppState;

use super::{
    QconnectConnectOptions, QconnectDeviceCapabilitiesPayload, QconnectDeviceInfoPayload,
};

use super::{AUDIO_QUALITY_HIRES_LEVEL2, AUDIO_QUALITY_MP3};

const DEFAULT_QCONNECT_DEVICE_BRAND: &str = "QBZ";
const DEFAULT_QCONNECT_DEVICE_MODEL: &str = "QBZ";
const DEFAULT_QCONNECT_DEVICE_TYPE: i32 = 5; // computer
const DEFAULT_QCONNECT_SOFTWARE_PREFIX: &str = "qbz";
const VOLUME_REMOTE_CONTROL_ALLOWED: i32 = 2;
const QCONNECT_QWS_TOKEN_KIND: &str = "jwt_qws";
const QCONNECT_QWS_CREATE_TOKEN_PATH: &str = "/qws/createToken";

static QCONNECT_DEVICE_UUID: OnceLock<String> = OnceLock::new();

pub(super) fn default_qconnect_device_info() -> QconnectDeviceInfoPayload {
    default_qconnect_device_info_with_name(None)
}

pub(super) fn default_qconnect_device_info_with_name(
    custom_name: Option<&str>,
) -> QconnectDeviceInfoPayload {
    let friendly_name = custom_name
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            std::env::var("QBZ_QCONNECT_DEVICE_NAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(resolve_default_qconnect_device_name);
    let brand = std::env::var("QBZ_QCONNECT_DEVICE_BRAND")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_QCONNECT_DEVICE_BRAND.to_string());
    let model = std::env::var("QBZ_QCONNECT_DEVICE_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_QCONNECT_DEVICE_MODEL.to_string());
    let software_version = std::env::var("QBZ_QCONNECT_SOFTWARE_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "{DEFAULT_QCONNECT_SOFTWARE_PREFIX}/{}",
                env!("CARGO_PKG_VERSION")
            )
        });
    let device_type = std::env::var("QBZ_QCONNECT_DEVICE_TYPE")
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .unwrap_or(DEFAULT_QCONNECT_DEVICE_TYPE);

    QconnectDeviceInfoPayload {
        device_uuid: Some(resolve_qconnect_device_uuid()),
        friendly_name: Some(friendly_name),
        brand: Some(brand),
        model: Some(model),
        serial_number: None,
        device_type: Some(device_type),
        capabilities: Some(QconnectDeviceCapabilitiesPayload {
            min_audio_quality: Some(AUDIO_QUALITY_MP3),
            max_audio_quality: Some(AUDIO_QUALITY_HIRES_LEVEL2),
            volume_remote_control: Some(VOLUME_REMOTE_CONTROL_ALLOWED),
        }),
        software_version: Some(software_version),
    }
}

pub(super) fn resolve_qconnect_device_uuid() -> String {
    if let Some(explicit) = std::env::var("QBZ_QCONNECT_DEVICE_UUID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return explicit;
    }

    QCONNECT_DEVICE_UUID
        .get_or_init(|| Uuid::new_v4().to_string())
        .clone()
}

pub(super) fn resolve_system_hostname() -> String {
    // Try HOSTNAME env var first
    if let Ok(h) = std::env::var("HOSTNAME") {
        if !h.trim().is_empty() {
            return h.trim().to_string();
        }
    }
    // Try /etc/hostname
    if let Ok(h) = std::fs::read_to_string("/etc/hostname") {
        let trimmed = h.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    // Fallback
    "Desktop".to_string()
}

/// Returns "Qbz - {hostname}" as the default device name.
pub(super) fn resolve_default_qconnect_device_name() -> String {
    let hostname = resolve_system_hostname();
    format!("Qbz - {hostname}")
}

/// Path to the QConnect settings database (global, not per-user).
fn qconnect_settings_db_path() -> Option<std::path::PathBuf> {
    let data_dir = dirs::data_dir()?.join("qbz");
    std::fs::create_dir_all(&data_dir).ok()?;
    Some(data_dir.join("qconnect_settings.db"))
}

/// Load the persisted custom device name from disk.
/// Returns None if not set or on any error (fail-open).
pub(super) fn load_persisted_device_name() -> Option<String> {
    let db_path = qconnect_settings_db_path()?;
    let conn = rusqlite::Connection::open(&db_path).ok()?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .ok()?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    )
    .ok()?;
    conn.query_row(
        "SELECT value FROM settings WHERE key = 'device_name'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .filter(|v| !v.trim().is_empty())
}

/// Persist the custom device name to disk. None clears it.
pub(super) fn persist_device_name(name: Option<&str>) {
    let Some(db_path) = qconnect_settings_db_path() else {
        return;
    };
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return;
    };
    let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;");
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    );
    match name {
        Some(n) => {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('device_name', ?1)",
                rusqlite::params![n],
            );
        }
        None => {
            let _ = conn.execute("DELETE FROM settings WHERE key = 'device_name'", []);
        }
    }
}

pub(crate) async fn resolve_transport_config(
    options: QconnectConnectOptions,
    app_state: &AppState,
) -> Result<WsTransportConfig, String> {
    let mut endpoint_url = normalize_opt_string(options.endpoint_url)
        .or_else(|| normalize_opt_string(std::env::var("QBZ_QCONNECT_WS_ENDPOINT").ok()));

    let mut jwt_qws = normalize_opt_string(options.jwt_qws)
        .or_else(|| normalize_opt_string(std::env::var("QBZ_QCONNECT_JWT_QWS").ok()))
        .or_else(|| normalize_opt_string(std::env::var("QBZ_QCONNECT_JWT").ok()));

    if endpoint_url.is_none() || jwt_qws.is_none() {
        match fetch_qconnect_transport_credentials(app_state).await {
            Ok((discovered_endpoint, discovered_jwt_qws)) => {
                endpoint_url = endpoint_url.or(discovered_endpoint);
                jwt_qws = jwt_qws.or(discovered_jwt_qws);
            }
            Err(err) if endpoint_url.is_some() => {
                log::warn!(
                    "[QConnect] qws/createToken auto-discovery failed, using provided endpoint: {err}"
                );
            }
            Err(err) => {
                return Err(format!(
                    "QConnect endpoint_url is required (arg or QBZ_QCONNECT_WS_ENDPOINT). Auto-discovery via qws/createToken failed: {err}"
                ));
            }
        }
    }

    let endpoint_url = endpoint_url.ok_or_else(|| {
        "QConnect endpoint_url is required (arg or QBZ_QCONNECT_WS_ENDPOINT)".to_string()
    })?;

    let subscribe_channels = if let Some(channels) = options.subscribe_channels_hex {
        parse_subscribe_channels(channels)?
    } else if let Ok(raw) = std::env::var("QBZ_QCONNECT_SUBSCRIBE_CHANNELS_HEX") {
        let channels: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect();
        parse_subscribe_channels(channels)?
    } else {
        // Default QConnect channels: connectionId(0x01), backend(0x02), controllers(0x03)
        vec![vec![0x01], vec![0x02], vec![0x03]]
    };

    let mut config = WsTransportConfig::default();
    config.endpoint_url = endpoint_url;
    config.jwt_qws = jwt_qws;
    config.reconnect_backoff_ms = options
        .reconnect_backoff_ms
        .unwrap_or(config.reconnect_backoff_ms);
    config.reconnect_backoff_max_ms = options
        .reconnect_backoff_max_ms
        .unwrap_or(config.reconnect_backoff_max_ms);
    config.connect_timeout_ms = options
        .connect_timeout_ms
        .unwrap_or(config.connect_timeout_ms);
    config.keepalive_interval_ms = options
        .keepalive_interval_ms
        .unwrap_or(config.keepalive_interval_ms);
    config.qcloud_proto = options.qcloud_proto.unwrap_or(config.qcloud_proto);
    config.subscribe_channels = subscribe_channels;

    Ok(config)
}

async fn fetch_qconnect_transport_credentials(
    app_state: &AppState,
) -> Result<(Option<String>, Option<String>), String> {
    let client = app_state.client.read().await.clone();
    let app_id = client
        .app_id()
        .await
        .map_err(|err| format!("qws/createToken requires initialized API client: {err}"))?;
    let user_auth_token = client
        .auth_token()
        .await
        .map_err(|err| format!("qws/createToken requires authenticated user: {err}"))?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "X-App-Id",
        reqwest::header::HeaderValue::from_str(&app_id)
            .map_err(|_| "invalid X-App-Id header value".to_string())?,
    );
    headers.insert(
        "X-User-Auth-Token",
        reqwest::header::HeaderValue::from_str(&user_auth_token)
            .map_err(|_| "invalid X-User-Auth-Token header value".to_string())?,
    );

    let url = crate::api::endpoints::build_url(QCONNECT_QWS_CREATE_TOKEN_PATH);
    let response = client
        .get_http()
        .post(&url)
        .headers(headers)
        .form(&[
            ("jwt", QCONNECT_QWS_TOKEN_KIND),
            ("user_auth_token_needed", "true"),
            ("strong_auth_needed", "true"),
        ])
        .send()
        .await
        .map_err(|err| format!("qws/createToken HTTP request failed: {err}"))?;

    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|err| format!("qws/createToken response decode failed: {err}"))?;

    if !status.is_success() {
        let preview = serde_json::to_string(&payload)
            .unwrap_or_else(|_| "<unserializable>".to_string())
            .chars()
            .take(300)
            .collect::<String>();
        return Err(format!("qws/createToken status {status}: {preview}"));
    }

    let jwt_qws_payload = payload
        .get("jwt_qws")
        .ok_or_else(|| "qws/createToken response missing jwt_qws payload".to_string())?;

    let endpoint_url = normalize_opt_string(
        jwt_qws_payload
            .get("endpoint")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    );
    let jwt_qws = normalize_opt_string(
        jwt_qws_payload
            .get("jwt")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    );

    if endpoint_url.is_none() {
        return Err("qws/createToken response missing jwt_qws.endpoint".to_string());
    }

    Ok((endpoint_url, jwt_qws))
}

pub(super) fn hex_preview(data: &[u8], max_bytes: usize) -> String {
    let take = data.len().min(max_bytes);
    let hex: String = data[..take]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("");
    if data.len() > max_bytes {
        format!("{hex}...({}B total)", data.len())
    } else {
        hex
    }
}

fn normalize_opt_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(super) fn parse_subscribe_channels(items: Vec<String>) -> Result<Vec<Vec<u8>>, String> {
    items
        .into_iter()
        .map(|item| decode_hex_channel(&item))
        .collect()
}

pub(super) fn decode_hex_channel(raw: &str) -> Result<Vec<u8>, String> {
    let normalized = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    if normalized.is_empty() {
        return Err("empty subscribe channel hex value".to_string());
    }

    let needs_padding = normalized.len() % 2 != 0;
    let value = if needs_padding {
        format!("0{normalized}")
    } else {
        normalized.to_string()
    };

    let mut bytes = Vec::with_capacity(value.len() / 2);
    let chars: Vec<char> = value.chars().collect();

    for idx in (0..chars.len()).step_by(2) {
        let pair = [chars[idx], chars[idx + 1]];
        let hex = pair.iter().collect::<String>();
        let byte = u8::from_str_radix(&hex, 16)
            .map_err(|_| format!("invalid subscribe channel hex byte '{hex}' in '{raw}'"))?;
        bytes.push(byte);
    }

    Ok(bytes)
}
