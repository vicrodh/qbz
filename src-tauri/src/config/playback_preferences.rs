//! Playback preferences
//!
//! Stores user preferences for playback behavior (autoplay mode, etc.)

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use log::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoplayMode {
    /// Continue playing within the source (album, playlist, etc.)
    #[serde(rename = "continue")]
    ContinueWithinSource,
    /// Play only the selected track, then stop
    #[serde(rename = "track_only")]
    PlayTrackOnly,
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
        }
    }

    fn from_db_value(s: &str) -> Self {
        match s {
            "track_only" => AutoplayMode::PlayTrackOnly,
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
            show_context_icon: true,
        }
    }
}

pub struct PlaybackPreferencesStore {
    conn: Connection,
}

impl PlaybackPreferencesStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("playback_preferences.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open playback preferences database: {}", e))?;

        // Step 1: Create table with old schema (for new installs) or do nothing if exists
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS playback_preferences (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                autoplay_mode TEXT NOT NULL DEFAULT 'continue'
            );"
        ).map_err(|e| format!("Failed to create playback preferences table: {}", e))?;

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

        info!("[PlaybackPrefs] Column show_context_icon exists: {}", column_exists);

        // Step 3: Add column if it doesn't exist (migration for existing users)
        if !column_exists {
            info!("[PlaybackPrefs] Migrating: adding show_context_icon column");
            conn.execute(
                "ALTER TABLE playback_preferences ADD COLUMN show_context_icon INTEGER NOT NULL DEFAULT 1",
                []
            ).map_err(|e| format!("Failed to add show_context_icon column: {}", e))?;
            info!("[PlaybackPrefs] Migration successful");
        }

        // Step 4: Insert default row if it doesn't exist
        conn.execute(
            "INSERT OR IGNORE INTO playback_preferences (id, autoplay_mode, show_context_icon)
            VALUES (1, 'continue', 1)",
            []
        ).map_err(|e| format!("Failed to insert default preferences: {}", e))?;

        Ok(Self { conn })
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
}

/// Global state wrapper for thread-safe access
pub struct PlaybackPreferencesState {
    store: Arc<Mutex<PlaybackPreferencesStore>>,
}

impl PlaybackPreferencesState {
    pub fn new() -> Result<Self, String> {
        let store = PlaybackPreferencesStore::new()?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
        })
    }

    pub fn get_preferences(&self) -> Result<PlaybackPreferences, String> {
        self.store.lock().unwrap().get_preferences()
    }

    pub fn set_autoplay_mode(&self, mode: AutoplayMode) -> Result<(), String> {
        self.store.lock().unwrap().set_autoplay_mode(mode)
    }

    pub fn set_show_context_icon(&self, show: bool) -> Result<(), String> {
        self.store.lock().unwrap().set_show_context_icon(show)
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
