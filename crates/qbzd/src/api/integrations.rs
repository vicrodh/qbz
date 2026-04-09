use std::sync::Arc;
use axum::Json;
use serde::Deserialize;

use crate::daemon::DaemonCore;

// For now, integrations are read/write via the shared SQLite databases
// in the user's data directory. The V2 integration states
// (ListenBrainzV2State, etc.) live in src-tauri/integrations_v2.rs
// and are not yet in a standalone crate. As a workaround, we read/write
// the cache databases directly.

pub async fn get_listenbrainz_status(
    daemon: Arc<DaemonCore>,
) -> Result<Json<serde_json::Value>, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let cache_path = session.data_dir.join("cache").join("listenbrainz_v2.db");

    if !cache_path.exists() {
        return Ok(Json(serde_json::json!({"connected": false})));
    }

    let conn = rusqlite::Connection::open(&cache_path)
        .map_err(|e| format!("DB error: {}", e))?;
    let token: Option<String> = conn
        .query_row("SELECT value FROM config WHERE key = 'token'", [], |row| row.get(0))
        .ok();

    Ok(Json(serde_json::json!({
        "connected": token.is_some(),
        "token_preview": token.as_ref().map(|t| {
            if t.len() > 8 { format!("{}...{}", &t[..4], &t[t.len()-4..]) } else { "****".to_string() }
        }),
    })))
}

#[derive(Deserialize)]
pub struct ListenBrainzConnectRequest {
    pub token: String,
}

pub async fn connect_listenbrainz(
    daemon: Arc<DaemonCore>,
    Json(req): Json<ListenBrainzConnectRequest>,
) -> Result<Json<serde_json::Value>, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let cache_dir = session.data_dir.join("cache");
    std::fs::create_dir_all(&cache_dir).ok();
    let cache_path = cache_dir.join("listenbrainz_v2.db");

    let conn = rusqlite::Connection::open(&cache_path)
        .map_err(|e| format!("DB error: {}", e))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        .map_err(|e| format!("Schema error: {}", e))?;
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES ('token', ?1)",
        rusqlite::params![req.token],
    ).map_err(|e| format!("Save error: {}", e))?;

    log::info!("[qbzd] ListenBrainz token saved");
    Ok(Json(serde_json::json!({"connected": true})))
}

pub async fn disconnect_listenbrainz(
    daemon: Arc<DaemonCore>,
) -> Result<&'static str, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let cache_path = session.data_dir.join("cache").join("listenbrainz_v2.db");

    if cache_path.exists() {
        let conn = rusqlite::Connection::open(&cache_path)
            .map_err(|e| format!("DB error: {}", e))?;
        let _ = conn.execute("DELETE FROM config WHERE key = 'token'", []);
    }

    log::info!("[qbzd] ListenBrainz disconnected");
    Ok("ok")
}

pub async fn get_lastfm_status(
    daemon: Arc<DaemonCore>,
) -> Result<Json<serde_json::Value>, String> {
    // Last.fm V2 state doesn't have persistent cache yet in the crate
    // Just report not connected for now
    Ok(Json(serde_json::json!({
        "connected": false,
        "note": "Last.fm setup requires desktop app or TUI wizard"
    })))
}
