use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryPreferences {
    pub tab_order: Vec<String>,
    pub hidden_tabs: Vec<String>,
}

impl Default for LibraryPreferences {
    fn default() -> Self {
        Self {
            // Default order matches the current LocalLibraryView tab list,
            // with the renamed 'folders' tab in place of the old 'albums'
            // tab. Phase 3 adds 'albums' (the new metadata-grouped view)
            // to this default.
            tab_order: vec![
                "tracks".to_string(),
                "folders".to_string(),
                "albums".to_string(),
                "artists".to_string(),
            ],
            hidden_tabs: vec![],
        }
    }
}

pub struct LibraryPreferencesStore {
    conn: Connection,
}

impl LibraryPreferencesStore {
    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open library preferences database: {}", e))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to set WAL mode on library preferences: {}", e))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS library_preferences (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                tab_order TEXT NOT NULL,
                hidden_tabs TEXT NOT NULL DEFAULT '[]'
            )",
            [],
        )
        .map_err(|e| format!("Failed to create library preferences table: {}", e))?;

        Ok(Self { conn })
    }

    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "library_preferences.db")
    }

    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "library_preferences.db")
    }

    pub fn get_preferences(&self) -> Result<LibraryPreferences, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT tab_order, hidden_tabs FROM library_preferences WHERE id = 1")
            .map_err(|e| format!("Failed to prepare select: {}", e))?;

        let result = stmt.query_row([], |row| {
            let tab_order_str: String = row.get(0)?;
            let hidden_tabs_str: String = row.get(1)?;

            let tab_order: Vec<String> = serde_json::from_str(&tab_order_str)
                .unwrap_or_else(|_| LibraryPreferences::default().tab_order);
            let hidden_tabs: Vec<String> =
                serde_json::from_str(&hidden_tabs_str).unwrap_or_default();

            Ok(LibraryPreferences {
                tab_order,
                hidden_tabs,
            })
        });

        match result {
            Ok(prefs) => Ok(prefs),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(LibraryPreferences::default()),
            Err(e) => Err(format!("Failed to query library preferences: {}", e)),
        }
    }

    pub fn save_preferences(
        &self,
        prefs: LibraryPreferences,
    ) -> Result<LibraryPreferences, String> {
        let tab_order_str = serde_json::to_string(&prefs.tab_order)
            .map_err(|e| format!("Failed to serialize tab_order: {}", e))?;
        let hidden_tabs_str = serde_json::to_string(&prefs.hidden_tabs)
            .map_err(|e| format!("Failed to serialize hidden_tabs: {}", e))?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO library_preferences (id, tab_order, hidden_tabs)
                 VALUES (1, ?1, ?2)",
                params![tab_order_str, hidden_tabs_str],
            )
            .map_err(|e| format!("Failed to save library preferences: {}", e))?;
        Ok(prefs)
    }
}

pub struct LibraryPreferencesState {
    pub store: Arc<Mutex<Option<LibraryPreferencesStore>>>,
}

impl LibraryPreferencesState {
    pub fn new() -> Result<Self, String> {
        let store = LibraryPreferencesStore::new()?;
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
        let new_store = LibraryPreferencesStore::new_at(base_dir)?;
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock library preferences store".to_string())?;
        *guard = Some(new_store);
        Ok(())
    }

    pub fn teardown(&self) -> Result<(), String> {
        let mut guard = self
            .store
            .lock()
            .map_err(|_| "Failed to lock library preferences store".to_string())?;
        *guard = None;
        Ok(())
    }
}

#[tauri::command]
pub fn get_library_preferences(
    state: tauri::State<LibraryPreferencesState>,
) -> Result<LibraryPreferences, String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock library preferences store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_preferences()
}

#[tauri::command]
pub fn save_library_preferences(
    prefs: LibraryPreferences,
    state: tauri::State<LibraryPreferencesState>,
) -> Result<LibraryPreferences, String> {
    let guard = state
        .store
        .lock()
        .map_err(|_| "Failed to lock library preferences store".to_string())?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.save_preferences(prefs)
}
