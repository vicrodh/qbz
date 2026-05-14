//! Legal settings storage.
//!
//! Persists user acceptance of legal terms.

use log::info;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegalSettings {
    /// User has accepted Qobuz Terms of Service.
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

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS legal_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                qobuz_tos_accepted INTEGER NOT NULL DEFAULT 0
            );",
        )
        .map_err(|e| format!("Failed to create legal settings table: {}", e))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_dir(name: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("qbz-app-{name}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn legal_settings_default_to_unaccepted() {
        let dir = unique_test_dir("legal-default");
        let store = LegalSettingsStore::new_at(&dir).unwrap();

        let settings = store.get_settings().unwrap();

        assert!(!settings.qobuz_tos_accepted);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legal_settings_persist_tos_acceptance() {
        let dir = unique_test_dir("legal-persist");
        {
            let store = LegalSettingsStore::new_at(&dir).unwrap();
            store.set_qobuz_tos_accepted(true).unwrap();
        }

        let reopened = LegalSettingsStore::new_at(&dir).unwrap();
        let settings = reopened.get_settings().unwrap();

        assert!(settings.qobuz_tos_accepted);
        let _ = std::fs::remove_dir_all(dir);
    }
}
