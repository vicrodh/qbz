// crates/qbzd/src/api/status.rs — `GET /api/status` composite contract
// (02-cli-and-api.md §3.3.3; memo D14). Struct shape only in T2 — the
// `auth`/`qconnect`/`last_errors` sections already read from `DaemonShared`;
// `audio`/`playback`/`network` are placeholders wired by T3 (audio/playback
// driver), T6 (HTTP server + network reachability) and T10 (qconnect report
// tick, already flowing through `DaemonShared::qconnect`).
use serde::Serialize;

use crate::state::{AuthState, DaemonShared, LatchedErrors, QconnectStatus};

#[derive(Debug, Clone, Serialize)]
pub struct StatusDoc {
    pub version: String,
    pub api_version: u32,
    pub uptime_secs: u64,
    pub data_root: String,
    pub driver_tick_age_ms: Option<u64>,
    pub auth: AuthStatus,
    pub audio: AudioStatus,
    pub playback: PlaybackStatus,
    pub qconnect: QconnectStatus,
    pub network: NetworkStatus,
    pub last_errors: LatchedErrors,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthStatus {
    pub state: AuthState,
    pub user_id: Option<u64>,
    pub subscription: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioStatus {
    pub backend: Option<String>,
    pub configured_device: Option<String>,
    pub device_present: bool,
    pub device_open: bool,
    /// `BitPerfectMode` serde variants: "DirectHardware"|"PluginFallback"|"Disabled"
    /// (crates/qbz-audio/src/backend.rs:226-233). Kept as a plain string here so
    /// this crate does not need to depend on the exact qbz-audio enum shape yet.
    pub bit_perfect: Option<String>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaybackStatus {
    pub state: String,
    pub track_id: Option<u64>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub position: Option<u64>,
    pub duration: Option<u64>,
    pub volume: f32,
    pub muted: bool,
    pub queue_len: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkStatus {
    pub online: bool,
}

/// Stub assembler: fills what T2 has (process facts + the `DaemonShared`
/// snapshot); `audio`/`playback`/`network` stay placeholders until their
/// producing tasks land.
pub fn assemble(shared: &DaemonShared, version: &str, data_root: &str) -> StatusDoc {
    StatusDoc {
        version: version.to_string(),
        api_version: crate::API_VERSION,
        uptime_secs: shared.started_at.elapsed().as_secs(),
        data_root: data_root.to_string(),
        driver_tick_age_ms: shared
            .driver_last_tick
            .map(|t| t.elapsed().as_millis() as u64),
        auth: AuthStatus {
            state: shared.auth,
            user_id: shared.user_id,
            subscription: shared.subscription.clone(),
        },
        audio: AudioStatus {
            backend: None,
            configured_device: None,
            device_present: false,
            device_open: false,
            bit_perfect: None,
            sample_rate: None,
            bit_depth: None,
        },
        playback: PlaybackStatus {
            state: "stopped".to_string(),
            track_id: None,
            title: None,
            artist: None,
            position: None,
            duration: None,
            volume: 0.0,
            muted: shared.muted,
            queue_len: 0,
        },
        qconnect: shared.qconnect.clone(),
        network: NetworkStatus { online: true },
        last_errors: shared.last_errors.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared_fixture() -> DaemonShared {
        DaemonShared {
            auth: AuthState::NeedsAuth,
            user_id: None,
            subscription: None,
            last_errors: LatchedErrors {
                stream: None,
                auth: Some("token rejected by Qobuz (401) — cleared".into()),
                transport: None,
            },
            driver_last_tick: None,
            muted: false,
            premute_volume: 1.0,
            started_at: std::time::Instant::now(),
            startup_warnings: 0,
            qconnect: QconnectStatus::default(),
        }
    }

    #[test]
    fn assemble_mirrors_the_top_level_status_contract_keys() {
        // 02-cli-and-api.md §3.3.3 top-level keys, exactly.
        let doc = assemble(&shared_fixture(), "2.1.0", "/home/pi/.local/share/qbzd");
        let json = serde_json::to_value(&doc).unwrap();
        let obj = json.as_object().unwrap();
        for key in [
            "version",
            "api_version",
            "uptime_secs",
            "data_root",
            "driver_tick_age_ms",
            "auth",
            "audio",
            "playback",
            "qconnect",
            "network",
            "last_errors",
        ] {
            assert!(obj.contains_key(key), "missing top-level key: {key}");
        }
    }

    #[test]
    fn assemble_needs_auth_fragment_matches_spec_example() {
        // 02 §3.3.3 NeedsAuth example fragment.
        let doc = assemble(&shared_fixture(), "2.1.0", "/home/pi/.local/share/qbzd");
        assert_eq!(doc.auth.state, AuthState::NeedsAuth);
        assert!(doc.auth.user_id.is_none());
        assert!(doc.auth.subscription.is_none());
        assert_eq!(
            doc.last_errors.auth.as_deref(),
            Some("token rejected by Qobuz (401) — cleared")
        );
        let json = serde_json::to_value(&doc.auth).unwrap();
        assert_eq!(json["state"], "needs_auth");
    }

    #[test]
    fn assemble_carries_data_root_and_version_through_verbatim() {
        let doc = assemble(&shared_fixture(), "2.1.0", "/home/pi/.local/share/qbzd");
        assert_eq!(doc.version, "2.1.0");
        assert_eq!(doc.data_root, "/home/pi/.local/share/qbzd");
        assert_eq!(doc.api_version, crate::API_VERSION);
    }
}
