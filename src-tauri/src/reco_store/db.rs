//! SQLite storage for recommendation events

use rusqlite::{params, Connection};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::reco_store::{RecoEventInput, TopArtistSeed};

#[derive(Debug, Clone)]
pub struct RecoEventRecord {
    pub event_type: String,
    pub item_type: String,
    pub track_id: Option<u64>,
    pub album_id: Option<String>,
    pub artist_id: Option<u64>,
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
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open reco database: {}", e))?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<(), String> {
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
                    created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?)
                "#,
                params![
                    event.event_type.as_str(),
                    event.item_type.as_str(),
                    event.track_id,
                    event.album_id.as_deref(),
                    event.artist_id,
                    event.playlist_id,
                    created_at,
                ],
            )
            .map_err(|e| format!("Failed to insert reco event: {}", e))?;

        Ok(())
    }

    pub fn get_recent_album_ids(&self, limit: u32) -> Result<Vec<String>, String> {
        let mut stmt = self.conn
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
        let mut stmt = self.conn
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
                    SELECT event_type, item_type, track_id, album_id, artist_id, created_at
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
                        created_at: row.get(5)?,
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
                    SELECT event_type, item_type, track_id, album_id, artist_id, created_at
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
                        created_at: row.get(5)?,
                    })
                })
                .map_err(|e| format!("Failed to query reco events: {}", e))?;

            for row in rows {
                events.push(row.map_err(|e| format!("Failed to read reco event row: {}", e))?);
            }
        }

        Ok(events)
    }

    pub fn get_top_artist_ids(&self, limit: u32) -> Result<Vec<TopArtistSeed>, String> {
        let mut stmt = self.conn
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
        let mut stmt = self.conn
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
        let mut stmt = self.conn
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

    pub fn get_scored_album_ids(&self, score_type: &str, limit: u32) -> Result<Vec<String>, String> {
        let mut stmt = self.conn
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
        let mut stmt = self.conn
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
        let mut stmt = self.conn
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
}
