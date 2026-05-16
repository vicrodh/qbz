//! Image cache settings persistence.
//!
//! This module stores portable image cache preferences only. Cache runtime,
//! file management, stats, and image resolution remain host-owned behavior.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
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
    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
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

    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "image_cache_settings.db")
    }

    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "image_cache_settings.db")
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

    fn fresh_store(name: &str) -> (std::path::PathBuf, ImageCacheSettingsStore) {
        let dir = unique_test_dir(name);
        let store = ImageCacheSettingsStore::new_at(&dir).expect("open store in temp dir");
        (dir, store)
    }

    #[test]
    fn image_cache_settings_default_values_are_stable() {
        let settings = ImageCacheSettings::default();

        assert!(settings.enabled);
        assert_eq!(settings.max_size_mb, 200);
    }

    #[test]
    fn image_cache_settings_store_returns_defaults() {
        let (dir, store) = fresh_store("image-cache-default");

        let settings = store.get_settings().expect("get settings");

        assert!(settings.enabled);
        assert_eq!(settings.max_size_mb, 200);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn image_cache_settings_persist_enabled() {
        let dir = unique_test_dir("image-cache-enabled");
        {
            let store = ImageCacheSettingsStore::new_at(&dir).expect("open store");
            store.set_enabled(false).expect("set enabled");
        }

        let reopened = ImageCacheSettingsStore::new_at(&dir).expect("reopen store");
        let settings = reopened.get_settings().expect("get settings");

        assert!(!settings.enabled);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn image_cache_settings_persist_max_size_mb() {
        let dir = unique_test_dir("image-cache-size");
        {
            let store = ImageCacheSettingsStore::new_at(&dir).expect("open store");
            store.set_max_size_mb(512).expect("set max size");
        }

        let reopened = ImageCacheSettingsStore::new_at(&dir).expect("reopen store");
        let settings = reopened.get_settings().expect("get settings");

        assert_eq!(settings.max_size_mb, 512);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn image_cache_settings_reopen_does_not_overwrite_existing_row() {
        let dir = unique_test_dir("image-cache-no-overwrite");
        {
            let store = ImageCacheSettingsStore::new_at(&dir).expect("open store");
            store.set_enabled(false).expect("set enabled");
            store.set_max_size_mb(512).expect("set max size");
        }

        let reopened = ImageCacheSettingsStore::new_at(&dir).expect("reopen store");
        let settings = reopened.get_settings().expect("get settings");

        assert!(!settings.enabled);
        assert_eq!(settings.max_size_mb, 512);
        let _ = std::fs::remove_dir_all(dir);
    }
}
