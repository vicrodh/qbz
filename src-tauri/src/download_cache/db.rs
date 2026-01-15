//! SQLite database for download cache index

use rusqlite::{Connection, params};
use std::path::Path;

use super::{CachedTrackInfo, DownloadCacheStats, DownloadStatus, TrackDownloadInfo};

/// Database wrapper for cached tracks index
pub struct DownloadCacheDb {
    conn: Connection,
}

impl DownloadCacheDb {
    /// Open or create the database
    pub fn new(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open download cache database: {}", e))?;

        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Get reference to the connection (for direct queries)
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Initialize database schema
    fn init_schema(&self) -> Result<(), String> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS cached_tracks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                track_id INTEGER UNIQUE NOT NULL,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT,
                album_id TEXT,
                duration_secs INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                file_size_bytes INTEGER NOT NULL DEFAULT 0,
                format TEXT NOT NULL DEFAULT 'flac',
                quality TEXT,
                bit_depth INTEGER,
                sample_rate REAL,
                artwork_path TEXT,
                status TEXT NOT NULL DEFAULT 'queued',
                progress_percent INTEGER NOT NULL DEFAULT 0,
                error_message TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_accessed_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_track_id ON cached_tracks(track_id);
            CREATE INDEX IF NOT EXISTS idx_status ON cached_tracks(status);
            CREATE INDEX IF NOT EXISTS idx_last_accessed ON cached_tracks(last_accessed_at);
            "
        ).map_err(|e| format!("Failed to initialize database schema: {}", e))?;

        Ok(())
    }

    /// Insert a new track to download
    pub fn insert_track(&self, info: &TrackDownloadInfo, file_path: &str) -> Result<(), String> {
        self.conn.execute(
            "INSERT OR REPLACE INTO cached_tracks
             (track_id, title, artist, album, album_id, duration_secs, file_path, quality, bit_depth, sample_rate, status, progress_percent, created_at, last_accessed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'queued', 0, datetime('now'), datetime('now'))",
            params![
                info.track_id as i64,
                info.title,
                info.artist,
                info.album,
                info.album_id,
                info.duration_secs as i64,
                file_path,
                info.quality,
                info.bit_depth.map(|v| v as i64),
                info.sample_rate,
            ],
        ).map_err(|e| format!("Failed to insert track: {}", e))?;

        Ok(())
    }

    /// Update track status
    pub fn update_status(&self, track_id: u64, status: DownloadStatus, error: Option<&str>) -> Result<(), String> {
        self.conn.execute(
            "UPDATE cached_tracks SET status = ?1, error_message = ?2 WHERE track_id = ?3",
            params![status.as_str(), error, track_id as i64],
        ).map_err(|e| format!("Failed to update status: {}", e))?;

        Ok(())
    }

    /// Update download progress
    pub fn update_progress(&self, track_id: u64, progress: u8, size_bytes: u64) -> Result<(), String> {
        self.conn.execute(
            "UPDATE cached_tracks SET progress_percent = ?1, file_size_bytes = ?2 WHERE track_id = ?3",
            params![progress as i64, size_bytes as i64, track_id as i64],
        ).map_err(|e| format!("Failed to update progress: {}", e))?;

        Ok(())
    }

    /// Mark download as complete
    pub fn mark_complete(&self, track_id: u64, file_size: u64) -> Result<(), String> {
        self.conn.execute(
            "UPDATE cached_tracks SET status = 'ready', progress_percent = 100, file_size_bytes = ?1, last_accessed_at = datetime('now') WHERE track_id = ?2",
            params![file_size as i64, track_id as i64],
        ).map_err(|e| format!("Failed to mark complete: {}", e))?;

        Ok(())
    }

    /// Update last accessed time (for LRU)
    pub fn touch(&self, track_id: u64) -> Result<(), String> {
        self.conn.execute(
            "UPDATE cached_tracks SET last_accessed_at = datetime('now') WHERE track_id = ?1",
            params![track_id as i64],
        ).map_err(|e| format!("Failed to update access time: {}", e))?;

        Ok(())
    }

    /// Check if a track is cached and ready
    pub fn is_cached(&self, track_id: u64) -> Result<bool, String> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM cached_tracks WHERE track_id = ?1 AND status = 'ready'",
            params![track_id as i64],
            |row| row.get(0),
        ).map_err(|e| format!("Failed to check cache: {}", e))?;

        Ok(count > 0)
    }

    /// Get file path for a cached track
    pub fn get_file_path(&self, track_id: u64) -> Result<Option<String>, String> {
        let result = self.conn.query_row(
            "SELECT file_path FROM cached_tracks WHERE track_id = ?1 AND status = 'ready'",
            params![track_id as i64],
            |row| row.get(0),
        );

        match result {
            Ok(path) => Ok(Some(path)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get file path: {}", e)),
        }
    }

    /// Get track info
    pub fn get_track(&self, track_id: u64) -> Result<Option<CachedTrackInfo>, String> {
        let result = self.conn.query_row(
            "SELECT track_id, title, artist, album, album_id, duration_secs, file_size_bytes, quality, bit_depth, sample_rate, status, progress_percent, error_message, created_at, last_accessed_at
             FROM cached_tracks WHERE track_id = ?1",
            params![track_id as i64],
            |row| {
                Ok(CachedTrackInfo {
                    track_id: row.get::<_, i64>(0)? as u64,
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    album: row.get(3)?,
                    album_id: row.get(4)?,
                    duration_secs: row.get::<_, i64>(5)? as u64,
                    file_size_bytes: row.get::<_, i64>(6)? as u64,
                    quality: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    bit_depth: row.get::<_, Option<i64>>(8)?.map(|v| v as u32),
                    sample_rate: row.get(9)?,
                    status: DownloadStatus::from_str(&row.get::<_, String>(10)?),
                    progress_percent: row.get::<_, i64>(11)? as u8,
                    error_message: row.get(12)?,
                    created_at: row.get(13)?,
                    last_accessed_at: row.get(14)?,
                })
            },
        );

        match result {
            Ok(info) => Ok(Some(info)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get track: {}", e)),
        }
    }

    /// Get all cached tracks
    pub fn get_all_tracks(&self) -> Result<Vec<CachedTrackInfo>, String> {
        let mut stmt = self.conn.prepare(
            "SELECT track_id, title, artist, album, album_id, duration_secs, file_size_bytes, quality, bit_depth, sample_rate, status, progress_percent, error_message, created_at, last_accessed_at
             FROM cached_tracks ORDER BY last_accessed_at DESC"
        ).map_err(|e| format!("Failed to prepare query: {}", e))?;

        let tracks = stmt.query_map([], |row| {
            Ok(CachedTrackInfo {
                track_id: row.get::<_, i64>(0)? as u64,
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                album_id: row.get(4)?,
                duration_secs: row.get::<_, i64>(5)? as u64,
                file_size_bytes: row.get::<_, i64>(6)? as u64,
                quality: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                bit_depth: row.get::<_, Option<i64>>(8)?.map(|v| v as u32),
                sample_rate: row.get(9)?,
                status: DownloadStatus::from_str(&row.get::<_, String>(10)?),
                progress_percent: row.get::<_, i64>(11)? as u8,
                error_message: row.get(12)?,
                created_at: row.get(13)?,
                last_accessed_at: row.get(14)?,
            })
        }).map_err(|e| format!("Failed to query tracks: {}", e))?;

        let mut result = Vec::new();
        for track in tracks {
            result.push(track.map_err(|e| format!("Failed to read track: {}", e))?);
        }

        Ok(result)
    }

    /// Delete a track from cache
    pub fn delete_track(&self, track_id: u64) -> Result<Option<String>, String> {
        // Get file path before deleting
        let file_path: Option<String> = self.conn.query_row(
            "SELECT file_path FROM cached_tracks WHERE track_id = ?1",
            params![track_id as i64],
            |row| row.get(0),
        ).ok();

        self.conn.execute(
            "DELETE FROM cached_tracks WHERE track_id = ?1",
            params![track_id as i64],
        ).map_err(|e| format!("Failed to delete track: {}", e))?;

        Ok(file_path)
    }

    /// Get statistics
    pub fn get_stats(&self, cache_path: &str, limit_bytes: Option<u64>) -> Result<DownloadCacheStats, String> {
        let total_tracks: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM cached_tracks",
            [],
            |row| row.get(0),
        ).map_err(|e| format!("Failed to count tracks: {}", e))?;

        let ready_tracks: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM cached_tracks WHERE status = 'ready'",
            [],
            |row| row.get(0),
        ).map_err(|e| format!("Failed to count ready tracks: {}", e))?;

        let downloading_tracks: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM cached_tracks WHERE status = 'downloading' OR status = 'queued'",
            [],
            |row| row.get(0),
        ).map_err(|e| format!("Failed to count downloading tracks: {}", e))?;

        let failed_tracks: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM cached_tracks WHERE status = 'failed'",
            [],
            |row| row.get(0),
        ).map_err(|e| format!("Failed to count failed tracks: {}", e))?;

        let total_size: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(file_size_bytes), 0) FROM cached_tracks WHERE status = 'ready'",
            [],
            |row| row.get(0),
        ).map_err(|e| format!("Failed to sum sizes: {}", e))?;

        Ok(DownloadCacheStats {
            total_tracks: total_tracks as usize,
            ready_tracks: ready_tracks as usize,
            downloading_tracks: downloading_tracks as usize,
            failed_tracks: failed_tracks as usize,
            total_size_bytes: total_size as u64,
            limit_bytes,
            cache_path: cache_path.to_string(),
        })
    }

    /// Get tracks to evict (LRU order) to free up space
    pub fn get_tracks_for_eviction(&self, bytes_to_free: u64) -> Result<Vec<(u64, String)>, String> {
        let mut stmt = self.conn.prepare(
            "SELECT track_id, file_path, file_size_bytes FROM cached_tracks
             WHERE status = 'ready'
             ORDER BY last_accessed_at ASC"
        ).map_err(|e| format!("Failed to prepare eviction query: {}", e))?;

        let mut result = Vec::new();
        let mut freed = 0u64;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)? as u64,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u64,
            ))
        }).map_err(|e| format!("Failed to query for eviction: {}", e))?;

        for row in rows {
            if freed >= bytes_to_free {
                break;
            }
            let (track_id, file_path, size) = row.map_err(|e| format!("Failed to read row: {}", e))?;
            result.push((track_id, file_path));
            freed += size;
        }

        Ok(result)
    }

    /// Clear all entries
    pub fn clear_all(&self) -> Result<Vec<String>, String> {
        // Get all file paths first
        let mut stmt = self.conn.prepare("SELECT file_path FROM cached_tracks")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let paths: Vec<String> = stmt.query_map([], |row| row.get(0))
            .map_err(|e| format!("Failed to query paths: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        self.conn.execute("DELETE FROM cached_tracks", [])
            .map_err(|e| format!("Failed to clear database: {}", e))?;

        Ok(paths)
    }

    /// Update file path for a track (after organizing)
    pub fn update_file_path(&self, track_id: u64, new_path: &str) -> Result<(), String> {
        self.conn.execute(
            "UPDATE cached_tracks SET file_path = ?1 WHERE track_id = ?2",
            params![new_path, track_id as i64],
        ).map_err(|e| format!("Failed to update file path: {}", e))?;
        Ok(())
    }

    /// Update artwork path for a track
    pub fn update_artwork_path(&self, track_id: u64, artwork_path: &str) -> Result<(), String> {
        self.conn.execute(
            "UPDATE cached_tracks SET artwork_path = ?1 WHERE track_id = ?2",
            params![artwork_path, track_id as i64],
        ).map_err(|e| format!("Failed to update artwork path: {}", e))?;
        Ok(())
    }
}
