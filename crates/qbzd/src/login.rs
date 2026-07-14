// crates/qbzd/src/login.rs — `qbzd login` / `qbzd logout` (02-cli-and-api.md §2.2,
// memo D6). Ported from the desktop system-browser OAuth (crates/qbz/src/auth.rs)
// with three deliberate daemon changes:
//
//   1. The one-shot listener binds an EPHEMERAL port (`bind((host, 0))`), NEVER
//      the control-API port (D6 — this is what dissolves the loopback-vs-LAN 401
//      contradiction: the callback lives on its own throwaway listener).
//   2. A CSPRNG nonce is bound into the redirect PATH
//      (`redirect_url=http://<host>:<port>/<nonce>`) and validated against the
//      callback's request path; a mismatched or second callback is dropped. The
//      nonce lives in the PATH — not an OAuth `state` param — because the
//      working desktop flow sends no `state` and there is no evidence Qobuz
//      echoes one; the redirect URL itself is preserved verbatim.
//   3. The command only VALIDATES (live `login_with_token` / `login_with_oauth_code`)
//      and PERSISTS the token into the daemon root, then best-effort nudges a
//      running daemon to reload. It never activates a session in-process — the
//      daemon (a separate process) owns session activation.
//
// There is NO email+password surface anywhere (D6/D12): the only ways in are the
// browser flow, a pasted redirect URL, and a directly-injected token.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use qbz_app::shell::AppRuntime;
use qbz_audio::settings::AudioSettings;
use qbz_core::NoOpAdapter;
use qbz_models::UserSession;

use crate::paths::ProfileRoots;

/// Browser-login deadline (02 §2.2). The desktop uses 180 s; the daemon spec
/// pins 300 s because a headless operator may need to forward the port first.
const LOGIN_DEADLINE: Duration = Duration::from_secs(300);

/// Cosmetic redirect port for the `--paste` flow. Nothing binds it — the browser
/// lands on a connection error and the operator copies the URL out of the address
/// bar — so the value only needs to be a syntactically valid, unprivileged port.
const PASTE_REDIRECT_PORT: u16 = 43717;

/// Everything that can go wrong on the way to a persisted session. Every variant
/// renders with a `→` fix line (02 §1.4). All three map to exit 1 in `main`
/// (login never reports 3/4 — it does its own OAuth and local persist, so it
/// works daemon-up or daemon-down, §2.2).
#[derive(Debug)]
pub enum LoginError {
    /// No nonce-valid redirect arrived within [`LOGIN_DEADLINE`]. Carries the
    /// ephemeral port so the timeout copy can forward exactly it.
    Timeout(u16),
    /// Qobuz explicitly rejected the credentials (401 / ineligible account).
    Rejected(String),
    /// Any other local or network-class failure.
    Failed(String),
}

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginError::Timeout(port) => write!(f, "{}", crate::cli::copy::login_timeout(*port)),
            LoginError::Rejected(msg) => write!(
                f,
                "error: Qobuz rejected the credentials ({msg})\n  \
                 → check the token or sign in again:  qbzd login"
            ),
            LoginError::Failed(msg) => {
                write!(f, "error: {msg}")?;
                if !msg.contains('→') {
                    write!(
                        f,
                        "\n  → check your connection and retry:  qbzd login\n  \
                         → or inject a token directly:      qbzd login --token <user_auth_token>"
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LoginError {}

// ============================ public entry points ============================

/// Live-validate a raw `user_auth_token` via `login_with_token` BEFORE it is
/// ever persisted. The returned session is the source of truth for the user id
/// and plan. Registers the token as a redaction secret first, so no log line
/// that might carry it (in the client or elsewhere) can leak it.
///
/// T12 (settings import) and T13 (setup TUI Account screen) reuse this.
pub async fn validate_token(token: &str) -> Result<UserSession, LoginError> {
    // §6.3: register before any log line can carry the token.
    qbz_log::register_secret(token.to_string());
    let runtime = build_login_runtime().await?;
    runtime
        .core()
        .login_with_token(token)
        .await
        .map_err(map_core_err)
}

/// Path 1 (02 §2.2): system-browser OAuth on a one-shot, nonce-bound, ephemeral
/// listener. `callback_host = Some(ip)` binds wide (`0.0.0.0`) so a LAN browser
/// (a phone) can reach the callback and embeds that host in the redirect URL;
/// `None` binds loopback only and redirects to `127.0.0.1`.
pub async fn login_browser(
    roots: &ProfileRoots,
    callback_host: Option<String>,
) -> Result<UserSession, LoginError> {
    let runtime = build_login_runtime().await?;
    let app_id = read_app_id(&runtime).await?;

    // D6: an EPHEMERAL port on its own listener — never the control-API port.
    let bind_host = if callback_host.is_some() {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };
    let redirect_host = callback_host.as_deref().unwrap_or("127.0.0.1").to_string();
    let listener = TcpListener::bind((bind_host, 0))
        .map_err(|e| LoginError::Failed(format!("could not bind the login listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| LoginError::Failed(e.to_string()))?
        .port();

    let nonce = gen_nonce();
    let url = build_oauth_url(&app_id, &redirect_host, port, &nonce);

    println!("Opening your browser to sign in to Qobuz.");
    println!("If it does not open, paste this URL into a browser:\n  {url}\n");
    if let Err(e) = open::that(&url) {
        // Headless boxes have no browser — not fatal; the listener still waits
        // and the printed URL is what the operator forwards/opens.
        log::debug!("could not open a browser automatically: {e}");
    }

    let nonce_owned = nonce.clone();
    let deadline = Instant::now() + LOGIN_DEADLINE;
    let captured = tokio::task::spawn_blocking(move || {
        capture_callback(listener, &nonce_owned, deadline)
    })
    .await
    .map_err(|e| LoginError::Failed(format!("login listener task panicked: {e}")))?
    .map_err(|e| LoginError::Failed(format!("login listener I/O error: {e}")))?;

    let code = captured.ok_or(LoginError::Timeout(port))?;
    let session = exchange_code(&runtime, &code).await?;
    finalize(roots, &session)?;
    Ok(session)
}

/// Path 2 (02 §2.2): print the authorize URL, read the redirect URL (or a bare
/// code) back from stdin. No listener binds — useful when the browser cannot
/// reach this machine at all. A pasted redirect URL carries the nonce in its
/// path, so it is validated (leniently); a bare code is accepted as-is
/// (explicit operator action).
pub async fn login_paste(roots: &ProfileRoots) -> Result<UserSession, LoginError> {
    let runtime = build_login_runtime().await?;
    let app_id = read_app_id(&runtime).await?;
    let nonce = gen_nonce();
    let url = build_oauth_url(&app_id, "127.0.0.1", PASTE_REDIRECT_PORT, &nonce);

    println!("Open this URL in a browser and sign in to Qobuz:\n  {url}\n");
    println!("Your browser will land on a page that fails to load — that is expected.");
    print!("Paste the full redirect URL (or just the code) here: ");
    let _ = std::io::stdout().flush();

    let line = read_stdin_line()?;
    let code = code_from_paste(line.trim(), &nonce).ok_or_else(|| {
        LoginError::Failed(
            "could not find an authorization code in the pasted input\n  \
             → paste the full redirect URL from the browser address bar\n  \
             → or inject a token directly:      qbzd login --token <user_auth_token>"
                .to_string(),
        )
    })?;

    let session = exchange_code(&runtime, &code).await?;
    finalize(roots, &session)?;
    Ok(session)
}

/// Path 3 (02 §2.2): a directly-injected `user_auth_token`. Validated live, then
/// persisted.
pub async fn login_with_token_arg(
    roots: &ProfileRoots,
    token: &str,
) -> Result<UserSession, LoginError> {
    let session = validate_token(token).await?;
    finalize(roots, &session)?;
    Ok(session)
}

/// `qbzd logout` (02 §2.2): clear the daemon-root credential file and nudge a
/// running daemon into NeedsAuth. Returns whether the daemon acknowledged the
/// reload, so the caller can pick the right success line.
pub fn logout(roots: &ProfileRoots) -> Result<bool, LoginError> {
    qbz_credentials::clear_oauth_token_at(&roots.config)
        .map_err(|e| LoginError::Failed(format!("could not clear the credential file: {e}")))?;
    let host = nudge_host(roots);
    // token: opt-in [server] token, wired by T6.
    Ok(nudge_reload(&host, None))
}

/// Best-effort `GET /api/ping` → `POST /api/settings/reload` against a local
/// daemon (the reload route lands in T11). Any failure — daemon down, route not
/// yet present, timeout — returns `false`, which every caller treats as "the
/// daemon will pick the credential change up on its next start" (login/logout
/// are specified to work daemon-down, 02 §2.2). `token` carries the opt-in
/// `[server] token` as `Authorization: Bearer` when present; T5 callers pass
/// `None`.
pub fn nudge_reload(host: &str, token: Option<&str>) -> bool {
    if !http_request_2xx(host, "GET", "/api/ping", token) {
        return false;
    }
    http_request_2xx(host, "POST", "/api/settings/reload", token)
}

// ============================ pure, unit-tested ============================

/// Build the Qobuz browser authorize URL. Mirrors the desktop shape
/// (`crates/qbz/src/auth.rs:76-80`) except the redirect URL carries the CSPRNG
/// nonce as its path segment: `redirect_url=http://<host>:<port>/<nonce>`. The
/// binding rides the redirect URL itself (preserved verbatim by the OAuth
/// round-trip) instead of a `state` param, because the working desktop flow
/// sends no `state` and Qobuz is not proven to echo one.
pub fn build_oauth_url(ext_app_id: &str, host: &str, port: u16, nonce: &str) -> String {
    let redirect = format!("http://{host}:{port}/{nonce}");
    format!(
        "https://www.qobuz.com/signin/oauth?ext_app_id={}&redirect_url={}",
        ext_app_id,
        urlencoding::encode(&redirect),
    )
}

/// Parse an HTTP request line from the one-shot listener. Returns the
/// authorization code ONLY when the request PATH carries the expected nonce
/// (`GET /<nonce>?...`) — a mismatched or absent path nonce is dropped (the D6
/// binding). No dependency on any `state` query param (Qobuz is not proven to
/// echo one; one present is simply ignored). `code_autorisation` wins over
/// `code`, matching the desktop.
pub fn parse_callback(request_line: &str, expected_nonce: &str) -> Option<String> {
    let target = request_line.split_whitespace().nth(1)?;
    let (path, query) = target.split_once('?')?;
    if path.trim_matches('/') != expected_nonce {
        return None; // wrong or absent path nonce → drop
    }
    code_from_query(query)
}

/// Parse pasted `--paste` input: either a full redirect URL or a bare
/// authorization code. A pasted URL carries the nonce in its PATH (that is how
/// the redirect was built); validation is lenient — an empty path is tolerated
/// (hand-pasted, possibly truncated), a present-but-wrong path nonce is
/// rejected.
pub fn code_from_paste(input: &str, expected_nonce: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    match input.split_once('?') {
        Some((prefix, query)) => {
            let seg = url_path(prefix).trim_matches('/');
            if !seg.is_empty() && seg != expected_nonce {
                return None; // present but mismatched → drop
            }
            code_from_query(query)
        }
        // No query string: a bare code is fine; a bare URL has nothing to extract.
        None if input.contains("://") => None,
        None => Some(input.to_string()),
    }
}

/// The path component of a URL prefix (everything before `?`): strips a
/// `scheme://authority` head when present; a bare path passes through.
fn url_path(prefix: &str) -> &str {
    match prefix.find("://") {
        Some(i) => {
            let rest = &prefix[i + 3..];
            match rest.find('/') {
                Some(j) => &rest[j..],
                None => "",
            }
        }
        None => prefix,
    }
}

/// Extract the authorization code from a `&`-joined query string.
/// `code_autorisation` wins over `code` (desktop parity).
fn code_from_query(query: &str) -> Option<String> {
    let mut code_aut: Option<String> = None;
    let mut code_plain: Option<String> = None;
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        match k {
            "code_autorisation" => code_aut = decode(v),
            "code" => code_plain = decode(v),
            _ => {}
        }
    }
    code_aut.or(code_plain)
}

fn decode(v: &str) -> Option<String> {
    urlencoding::decode(v).ok().map(|s| s.into_owned())
}

/// A 48-hex-char (24-byte) CSPRNG nonce, bound into the redirect-URL path.
fn gen_nonce() -> String {
    use rand::RngExt;
    let mut bytes = [0u8; 24];
    rand::rng().fill(&mut bytes);
    let mut s = String::with_capacity(48);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

// ============================ IO helpers ============================

/// Compose the minimal client-only runtime: a headless [`NoOpAdapter`] and
/// default audio settings (no store is opened — login never touches audio),
/// then `init()` to extract the Qobuz bundle tokens the sign-in calls need.
async fn build_login_runtime() -> Result<AppRuntime<NoOpAdapter>, LoginError> {
    let runtime =
        AppRuntime::with_audio_settings(NoOpAdapter, None, AudioSettings::default(), None);
    if let Err(e) = runtime.init().await {
        return Err(LoginError::Failed(format!(
            "could not reach Qobuz to start login: {e}\n  → check your connection and retry"
        )));
    }
    Ok(runtime)
}

async fn read_app_id(runtime: &AppRuntime<NoOpAdapter>) -> Result<String, LoginError> {
    let client_lock = runtime.core().client();
    let guard = client_lock.read().await;
    let client = guard.as_ref().ok_or_else(|| {
        LoginError::Failed(
            "Qobuz client not initialized — could not reach Qobuz\n  \
             → check your connection and retry"
                .to_string(),
        )
    })?;
    client
        .app_id()
        .await
        .map_err(|e| LoginError::Failed(format!("could not read the Qobuz app id: {e}")))
}

async fn exchange_code(
    runtime: &AppRuntime<NoOpAdapter>,
    code: &str,
) -> Result<UserSession, LoginError> {
    let client_lock = runtime.core().client();
    let guard = client_lock.read().await;
    let client = guard
        .as_ref()
        .ok_or_else(|| LoginError::Failed("Qobuz client not initialized".to_string()))?;
    client.login_with_oauth_code(code).await.map_err(map_api_err)
}

/// Register the secret, persist the token into the daemon config root (0600),
/// then best-effort nudge a running daemon. Persist happens ONLY here — after
/// the caller already live-validated the session.
fn finalize(roots: &ProfileRoots, session: &UserSession) -> Result<(), LoginError> {
    // §6.3: register before the token can reach any log line (idempotent — the
    // token path already registered it in `validate_token`).
    qbz_log::register_secret(session.user_auth_token.clone());
    qbz_credentials::save_oauth_token_at(&roots.config, &session.user_auth_token).map_err(|e| {
        LoginError::Failed(format!(
            "could not save credentials to {}: {e}",
            roots.config.display()
        ))
    })?;
    let host = nudge_host(roots);
    // token: opt-in [server] token, wired by T6.
    let _ = nudge_reload(&host, None);
    Ok(())
}

/// The local daemon's reload address. Credentials are written to the LOCAL
/// config root, so the daemon to nudge is always local; its port comes from the
/// same `qbzd.toml` the daemon reads (default 8182).
fn nudge_host(roots: &ProfileRoots) -> String {
    let port = crate::config::QbzdConfig::load(&roots.config.join("qbzd.toml"))
        .map(|(c, _)| c.server.port)
        .unwrap_or(8182);
    format!("127.0.0.1:{port}")
}

fn read_stdin_line() -> Result<String, LoginError> {
    use std::io::BufRead;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| LoginError::Failed(format!("could not read from stdin: {e}")))?;
    Ok(line)
}

const SUCCESS_HTML: &str = "<html><body style=\"font-family:system-ui;text-align:center;padding:64px;background:#0f0f0f;color:#fff\">\
<h2>Login successful</h2><p>You can close this tab and return to your terminal.</p></body></html>";
const WAITING_HTML: &str = "<html><body style=\"font-family:system-ui;text-align:center;padding:64px;background:#0f0f0f;color:#fff\">\
<h2>Waiting for Qobuz…</h2></body></html>";

/// Accept connections until one carries a nonce-valid authorization code, then
/// return it and stop (exactly one accepted). Browser noise and nonce-mismatched
/// requests are answered with a neutral page and skipped. Non-blocking with a
/// 100 ms poll so the deadline is honored without a background thread leak — this
/// runs inside `spawn_blocking` and self-terminates at `deadline`.
fn capture_callback(
    listener: TcpListener,
    expected_nonce: &str,
    deadline: Instant,
) -> std::io::Result<Option<String>> {
    listener.set_nonblocking(true)?;
    loop {
        if Instant::now() >= deadline {
            return Ok(None);
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).ok();
                stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let request_line = request.lines().next().unwrap_or("");
                let code = parse_callback(request_line, expected_nonce);

                let body = if code.is_some() { SUCCESS_HTML } else { WAITING_HTML };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body,
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();

                if code.is_some() {
                    return Ok(code);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e),
        }
    }
}

fn http_request_2xx(host: &str, method: &str, path: &str, token: Option<&str>) -> bool {
    let addr = match host.to_socket_addrs().ok().and_then(|mut a| a.next()) {
        Some(a) => a,
        None => return false,
    };
    let mut stream = match TcpStream::connect_timeout(&addr, Duration::from_millis(600)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{auth}Content-Length: 0\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let _ = stream.flush();
    let mut buf = [0u8; 128];
    let n = stream.read(&mut buf).unwrap_or(0);
    let status = String::from_utf8_lossy(&buf[..n]);
    matches!(
        status.lines().next().and_then(|l| l.split_whitespace().nth(1)),
        Some(code) if code.starts_with('2')
    )
}

// ============================ error mapping ============================

fn map_api_err(e: qbz_qobuz::ApiError) -> LoginError {
    match e {
        qbz_qobuz::ApiError::AuthenticationError(_) | qbz_qobuz::ApiError::IneligibleUser => {
            LoginError::Rejected(e.to_string())
        }
        other => LoginError::Failed(other.to_string()),
    }
}

fn map_core_err(e: qbz_core::CoreError) -> LoginError {
    if matches!(
        e,
        qbz_core::CoreError::Api(
            qbz_qobuz::ApiError::AuthenticationError(_) | qbz_qobuz::ApiError::IneligibleUser
        )
    ) {
        LoginError::Rejected(e.to_string())
    } else {
        LoginError::Failed(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_oauth_url_embeds_nonce_in_the_redirect_path() {
        // Step 1(a), amended: the URL embeds
        // redirect_url=http://<host>:<port>/<nonce> — the nonce rides the PATH,
        // never a state param the provider would have to echo.
        let url = build_oauth_url("app123", "127.0.0.1", 39114, "NONCEabc");
        assert!(url.starts_with("https://www.qobuz.com/signin/oauth?"), "{url}");
        assert!(url.contains("ext_app_id=app123"), "{url}");
        let decoded = urlencoding::decode(&url).unwrap();
        assert!(
            decoded.contains("redirect_url=http://127.0.0.1:39114/NONCEabc"),
            "{decoded}"
        );
    }

    #[test]
    fn callback_host_is_embedded_in_the_redirect() {
        // Step 1(c): --callback-host embeds that host in the redirect URL.
        let url = build_oauth_url("app123", "192.168.0.40", 40000, "n");
        let decoded = urlencoding::decode(&url).unwrap();
        assert!(
            decoded.contains("redirect_url=http://192.168.0.40:40000/n"),
            "{decoded}"
        );
    }

    #[test]
    fn parse_callback_accepts_matching_path_nonce_and_extracts_code() {
        let line = "GET /abc123?code_autorisation=THECODE HTTP/1.1";
        assert_eq!(parse_callback(line, "abc123"), Some("THECODE".to_string()));
    }

    #[test]
    fn parse_callback_falls_back_to_plain_code() {
        let line = "GET /n?code=plaincode HTTP/1.1";
        assert_eq!(parse_callback(line, "n"), Some("plaincode".to_string()));
    }

    #[test]
    fn parse_callback_prefers_code_autorisation_over_code() {
        let line = "GET /n?code=plain&code_autorisation=preferred HTTP/1.1";
        assert_eq!(parse_callback(line, "n"), Some("preferred".to_string()));
    }

    #[test]
    fn parse_callback_rejects_mismatched_path_nonce() {
        // Step 1(b): a wrong path nonce is dropped even with a valid-looking code.
        let line = "GET /WRONG?code_autorisation=THECODE HTTP/1.1";
        assert_eq!(parse_callback(line, "abc123"), None);
    }

    #[test]
    fn parse_callback_rejects_absent_path_nonce() {
        let line = "GET /?code_autorisation=THECODE HTTP/1.1";
        assert_eq!(parse_callback(line, "abc123"), None);
    }

    #[test]
    fn parse_callback_needs_no_state_param_and_ignores_one() {
        // The provider echoing state is exactly what we no longer depend on.
        let no_state = "GET /abc123?code=OK HTTP/1.1";
        assert_eq!(parse_callback(no_state, "abc123"), Some("OK".to_string()));
        let stray_state = "GET /abc123?state=whatever&code=OK HTTP/1.1";
        assert_eq!(parse_callback(stray_state, "abc123"), Some("OK".to_string()));
    }

    #[test]
    fn parse_callback_ignores_browser_noise() {
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1", "abc123"), None);
        assert_eq!(parse_callback("", "abc123"), None);
    }

    #[test]
    fn parse_callback_percent_decodes_the_code() {
        let line = "GET /n?code=x%2Fy HTTP/1.1";
        assert_eq!(parse_callback(line, "n"), Some("x/y".to_string()));
    }

    #[test]
    fn code_from_paste_accepts_full_redirect_url_with_path_nonce() {
        let pasted = "http://127.0.0.1:43717/nn?code_autorisation=PASTED";
        assert_eq!(code_from_paste(pasted, "nn"), Some("PASTED".to_string()));
    }

    #[test]
    fn code_from_paste_tolerates_a_missing_path_nonce() {
        // Lenient by design: the operator pasted the URL by hand.
        let pasted = "http://127.0.0.1:43717/?code_autorisation=PASTED";
        assert_eq!(code_from_paste(pasted, "nn"), Some("PASTED".to_string()));
    }

    #[test]
    fn code_from_paste_accepts_a_bare_code() {
        assert_eq!(code_from_paste("JUSTACODE", "nn"), Some("JUSTACODE".to_string()));
        assert_eq!(code_from_paste("  JUSTACODE  ", "nn"), Some("JUSTACODE".to_string()));
    }

    #[test]
    fn code_from_paste_rejects_mismatched_path_nonce_in_url() {
        let pasted = "http://127.0.0.1:43717/WRONG?code_autorisation=PASTED";
        assert_eq!(code_from_paste(pasted, "nn"), None);
    }

    #[test]
    fn code_from_paste_rejects_empty_input() {
        assert_eq!(code_from_paste("   ", "nn"), None);
    }

    #[test]
    fn gen_nonce_is_long_unique_and_hex() {
        let a = gen_nonce();
        let b = gen_nonce();
        assert_ne!(a, b, "two nonces collided");
        assert_eq!(a.len(), 48, "nonce length: {}", a.len());
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "non-hex: {a}");
    }

    #[test]
    fn nudge_reload_is_false_when_daemon_is_down() {
        // Bind then immediately drop to obtain a definitely-closed local port.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        assert!(!nudge_reload(&format!("127.0.0.1:{port}"), None));
    }

    #[test]
    fn login_timeout_error_renders_the_verbatim_copy_with_the_port() {
        let rendered = LoginError::Timeout(39114).to_string();
        assert!(rendered.contains("no OAuth redirect received within 300 s"), "{rendered}");
        assert!(rendered.contains("ssh -L 39114:localhost:39114"), "{rendered}");
        assert!(rendered.contains("qbzd login --paste"), "{rendered}");
        assert!(rendered.contains("qbzd login --token <user_auth_token>"), "{rendered}");
    }
}
