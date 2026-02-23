use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

fn default_streamable() -> bool {
    true
}

/// Represents a track in the persisted queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedQueueTrack {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub hires: bool,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
    #[serde(default)]
    pub is_local: bool,
    pub album_id: Option<String>,
    pub artist_id: Option<u64>,
    #[serde(default = "default_streamable")]
    pub streamable: bool,
    #[serde(default)]
    pub source: Option<String>,
}

/// Represents the full persisted session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub queue_tracks: Vec<PersistedQueueTrack>,
    pub current_index: Option<usize>,
    pub current_position_secs: u64,
    pub volume: f32,
    pub shuffle_enabled: bool,
    pub repeat_mode: String, // "off", "all", "one"
    pub was_playing: bool,
    pub saved_at: i64,
    #[serde(default = "default_last_view")]
    pub last_view: String,
    #[serde(default)]
    pub view_context_id: Option<String>,
    #[serde(default)]
    pub view_context_type: Option<String>,
}

fn default_last_view() -> String {
    "home".to_string()
}

impl Default for PersistedSession {
    fn default() -> Self {
        Self {
            queue_tracks: Vec::new(),
            current_index: None,
            current_position_secs: 0,
            volume: 0.75,
            shuffle_enabled: false,
            repeat_mode: "off".to_string(),
            was_playing: false,
            saved_at: 0,
            last_view: "home".to_string(),
            view_context_id: None,
            view_context_type: None,
        }
    }
}

pub struct SessionStore {
    conn: Connection,
}

impl SessionStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "session.db")
    }

    /// Open the session store at a specific base directory
    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "session.db")
    }

    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open session database: {}", e))?;

        // Enable WAL mode for better concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to set WAL mode: {}", e))?;

        // Create tables
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS player_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                current_index INTEGER,
                current_position_secs INTEGER NOT NULL DEFAULT 0,
                volume REAL NOT NULL DEFAULT 0.75,
                shuffle_enabled INTEGER NOT NULL DEFAULT 0,
                repeat_mode TEXT NOT NULL DEFAULT 'off',
                was_playing INTEGER NOT NULL DEFAULT 0,
                saved_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS queue_tracks (
                position INTEGER PRIMARY KEY,
                track_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT NOT NULL,
                duration_secs INTEGER NOT NULL,
                artwork_url TEXT,
                hires INTEGER NOT NULL DEFAULT 0,
                bit_depth INTEGER,
                sample_rate REAL,
                source TEXT
            );

            -- Insert default row if not exists
            INSERT OR IGNORE INTO player_state (id, current_position_secs, volume, shuffle_enabled, repeat_mode, was_playing, saved_at)
            VALUES (1, 0, 0.75, 0, 'off', 0, 0);
            "
        ).map_err(|e| format!("Failed to create session tables: {}", e))?;

        // Migrate existing queue_tracks table to add new columns if they don't exist
        // SQLite doesn't support IF NOT EXISTS for ALTER TABLE, so we check the schema
        let has_hires: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('queue_tracks') WHERE name = 'hires'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_hires {
            let _ = conn.execute_batch(
                "
                ALTER TABLE queue_tracks ADD COLUMN hires INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE queue_tracks ADD COLUMN bit_depth INTEGER;
                ALTER TABLE queue_tracks ADD COLUMN sample_rate REAL;
                ",
            );
        }

        // Add is_local, album_id, artist_id columns if they don't exist
        let has_is_local: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('queue_tracks') WHERE name = 'is_local'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_is_local {
            let _ = conn.execute_batch(
                "
                ALTER TABLE queue_tracks ADD COLUMN is_local INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE queue_tracks ADD COLUMN album_id TEXT;
                ALTER TABLE queue_tracks ADD COLUMN artist_id INTEGER;
                ",
            );
        }

        let has_source: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('queue_tracks') WHERE name = 'source'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_source {
            let _ = conn.execute_batch(
                "
                ALTER TABLE queue_tracks ADD COLUMN source TEXT;
                ",
            );
        }

        // Add last_view, view_context_id, view_context_type columns to player_state
        let has_last_view: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('player_state') WHERE name = 'last_view'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_last_view {
            let _ = conn.execute_batch(
                "
                ALTER TABLE player_state ADD COLUMN last_view TEXT NOT NULL DEFAULT 'home';
                ALTER TABLE player_state ADD COLUMN view_context_id TEXT;
                ALTER TABLE player_state ADD COLUMN view_context_type TEXT;
                ",
            );
        }

        Ok(Self { conn })
    }

    /// Save the complete session state
    pub fn save_session(&self, session: &PersistedSession) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Use a transaction for atomicity
        self.conn
            .execute("BEGIN TRANSACTION", [])
            .map_err(|e| format!("Failed to begin transaction: {}", e))?;

        // Clear existing queue
        if let Err(e) = self.conn.execute("DELETE FROM queue_tracks", []) {
            let _ = self.conn.execute("ROLLBACK", []);
            return Err(format!("Failed to clear queue: {}", e));
        }

        // Insert queue tracks
        for (pos, track) in session.queue_tracks.iter().enumerate() {
            if let Err(e) = self.conn.execute(
                "INSERT INTO queue_tracks (position, track_id, title, artist, album, duration_secs, artwork_url, hires, bit_depth, sample_rate, is_local, album_id, artist_id, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    pos as i64,
                    track.id as i64,
                    track.title,
                    track.artist,
                    track.album,
                    track.duration_secs as i64,
                    track.artwork_url,
                    track.hires as i64,
                    track.bit_depth.map(|v| v as i64),
                    track.sample_rate,
                    track.is_local as i64,
                    track.album_id,
                    track.artist_id.map(|v| v as i64),
                    track.source,
                ],
            ) {
                let _ = self.conn.execute("ROLLBACK", []);
                return Err(format!("Failed to insert queue track: {}", e));
            }
        }

        // Update player state
        if let Err(e) = self.conn.execute(
            "UPDATE player_state SET
                current_index = ?1,
                current_position_secs = ?2,
                volume = ?3,
                shuffle_enabled = ?4,
                repeat_mode = ?5,
                was_playing = ?6,
                saved_at = ?7,
                last_view = ?8,
                view_context_id = ?9,
                view_context_type = ?10
             WHERE id = 1",
            params![
                session.current_index.map(|i| i as i64),
                session.current_position_secs as i64,
                session.volume as f64,
                session.shuffle_enabled as i64,
                session.repeat_mode,
                session.was_playing as i64,
                now,
                session.last_view,
                session.view_context_id,
                session.view_context_type,
            ],
        ) {
            let _ = self.conn.execute("ROLLBACK", []);
            return Err(format!("Failed to update player state: {}", e));
        }

        self.conn
            .execute("COMMIT", [])
            .map_err(|e| format!("Failed to commit transaction: {}", e))?;

        Ok(())
    }

    /// Load the persisted session state
    pub fn load_session(&self) -> Result<PersistedSession, String> {
        // Load player state
        let (current_index, current_position_secs, volume, shuffle_enabled, repeat_mode, was_playing, saved_at, last_view, view_context_id, view_context_type):
            (Option<i64>, i64, f64, i64, String, i64, i64, String, Option<String>, Option<String>) = self.conn
            .query_row(
                "SELECT current_index, current_position_secs, volume, shuffle_enabled, repeat_mode, was_playing, saved_at, last_view, view_context_id, view_context_type
                 FROM player_state WHERE id = 1",
                [],
                |row| Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get::<_, String>(7).unwrap_or_else(|_| "home".to_string()),
                    row.get(8)?,
                    row.get(9)?,
                )),
            )
            .map_err(|e| format!("Failed to load player state: {}", e))?;

        // Load queue tracks
        let mut stmt = self.conn
            .prepare("SELECT track_id, title, artist, album, duration_secs, artwork_url, hires, bit_depth, sample_rate, is_local, album_id, artist_id, source FROM queue_tracks ORDER BY position")
            .map_err(|e| format!("Failed to prepare queue query: {}", e))?;

        let tracks: Vec<PersistedQueueTrack> = stmt
            .query_map([], |row| {
                Ok(PersistedQueueTrack {
                    id: row.get::<_, i64>(0)? as u64,
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    album: row.get(3)?,
                    duration_secs: row.get::<_, i64>(4)? as u64,
                    artwork_url: row.get(5)?,
                    hires: row.get::<_, i64>(6).unwrap_or(0) != 0,
                    bit_depth: row.get::<_, Option<i64>>(7)?.map(|v| v as u32),
                    sample_rate: row.get(8)?,
                    is_local: row.get::<_, i64>(9).unwrap_or(0) != 0,
                    album_id: row.get(10)?,
                    artist_id: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                    streamable: true, // Default to true for persisted tracks
                    source: row.get(12)?,
                })
            })
            .map_err(|e| format!("Failed to query queue tracks: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(PersistedSession {
            queue_tracks: tracks,
            current_index: current_index.map(|i| i as usize),
            current_position_secs: current_position_secs as u64,
            volume: volume as f32,
            shuffle_enabled: shuffle_enabled != 0,
            repeat_mode,
            was_playing: was_playing != 0,
            saved_at,
            last_view,
            view_context_id,
            view_context_type,
        })
    }

    /// Quick save of just player position (for debounced saves during playback)
    pub fn save_position(&self, position_secs: u64) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.conn
            .execute(
                "UPDATE player_state SET current_position_secs = ?1, saved_at = ?2 WHERE id = 1",
                params![position_secs as i64, now],
            )
            .map_err(|e| format!("Failed to save position: {}", e))?;

        Ok(())
    }

    /// Quick save of volume
    pub fn save_volume(&self, volume: f32) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE player_state SET volume = ?1 WHERE id = 1",
                params![volume as f64],
            )
            .map_err(|e| format!("Failed to save volume: {}", e))?;

        Ok(())
    }

    /// Save shuffle/repeat state
    pub fn save_playback_mode(&self, shuffle: bool, repeat_mode: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE player_state SET shuffle_enabled = ?1, repeat_mode = ?2 WHERE id = 1",
                params![shuffle as i64, repeat_mode],
            )
            .map_err(|e| format!("Failed to save playback mode: {}", e))?;

        Ok(())
    }

    /// Clear the session (e.g., on logout)
    pub fn clear_session(&self) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM queue_tracks", [])
            .map_err(|e| format!("Failed to clear queue: {}", e))?;

        self.conn.execute(
            "UPDATE player_state SET current_index = NULL, current_position_secs = 0, was_playing = 0, last_view = 'home', view_context_id = NULL, view_context_type = NULL WHERE id = 1",
            [],
        ).map_err(|e| format!("Failed to reset player state: {}", e))?;

        Ok(())
    }
}

/// Thread-safe wrapper for SessionStore
pub struct SessionStoreState {
    pub store: Arc<Mutex<Option<SessionStore>>>,
}

impl SessionStoreState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(Some(SessionStore::new()?))),
        })
    }

    /// Create an empty state (no active session store)
    pub fn new_empty() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }

    /// Initialize the store at a specific directory
    pub fn init_at(&self, base_dir: &Path) -> Result<(), String> {
        let store = SessionStore::new_at(base_dir)?;
        *self
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))? = Some(store);
        Ok(())
    }

    /// Close the store (logout)
    pub fn teardown(&self) {
        if let Ok(mut guard) = self.store.lock() {
            *guard = None;
        }
    }
}

// Tauri commands
#[tauri::command]
pub fn save_session_state(
    state: tauri::State<'_, SessionStoreState>,
    queue_tracks: Vec<PersistedQueueTrack>,
    current_index: Option<usize>,
    current_position_secs: u64,
    volume: f32,
    shuffle_enabled: bool,
    repeat_mode: String,
    was_playing: bool,
) -> Result<(), String> {
    let session = PersistedSession {
        queue_tracks,
        current_index,
        current_position_secs,
        volume,
        shuffle_enabled,
        repeat_mode,
        was_playing,
        saved_at: 0, // Will be set in save_session
        last_view: "home".to_string(),
        view_context_id: None,
        view_context_type: None,
    };

    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.save_session(&session)
}

#[tauri::command]
pub fn load_session_state(
    state: tauri::State<'_, SessionStoreState>,
) -> Result<PersistedSession, String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.load_session()
}

#[tauri::command]
pub fn save_session_volume(
    state: tauri::State<'_, SessionStoreState>,
    volume: f32,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.save_volume(volume)
}

#[tauri::command]
pub fn save_session_position(
    state: tauri::State<'_, SessionStoreState>,
    position_secs: u64,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.save_position(position_secs)
}

#[tauri::command]
pub fn save_session_playback_mode(
    state: tauri::State<'_, SessionStoreState>,
    shuffle: bool,
    repeat_mode: String,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.save_playback_mode(shuffle, &repeat_mode)
}

#[tauri::command]
pub fn clear_session(state: tauri::State<'_, SessionStoreState>) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.clear_session()
}
