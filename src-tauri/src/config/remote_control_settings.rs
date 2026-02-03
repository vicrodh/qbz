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
    pub secure: bool,
    pub token: String,
}

impl Default for RemoteControlSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8182,
            secure: true,  // HTTPS by default for security
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
                enabled INTEGER NOT NULL DEFAULT 0,
                port INTEGER NOT NULL DEFAULT 8182,
                secure INTEGER NOT NULL DEFAULT 1,
                token TEXT NOT NULL DEFAULT ''
            );"
        ).map_err(|e| format!("Failed to create remote control settings table: {}", e))?;

        ensure_secure_column(&conn)?;

        let token = generate_token();
        conn.execute(
            "INSERT OR IGNORE INTO remote_control_settings (id, enabled, port, secure, token)
            VALUES (1, 0, 8182, 0, ?1)",
            params![token],
        ).map_err(|e| format!("Failed to insert default remote control settings: {}", e))?;

        Ok(Self { conn })
    }

    pub fn get_settings(&self) -> Result<RemoteControlSettings, String> {
        let mut settings = self.conn
            .query_row(
                "SELECT enabled, port, secure, token FROM remote_control_settings WHERE id = 1",
                [],
                |row| {
                    let enabled: i32 = row.get(0)?;
                    let port: i64 = row.get(1)?;
                    let secure: i32 = row.get(2)?;
                    let token: String = row.get(3)?;
                    Ok(RemoteControlSettings {
                        enabled: enabled != 0,
                        port: port as u16,
                        secure: secure != 0,
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

    pub fn set_secure(&self, secure: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE remote_control_settings SET secure = ?1 WHERE id = 1",
                params![if secure { 1 } else { 0 }],
            )
            .map_err(|e| format!("Failed to set remote control secure: {}", e))?;
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

    pub fn set_secure(&self, secure: bool) -> Result<(), String> {
        self.store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .set_secure(secure)
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

fn ensure_secure_column(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(remote_control_settings)")
        .map_err(|e| format!("Failed to read settings schema: {}", e))?;
    let mut rows = stmt
        .query([])
        .map_err(|e| format!("Failed to read settings schema: {}", e))?;

    while let Some(row) = rows
        .next()
        .map_err(|e| format!("Schema read error: {}", e))?
    {
        let name: String = row.get(1).map_err(|e| format!("Schema read error: {}", e))?;
        if name == "secure" {
            return Ok(());
        }
    }

    conn.execute(
        "ALTER TABLE remote_control_settings ADD COLUMN secure INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .map_err(|e| format!("Failed to migrate remote control settings: {}", e))?;

    Ok(())
}

// ============================================================================
// Allowed Origins for CORS
// ============================================================================

/// Default allowed origins for the PWA
const DEFAULT_ALLOWED_ORIGINS: &[&str] = &[
    "vicrodh.github.io",
    "control.qbz.lol",
    "www.control.qbz.lol",
];

/// Allowed origin entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowedOrigin {
    pub id: i64,
    pub origin: String,
    pub is_default: bool,
}

/// Store for allowed CORS origins
pub struct AllowedOriginsStore {
    conn: Connection,
}

impl AllowedOriginsStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("remote_control_settings.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open allowed origins database: {}", e))?;

        // Create allowed_origins table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS allowed_origins (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                origin TEXT NOT NULL UNIQUE,
                is_default INTEGER NOT NULL DEFAULT 0
            );"
        ).map_err(|e| format!("Failed to create allowed_origins table: {}", e))?;

        // Insert default origins if table is empty
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM allowed_origins",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        if count == 0 {
            for origin in DEFAULT_ALLOWED_ORIGINS {
                conn.execute(
                    "INSERT OR IGNORE INTO allowed_origins (origin, is_default) VALUES (?1, 1)",
                    params![origin],
                ).ok();
            }
        }

        Ok(Self { conn })
    }

    /// Get all allowed origins
    pub fn get_origins(&self) -> Result<Vec<AllowedOrigin>, String> {
        let mut stmt = self.conn
            .prepare("SELECT id, origin, is_default FROM allowed_origins ORDER BY is_default DESC, origin ASC")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let origins = stmt.query_map([], |row| {
            Ok(AllowedOrigin {
                id: row.get(0)?,
                origin: row.get(1)?,
                is_default: row.get::<_, i32>(2)? != 0,
            })
        }).map_err(|e| format!("Failed to query origins: {}", e))?;

        origins.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect origins: {}", e))
    }

    /// Check if an origin is allowed
    pub fn is_origin_allowed(&self, origin: &str) -> bool {
        self.conn.query_row(
            "SELECT 1 FROM allowed_origins WHERE origin = ?1",
            params![origin],
            |_| Ok(())
        ).is_ok()
    }

    /// Add a new allowed origin
    pub fn add_origin(&self, origin: &str) -> Result<AllowedOrigin, String> {
        // Normalize origin (lowercase, trim)
        let normalized = origin.trim().to_lowercase();

        if normalized.is_empty() {
            return Err("Origin cannot be empty".to_string());
        }

        self.conn.execute(
            "INSERT INTO allowed_origins (origin, is_default) VALUES (?1, 0)",
            params![normalized],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE constraint") {
                "Origin already exists".to_string()
            } else {
                format!("Failed to add origin: {}", e)
            }
        })?;

        let id = self.conn.last_insert_rowid();
        Ok(AllowedOrigin {
            id,
            origin: normalized,
            is_default: false,
        })
    }

    /// Remove an allowed origin by ID
    pub fn remove_origin(&self, id: i64) -> Result<(), String> {
        let affected = self.conn.execute(
            "DELETE FROM allowed_origins WHERE id = ?1",
            params![id],
        ).map_err(|e| format!("Failed to remove origin: {}", e))?;

        if affected == 0 {
            return Err("Origin not found".to_string());
        }
        Ok(())
    }

    /// Restore default origins (adds missing defaults back)
    pub fn restore_defaults(&self) -> Result<(), String> {
        for origin in DEFAULT_ALLOWED_ORIGINS {
            self.conn.execute(
                "INSERT OR IGNORE INTO allowed_origins (origin, is_default) VALUES (?1, 1)",
                params![origin],
            ).ok();
        }
        Ok(())
    }
}

/// Global state wrapper for thread-safe access to allowed origins
pub struct AllowedOriginsState {
    store: Arc<Mutex<AllowedOriginsStore>>,
}

impl AllowedOriginsState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(AllowedOriginsStore::new()?)),
        })
    }

    pub fn get_origins(&self) -> Result<Vec<AllowedOrigin>, String> {
        self.store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .get_origins()
    }

    pub fn is_origin_allowed(&self, origin: &str) -> bool {
        self.store
            .lock()
            .map(|s| s.is_origin_allowed(origin))
            .unwrap_or(false)
    }

    pub fn add_origin(&self, origin: &str) -> Result<AllowedOrigin, String> {
        self.store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .add_origin(origin)
    }

    pub fn remove_origin(&self, id: i64) -> Result<(), String> {
        self.store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .remove_origin(id)
    }

    pub fn restore_defaults(&self) -> Result<(), String> {
        self.store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .restore_defaults()
    }
}
