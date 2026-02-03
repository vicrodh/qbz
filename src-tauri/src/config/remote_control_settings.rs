//! Remote control API settings
//!
//! Stores user preferences for the local REST/WebSocket server:
//! - enabled: turn the API server on/off
//! - port: TCP port for the API server
//! - token: pairing token used by the remote control PWA

use base64::Engine;
use rand::RngCore;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteControlSettings {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
}

impl Default for RemoteControlSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8182,
            token: String::new(),
        }
    }
}

pub struct RemoteControlSettingsStore {
    conn: Connection,
}

impl RemoteControlSettingsStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("remote_control_settings.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open remote control settings database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS remote_control_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                enabled INTEGER NOT NULL DEFAULT 1,
                port INTEGER NOT NULL DEFAULT 8182,
                token TEXT NOT NULL DEFAULT ''
            );"
        ).map_err(|e| format!("Failed to create remote control settings table: {}", e))?;

        let token = generate_token();
        conn.execute(
            "INSERT OR IGNORE INTO remote_control_settings (id, enabled, port, token)
            VALUES (1, 0, 8182, ?1)",
            params![token],
        ).map_err(|e| format!("Failed to insert default remote control settings: {}", e))?;

        Ok(Self { conn })
    }

    pub fn get_settings(&self) -> Result<RemoteControlSettings, String> {
        let mut settings = self.conn
            .query_row(
                "SELECT enabled, port, token FROM remote_control_settings WHERE id = 1",
                [],
                |row| {
                    let enabled: i32 = row.get(0)?;
                    let port: i64 = row.get(1)?;
                    let token: String = row.get(2)?;
                    Ok(RemoteControlSettings {
                        enabled: enabled != 0,
                        port: port as u16,
                        token,
                    })
                },
            )
            .map_err(|e| format!("Failed to get remote control settings: {}", e))?;

        if settings.token.is_empty() {
            settings.token = generate_token();
            self.set_token(&settings.token)?;
        }

        Ok(settings)
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE remote_control_settings SET enabled = ?1 WHERE id = 1",
                params![if enabled { 1 } else { 0 }],
            )
            .map_err(|e| format!("Failed to set remote control enabled: {}", e))?;
        Ok(())
    }

    pub fn set_port(&self, port: u16) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE remote_control_settings SET port = ?1 WHERE id = 1",
                params![port as i64],
            )
            .map_err(|e| format!("Failed to set remote control port: {}", e))?;
        Ok(())
    }

    pub fn set_token(&self, token: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE remote_control_settings SET token = ?1 WHERE id = 1",
                params![token],
            )
            .map_err(|e| format!("Failed to set remote control token: {}", e))?;
        Ok(())
    }

    pub fn regenerate_token(&self) -> Result<String, String> {
        let token = generate_token();
        self.set_token(&token)?;
        Ok(token)
    }
}

/// Global state wrapper for thread-safe access
pub struct RemoteControlSettingsState {
    store: Arc<Mutex<RemoteControlSettingsStore>>,
}

impl RemoteControlSettingsState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(RemoteControlSettingsStore::new()?)),
        })
    }

    pub fn get_settings(&self) -> Result<RemoteControlSettings, String> {
        self.store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .get_settings()
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        self.store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .set_enabled(enabled)
    }

    pub fn set_port(&self, port: u16) -> Result<(), String> {
        self.store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .set_port(port)
    }

    pub fn regenerate_token(&self) -> Result<String, String> {
        self.store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .regenerate_token()
    }
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
