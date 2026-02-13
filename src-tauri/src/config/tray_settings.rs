//! Tray icon settings
//!
//! Stores user preferences for system tray behavior:
//! - enable_tray: Show tray icon (requires restart)
//! - minimize_to_tray: Hide to tray when minimizing
//! - close_to_tray: Hide to tray when closing window

use log::info;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraySettings {
    /// Show system tray icon (requires restart to take effect)
    pub enable_tray: bool,
    /// Hide window to tray when clicking minimize
    pub minimize_to_tray: bool,
    /// Hide window to tray instead of quitting when clicking close
    pub close_to_tray: bool,
}

impl Default for TraySettings {
    fn default() -> Self {
        Self {
            enable_tray: true,
            minimize_to_tray: false,
            close_to_tray: false,
        }
    }
}

pub struct TraySettingsStore {
    conn: Connection,
}

impl TraySettingsStore {
    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open tray settings database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to enable WAL for tray settings database: {}", e))?;

        // Create table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tray_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                enable_tray INTEGER NOT NULL DEFAULT 1,
                minimize_to_tray INTEGER NOT NULL DEFAULT 0,
                close_to_tray INTEGER NOT NULL DEFAULT 0
            );",
        )
        .map_err(|e| format!("Failed to create tray settings table: {}", e))?;

        // Insert default row if it doesn't exist
        conn.execute(
            "INSERT OR IGNORE INTO tray_settings (id, enable_tray, minimize_to_tray, close_to_tray)
            VALUES (1, 1, 0, 0)",
            [],
        )
        .map_err(|e| format!("Failed to insert default tray settings: {}", e))?;

        info!("[TraySettings] Database initialized");

        Ok(Self { conn })
    }

    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "tray_settings.db")
    }

    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "tray_settings.db")
    }

    pub fn get_settings(&self) -> Result<TraySettings, String> {
        self.conn
            .query_row(
                "SELECT enable_tray, minimize_to_tray, close_to_tray FROM tray_settings WHERE id = 1",
                [],
                |row| {
                    let enable_tray: i32 = row.get(0)?;
                    let minimize_to_tray: i32 = row.get(1)?;
                    let close_to_tray: i32 = row.get(2)?;
                    Ok(TraySettings {
                        enable_tray: enable_tray != 0,
                        minimize_to_tray: minimize_to_tray != 0,
                        close_to_tray: close_to_tray != 0,
                    })
                },
            )
            .map_err(|e| format!("Failed to get tray settings: {}", e))
    }

    pub fn set_enable_tray(&self, value: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE tray_settings SET enable_tray = ?1 WHERE id = 1",
                params![if value { 1 } else { 0 }],
            )
            .map_err(|e| format!("Failed to set enable_tray: {}", e))?;
        Ok(())
    }

    pub fn set_minimize_to_tray(&self, value: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE tray_settings SET minimize_to_tray = ?1 WHERE id = 1",
                params![if value { 1 } else { 0 }],
            )
            .map_err(|e| format!("Failed to set minimize_to_tray: {}", e))?;
        Ok(())
    }

    pub fn set_close_to_tray(&self, value: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE tray_settings SET close_to_tray = ?1 WHERE id = 1",
                params![if value { 1 } else { 0 }],
            )
            .map_err(|e| format!("Failed to set close_to_tray: {}", e))?;
        Ok(())
    }
}

/// Global state wrapper for thread-safe access
pub struct TraySettingsState {
    pub store: Arc<Mutex<Option<TraySettingsStore>>>,
}

impl TraySettingsState {
    pub fn new() -> Result<Self, String> {
        let store = TraySettingsStore::new()?;
        Ok(Self {
            store: Arc::new(Mutex::new(Some(store))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }

    pub fn init_at(&self, base_dir: &Path) -> Result<(), String> {
        let new_store = TraySettingsStore::new_at(base_dir)?;
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock tray settings store".to_string())?;
        *guard = Some(new_store);
        Ok(())
    }

    pub fn teardown(&self) -> Result<(), String> {
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock tray settings store".to_string())?;
        *guard = None;
        Ok(())
    }

    pub fn get_settings(&self) -> Result<TraySettings, String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock tray settings store".to_string())?;
        let store = guard.as_ref().ok_or("No active session - please log in")?;
        store.get_settings()
    }

    pub fn set_enable_tray(&self, value: bool) -> Result<(), String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock tray settings store".to_string())?;
        let store = guard.as_ref().ok_or("No active session - please log in")?;
        store.set_enable_tray(value)
    }

    pub fn set_minimize_to_tray(&self, value: bool) -> Result<(), String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock tray settings store".to_string())?;
        let store = guard.as_ref().ok_or("No active session - please log in")?;
        store.set_minimize_to_tray(value)
    }

    pub fn set_close_to_tray(&self, value: bool) -> Result<(), String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock tray settings store".to_string())?;
        let store = guard.as_ref().ok_or("No active session - please log in")?;
        store.set_close_to_tray(value)
    }
}

// Tauri commands

#[tauri::command]
pub fn get_tray_settings(state: tauri::State<TraySettingsState>) -> Result<TraySettings, String> {
    state.get_settings()
}

#[tauri::command]
pub fn set_enable_tray(value: bool, state: tauri::State<TraySettingsState>) -> Result<(), String> {
    info!(
        "[TraySettings] Setting enable_tray to {} (restart required)",
        value
    );
    state.set_enable_tray(value)
}

#[tauri::command]
pub fn set_minimize_to_tray(
    value: bool,
    state: tauri::State<TraySettingsState>,
) -> Result<(), String> {
    info!("[TraySettings] Setting minimize_to_tray to {}", value);
    state.set_minimize_to_tray(value)
}

#[tauri::command]
pub fn set_close_to_tray(
    value: bool,
    state: tauri::State<TraySettingsState>,
) -> Result<(), String> {
    info!("[TraySettings] Setting close_to_tray to {}", value);
    state.set_close_to_tray(value)
}
