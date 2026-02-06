//! Download settings persistence
//!
//! Stores user preferences for download path and library integration.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSettings {
    pub download_root: String,
    pub show_in_library: bool,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        let default_root = dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("qbz")
            .join("audio")
            .to_string_lossy()
            .to_string();

        Self {
            download_root: default_root,
            show_in_library: false,
        }
    }
}

pub struct DownloadSettingsStore {
    conn: Connection,
}

impl DownloadSettingsStore {
    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open download settings database: {}", e))?;

        let default_settings = DownloadSettings::default();

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS download_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                download_root TEXT NOT NULL,
                show_in_library INTEGER NOT NULL DEFAULT 0
            );"
        ).map_err(|e| format!("Failed to create download settings table: {}", e))?;

        conn.execute(
            "INSERT OR IGNORE INTO download_settings (id, download_root, show_in_library)
             VALUES (1, ?1, 0)",
            params![default_settings.download_root],
        ).map_err(|e| format!("Failed to initialize download settings: {}", e))?;

        Ok(Self { conn })
    }

    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "download_settings.db")
    }

    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "download_settings.db")
    }

    pub fn get_settings(&self) -> Result<DownloadSettings, String> {
        self.conn
            .query_row(
                "SELECT download_root, show_in_library FROM download_settings WHERE id = 1",
                [],
                |row| {
                    Ok(DownloadSettings {
                        download_root: row.get(0)?,
                        show_in_library: row.get::<_, i64>(1)? != 0,
                    })
                },
            )
            .map_err(|e| format!("Failed to get download settings: {}", e))
    }

    pub fn set_download_root(&self, path: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE download_settings SET download_root = ?1 WHERE id = 1",
                params![path],
            )
            .map_err(|e| format!("Failed to set download root: {}", e))?;
        Ok(())
    }

    pub fn set_show_in_library(&self, show: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE download_settings SET show_in_library = ?1 WHERE id = 1",
                params![show as i64],
            )
            .map_err(|e| format!("Failed to set show_in_library: {}", e))?;
        Ok(())
    }
}

pub type DownloadSettingsState = Arc<Mutex<Option<DownloadSettingsStore>>>;

pub fn create_download_settings_state() -> Result<DownloadSettingsState, String> {
    let store = DownloadSettingsStore::new()?;
    Ok(Arc::new(Mutex::new(Some(store))))
}

pub fn create_empty_download_settings_state() -> DownloadSettingsState {
    Arc::new(Mutex::new(None))
}

// Tauri commands

#[tauri::command]
pub fn get_download_settings(
    state: tauri::State<DownloadSettingsState>,
) -> Result<DownloadSettings, String> {
    log::info!("Command: get_download_settings");
    let guard = state.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_settings()
}

#[tauri::command]
pub fn set_download_root(
    path: String,
    state: tauri::State<DownloadSettingsState>,
) -> Result<(), String> {
    log::info!("Command: set_download_root to: {}", path);

    // Basic validation
    let path_obj = std::path::Path::new(&path);
    if !path_obj.exists() {
        return Err("Path does not exist".to_string());
    }
    if !path_obj.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    let guard = state.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_download_root(&path)
}

#[tauri::command]
pub fn set_show_downloads_in_library(
    show: bool,
    state: tauri::State<DownloadSettingsState>,
) -> Result<(), String> {
    log::info!("Command: set_show_downloads_in_library to: {}", show);
    let guard = state.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_show_in_library(show)
}

#[tauri::command]
pub fn validate_download_root(path: String) -> Result<bool, String> {
    log::info!("Command: validate_download_root: {}", path);

    let path_obj = std::path::Path::new(&path);

    if !path_obj.exists() {
        return Ok(false);
    }
    if !path_obj.is_dir() {
        return Err("Path exists but is not a directory".to_string());
    }

    // Check write permissions by trying to create a temp file
    let test_file = path_obj.join(".qbz_write_test");
    match std::fs::write(&test_file, b"test") {
        Ok(_) => {
            let _ = std::fs::remove_file(&test_file);
            Ok(true)
        }
        Err(e) => Err(format!("No write permission: {}", e)),
    }
}
