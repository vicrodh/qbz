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
    /// Tray icon variant override: "auto" (default; follows system color
    /// scheme), "light" (white glyph for dark panels — useful on GNOME
    /// where the top bar is dark even with a light system theme), or
    /// "dark" (black glyph for light panels).
    #[serde(default = "default_tray_icon_theme")]
    pub tray_icon_theme: String,
}

fn default_tray_icon_theme() -> String {
    "auto".to_string()
}

/// Coerce free-form values to the supported set. Anything outside
/// "auto"/"light"/"dark" falls back to "auto".
pub fn normalize_tray_icon_theme(input: &str) -> String {
    match input {
        "light" | "dark" | "auto" => input.to_string(),
        _ => "auto".to_string(),
    }
}

impl Default for TraySettings {
    fn default() -> Self {
        Self {
            enable_tray: true,
            minimize_to_tray: false,
            close_to_tray: false,
            tray_icon_theme: default_tray_icon_theme(),
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

        // Migration: add tray_icon_theme column if it doesn't exist (older DBs).
        let has_theme_column: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('tray_settings') WHERE name = 'tray_icon_theme'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to check tray_icon_theme column: {}", e))?;
        if has_theme_column == 0 {
            conn.execute_batch(
                "ALTER TABLE tray_settings ADD COLUMN tray_icon_theme TEXT NOT NULL DEFAULT 'auto';",
            )
            .map_err(|e| format!("Failed to add tray_icon_theme column: {}", e))?;
        }

        // Insert default row if it doesn't exist
        conn.execute(
            "INSERT OR IGNORE INTO tray_settings (id, enable_tray, minimize_to_tray, close_to_tray, tray_icon_theme)
            VALUES (1, 1, 0, 0, 'auto')",
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
                "SELECT enable_tray, minimize_to_tray, close_to_tray, tray_icon_theme FROM tray_settings WHERE id = 1",
                [],
                |row| {
                    let enable_tray: i32 = row.get(0)?;
                    let minimize_to_tray: i32 = row.get(1)?;
                    let close_to_tray: i32 = row.get(2)?;
                    let tray_icon_theme: String = row.get(3)?;
                    Ok(TraySettings {
                        enable_tray: enable_tray != 0,
                        minimize_to_tray: minimize_to_tray != 0,
                        close_to_tray: close_to_tray != 0,
                        tray_icon_theme: normalize_tray_icon_theme(&tray_icon_theme),
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

    pub fn set_tray_icon_theme(&self, value: &str) -> Result<(), String> {
        let normalized = normalize_tray_icon_theme(value);
        self.conn
            .execute(
                "UPDATE tray_settings SET tray_icon_theme = ?1 WHERE id = 1",
                params![normalized],
            )
            .map_err(|e| format!("Failed to set tray_icon_theme: {}", e))?;
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

    pub fn set_tray_icon_theme(&self, value: &str) -> Result<(), String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock tray settings store".to_string())?;
        let store = guard.as_ref().ok_or("No active session - please log in")?;
        store.set_tray_icon_theme(value)
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
