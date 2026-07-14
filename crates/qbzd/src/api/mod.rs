// crates/qbzd/src/api/mod.rs — the HTTP control plane (02-cli-and-api.md §3).
//
// tiny_http 0.12, single listener, thread-per-connection collapsed to ONE
// serving thread (requests are handled inline, serialized — a control plane
// sees a handful of clients, never a load). Two mechanisms qualify the
// otherwise-open surface (§3.1.2):
//   - Origin shield (ALWAYS on): any request carrying an `Origin` header is
//     refused `403 origin_forbidden` before routing — CLI/curl/scripts send no
//     `Origin`, browsers do (CSRF / DNS-rebinding guard at zero UX cost).
//   - Opt-in `[server] token`: when set, `Authorization: Bearer <token>` is
//     required on every route EXCEPT `GET /api/ping`; a mismatch is
//     `401 invalid_token`. Unset = no auth machinery exists at runtime.
//
// The two-call split — `bind` at boot step 5 (stateless, so the foreign-occupant
// diagnosis runs BEFORE the stores/runtime exist), `serve` at boot step 11 — is
// what keeps the 01-architecture.md §8.1 boot order honest.
pub mod playback;
pub mod status;

use std::io::Cursor;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tiny_http::{Header, Request, Response};

use crate::adapter::DaemonAdapter;
use crate::paths::ProfileRoots;
use crate::state::DaemonShared;
use qbz_app::shell::AppRuntime;
use qbz_audio::settings::AudioSettingsStore;

/// The counted P0 route table (02 §3.2). A route exists only iff a shipped
/// client calls it (§3.1.4). T1-T6 landed the first 3; T7 (this task) adds the
/// 9 playback + now-playing routes (rows 4-12); T8 adds the 4 queue routes,
/// T11 `/api/settings/reload` — the inline `route_table_matches_spec_count`
/// test pins the number so the 68-routes failure shape (a route with no
/// caller) cannot creep back in.
pub const P0_ROUTES: &[(&str, &str)] = &[
    ("GET", "/api/ping"),
    ("GET", "/api/info"),
    ("GET", "/api/status"),
    ("GET", "/api/now-playing"),
    ("POST", "/api/playback/play"),
    ("POST", "/api/playback/pause"),
    ("POST", "/api/playback/toggle"),
    ("POST", "/api/playback/stop"),
    ("POST", "/api/playback/next"),
    ("POST", "/api/playback/previous"),
    ("POST", "/api/playback/seek"),
    ("POST", "/api/playback/volume"),
];

/// A socket bound at boot step 5, not yet serving. Wraps the tiny_http server
/// in an `Arc` so the serving thread and the shutdown handle can both hold it
/// (`unblock` from the handle terminates the thread's `incoming_requests`).
pub struct BoundServer {
    server: Arc<tiny_http::Server>,
}

/// Everything the route handlers read. Owned by the single serving thread
/// (moved into it by [`serve`]), so it only needs `Send`, never `Sync` — which
/// is why a plain `AudioSettingsStore` (rusqlite `Connection`: Send, not Sync)
/// can live here directly. `token` is the opt-in `[server] token`, read once at
/// boot (`None` = open).
pub struct ApiState {
    pub runtime: Arc<AppRuntime<DaemonAdapter>>,
    pub shared: Arc<Mutex<DaemonShared>>,
    pub roots: ProfileRoots,
    pub token: Option<String>,
    /// The bound address, echoed verbatim by `/api/info`.
    pub bind: String,
    /// Handle to the daemon's tokio runtime — the serving thread is a plain
    /// `std::thread`, so async core calls (`get_queue_state`) run via
    /// `Handle::block_on` (never called from a runtime worker → no panic).
    pub rt: tokio::runtime::Handle,
    /// Second read-only connection to the daemon-root audio settings DB (WAL
    /// allows it alongside the Player's). Supplies `configured_device`/`backend`.
    pub audio: AudioSettingsStore,
    /// Cached device enumeration for `device_present` (refreshed on a TTL so a
    /// `status` poll never re-enumerates CPAL on every call).
    pub devices: Mutex<DeviceCache>,
}

/// TTL-cached output-device names for the `device_present` check.
#[derive(Default)]
pub struct DeviceCache {
    pub at: Option<Instant>,
    pub names: Vec<String>,
}

/// Live serving handle. [`ApiHandle::shutdown`] unblocks the serving thread and
/// joins it — dropping the thread's `ApiState` (and with it the `Arc<AppRuntime>`
/// clone) BEFORE the daemon drops the runtime, preserving the §8.2 audio
/// clock-release ordering (the API thread is one more `Arc<AppRuntime>` holder,
/// exactly like the driver and auth-retry tasks).
pub struct ApiHandle {
    server: Arc<tiny_http::Server>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ApiHandle {
    pub fn shutdown(mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Why a bind failed — `AddrInUse` is the case the boot step-5 diagnosis probes
/// (foreign qbzd vs another process); everything else is a generic fatal.
#[derive(Debug)]
pub enum BindError {
    AddrInUse(SocketAddr),
    Other(String),
}

/// Boot step 5 (01 §8.1): bind only — stateless, so the foreign-occupant
/// diagnosis (in `daemon.rs`) runs BEFORE stores (6) and runtime composition (7).
pub fn bind(addr: SocketAddr) -> Result<BoundServer, BindError> {
    match tiny_http::Server::http(addr) {
        Ok(server) => Ok(BoundServer {
            server: Arc::new(server),
        }),
        Err(e) => Err(classify_bind_error(e, addr)),
    }
}

fn classify_bind_error(
    e: Box<dyn std::error::Error + Send + Sync + 'static>,
    addr: SocketAddr,
) -> BindError {
    if let Some(io) = e.downcast_ref::<std::io::Error>() {
        if io.kind() == std::io::ErrorKind::AddrInUse {
            return BindError::AddrInUse(addr);
        }
    }
    BindError::Other(e.to_string())
}

/// Best-effort occupant probe for the step-5 diagnosis: `GET /api/ping` and
/// check the response identifies as qbzd (`"app":"qbzd"`). Loopback, short
/// timeout, no dependency on reqwest (the CLI's async client is not built here).
pub fn probe_is_qbzd(addr: SocketAddr) -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    let mut stream = match TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let req = format!(
        "GET /api/ping HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let _ = stream.flush();
    let mut buf = Vec::new();
    let _ = stream.take(4096).read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    text.contains("\"app\":\"qbzd\"")
}

/// Boot step 11 (01 §8.1): start serving on the already-bound socket. Requests
/// are handled inline on one thread (serialized). `unblock` (from the handle)
/// ends `incoming_requests` for graceful shutdown.
pub fn serve(server: BoundServer, state: ApiState) -> ApiHandle {
    // The counted P0 surface — logged at boot so an operator sees exactly how
    // many routes this build exposes (and it anchors the const to production).
    log::info!("control plane serving {} P0 route(s)", P0_ROUTES.len());
    let srv = server.server;
    let srv_handle = srv.clone();
    let thread = std::thread::Builder::new()
        .name("qbzd-api".into())
        .spawn(move || {
            for mut req in srv.incoming_requests() {
                let resp = route(&state, &mut req);
                let _ = req.respond(resp);
            }
        })
        .expect("failed to spawn qbzd api thread");
    ApiHandle {
        server: srv_handle,
        thread: Some(thread),
    }
}

fn route(state: &ApiState, req: &mut Request) -> Response<Cursor<Vec<u8>>> {
    let method = req.method().as_str().to_owned();
    let path = req.url().split('?').next().unwrap_or("").to_owned();
    let has_origin = req.headers().iter().any(|h| h.field.equiv("Origin"));
    let auth_header = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str());

    // Origin shield (ALWAYS on) + opt-in [server] token — one pre-routing gate,
    // /api/ping exempt from the token only (§3.1.2).
    if let Some(reject) = access_gate(has_origin, &method, &path, auth_header, state.token.as_deref())
    {
        return reject.response();
    }

    match (method.as_str(), path.as_str()) {
        ("GET", "/api/ping") => json(
            200,
            serde_json::json!({"ok": true, "app": "qbzd", "api_version": crate::API_VERSION}),
        ),
        ("GET", "/api/info") => status::info(state),
        ("GET", "/api/status") => status::status(state),
        ("GET", "/api/now-playing") => playback::now_playing(state),
        ("POST", "/api/playback/play") => playback::play(state),
        ("POST", "/api/playback/pause") => playback::pause(state),
        ("POST", "/api/playback/toggle") => playback::toggle(state),
        ("POST", "/api/playback/stop") => playback::stop(state),
        ("POST", "/api/playback/next") => playback::next(state),
        ("POST", "/api/playback/previous") => playback::previous(state),
        ("POST", "/api/playback/seek") => {
            let body = read_json_body(req);
            playback::seek(state, &body)
        }
        ("POST", "/api/playback/volume") => {
            let body = read_json_body(req);
            playback::volume(state, &body)
        }
        // T8: 4 queue routes · T11: /api/settings/reload.
        _ => err_json(404, "not_found", "unknown route", "see qbzd --help"),
    }
}

/// Read and parse a request body as JSON (T7's seek/volume POST bodies).
/// An unreadable or absent body parses to `Value::Null` — the route handlers
/// treat a missing expected field as `400 bad_request`, never a panic.
fn read_json_body(req: &mut Request) -> serde_json::Value {
    let mut buf = String::new();
    let _ = req.as_reader().read_to_string(&mut buf);
    serde_json::from_str(&buf).unwrap_or(serde_json::Value::Null)
}

/// A pre-routing rejection. Carried as a small enum so [`access_gate`] stays a
/// pure decision (unit-testable without a tiny_http `Request`, which has no
/// public constructor) while `route` renders the normative envelope.
enum GateReject {
    OriginForbidden,
    InvalidToken,
}

impl GateReject {
    fn response(&self) -> Response<Cursor<Vec<u8>>> {
        match self {
            GateReject::OriginForbidden => err_json(
                403,
                "origin_forbidden",
                "requests with an Origin header are refused",
                "the control plane is not a browser API",
            ),
            GateReject::InvalidToken => err_json(
                401,
                "invalid_token",
                "missing or wrong bearer token",
                "set QBZD_TOKEN or check [server] token in qbzd.toml",
            ),
        }
    }
}

/// The pre-routing access decision (02 §3.1.2): Origin shield always on; the
/// opt-in Bearer required on every route except `GET /api/ping` when `token`
/// is `Some`. `None` = open (no auth machinery). Returns `Some(_)` to reject.
fn access_gate(
    has_origin: bool,
    method: &str,
    path: &str,
    auth_header: Option<&str>,
    token: Option<&str>,
) -> Option<GateReject> {
    if has_origin {
        return Some(GateReject::OriginForbidden);
    }
    if let Some(secret) = token {
        let is_ping = method == "GET" && path == "/api/ping";
        let expected = format!("Bearer {secret}");
        let ok = auth_header
            .map(|v| constant_time_eq(v.as_bytes(), expected.as_bytes()))
            .unwrap_or(false);
        if !is_ping && !ok {
            return Some(GateReject::InvalidToken);
        }
    }
    None
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// A 2xx JSON response. `pub(crate)` so the per-route handlers in `status.rs`
/// share the exact same envelope framing.
pub(crate) fn json(status: u16, body: serde_json::Value) -> Response<Cursor<Vec<u8>>> {
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static content-type header");
    Response::from_data(bytes)
        .with_status_code(status)
        .with_header(header)
}

/// The normative error envelope (02 §3.1.3): `{"error":{"code","message","hint"}}`.
/// The CLI keys its exit code off `code` (never raw HTTP status), and every hint
/// names the fix (§1.4 error voice). The G0 addendum's shorthand
/// `{"error":"origin_forbidden"}` is this same nested envelope — the uniform
/// §3.1.3 shape the CLI's `error_from_envelope` reads via `error.code`.
pub(crate) fn err_json(
    status: u16,
    code: &str,
    message: &str,
    hint: &str,
) -> Response<Cursor<Vec<u8>>> {
    json(status, error_body(code, message, hint))
}

pub(crate) fn error_body(code: &str, message: &str, hint: &str) -> serde_json::Value {
    serde_json::json!({"error": {"code": code, "message": message, "hint": hint}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_table_matches_spec_count() {
        // 02-cli-and-api.md §3.2 — P0 = exactly 17 routes; grows ONLY with a
        // shipped client. T1-T6 landed 3; T7 (this task) +9 = 12; T8 +4; T11 +1.
        assert_eq!(P0_ROUTES.len(), 12);
        assert!(P0_ROUTES.contains(&("GET", "/api/ping")));
        assert!(P0_ROUTES.contains(&("GET", "/api/info")));
        assert!(P0_ROUTES.contains(&("GET", "/api/status")));
        assert!(P0_ROUTES.contains(&("GET", "/api/now-playing")));
        assert!(P0_ROUTES.contains(&("POST", "/api/playback/play")));
        assert!(P0_ROUTES.contains(&("POST", "/api/playback/pause")));
        assert!(P0_ROUTES.contains(&("POST", "/api/playback/toggle")));
        assert!(P0_ROUTES.contains(&("POST", "/api/playback/stop")));
        assert!(P0_ROUTES.contains(&("POST", "/api/playback/next")));
        assert!(P0_ROUTES.contains(&("POST", "/api/playback/previous")));
        assert!(P0_ROUTES.contains(&("POST", "/api/playback/seek")));
        assert!(P0_ROUTES.contains(&("POST", "/api/playback/volume")));
    }

    #[test]
    fn constant_time_eq_matches_only_identical_slices() {
        assert!(constant_time_eq(b"Bearer s3cret", b"Bearer s3cret"));
        assert!(!constant_time_eq(b"Bearer s3cret", b"Bearer wrong"));
        assert!(!constant_time_eq(b"Bearer s3cret", b"Bearer s3cret-extra"));
        assert!(!constant_time_eq(b"", b"x"));
    }

    fn code(r: Option<GateReject>) -> Option<&'static str> {
        r.map(|r| match r {
            GateReject::OriginForbidden => "origin_forbidden",
            GateReject::InvalidToken => "invalid_token",
        })
    }

    #[test]
    fn origin_header_is_refused_on_every_route_including_ping() {
        // Step 4(a): an Origin header → 403 origin_forbidden everywhere, even
        // /api/ping, and even in the open (token=None) default.
        for (m, p) in P0_ROUTES {
            assert_eq!(
                code(access_gate(true, m, p, None, None)),
                Some("origin_forbidden"),
                "{m} {p} with Origin must be refused"
            );
        }
        // ...and the Origin shield wins even when a valid Bearer is present.
        assert_eq!(
            code(access_gate(true, "GET", "/api/ping", Some("Bearer s3cret"), Some("s3cret"))),
            Some("origin_forbidden")
        );
    }

    #[test]
    fn open_mode_answers_every_route_without_auth() {
        // Step 4(b): token=None → no auth machinery; nothing is rejected.
        for (m, p) in P0_ROUTES {
            assert!(access_gate(false, m, p, None, None).is_none(), "{m} {p} must be open");
        }
    }

    #[test]
    fn opt_in_token_rejects_missing_or_wrong_bearer_but_never_ping() {
        // Step 4(c): token=Some → missing/wrong bearer is 401 on non-ping routes;
        // /api/ping stays 200 (exempt); the correct bearer passes.
        let tok = Some("s3cret");
        assert_eq!(code(access_gate(false, "GET", "/api/status", None, tok)), Some("invalid_token"));
        assert_eq!(
            code(access_gate(false, "GET", "/api/status", Some("Bearer nope"), tok)),
            Some("invalid_token")
        );
        assert!(access_gate(false, "GET", "/api/status", Some("Bearer s3cret"), tok).is_none());
        // /api/ping answers even with no/ wrong bearer.
        assert!(access_gate(false, "GET", "/api/ping", None, tok).is_none());
        assert!(access_gate(false, "GET", "/api/ping", Some("Bearer nope"), tok).is_none());
    }

    #[test]
    fn error_envelope_is_the_nested_shape_with_code() {
        // 02 §3.1.3 — the on-the-wire shape the CLI reads via `error.code`.
        let body = error_body("origin_forbidden", "refused", "not a browser API");
        assert_eq!(body["error"]["code"], "origin_forbidden");
        assert_eq!(body["error"]["message"], "refused");
        assert_eq!(body["error"]["hint"], "not a browser API");
    }
}
