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

    // Start playback state polling loop (broadcasts to event bus)
    spawn_playback_loop(daemon.core.clone(), daemon.event_bus.clone());

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

/// Spawn the playback state polling loop.
/// Reads player state and broadcasts PlaybackSnapshot events.
/// Adaptive polling: 250ms playing, 1s paused, 5s idle.
fn spawn_playback_loop(
    core: Arc<QbzCore<DaemonAdapter>>,
    event_tx: broadcast::Sender<DaemonEvent>,
) {
    tokio::spawn(async move {
        let mut last_position: u64 = 0;
        let mut last_is_playing = false;
        let mut last_track_id: u64 = 0;

        loop {
            let player = core.player();
            let state = &player.state;

            let is_playing = state.is_playing();
            let track_id = state.current_track_id();
            let position = state.current_position();
            let duration = state.duration();
            let volume = state.volume();
            let sample_rate = state.get_sample_rate();
            let bit_depth = state.get_bit_depth();

            let track_cleared = track_id == 0 && last_track_id != 0;
            let should_emit = (track_id != 0
                && (is_playing != last_is_playing
                    || track_id != last_track_id
                    || (is_playing && position != last_position)))
                || track_cleared;

            if should_emit {
                let snapshot = crate::adapter::PlaybackSnapshot {
                    state: if is_playing {
                        "Playing".to_string()
                    } else if track_id != 0 {
                        "Paused".to_string()
                    } else {
                        "Stopped".to_string()
                    },
                    track_id,
                    position_secs: position,
                    duration_secs: duration,
                    volume,
                    sample_rate,
                    bit_depth,
                };
                let _ = event_tx.send(DaemonEvent::Playback(snapshot));
                last_position = position;
                last_is_playing = is_playing;
                last_track_id = track_id;
            }

            // Adaptive polling
            let sleep_ms = if is_playing {
                250
            } else if track_id == 0 {
                5000
            } else {
                1000
            };
            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
        }
    });
}

/// Macro to reduce boilerplate for route handlers that take Arc<DaemonCore>
macro_rules! with_daemon {
    ($daemon:expr, $handler:path) => {{
        let d = $daemon.clone();
        move || $handler(d.clone())
    }};
    ($daemon:expr, $handler:path, json) => {{
        let d = $daemon.clone();
        move |body| $handler(d.clone(), body)
    }};
    ($daemon:expr, $handler:path, query) => {{
        let d = $daemon.clone();
        move |q| $handler(d.clone(), q)
    }};
    ($daemon:expr, $handler:path, path) => {{
        let d = $daemon.clone();
        move |p| $handler(d.clone(), p)
    }};
    ($daemon:expr, $handler:path, path_query) => {{
        let d = $daemon.clone();
        move |p, q| $handler(d.clone(), p, q)
    }};
}

/// Build the Axum HTTP router.
fn build_router(daemon: Arc<DaemonCore>) -> axum::Router {
    use axum::routing::{get, post};
    use crate::api::{playback, queue, search, catalog};

    axum::Router::new()
        // System
        .route("/api/ping", get(ping_handler))
        .route("/api/info", get(with_daemon!(daemon, info_handler)))
        .route("/api/status", get(with_daemon!(daemon, status_handler)))
        .route("/api/events", get(with_daemon!(daemon, crate::api::events::sse_handler)))
        // Playback
        .route("/api/playback", get(with_daemon!(daemon, playback::get_playback)))
        .route("/api/playback/play", post(with_daemon!(daemon, playback::play)))
        .route("/api/playback/pause", post(with_daemon!(daemon, playback::pause)))
        .route("/api/playback/stop", post(with_daemon!(daemon, playback::stop)))
        .route("/api/playback/next", post(with_daemon!(daemon, playback::next)))
        .route("/api/playback/previous", post(with_daemon!(daemon, playback::previous)))
        .route("/api/playback/seek", post(with_daemon!(daemon, playback::seek, json)))
        .route("/api/playback/volume", post(with_daemon!(daemon, playback::volume, json)))
        // Queue
        .route("/api/queue", get(with_daemon!(daemon, queue::get_queue)))
        .route("/api/queue/set", post(with_daemon!(daemon, queue::set_queue, json)))
        .route("/api/queue/add", post(with_daemon!(daemon, queue::add, json)))
        .route("/api/queue/add-next", post(with_daemon!(daemon, queue::add_next, json)))
        .route("/api/queue/play-index", post(with_daemon!(daemon, queue::play_index, json)))
        .route("/api/queue/remove", post(with_daemon!(daemon, queue::remove, json)))
        .route("/api/queue/move", post(with_daemon!(daemon, queue::move_track, json)))
        .route("/api/queue/clear", post(with_daemon!(daemon, queue::clear)))
        .route("/api/queue/shuffle", post(with_daemon!(daemon, queue::shuffle, json)))
        .route("/api/queue/repeat", post(with_daemon!(daemon, queue::repeat, json)))
        // Search
        .route("/api/search", get(with_daemon!(daemon, search::search, query)))
        // Catalog
        .route("/api/albums/{id}", get(with_daemon!(daemon, catalog::get_album, path)))
        .route("/api/artists/{id}", get(with_daemon!(daemon, catalog::get_artist, path)))
        .route("/api/tracks/{id}", get(with_daemon!(daemon, catalog::get_track, path)))
        .route("/api/tracks/batch", get(with_daemon!(daemon, catalog::get_tracks_batch, query)))
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
