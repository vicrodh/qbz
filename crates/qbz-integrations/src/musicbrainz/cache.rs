//! MusicBrainz cache for resolved entities
//!
//! SQLite-based cache with TTL expiration for MusicBrainz lookups.
//! Caches:
//! - Resolved tracks (by ISRC)
//! - Resolved artists (by name)
//! - Resolved releases (by title/artist)

use rusqlite::{Connection, Result as SqlResult};
use std::path::Path;

use super::models::{MatchConfidence, ResolvedArtist, ResolvedTrack, ArtistType};

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub tracks: u64,
    pub artists: u64,
    pub hits: u64,
    pub misses: u64,
}

/// MusicBrainz cache
pub struct MusicBrainzCache {
    conn: Connection,
}

impl MusicBrainzCache {
    /// Create a new cache at the given path
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("Failed to open MusicBrainz cache: {}", e))?;

        // Enable WAL mode for concurrent read/write (ADR-002)
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to enable WAL mode: {}", e))?;

        let cache = Self { conn };
        cache.init_schema()?;

        Ok(cache)
    }

    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS resolved_tracks (
                    isrc TEXT PRIMARY KEY,
                    recording_mbid TEXT NOT NULL,
                    title TEXT NOT NULL,
                    artist_mbids TEXT NOT NULL,
                    release_mbid TEXT,
                    isrcs TEXT NOT NULL,
                    confidence TEXT NOT NULL,
                    cached_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
                );

                CREATE TABLE IF NOT EXISTS resolved_artists (
                    name_lower TEXT PRIMARY KEY,
                    mbid TEXT NOT NULL,
                    name TEXT NOT NULL,
                    sort_name TEXT,
                    artist_type TEXT NOT NULL,
                    country TEXT,
                    disambiguation TEXT,
                    confidence TEXT NOT NULL,
                    cached_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
                );

                CREATE TABLE IF NOT EXISTS cache_stats (
                    key TEXT PRIMARY KEY,
                    value INTEGER NOT NULL DEFAULT 0
                );

                INSERT OR IGNORE INTO cache_stats (key, value) VALUES ('hits', 0);
                INSERT OR IGNORE INTO cache_stats (key, value) VALUES ('misses', 0);
            ",
            )
            .map_err(|e| format!("Failed to init MusicBrainz schema: {}", e))
    }

    /// Get cached track by ISRC
    pub fn get_track(&self, isrc: &str) -> Result<Option<ResolvedTrack>, String> {
        let result: SqlResult<ResolvedTrack> = self.conn.query_row(
            "SELECT recording_mbid, title, artist_mbids, release_mbid, isrcs, confidence
             FROM resolved_tracks WHERE isrc = ?",
            [isrc],
            |row| {
                let artist_mbids_json: String = row.get(2)?;
                let isrcs_json: String = row.get(4)?;
                let confidence_str: String = row.get(5)?;

                Ok(ResolvedTrack {
                    recording_mbid: row.get(0)?,
                    title: row.get(1)?,
                    artist_mbids: serde_json::from_str(&artist_mbids_json).unwrap_or_default(),
                    release_mbid: row.get(3)?,
                    isrcs: serde_json::from_str(&isrcs_json).unwrap_or_default(),
                    confidence: match confidence_str.as_str() {
                        "exact" => MatchConfidence::Exact,
                        "high" => MatchConfidence::High,
                        "medium" => MatchConfidence::Medium,
                        "low" => MatchConfidence::Low,
                        _ => MatchConfidence::None,
                    },
                })
            },
        );

        match result {
            Ok(track) => {
                self.increment_stat("hits");
                Ok(Some(track))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.increment_stat("misses");
                Ok(None)
            }
            Err(e) => Err(format!("Failed to get track: {}", e)),
        }
    }

    /// Cache a resolved track
    pub fn put_track(&self, isrc: &str, track: &ResolvedTrack) -> Result<(), String> {
        let artist_mbids_json = serde_json::to_string(&track.artist_mbids).unwrap_or_default();
        let isrcs_json = serde_json::to_string(&track.isrcs).unwrap_or_default();
        let confidence = match track.confidence {
            MatchConfidence::Exact => "exact",
            MatchConfidence::High => "high",
            MatchConfidence::Medium => "medium",
            MatchConfidence::Low => "low",
            MatchConfidence::None => "none",
        };

        self.conn
            .execute(
                "INSERT OR REPLACE INTO resolved_tracks (isrc, recording_mbid, title, artist_mbids, release_mbid, isrcs, confidence)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    isrc,
                    track.recording_mbid,
                    track.title,
                    artist_mbids_json,
                    track.release_mbid,
                    isrcs_json,
                    confidence,
                ],
            )
            .map_err(|e| format!("Failed to cache track: {}", e))?;

        Ok(())
    }

    /// Get cached artist by name
    pub fn get_artist(&self, name: &str) -> Result<Option<ResolvedArtist>, String> {
        let name_lower = name.to_lowercase();

        let result: SqlResult<ResolvedArtist> = self.conn.query_row(
            "SELECT mbid, name, sort_name, artist_type, country, disambiguation, confidence
             FROM resolved_artists WHERE name_lower = ?",
            [&name_lower],
            |row| {
                let artist_type_str: String = row.get(3)?;
                let confidence_str: String = row.get(6)?;

                Ok(ResolvedArtist {
                    mbid: row.get(0)?,
                    name: row.get(1)?,
                    sort_name: row.get(2)?,
                    artist_type: ArtistType::from(Some(artist_type_str.as_str())),
                    country: row.get(4)?,
                    disambiguation: row.get(5)?,
                    confidence: match confidence_str.as_str() {
                        "exact" => MatchConfidence::Exact,
                        "high" => MatchConfidence::High,
                        "medium" => MatchConfidence::Medium,
                        "low" => MatchConfidence::Low,
                        _ => MatchConfidence::None,
                    },
                })
            },
        );

        match result {
            Ok(artist) => {
                self.increment_stat("hits");
                Ok(Some(artist))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.increment_stat("misses");
                Ok(None)
            }
            Err(e) => Err(format!("Failed to get artist: {}", e)),
        }
    }

    /// Cache a resolved artist
    pub fn put_artist(&self, artist: &ResolvedArtist) -> Result<(), String> {
        let name_lower = artist.name.to_lowercase();
        let artist_type = match artist.artist_type {
            ArtistType::Person => "person",
            ArtistType::Group => "group",
            ArtistType::Orchestra => "orchestra",
            ArtistType::Choir => "choir",
            ArtistType::Character => "character",
            ArtistType::Other => "other",
        };
        let confidence = match artist.confidence {
            MatchConfidence::Exact => "exact",
            MatchConfidence::High => "high",
            MatchConfidence::Medium => "medium",
            MatchConfidence::Low => "low",
            MatchConfidence::None => "none",
        };

        self.conn
            .execute(
                "INSERT OR REPLACE INTO resolved_artists (name_lower, mbid, name, sort_name, artist_type, country, disambiguation, confidence)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    name_lower,
                    artist.mbid,
                    artist.name,
                    artist.sort_name,
                    artist_type,
                    artist.country,
                    artist.disambiguation,
                    confidence,
                ],
            )
            .map_err(|e| format!("Failed to cache artist: {}", e))?;

        Ok(())
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> Result<CacheStats, String> {
        let tracks: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM resolved_tracks", [], |row| row.get(0))
            .unwrap_or(0);

        let artists: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM resolved_artists", [], |row| row.get(0))
            .unwrap_or(0);

        let hits: u64 = self
            .conn
            .query_row(
                "SELECT value FROM cache_stats WHERE key = 'hits'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let misses: u64 = self
            .conn
            .query_row(
                "SELECT value FROM cache_stats WHERE key = 'misses'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(CacheStats {
            tracks,
            artists,
            hits,
            misses,
        })
    }

    fn increment_stat(&self, key: &str) {
        let _ = self.conn.execute(
            "UPDATE cache_stats SET value = value + 1 WHERE key = ?",
            [key],
        );
    }

    /// Clear expired entries (older than ttl_days)
    pub fn cleanup(&self, ttl_days: u32) -> Result<(u64, u64), String> {
        let cutoff = chrono::Utc::now().timestamp() - (ttl_days as i64 * 86400);

        let tracks_deleted = self
            .conn
            .execute(
                "DELETE FROM resolved_tracks WHERE cached_at < ?",
                [cutoff],
            )
            .map_err(|e| format!("Failed to cleanup tracks: {}", e))? as u64;

        let artists_deleted = self
            .conn
            .execute(
                "DELETE FROM resolved_artists WHERE cached_at < ?",
                [cutoff],
            )
            .map_err(|e| format!("Failed to cleanup artists: {}", e))? as u64;

        Ok((tracks_deleted, artists_deleted))
    }
}
