//! QConnect transport config + credential discovery + device identity for the
//! Slint frontend (slice 6, Phase S).
//!
//! Ports the Tauri `src-tauri/src/qconnect/transport.rs` config layer. The only
//! frontend-specific change: the Qobuz client is reached via
//! `runtime.core().client()` — an `Arc<RwLock<Option<QobuzClient>>>` (may be
//! `None` before the API is initialized) — instead of Tauri's always-present
//! `AppState.client`. The device-uuid + settings-DB path delegate to the shared
//! `qbz_app::qconnect_identity` so Tauri and Slint resolve the SAME identity.
//!
//! Wired by the Slint QConnect service (next Phase S step); unused until then.
#![allow(dead_code)]

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qconnect_transport_ws::WsTransportConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapter::SlintAdapter;

const DEFAULT_QCONNECT_DEVICE_BRAND: &str = "QBZ";
const DEFAULT_QCONNECT_DEVICE_MODEL: &str = "QBZ";
const DEFAULT_QCONNECT_DEVICE_TYPE: i32 = 5; // computer
const DEFAULT_QCONNECT_SOFTWARE_PREFIX: &str = "qbz";
const QCONNECT_QWS_TOKEN_KIND: &str = "jwt_qws";
const QCONNECT_QWS_CREATE_TOKEN_PATH: &str = "/qws/createToken";

type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Persistent QConnect device identity — delegates to the shared module so the
/// Slint install resolves the SAME uuid as Tauri (same DB path/key/env override).
pub fn resolve_qconnect_device_uuid() -> String {
    qbz_app::qconnect_identity::resolve_qconnect_device_uuid()
}

/// `qbz/<version>` software-version string for the device-info payload.
pub fn resolve_qconnect_software_version() -> String {
    std::env::var("QBZ_QCONNECT_SOFTWARE_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "{DEFAULT_QCONNECT_SOFTWARE_PREFIX}/{}",
                env!("CARGO_PKG_VERSION")
            )
        })
}

pub fn resolve_qconnect_device_brand() -> String {
    std::env::var("QBZ_QCONNECT_DEVICE_BRAND")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_QCONNECT_DEVICE_BRAND.to_string())
}

pub fn resolve_qconnect_device_model() -> String {
    std::env::var("QBZ_QCONNECT_DEVICE_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_QCONNECT_DEVICE_MODEL.to_string())
}

pub fn resolve_qconnect_device_type() -> i32 {
    std::env::var("QBZ_QCONNECT_DEVICE_TYPE")
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .unwrap_or(DEFAULT_QCONNECT_DEVICE_TYPE)
}

pub fn resolve_system_hostname() -> String {
    if let Ok(h) = std::env::var("HOSTNAME") {
        if !h.trim().is_empty() {
            return h.trim().to_string();
        }
    }
    if let Ok(h) = std::fs::read_to_string("/etc/hostname") {
        let trimmed = h.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    "Desktop".to_string()
}

/// Returns "Qbz - {hostname}" as the default device name.
pub fn resolve_default_qconnect_device_name() -> String {
    format!("Qbz - {}", resolve_system_hostname())
}

/// Effective friendly name: custom override -> env -> "Qbz - {hostname}".
pub fn resolve_qconnect_friendly_name(custom_name: Option<&str>) -> String {
    custom_name
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            std::env::var("QBZ_QCONNECT_DEVICE_NAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(resolve_default_qconnect_device_name)
}

// ---------------------------------------------------------------------------
// Device-info + join payload shapes (piece a). Mirror the Tauri adapter
// `types.rs` `QconnectDeviceInfoPayload` / `QconnectDeviceCapabilitiesPayload` /
// `QconnectJoinSessionRequest` exactly so the wire frames the server sees are
// byte-identical between the two frontends. Built from the device-identity
// resolvers above; the uuid delegates to the shared `qbz_app::qconnect_identity`
// so a device keeps ONE identity across both frontends.
// ---------------------------------------------------------------------------

// AudioQuality wire levels: 1=mp3, 4=hires_l2(192k). Capabilities advertise the
// device's min/max decode support; VOLUME_REMOTE_CONTROL_ALLOWED(2) means a
// controller may set our volume.
pub const AUDIO_QUALITY_MP3: i32 = 1;
pub const AUDIO_QUALITY_HIRES_LEVEL2: i32 = 4;
const VOLUME_REMOTE_CONTROL_ALLOWED: i32 = 2;
/// Renderer buffer-state wire value for OK/ready (mirrors the Tauri adapter).
pub const BUFFER_STATE_OK: i32 = 2;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectDeviceCapabilitiesPayload {
    pub min_audio_quality: Option<i32>,
    pub max_audio_quality: Option<i32>,
    pub volume_remote_control: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectDeviceInfoPayload {
    pub device_uuid: Option<String>,
    pub friendly_name: Option<String>,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub device_type: Option<i32>,
    pub capabilities: Option<QconnectDeviceCapabilitiesPayload>,
    pub software_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectJoinSessionRequest {
    pub session_uuid: Option<String>,
    pub device_info: Option<QconnectDeviceInfoPayload>,
}

// ---------------------------------------------------------------------------
// Controller-mode transport request DTOs (Slint CONTROLLER port). Mirror the
// Tauri adapter `types.rs` (`QconnectQueueVersionPayload`,
// `QconnectSetPlayerStateQueueItemPayload`, `QconnectSetPlayerStateRequest`,
// `QconnectSetVolumeRequest`, `QconnectMuteVolumeRequest`) field-for-field so the
// wire frames the cloud parses are byte-identical between both frontends. Do NOT
// rename/reorder optionals.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QconnectQueueVersionPayload {
    pub major: u64,
    pub minor: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSetPlayerStateQueueItemPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_version: Option<QconnectQueueVersionPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i32>,
}

// `skip_serializing_if` is LOAD-BEARING here: a `None` field must be OMITTED, not
// serialized as JSON `null`. The protocol mapper rejects a `current_queue_item`
// that is present-but-null ("must be an object") and aborts the whole send, so a
// bare play/pause toggle (which sends only `playing_state`) would never reach the
// wire without these. Each SetPlayerState field is independently optional per the
// protocol, so omitting them is the intended "do one or several" behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSetPlayerStateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playing_state: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_position: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_queue_item: Option<QconnectSetPlayerStateQueueItemPayload>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSetVolumeRequest {
    pub renderer_id: Option<i32>,
    pub volume: Option<i32>,
    pub volume_delta: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectMuteVolumeRequest {
    pub renderer_id: Option<i32>,
    pub value: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSetActiveRendererRequest {
    pub renderer_id: Option<i32>,
}

/// Build a `CtrlSrvrSetPlayerState` request that seeks the remote renderer to
/// `position_ms` for `current_queue_item_id` under optimistic concurrency.
///
/// `playing_state` is intentionally `None`: a seek must not toggle play/pause.
/// Pure mirror of the Tauri `build_set_position_player_state_request`
/// (`src-tauri/src/qconnect/commands.rs`).
pub fn build_set_position_player_state_request(
    position_ms: i64,
    current_queue_item_id: Option<u64>,
    queue_version: QconnectQueueVersionPayload,
) -> QconnectSetPlayerStateRequest {
    let current_position = i32::try_from(position_ms.max(0)).ok();
    let current_queue_item = current_queue_item_id.and_then(|qid| {
        i32::try_from(qid)
            .ok()
            .map(|id| QconnectSetPlayerStateQueueItemPayload {
                queue_version: Some(QconnectQueueVersionPayload {
                    major: queue_version.major,
                    minor: queue_version.minor,
                }),
                id: Some(id),
            })
    });
    QconnectSetPlayerStateRequest {
        playing_state: None,
        current_position,
        current_queue_item,
    }
}

/// Build the device-info payload with the default friendly name.
pub fn default_qconnect_device_info() -> QconnectDeviceInfoPayload {
    default_qconnect_device_info_with_name(None)
}

/// Build the device-info payload, overriding the friendly name when a custom
/// device name is set. Mirrors the Tauri `default_qconnect_device_info_with_name`.
pub fn default_qconnect_device_info_with_name(
    custom_name: Option<&str>,
) -> QconnectDeviceInfoPayload {
    QconnectDeviceInfoPayload {
        device_uuid: Some(resolve_qconnect_device_uuid()),
        friendly_name: Some(resolve_qconnect_friendly_name(custom_name)),
        brand: Some(resolve_qconnect_device_brand()),
        model: Some(resolve_qconnect_device_model()),
        serial_number: None,
        device_type: Some(resolve_qconnect_device_type()),
        capabilities: Some(QconnectDeviceCapabilitiesPayload {
            min_audio_quality: Some(AUDIO_QUALITY_MP3),
            max_audio_quality: Some(AUDIO_QUALITY_HIRES_LEVEL2),
            volume_remote_control: Some(VOLUME_REMOTE_CONTROL_ALLOWED),
        }),
        software_version: Some(resolve_qconnect_software_version()),
    }
}

/// Resolve THIS device's identity for injection into the frontend-agnostic
/// session-apply logic in qconnect-app (`apply_session_management_event` takes a
/// `LocalIdentity` so the crate never depends on the persistence layer). Mirrors
/// the Tauri `resolve_local_identity`.
pub fn resolve_local_identity() -> qconnect_app::LocalIdentity {
    let info = default_qconnect_device_info();
    qconnect_app::LocalIdentity {
        device_uuid: info.device_uuid.unwrap_or_default(),
        friendly_name: info.friendly_name,
        brand: info.brand,
        model: info.model,
        device_type: info.device_type,
    }
}

fn qconnect_settings_db_path() -> Option<std::path::PathBuf> {
    qbz_app::qconnect_identity::qconnect_settings_db_path()
}

/// Load the persisted custom device name (fail-open: None on any error).
pub fn load_persisted_device_name() -> Option<String> {
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

/// Persist the custom device name (None clears it).
pub fn persist_device_name(name: Option<&str>) {
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

/// Build the WS transport config: env overrides first, then auto-discovery via
/// `qws/createToken`. Mirrors the Tauri `resolve_transport_config` (the
/// per-option knobs are deferred — the Slint connect path uses defaults +
/// env). `require_jwt = true` (hard) and `reconnect_idle_retry_ms = 60s` match
/// the hardened Tauri defaults.
pub async fn resolve_transport_config(runtime: &Runtime) -> Result<WsTransportConfig, String> {
    let mut endpoint_url = normalize_opt_string(std::env::var("QBZ_QCONNECT_WS_ENDPOINT").ok());
    let mut jwt_qws = normalize_opt_string(std::env::var("QBZ_QCONNECT_JWT_QWS").ok())
        .or_else(|| normalize_opt_string(std::env::var("QBZ_QCONNECT_JWT").ok()));

    if endpoint_url.is_none() || jwt_qws.is_none() {
        match fetch_qconnect_transport_credentials(runtime).await {
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
                    "QConnect endpoint_url is required (or QBZ_QCONNECT_WS_ENDPOINT). Auto-discovery via qws/createToken failed: {err}"
                ));
            }
        }
    }

    let endpoint_url = endpoint_url.ok_or_else(|| {
        "QConnect endpoint_url is required (or QBZ_QCONNECT_WS_ENDPOINT)".to_string()
    })?;

    let subscribe_channels = if let Ok(raw) = std::env::var("QBZ_QCONNECT_SUBSCRIBE_CHANNELS_HEX") {
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
    config.require_jwt = true;
    config.reconnect_idle_retry_ms = 60_000;
    config.subscribe_channels = subscribe_channels;
    Ok(config)
}

async fn fetch_qconnect_transport_credentials(
    runtime: &Runtime,
) -> Result<(Option<String>, Option<String>), String> {
    // Slint difference: the client is Option-wrapped (None before API init).
    let client = runtime
        .core()
        .client()
        .read()
        .await
        .clone()
        .ok_or_else(|| "qws/createToken requires an initialized API client".to_string())?;

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

    let url = qbz_qobuz::endpoints::build_url(QCONNECT_QWS_CREATE_TOKEN_PATH);
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

pub fn parse_subscribe_channels(items: Vec<String>) -> Result<Vec<Vec<u8>>, String> {
    items.into_iter().map(|item| decode_hex_channel(&item)).collect()
}

pub fn decode_hex_channel(raw: &str) -> Result<Vec<u8>, String> {
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
