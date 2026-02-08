//! Loudness cache — persists EBU R128 measurements in SQLite.
//!
//! Follows the `AudioSettingsStore` pattern: database lives in
//! `dirs::data_dir()/qbz/loudness_cache.db`.
//!
//! Thread-safe via `Mutex<Connection>`.

use rusqlite::{Connection, params};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct CachedLoudness {
    pub gain_db: f32,
    pub peak: f32,
    /// Source of the measurement: "ebur128" or "replaygain"
    pub source: String,
}

pub struct LoudnessCache {
    conn: Mutex<Connection>,
}

impl LoudnessCache {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or_else(|| "Could not determine data directory".to_string())?
            .join("qbz");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("loudness_cache.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open loudness cache database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS track_loudness (
                track_id INTEGER PRIMARY KEY,
                gain_db REAL NOT NULL,
                peak REAL NOT NULL DEFAULT 0.0,
                source TEXT NOT NULL DEFAULT 'ebur128',
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )"
        ).map_err(|e| format!("Failed to create loudness table: {}", e))?;

        log::info!("[LoudnessCache] Opened at {}", db_path.display());

        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Look up cached loudness for a track.
    pub fn get(&self, track_id: u64) -> Option<CachedLoudness> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(
            "SELECT gain_db, peak, source FROM track_loudness WHERE track_id = ?1",
            params![track_id as i64],
            |row| {
                Ok(CachedLoudness {
                    gain_db: row.get::<_, f64>(0)? as f32,
                    peak: row.get::<_, f64>(1)? as f32,
                    source: row.get(2)?,
                })
            },
        ).ok()
    }

    /// Store or update loudness data for a track.
    pub fn set(&self, track_id: u64, gain_db: f32, peak: f32, source: &str) {
        if let Ok(conn) = self.conn.lock() {
            let result = conn.execute(
                "INSERT OR REPLACE INTO track_loudness (track_id, gain_db, peak, source, created_at)
                 VALUES (?1, ?2, ?3, ?4, strftime('%s', 'now'))",
                params![track_id as i64, gain_db as f64, peak as f64, source],
            );
            if let Err(e) = result {
                log::warn!("[LoudnessCache] Failed to store loudness for track {}: {}", track_id, e);
            }
        }
    }
}
