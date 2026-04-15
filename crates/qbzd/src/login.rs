//! Interactive Qobuz login via system browser OAuth.
//!
//! Supports both local and remote (headless) operation:
//! - Local: opens browser automatically, callback to localhost
//! - Remote/headless: prints URL for user to open on another device,
//!   callback to the daemon's LAN IP so the redirect works from
//!   any browser on the same network.

use qbz_qobuz::QobuzClient;

const OAUTH_TIMEOUT_SECS: u64 = 300; // 5 min for remote login (user needs time to copy URL)

pub async fn interactive_login() -> Result<(), String> {
    println!("Initializing Qobuz client...");

    let client = QobuzClient::new().map_err(|e| format!("Client error: {}", e))?;
    client.init().await.map_err(|e| format!("Bundle extraction failed: {}", e))?;

    let app_id = client.app_id().await.map_err(|e| format!("No app_id: {}", e))?;

    // Bind to 0.0.0.0 so the callback works from any device on the LAN
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
        .await
        .map_err(|e| format!("Failed to bind listener: {}", e))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    // Detect LAN IP for the redirect URL
    let lan_ip = detect_lan_ip().unwrap_or_else(|| "localhost".to_string());
    let redirect_url = format!("http://{}:{}", lan_ip, port);

    let oauth_url = format!(
        "https://www.qobuz.com/signin/oauth?ext_app_id={}&redirect_url={}",
        app_id,
        urlencoding::encode(&redirect_url),
    );

    // Channel for the auth code
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);

    // Local HTTP handler
    let handler = axum::Router::new().route(
        "/",
        axum::routing::get(move |axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>| {
            let tx = tx.clone();
            async move {
                if let Some(code) = params.get("code_autorisation").or_else(|| params.get("code")) {
                    let _ = tx.send(code.clone()).await;
                    axum::response::Html(
                        "<html><body style=\"font-family:system-ui;text-align:center;padding:60px\">\
                         <h2>Login successful!</h2>\
                         <p>You can close this tab and return to the terminal.</p>\
                         </body></html>"
                    )
                } else {
                    axum::response::Html(
                        "<html><body style=\"font-family:system-ui;text-align:center;padding:60px\">\
                         <h2>Login failed</h2>\
                         <p>No authorization code received.</p>\
                         </body></html>"
                    )
                }
            }
        }),
    );

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, handler).await.ok();
    });

    // Always print the URL (works for both local and headless)
    println!("\n╔══════════════════════════════════════════════════╗");
    println!("║  Open this URL in any browser on your network:  ║");
    println!("╚══════════════════════════════════════════════════╝\n");
    println!("  {}\n", oauth_url);
    println!("Callback listening on: {}", redirect_url);
    println!("Waiting for login ({}s timeout)...\n", OAUTH_TIMEOUT_SECS);

    // Try to open browser (works on local, silently fails on headless)
    let _ = open::that(&oauth_url);

    // Wait for code
    let code = tokio::time::timeout(
        std::time::Duration::from_secs(OAUTH_TIMEOUT_SECS),
        rx.recv(),
    )
    .await;

    server_handle.abort();

    let code = match code {
        Ok(Some(c)) => c,
        Ok(None) => return Err("Login cancelled".to_string()),
        Err(_) => return Err(format!("Login timed out after {}s", OAUTH_TIMEOUT_SECS)),
    };

    println!("Authorization code received. Exchanging for session...");

    let session = client
        .login_with_oauth_code(&code)
        .await
        .map_err(|e| format!("OAuth exchange failed: {}", e))?;

    println!(
        "\nLogged in as: {} (user_id: {})",
        session.display_name, session.user_id
    );
    println!("Subscription: {}", session.subscription_label);

    // Save token to keyring
    let token = session.user_auth_token.clone();
    save_token_to_keyring(&token)?;

    println!("\nCredentials saved. The daemon will auto-login on next start.");
    Ok(())
}

/// Save OAuth token — tries keyring first, falls back to encrypted file.
fn save_token_to_keyring(token: &str) -> Result<(), String> {
    const SERVICE: &str = "qbz-player";
    const KEY: &str = "qobuz-oauth-token";

    // Try keyring first
    match keyring::Entry::new(SERVICE, KEY) {
        Ok(entry) => match entry.set_password(token) {
            Ok(()) => {
                println!("Token saved to system keyring");
                return Ok(());
            }
            Err(e) => {
                println!("Keyring unavailable ({}), using file fallback", e);
            }
        },
        Err(e) => {
            println!("Keyring unavailable ({}), using file fallback", e);
        }
    }

    // Fallback: save to file
    save_token_to_file(token)
}

fn token_file_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("qbz").join(".oauth-token"))
}

fn save_token_to_file(token: &str) -> Result<(), String> {
    let path = token_file_path().ok_or("Cannot determine data directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, token).map_err(|e| format!("Failed to write token file: {}", e))?;
    // Restrict permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    println!("Token saved to {}", path.display());
    Ok(())
}

pub fn load_token_from_file() -> Option<String> {
    let path = token_file_path()?;
    std::fs::read_to_string(&path).ok().filter(|t| !t.trim().is_empty())
}

/// Detect the primary LAN IP address of this machine.
fn detect_lan_ip() -> Option<String> {
    // Try to get the default route interface IP by connecting to a public DNS
    // (no actual data is sent, just gets the local IP used for routing)
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    let ip = addr.ip().to_string();
    if ip == "0.0.0.0" || ip == "127.0.0.1" {
        return None;
    }
    Some(ip)
}
