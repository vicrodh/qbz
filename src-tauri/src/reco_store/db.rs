//! SQLite storage for recommendation events

use rusqlite::{params, Connection};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::reco_store::{
    AlbumCardMeta, ArtistCardMeta, RecoEventInput, TopArtistSeed, TrackDisplayMeta,
};

#[derive(Debug, Clone)]
pub struct RecoEventRecord {
    pub event_type: String,
    pub item_type: String,
    pub track_id: Option<u64>,
    pub album_id: Option<String>,
    pub artist_id: Option<u64>,
    pub genre_id: Option<u64>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct RecoScoreEntry {
    pub track_id: Option<u64>,
    pub album_id: Option<String>,
    pub artist_id: Option<u64>,
    pub score: f64,
}

pub struct RecoStoreDb {
    conn: Connection,
}

impl RecoStoreDb {
    pub fn new(path: &Path) -> Result<Self, String> {
        let conn =
            Connection::open(path).map_err(|e| format!("Failed to open reco database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to enable WAL for reco database: {}", e))?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<(), String> {
        // Base schema - does NOT include genre_id (added via migration)
        self.conn
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS reco_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_type TEXT NOT NULL,
                    item_type TEXT NOT NULL,
                    track_id INTEGER,
                    album_id TEXT,
                    artist_id INTEGER,
                    playlist_id INTEGER,
                    created_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_reco_events_type ON reco_events(event_type);
                CREATE INDEX IF NOT EXISTS idx_reco_events_track ON reco_events(track_id);
                CREATE INDEX IF NOT EXISTS idx_reco_events_album ON reco_events(album_id);
                CREATE INDEX IF NOT EXISTS idx_reco_events_artist ON reco_events(artist_id);
                CREATE INDEX IF NOT EXISTS idx_reco_events_created ON reco_events(created_at);

                CREATE TABLE IF NOT EXISTS reco_scores (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    score_type TEXT NOT NULL,
                    item_type TEXT NOT NULL,
                    track_id INTEGER,
                    album_id TEXT,
                    artist_id INTEGER,
                    score REAL NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_reco_scores_type ON reco_scores(score_type);
                CREATE INDEX IF NOT EXISTS idx_reco_scores_item ON reco_scores(item_type);
                CREATE INDEX IF NOT EXISTS idx_reco_scores_track ON reco_scores(track_id);
                CREATE INDEX IF NOT EXISTS idx_reco_scores_album ON reco_scores(album_id);
                CREATE INDEX IF NOT EXISTS idx_reco_scores_artist ON reco_scores(artist_id);
                "#,
            )
            .map_err(|e| format!("Failed to initialize reco database: {}", e))?;

        // Migrations - run after base schema
        self.migrate_add_genre_id()?;
        self.migrate_add_meta_tables()?;

        Ok(())
    }

    /// Migration to add genre_id column to existing databases
    fn migrate_add_genre_id(&self) -> Result<(), String> {
        // Check if column exists by querying table info
        let has_column: bool = self
            .conn
            .prepare("PRAGMA table_info(reco_events)")
            .map_err(|e| format!("Failed to query table info: {}", e))?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("Failed to read table info: {}", e))?
            .filter_map(Result::ok)
            .any(|col| col == "genre_id");

        if !has_column {
            log::info!("Migrating reco_events: adding genre_id column");
            self.conn
                .execute("ALTER TABLE reco_events ADD COLUMN genre_id INTEGER", [])
                .map_err(|e| format!("Failed to add genre_id column: {}", e))?;
            self.conn
                .execute(
                    "CREATE INDEX IF NOT EXISTS idx_reco_events_genre ON reco_events(genre_id)",
                    [],
                )
                .map_err(|e| format!("Failed to create genre_id index: {}", e))?;
        }

        Ok(())
    }

    pub fn insert_event(&self, event: &RecoEventInput) -> Result<(), String> {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn
            .execute(
                r#"
                INSERT INTO reco_events (
                    event_type,
                    item_type,
                    track_id,
                    album_id,
                    artist_id,
                    playlist_id,
                    genre_id,
                    created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                params![
                    event.event_type.as_str(),
                    event.item_type.as_str(),
                    event.track_id,
                    event.album_id.as_deref(),
                    event.artist_id,
                    event.playlist_id,
                    event.genre_id,
                    created_at,
                ],
            )
            .map_err(|e| format!("Failed to insert reco event: {}", e))?;

        Ok(())
    }

    pub fn get_recent_album_ids(&self, limit: u32) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT album_id, MAX(created_at) AS last_played
                FROM reco_events
                WHERE event_type = 'play' AND album_id IS NOT NULL
                GROUP BY album_id
                ORDER BY last_played DESC
                LIMIT ?
                "#,
            )
            .map_err(|e| format!("Failed to prepare recent albums query: {}", e))?;

        let rows = stmt
            .query_map(params![limit], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Failed to query recent albums: {}", e))?;

        let mut albums = Vec::new();
        for row in rows {
            albums.push(row.map_err(|e| format!("Failed to read recent album row: {}", e))?);
        }
        Ok(albums)
    }

    pub fn get_recent_track_ids(&self, limit: u32) -> Result<Vec<u64>, String> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT track_id, MAX(created_at) AS last_played
                FROM reco_events
                WHERE event_type = 'play' AND track_id IS NOT NULL
                GROUP BY track_id
                ORDER BY last_played DESC
                LIMIT ?
                "#,
            )
            .map_err(|e| format!("Failed to prepare recent tracks query: {}", e))?;

        let rows = stmt
            .query_map(params![limit], |row| row.get::<_, u64>(0))
            .map_err(|e| format!("Failed to query recent tracks: {}", e))?;

        let mut tracks = Vec::new();
        for row in rows {
            tracks.push(row.map_err(|e| format!("Failed to read recent track row: {}", e))?);
        }
        Ok(tracks)
    }

    pub fn get_events_since(
        &self,
        since_ts: i64,
        limit: Option<u32>,
    ) -> Result<Vec<RecoEventRecord>, String> {
        let mut events = Vec::new();

        if let Some(limit) = limit {
            let mut stmt = self.conn
                .prepare(
                    r#"
                    SELECT event_type, item_type, track_id, album_id, artist_id, genre_id, created_at
                    FROM reco_events
                    WHERE created_at >= ?
                    ORDER BY created_at DESC
                    LIMIT ?
                    "#,
                )
                .map_err(|e| format!("Failed to prepare reco events query: {}", e))?;

            let rows = stmt
                .query_map(params![since_ts, limit], |row| {
                    Ok(RecoEventRecord {
                        event_type: row.get(0)?,
                        item_type: row.get(1)?,
                        track_id: row.get(2)?,
                        album_id: row.get(3)?,
                        artist_id: row.get(4)?,
                        genre_id: row.get(5)?,
                        created_at: row.get(6)?,
                    })
                })
                .map_err(|e| format!("Failed to query reco events: {}", e))?;

            for row in rows {
                events.push(row.map_err(|e| format!("Failed to read reco event row: {}", e))?);
            }
        } else {
            let mut stmt = self.conn
                .prepare(
                    r#"
                    SELECT event_type, item_type, track_id, album_id, artist_id, genre_id, created_at
                    FROM reco_events
                    WHERE created_at >= ?
                    ORDER BY created_at DESC
                    "#,
                )
                .map_err(|e| format!("Failed to prepare reco events query: {}", e))?;

            let rows = stmt
                .query_map(params![since_ts], |row| {
                    Ok(RecoEventRecord {
                        event_type: row.get(0)?,
                        item_type: row.get(1)?,
                        track_id: row.get(2)?,
                        album_id: row.get(3)?,
                        artist_id: row.get(4)?,
                        genre_id: row.get(5)?,
                        created_at: row.get(6)?,
                    })
                })
                .map_err(|e| format!("Failed to query reco events: {}", e))?;

            for row in rows {
                events.push(row.map_err(|e| format!("Failed to read reco event row: {}", e))?);
            }
        }

        Ok(events)
    }

    /// Get unique album_ids that have NULL genre_id (for backfill)
    pub fn get_album_ids_without_genre(&self, limit: u32) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT DISTINCT album_id
                FROM reco_events
                WHERE album_id IS NOT NULL AND genre_id IS NULL
                LIMIT ?
                "#,
            )
            .map_err(|e| format!("Failed to prepare albums without genre query: {}", e))?;

        let rows = stmt
            .query_map(params![limit], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Failed to query albums without genre: {}", e))?;

        let mut albums = Vec::new();
        for row in rows {
            albums.push(row.map_err(|e| format!("Failed to read album row: {}", e))?);
        }
        Ok(albums)
    }

    /// Update genre_id for all events with a given album_id
    pub fn update_genre_for_album(&self, album_id: &str, genre_id: u64) -> Result<u64, String> {
        let affected = self
            .conn
            .execute(
                "UPDATE reco_events SET genre_id = ? WHERE album_id = ? AND genre_id IS NULL",
                params![genre_id, album_id],
            )
            .map_err(|e| format!("Failed to update genre for album: {}", e))?;
        Ok(affected as u64)
    }

    pub fn get_top_artist_ids(&self, limit: u32) -> Result<Vec<TopArtistSeed>, String> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT artist_id, COUNT(*) AS play_count, MAX(created_at) AS last_played
                FROM reco_events
                WHERE event_type = 'play' AND artist_id IS NOT NULL
                GROUP BY artist_id
                ORDER BY play_count DESC, last_played DESC
                LIMIT ?
                "#,
            )
            .map_err(|e| format!("Failed to prepare top artists query: {}", e))?;

        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(TopArtistSeed {
                    artist_id: row.get::<_, u64>(0)?,
                    play_count: row.get::<_, u32>(1)?,
                })
            })
            .map_err(|e| format!("Failed to query top artists: {}", e))?;

        let mut artists = Vec::new();
        for row in rows {
            artists.push(row.map_err(|e| format!("Failed to read top artist row: {}", e))?);
        }
        Ok(artists)
    }

    pub fn get_favorite_album_ids(&self, limit: u32) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT album_id, MAX(created_at) AS last_favorite
                FROM reco_events
                WHERE event_type = 'favorite' AND album_id IS NOT NULL
                GROUP BY album_id
                ORDER BY last_favorite DESC
                LIMIT ?
                "#,
            )
            .map_err(|e| format!("Failed to prepare favorite albums query: {}", e))?;

        let rows = stmt
            .query_map(params![limit], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Failed to query favorite albums: {}", e))?;

        let mut albums = Vec::new();
        for row in rows {
            albums.push(row.map_err(|e| format!("Failed to read favorite album row: {}", e))?);
        }
        Ok(albums)
    }

    pub fn get_favorite_track_ids(&self, limit: u32) -> Result<Vec<u64>, String> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT track_id, MAX(created_at) AS last_favorite
                FROM reco_events
                WHERE event_type = 'favorite' AND track_id IS NOT NULL
                GROUP BY track_id
                ORDER BY last_favorite DESC
                LIMIT ?
                "#,
            )
            .map_err(|e| format!("Failed to prepare favorite tracks query: {}", e))?;

        let rows = stmt
            .query_map(params![limit], |row| row.get::<_, u64>(0))
            .map_err(|e| format!("Failed to query favorite tracks: {}", e))?;

        let mut tracks = Vec::new();
        for row in rows {
            tracks.push(row.map_err(|e| format!("Failed to read favorite track row: {}", e))?);
        }
        Ok(tracks)
    }

    pub fn replace_scores(
        &mut self,
        score_type: &str,
        item_type: &str,
        entries: &[RecoScoreEntry],
    ) -> Result<(), String> {
        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("Failed to start reco scores transaction: {}", e))?;

        tx.execute(
            "DELETE FROM reco_scores WHERE score_type = ? AND item_type = ?",
            params![score_type, item_type],
        )
        .map_err(|e| format!("Failed to clear reco scores: {}", e))?;

        if !entries.is_empty() {
            let mut stmt = tx
                .prepare(
                    r#"
                    INSERT INTO reco_scores (
                        score_type,
                        item_type,
                        track_id,
                        album_id,
                        artist_id,
                        score,
                        updated_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?)
                    "#,
                )
                .map_err(|e| format!("Failed to prepare reco scores insert: {}", e))?;

            for entry in entries {
                stmt.execute(params![
                    score_type,
                    item_type,
                    entry.track_id,
                    entry.album_id.as_deref(),
                    entry.artist_id,
                    entry.score,
                    updated_at
                ])
                .map_err(|e| format!("Failed to insert reco score: {}", e))?;
            }
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit reco scores: {}", e))?;

        Ok(())
    }

    pub fn has_scores(&self, score_type: &str) -> Result<bool, String> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT COUNT(*) FROM reco_scores WHERE score_type = ?
                "#,
            )
            .map_err(|e| format!("Failed to prepare reco scores count: {}", e))?;

        let count: i64 = stmt
            .query_row(params![score_type], |row| row.get(0))
            .map_err(|e| format!("Failed to query reco scores count: {}", e))?;

        Ok(count > 0)
    }

    pub fn get_scored_album_ids(
        &self,
        score_type: &str,
        limit: u32,
    ) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT album_id
                FROM reco_scores
                WHERE score_type = ? AND item_type = 'album' AND album_id IS NOT NULL
                ORDER BY score DESC
                LIMIT ?
                "#,
            )
            .map_err(|e| format!("Failed to prepare scored albums query: {}", e))?;

        let rows = stmt
            .query_map(params![score_type, limit], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Failed to query scored albums: {}", e))?;

        let mut albums = Vec::new();
        for row in rows {
            albums.push(row.map_err(|e| format!("Failed to read scored album row: {}", e))?);
        }
        Ok(albums)
    }

    pub fn get_scored_track_ids(&self, score_type: &str, limit: u32) -> Result<Vec<u64>, String> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT track_id
                FROM reco_scores
                WHERE score_type = ? AND item_type = 'track' AND track_id IS NOT NULL
                ORDER BY score DESC
                LIMIT ?
                "#,
            )
            .map_err(|e| format!("Failed to prepare scored tracks query: {}", e))?;

        let rows = stmt
            .query_map(params![score_type, limit], |row| row.get::<_, u64>(0))
            .map_err(|e| format!("Failed to query scored tracks: {}", e))?;

        let mut tracks = Vec::new();
        for row in rows {
            tracks.push(row.map_err(|e| format!("Failed to read scored track row: {}", e))?);
        }
        Ok(tracks)
    }

    pub fn get_scored_artist_scores(
        &self,
        score_type: &str,
        limit: u32,
    ) -> Result<Vec<(u64, f64)>, String> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT artist_id, score
                FROM reco_scores
                WHERE score_type = ? AND item_type = 'artist' AND artist_id IS NOT NULL
                ORDER BY score DESC
                LIMIT ?
                "#,
            )
            .map_err(|e| format!("Failed to prepare scored artists query: {}", e))?;

        let rows = stmt
            .query_map(params![score_type, limit], |row| {
                Ok((row.get::<_, u64>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| format!("Failed to query scored artists: {}", e))?;

        let mut artists = Vec::new();
        for row in rows {
            artists.push(row.map_err(|e| format!("Failed to read scored artist row: {}", e))?);
        }
        Ok(artists)
    }

    // ============ Metadata Cache Tables ============

    /// Migration to add persistent metadata cache tables (no TTL)
    fn migrate_add_meta_tables(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS reco_album_meta (
                    album_id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    artist_name TEXT NOT NULL,
                    artist_id INTEGER,
                    artwork_url TEXT NOT NULL DEFAULT '',
                    genre_name TEXT NOT NULL DEFAULT '',
                    quality TEXT NOT NULL DEFAULT '',
                    release_date TEXT,
                    updated_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS reco_track_meta (
                    track_id INTEGER PRIMARY KEY,
                    title TEXT NOT NULL,
                    artist_name TEXT NOT NULL DEFAULT '',
                    artist_id INTEGER,
                    album_title TEXT NOT NULL DEFAULT '',
                    album_id TEXT,
                    album_art_url TEXT NOT NULL DEFAULT '',
                    duration_secs INTEGER NOT NULL DEFAULT 0,
                    hires INTEGER NOT NULL DEFAULT 0,
                    bit_depth INTEGER,
                    sampling_rate REAL,
                    isrc TEXT,
                    updated_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS reco_artist_meta (
                    artist_id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    image_url TEXT NOT NULL DEFAULT '',
                    updated_at INTEGER NOT NULL
                );
                "#,
            )
            .map_err(|e| format!("Failed to create meta tables: {}", e))?;
        Ok(())
    }

    /// Get album metadata for multiple IDs at once
    pub fn get_album_metas(&self, ids: &[String]) -> Result<Vec<AlbumCardMeta>, String> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders: Vec<&str> = ids.iter().map(|_| "?").collect();
        let query = format!(
            "SELECT album_id, title, artist_name, artist_id, artwork_url, genre_name, quality, release_date FROM reco_album_meta WHERE album_id IN ({})",
            placeholders.join(",")
        );

        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| format!("Failed to prepare album meta query: {}", e))?;

        let params_vec: Vec<Box<dyn rusqlite::ToSql>> = ids
            .iter()
            .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::ToSql>)
            .collect();

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(AlbumCardMeta {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    artist_id: row.get(3)?,
                    artwork: row.get(4)?,
                    genre: row.get(5)?,
                    quality: row.get(6)?,
                    release_date: row.get(7)?,
                })
            })
            .map_err(|e| format!("Failed to query album metas: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Failed to read album meta row: {}", e))?);
        }
        Ok(results)
    }

    /// Upsert a single album metadata entry
    pub fn set_album_meta(&self, meta: &AlbumCardMeta) -> Result<(), String> {
        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn
            .execute(
                r#"INSERT OR REPLACE INTO reco_album_meta
                   (album_id, title, artist_name, artist_id, artwork_url, genre_name, quality, release_date, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                params![
                    meta.id,
                    meta.title,
                    meta.artist,
                    meta.artist_id,
                    meta.artwork,
                    meta.genre,
                    meta.quality,
                    meta.release_date,
                    updated_at,
                ],
            )
            .map_err(|e| format!("Failed to upsert album meta: {}", e))?;
        Ok(())
    }

    /// Get track metadata for multiple IDs at once
    pub fn get_track_metas(&self, ids: &[u64]) -> Result<Vec<TrackDisplayMeta>, String> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders: Vec<&str> = ids.iter().map(|_| "?").collect();
        let query = format!(
            "SELECT track_id, title, artist_name, artist_id, album_title, album_id, album_art_url, duration_secs, hires, bit_depth, sampling_rate, isrc FROM reco_track_meta WHERE track_id IN ({})",
            placeholders.join(",")
        );

        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| format!("Failed to prepare track meta query: {}", e))?;

        let params_vec: Vec<Box<dyn rusqlite::ToSql>> = ids
            .iter()
            .map(|id| Box::new(*id as i64) as Box<dyn rusqlite::ToSql>)
            .collect();

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let duration_secs: u32 = row.get(7)?;
                let mins = duration_secs / 60;
                let secs = duration_secs % 60;
                let hires_int: i32 = row.get(8)?;
                Ok(TrackDisplayMeta {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    artist_id: row.get(3)?,
                    album: row.get(4)?,
                    album_id: row.get(5)?,
                    album_art: row.get(6)?,
                    duration: format!("{}:{:02}", mins, secs),
                    duration_seconds: duration_secs,
                    hires: hires_int != 0,
                    bit_depth: row.get(9)?,
                    sampling_rate: row.get(10)?,
                    isrc: row.get(11)?,
                })
            })
            .map_err(|e| format!("Failed to query track metas: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Failed to read track meta row: {}", e))?);
        }
        Ok(results)
    }

    /// Upsert a single track metadata entry
    pub fn set_track_meta(&self, meta: &TrackDisplayMeta) -> Result<(), String> {
        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn
            .execute(
                r#"INSERT OR REPLACE INTO reco_track_meta
                   (track_id, title, artist_name, artist_id, album_title, album_id, album_art_url, duration_secs, hires, bit_depth, sampling_rate, isrc, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                params![
                    meta.id as i64,
                    meta.title,
                    meta.artist,
                    meta.artist_id,
                    meta.album,
                    meta.album_id,
                    meta.album_art,
                    meta.duration_seconds,
                    meta.hires as i32,
                    meta.bit_depth,
                    meta.sampling_rate,
                    meta.isrc,
                    updated_at,
                ],
            )
            .map_err(|e| format!("Failed to upsert track meta: {}", e))?;
        Ok(())
    }

    /// Get artist metadata for multiple IDs at once
    pub fn get_artist_metas(&self, ids: &[u64]) -> Result<Vec<ArtistCardMeta>, String> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders: Vec<&str> = ids.iter().map(|_| "?").collect();
        let query = format!(
            "SELECT artist_id, name, image_url FROM reco_artist_meta WHERE artist_id IN ({})",
            placeholders.join(",")
        );

        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| format!("Failed to prepare artist meta query: {}", e))?;

        let params_vec: Vec<Box<dyn rusqlite::ToSql>> = ids
            .iter()
            .map(|id| Box::new(*id as i64) as Box<dyn rusqlite::ToSql>)
            .collect();

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let image_url: String = row.get(2)?;
                Ok(ArtistCardMeta {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    image: if image_url.is_empty() {
                        None
                    } else {
                        Some(image_url)
                    },
                    play_count: None, // filled by caller from seeds
                })
            })
            .map_err(|e| format!("Failed to query artist metas: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Failed to read artist meta row: {}", e))?);
        }
        Ok(results)
    }

    /// Upsert a single artist metadata entry
    pub fn set_artist_meta(&self, meta: &ArtistCardMeta) -> Result<(), String> {
        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn
            .execute(
                r#"INSERT OR REPLACE INTO reco_artist_meta
                   (artist_id, name, image_url, updated_at)
                   VALUES (?, ?, ?, ?)"#,
                params![
                    meta.id as i64,
                    meta.name,
                    meta.image.as_deref().unwrap_or(""),
                    updated_at,
                ],
            )
            .map_err(|e| format!("Failed to upsert artist meta: {}", e))?;
        Ok(())
    }
}
