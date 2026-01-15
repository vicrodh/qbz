//! SQLite database layer for library persistence

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

use crate::library::{AudioFormat, LibraryError, LocalAlbum, LocalArtist, LocalTrack};

/// Library database wrapper
pub struct LibraryDatabase {
    conn: Connection,
}

impl LibraryDatabase {
    /// Open or create database at path
    pub fn open(db_path: &Path) -> Result<Self, LibraryError> {
        log::info!("Opening library database at: {}", db_path.display());

        let conn = Connection::open(db_path)
            .map_err(|e| LibraryError::Database(format!("Failed to open database: {}", e)))?;

        // Enable WAL mode for better concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| LibraryError::Database(format!("Failed to set WAL mode: {}", e)))?;

        let db = Self { conn };
        db.init_schema()?;
        db.run_migrations()?;
        Ok(db)
    }

    /// Create tables if they don't exist
    fn init_schema(&self) -> Result<(), LibraryError> {
        self.conn
            .execute_batch(
                r#"
            CREATE TABLE IF NOT EXISTS library_folders (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                enabled INTEGER DEFAULT 1,
                last_scan INTEGER
            );

            CREATE TABLE IF NOT EXISTS local_tracks (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT NOT NULL,
                album_artist TEXT,
                track_number INTEGER,
                disc_number INTEGER,
                year INTEGER,
                genre TEXT,
                duration_secs INTEGER NOT NULL,
                format TEXT NOT NULL,
                bit_depth INTEGER,
                sample_rate INTEGER NOT NULL,
                channels INTEGER NOT NULL,
                file_size_bytes INTEGER NOT NULL,
                cue_file_path TEXT,
                cue_start_secs REAL,
                cue_end_secs REAL,
                artwork_path TEXT,
                last_modified INTEGER NOT NULL,
                indexed_at INTEGER NOT NULL,
                album_group_key TEXT,
                album_group_title TEXT,
                UNIQUE(file_path, cue_start_secs)
            );

            CREATE INDEX IF NOT EXISTS idx_tracks_artist ON local_tracks(artist);
            CREATE INDEX IF NOT EXISTS idx_tracks_album ON local_tracks(album);
            CREATE INDEX IF NOT EXISTS idx_tracks_album_artist ON local_tracks(album_artist);
            CREATE INDEX IF NOT EXISTS idx_tracks_file_path ON local_tracks(file_path);
            CREATE INDEX IF NOT EXISTS idx_tracks_title ON local_tracks(title);

            -- Playlist local settings (enhances remote Qobuz playlists)
            CREATE TABLE IF NOT EXISTS playlist_settings (
                qobuz_playlist_id INTEGER PRIMARY KEY,
                custom_artwork_path TEXT,
                sort_by TEXT DEFAULT 'default',
                sort_order TEXT DEFAULT 'asc',
                last_search_query TEXT,
                notes TEXT,
                hidden INTEGER DEFAULT 0,
                position INTEGER DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            -- Playlist statistics (play counts, etc.)
            CREATE TABLE IF NOT EXISTS playlist_stats (
                qobuz_playlist_id INTEGER PRIMARY KEY,
                play_count INTEGER DEFAULT 0,
                last_played_at INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            -- Local tracks added to playlists (mixed with remote Qobuz tracks)
            CREATE TABLE IF NOT EXISTS playlist_local_tracks (
                id INTEGER PRIMARY KEY,
                qobuz_playlist_id INTEGER NOT NULL,
                local_track_id INTEGER NOT NULL,
                position INTEGER NOT NULL,
                added_at INTEGER NOT NULL,
                FOREIGN KEY (local_track_id) REFERENCES local_tracks(id) ON DELETE CASCADE,
                UNIQUE(qobuz_playlist_id, local_track_id)
            );

            CREATE INDEX IF NOT EXISTS idx_playlist_local_tracks_playlist
                ON playlist_local_tracks(qobuz_playlist_id);

            -- Album settings (per-album customization)
            CREATE TABLE IF NOT EXISTS album_settings (
                album_group_key TEXT PRIMARY KEY,
                hidden INTEGER DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
        "#,
            )
            .map_err(|e| LibraryError::Database(format!("Failed to create schema: {}", e)))?;

        Ok(())
    }

    /// Run schema migrations for existing databases
    fn run_migrations(&self) -> Result<(), LibraryError> {
        // Migration: Add qobuz download tracking fields
        let has_source: bool = self.conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('local_tracks') WHERE name = 'source'",
                [],
                |row| row.get::<_, i32>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);

        if !has_source {
            log::info!("Running migration: adding source and qobuz_track_id to local_tracks");
            self.conn.execute_batch(
                "ALTER TABLE local_tracks ADD COLUMN source TEXT DEFAULT 'user';
                 ALTER TABLE local_tracks ADD COLUMN qobuz_track_id INTEGER;
                 CREATE INDEX IF NOT EXISTS idx_tracks_source ON local_tracks(source);
                 CREATE INDEX IF NOT EXISTS idx_tracks_qobuz_id ON local_tracks(qobuz_track_id);"
            ).map_err(|e| LibraryError::Database(format!("Migration failed: {}", e)))?;
        }

        // Check if playlist_settings has the 'hidden' column (added in v2)
        let has_hidden: bool = self.conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('playlist_settings') WHERE name = 'hidden'",
                [],
                |row| row.get::<_, i32>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);

        if !has_hidden {
            log::info!("Running migration: adding hidden and position columns to playlist_settings");
            self.conn.execute_batch(
                "ALTER TABLE playlist_settings ADD COLUMN hidden INTEGER DEFAULT 0;
                 ALTER TABLE playlist_settings ADD COLUMN position INTEGER DEFAULT 0;"
            ).map_err(|e| LibraryError::Database(format!("Migration failed: {}", e)))?;
        }

        // Check if playlist_stats table exists
        let has_stats_table: bool = self.conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='playlist_stats'",
                [],
                |row| row.get::<_, i32>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);

        if !has_stats_table {
            log::info!("Running migration: creating playlist_stats table");
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS playlist_stats (
                    qobuz_playlist_id INTEGER PRIMARY KEY,
                    play_count INTEGER DEFAULT 0,
                    last_played_at INTEGER,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );"
            ).map_err(|e| LibraryError::Database(format!("Migration failed: {}", e)))?;
        }

        let has_album_group_key: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('local_tracks') WHERE name = 'album_group_key'",
                [],
                |row| row.get::<_, i32>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);

        if !has_album_group_key {
            log::info!("Running migration: adding album_group_key to local_tracks");
            self.conn
                .execute_batch("ALTER TABLE local_tracks ADD COLUMN album_group_key TEXT;")
                .map_err(|e| LibraryError::Database(format!("Migration failed: {}", e)))?;
        }

        let has_album_group_title: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('local_tracks') WHERE name = 'album_group_title'",
                [],
                |row| row.get::<_, i32>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);

        if !has_album_group_title {
            log::info!("Running migration: adding album_group_title to local_tracks");
            self.conn
                .execute_batch("ALTER TABLE local_tracks ADD COLUMN album_group_title TEXT;")
                .map_err(|e| LibraryError::Database(format!("Migration failed: {}", e)))?;
        }

        self.conn
            .execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_tracks_album_group ON local_tracks(album_group_key);",
            )
            .map_err(|e| LibraryError::Database(format!("Migration failed: {}", e)))?;

        let has_file_nocue_index: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_tracks_file_nocue'",
                [],
                |row| row.get::<_, i32>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);

        if !has_file_nocue_index {
            log::info!("Running migration: dedupe non-CUE tracks and add unique index");
            self.conn
                .execute_batch(
                    r#"
                DELETE FROM local_tracks
                WHERE cue_file_path IS NULL
                  AND rowid NOT IN (
                    SELECT MAX(rowid)
                    FROM local_tracks
                    WHERE cue_file_path IS NULL
                    GROUP BY file_path
                  );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_tracks_file_nocue
                  ON local_tracks(file_path)
                  WHERE cue_file_path IS NULL;
            "#,
                )
                .map_err(|e| LibraryError::Database(format!("Migration failed: {}", e)))?;
        }

        Ok(())
    }

    // === Folder Management ===

    /// Add a folder to the library
    pub fn add_folder(&self, path: &str) -> Result<(), LibraryError> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO library_folders (path) VALUES (?)",
                params![path],
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;
        Ok(())
    }

    /// Remove a folder from the library
    pub fn remove_folder(&self, path: &str) -> Result<(), LibraryError> {
        self.conn
            .execute("DELETE FROM library_folders WHERE path = ?", params![path])
            .map_err(|e| LibraryError::Database(e.to_string()))?;
        Ok(())
    }

    /// Get all enabled library folders
    pub fn get_folders(&self) -> Result<Vec<String>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM library_folders WHERE enabled = 1")
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let mut folders = Vec::new();
        for path in rows {
            folders.push(path.map_err(|e| LibraryError::Database(e.to_string()))?);
        }
        Ok(folders)
    }

    /// Update last scan time for a folder
    pub fn update_folder_scan_time(&self, path: &str, timestamp: i64) -> Result<(), LibraryError> {
        self.conn
            .execute(
                "UPDATE library_folders SET last_scan = ? WHERE path = ?",
                params![timestamp, path],
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;
        Ok(())
    }

    // === Track Management ===

    /// Insert or update a track
    pub fn insert_track(&self, track: &LocalTrack) -> Result<i64, LibraryError> {
        self.conn
            .execute(
                r#"INSERT OR REPLACE INTO local_tracks
               (file_path, title, artist, album, album_artist, track_number,
                disc_number, year, genre, duration_secs, format, bit_depth,
                sample_rate, channels, file_size_bytes, cue_file_path,
                cue_start_secs, cue_end_secs, artwork_path, last_modified, indexed_at,
                album_group_key, album_group_title)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                params![
                    track.file_path,
                    track.title,
                    track.artist,
                    track.album,
                    track.album_artist,
                    track.track_number,
                    track.disc_number,
                    track.year,
                    track.genre,
                    track.duration_secs,
                    track.format.to_string(),
                    track.bit_depth,
                    track.sample_rate,
                    track.channels,
                    track.file_size_bytes,
                    track.cue_file_path,
                    track.cue_start_secs,
                    track.cue_end_secs,
                    track.artwork_path,
                    track.last_modified,
                    track.indexed_at,
                    track.album_group_key,
                    track.album_group_title
                ],
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get a track by ID
    pub fn get_track(&self, id: i64) -> Result<Option<LocalTrack>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM local_tracks WHERE id = ?")
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        stmt.query_row(params![id], |row| Self::row_to_track(row))
            .optional()
            .map_err(|e| LibraryError::Database(e.to_string()))
    }

    /// Get a track by file path (for non-CUE tracks)
    pub fn get_track_by_path(&self, path: &str) -> Result<Option<LocalTrack>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM local_tracks WHERE file_path = ? AND cue_file_path IS NULL")
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        stmt.query_row(params![path], |row| Self::row_to_track(row))
            .optional()
            .map_err(|e| LibraryError::Database(e.to_string()))
    }

    /// Delete all tracks in a folder
    pub fn delete_tracks_in_folder(&self, folder: &str) -> Result<usize, LibraryError> {
        let pattern = format!("{}%", folder);
        let count = self
            .conn
            .execute(
                "DELETE FROM local_tracks WHERE file_path LIKE ?",
                params![pattern],
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;
        Ok(count)
    }

    /// Clear all tracks
    pub fn clear_all_tracks(&self) -> Result<(), LibraryError> {
        self.conn
            .execute("DELETE FROM local_tracks", [])
            .map_err(|e| LibraryError::Database(e.to_string()))?;
        Ok(())
    }

    // === Query Methods ===

    /// Get all albums with optional hidden filter
    pub fn get_albums(&self, include_hidden: bool) -> Result<Vec<LocalAlbum>, LibraryError> {
        self.get_albums_with_filter(include_hidden, true)
    }

    /// Get all albums with optional filters for hidden and Qobuz downloads
    pub fn get_albums_with_filter(
        &self,
        include_hidden: bool,
        include_qobuz_downloads: bool,
    ) -> Result<Vec<LocalAlbum>, LibraryError> {
        let source_filter = if include_qobuz_downloads {
            ""
        } else {
            "AND (source IS NULL OR source != 'qobuz_download')"
        };

        let query = if include_hidden {
            format!(
                r#"
            SELECT
                group_key,
                MIN(title) as title,
                CASE
                    WHEN COUNT(DISTINCT artist) > 1 THEN 'Various Artists'
                    ELSE MIN(artist)
                END as artist,
                MIN(year) as year,
                MAX(artwork_path) as artwork,
                COUNT(*) as track_count,
                SUM(duration_secs) as total_duration,
                MAX(format) as format,
                MAX(bit_depth) as bit_depth,
                MAX(sample_rate) as sample_rate,
                MAX(group_key) as directory_path
            FROM (
                SELECT
                    COALESCE(album_group_key, album || '|' || COALESCE(album_artist, artist)) as group_key,
                    COALESCE(album_group_title, album) as title,
                    COALESCE(album_artist, artist) as artist,
                    year,
                    artwork_path,
                    duration_secs,
                    format,
                    bit_depth,
                    sample_rate
                FROM local_tracks
                WHERE 1=1 {}
            )
            GROUP BY group_key
            ORDER BY artist, title
            "#,
                source_filter
            )
        } else {
            format!(
                r#"
            SELECT
                group_key,
                MIN(title) as title,
                CASE
                    WHEN COUNT(DISTINCT artist) > 1 THEN 'Various Artists'
                    ELSE MIN(artist)
                END as artist,
                MIN(year) as year,
                MAX(artwork_path) as artwork,
                COUNT(*) as track_count,
                SUM(duration_secs) as total_duration,
                MAX(format) as format,
                MAX(bit_depth) as bit_depth,
                MAX(sample_rate) as sample_rate,
                MAX(group_key) as directory_path
            FROM (
                SELECT
                    COALESCE(album_group_key, album || '|' || COALESCE(album_artist, artist)) as group_key,
                    COALESCE(album_group_title, album) as title,
                    COALESCE(album_artist, artist) as artist,
                    year,
                    artwork_path,
                    duration_secs,
                    format,
                    bit_depth,
                    sample_rate
                FROM local_tracks
                WHERE 1=1 {}
            )
            WHERE group_key NOT IN (
                SELECT album_group_key FROM album_settings WHERE hidden = 1
            )
            GROUP BY group_key
            ORDER BY artist, title
            "#,
                source_filter
            )
        };

        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                let group_key: String = row.get(0)?;
                let album: String = row.get(1)?;
                let artist: String = row.get(2)?;
                Ok(LocalAlbum {
                    id: group_key.clone(),
                    title: album,
                    artist,
                    year: row.get(3)?,
                    artwork_path: row.get(4)?,
                    track_count: row.get(5)?,
                    total_duration_secs: row.get(6)?,
                    format: Self::parse_format(
                        &row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    ),
                    bit_depth: row.get(8)?,
                    sample_rate: row.get::<_, Option<u32>>(9)?.unwrap_or(44100),
                    directory_path: row
                        .get::<_, Option<String>>(10)?
                        .unwrap_or_else(|| group_key.clone()),
                })
            })
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let mut albums = Vec::new();
        for album in rows {
            albums.push(album.map_err(|e| LibraryError::Database(e.to_string()))?);
        }
        Ok(albums)
    }

    /// Get tracks for an album group
    pub fn get_album_tracks(&self, group_key: &str) -> Result<Vec<LocalTrack>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
            SELECT * FROM local_tracks
            WHERE COALESCE(album_group_key, album || '|' || COALESCE(album_artist, artist)) = ?
            ORDER BY disc_number, track_number, title
        "#,
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![group_key], |row| Self::row_to_track(row))
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let mut tracks = Vec::new();
        for track in rows {
            tracks.push(track.map_err(|e| LibraryError::Database(e.to_string()))?);
        }
        Ok(tracks)
    }

    /// Get all artists
    pub fn get_artists(&self) -> Result<Vec<LocalArtist>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
            SELECT
                COALESCE(album_artist, artist) as name,
                COUNT(DISTINCT COALESCE(album_group_key, album || '|' || COALESCE(album_artist, artist))) as album_count,
                COUNT(*) as track_count
            FROM local_tracks
            GROUP BY name
            ORDER BY name
        "#,
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(LocalArtist {
                    name: row.get(0)?,
                    album_count: row.get(1)?,
                    track_count: row.get(2)?,
                })
            })
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let mut artists = Vec::new();
        for artist in rows {
            artists.push(artist.map_err(|e| LibraryError::Database(e.to_string()))?);
        }
        Ok(artists)
    }

    /// Get album groups without artwork (for Discogs fetching)
    pub fn get_albums_without_artwork(
        &self,
    ) -> Result<Vec<(String, String, String)>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
            SELECT
                group_key,
                MIN(title) as title,
                CASE
                    WHEN COUNT(DISTINCT artist) > 1 THEN 'Various Artists'
                    ELSE MIN(artist)
                END as artist
            FROM (
                SELECT
                    COALESCE(album_group_key, album || '|' || COALESCE(album_artist, artist)) as group_key,
                    COALESCE(album_group_title, album) as title,
                    COALESCE(album_artist, artist) as artist,
                    artwork_path
                FROM local_tracks
                WHERE artwork_path IS NULL OR artwork_path = ''
            )
            GROUP BY group_key
            ORDER BY artist, title
        "#,
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let mut albums = Vec::new();
        for album in rows {
            albums.push(album.map_err(|e| LibraryError::Database(e.to_string()))?);
        }
        Ok(albums)
    }

    /// Update artwork path for all tracks in an album
    pub fn update_album_artwork(
        &self,
        album: &str,
        artist: &str,
        artwork_path: &str,
    ) -> Result<(), LibraryError> {
        self.conn
            .execute(
                r#"
            UPDATE local_tracks
            SET artwork_path = ?
            WHERE album = ? AND COALESCE(album_artist, artist) = ?
        "#,
                params![artwork_path, album, artist],
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;
        Ok(())
    }

    /// Update artwork path for all tracks in an album group
    pub fn update_album_group_artwork(
        &self,
        group_key: &str,
        artwork_path: &str,
    ) -> Result<(), LibraryError> {
        self.conn
            .execute(
                r#"
            UPDATE local_tracks
            SET artwork_path = ?
            WHERE COALESCE(album_group_key, album || '|' || COALESCE(album_artist, artist)) = ?
        "#,
                params![artwork_path, group_key],
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn find_album_group_key(
        &self,
        album: &str,
        artist: &str,
    ) -> Result<Option<String>, LibraryError> {
        self.conn
            .query_row(
                r#"
            SELECT COALESCE(album_group_key, album || '|' || COALESCE(album_artist, artist))
            FROM local_tracks
            WHERE album = ? AND COALESCE(album_artist, artist) = ?
            LIMIT 1
        "#,
                params![album, artist],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| LibraryError::Database(e.to_string()))
    }

    /// Search tracks by title, artist, or album
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<LocalTrack>, LibraryError> {
        let pattern = format!("%{}%", query);
        let mut stmt = self
            .conn
            .prepare(
                r#"
            SELECT * FROM local_tracks
            WHERE title LIKE ? OR artist LIKE ? OR album LIKE ?
            LIMIT ?
        "#,
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![&pattern, &pattern, &pattern, limit], |row| {
                Self::row_to_track(row)
            })
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let mut tracks = Vec::new();
        for track in rows {
            tracks.push(track.map_err(|e| LibraryError::Database(e.to_string()))?);
        }
        Ok(tracks)
    }

    /// Get library statistics
    pub fn get_stats(&self) -> Result<LibraryStats, LibraryError> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
            SELECT
                COUNT(*) as track_count,
                COUNT(DISTINCT COALESCE(album_group_key, album || '|' || COALESCE(album_artist, artist))) as album_count,
                COUNT(DISTINCT COALESCE(album_artist, artist)) as artist_count,
                COALESCE(SUM(duration_secs), 0) as total_duration,
                COALESCE(SUM(file_size_bytes), 0) as total_size
            FROM local_tracks
        "#,
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        stmt.query_row([], |row| {
            Ok(LibraryStats {
                track_count: row.get(0)?,
                album_count: row.get(1)?,
                artist_count: row.get(2)?,
                total_duration_secs: row.get(3)?,
                total_size_bytes: row.get(4)?,
            })
        })
        .map_err(|e| LibraryError::Database(e.to_string()))
    }

    // === Helpers ===

    /// Convert a database row to LocalTrack
    fn row_to_track(row: &rusqlite::Row) -> rusqlite::Result<LocalTrack> {
        Ok(LocalTrack {
            id: row.get(0)?,
            file_path: row.get(1)?,
            title: row.get(2)?,
            artist: row.get(3)?,
            album: row.get(4)?,
            album_artist: row.get(5)?,
            album_group_key: row.get::<_, Option<String>>(22)?.unwrap_or_default(),
            album_group_title: row.get::<_, Option<String>>(23)?.unwrap_or_default(),
            track_number: row.get(6)?,
            disc_number: row.get(7)?,
            year: row.get(8)?,
            genre: row.get(9)?,
            duration_secs: row.get(10)?,
            format: Self::parse_format(&row.get::<_, String>(11)?),
            bit_depth: row.get(12)?,
            sample_rate: row.get(13)?,
            channels: row.get(14)?,
            file_size_bytes: row.get(15)?,
            cue_file_path: row.get(16)?,
            cue_start_secs: row.get(17)?,
            cue_end_secs: row.get(18)?,
            artwork_path: row.get(19)?,
            last_modified: row.get(20)?,
            indexed_at: row.get(21)?,
        })
    }

    /// Parse format string to AudioFormat
    fn parse_format(s: &str) -> AudioFormat {
        match s.to_uppercase().as_str() {
            "FLAC" => AudioFormat::Flac,
            "ALAC" => AudioFormat::Alac,
            "WAV" => AudioFormat::Wav,
            "AIFF" => AudioFormat::Aiff,
            "APE" => AudioFormat::Ape,
            "MP3" => AudioFormat::Mp3,
            _ => AudioFormat::Unknown,
        }
    }
}

/// Library statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct LibraryStats {
    pub track_count: u32,
    pub album_count: u32,
    pub artist_count: u32,
    pub total_duration_secs: u64,
    pub total_size_bytes: u64,
}

/// Playlist local settings (enhances remote Qobuz playlists)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaylistSettings {
    pub qobuz_playlist_id: u64,
    pub custom_artwork_path: Option<String>,
    pub sort_by: String,
    pub sort_order: String,
    pub last_search_query: Option<String>,
    pub notes: Option<String>,
    pub hidden: bool,
    pub position: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Playlist statistics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaylistStats {
    pub qobuz_playlist_id: u64,
    pub play_count: u32,
    pub last_played_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Default for PlaylistSettings {
    fn default() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Self {
            qobuz_playlist_id: 0,
            custom_artwork_path: None,
            sort_by: "default".to_string(),
            sort_order: "asc".to_string(),
            last_search_query: None,
            notes: None,
            hidden: false,
            position: 0,
            created_at: now,
            updated_at: now,
        }
    }
}

impl Default for PlaylistStats {
    fn default() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Self {
            qobuz_playlist_id: 0,
            play_count: 0,
            last_played_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}

impl LibraryDatabase {
    // === Playlist Settings ===

    /// Get playlist settings by Qobuz playlist ID
    pub fn get_playlist_settings(&self, qobuz_playlist_id: u64) -> Result<Option<PlaylistSettings>, LibraryError> {
        let result = self.conn.query_row(
            "SELECT qobuz_playlist_id, custom_artwork_path, sort_by, sort_order,
                    last_search_query, notes, hidden, position, created_at, updated_at
             FROM playlist_settings WHERE qobuz_playlist_id = ?1",
            params![qobuz_playlist_id as i64],
            |row| {
                Ok(PlaylistSettings {
                    qobuz_playlist_id: row.get::<_, i64>(0)? as u64,
                    custom_artwork_path: row.get(1)?,
                    sort_by: row.get(2)?,
                    sort_order: row.get(3)?,
                    last_search_query: row.get(4)?,
                    notes: row.get(5)?,
                    hidden: row.get::<_, i32>(6)? != 0,
                    position: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            },
        ).optional()
        .map_err(|e| LibraryError::Database(format!("Failed to get playlist settings: {}", e)))?;

        Ok(result)
    }

    /// Save or update playlist settings
    pub fn save_playlist_settings(&self, settings: &PlaylistSettings) -> Result<(), LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn.execute(
            "INSERT INTO playlist_settings
                (qobuz_playlist_id, custom_artwork_path, sort_by, sort_order,
                 last_search_query, notes, hidden, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(qobuz_playlist_id) DO UPDATE SET
                custom_artwork_path = excluded.custom_artwork_path,
                sort_by = excluded.sort_by,
                sort_order = excluded.sort_order,
                last_search_query = excluded.last_search_query,
                notes = excluded.notes,
                hidden = excluded.hidden,
                position = excluded.position,
                updated_at = excluded.updated_at",
            params![
                settings.qobuz_playlist_id as i64,
                &settings.custom_artwork_path,
                &settings.sort_by,
                &settings.sort_order,
                &settings.last_search_query,
                &settings.notes,
                settings.hidden as i32,
                settings.position,
                settings.created_at,
                now,
            ],
        ).map_err(|e| LibraryError::Database(format!("Failed to save playlist settings: {}", e)))?;

        Ok(())
    }

    /// Update just the sort settings for a playlist
    pub fn update_playlist_sort(&self, qobuz_playlist_id: u64, sort_by: &str, sort_order: &str) -> Result<(), LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // First check if settings exist, if not create default
        let existing = self.get_playlist_settings(qobuz_playlist_id)?;
        if existing.is_none() {
            let mut settings = PlaylistSettings::default();
            settings.qobuz_playlist_id = qobuz_playlist_id;
            settings.sort_by = sort_by.to_string();
            settings.sort_order = sort_order.to_string();
            return self.save_playlist_settings(&settings);
        }

        self.conn.execute(
            "UPDATE playlist_settings SET sort_by = ?1, sort_order = ?2, updated_at = ?3
             WHERE qobuz_playlist_id = ?4",
            params![sort_by, sort_order, now, qobuz_playlist_id as i64],
        ).map_err(|e| LibraryError::Database(format!("Failed to update playlist sort: {}", e)))?;

        Ok(())
    }

    /// Update custom artwork path for a playlist
    pub fn update_playlist_artwork(&self, qobuz_playlist_id: u64, artwork_path: Option<&str>) -> Result<(), LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // First check if settings exist, if not create default
        let existing = self.get_playlist_settings(qobuz_playlist_id)?;
        if existing.is_none() {
            let mut settings = PlaylistSettings::default();
            settings.qobuz_playlist_id = qobuz_playlist_id;
            settings.custom_artwork_path = artwork_path.map(|s| s.to_string());
            return self.save_playlist_settings(&settings);
        }

        self.conn.execute(
            "UPDATE playlist_settings SET custom_artwork_path = ?1, updated_at = ?2
             WHERE qobuz_playlist_id = ?3",
            params![artwork_path, now, qobuz_playlist_id as i64],
        ).map_err(|e| LibraryError::Database(format!("Failed to update playlist artwork: {}", e)))?;

        Ok(())
    }

    /// Update last search query for a playlist
    pub fn update_playlist_search_query(&self, qobuz_playlist_id: u64, query: Option<&str>) -> Result<(), LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // First check if settings exist, if not create default
        let existing = self.get_playlist_settings(qobuz_playlist_id)?;
        if existing.is_none() {
            let mut settings = PlaylistSettings::default();
            settings.qobuz_playlist_id = qobuz_playlist_id;
            settings.last_search_query = query.map(|s| s.to_string());
            return self.save_playlist_settings(&settings);
        }

        self.conn.execute(
            "UPDATE playlist_settings SET last_search_query = ?1, updated_at = ?2
             WHERE qobuz_playlist_id = ?3",
            params![query, now, qobuz_playlist_id as i64],
        ).map_err(|e| LibraryError::Database(format!("Failed to update playlist search query: {}", e)))?;

        Ok(())
    }

    /// Delete playlist settings
    pub fn delete_playlist_settings(&self, qobuz_playlist_id: u64) -> Result<(), LibraryError> {
        self.conn.execute(
            "DELETE FROM playlist_settings WHERE qobuz_playlist_id = ?1",
            params![qobuz_playlist_id as i64],
        ).map_err(|e| LibraryError::Database(format!("Failed to delete playlist settings: {}", e)))?;

        Ok(())
    }

    /// Get all playlist settings (for syncing/export)
    pub fn get_all_playlist_settings(&self) -> Result<Vec<PlaylistSettings>, LibraryError> {
        let mut stmt = self.conn.prepare(
            "SELECT qobuz_playlist_id, custom_artwork_path, sort_by, sort_order,
                    last_search_query, notes, hidden, position, created_at, updated_at
             FROM playlist_settings ORDER BY position ASC, updated_at DESC"
        ).map_err(|e| LibraryError::Database(format!("Failed to prepare statement: {}", e)))?;

        let settings = stmt.query_map([], |row| {
            Ok(PlaylistSettings {
                qobuz_playlist_id: row.get::<_, i64>(0)? as u64,
                custom_artwork_path: row.get(1)?,
                sort_by: row.get(2)?,
                sort_order: row.get(3)?,
                last_search_query: row.get(4)?,
                notes: row.get(5)?,
                hidden: row.get::<_, i32>(6)? != 0,
                position: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        }).map_err(|e| LibraryError::Database(format!("Failed to query playlist settings: {}", e)))?;

        settings.collect::<Result<Vec<_>, _>>()
            .map_err(|e| LibraryError::Database(format!("Failed to collect playlist settings: {}", e)))
    }

    /// Update hidden status for a playlist
    pub fn set_playlist_hidden(&self, qobuz_playlist_id: u64, hidden: bool) -> Result<(), LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // First check if settings exist, if not create default
        let existing = self.get_playlist_settings(qobuz_playlist_id)?;
        if existing.is_none() {
            let mut settings = PlaylistSettings::default();
            settings.qobuz_playlist_id = qobuz_playlist_id;
            settings.hidden = hidden;
            return self.save_playlist_settings(&settings);
        }

        self.conn.execute(
            "UPDATE playlist_settings SET hidden = ?1, updated_at = ?2
             WHERE qobuz_playlist_id = ?3",
            params![hidden as i32, now, qobuz_playlist_id as i64],
        ).map_err(|e| LibraryError::Database(format!("Failed to update playlist hidden: {}", e)))?;

        Ok(())
    }

    /// Update position for a playlist
    pub fn set_playlist_position(&self, qobuz_playlist_id: u64, position: i32) -> Result<(), LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // First check if settings exist, if not create default
        let existing = self.get_playlist_settings(qobuz_playlist_id)?;
        if existing.is_none() {
            let mut settings = PlaylistSettings::default();
            settings.qobuz_playlist_id = qobuz_playlist_id;
            settings.position = position;
            return self.save_playlist_settings(&settings);
        }

        self.conn.execute(
            "UPDATE playlist_settings SET position = ?1, updated_at = ?2
             WHERE qobuz_playlist_id = ?3",
            params![position, now, qobuz_playlist_id as i64],
        ).map_err(|e| LibraryError::Database(format!("Failed to update playlist position: {}", e)))?;

        Ok(())
    }

    /// Bulk reorder playlists by setting positions
    pub fn reorder_playlists(&self, playlist_ids: &[u64]) -> Result<(), LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        for (index, &playlist_id) in playlist_ids.iter().enumerate() {
            // Ensure settings exist first
            let existing = self.get_playlist_settings(playlist_id)?;
            if existing.is_none() {
                let mut settings = PlaylistSettings::default();
                settings.qobuz_playlist_id = playlist_id;
                settings.position = index as i32;
                self.save_playlist_settings(&settings)?;
            } else {
                self.conn.execute(
                    "UPDATE playlist_settings SET position = ?1, updated_at = ?2
                     WHERE qobuz_playlist_id = ?3",
                    params![index as i32, now, playlist_id as i64],
                ).map_err(|e| LibraryError::Database(format!("Failed to reorder playlists: {}", e)))?;
            }
        }

        Ok(())
    }

    // === Playlist Stats ===

    /// Get playlist stats
    pub fn get_playlist_stats(&self, qobuz_playlist_id: u64) -> Result<Option<PlaylistStats>, LibraryError> {
        let result = self.conn.query_row(
            "SELECT qobuz_playlist_id, play_count, last_played_at, created_at, updated_at
             FROM playlist_stats WHERE qobuz_playlist_id = ?1",
            params![qobuz_playlist_id as i64],
            |row| {
                Ok(PlaylistStats {
                    qobuz_playlist_id: row.get::<_, i64>(0)? as u64,
                    play_count: row.get::<_, i32>(1)? as u32,
                    last_played_at: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        ).optional()
        .map_err(|e| LibraryError::Database(format!("Failed to get playlist stats: {}", e)))?;

        Ok(result)
    }

    /// Increment play count and update last_played_at for a playlist
    pub fn increment_playlist_play_count(&self, qobuz_playlist_id: u64) -> Result<PlaylistStats, LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Try to update existing, if none exists, insert new
        let existing = self.get_playlist_stats(qobuz_playlist_id)?;

        if let Some(mut stats) = existing {
            stats.play_count += 1;
            stats.last_played_at = Some(now);
            stats.updated_at = now;

            self.conn.execute(
                "UPDATE playlist_stats SET play_count = ?1, last_played_at = ?2, updated_at = ?3
                 WHERE qobuz_playlist_id = ?4",
                params![stats.play_count as i32, now, now, qobuz_playlist_id as i64],
            ).map_err(|e| LibraryError::Database(format!("Failed to increment play count: {}", e)))?;

            Ok(stats)
        } else {
            let stats = PlaylistStats {
                qobuz_playlist_id,
                play_count: 1,
                last_played_at: Some(now),
                created_at: now,
                updated_at: now,
            };

            self.conn.execute(
                "INSERT INTO playlist_stats (qobuz_playlist_id, play_count, last_played_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![qobuz_playlist_id as i64, 1, now, now, now],
            ).map_err(|e| LibraryError::Database(format!("Failed to create playlist stats: {}", e)))?;

            Ok(stats)
        }
    }

    /// Get all playlist stats (for sorting by play count)
    pub fn get_all_playlist_stats(&self) -> Result<Vec<PlaylistStats>, LibraryError> {
        let mut stmt = self.conn.prepare(
            "SELECT qobuz_playlist_id, play_count, last_played_at, created_at, updated_at
             FROM playlist_stats ORDER BY play_count DESC"
        ).map_err(|e| LibraryError::Database(format!("Failed to prepare statement: {}", e)))?;

        let stats = stmt.query_map([], |row| {
            Ok(PlaylistStats {
                qobuz_playlist_id: row.get::<_, i64>(0)? as u64,
                play_count: row.get::<_, i32>(1)? as u32,
                last_played_at: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        }).map_err(|e| LibraryError::Database(format!("Failed to query playlist stats: {}", e)))?;

        stats.collect::<Result<Vec<_>, _>>()
            .map_err(|e| LibraryError::Database(format!("Failed to collect playlist stats: {}", e)))
    }

    // === Playlist Local Tracks ===

    /// Add a local track to a playlist
    pub fn add_local_track_to_playlist(&self, qobuz_playlist_id: u64, local_track_id: i64, position: i32) -> Result<(), LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn.execute(
            "INSERT OR REPLACE INTO playlist_local_tracks
                (qobuz_playlist_id, local_track_id, position, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![qobuz_playlist_id as i64, local_track_id, position, now],
        ).map_err(|e| LibraryError::Database(format!("Failed to add local track to playlist: {}", e)))?;

        Ok(())
    }

    /// Remove a local track from a playlist
    pub fn remove_local_track_from_playlist(&self, qobuz_playlist_id: u64, local_track_id: i64) -> Result<(), LibraryError> {
        self.conn.execute(
            "DELETE FROM playlist_local_tracks
             WHERE qobuz_playlist_id = ?1 AND local_track_id = ?2",
            params![qobuz_playlist_id as i64, local_track_id],
        ).map_err(|e| LibraryError::Database(format!("Failed to remove local track from playlist: {}", e)))?;

        Ok(())
    }

    /// Get all local tracks in a playlist
    pub fn get_playlist_local_tracks(&self, qobuz_playlist_id: u64) -> Result<Vec<LocalTrack>, LibraryError> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.file_path, t.title, t.artist, t.album, t.album_artist,
                    t.album_group_key, t.album_group_title, t.track_number, t.disc_number,
                    t.year, t.genre, t.duration_secs, t.format, t.bit_depth, t.sample_rate,
                    t.channels, t.file_size_bytes, t.cue_file_path, t.cue_start_secs,
                    t.cue_end_secs, t.artwork_path, t.last_modified, t.indexed_at, plt.position
             FROM playlist_local_tracks plt
             JOIN local_tracks t ON plt.local_track_id = t.id
             WHERE plt.qobuz_playlist_id = ?1
             ORDER BY plt.position ASC"
        ).map_err(|e| LibraryError::Database(format!("Failed to prepare statement: {}", e)))?;

        let tracks = stmt.query_map(params![qobuz_playlist_id as i64], |row| {
            Ok(LocalTrack {
                id: row.get(0)?,
                file_path: row.get(1)?,
                title: row.get(2)?,
                artist: row.get(3)?,
                album: row.get(4)?,
                album_artist: row.get(5)?,
                album_group_key: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                album_group_title: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                track_number: row.get(8)?,
                disc_number: row.get(9)?,
                year: row.get(10)?,
                genre: row.get(11)?,
                duration_secs: row.get(12)?,
                format: Self::parse_format(&row.get::<_, String>(13)?),
                bit_depth: row.get(14)?,
                sample_rate: row.get(15)?,
                channels: row.get(16)?,
                file_size_bytes: row.get(17)?,
                cue_file_path: row.get(18)?,
                cue_start_secs: row.get(19)?,
                cue_end_secs: row.get(20)?,
                artwork_path: row.get(21)?,
                last_modified: row.get(22)?,
                indexed_at: row.get(23)?,
            })
        }).map_err(|e| LibraryError::Database(format!("Failed to query playlist local tracks: {}", e)))?;

        tracks.collect::<Result<Vec<_>, _>>()
            .map_err(|e| LibraryError::Database(format!("Failed to collect playlist local tracks: {}", e)))
    }

    /// Get count of local tracks in a playlist
    pub fn get_playlist_local_track_count(&self, qobuz_playlist_id: u64) -> Result<u32, LibraryError> {
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM playlist_local_tracks WHERE qobuz_playlist_id = ?1",
            params![qobuz_playlist_id as i64],
            |row| row.get(0),
        ).map_err(|e| LibraryError::Database(format!("Failed to count playlist local tracks: {}", e)))?;

        Ok(count)
    }

    /// Update position of a local track in a playlist
    pub fn update_local_track_position(&self, qobuz_playlist_id: u64, local_track_id: i64, new_position: i32) -> Result<(), LibraryError> {
        self.conn.execute(
            "UPDATE playlist_local_tracks SET position = ?1
             WHERE qobuz_playlist_id = ?2 AND local_track_id = ?3",
            params![new_position, qobuz_playlist_id as i64, local_track_id],
        ).map_err(|e| LibraryError::Database(format!("Failed to update local track position: {}", e)))?;

        Ok(())
    }

    /// Clear all local tracks from a playlist
    pub fn clear_playlist_local_tracks(&self, qobuz_playlist_id: u64) -> Result<(), LibraryError> {
        self.conn.execute(
            "DELETE FROM playlist_local_tracks WHERE qobuz_playlist_id = ?1",
            params![qobuz_playlist_id as i64],
        ).map_err(|e| LibraryError::Database(format!("Failed to clear playlist local tracks: {}", e)))?;

        Ok(())
    }

    // === Album Settings ===

    /// Get album settings
    pub fn get_album_settings(&self, album_group_key: &str) -> Result<Option<crate::library::AlbumSettings>, LibraryError> {
        let result = self.conn.query_row(
            "SELECT album_group_key, hidden, created_at, updated_at
             FROM album_settings WHERE album_group_key = ?1",
            params![album_group_key],
            |row| {
                Ok(crate::library::AlbumSettings {
                    album_group_key: row.get(0)?,
                    hidden: row.get::<_, i32>(1)? != 0,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        ).optional()
        .map_err(|e| LibraryError::Database(format!("Failed to get album settings: {}", e)))?;

        Ok(result)
    }

    /// Set album hidden status
    pub fn set_album_hidden(&self, album_group_key: &str, hidden: bool) -> Result<(), LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn.execute(
            "INSERT INTO album_settings (album_group_key, hidden, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(album_group_key) DO UPDATE SET
                hidden = excluded.hidden,
                updated_at = excluded.updated_at",
            params![album_group_key, hidden as i32, now, now],
        ).map_err(|e| LibraryError::Database(format!("Failed to set album hidden: {}", e)))?;

        Ok(())
    }

    /// Get all hidden albums
    pub fn get_hidden_albums(&self) -> Result<Vec<String>, LibraryError> {
        let mut stmt = self.conn
            .prepare("SELECT album_group_key FROM album_settings WHERE hidden = 1")
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| LibraryError::Database(e.to_string()))
    }

    // === Qobuz Downloads Integration ===

    /// Check if a track exists by Qobuz track ID
    pub fn track_exists_by_qobuz_id(&self, qobuz_track_id: u64) -> Result<bool, LibraryError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM local_tracks WHERE qobuz_track_id = ?1",
                params![qobuz_track_id as i64],
                |row| row.get(0),
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;
        Ok(count > 0)
    }

    /// Insert a Qobuz download into the library
    pub fn insert_qobuz_download_direct(
        &self,
        track_id: u64,
        title: &str,
        artist: &str,
        album: Option<&str>,
        duration_secs: u64,
        file_path: &str,
        bit_depth: Option<u32>,
        sample_rate: Option<f64>,
    ) -> Result<(), LibraryError> {
        self.conn.execute(
            r#"
            INSERT INTO local_tracks (
                file_path, title, artist, album, album_artist,
                track_number, disc_number, year, duration_secs,
                format, bit_depth, sample_rate, channels,
                source, qobuz_track_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'qobuz_download', ?14)
            "#,
            params![
                file_path,
                title,
                artist,
                album.unwrap_or("Unknown Album"),
                album.unwrap_or("Unknown Album"), // Use album as album_artist
                0, // track_number - will be updated if metadata is available
                None::<u32>, // disc_number
                None::<u32>, // year
                duration_secs as i64,
                "flac", // Default format for downloads
                bit_depth.map(|v| v as i64),
                sample_rate,
                2, // Assume stereo
                track_id as i64,
            ],
        )
        .map_err(|e| LibraryError::Database(format!("Failed to insert Qobuz download: {}", e)))?;
        Ok(())
    }

    /// Insert a Qobuz download with full metadata and album grouping
    pub fn insert_qobuz_download_with_grouping(
        &self,
        track_id: u64,
        title: &str,
        artist: &str,
        album: Option<&str>,
        album_artist: Option<&str>,
        track_number: Option<u32>,
        disc_number: Option<u32>,
        year: Option<u32>,
        duration_secs: u64,
        file_path: &str,
        album_group_key: &str,
        album_group_title: &str,
        bit_depth: Option<u32>,
        sample_rate: Option<f64>,
    ) -> Result<(), LibraryError> {
        self.conn.execute(
            r#"
            INSERT INTO local_tracks (
                file_path, title, artist, album, album_artist,
                track_number, disc_number, year, duration_secs,
                format, bit_depth, sample_rate, channels,
                album_group_key, album_group_title,
                source, qobuz_track_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 'qobuz_download', ?16)
            "#,
            params![
                file_path,
                title,
                artist,
                album.unwrap_or("Unknown Album"),
                album_artist.unwrap_or(artist),
                track_number.map(|v| v as i64),
                disc_number.map(|v| v as i64),
                year.map(|v| v as i64),
                duration_secs as i64,
                "flac",
                bit_depth.map(|v| v as i64),
                sample_rate,
                2, // Assume stereo
                album_group_key,
                album_group_title,
                track_id as i64,
            ],
        )
        .map_err(|e| LibraryError::Database(format!("Failed to insert Qobuz download: {}", e)))?;
        Ok(())
    }

    /// Remove a Qobuz download from the library by track_id
    pub fn remove_qobuz_download(&self, qobuz_track_id: u64) -> Result<(), LibraryError> {
        self.conn.execute(
            "DELETE FROM local_tracks WHERE qobuz_track_id = ?1 AND source = 'qobuz_download'",
            params![qobuz_track_id as i64],
        )
        .map_err(|e| LibraryError::Database(format!("Failed to remove Qobuz download: {}", e)))?;
        Ok(())
    }

    /// Remove all Qobuz downloads from the library
    pub fn remove_all_qobuz_downloads(&self) -> Result<usize, LibraryError> {
        let count = self.conn.execute(
            "DELETE FROM local_tracks WHERE source = 'qobuz_download'",
            [],
        )
        .map_err(|e| LibraryError::Database(format!("Failed to remove all Qobuz downloads: {}", e)))?;
        Ok(count)
    }
}
