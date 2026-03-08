//! Image cache settings persistence
//!
//! Stores user preferences for the Qobuz image cache:
//! - enabled: whether to cache images locally (default: true)
//! - max_size_mb: maximum cache size in megabytes (default: 200)

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageCacheSettings {
    pub enabled: bool,
    pub max_size_mb: u32,
}

impl Default for ImageCacheSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size_mb: 200,
        }
    }
}

pub struct ImageCacheSettingsStore {
    conn: Connection,
}

impl ImageCacheSettingsStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("image_cache_settings.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open image cache settings database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to enable WAL: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS image_cache_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                enabled INTEGER NOT NULL DEFAULT 1,
                max_size_mb INTEGER NOT NULL DEFAULT 200
            );
            INSERT OR IGNORE INTO image_cache_settings (id, enabled, max_size_mb)
            VALUES (1, 1, 200);",
        )
        .map_err(|e| format!("Failed to create image cache settings table: {}", e))?;

        Ok(Self { conn })
    }

    pub fn get_settings(&self) -> Result<ImageCacheSettings, String> {
        self.conn
            .query_row(
                "SELECT enabled, max_size_mb FROM image_cache_settings WHERE id = 1",
                [],
                |row| {
                    Ok(ImageCacheSettings {
                        enabled: row.get::<_, i32>(0)? != 0,
                        max_size_mb: row.get::<_, u32>(1)?,
                    })
                },
            )
            .map_err(|e| format!("Failed to read image cache settings: {}", e))
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE image_cache_settings SET enabled = ?1 WHERE id = 1",
                params![enabled as i32],
            )
            .map_err(|e| format!("Failed to update image cache enabled: {}", e))?;
        Ok(())
    }

    pub fn set_max_size_mb(&self, max_size_mb: u32) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE image_cache_settings SET max_size_mb = ?1 WHERE id = 1",
                params![max_size_mb],
            )
            .map_err(|e| format!("Failed to update image cache max size: {}", e))?;
        Ok(())
    }
}

pub struct ImageCacheSettingsState {
    pub store: Arc<Mutex<Option<ImageCacheSettingsStore>>>,
}

impl ImageCacheSettingsState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(Some(ImageCacheSettingsStore::new()?))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }
}
