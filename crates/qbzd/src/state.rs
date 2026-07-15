// crates/qbzd/src/state.rs — shared in-memory daemon state (one
// `Arc<Mutex<DaemonShared>>` shared by the playback driver + the HTTP API).
// Fields land now; real sources wire in as each producing task lands
// (T3 driver/audio, T6 HTTP server, T7 transport, T9/T10 QConnect).
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct LatchedErrors {
    // 01 §9.4 — drain-once channels become latches
    pub stream: Option<String>,
    pub auth: Option<String>,
    pub transport: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    NeedsAuth,
    Restoring,
    LoggedIn,
} // 01 §6.2 machine

pub struct DaemonShared {
    // one Arc<Mutex<...>> shared by driver + API
    pub auth: AuthState,
    pub user_id: Option<u64>,
    pub subscription: Option<String>,
    pub last_errors: LatchedErrors,
    pub driver_last_tick: Option<std::time::Instant>,
    pub muted: bool,
    pub premute_volume: f32,
    pub started_at: std::time::Instant,
    pub startup_warnings: u32,
    pub qconnect: QconnectStatus,
    /// T11 (`POST /api/settings/reload`, 02 §3.3.17): a fingerprint of the
    /// credential-file token currently applied to the live session, so reload
    /// can tell "new token on disk" (re-login) from "same token, unrelated
    /// nudge" (no-op) without keeping a second copy of the secret in memory.
    /// `None` whenever the daemon is not LoggedIn against a known token
    /// (cleared alongside every `set_needs_auth`).
    pub credential_fingerprint: Option<u64>,
}

/// A non-reversible-in-practice fingerprint of a credential token (SipHash via
/// the stdlib default hasher) — used ONLY to detect "the file changed", never
/// to reconstruct the token. Keeps `DaemonShared` from holding a second live
/// copy of the secret alongside the credential file + the Qobuz client.
pub fn token_fingerprint(token: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct QconnectStatus {
    pub enabled: bool,
    pub state: String, // "off"|"connecting"|"connected"|"retrying"|"exhausted"
    pub session_active: bool,
    pub device_name: String,
    pub last_transport_reconnect: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latched_errors_default_all_none() {
        let e = LatchedErrors::default();
        assert!(e.stream.is_none());
        assert!(e.auth.is_none());
        assert!(e.transport.is_none());
    }

    #[test]
    fn qconnect_status_default_is_off_and_inactive() {
        let q = QconnectStatus::default();
        assert!(!q.enabled);
        assert_eq!(q.state, "");
        assert!(!q.session_active);
        assert_eq!(q.device_name, "");
        assert!(q.last_transport_reconnect.is_none());
    }

    #[test]
    fn auth_state_serializes_to_contract_strings() {
        // 02-cli-and-api.md §3.3.3: auth.state ∈ logged_in|needs_auth|restoring
        assert_eq!(
            serde_json::to_string(&AuthState::NeedsAuth).unwrap(),
            "\"needs_auth\""
        );
        assert_eq!(
            serde_json::to_string(&AuthState::Restoring).unwrap(),
            "\"restoring\""
        );
        assert_eq!(
            serde_json::to_string(&AuthState::LoggedIn).unwrap(),
            "\"logged_in\""
        );
    }

    #[test]
    fn daemon_shared_holds_the_fields_the_status_route_needs() {
        // Construction smoke test: DaemonShared has no derive (Instant isn't
        // Serialize) so this is the only compile-time guard that the field
        // set/types stay what api::status::assemble expects.
        let shared = DaemonShared {
            auth: AuthState::LoggedIn,
            user_id: Some(1234567),
            subscription: Some("studio".into()),
            last_errors: LatchedErrors::default(),
            driver_last_tick: None,
            muted: false,
            premute_volume: 1.0,
            started_at: std::time::Instant::now(),
            startup_warnings: 0,
            qconnect: QconnectStatus::default(),
            credential_fingerprint: None,
        };
        assert_eq!(shared.auth, AuthState::LoggedIn);
        assert_eq!(shared.user_id, Some(1234567));
    }

    #[test]
    fn token_fingerprint_is_stable_and_distinguishes_tokens() {
        let a = token_fingerprint("token-a");
        let a_again = token_fingerprint("token-a");
        let b = token_fingerprint("token-b");
        assert_eq!(a, a_again);
        assert_ne!(a, b);
    }
}
