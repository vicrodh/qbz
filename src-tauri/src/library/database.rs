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
                UNIQUE(file_path, cue_start_secs)
            );

            CREATE INDEX IF NOT EXISTS idx_tracks_artist ON local_tracks(artist);
            CREATE INDEX IF NOT EXISTS idx_tracks_album ON local_tracks(album);
            CREATE INDEX IF NOT EXISTS idx_tracks_album_artist ON local_tracks(album_artist);
            CREATE INDEX IF NOT EXISTS idx_tracks_file_path ON local_tracks(file_path);
            CREATE INDEX IF NOT EXISTS idx_tracks_title ON local_tracks(title);
        "#,
            )
            .map_err(|e| LibraryError::Database(format!("Failed to create schema: {}", e)))?;

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
                cue_start_secs, cue_end_secs, artwork_path, last_modified, indexed_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
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
                    track.indexed_at
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

    /// Get all albums (paginated)
    pub fn get_albums(&self, limit: u32, offset: u32) -> Result<Vec<LocalAlbum>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
            SELECT
                album,
                COALESCE(album_artist, artist) as artist,
                MIN(year) as year,
                MAX(artwork_path) as artwork,
                COUNT(*) as track_count,
                SUM(duration_secs) as total_duration,
                MAX(format) as format,
                MIN(file_path) as directory_path
            FROM local_tracks
            GROUP BY album, COALESCE(album_artist, artist)
            ORDER BY artist, album
            LIMIT ? OFFSET ?
        "#,
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![limit, offset], |row| {
                let album: String = row.get(0)?;
                let artist: String = row.get(1)?;
                Ok(LocalAlbum {
                    id: format!("{}_{}", artist, album),
                    title: album,
                    artist,
                    year: row.get(2)?,
                    artwork_path: row.get(3)?,
                    track_count: row.get(4)?,
                    total_duration_secs: row.get(5)?,
                    format: Self::parse_format(
                        &row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    ),
                    directory_path: row.get::<_, String>(7).unwrap_or_default(),
                })
            })
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let mut albums = Vec::new();
        for album in rows {
            albums.push(album.map_err(|e| LibraryError::Database(e.to_string()))?);
        }
        Ok(albums)
    }

    /// Get tracks for an album
    pub fn get_album_tracks(
        &self,
        album: &str,
        artist: &str,
    ) -> Result<Vec<LocalTrack>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
            SELECT * FROM local_tracks
            WHERE album = ? AND COALESCE(album_artist, artist) = ?
            ORDER BY disc_number, track_number, title
        "#,
            )
            .map_err(|e| LibraryError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![album, artist], |row| Self::row_to_track(row))
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
                COUNT(DISTINCT album) as album_count,
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
                COUNT(DISTINCT album || COALESCE(album_artist, artist)) as album_count,
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
