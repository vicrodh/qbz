//! MusicBrainz cache layer
//!
//! SQLite-based cache for MusicBrainz lookups with TTL expiration

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use super::models::*;

/// TTL for recording cache (30 days)
const RECORDING_TTL_SECS: i64 = 30 * 24 * 60 * 60;

/// TTL for artist cache (7 days)
const ARTIST_TTL_SECS: i64 = 7 * 24 * 60 * 60;

/// TTL for release cache (30 days)
const RELEASE_TTL_SECS: i64 = 30 * 24 * 60 * 60;

/// TTL for artist relationships cache (7 days)
const RELATIONS_TTL_SECS: i64 = 7 * 24 * 60 * 60;

/// MusicBrainz cache state shared across commands
pub struct MusicBrainzCacheState {
    pub cache: Arc<Mutex<Option<MusicBrainzCache>>>,
}

impl MusicBrainzCacheState {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz")
            .join("cache");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;

        let db_path = data_dir.join("musicbrainz_cache.db");
        let cache = MusicBrainzCache::new(&db_path)?;

        log::info!("MusicBrainz cache initialized at {:?}", db_path);

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
        let db_path = cache_dir.join("musicbrainz_cache.db");
        let new_cache = MusicBrainzCache::new(&db_path)?;
        log::info!("MusicBrainz cache initialized at {:?}", db_path);
        let mut guard = self.cache.blocking_lock();
        *guard = Some(new_cache);
        Ok(())
    }

    pub fn teardown(&self) {
        let mut guard = self.cache.blocking_lock();
        *guard = None;
    }
}

/// MusicBrainz SQLite cache
pub struct MusicBrainzCache {
    conn: Connection,
}

impl MusicBrainzCache {
    pub fn new(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open MusicBrainz cache: {}", e))?;

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
                -- Recordings indexed by ISRC
                CREATE TABLE IF NOT EXISTS mb_recordings (
                    isrc TEXT PRIMARY KEY,
                    data TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_mb_recordings_fetched ON mb_recordings(fetched_at);

                -- Artists indexed by normalized name
                CREATE TABLE IF NOT EXISTS mb_artists (
                    name_normalized TEXT PRIMARY KEY,
                    data TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_mb_artists_fetched ON mb_artists(fetched_at);

                -- Releases indexed by UPC/barcode
                CREATE TABLE IF NOT EXISTS mb_releases (
                    barcode TEXT PRIMARY KEY,
                    data TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_mb_releases_fetched ON mb_releases(fetched_at);

                -- Artist relationships indexed by MBID
                CREATE TABLE IF NOT EXISTS mb_artist_relations (
                    mbid TEXT PRIMARY KEY,
                    data TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_mb_relations_fetched ON mb_artist_relations(fetched_at);
                "#,
            )
            .map_err(|e| format!("Failed to initialize MusicBrainz cache: {}", e))?;
        Ok(())
    }

    fn current_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// Normalize artist name for consistent cache keys
    pub fn normalize_name(name: &str) -> String {
        name.to_lowercase()
            .trim()
            .replace(['\'', '"', '.', ','], "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    // ============ Recording Cache ============

    /// Get cached recording by ISRC
    pub fn get_recording(&self, isrc: &str) -> Result<Option<ResolvedTrack>, String> {
        let min_fetched_at = Self::current_timestamp() - RECORDING_TTL_SECS;

        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM mb_recordings WHERE isrc = ? AND fetched_at > ?",
                params![isrc, min_fetched_at],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to query recording cache: {}", e))?;

        if let Some(data) = result {
            serde_json::from_str(&data)
                .map(Some)
                .map_err(|e| format!("Failed to parse cached recording: {}", e))
        } else {
            Ok(None)
        }
    }

    /// Cache a resolved recording
    pub fn set_recording(&self, isrc: &str, data: &ResolvedTrack) -> Result<(), String> {
        let fetched_at = Self::current_timestamp();
        let json = serde_json::to_string(data)
            .map_err(|e| format!("Failed to serialize recording: {}", e))?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO mb_recordings (isrc, data, fetched_at) VALUES (?, ?, ?)",
                params![isrc, json, fetched_at],
            )
            .map_err(|e| format!("Failed to cache recording: {}", e))?;
        Ok(())
    }

    // ============ Artist Cache ============

    /// Get cached artist by name
    pub fn get_artist(&self, name: &str) -> Result<Option<ResolvedArtist>, String> {
        let normalized = Self::normalize_name(name);
        let min_fetched_at = Self::current_timestamp() - ARTIST_TTL_SECS;

        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM mb_artists WHERE name_normalized = ? AND fetched_at > ?",
                params![normalized, min_fetched_at],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to query artist cache: {}", e))?;

        if let Some(data) = result {
            serde_json::from_str(&data)
                .map(Some)
                .map_err(|e| format!("Failed to parse cached artist: {}", e))
        } else {
            Ok(None)
        }
    }

    /// Cache a resolved artist
    pub fn set_artist(&self, name: &str, data: &ResolvedArtist) -> Result<(), String> {
        let normalized = Self::normalize_name(name);
        let fetched_at = Self::current_timestamp();
        let json = serde_json::to_string(data)
            .map_err(|e| format!("Failed to serialize artist: {}", e))?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO mb_artists (name_normalized, data, fetched_at) VALUES (?, ?, ?)",
                params![normalized, json, fetched_at],
            )
            .map_err(|e| format!("Failed to cache artist: {}", e))?;
        Ok(())
    }

    // ============ Release Cache ============

    /// Get cached release by barcode
    pub fn get_release(&self, barcode: &str) -> Result<Option<ResolvedRelease>, String> {
        let min_fetched_at = Self::current_timestamp() - RELEASE_TTL_SECS;

        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM mb_releases WHERE barcode = ? AND fetched_at > ?",
                params![barcode, min_fetched_at],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to query release cache: {}", e))?;

        if let Some(data) = result {
            serde_json::from_str(&data)
                .map(Some)
                .map_err(|e| format!("Failed to parse cached release: {}", e))
        } else {
            Ok(None)
        }
    }

    /// Cache a resolved release
    pub fn set_release(&self, barcode: &str, data: &ResolvedRelease) -> Result<(), String> {
        let fetched_at = Self::current_timestamp();
        let json = serde_json::to_string(data)
            .map_err(|e| format!("Failed to serialize release: {}", e))?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO mb_releases (barcode, data, fetched_at) VALUES (?, ?, ?)",
                params![barcode, json, fetched_at],
            )
            .map_err(|e| format!("Failed to cache release: {}", e))?;
        Ok(())
    }

    // ============ Artist Relations Cache ============

    /// Get cached artist relationships by MBID
    pub fn get_artist_relations(&self, mbid: &str) -> Result<Option<ArtistRelationships>, String> {
        let min_fetched_at = Self::current_timestamp() - RELATIONS_TTL_SECS;

        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM mb_artist_relations WHERE mbid = ? AND fetched_at > ?",
                params![mbid, min_fetched_at],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to query relations cache: {}", e))?;

        if let Some(data) = result {
            serde_json::from_str(&data)
                .map(Some)
                .map_err(|e| format!("Failed to parse cached relations: {}", e))
        } else {
            Ok(None)
        }
    }

    /// Cache artist relationships
    pub fn set_artist_relations(
        &self,
        mbid: &str,
        data: &ArtistRelationships,
    ) -> Result<(), String> {
        let fetched_at = Self::current_timestamp();
        let json = serde_json::to_string(data)
            .map_err(|e| format!("Failed to serialize relations: {}", e))?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO mb_artist_relations (mbid, data, fetched_at) VALUES (?, ?, ?)",
                params![mbid, json, fetched_at],
            )
            .map_err(|e| format!("Failed to cache relations: {}", e))?;
        Ok(())
    }

    // ============ Maintenance ============

    /// Clear expired entries from all tables
    pub fn cleanup_expired(&self) -> Result<usize, String> {
        let now = Self::current_timestamp();
        let mut total_deleted = 0;

        total_deleted += self
            .conn
            .execute(
                "DELETE FROM mb_recordings WHERE fetched_at <= ?",
                params![now - RECORDING_TTL_SECS],
            )
            .map_err(|e| format!("Failed to cleanup recordings: {}", e))?;

        total_deleted += self
            .conn
            .execute(
                "DELETE FROM mb_artists WHERE fetched_at <= ?",
                params![now - ARTIST_TTL_SECS],
            )
            .map_err(|e| format!("Failed to cleanup artists: {}", e))?;

        total_deleted += self
            .conn
            .execute(
                "DELETE FROM mb_releases WHERE fetched_at <= ?",
                params![now - RELEASE_TTL_SECS],
            )
            .map_err(|e| format!("Failed to cleanup releases: {}", e))?;

        total_deleted += self
            .conn
            .execute(
                "DELETE FROM mb_artist_relations WHERE fetched_at <= ?",
                params![now - RELATIONS_TTL_SECS],
            )
            .map_err(|e| format!("Failed to cleanup relations: {}", e))?;

        if total_deleted > 0 {
            log::info!(
                "MusicBrainz cache cleanup: removed {} expired entries",
                total_deleted
            );
        }

        Ok(total_deleted)
    }

    /// Clear all cached data
    pub fn clear_all(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                r#"
                DELETE FROM mb_recordings;
                DELETE FROM mb_artists;
                DELETE FROM mb_releases;
                DELETE FROM mb_artist_relations;
                "#,
            )
            .map_err(|e| format!("Failed to clear MusicBrainz cache: {}", e))?;

        log::info!("MusicBrainz cache cleared");
        Ok(())
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> Result<CacheStats, String> {
        let recordings: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM mb_recordings", [], |row| row.get(0))
            .unwrap_or(0);

        let artists: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM mb_artists", [], |row| row.get(0))
            .unwrap_or(0);

        let releases: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM mb_releases", [], |row| row.get(0))
            .unwrap_or(0);

        let relations: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM mb_artist_relations", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);

        Ok(CacheStats {
            recordings: recordings as u64,
            artists: artists as u64,
            releases: releases as u64,
            relations: relations as u64,
        })
    }
}

/// Cache statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheStats {
    pub recordings: u64,
    pub artists: u64,
    pub releases: u64,
    pub relations: u64,
}
