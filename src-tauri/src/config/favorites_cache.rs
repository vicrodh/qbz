//! Local cache for favorite tracks, albums, and artists
//!
//! This module provides local persistence for favorites to:
//! - Enable instant UI updates without API calls
//! - Reduce load on Qobuz API
//! - Provide offline favorite status display
//!
//! Sync strategy:
//! - On login: Fetch all favorites from API and populate local cache
//! - On toggle: API call first, then update local cache on success
//! - FavoritesView reads from API and syncs local cache

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Represents a cached favorite track
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFavoriteTrack {
    pub track_id: i64,
}

/// Represents a cached favorite album
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFavoriteAlbum {
    pub album_id: String,
}

/// Represents a cached favorite artist
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFavoriteArtist {
    pub artist_id: i64,
}

pub struct FavoritesCacheStore {
    conn: Connection,
}

impl FavoritesCacheStore {
    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open favorites cache database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to enable WAL for favorites cache database: {}", e))?;

        // Create tables
        conn.execute(
            "CREATE TABLE IF NOT EXISTS favorite_tracks (
                track_id INTEGER PRIMARY KEY,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )
        .map_err(|e| format!("Failed to create favorite_tracks table: {}", e))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS favorite_albums (
                album_id TEXT PRIMARY KEY,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )
        .map_err(|e| format!("Failed to create favorite_albums table: {}", e))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS favorite_artists (
                artist_id INTEGER PRIMARY KEY,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )
        .map_err(|e| format!("Failed to create favorite_artists table: {}", e))?;

        Ok(Self { conn })
    }

    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "favorites_cache.db")
    }

    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "favorites_cache.db")
    }

    // ============ Track favorites ============

    pub fn get_favorite_track_ids(&self) -> Result<Vec<i64>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT track_id FROM favorite_tracks")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Failed to query favorite tracks: {}", e))?;

        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(ids)
    }

    pub fn is_track_favorite(&self, track_id: i64) -> Result<bool, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM favorite_tracks WHERE track_id = ?1")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let exists = stmt
            .exists(params![track_id])
            .map_err(|e| format!("Failed to check favorite: {}", e))?;

        Ok(exists)
    }

    pub fn add_favorite_track(&self, track_id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO favorite_tracks (track_id) VALUES (?1)",
                params![track_id],
            )
            .map_err(|e| format!("Failed to add favorite track: {}", e))?;
        Ok(())
    }

    pub fn remove_favorite_track(&self, track_id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM favorite_tracks WHERE track_id = ?1",
                params![track_id],
            )
            .map_err(|e| format!("Failed to remove favorite track: {}", e))?;
        Ok(())
    }

    pub fn sync_favorite_tracks(&self, track_ids: &[i64]) -> Result<(), String> {
        // Clear existing and insert new
        self.conn
            .execute("DELETE FROM favorite_tracks", [])
            .map_err(|e| format!("Failed to clear favorite tracks: {}", e))?;

        for &track_id in track_ids {
            self.conn
                .execute(
                    "INSERT INTO favorite_tracks (track_id) VALUES (?1)",
                    params![track_id],
                )
                .map_err(|e| format!("Failed to insert favorite track: {}", e))?;
        }
        Ok(())
    }

    // ============ Album favorites ============

    pub fn get_favorite_album_ids(&self) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT album_id FROM favorite_albums")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Failed to query favorite albums: {}", e))?;

        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(ids)
    }

    pub fn is_album_favorite(&self, album_id: &str) -> Result<bool, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM favorite_albums WHERE album_id = ?1")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let exists = stmt
            .exists(params![album_id])
            .map_err(|e| format!("Failed to check favorite: {}", e))?;

        Ok(exists)
    }

    pub fn add_favorite_album(&self, album_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO favorite_albums (album_id) VALUES (?1)",
                params![album_id],
            )
            .map_err(|e| format!("Failed to add favorite album: {}", e))?;
        Ok(())
    }

    pub fn remove_favorite_album(&self, album_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM favorite_albums WHERE album_id = ?1",
                params![album_id],
            )
            .map_err(|e| format!("Failed to remove favorite album: {}", e))?;
        Ok(())
    }

    pub fn sync_favorite_albums(&self, album_ids: &[String]) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM favorite_albums", [])
            .map_err(|e| format!("Failed to clear favorite albums: {}", e))?;

        for album_id in album_ids {
            self.conn
                .execute(
                    "INSERT INTO favorite_albums (album_id) VALUES (?1)",
                    params![album_id],
                )
                .map_err(|e| format!("Failed to insert favorite album: {}", e))?;
        }
        Ok(())
    }

    // ============ Artist favorites ============

    pub fn get_favorite_artist_ids(&self) -> Result<Vec<i64>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT artist_id FROM favorite_artists")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Failed to query favorite artists: {}", e))?;

        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(ids)
    }

    pub fn is_artist_favorite(&self, artist_id: i64) -> Result<bool, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM favorite_artists WHERE artist_id = ?1")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let exists = stmt
            .exists(params![artist_id])
            .map_err(|e| format!("Failed to check favorite: {}", e))?;

        Ok(exists)
    }

    pub fn add_favorite_artist(&self, artist_id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO favorite_artists (artist_id) VALUES (?1)",
                params![artist_id],
            )
            .map_err(|e| format!("Failed to add favorite artist: {}", e))?;
        Ok(())
    }

    pub fn remove_favorite_artist(&self, artist_id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM favorite_artists WHERE artist_id = ?1",
                params![artist_id],
            )
            .map_err(|e| format!("Failed to remove favorite artist: {}", e))?;
        Ok(())
    }

    pub fn sync_favorite_artists(&self, artist_ids: &[i64]) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM favorite_artists", [])
            .map_err(|e| format!("Failed to clear favorite artists: {}", e))?;

        for &artist_id in artist_ids {
            self.conn
                .execute(
                    "INSERT INTO favorite_artists (artist_id) VALUES (?1)",
                    params![artist_id],
                )
                .map_err(|e| format!("Failed to insert favorite artist: {}", e))?;
        }
        Ok(())
    }

    // ============ Clear all (for logout) ============

    pub fn clear_all(&self) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM favorite_tracks", [])
            .map_err(|e| format!("Failed to clear favorite tracks: {}", e))?;
        self.conn
            .execute("DELETE FROM favorite_albums", [])
            .map_err(|e| format!("Failed to clear favorite albums: {}", e))?;
        self.conn
            .execute("DELETE FROM favorite_artists", [])
            .map_err(|e| format!("Failed to clear favorite artists: {}", e))?;
        Ok(())
    }
}

// ============ Tauri State ============

pub struct FavoritesCacheState {
    pub store: Arc<Mutex<Option<FavoritesCacheStore>>>,
}

impl FavoritesCacheState {
    pub fn new() -> Result<Self, String> {
        let store = FavoritesCacheStore::new()?;
        Ok(Self {
            store: Arc::new(Mutex::new(Some(store))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }

    pub fn init_at(&self, base_dir: &Path) -> Result<(), String> {
        let new_store = FavoritesCacheStore::new_at(base_dir)?;
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock favorites cache store".to_string())?;
        *guard = Some(new_store);
        Ok(())
    }

    pub fn teardown(&self) -> Result<(), String> {
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock favorites cache store".to_string())?;
        *guard = None;
        Ok(())
    }
}

// ============ Tauri Commands ============

/// Get all cached favorite track IDs
#[tauri::command]
pub fn get_cached_favorite_tracks(
    state: tauri::State<FavoritesCacheState>,
) -> Result<Vec<i64>, String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_favorite_track_ids()
}

/// Get all cached favorite album IDs
#[tauri::command]
pub fn get_cached_favorite_albums(
    state: tauri::State<FavoritesCacheState>,
) -> Result<Vec<String>, String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_favorite_album_ids()
}

/// Get all cached favorite artist IDs
#[tauri::command]
pub fn get_cached_favorite_artists(
    state: tauri::State<FavoritesCacheState>,
) -> Result<Vec<i64>, String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_favorite_artist_ids()
}

/// Add a track to local favorites cache (call after API success)
#[tauri::command]
pub fn cache_favorite_track(
    track_id: i64,
    state: tauri::State<FavoritesCacheState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.add_favorite_track(track_id)
}

/// Remove a track from local favorites cache (call after API success)
#[tauri::command]
pub fn uncache_favorite_track(
    track_id: i64,
    state: tauri::State<FavoritesCacheState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.remove_favorite_track(track_id)
}

/// Add an album to local favorites cache (call after API success)
#[tauri::command]
pub fn cache_favorite_album(
    album_id: String,
    state: tauri::State<FavoritesCacheState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.add_favorite_album(&album_id)
}

/// Remove an album from local favorites cache (call after API success)
#[tauri::command]
pub fn uncache_favorite_album(
    album_id: String,
    state: tauri::State<FavoritesCacheState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.remove_favorite_album(&album_id)
}

/// Add an artist to local favorites cache (call after API success)
#[tauri::command]
pub fn cache_favorite_artist(
    artist_id: i64,
    state: tauri::State<FavoritesCacheState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.add_favorite_artist(artist_id)
}

/// Remove an artist from local favorites cache (call after API success)
#[tauri::command]
pub fn uncache_favorite_artist(
    artist_id: i64,
    state: tauri::State<FavoritesCacheState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.remove_favorite_artist(artist_id)
}

/// Sync track favorites from a list of IDs (call after fetching from API)
#[tauri::command]
pub fn sync_cached_favorite_tracks(
    track_ids: Vec<i64>,
    state: tauri::State<FavoritesCacheState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.sync_favorite_tracks(&track_ids)
}

/// Sync album favorites from a list of IDs (call after fetching from API)
#[tauri::command]
pub fn sync_cached_favorite_albums(
    album_ids: Vec<String>,
    state: tauri::State<FavoritesCacheState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.sync_favorite_albums(&album_ids)
}

/// Sync artist favorites from a list of IDs (call after fetching from API)
#[tauri::command]
pub fn sync_cached_favorite_artists(
    artist_ids: Vec<i64>,
    state: tauri::State<FavoritesCacheState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.sync_favorite_artists(&artist_ids)
}

/// Clear all cached favorites (call on logout)
#[tauri::command]
pub fn clear_favorites_cache(state: tauri::State<FavoritesCacheState>) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.clear_all()
}

/// Check if a track is cached as favorite.
#[tauri::command]
pub fn is_track_favorite(
    track_id: i64,
    state: tauri::State<FavoritesCacheState>,
) -> Result<bool, String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites cache store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.is_track_favorite(track_id)
}
