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

/// Run the full system-browser OAuth login. Returns the authenticated
/// user id on success.
pub async fn login_via_system_browser<A>(runtime: &Arc<AppRuntime<A>>) -> Result<u64, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let core = runtime.core();

    // Ensure the Qobuz client is initialized (bundle tokens).
    if !core.is_api_initialized().await {
        core.try_init_api().await.map_err(|e| e.to_string())?;
    }

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
        client
            .login_with_oauth_code(&code)
            .await
            .map_err(|e| e.to_string())?
    };
    let user_id = session.user_id;
    let token = session.user_auth_token.clone();

    // Emit LoggedIn through the core (idempotent set_session).
    core.set_session(session).await.map_err(|e| e.to_string())?;

    // Activate the per-user session (creates dirs, opens the session store).
    runtime.activate(user_id).await?;

    // Persist the token so the next launch restores the session silently.
    if let Err(e) = qbz_credentials::save_oauth_token(&token) {
        log::warn!("[qbz-slint] failed to persist OAuth token: {e}");
    }

    log::info!("[qbz-slint] login complete for user {user_id}");
    Ok(user_id)
}

/// Restore a previously saved session from the encrypted token store
/// (keyring + AES-256-GCM file — the same store the Tauri app uses).
///
/// Returns `Ok(Some(user_id))` when a saved token is valid and the session
/// is activated, `Ok(None)` when there is no token. A token that exists
/// but is rejected by Qobuz is cleared and treated as `None`.
pub async fn restore_saved_session<A>(runtime: &Arc<AppRuntime<A>>) -> Result<Option<u64>, String>
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
    if !core.is_api_initialized().await {
        core.try_init_api().await.map_err(|e| e.to_string())?;
    }

    match core.login_with_token(&token).await {
        Ok(session) => {
            let user_id = session.user_id;
            core.set_session(session).await.map_err(|e| e.to_string())?;
            runtime.activate(user_id).await?;
            log::info!("[qbz-slint] restored saved session for user {user_id}");
            Ok(Some(user_id))
        }
        Err(e) => {
            log::warn!("[qbz-slint] saved token rejected, clearing: {e}");
            let _ = qbz_credentials::clear_oauth_token();
            Ok(None)
        }
    }
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
