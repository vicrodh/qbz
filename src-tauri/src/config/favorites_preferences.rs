use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoritesPreferences {
    pub custom_icon_path: Option<String>,
    pub custom_icon_preset: Option<String>,
    pub icon_background: Option<String>,
    pub tab_order: Vec<String>,
}

impl Default for FavoritesPreferences {
    fn default() -> Self {
        Self {
            custom_icon_path: None,
            custom_icon_preset: Some("heart".to_string()),
            icon_background: None,
            tab_order: vec![
                "tracks".to_string(),
                "albums".to_string(),
                "artists".to_string(),
                "playlists".to_string(),
            ],
        }
    }
}

pub struct FavoritesPreferencesStore {
    conn: Connection,
}

impl FavoritesPreferencesStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("favorites_preferences.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open favorites preferences database: {}", e))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS favorites_preferences (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                custom_icon_path TEXT,
                custom_icon_preset TEXT,
                tab_order TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| format!("Failed to create favorites preferences table: {}", e))?;

        // Migration: Add icon_background column if it doesn't exist
        let has_icon_background = conn
            .prepare("SELECT icon_background FROM favorites_preferences LIMIT 1")
            .is_ok();
        
        if !has_icon_background {
            conn.execute(
                "ALTER TABLE favorites_preferences ADD COLUMN icon_background TEXT",
                [],
            )
            .map_err(|e| format!("Failed to add icon_background column: {}", e))?;
        }

        Ok(Self { conn })
    }

    pub fn get_preferences(&self) -> Result<FavoritesPreferences, String> {
        let mut stmt = self.conn.prepare("SELECT custom_icon_path, custom_icon_preset, icon_background, tab_order FROM favorites_preferences WHERE id = 1")
            .map_err(|e| format!("Failed to prepare select: {}", e))?;
        
        let result = stmt.query_row([], |row| {
            let custom_icon_path: Option<String> = row.get(0)?;
            let custom_icon_preset: Option<String> = row.get(1)?;
            let icon_background: Option<String> = row.get(2)?;
            let tab_order_str: String = row.get(3)?;
            
            let tab_order: Vec<String> = serde_json::from_str(&tab_order_str).unwrap_or_else(|_| {
                vec![
                    "tracks".to_string(),
                    "albums".to_string(),
                    "artists".to_string(),
                    "playlists".to_string(),
                ]
            });

            Ok(FavoritesPreferences {
                custom_icon_path,
                custom_icon_preset,
                icon_background,
                tab_order,
            })
        });

        match result {
            Ok(prefs) => Ok(prefs),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(FavoritesPreferences::default()),
            Err(e) => Err(format!("Failed to query preferences: {}", e)),
        }
    }

    pub fn save_preferences(&self, prefs: &FavoritesPreferences) -> Result<(), String> {
        let tab_order_str = serde_json::to_string(&prefs.tab_order)
            .map_err(|e| format!("Failed to serialize tab_order: {}", e))?;
        
        self.conn.execute(
            "INSERT OR REPLACE INTO favorites_preferences (id, custom_icon_path, custom_icon_preset, icon_background, tab_order)
             VALUES (1, ?1, ?2, ?3, ?4)",
            params![prefs.custom_icon_path, prefs.custom_icon_preset, prefs.icon_background, tab_order_str],
        )
        .map_err(|e| format!("Failed to save preferences: {}", e))?;
        Ok(())
    }
}

pub struct FavoritesPreferencesState {
    pub store: Arc<Mutex<FavoritesPreferencesStore>>,
}

impl FavoritesPreferencesState {
    pub fn new() -> Result<Self, String> {
        let store = FavoritesPreferencesStore::new()?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
        })
    }
}

#[tauri::command]
pub fn get_favorites_preferences(
    state: tauri::State<FavoritesPreferencesState>,
) -> Result<FavoritesPreferences, String> {
    let store = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites preferences store".to_string())?;
    store.get_preferences()
}

#[tauri::command]
pub fn save_favorites_preferences(
    prefs: FavoritesPreferences,
    state: tauri::State<FavoritesPreferencesState>,
) -> Result<(), String> {
    let store = state
        .store
        .lock()
        .map_err(|_| "Failed to lock favorites preferences store".to_string())?;
    store.save_preferences(&prefs)
}

pub fn create_table(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS favorites_preferences (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            custom_icon_path TEXT,
            custom_icon_preset TEXT,
            tab_order TEXT NOT NULL
        )",
        [],
    )?;
    
    // Migration: Add icon_background column if it doesn't exist
    let has_icon_background = conn
        .prepare("SELECT icon_background FROM favorites_preferences LIMIT 1")
        .is_ok();
    
    if !has_icon_background {
        conn.execute(
            "ALTER TABLE favorites_preferences ADD COLUMN icon_background TEXT",
            [],
        )?;
    }
    
    Ok(())
}

pub fn load_preferences(conn: &Connection) -> Result<FavoritesPreferences> {
    let mut stmt = conn.prepare("SELECT custom_icon_path, custom_icon_preset, icon_background, tab_order FROM favorites_preferences WHERE id = 1")?;
    
    let result = stmt.query_row([], |row| {
        let custom_icon_path: Option<String> = row.get(0)?;
        let custom_icon_preset: Option<String> = row.get(1)?;
        let icon_background: Option<String> = row.get(2)?;
        let tab_order_str: String = row.get(3)?;
        
        let tab_order: Vec<String> = serde_json::from_str(&tab_order_str).unwrap_or_else(|_| {
            vec![
                "tracks".to_string(),
                "albums".to_string(),
                "artists".to_string(),
                "playlists".to_string(),
            ]
        });

        Ok(FavoritesPreferences {
            custom_icon_path,
            custom_icon_preset,
            icon_background,
            tab_order,
        })
    });

    match result {
        Ok(prefs) => Ok(prefs),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(FavoritesPreferences::default()),
        Err(e) => Err(e),
    }
}

pub fn save_preferences(conn: &Connection, prefs: &FavoritesPreferences) -> Result<()> {
    let tab_order_str = serde_json::to_string(&prefs.tab_order).unwrap();
    
    conn.execute(
        "INSERT OR REPLACE INTO favorites_preferences (id, custom_icon_path, custom_icon_preset, icon_background, tab_order)
         VALUES (1, ?1, ?2, ?3, ?4)",
        params![prefs.custom_icon_path, prefs.custom_icon_preset, prefs.icon_background, tab_order_str],
    )?;
    Ok(())
}

