use std::sync::Arc;
use tokio::sync::broadcast;

use qbz_audio::{AudioDiagnostic, AudioSettings};
use qbz_audio::settings::AudioSettingsStore;
use qbz_cache::AudioCache;
use qbz_core::QbzCore;
use qbz_player::Player;

use tokio::sync::RwLock;

use crate::adapter::{DaemonAdapter, DaemonEvent};
use crate::config::DaemonConfig;
use crate::session::UserSession;

/// Central state container for the headless daemon.
/// Replaces Tauri's app.state::<T>() with direct field access.
pub struct DaemonCore {
    pub config: DaemonConfig,
    pub core: Arc<QbzCore<DaemonAdapter>>,
    pub audio_cache: Arc<AudioCache>,
    pub event_bus: broadcast::Sender<DaemonEvent>,
    /// Per-user state, populated after login + session activation
    pub user: RwLock<Option<UserSession>>,
}

/// Run the daemon main loop.
pub async fn run(mut config: DaemonConfig) -> Result<(), String> {
    // Resolve token
    config.resolve_token();
    log::info!("[qbzd] API token: {}...", &config.server.token[..8.min(config.server.token.len())]);

    // Create event bus (bounded, slow SSE clients get dropped)
    let (event_tx, _) = broadcast::channel::<DaemonEvent>(256);

    // Create adapter for QbzCore events
    let adapter = DaemonAdapter::new(event_tx.clone());

    // Load audio settings (from shared database if exists)
    let audio_settings = AudioSettingsStore::new()
        .ok()
        .and_then(|store| store.get_settings().ok())
        .unwrap_or_else(|| {
            log::info!("[qbzd] No saved audio settings, using defaults");
            AudioSettings::default()
        });

    let device_name = audio_settings.output_device.clone();
    log::info!(
        "[qbzd] Audio: backend={:?}, device={:?}, exclusive={}, gapless={}",
        audio_settings.backend_type,
        device_name,
        audio_settings.exclusive_mode,
        config.audio.gapless,
    );

    // Create player (audio thread starts immediately)
    let diagnostic = AudioDiagnostic::new();
    let player = Player::new(device_name, audio_settings, None, diagnostic);

    // Create QbzCore — this extracts Qobuz bundle tokens (requires network)
    let core = QbzCore::new(adapter, player);
    log::info!("[qbzd] Initializing QbzCore (extracting Qobuz bundle tokens)...");
    core.init().await.map_err(|e| format!("QbzCore init failed: {}", e))?;
    log::info!("[qbzd] QbzCore initialized");

    // Create audio cache with configured sizes
    let cache_bytes = config.memory_cache_bytes();
    let audio_cache = Arc::new(AudioCache::new(cache_bytes));
    log::info!("[qbzd] Audio cache: {} MB", config.cache.memory_mb);

    let core = Arc::new(core);

    let daemon = Arc::new(DaemonCore {
        config: config.clone(),
        core: core.clone(),
        audio_cache,
        event_bus: event_tx.clone(),
        user: RwLock::new(None),
    });

    // Try auto-login from saved OAuth token
    match try_auto_login(&core).await {
        Some(user_id) => {
            log::info!("[qbzd] Auto-login successful (user_id: {})", user_id);
            // Activate per-user session (initialize stores, sync settings)
            match crate::session::activate_session(user_id, &core, &event_tx).await {
                Ok(session) => {
                    *daemon.user.write().await = Some(session);
                    log::info!("[qbzd] User session activated");
                }
                Err(e) => {
                    log::error!("[qbzd] Session activation failed: {}", e);
                }
            }
        }
        None => {
            log::warn!("[qbzd] No saved credentials. Run `qbzd login` to authenticate.");
        }
    }

    log::info!(
        "[qbzd] Daemon ready on {}:{}",
        config.server.bind,
        config.server.port
    );

    // Start HTTP server
    let bind_addr = format!("{}:{}", config.server.bind, config.server.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| format!("Failed to bind {}: {}", bind_addr, e))?;

    log::info!("[qbzd] HTTP server listening on {}", bind_addr);

    // Axum router
    let app = build_router(daemon.clone());

    // Graceful shutdown on SIGTERM/SIGINT
    let shutdown = async {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to register SIGTERM handler");

        #[cfg(unix)]
        tokio::select! {
            _ = ctrl_c => log::info!("[qbzd] Received SIGINT, shutting down..."),
            _ = sigterm.recv() => log::info!("[qbzd] Received SIGTERM, shutting down..."),
        }

        #[cfg(not(unix))]
        ctrl_c.await.ok();
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| format!("HTTP server error: {}", e))?;

    // Graceful shutdown: stop player
    log::info!("[qbzd] Stopping player...");
    let _ = daemon.core.stop();

    log::info!("[qbzd] Shutdown complete");
    Ok(())
}

/// Try to restore session from saved OAuth token in keyring.
/// Returns the user_id on success.
async fn try_auto_login(core: &QbzCore<DaemonAdapter>) -> Option<u64> {
    let token = load_oauth_token()?;

    match core.login_with_token(&token).await {
        Ok(session) => {
            log::info!(
                "[qbzd] Session restored for user {} ({})",
                session.display_name,
                session.user_id
            );
            Some(session.user_id)
        }
        Err(e) => {
            log::warn!("[qbzd] Saved token expired or invalid: {}", e);
            None
        }
    }
}

/// Load OAuth token from system keyring.
/// Uses the same service/key as the desktop app so credentials are shared.
fn load_oauth_token() -> Option<String> {
    const SERVICE: &str = "qbz-player";
    const KEY: &str = "qobuz-oauth-token";

    let entry = keyring::Entry::new(SERVICE, KEY).ok()?;
    match entry.get_password() {
        Ok(token) if !token.is_empty() => {
            log::info!("[qbzd] OAuth token loaded from keyring");
            Some(token)
        }
        Ok(_) => None,
        Err(keyring::Error::NoEntry) => {
            log::debug!("[qbzd] No OAuth token in keyring");
            None
        }
        Err(e) => {
            log::warn!("[qbzd] Keyring access failed: {}", e);
            None
        }
    }
}

/// Build the Axum HTTP router.
fn build_router(daemon: Arc<DaemonCore>) -> axum::Router {
    use axum::routing::get;

    axum::Router::new()
        .route("/api/ping", get(ping_handler))
        .route(
            "/api/info",
            get({
                let d = daemon.clone();
                move || info_handler(d.clone())
            }),
        )
        .route(
            "/api/status",
            get({
                let d = daemon.clone();
                move || status_handler(d.clone())
            }),
        )
}

async fn ping_handler() -> &'static str {
    "pong"
}

async fn info_handler(daemon: Arc<DaemonCore>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "name": "qbzd",
        "version": env!("CARGO_PKG_VERSION"),
        "cache": {
            "memory_mb": daemon.config.cache.memory_mb,
            "disk_mb": daemon.config.cache.disk_mb,
            "prefetch_count": daemon.config.cache.prefetch_count,
        },
    }))
}

async fn status_handler(daemon: Arc<DaemonCore>) -> axum::Json<serde_json::Value> {
    let logged_in = daemon.core.has_session().await;
    axum::Json(serde_json::json!({
        "state": if logged_in { "ready" } else { "no_session" },
        "logged_in": logged_in,
        "audio": {
            "cache_mb": daemon.config.cache.memory_mb,
        },
    }))
}
