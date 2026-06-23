//! System-browser OAuth login for the Slint MVP.
//!
//! Mirrors the QBZ Tauri `v2_start_system_browser_oauth` flow without any
//! Tauri or WebView dependency: it opens the user's default browser to the
//! Qobuz OAuth page with a localhost redirect, captures the authorization
//! code on a one-shot local HTTP listener, exchanges it through the core
//! Qobuz client, and activates the per-user session via `AppRuntime`.

use std::sync::Arc;
use std::time::Duration;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const OAUTH_TIMEOUT: Duration = Duration::from_secs(180);

/// The authenticated user, as the shell needs it.
pub struct SessionInfo {
    pub user_id: u64,
    pub display_name: String,
    pub subscription: String,
}

/// Run the full system-browser OAuth login. Returns the authenticated
/// session info on success.
pub async fn login_via_system_browser<A>(
    runtime: &Arc<AppRuntime<A>>,
) -> Result<SessionInfo, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let core = runtime.core();

    // NOTE: no offline_session pre-clear here. The sign-in endpoints are
    // EXEMPT from the offline gate (qbz-qobuz raw-client auth methods since
    // c207f232), so a live offline session never blocks the login itself —
    // and session activation (`runtime.activate`) is purely local. The old
    // upfront clear ended the offline session the moment the attempt
    // STARTED, which unlocked the shell (empty Discover/Library) while the
    // browser OAuth was still pending. The flag now drops on SUCCESS only
    // (below); the one still-gated dependency — a cold bundle-token init —
    // is scoped inside ensure_api_initialized.
    ensure_api_initialized(core).await?;

    let app_id = {
        let client_lock = core.client();
        let guard = client_lock.read().await;
        let client = guard.as_ref().ok_or("Qobuz client not initialized")?;
        client.app_id().await.map_err(|e| e.to_string())?
    };

    // One-shot local listener for the OAuth redirect.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind OAuth listener: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let oauth_url = format!(
        "https://www.qobuz.com/signin/oauth?ext_app_id={}&redirect_url={}",
        app_id,
        urlencoding::encode(&format!("http://localhost:{port}")),
    );

    log::info!("[qbz-slint] opening system browser for OAuth (port {port})");
    open::that(&oauth_url).map_err(|e| format!("Failed to open browser: {e}"))?;

    let code = tokio::time::timeout(OAUTH_TIMEOUT, capture_oauth_code(listener))
        .await
        .map_err(|_| "OAuth login timed out".to_string())?
        .ok_or_else(|| "OAuth login cancelled or no code received".to_string())?;

    log::info!("[qbz-slint] OAuth code captured, exchanging for session");

    let session = {
        let client_lock = core.client();
        let guard = client_lock.read().await;
        let client = guard.as_ref().ok_or("Qobuz client not initialized")?;
        match client.login_with_oauth_code(&code).await {
            Ok(session) => session,
            Err(e) => {
                // D4 producer: ONLY an explicit ineligible-account verdict
                // starts the grace clock. Generic 401/network errors never do.
                if matches!(e, qbz_qobuz::ApiError::IneligibleUser) {
                    crate::offline_mode::subscription_mark_invalid();
                }
                return Err(e.to_string());
            }
        }
    };
    let user_id = session.user_id;
    let display_name = session.display_name.clone();
    let subscription = session.subscription_label.clone();
    let token = session.user_auth_token.clone();

    // Emit LoggedIn through the core (idempotent set_session).
    core.set_session(session).await.map_err(|e| e.to_string())?;

    // Activate the per-user session (creates dirs, opens the session store).
    runtime.activate(user_id).await?;

    // Bring up the per-user offline cache (shared index.db + library.db with Tauri).
    crate::offline::activate(user_id).await;
    crate::offline_cache::load_cached_ids().await;

    // Offline-MODE per-user binding (after offline::activate so the purge
    // consumer can reach the cache), then the D4 valid verdict and the D2
    // recovery: a successful login ends any unauthenticated offline session.
    if let Some(dir) = crate::offline_mode::user_data_dir(user_id) {
        crate::offline_mode::init_for_user(&dir);
        crate::fav_cache::init_for_user(&dir);
        crate::discover_prefs::init_for_user(&dir);
        crate::artist_blacklist::init_for_user(&dir);
        // Intelligent Search (cache + ranking), seeded from the persisted pref.
        crate::search_service::init(&dir, crate::ui_prefs::load().intelligent_search);
        // Session persistence (queue + playback): open the per-user session.db
        // and seed the persist/resume gates from the playback prefs.
        crate::session_persist::init_for_user(&dir);
    }
    // Lyrics cache (per-user, shared lyrics.db with Tauri).
    crate::lyrics::init_for_user(core.client(), user_id);
    crate::offline_mode::subscription_mark_valid();
    crate::offline_mode::engine().set_offline_session(false);

    // Persist the token so the next launch restores the session silently.
    if let Err(e) = qbz_credentials::save_oauth_token(&token) {
        log::warn!("[qbz-slint] failed to persist OAuth token: {e}");
    }

    log::info!("[qbz-slint] login complete for user {user_id}");
    Ok(SessionInfo {
        user_id,
        display_name,
        subscription,
    })
}

/// Make sure the Qobuz client holds bundle tokens before a sign-in call.
///
/// The sign-in POSTs are gate-exempt, but `try_init_api`'s cold bundle
/// fetch is a network SERVICE request and stays gated on purpose. When an
/// unauthenticated offline session holds the gate closed (sign-in from the
/// offline shell's badge flyout / recovery path), lift the session flag
/// ONLY around this init and put it back immediately after, success or
/// failure — the offline session must end exclusively on a COMPLETED
/// login (the callers' success paths), never as a side effect of merely
/// starting an attempt. No-op in the normal case: an offline session boots
/// from cached bundle tokens, so the client is already initialized and the
/// flag is never touched.
async fn ensure_api_initialized<A>(core: &qbz_core::QbzCore<A>) -> Result<(), String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    if core.is_api_initialized().await {
        return Ok(());
    }
    let offline_engine = crate::offline_mode::engine();
    let lifted = offline_engine.status().offline_session;
    if lifted {
        log::info!(
            "[qbz-slint] cold bundle init with an offline session active — lifting the flag for the init only"
        );
        offline_engine.set_offline_session(false);
    }
    let result = core.try_init_api().await.map_err(|e| e.to_string());
    if lifted {
        offline_engine.set_offline_session(true);
    }
    result
}

/// True only for an EXPLICIT auth rejection from Qobuz: a 401 on the token
/// login (`AuthenticationError`) or an ineligible-account verdict. Network
/// failures, the offline gate, 5xx, rate limiting, parse errors and unknown
/// statuses (`ApiResponse`) all return false — on those the saved token must
/// be KEPT (spec §4.1 D1: the boot token-clearing bug).
fn is_auth_rejection(error: &qbz_core::CoreError) -> bool {
    matches!(
        error,
        qbz_core::CoreError::Api(
            qbz_qobuz::ApiError::AuthenticationError(_) | qbz_qobuz::ApiError::IneligibleUser
        )
    )
}

/// Restore a previously saved session from the encrypted token store
/// (keyring + AES-256-GCM file — the same store the Tauri app uses).
///
/// Returns `Ok(Some(SessionInfo))` when a saved token is valid and the
/// session is activated, `Ok(None)` when there is no token. A token that
/// exists but is explicitly rejected by Qobuz is cleared and treated as
/// `None`; on network-class failures the token is kept so the session can
/// still be restored later (offline boot, D2 recovery).
pub async fn restore_saved_session<A>(
    runtime: &Arc<AppRuntime<A>>,
) -> Result<Option<SessionInfo>, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let token = match qbz_credentials::load_oauth_token() {
        Ok(Some(token)) => token,
        Ok(None) => return Ok(None),
        Err(e) => {
            log::warn!("[qbz-slint] could not read saved token: {e}");
            return Ok(None);
        }
    };

    let core = runtime.core();
    // Same scoped handling of the gated cold bundle init as the browser
    // flow (no-op when the client already holds tokens).
    ensure_api_initialized(core).await?;

    match core.login_with_token(&token).await {
        Ok(session) => {
            let user_id = session.user_id;
            let display_name = session.display_name.clone();
            let subscription = session.subscription_label.clone();
            core.set_session(session).await.map_err(|e| e.to_string())?;
            runtime.activate(user_id).await?;
            crate::offline::activate(user_id).await;
            crate::offline_cache::load_cached_ids().await;
            // Offline-MODE per-user binding + D4 valid verdict + D2 recovery
            // (same ordering as login_via_system_browser).
            if let Some(dir) = crate::offline_mode::user_data_dir(user_id) {
                crate::offline_mode::init_for_user(&dir);
                crate::fav_cache::init_for_user(&dir);
                crate::discover_prefs::init_for_user(&dir);
                crate::artist_blacklist::init_for_user(&dir);
                // Intelligent Search (cache + ranking), seeded from the pref.
                crate::search_service::init(&dir, crate::ui_prefs::load().intelligent_search);
                // Session persistence (queue + playback): open the per-user
                // session.db and seed the persist/resume gates.
                crate::session_persist::init_for_user(&dir);
            }
            // Lyrics cache (per-user, shared lyrics.db with Tauri).
            crate::lyrics::init_for_user(core.client(), user_id);
            crate::offline_mode::subscription_mark_valid();
            crate::offline_mode::engine().set_offline_session(false);
            log::info!("[qbz-slint] restored saved session for user {user_id}");
            Ok(Some(SessionInfo {
                user_id,
                display_name,
                subscription,
            }))
        }
        Err(e) if is_auth_rejection(&e) => {
            log::warn!("[qbz-slint] saved token rejected by Qobuz, clearing: {e}");
            let _ = qbz_credentials::clear_oauth_token();
            // D4 producer: only the explicit ineligible verdict starts the
            // grace clock; a plain 401 does not.
            if matches!(
                &e,
                qbz_core::CoreError::Api(qbz_qobuz::ApiError::IneligibleUser)
            ) {
                crate::offline_mode::subscription_mark_invalid();
            }
            Ok(None)
        }
        Err(e) => {
            // Network-class failure (offline boot, timeout, 5xx, ...): KEEP
            // the token. The login screen shows with the session intact so
            // "Start offline" / the D2 recovery banner can use it later.
            log::warn!("[qbz-slint] session restore failed, keeping saved token: {e}");
            Ok(None)
        }
    }
}

/// Log out: clear the saved token, deactivate the per-user session, and
/// drop the Qobuz client session.
pub async fn logout<A>(runtime: &Arc<AppRuntime<A>>) -> Result<(), String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let _ = qbz_credentials::clear_oauth_token();
    let _ = runtime.core().logout().await;
    crate::offline::deactivate().await;
    crate::offline_mode::teardown();
    crate::fav_cache::teardown();
    crate::discover_prefs::teardown();
    crate::artist_blacklist::teardown();
    crate::search_service::teardown();
    crate::lyrics::teardown();
    runtime.deactivate().await?;
    log::info!("[qbz-slint] logged out");
    Ok(())
}

/// Accept connections until one carries the OAuth code, replying with a
/// minimal success page. Browser noise (favicon requests, etc.) is answered
/// and skipped.
async fn capture_oauth_code(listener: TcpListener) -> Option<String> {
    loop {
        let (mut stream, _) = listener.accept().await.ok()?;
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).await.ok()?;
        let request = String::from_utf8_lossy(&buf[..n]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("");
        let code = query_param(target, "code_autorisation")
            .or_else(|| query_param(target, "code"));

        let body = if code.is_some() {
            "<html><body style=\"font-family:system-ui;text-align:center;padding:64px;background:#0f0f0f;color:#fff\">\
             <h2>Login successful</h2><p>You can close this tab and return to QBZ.</p></body></html>"
        } else {
            "<html><body style=\"font-family:system-ui;text-align:center;padding:64px;background:#0f0f0f;color:#fff\">\
             <h2>Waiting for Qobuz...</h2></body></html>"
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.flush().await;

        if code.is_some() {
            return code;
        }
    }
}

/// Extract and percent-decode a query parameter from an HTTP request target.
fn query_param(target: &str, key: &str) -> Option<String> {
    let query = target.split_once('?')?.1;
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if k == key {
            return urlencoding::decode(v).ok().map(|s| s.into_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::query_param;

    #[test]
    fn query_param_extracts_and_decodes() {
        assert_eq!(
            query_param("/?code_autorisation=abc123", "code_autorisation"),
            Some("abc123".to_string())
        );
        assert_eq!(
            query_param("/?a=1&code=x%2Fy", "code"),
            Some("x/y".to_string())
        );
        assert_eq!(query_param("/favicon.ico", "code"), None);
        assert_eq!(query_param("/?other=1", "code"), None);
    }
}
