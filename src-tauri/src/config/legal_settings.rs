//! Legal settings storage
//!
//! Persists user acceptance of legal terms:
//! - qobuz_tos_accepted: User has accepted Qobuz Terms of Service

use log::info;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegalSettings {
    /// User has accepted Qobuz Terms of Service
    pub qobuz_tos_accepted: bool,
}

impl Default for LegalSettings {
    fn default() -> Self {
        Self {
            qobuz_tos_accepted: false,
        }
    }
}

pub struct LegalSettingsStore {
    conn: Connection,
}

impl LegalSettingsStore {
    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open legal settings database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to enable WAL for legal settings database: {}", e))?;

        // Create table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS legal_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                qobuz_tos_accepted INTEGER NOT NULL DEFAULT 0
            );",
        )
        .map_err(|e| format!("Failed to create legal settings table: {}", e))?;

        // Insert default row if it doesn't exist
        conn.execute(
            "INSERT OR IGNORE INTO legal_settings (id, qobuz_tos_accepted) VALUES (1, 0)",
            [],
        )
        .map_err(|e| format!("Failed to insert default legal settings: {}", e))?;

        info!("[LegalSettings] Database initialized");

        Ok(Self { conn })
    }

    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "legal_settings.db")
    }

    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "legal_settings.db")
    }

    pub fn get_settings(&self) -> Result<LegalSettings, String> {
        self.conn
            .query_row(
                "SELECT qobuz_tos_accepted FROM legal_settings WHERE id = 1",
                [],
                |row| {
                    let qobuz_tos_accepted: i32 = row.get(0)?;
                    Ok(LegalSettings {
                        qobuz_tos_accepted: qobuz_tos_accepted != 0,
                    })
                },
            )
            .map_err(|e| format!("Failed to get legal settings: {}", e))
    }

    pub fn set_qobuz_tos_accepted(&self, accepted: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE legal_settings SET qobuz_tos_accepted = ? WHERE id = 1",
                params![accepted as i32],
            )
            .map_err(|e| format!("Failed to update ToS acceptance: {}", e))?;

        info!("[LegalSettings] Qobuz ToS accepted: {}", accepted);
        Ok(())
    }
}

pub type LegalSettingsState = Arc<Mutex<Option<LegalSettingsStore>>>;

pub fn create_legal_settings_state() -> Result<LegalSettingsState, String> {
    let store = LegalSettingsStore::new()?;
    Ok(Arc::new(Mutex::new(Some(store))))
}

pub fn create_empty_legal_settings_state() -> LegalSettingsState {
    Arc::new(Mutex::new(None))
}

// Tauri commands

#[tauri::command]
pub fn get_legal_settings(
    state: tauri::State<'_, LegalSettingsState>,
) -> Result<LegalSettings, String> {
    let guard = state.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_settings()
}

#[tauri::command]
pub fn get_qobuz_tos_accepted(state: tauri::State<'_, LegalSettingsState>) -> Result<bool, String> {
    let guard = state.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    let settings = store.get_settings()?;
    Ok(settings.qobuz_tos_accepted)
}

#[tauri::command]
pub fn set_qobuz_tos_accepted(
    state: tauri::State<'_, LegalSettingsState>,
    accepted: bool,
) -> Result<(), String> {
    let guard = state.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_qobuz_tos_accepted(accepted)
}
