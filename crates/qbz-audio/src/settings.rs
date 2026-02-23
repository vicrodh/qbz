//! Audio settings persistence
//!
//! Stores user preferences for audio output device, exclusive mode, and DAC passthrough.
//!
//! NOTE: Tauri command wrappers remain in qbz-nix. This module contains only
//! the core types and persistence logic.

use crate::{AlsaPlugin, AudioBackendType};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub output_device: Option<String>, // None = system default
    pub exclusive_mode: bool,
    pub dac_passthrough: bool,
    pub preferred_sample_rate: Option<u32>,     // None = auto
    pub backend_type: Option<AudioBackendType>, // None = auto-detect
    pub alsa_plugin: Option<AlsaPlugin>,        // Only used when backend is ALSA
    pub alsa_hardware_volume: bool,             // Use ALSA mixer for volume (only with hw: devices)
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
    /// Per-device sample rate limits: device_id -> max_sample_rate
    /// Allows different DACs to have independent max sample rate configurations
    #[serde(default)]
    pub device_sample_rate_limits: HashMap<String, u32>,
    /// When true, apply volume normalization using ReplayGain metadata.
    /// When false (default), the audio pipeline is 100% bit-perfect — no sample modification.
    pub normalization_enabled: bool,
    /// Target loudness in LUFS for normalization.
    /// Common values: -14.0 (Spotify/YouTube), -18.0 (audiophile), -23.0 (EBU broadcast)
    pub normalization_target_lufs: f32,
    /// When true, tracks with the same format are cross-faded seamlessly via Rodio Sink queueing.
    /// Only works with cached tracks on Rodio backend (not ALSA Direct or streaming).
    pub gapless_enabled: bool,
    /// When true, force PipeWire clock.force-quantum alongside clock.force-rate for bit-perfect.
    /// Reset both to 0 on stop. PipeWire-only, requires dac_passthrough.
    pub pw_force_bitperfect: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            output_device: None,
            exclusive_mode: false,
            dac_passthrough: false,
            preferred_sample_rate: None,
            backend_type: None, // Auto-detect (PipeWire if available, else ALSA)
            alsa_plugin: Some(AlsaPlugin::Hw), // Default to hw (bit-perfect)
            alsa_hardware_volume: false, // Disabled by default (maximum compatibility)
            stream_first_track: false, // Disabled by default — user opts in
            stream_buffer_seconds: 3, // 3 seconds initial buffer
            streaming_only: false, // Disabled by default (cache tracks for instant replay)
            limit_quality_to_device: false, // Disabled in 1.1.9 — detection logic unreliable (#45)
            device_max_sample_rate: None, // Set when device is selected
            device_sample_rate_limits: HashMap::new(), // Per-device limits (empty = no limit)
            normalization_enabled: false, // Off by default — preserves bit-perfect pipeline
            normalization_target_lufs: -14.0, // Spotify/YouTube standard
            gapless_enabled: false, // Off by default — user opts in
            pw_force_bitperfect: false, // Off by default — experimental PipeWire feature
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

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to enable WAL for audio settings database: {}", e))?;

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
            VALUES (1, 0, 0);",
        )
        .map_err(|e| format!("Failed to create audio settings table: {}", e))?;

        // Migration: Add new columns if they don't exist (for existing databases)
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN backend_type TEXT",
            [],
        );
        let _ = conn.execute("ALTER TABLE audio_settings ADD COLUMN alsa_plugin TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN alsa_hardware_volume INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN stream_first_track INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN stream_buffer_seconds INTEGER DEFAULT 3",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN streaming_only INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN limit_quality_to_device INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN device_max_sample_rate INTEGER",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN normalization_enabled INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN normalization_target_lufs REAL DEFAULT -14.0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN gapless_enabled INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN device_sample_rate_limits TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE audio_settings ADD COLUMN pw_force_bitperfect INTEGER DEFAULT 0",
            [],
        );

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
                "SELECT output_device, exclusive_mode, dac_passthrough, preferred_sample_rate, backend_type, alsa_plugin, alsa_hardware_volume, stream_first_track, stream_buffer_seconds, streaming_only, limit_quality_to_device, device_max_sample_rate, normalization_enabled, normalization_target_lufs, gapless_enabled, device_sample_rate_limits, pw_force_bitperfect FROM audio_settings WHERE id = 1",
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

                    // Parse device_sample_rate_limits from JSON string
                    let device_sample_rate_limits: HashMap<String, u32> = row
                        .get::<_, Option<String>>(15)?
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default();

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
                        device_sample_rate_limits,
                        normalization_enabled: row.get::<_, Option<i64>>(12)?.unwrap_or(0) != 0,
                        normalization_target_lufs: row.get::<_, Option<f64>>(13)?.unwrap_or(-14.0) as f32,
                        gapless_enabled: row.get::<_, Option<i64>>(14)?.unwrap_or(0) != 0,
                        pw_force_bitperfect: row.get::<_, Option<i64>>(16)?.unwrap_or(0) != 0,
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

    /// Set the sample rate limit for a specific device
    /// If rate is None, removes the limit for that device
    pub fn set_device_sample_rate_limit(
        &self,
        device_id: &str,
        rate: Option<u32>,
    ) -> Result<(), String> {
        // Get current limits
        let mut limits = self.get_device_sample_rate_limits()?;

        // Update or remove the limit for this device
        if let Some(r) = rate {
            limits.insert(device_id.to_string(), r);
        } else {
            limits.remove(device_id);
        }

        // Serialize and save
        let json = serde_json::to_string(&limits)
            .map_err(|e| format!("Failed to serialize device sample rate limits: {}", e))?;

        self.conn
            .execute(
                "UPDATE audio_settings SET device_sample_rate_limits = ?1 WHERE id = 1",
                params![json],
            )
            .map_err(|e| format!("Failed to set device sample rate limits: {}", e))?;
        Ok(())
    }

    /// Get the sample rate limit for a specific device
    /// Returns None if no limit is set for this device
    pub fn get_device_sample_rate_limit(&self, device_id: &str) -> Result<Option<u32>, String> {
        let limits = self.get_device_sample_rate_limits()?;
        Ok(limits.get(device_id).copied())
    }

    /// Get all device sample rate limits
    fn get_device_sample_rate_limits(&self) -> Result<HashMap<String, u32>, String> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT device_sample_rate_limits FROM audio_settings WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to get device sample rate limits: {}", e))?;

        match json {
            Some(s) if !s.is_empty() => {
                serde_json::from_str(&s).map_err(|e| {
                    format!("Failed to parse device sample rate limits: {}", e)
                })
            }
            _ => Ok(HashMap::new()),
        }
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

    pub fn set_pw_force_bitperfect(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE audio_settings SET pw_force_bitperfect = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set pw_force_bitperfect: {}", e))?;
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

    /// Reset all audio settings to their default values
    pub fn reset_all(&self) -> Result<AudioSettings, String> {
        let defaults = AudioSettings::default();
        let backend_json: Option<String> = defaults
            .backend_type
            .map(|b| serde_json::to_string(&b))
            .transpose()
            .map_err(|e| format!("Failed to serialize backend type: {}", e))?;
        let plugin_json: Option<String> = defaults
            .alsa_plugin
            .map(|p| serde_json::to_string(&p))
            .transpose()
            .map_err(|e| format!("Failed to serialize ALSA plugin: {}", e))?;

        // Serialize per-device limits (empty on reset)
        let limits_json = serde_json::to_string(&defaults.device_sample_rate_limits)
            .map_err(|e| format!("Failed to serialize device sample rate limits: {}", e))?;

        self.conn
            .execute(
                "UPDATE audio_settings SET
                    output_device = ?1,
                    exclusive_mode = ?2,
                    dac_passthrough = ?3,
                    preferred_sample_rate = ?4,
                    backend_type = ?5,
                    alsa_plugin = ?6,
                    alsa_hardware_volume = ?7,
                    stream_first_track = ?8,
                    stream_buffer_seconds = ?9,
                    streaming_only = ?10,
                    limit_quality_to_device = ?11,
                    device_max_sample_rate = ?12,
                    normalization_enabled = ?13,
                    normalization_target_lufs = ?14,
                    gapless_enabled = ?15,
                    device_sample_rate_limits = ?16,
                    pw_force_bitperfect = ?17
                WHERE id = 1",
                params![
                    defaults.output_device,
                    defaults.exclusive_mode as i64,
                    defaults.dac_passthrough as i64,
                    defaults.preferred_sample_rate.map(|r| r as i64),
                    backend_json,
                    plugin_json,
                    defaults.alsa_hardware_volume as i64,
                    defaults.stream_first_track as i64,
                    defaults.stream_buffer_seconds as i64,
                    defaults.streaming_only as i64,
                    defaults.limit_quality_to_device as i64,
                    defaults.device_max_sample_rate.map(|r| r as i64),
                    defaults.normalization_enabled as i64,
                    defaults.normalization_target_lufs as f64,
                    defaults.gapless_enabled as i64,
                    limits_json,
                    defaults.pw_force_bitperfect as i64,
                ],
            )
            .map_err(|e| format!("Failed to reset audio settings: {}", e))?;

        Ok(defaults)
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
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock audio settings store".to_string())?;
        *guard = Some(new_store);
        Ok(())
    }

    pub fn teardown(&self) -> Result<(), String> {
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock audio settings store".to_string())?;
        *guard = None;
        Ok(())
    }
}

impl Default for AudioSettingsState {
    fn default() -> Self {
        Self::new_empty()
    }
}
