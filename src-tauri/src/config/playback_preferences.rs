//! Playback preferences
//!
//! Stores user preferences for playback behavior (autoplay mode, etc.)

use log::info;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoplayMode {
    /// Continue playing within the source (album, playlist, etc.)
    #[serde(rename = "continue")]
    ContinueWithinSource,
    /// Play only the selected track, then stop
    #[serde(rename = "track_only")]
    PlayTrackOnly,
    /// Create infinite radio when queue ends (based on recent tracks)
    #[serde(rename = "infinite")]
    InfiniteRadio,
}

impl Default for AutoplayMode {
    fn default() -> Self {
        Self::ContinueWithinSource
    }
}

impl AutoplayMode {
    fn to_db_value(&self) -> &'static str {
        match self {
            AutoplayMode::ContinueWithinSource => "continue",
            AutoplayMode::PlayTrackOnly => "track_only",
            AutoplayMode::InfiniteRadio => "infinite",
        }
    }

    fn from_db_value(s: &str) -> Self {
        match s {
            "track_only" => AutoplayMode::PlayTrackOnly,
            "infinite" => AutoplayMode::InfiniteRadio,
            _ => AutoplayMode::ContinueWithinSource,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackPreferences {
    pub autoplay_mode: AutoplayMode,
    pub show_context_icon: bool,
}

impl Default for PlaybackPreferences {
    fn default() -> Self {
        Self {
            autoplay_mode: AutoplayMode::ContinueWithinSource,
            show_context_icon: false,
        }
    }
}

pub struct PlaybackPreferencesStore {
    conn: Connection,
}

impl PlaybackPreferencesStore {
    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open playback preferences database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| {
                format!(
                    "Failed to enable WAL for playback preferences database: {}",
                    e
                )
            })?;

        // Step 1: Create table with old schema (for new installs) or do nothing if exists
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS playback_preferences (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                autoplay_mode TEXT NOT NULL DEFAULT 'continue'
            );",
        )
        .map_err(|e| format!("Failed to create playback preferences table: {}", e))?;

        // Step 2: Check if show_context_icon column exists
        let column_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('playback_preferences') WHERE name='show_context_icon'",
                [],
                |row| {
                    let count: i32 = row.get(0)?;
                    Ok(count > 0)
                }
            )
            .unwrap_or(false);

        info!(
            "[PlaybackPrefs] Column show_context_icon exists: {}",
            column_exists
        );

        // Step 3: Add column if it doesn't exist (migration for existing users)
        if !column_exists {
            info!("[PlaybackPrefs] Migrating: adding show_context_icon column");
            conn.execute(
                "ALTER TABLE playback_preferences ADD COLUMN show_context_icon INTEGER NOT NULL DEFAULT 0",
                []
            ).map_err(|e| format!("Failed to add show_context_icon column: {}", e))?;
            info!("[PlaybackPrefs] Migration successful");
        }

        // Step 4: Insert default row if it doesn't exist
        conn.execute(
            "INSERT OR IGNORE INTO playback_preferences (id, autoplay_mode, show_context_icon)
            VALUES (1, 'continue', 0)",
            [],
        )
        .map_err(|e| format!("Failed to insert default preferences: {}", e))?;

        Ok(Self { conn })
    }

    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "playback_preferences.db")
    }

    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "playback_preferences.db")
    }

    pub fn get_preferences(&self) -> Result<PlaybackPreferences, String> {
        self.conn
            .query_row(
                "SELECT autoplay_mode, show_context_icon FROM playback_preferences WHERE id = 1",
                [],
                |row| {
                    let autoplay_str: String = row.get(0)?;
                    let show_icon: i32 = row.get(1)?;
                    Ok(PlaybackPreferences {
                        autoplay_mode: AutoplayMode::from_db_value(&autoplay_str),
                        show_context_icon: show_icon != 0,
                    })
                },
            )
            .map_err(|e| format!("Failed to get playback preferences: {}", e))
    }

    pub fn set_autoplay_mode(&self, mode: AutoplayMode) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE playback_preferences SET autoplay_mode = ?1 WHERE id = 1",
                params![mode.to_db_value()],
            )
            .map_err(|e| format!("Failed to set autoplay mode: {}", e))?;
        Ok(())
    }

    pub fn set_show_context_icon(&self, show: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE playback_preferences SET show_context_icon = ?1 WHERE id = 1",
                params![if show { 1 } else { 0 }],
            )
            .map_err(|e| format!("Failed to set show context icon: {}", e))?;
        Ok(())
    }

    /// Reset all playback preferences to their default values
    pub fn reset_all(&self) -> Result<PlaybackPreferences, String> {
        let defaults = PlaybackPreferences::default();
        self.conn
            .execute(
                "UPDATE playback_preferences SET autoplay_mode = ?1, show_context_icon = ?2 WHERE id = 1",
                params![defaults.autoplay_mode.to_db_value(), if defaults.show_context_icon { 1 } else { 0 }],
            )
            .map_err(|e| format!("Failed to reset playback preferences: {}", e))?;
        Ok(defaults)
    }
}

/// Global state wrapper for thread-safe access
pub struct PlaybackPreferencesState {
    pub store: Arc<Mutex<Option<PlaybackPreferencesStore>>>,
}

impl PlaybackPreferencesState {
    pub fn new() -> Result<Self, String> {
        let store = PlaybackPreferencesStore::new()?;
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
        let new_store = PlaybackPreferencesStore::new_at(base_dir)?;
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock playback preferences store".to_string())?;
        *guard = Some(new_store);
        Ok(())
    }

    pub fn teardown(&self) -> Result<(), String> {
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock playback preferences store".to_string())?;
        *guard = None;
        Ok(())
    }

    pub fn get_preferences(&self) -> Result<PlaybackPreferences, String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock playback preferences store".to_string())?;
        let store = guard.as_ref().ok_or("No active session - please log in")?;
        store.get_preferences()
    }

    pub fn set_autoplay_mode(&self, mode: AutoplayMode) -> Result<(), String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock playback preferences store".to_string())?;
        let store = guard.as_ref().ok_or("No active session - please log in")?;
        store.set_autoplay_mode(mode)
    }

    pub fn set_show_context_icon(&self, show: bool) -> Result<(), String> {
        let guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock playback preferences store".to_string())?;
        let store = guard.as_ref().ok_or("No active session - please log in")?;
        store.set_show_context_icon(show)
    }
}

// Tauri commands

#[tauri::command]
pub fn get_playback_preferences(
    state: tauri::State<PlaybackPreferencesState>,
) -> Result<PlaybackPreferences, String> {
    state.get_preferences()
}

#[tauri::command]
pub fn set_autoplay_mode(
    mode: String,
    state: tauri::State<PlaybackPreferencesState>,
) -> Result<(), String> {
    let autoplay_mode = match mode.as_str() {
        "continue" => AutoplayMode::ContinueWithinSource,
        "track_only" => AutoplayMode::PlayTrackOnly,
        "infinite" => AutoplayMode::InfiniteRadio,
        _ => return Err(format!("Invalid autoplay mode: {}", mode)),
    };
    state.set_autoplay_mode(autoplay_mode)
}

#[tauri::command]
pub fn set_show_context_icon(
    show: bool,
    state: tauri::State<PlaybackPreferencesState>,
) -> Result<(), String> {
    state.set_show_context_icon(show)
}
