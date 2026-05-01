//! Persistence layer for QConnect startup mode + last-known state.
//!
//! Mirrors the device_name persistence pattern in `transport.rs`.
//! Reuses the existing `qconnect_settings.db` (key/value table).
//!
//! All operations are fail-open: any I/O or SQLite error returns the
//! default (Off / None) rather than propagating, so a corrupt DB never
//! prevents the app from starting.

use qconnect_app::QconnectStartupMode;

/// Path to the QConnect settings database (global, not per-user).
/// Same path used by `transport.rs::qconnect_settings_db_path`.
fn db_path() -> Option<std::path::PathBuf> {
    let data_dir = dirs::data_dir()?.join("qbz");
    std::fs::create_dir_all(&data_dir).ok()?;
    Some(data_dir.join("qconnect_settings.db"))
}

fn open_settings_conn() -> Option<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(db_path()?).ok()?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .ok()?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    )
    .ok()?;
    Some(conn)
}

/// Load the persisted startup mode. Returns `Off` (default) when missing or invalid.
pub fn load_startup_mode() -> QconnectStartupMode {
    let Some(conn) = open_settings_conn() else {
        return QconnectStartupMode::default();
    };
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'startup_mode'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok();
    value
        .as_deref()
        .and_then(QconnectStartupMode::from_str)
        .unwrap_or_default()
}

/// Persist the startup mode.
pub fn save_startup_mode(mode: QconnectStartupMode) {
    let Some(conn) = open_settings_conn() else {
        return;
    };
    let _ = conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('startup_mode', ?1)",
        rusqlite::params![mode.as_str()],
    );
}

/// Load the last-known QConnect on/off state, if recorded.
pub fn load_last_known_state() -> Option<bool> {
    let conn = open_settings_conn()?;
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'last_known_state'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok();
    match value.as_deref() {
        Some("on") => Some(true),
        Some("off") => Some(false),
        _ => None,
    }
}

/// Persist the last-known on/off state. Called from the V2 connect/disconnect
/// commands when `startup_mode == RememberLast`.
pub fn save_last_known_state(state: bool) {
    let Some(conn) = open_settings_conn() else {
        return;
    };
    let value = if state { "on" } else { "off" };
    let _ = conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('last_known_state', ?1)",
        rusqlite::params![value],
    );
}
