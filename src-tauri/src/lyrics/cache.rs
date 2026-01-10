//! SQLite cache for lyrics

use rusqlite::{params, Connection};
use std::path::Path;

use super::{LyricsPayload, LyricsProvider};

/// Database wrapper for lyrics cache
pub struct LyricsCacheDb {
    conn: Connection,
}

impl LyricsCacheDb {
    /// Open or create the database
    pub fn new(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open lyrics cache database: {}", e))?;

        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<(), String> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS lyrics_cache (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                track_id INTEGER,
                cache_key TEXT UNIQUE NOT NULL,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT,
                duration_secs INTEGER,
                plain TEXT,
                synced_lrc TEXT,
                provider TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_lyrics_track_id ON lyrics_cache(track_id);
            CREATE INDEX IF NOT EXISTS idx_lyrics_cache_key ON lyrics_cache(cache_key);
            ",
        )
        .map_err(|e| format!("Failed to initialize lyrics cache schema: {}", e))?;

        Ok(())
    }

    pub fn get_by_track_id(&self, track_id: u64) -> Result<Option<LyricsPayload>, String> {
        let result = self.conn.query_row(
            "SELECT track_id, title, artist, album, duration_secs, plain, synced_lrc, provider
             FROM lyrics_cache WHERE track_id = ?1",
            params![track_id as i64],
            |row| {
                Ok(LyricsPayload {
                    track_id: row.get::<_, Option<i64>>(0)?.map(|v| v as u64),
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    album: row.get(3)?,
                    duration_secs: row.get::<_, Option<i64>>(4)?.map(|v| v as u64),
                    plain: row.get(5)?,
                    synced_lrc: row.get(6)?,
                    provider: LyricsProvider::from_str(&row.get::<_, String>(7)?),
                    cached: true,
                })
            },
        );

        match result {
            Ok(payload) => Ok(Some(payload)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to read cached lyrics: {}", e)),
        }
    }

    pub fn get_by_cache_key(&self, cache_key: &str) -> Result<Option<LyricsPayload>, String> {
        let result = self.conn.query_row(
            "SELECT track_id, title, artist, album, duration_secs, plain, synced_lrc, provider
             FROM lyrics_cache WHERE cache_key = ?1",
            params![cache_key],
            |row| {
                Ok(LyricsPayload {
                    track_id: row.get::<_, Option<i64>>(0)?.map(|v| v as u64),
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    album: row.get(3)?,
                    duration_secs: row.get::<_, Option<i64>>(4)?.map(|v| v as u64),
                    plain: row.get(5)?,
                    synced_lrc: row.get(6)?,
                    provider: LyricsProvider::from_str(&row.get::<_, String>(7)?),
                    cached: true,
                })
            },
        );

        match result {
            Ok(payload) => Ok(Some(payload)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to read cached lyrics: {}", e)),
        }
    }

    pub fn upsert(&self, cache_key: &str, payload: &LyricsPayload) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO lyrics_cache
                 (track_id, cache_key, title, artist, album, duration_secs, plain, synced_lrc, provider, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'), datetime('now'))",
                params![
                    payload.track_id.map(|v| v as i64),
                    cache_key,
                    payload.title,
                    payload.artist,
                    payload.album,
                    payload.duration_secs.map(|v| v as i64),
                    payload.plain,
                    payload.synced_lrc,
                    payload.provider.as_str(),
                ],
            )
            .map_err(|e| format!("Failed to write lyrics cache: {}", e))?;

        Ok(())
    }

    pub fn clear(&self) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM lyrics_cache", [])
            .map_err(|e| format!("Failed to clear lyrics cache: {}", e))?;
        Ok(())
    }
}
