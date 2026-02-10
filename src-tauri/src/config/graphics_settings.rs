//! Graphics settings persistence
//!
//! Stores GPU/rendering preferences that take effect before WebView initialization.
//! These settings are device-level (not per-user) and persist across sessions.
//!
//! - hardware_acceleration: opt-in GPU rendering (default: off)
//!   Env var QBZ_HARDWARE_ACCEL=1|0 always overrides the stored value.
//! - force_x11: force X11/XWayland backend on Wayland sessions (default: off)
//!   Env var QBZ_FORCE_X11=1|0 always overrides the stored value.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphicsSettings {
    /// Enable hardware-accelerated rendering (requires restart)
    pub hardware_acceleration: bool,
    /// Force X11 (XWayland) backend on Wayland sessions (requires restart)
    pub force_x11: bool,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            hardware_acceleration: false,
            force_x11: false,
        }
    }
}

pub struct GraphicsSettingsStore {
    conn: Connection,
}

impl GraphicsSettingsStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("graphics_settings.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open graphics settings database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS graphics_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                hardware_acceleration INTEGER NOT NULL DEFAULT 0,
                force_x11 INTEGER NOT NULL DEFAULT 0
            );
            INSERT OR IGNORE INTO graphics_settings (id, hardware_acceleration, force_x11) VALUES (1, 0, 0);"
        ).map_err(|e| format!("Failed to create graphics settings table: {}", e))?;

        // Migration: add force_x11 column for existing databases
        let _ = conn.execute_batch(
            "ALTER TABLE graphics_settings ADD COLUMN force_x11 INTEGER NOT NULL DEFAULT 0;"
        );

        Ok(Self { conn })
    }

    pub fn get_settings(&self) -> Result<GraphicsSettings, String> {
        self.conn
            .query_row(
                "SELECT hardware_acceleration, force_x11 FROM graphics_settings WHERE id = 1",
                [],
                |row| {
                    Ok(GraphicsSettings {
                        hardware_acceleration: row.get::<_, i64>(0)? != 0,
                        force_x11: row.get::<_, i64>(1)? != 0,
                    })
                },
            )
            .map_err(|e| format!("Failed to get graphics settings: {}", e))
    }

    pub fn set_hardware_acceleration(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE graphics_settings SET hardware_acceleration = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set hardware_acceleration: {}", e))?;
        Ok(())
    }

    pub fn set_force_x11(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE graphics_settings SET force_x11 = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set force_x11: {}", e))?;
        Ok(())
    }
}

/// Thread-safe wrapper for Tauri state management
pub struct GraphicsSettingsState {
    pub store: Arc<Mutex<Option<GraphicsSettingsStore>>>,
}

impl GraphicsSettingsState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(Some(GraphicsSettingsStore::new()?))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }
}

// Tauri commands

#[tauri::command]
pub fn get_graphics_settings(
    state: tauri::State<'_, GraphicsSettingsState>,
) -> Result<GraphicsSettings, String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("Graphics settings store not initialized")?;
    store.get_settings()
}

#[tauri::command]
pub fn set_hardware_acceleration(
    state: tauri::State<'_, GraphicsSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    log::info!("[GraphicsSettings] Setting hardware_acceleration to {} (restart required)", enabled);
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("Graphics settings store not initialized")?;
    store.set_hardware_acceleration(enabled)
}

#[tauri::command]
pub fn set_force_x11(
    state: tauri::State<'_, GraphicsSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    log::info!("[GraphicsSettings] Setting force_x11 to {} (restart required)", enabled);
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("Graphics settings store not initialized")?;
    store.set_force_x11(enabled)
}
