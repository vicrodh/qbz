//! Window decoration settings
//!
//! Stores user preferences for window title bar behavior:
//! - use_system_titlebar: Use OS native window decorations instead of custom CSD title bar

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use log::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSettings {
    /// Use OS native window decorations instead of custom title bar
    pub use_system_titlebar: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            use_system_titlebar: false,
        }
    }
}

pub struct WindowSettingsStore {
    conn: Connection,
}

impl WindowSettingsStore {
    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open window settings database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS window_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                use_system_titlebar INTEGER NOT NULL DEFAULT 0
            );"
        ).map_err(|e| format!("Failed to create window settings table: {}", e))?;

        conn.execute(
            "INSERT OR IGNORE INTO window_settings (id, use_system_titlebar)
            VALUES (1, 0)",
            []
        ).map_err(|e| format!("Failed to insert default window settings: {}", e))?;

        info!("[WindowSettings] Database initialized");

        Ok(Self { conn })
    }

    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "window_settings.db")
    }

    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "window_settings.db")
    }

    pub fn get_settings(&self) -> Result<WindowSettings, String> {
        self.conn
            .query_row(
                "SELECT use_system_titlebar FROM window_settings WHERE id = 1",
                [],
                |row| {
                    let use_system_titlebar: i32 = row.get(0)?;
                    Ok(WindowSettings {
                        use_system_titlebar: use_system_titlebar != 0,
                    })
                },
            )
            .map_err(|e| format!("Failed to get window settings: {}", e))
    }

    pub fn set_use_system_titlebar(&self, value: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE window_settings SET use_system_titlebar = ?1 WHERE id = 1",
                params![if value { 1 } else { 0 }],
            )
            .map_err(|e| format!("Failed to set use_system_titlebar: {}", e))?;
        Ok(())
    }
}

/// Global state wrapper for thread-safe access
pub struct WindowSettingsState {
    pub store: Arc<Mutex<Option<WindowSettingsStore>>>,
}

impl WindowSettingsState {
    pub fn new() -> Result<Self, String> {
        let store = WindowSettingsStore::new()?;
        Ok(Self {
            store: Arc::new(Mutex::new(Some(store))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }

    pub fn get_settings(&self) -> Result<WindowSettings, String> {
        let guard = self.store.lock().map_err(|_| "Failed to lock window settings store".to_string())?;
        let store = guard.as_ref().ok_or("Window settings store not initialized")?;
        store.get_settings()
    }

    pub fn set_use_system_titlebar(&self, value: bool) -> Result<(), String> {
        let guard = self.store.lock().map_err(|_| "Failed to lock window settings store".to_string())?;
        let store = guard.as_ref().ok_or("Window settings store not initialized")?;
        store.set_use_system_titlebar(value)
    }
}

// Tauri commands

#[tauri::command]
pub fn get_window_settings(
    state: tauri::State<WindowSettingsState>,
) -> Result<WindowSettings, String> {
    state.get_settings()
}

#[tauri::command]
pub fn set_use_system_titlebar(
    value: bool,
    state: tauri::State<WindowSettingsState>,
) -> Result<(), String> {
    info!("[WindowSettings] Setting use_system_titlebar to {}", value);
    state.set_use_system_titlebar(value)
}
