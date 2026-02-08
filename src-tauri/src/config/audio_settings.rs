//! Audio settings persistence
//!
//! Stores user preferences for audio output device, exclusive mode, and DAC passthrough.

use crate::audio::{AlsaPlugin, AudioBackendType};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub output_device: Option<String>,  // None = system default
    pub exclusive_mode: bool,
    pub dac_passthrough: bool,
    pub preferred_sample_rate: Option<u32>,  // None = auto
    pub backend_type: Option<AudioBackendType>,  // None = auto-detect
    pub alsa_plugin: Option<AlsaPlugin>,  // Only used when backend is ALSA
    pub alsa_hardware_volume: bool,  // Use ALSA mixer for volume (only with hw: devices)
    /// When true, uncached tracks start playing via streaming instead of waiting for full download
    pub stream_first_track: bool,
    /// Initial buffer size in seconds before starting streaming playback (1-10, default 3)
    pub stream_buffer_seconds: u8,
    /// When true, skip L1+L2 cache writes (streaming-only mode). Offline cache still works.
    pub streaming_only: bool,
    /// When true, limit streaming quality to device's max supported sample rate.
    /// This ensures bit-perfect playback by avoiding tracks that exceed device capabilities.
    /// Default: true (recommended for bit-perfect setups)
    pub limit_quality_to_device: bool,
    /// Cached max sample rate of the selected device (set when device is selected)
    /// Used when limit_quality_to_device is true
    pub device_max_sample_rate: Option<u32>,
    /// When true, apply volume normalization using ReplayGain metadata.
    /// When false (default), the audio pipeline is 100% bit-perfect — no sample modification.
    pub normalization_enabled: bool,
    /// Target loudness in LUFS for normalization.
    /// Common values: -14.0 (Spotify/YouTube), -18.0 (audiophile), -23.0 (EBU broadcast)
    pub normalization_target_lufs: f32,
    /// When true, tracks with the same format are cross-faded seamlessly via Rodio Sink queueing.
    /// Only works with cached tracks on Rodio backend (not ALSA Direct or streaming).
    pub gapless_enabled: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            output_device: None,
            exclusive_mode: false,
            dac_passthrough: false,
            preferred_sample_rate: None,
            backend_type: None,  // Auto-detect (PipeWire if available, else ALSA)
            alsa_plugin: Some(AlsaPlugin::Hw),  // Default to hw (bit-perfect)
            alsa_hardware_volume: false,  // Disabled by default (maximum compatibility)
            stream_first_track: true,  // Enabled by default for faster playback start
            stream_buffer_seconds: 3,  // 3 seconds initial buffer
            streaming_only: false,  // Disabled by default (cache tracks for instant replay)
            limit_quality_to_device: false,  // Disabled in 1.1.9 — detection logic unreliable (#45)
            device_max_sample_rate: None,   // Set when device is selected
            normalization_enabled: false,   // Off by default — preserves bit-perfect pipeline
            normalization_target_lufs: -14.0, // Spotify/YouTube standard
            gapless_enabled: true, // On by default — seamless track transitions
        }
    }
}

pub struct AudioSettingsStore {
    conn: Connection,
}

impl AudioSettingsStore {
    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open audio settings database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audio_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                output_device TEXT,
                exclusive_mode INTEGER NOT NULL DEFAULT 0,
                dac_passthrough INTEGER NOT NULL DEFAULT 0,
                preferred_sample_rate INTEGER,
                backend_type TEXT,
                alsa_plugin TEXT,
                alsa_hardware_volume INTEGER NOT NULL DEFAULT 0,
                stream_first_track INTEGER NOT NULL DEFAULT 0,
                stream_buffer_seconds INTEGER NOT NULL DEFAULT 3
            );
            INSERT OR IGNORE INTO audio_settings (id, exclusive_mode, dac_passthrough)
            VALUES (1, 0, 0);"
        ).map_err(|e| format!("Failed to create audio settings table: {}", e))?;

        // Migration: Add new columns if they don't exist (for existing databases)
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN backend_type TEXT", []);
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN alsa_plugin TEXT", []);
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN alsa_hardware_volume INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN stream_first_track INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN stream_buffer_seconds INTEGER DEFAULT 3", []);
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN streaming_only INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN limit_quality_to_device INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN device_max_sample_rate INTEGER", []);
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN normalization_enabled INTEGER DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN normalization_target_lufs REAL DEFAULT -14.0", []);
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN gapless_enabled INTEGER DEFAULT 1", []);

        Ok(Self { conn })
    }

    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "audio_settings.db")
    }

    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "audio_settings.db")
    }

    pub fn get_settings(&self) -> Result<AudioSettings, String> {
        self.conn
            .query_row(
                "SELECT output_device, exclusive_mode, dac_passthrough, preferred_sample_rate, backend_type, alsa_plugin, alsa_hardware_volume, stream_first_track, stream_buffer_seconds, streaming_only, limit_quality_to_device, device_max_sample_rate, normalization_enabled, normalization_target_lufs, gapless_enabled FROM audio_settings WHERE id = 1",
                [],
                |row| {
                    // Parse backend_type from JSON string
                    let backend_type: Option<AudioBackendType> = row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| serde_json::from_str(&s).ok());

                    // Parse alsa_plugin from JSON string
                    let alsa_plugin: Option<AlsaPlugin> = row
                        .get::<_, Option<String>>(5)?
                        .and_then(|s| serde_json::from_str(&s).ok());

                    Ok(AudioSettings {
                        output_device: row.get(0)?,
                        exclusive_mode: row.get::<_, i64>(1)? != 0,
                        dac_passthrough: row.get::<_, i64>(2)? != 0,
                        preferred_sample_rate: row.get(3)?,
                        backend_type,
                        alsa_plugin,
                        alsa_hardware_volume: row.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
                        stream_first_track: row.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
                        stream_buffer_seconds: row.get::<_, Option<i64>>(8)?.unwrap_or(3) as u8,
                        streaming_only: row.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0,
                        limit_quality_to_device: row.get::<_, Option<i64>>(10)?.unwrap_or(1) != 0,
                        device_max_sample_rate: row.get::<_, Option<i64>>(11)?.map(|r| r as u32),
                        normalization_enabled: row.get::<_, Option<i64>>(12)?.unwrap_or(0) != 0,
                        normalization_target_lufs: row.get::<_, Option<f64>>(13)?.unwrap_or(-14.0) as f32,
                        gapless_enabled: row.get::<_, Option<i64>>(14)?.unwrap_or(1) != 0,
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

    pub fn set_backend_type(&self, backend: Option<AudioBackendType>) -> Result<(), String> {
        let backend_json = backend
            .map(|b| serde_json::to_string(&b))
            .transpose()
            .map_err(|e| format!("Failed to serialize backend type: {}", e))?;

        self.conn
            .execute(
                "UPDATE audio_settings SET backend_type = ?1 WHERE id = 1",
                params![backend_json],
            )
            .map_err(|e| format!("Failed to set backend type: {}", e))?;
        Ok(())
    }

    pub fn set_alsa_plugin(&self, plugin: Option<AlsaPlugin>) -> Result<(), String> {
        let plugin_json = plugin
            .map(|p| serde_json::to_string(&p))
            .transpose()
            .map_err(|e| format!("Failed to serialize ALSA plugin: {}", e))?;

        self.conn
            .execute(
                "UPDATE audio_settings SET alsa_plugin = ?1 WHERE id = 1",
                params![plugin_json],
            )
            .map_err(|e| format!("Failed to set ALSA plugin: {}", e))?;
        Ok(())
    }

    pub fn set_alsa_hardware_volume(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET alsa_hardware_volume = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set ALSA hardware volume: {}", e))?;
        Ok(())
    }

    pub fn set_stream_first_track(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET stream_first_track = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set stream first track: {}", e))?;
        Ok(())
    }

    pub fn set_stream_buffer_seconds(&self, seconds: u8) -> Result<(), String> {
        // Clamp to valid range 1-10
        let clamped = seconds.clamp(1, 10);
        self.conn
            .execute(
                "UPDATE audio_settings SET stream_buffer_seconds = ?1 WHERE id = 1",
                params![clamped as i64],
            )
            .map_err(|e| format!("Failed to set stream buffer seconds: {}", e))?;
        Ok(())
    }

    pub fn set_streaming_only(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET streaming_only = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set streaming only: {}", e))?;
        Ok(())
    }

    pub fn set_limit_quality_to_device(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET limit_quality_to_device = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set limit quality to device: {}", e))?;
        Ok(())
    }

    pub fn set_device_max_sample_rate(&self, rate: Option<u32>) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET device_max_sample_rate = ?1 WHERE id = 1",
                params![rate.map(|r| r as i64)],
            )
            .map_err(|e| format!("Failed to set device max sample rate: {}", e))?;
        Ok(())
    }

    pub fn set_normalization_enabled(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET normalization_enabled = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set normalization enabled: {}", e))?;
        Ok(())
    }

    pub fn set_gapless_enabled(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET gapless_enabled = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set gapless enabled: {}", e))?;
        Ok(())
    }

    pub fn set_normalization_target_lufs(&self, target: f32) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET normalization_target_lufs = ?1 WHERE id = 1",
                params![target as f64],
            )
            .map_err(|e| format!("Failed to set normalization target LUFS: {}", e))?;
        Ok(())
    }
}

/// Thread-safe wrapper
pub struct AudioSettingsState {
    pub store: Arc<Mutex<Option<AudioSettingsStore>>>,
}

impl AudioSettingsState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(Some(AudioSettingsStore::new()?))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }

    pub fn init_at(&self, base_dir: &Path) -> Result<(), String> {
        let new_store = AudioSettingsStore::new_at(base_dir)?;
        let mut guard = self.store.lock()
            .map_err(|_| "Failed to lock audio settings store".to_string())?;
        *guard = Some(new_store);
        Ok(())
    }

    pub fn teardown(&self) -> Result<(), String> {
        let mut guard = self.store.lock()
            .map_err(|_| "Failed to lock audio settings store".to_string())?;
        *guard = None;
        Ok(())
    }
}

// Tauri commands
#[tauri::command]
pub fn get_audio_settings(
    state: tauri::State<'_, AudioSettingsState>,
) -> Result<AudioSettings, String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_settings()
}

#[tauri::command]
pub fn set_audio_output_device(
    state: tauri::State<'_, AudioSettingsState>,
    device: Option<String>,
) -> Result<(), String> {
    // Normalize hw:X,0 to stable front:CARD=name,DEV=0 format
    // This ensures the saved device ID survives reboots and USB reconnections
    let normalized_device = device.as_ref().map(|d| {
        crate::audio::normalize_device_id_to_stable(d)
    });

    log::info!(
        "Command: set_audio_output_device {:?} -> {:?} (normalized)",
        device,
        normalized_device
    );

    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_output_device(normalized_device.as_deref())
}

#[tauri::command]
pub fn set_audio_exclusive_mode(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_exclusive_mode(enabled)
}

#[tauri::command]
pub fn set_audio_dac_passthrough(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_dac_passthrough(enabled)
}

#[tauri::command]
pub fn set_audio_sample_rate(
    state: tauri::State<'_, AudioSettingsState>,
    rate: Option<u32>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_sample_rate(rate)
}

#[tauri::command]
pub fn set_audio_backend_type(
    state: tauri::State<'_, AudioSettingsState>,
    backend_type: Option<AudioBackendType>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_backend_type(backend_type)
}

#[tauri::command]
pub fn set_audio_alsa_plugin(
    state: tauri::State<'_, AudioSettingsState>,
    plugin: Option<AlsaPlugin>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_alsa_plugin(plugin)
}

#[tauri::command]
pub fn set_audio_alsa_hardware_volume(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_alsa_hardware_volume(enabled)
}

#[tauri::command]
pub fn set_audio_stream_first_track(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_stream_first_track(enabled)
}

#[tauri::command]
pub fn set_audio_stream_buffer_seconds(
    state: tauri::State<'_, AudioSettingsState>,
    seconds: u8,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_stream_buffer_seconds(seconds)
}

#[tauri::command]
pub fn set_audio_streaming_only(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_streaming_only(enabled)
}

#[tauri::command]
pub fn set_audio_limit_quality_to_device(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_limit_quality_to_device(enabled)
}

#[tauri::command]
pub fn set_audio_device_max_sample_rate(
    state: tauri::State<'_, AudioSettingsState>,
    rate: Option<u32>,
) -> Result<(), String> {
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_device_max_sample_rate(rate)
}

#[tauri::command]
pub fn set_audio_normalization_enabled(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    log::info!("Command: set_audio_normalization_enabled {}", enabled);
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_normalization_enabled(enabled)
}

#[tauri::command]
pub fn set_audio_normalization_target(
    state: tauri::State<'_, AudioSettingsState>,
    target_lufs: f32,
) -> Result<(), String> {
    log::info!("Command: set_audio_normalization_target {} LUFS", target_lufs);
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_normalization_target_lufs(target_lufs)
}

#[tauri::command]
pub fn set_audio_gapless_enabled(
    state: tauri::State<'_, AudioSettingsState>,
    enabled: bool,
) -> Result<(), String> {
    log::info!("Command: set_audio_gapless_enabled {}", enabled);
    let guard = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_gapless_enabled(enabled)
}
