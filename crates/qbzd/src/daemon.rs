use std::sync::Arc;
use tokio::sync::broadcast;

use crate::adapter::{DaemonAdapter, DaemonEvent};
use crate::config::DaemonConfig;

/// Central state container for the headless daemon.
/// Replaces Tauri's app.state::<T>() with direct field access.
pub struct DaemonCore {
    pub config: DaemonConfig,
    pub event_bus: broadcast::Sender<DaemonEvent>,
    // Phase 1: Core crates initialized after login
    // pub core: Arc<QbzCore<DaemonAdapter>>,
    // pub audio_cache: Arc<qbz_cache::AudioCache>,
}

/// Run the daemon main loop.
pub async fn run(mut config: DaemonConfig) -> Result<(), String> {
    // Resolve token
    config.resolve_token();
    log::info!("[qbzd] API token: {}", &config.server.token[..8]);

    // Create event bus
    let (event_tx, _) = broadcast::channel::<DaemonEvent>(256);

    // Create adapter (will be used by QbzCore)
    let _adapter = DaemonAdapter::new(event_tx.clone());

    let daemon = Arc::new(DaemonCore {
        config: config.clone(),
        event_bus: event_tx,
    });

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

    // Minimal Axum router (Phase 0 — just ping)
    let app = axum::Router::new()
        .route("/api/ping", axum::routing::get(ping_handler))
        .route("/api/info", axum::routing::get({
            let daemon = daemon.clone();
            move || info_handler(daemon.clone())
        }));

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

    log::info!("[qbzd] Shutdown complete");
    Ok(())
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
