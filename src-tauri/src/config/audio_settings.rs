//! Audio settings persistence
//!
//! Stores user preferences for audio output device, exclusive mode, and DAC passthrough.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AudioSettings {
    pub output_device: Option<String>,  // None = system default
    pub exclusive_mode: bool,
    pub dac_passthrough: bool,
    pub preferred_sample_rate: Option<u32>,  // None = auto
}

pub struct AudioSettingsStore {
    conn: Connection,
}

impl AudioSettingsStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz-nix");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("audio_settings.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open audio settings database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audio_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                output_device TEXT,
                exclusive_mode INTEGER NOT NULL DEFAULT 0,
                dac_passthrough INTEGER NOT NULL DEFAULT 0,
                preferred_sample_rate INTEGER
            );
            INSERT OR IGNORE INTO audio_settings (id, exclusive_mode, dac_passthrough)
            VALUES (1, 0, 0);"
        ).map_err(|e| format!("Failed to create audio settings table: {}", e))?;

        Ok(Self { conn })
    }

    pub fn get_settings(&self) -> Result<AudioSettings, String> {
        self.conn
            .query_row(
                "SELECT output_device, exclusive_mode, dac_passthrough, preferred_sample_rate FROM audio_settings WHERE id = 1",
                [],
                |row| {
                    Ok(AudioSettings {
                        output_device: row.get(0)?,
                        exclusive_mode: row.get::<_, i64>(1)? != 0,
                        dac_passthrough: row.get::<_, i64>(2)? != 0,
                        preferred_sample_rate: row.get(3)?,
                    })
                },
            )
            .map_err(|e| format!("Failed to get audio settings: {}", e))
    }

    pub fn set_output_device(&self, device: Option<&str>) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET output_device = ?1 WHERE id = 1",
                params![device],
            )
            .map_err(|e| format!("Failed to set output device: {}", e))?;
        Ok(())
    }

    pub fn set_exclusive_mode(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET exclusive_mode = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set exclusive mode: {}", e))?;
        Ok(())
    }

    pub fn set_dac_passthrough(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET dac_passthrough = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set DAC passthrough: {}", e))?;
        Ok(())
    }

    pub fn set_sample_rate(&self, rate: Option<u32>) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET preferred_sample_rate = ?1 WHERE id = 1",
                params![rate.map(|r| r as i64)],
            )
            .map_err(|e| format!("Failed to set sample rate: {}", e))?;
        Ok(())
    }
}

/// Thread-safe wrapper
pub struct AudioSettingsState {
    pub store: Arc<Mutex<AudioSettingsStore>>,
}

impl AudioSettingsState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(AudioSettingsStore::new()?)),
        })
    }
}

// Tauri commands
#[tauri::command]
pub fn get_audio_settings(
    state: tauri::State<'_, AudioSettingsState>,
) -> Result<AudioSettings, String> {
    let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    store.get_settings()
}

#[tauri::command]
pub fn set_audio_output_device(
    state: tauri::State<'_, AudioSettingsState>,
    device: Option<String>,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    store.set_output_device(device.as_deref())
}

#[tauri::command]
pub fn set_audio_exclusive_mode(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    store.set_exclusive_mode(enabled)
}

#[tauri::command]
pub fn set_audio_dac_passthrough(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    store.set_dac_passthrough(enabled)
}

#[tauri::command]
pub fn set_audio_sample_rate(
    state: tauri::State<'_, AudioSettingsState>,
    rate: Option<u32>,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    store.set_sample_rate(rate)
}
