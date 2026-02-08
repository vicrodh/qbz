//! Developer settings persistence
//!
//! Stores developer-mode toggles (e.g. force DMA-BUF override for NVIDIA+Wayland).
//! These settings are device-level (not per-user) and persist across sessions.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperSettings {
    pub force_dmabuf: bool,
}

impl Default for DeveloperSettings {
    fn default() -> Self {
        Self {
            force_dmabuf: false,
        }
    }
}

pub struct DeveloperSettingsStore {
    conn: Connection,
}

impl DeveloperSettingsStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("developer_settings.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open developer settings database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS developer_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                force_dmabuf INTEGER NOT NULL DEFAULT 0
            );
            INSERT OR IGNORE INTO developer_settings (id, force_dmabuf) VALUES (1, 0);"
        ).map_err(|e| format!("Failed to create developer settings table: {}", e))?;

        Ok(Self { conn })
    }

    pub fn get_settings(&self) -> Result<DeveloperSettings, String> {
        self.conn
            .query_row(
                "SELECT force_dmabuf FROM developer_settings WHERE id = 1",
                [],
                |row| {
                    Ok(DeveloperSettings {
                        force_dmabuf: row.get::<_, i64>(0)? != 0,
                    })
                },
            )
            .map_err(|e| format!("Failed to get developer settings: {}", e))
    }

    pub fn set_force_dmabuf(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE developer_settings SET force_dmabuf = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set force_dmabuf: {}", e))?;
        Ok(())
    }
}

/// Thread-safe wrapper for Tauri state management
pub struct DeveloperSettingsState {
    pub store: Arc<Mutex<Option<DeveloperSettingsStore>>>,
}

impl DeveloperSettingsState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(Some(DeveloperSettingsStore::new()?))),
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
pub fn get_developer_settings(
    state: tauri::State<'_, DeveloperSettingsState>,
) -> Result<DeveloperSettings, String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("Developer settings store not initialized")?;
    store.get_settings()
}

#[tauri::command]
pub fn set_developer_force_dmabuf(
    state: tauri::State<'_, DeveloperSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    log::info!("Command: set_developer_force_dmabuf {}", enabled);
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("Developer settings store not initialized")?;
    store.set_force_dmabuf(enabled)
}
