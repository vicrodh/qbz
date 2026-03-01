//! Window decoration settings
//!
//! Stores user preferences for window title bar behavior:
//! - use_system_titlebar: Use OS native window decorations instead of custom CSD title bar

use log::info;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSettings {
    /// Use OS native window decorations instead of custom title bar
    pub use_system_titlebar: bool,
    /// Last saved window width (logical pixels, non-maximized)
    pub window_width: f64,
    /// Last saved window height (logical pixels, non-maximized)
    pub window_height: f64,
    /// Whether the window was maximized when last closed
    pub is_maximized: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            use_system_titlebar: false,
            window_width: 1280.0,
            window_height: 800.0,
            is_maximized: false,
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

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to enable WAL for window settings database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS window_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                use_system_titlebar INTEGER NOT NULL DEFAULT 0
            );",
        )
        .map_err(|e| format!("Failed to create window settings table: {}", e))?;

        conn.execute(
            "INSERT OR IGNORE INTO window_settings (id, use_system_titlebar)
            VALUES (1, 0)",
            [],
        )
        .map_err(|e| format!("Failed to insert default window settings: {}", e))?;

        // Migrations: add window size columns on existing DBs (errors ignored when column exists)
        let _ = conn.execute("ALTER TABLE window_settings ADD COLUMN window_width REAL NOT NULL DEFAULT 1280.0", []);
        let _ = conn.execute("ALTER TABLE window_settings ADD COLUMN window_height REAL NOT NULL DEFAULT 800.0", []);
        let _ = conn.execute("ALTER TABLE window_settings ADD COLUMN is_maximized INTEGER NOT NULL DEFAULT 0", []);

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
                "SELECT use_system_titlebar, window_width, window_height, is_maximized
                 FROM window_settings WHERE id = 1",
                [],
                |row| {
                    let use_system_titlebar: i32 = row.get(0)?;
                    let window_width: f64 = row.get(1)?;
                    let window_height: f64 = row.get(2)?;
                    let is_maximized: i32 = row.get(3)?;
                    let defaults = WindowSettings::default();
                    let (w, h) = if is_valid_window_size(window_width, window_height) {
                        (window_width, window_height)
                    } else {
                        log::warn!(
                            "[WindowSettings] Corrupt size in DB: {}x{}, using defaults",
                            window_width, window_height
                        );
                        (defaults.window_width, defaults.window_height)
                    };
                    Ok(WindowSettings {
                        use_system_titlebar: use_system_titlebar != 0,
                        window_width: w,
                        window_height: h,
                        is_maximized: is_maximized != 0,
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

    /// Save the non-maximized window dimensions (called on resize while not maximized).
    pub fn set_window_size(&self, width: f64, height: f64) -> Result<(), String> {
        if !is_valid_window_size(width, height) {
            log::warn!(
                "[WindowSettings] Ignoring invalid window size: {}x{}",
                width, height
            );
            return Ok(());
        }
        self.conn
            .execute(
                "UPDATE window_settings SET window_width = ?1, window_height = ?2 WHERE id = 1",
                params![width, height],
            )
            .map_err(|e| format!("Failed to save window size: {}", e))?;
        Ok(())
    }

    /// Save the maximized state (called on window close).
    pub fn set_is_maximized(&self, value: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE window_settings SET is_maximized = ?1 WHERE id = 1",
                params![if value { 1 } else { 0 }],
            )
            .map_err(|e| format!("Failed to save maximized state: {}", e))?;
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
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock window settings store".to_string())?;
        let store = guard
            .as_ref()
            .ok_or("Window settings store not initialized")?;
        store.get_settings()
    }

    pub fn set_use_system_titlebar(&self, value: bool) -> Result<(), String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock window settings store".to_string())?;
        let store = guard
            .as_ref()
            .ok_or("Window settings store not initialized")?;
        store.set_use_system_titlebar(value)
    }

    pub fn set_window_size(&self, width: f64, height: f64) -> Result<(), String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock window settings store".to_string())?;
        let store = guard
            .as_ref()
            .ok_or("Window settings store not initialized")?;
        store.set_window_size(width, height)
    }

    pub fn set_is_maximized(&self, value: bool) -> Result<(), String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock window settings store".to_string())?;
        let store = guard
            .as_ref()
            .ok_or("Window settings store not initialized")?;
        store.set_is_maximized(value)
    }
}

/// Check that window dimensions are within a sane range.
/// Prevents GDK/Cairo crashes from corrupt values (e.g. 9084748x62267212).
fn is_valid_window_size(width: f64, height: f64) -> bool {
    const MIN: f64 = 200.0;
    const MAX: f64 = 32767.0;
    width.is_finite() && height.is_finite()
        && width >= MIN && width <= MAX
        && height >= MIN && height <= MAX
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
