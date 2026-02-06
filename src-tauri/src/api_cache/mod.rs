//! API Response Cache
//!
//! SQLite-based cache for API responses (albums, artists, etc.)
//! with TTL-based expiration.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

/// API cache state shared across commands
pub struct ApiCacheState {
    pub cache: Arc<Mutex<Option<ApiCache>>>,
}

impl ApiCacheState {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz")
            .join("cache");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;

        let db_path = data_dir.join("api_cache.db");
        let cache = ApiCache::new(&db_path)?;

        log::info!("API cache initialized at {:?}", db_path);

        Ok(Self {
            cache: Arc::new(Mutex::new(Some(cache))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            cache: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn init_at(&self, base_dir: &Path) -> Result<(), String> {
        let cache_dir = base_dir.join("cache");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
        let db_path = cache_dir.join("api_cache.db");
        let new_cache = ApiCache::new(&db_path)?;
        log::info!("API cache initialized at {:?}", db_path);
        let mut guard = self.cache.lock().await;
        *guard = Some(new_cache);
        Ok(())
    }

    pub async fn teardown(&self) {
        let mut guard = self.cache.lock().await;
        *guard = None;
    }
}

/// Default TTL for cached items (24 hours)
const DEFAULT_TTL_SECS: i64 = 24 * 60 * 60;

/// TTL for genres — Qobuz rarely updates genre lists (7 days)
const GENRE_TTL_SECS: i64 = 7 * 24 * 60 * 60;

pub struct ApiCache {
    conn: Connection,
}

impl ApiCache {
    pub fn new(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open API cache database: {}", e))?;
        let cache = Self { conn };
        cache.init()?;
        Ok(cache)
    }

    fn init(&self) -> Result<(), String> {
        // Run migrations first
        self.migrate_artists_table()?;

        self.conn
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS cached_albums (
                    album_id TEXT PRIMARY KEY,
                    data TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_cached_albums_fetched ON cached_albums(fetched_at);

                CREATE TABLE IF NOT EXISTS cached_artists (
                    artist_id INTEGER NOT NULL,
                    locale TEXT NOT NULL,
                    data TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL,
                    PRIMARY KEY (artist_id, locale)
                );
                CREATE INDEX IF NOT EXISTS idx_cached_artists_fetched ON cached_artists(fetched_at);

                CREATE TABLE IF NOT EXISTS cached_tracks (
                    track_id INTEGER PRIMARY KEY,
                    data TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_cached_tracks_fetched ON cached_tracks(fetched_at);

                CREATE TABLE IF NOT EXISTS cached_genres (
                    cache_key TEXT PRIMARY KEY,
                    data TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_cached_genres_fetched ON cached_genres(fetched_at);
                "#,
            )
            .map_err(|e| format!("Failed to initialize API cache: {}", e))?;
        Ok(())
    }

    /// Migrate old cached_artists table (without locale) to new schema
    fn migrate_artists_table(&self) -> Result<(), String> {
        // Check if cached_artists table exists
        let table_exists: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cached_artists'",
                [],
                |row| row.get::<_, i32>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);

        if !table_exists {
            return Ok(()); // Table doesn't exist, will be created fresh
        }

        // Check if locale column exists
        let has_locale: bool = self
            .conn
            .prepare("PRAGMA table_info(cached_artists)")
            .map_err(|e| format!("Failed to query table info: {}", e))?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("Failed to read table info: {}", e))?
            .filter_map(Result::ok)
            .any(|col| col == "locale");

        if has_locale {
            return Ok(()); // Already migrated
        }

        // Old table without locale column - drop it (cache can be regenerated)
        log::info!("Migrating cached_artists table: dropping old schema without locale column");
        self.conn
            .execute("DROP TABLE cached_artists", [])
            .map_err(|e| format!("Failed to drop old cached_artists table: {}", e))?;

        Ok(())
    }

    fn current_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    // ============ Album Cache ============

    /// Get a cached album if it exists and hasn't expired
    pub fn get_album(&self, album_id: &str, ttl_secs: Option<i64>) -> Result<Option<String>, String> {
        let ttl = ttl_secs.unwrap_or(DEFAULT_TTL_SECS);
        let min_fetched_at = Self::current_timestamp() - ttl;

        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM cached_albums WHERE album_id = ? AND fetched_at > ?",
                params![album_id, min_fetched_at],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to query cached album: {}", e))?;

        Ok(result)
    }

    /// Cache an album response
    pub fn set_album(&self, album_id: &str, data: &str) -> Result<(), String> {
        let fetched_at = Self::current_timestamp();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO cached_albums (album_id, data, fetched_at) VALUES (?, ?, ?)",
                params![album_id, data, fetched_at],
            )
            .map_err(|e| format!("Failed to cache album: {}", e))?;
        Ok(())
    }

    /// Get multiple cached albums at once
    pub fn get_albums(&self, album_ids: &[String], ttl_secs: Option<i64>) -> Result<Vec<(String, String)>, String> {
        if album_ids.is_empty() {
            return Ok(Vec::new());
        }

        let ttl = ttl_secs.unwrap_or(DEFAULT_TTL_SECS);
        let min_fetched_at = Self::current_timestamp() - ttl;

        let placeholders: Vec<&str> = album_ids.iter().map(|_| "?").collect();
        let query = format!(
            "SELECT album_id, data FROM cached_albums WHERE album_id IN ({}) AND fetched_at > ?",
            placeholders.join(",")
        );

        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| format!("Failed to prepare cached albums query: {}", e))?;

        // Build params: album_ids + min_fetched_at
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = album_ids
            .iter()
            .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::ToSql>)
            .collect();
        params_vec.push(Box::new(min_fetched_at));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("Failed to query cached albums: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Failed to read cached album row: {}", e))?);
        }
        Ok(results)
    }

    // ============ Artist Cache ============

    /// Get a cached artist if it exists and hasn't expired
    pub fn get_artist(&self, artist_id: u64, locale: &str, ttl_secs: Option<i64>) -> Result<Option<String>, String> {
        let ttl = ttl_secs.unwrap_or(DEFAULT_TTL_SECS);
        let min_fetched_at = Self::current_timestamp() - ttl;

        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM cached_artists WHERE artist_id = ? AND locale = ? AND fetched_at > ?",
                params![artist_id, locale, min_fetched_at],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to query cached artist: {}", e))?;

        Ok(result)
    }

    /// Cache an artist response
    pub fn set_artist(&self, artist_id: u64, locale: &str, data: &str) -> Result<(), String> {
        let fetched_at = Self::current_timestamp();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO cached_artists (artist_id, locale, data, fetched_at) VALUES (?, ?, ?, ?)",
                params![artist_id, locale, data, fetched_at],
            )
            .map_err(|e| format!("Failed to cache artist: {}", e))?;
        Ok(())
    }

    // ============ Track Cache ============

    /// Get a cached track if it exists and hasn't expired
    pub fn get_track(&self, track_id: u64, ttl_secs: Option<i64>) -> Result<Option<String>, String> {
        let ttl = ttl_secs.unwrap_or(DEFAULT_TTL_SECS);
        let min_fetched_at = Self::current_timestamp() - ttl;

        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM cached_tracks WHERE track_id = ? AND fetched_at > ?",
                params![track_id, min_fetched_at],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to query cached track: {}", e))?;

        Ok(result)
    }

    /// Cache a track response
    pub fn set_track(&self, track_id: u64, data: &str) -> Result<(), String> {
        let fetched_at = Self::current_timestamp();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO cached_tracks (track_id, data, fetched_at) VALUES (?, ?, ?)",
                params![track_id, data, fetched_at],
            )
            .map_err(|e| format!("Failed to cache track: {}", e))?;
        Ok(())
    }

    // ============ Genre Cache ============

    fn genre_cache_key(parent_id: Option<u64>) -> String {
        match parent_id {
            Some(id) => format!("parent_{}", id),
            None => "root".to_string(),
        }
    }

    /// Get cached genres if they exist and haven't expired
    pub fn get_genres(&self, parent_id: Option<u64>) -> Result<Option<String>, String> {
        let key = Self::genre_cache_key(parent_id);
        let min_fetched_at = Self::current_timestamp() - GENRE_TTL_SECS;

        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM cached_genres WHERE cache_key = ? AND fetched_at > ?",
                params![key, min_fetched_at],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to query cached genres: {}", e))?;

        Ok(result)
    }

    /// Cache a genres response
    pub fn set_genres(&self, parent_id: Option<u64>, data: &str) -> Result<(), String> {
        let key = Self::genre_cache_key(parent_id);
        let fetched_at = Self::current_timestamp();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO cached_genres (cache_key, data, fetched_at) VALUES (?, ?, ?)",
                params![key, data, fetched_at],
            )
            .map_err(|e| format!("Failed to cache genres: {}", e))?;
        Ok(())
    }

    // ============ Maintenance ============

    /// Clear all cached artists for a specific locale
    /// This is useful when user changes language and wants fresh data in the new language
    pub fn clear_artists_by_locale(&self, locale: &str) -> Result<usize, String> {
        let deleted = self
            .conn
            .execute(
                "DELETE FROM cached_artists WHERE locale = ?",
                params![locale],
            )
            .map_err(|e| format!("Failed to clear cached artists by locale: {}", e))?;
        
        log::info!("Cleared {} cached artist(s) for locale '{}'", deleted, locale);
        Ok(deleted)
    }

    /// Clear all cached artists (useful for forcing refresh across all languages)
    pub fn clear_all_artists(&self) -> Result<usize, String> {
        let deleted = self
            .conn
            .execute("DELETE FROM cached_artists", [])
            .map_err(|e| format!("Failed to clear all cached artists: {}", e))?;
        
        log::info!("Cleared {} cached artist(s)", deleted);
        Ok(deleted)
    }

    /// Clear expired entries from all tables
    pub fn cleanup_expired(&self, ttl_secs: Option<i64>) -> Result<usize, String> {
        let ttl = ttl_secs.unwrap_or(DEFAULT_TTL_SECS);
        let min_fetched_at = Self::current_timestamp() - ttl;

        let mut total_deleted = 0;

        total_deleted += self
            .conn
            .execute(
                "DELETE FROM cached_albums WHERE fetched_at <= ?",
                params![min_fetched_at],
            )
            .map_err(|e| format!("Failed to cleanup cached albums: {}", e))?;

        total_deleted += self
            .conn
            .execute(
                "DELETE FROM cached_artists WHERE fetched_at <= ?",
                params![min_fetched_at],
            )
            .map_err(|e| format!("Failed to cleanup cached artists: {}", e))?;

        total_deleted += self
            .conn
            .execute(
                "DELETE FROM cached_tracks WHERE fetched_at <= ?",
                params![min_fetched_at],
            )
            .map_err(|e| format!("Failed to cleanup cached tracks: {}", e))?;

        let genre_min_fetched_at = Self::current_timestamp() - GENRE_TTL_SECS;
        total_deleted += self
            .conn
            .execute(
                "DELETE FROM cached_genres WHERE fetched_at <= ?",
                params![genre_min_fetched_at],
            )
            .map_err(|e| format!("Failed to cleanup cached genres: {}", e))?;

        Ok(total_deleted)
    }

    /// Clear all cached data
    pub fn clear_all(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                r#"
                DELETE FROM cached_albums;
                DELETE FROM cached_artists;
                DELETE FROM cached_tracks;
                DELETE FROM cached_genres;
                "#,
            )
            .map_err(|e| format!("Failed to clear API cache: {}", e))?;
        Ok(())
    }
}
