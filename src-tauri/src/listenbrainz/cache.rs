//! ListenBrainz cache and queue layer
//!
//! SQLite-based queue for offline listens and token persistence

use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use super::models::*;

/// Max age for queued listens (30 days in seconds)
const QUEUE_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;

/// Max queue size
const QUEUE_MAX_SIZE: i64 = 500;

/// ListenBrainz cache state for Tauri
pub struct ListenBrainzCacheState {
    pub cache: Arc<Mutex<Option<ListenBrainzCache>>>,
}

impl ListenBrainzCacheState {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz")
            .join("cache");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;

        let db_path = data_dir.join("listenbrainz_cache.db");
        let cache = ListenBrainzCache::new(&db_path)?;

        log::info!("ListenBrainz cache initialized at {:?}", db_path);

        Ok(Self {
            cache: Arc::new(Mutex::new(Some(cache))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            cache: Arc::new(Mutex::new(None)),
        }
    }

    pub fn init_at(&self, base_dir: &Path) -> Result<(), String> {
        let cache_dir = base_dir.join("cache");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
        let db_path = cache_dir.join("listenbrainz_cache.db");
        let new_cache = ListenBrainzCache::new(&db_path)?;
        log::info!("ListenBrainz cache initialized at {:?}", db_path);
        let mut guard = self.cache.blocking_lock();
        *guard = Some(new_cache);
        Ok(())
    }

    pub fn teardown(&self) {
        let mut guard = self.cache.blocking_lock();
        *guard = None;
    }
}

/// ListenBrainz SQLite cache
pub struct ListenBrainzCache {
    conn: Connection,
}

impl ListenBrainzCache {
    pub fn new(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open ListenBrainz cache: {}", e))?;

        // Enable WAL mode for better concurrency
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to set pragmas: {}", e))?;

        let cache = Self { conn };
        cache.init()?;
        Ok(cache)
    }

    fn init(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                r#"
                -- Token persistence
                CREATE TABLE IF NOT EXISTS lb_credentials (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    token TEXT,
                    user_name TEXT,
                    updated_at INTEGER NOT NULL
                );
                INSERT OR IGNORE INTO lb_credentials (id, updated_at) VALUES (1, 0);

                -- Offline listen queue
                CREATE TABLE IF NOT EXISTS lb_queue (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    listened_at INTEGER NOT NULL,
                    artist_name TEXT NOT NULL,
                    track_name TEXT NOT NULL,
                    release_name TEXT,
                    recording_mbid TEXT,
                    release_mbid TEXT,
                    artist_mbids TEXT,
                    isrc TEXT,
                    duration_ms INTEGER,
                    created_at INTEGER NOT NULL,
                    attempts INTEGER DEFAULT 0,
                    sent INTEGER NOT NULL DEFAULT 0
                );
                CREATE INDEX IF NOT EXISTS idx_lb_queue_sent ON lb_queue(sent);
                CREATE INDEX IF NOT EXISTS idx_lb_queue_created ON lb_queue(created_at);

                -- Settings
                CREATE TABLE IF NOT EXISTS lb_settings (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    enabled INTEGER NOT NULL DEFAULT 1
                );
                INSERT OR IGNORE INTO lb_settings (id, enabled) VALUES (1, 1);
                "#,
            )
            .map_err(|e| format!("Failed to initialize ListenBrainz cache: {}", e))?;
        Ok(())
    }

    fn current_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    // ============ Credentials ============

    /// Get saved token and username
    pub fn get_credentials(&self) -> Result<(Option<String>, Option<String>), String> {
        let result: (Option<String>, Option<String>) = self
            .conn
            .query_row(
                "SELECT token, user_name FROM lb_credentials WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("Failed to get credentials: {}", e))?;
        Ok(result)
    }

    /// Save token and username
    pub fn set_credentials(&self, token: Option<&str>, user_name: Option<&str>) -> Result<(), String> {
        let now = Self::current_timestamp();
        self.conn
            .execute(
                "UPDATE lb_credentials SET token = ?, user_name = ?, updated_at = ? WHERE id = 1",
                params![token, user_name, now],
            )
            .map_err(|e| format!("Failed to save credentials: {}", e))?;
        Ok(())
    }

    /// Clear saved credentials
    pub fn clear_credentials(&self) -> Result<(), String> {
        self.set_credentials(None, None)
    }

    // ============ Settings ============

    /// Check if ListenBrainz is enabled
    pub fn is_enabled(&self) -> Result<bool, String> {
        let enabled: i64 = self
            .conn
            .query_row("SELECT enabled FROM lb_settings WHERE id = 1", [], |row| row.get(0))
            .unwrap_or(1);
        Ok(enabled != 0)
    }

    /// Set enabled state
    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE lb_settings SET enabled = ? WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set enabled state: {}", e))?;
        Ok(())
    }

    // ============ Queue ============

    /// Queue a listen for later submission
    pub fn queue_listen(
        &self,
        listened_at: i64,
        artist: &str,
        track: &str,
        album: Option<&str>,
        recording_mbid: Option<&str>,
        release_mbid: Option<&str>,
        artist_mbids: Option<&[String]>,
        isrc: Option<&str>,
        duration_ms: Option<u64>,
    ) -> Result<i64, String> {
        // First, enforce max queue size by removing oldest entries
        self.enforce_queue_limits()?;

        let now = Self::current_timestamp();
        let artist_mbids_json = artist_mbids.map(|ids| serde_json::to_string(ids).unwrap_or_default());

        self.conn
            .execute(
                "INSERT INTO lb_queue (listened_at, artist_name, track_name, release_name,
                                       recording_mbid, release_mbid, artist_mbids, isrc,
                                       duration_ms, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    listened_at,
                    artist,
                    track,
                    album,
                    recording_mbid,
                    release_mbid,
                    artist_mbids_json,
                    isrc,
                    duration_ms.map(|d| d as i64),
                    now
                ],
            )
            .map_err(|e| format!("Failed to queue listen: {}", e))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Enforce queue size and age limits
    fn enforce_queue_limits(&self) -> Result<(), String> {
        let now = Self::current_timestamp();
        let cutoff = now - QUEUE_MAX_AGE_SECS;

        // Remove old entries
        self.conn
            .execute("DELETE FROM lb_queue WHERE created_at < ?", params![cutoff])
            .map_err(|e| format!("Failed to cleanup old queue entries: {}", e))?;

        // Remove excess entries (keep newest)
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM lb_queue WHERE sent = 0", [], |row| row.get(0))
            .unwrap_or(0);

        if count >= QUEUE_MAX_SIZE {
            let to_remove = count - QUEUE_MAX_SIZE + 1;
            self.conn
                .execute(
                    "DELETE FROM lb_queue WHERE id IN (
                        SELECT id FROM lb_queue WHERE sent = 0
                        ORDER BY listened_at ASC LIMIT ?
                    )",
                    params![to_remove],
                )
                .map_err(|e| format!("Failed to trim queue: {}", e))?;
        }

        Ok(())
    }

    /// Get queued listens (unsent, up to limit)
    pub fn get_queued_listens(&self, limit: u32) -> Result<Vec<QueuedListen>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, listened_at, artist_name, track_name, release_name,
                        recording_mbid, release_mbid, artist_mbids, isrc,
                        duration_ms, created_at, attempts, sent
                 FROM lb_queue WHERE sent = 0
                 ORDER BY listened_at ASC LIMIT ?",
            )
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let listens = stmt
            .query_map(params![limit], |row| {
                let artist_mbids_json: Option<String> = row.get(7)?;
                let artist_mbids: Option<Vec<String>> = artist_mbids_json
                    .and_then(|json| serde_json::from_str(&json).ok());

                Ok(QueuedListen {
                    id: row.get(0)?,
                    listened_at: row.get(1)?,
                    artist_name: row.get(2)?,
                    track_name: row.get(3)?,
                    release_name: row.get(4)?,
                    recording_mbid: row.get(5)?,
                    release_mbid: row.get(6)?,
                    artist_mbids,
                    isrc: row.get(8)?,
                    duration_ms: row.get::<_, Option<i64>>(9)?.map(|d| d as u64),
                    created_at: row.get(10)?,
                    attempts: row.get(11)?,
                    sent: row.get::<_, i64>(12)? != 0,
                })
            })
            .map_err(|e| format!("Failed to query queued listens: {}", e))?;

        listens
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect queued listens: {}", e))
    }

    /// Mark listens as sent
    pub fn mark_listens_sent(&self, ids: &[i64]) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }

        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "UPDATE lb_queue SET sent = 1 WHERE id IN ({})",
            placeholders.join(",")
        );

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        stmt.execute(params.as_slice())
            .map_err(|e| format!("Failed to mark listens as sent: {}", e))?;

        Ok(())
    }

    /// Increment attempt count for a listen
    pub fn increment_attempts(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE lb_queue SET attempts = attempts + 1 WHERE id = ?",
                params![id],
            )
            .map_err(|e| format!("Failed to increment attempts: {}", e))?;
        Ok(())
    }

    /// Get count of queued (unsent) listens
    pub fn get_queue_count(&self) -> Result<u32, String> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM lb_queue WHERE sent = 0",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as u32)
            .map_err(|e| format!("Failed to count queued listens: {}", e))
    }

    /// Clear all queued listens
    pub fn clear_queue(&self) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM lb_queue", [])
            .map_err(|e| format!("Failed to clear queue: {}", e))?;
        Ok(())
    }

    /// Cleanup sent listens older than specified days
    pub fn cleanup_sent_listens(&self, older_than_days: u32) -> Result<u32, String> {
        let cutoff = Self::current_timestamp() - (older_than_days as i64 * 24 * 60 * 60);

        let deleted = self
            .conn
            .execute(
                "DELETE FROM lb_queue WHERE sent = 1 AND created_at < ?",
                params![cutoff],
            )
            .map_err(|e| format!("Failed to cleanup sent listens: {}", e))?;

        Ok(deleted as u32)
    }
}

/// Queue statistics
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStats {
    pub pending_count: u32,
    pub total_count: u32,
}
