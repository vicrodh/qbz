//! Artist blacklist service
//!
//! Provides O(1) artist blacklist checks using an in-memory HashSet
//! with SQLite persistence.

use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use super::models::{BlacklistSettings, BlacklistedArtist};

/// Artist blacklist service with O(1) lookup performance
pub struct BlacklistService {
    conn: Connection,
    /// In-memory set for O(1) lookups
    blacklisted_ids: RwLock<HashSet<u64>>,
    /// Feature flag - when false, is_blacklisted() always returns false
    enabled: AtomicBool,
}

impl BlacklistService {
    /// Create a new blacklist service, opening or creating the database
    pub fn new(db_path: &Path) -> Result<Self, String> {
        log::info!("[Blacklist] Opening database at: {}", db_path.display());

        let conn = Connection::open(db_path)
            .map_err(|e| format!("Failed to open blacklist database: {}", e))?;

        // Enable WAL mode for better concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to set WAL mode: {}", e))?;

        let service = Self {
            conn,
            blacklisted_ids: RwLock::new(HashSet::new()),
            enabled: AtomicBool::new(true),
        };

        service.init_schema()?;
        service.load_from_db()?;
        service.load_settings()?;

        Ok(service)
    }

    /// Initialize database schema
    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                r#"
                -- Artist blacklist entries
                CREATE TABLE IF NOT EXISTS artist_blacklist (
                    artist_id INTEGER PRIMARY KEY,
                    artist_name TEXT NOT NULL,
                    added_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                    notes TEXT
                );

                -- Index for name search in UI
                CREATE INDEX IF NOT EXISTS idx_artist_blacklist_name
                    ON artist_blacklist(artist_name COLLATE NOCASE);

                -- Settings table (single row)
                CREATE TABLE IF NOT EXISTS blacklist_settings (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    enabled INTEGER NOT NULL DEFAULT 1
                );

                -- Insert default settings if not present
                INSERT OR IGNORE INTO blacklist_settings (id, enabled) VALUES (1, 1);
                "#,
            )
            .map_err(|e| format!("Failed to initialize blacklist schema: {}", e))?;

        Ok(())
    }

    /// Load all blacklisted IDs from database into memory
    fn load_from_db(&self) -> Result<(), String> {
        let mut stmt = self
            .conn
            .prepare("SELECT artist_id FROM artist_blacklist")
            .map_err(|e| format!("Failed to prepare blacklist query: {}", e))?;

        let ids: Vec<u64> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Failed to query blacklist: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        let count = ids.len();
        let mut set = self
            .blacklisted_ids
            .write()
            .map_err(|_| "Failed to acquire write lock")?;
        *set = ids.into_iter().collect();

        log::info!(
            "[Blacklist] Loaded {} blacklisted artists into memory",
            count
        );
        Ok(())
    }

    /// Load enabled setting from database
    fn load_settings(&self) -> Result<(), String> {
        let enabled: bool = self
            .conn
            .query_row(
                "SELECT enabled FROM blacklist_settings WHERE id = 1",
                [],
                |row| {
                    let val: i32 = row.get(0)?;
                    Ok(val != 0)
                },
            )
            .map_err(|e| format!("Failed to load blacklist settings: {}", e))?;

        self.enabled.store(enabled, Ordering::Relaxed);
        log::info!("[Blacklist] Feature enabled: {}", enabled);
        Ok(())
    }

    /// Check if an artist is blacklisted - O(1) operation
    ///
    /// Returns false if the feature is disabled.
    #[inline]
    pub fn is_blacklisted(&self, artist_id: u64) -> bool {
        // Fast path: if feature is disabled, always return false
        if !self.enabled.load(Ordering::Relaxed) {
            return false;
        }

        // O(1) HashSet lookup
        self.blacklisted_ids
            .read()
            .map(|set| set.contains(&artist_id))
            .unwrap_or(false)
    }

    /// Add an artist to the blacklist
    pub fn add(
        &self,
        artist_id: u64,
        artist_name: &str,
        notes: Option<&str>,
    ) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO artist_blacklist (artist_id, artist_name, added_at, notes)
                 VALUES (?1, ?2, ?3, ?4)",
                params![artist_id as i64, artist_name, now, notes],
            )
            .map_err(|e| format!("Failed to add artist to blacklist: {}", e))?;

        // Update in-memory set
        if let Ok(mut set) = self.blacklisted_ids.write() {
            set.insert(artist_id);
        }

        log::info!(
            "[Blacklist] Added artist: {} (id={})",
            artist_name,
            artist_id
        );
        Ok(())
    }

    /// Remove an artist from the blacklist
    pub fn remove(&self, artist_id: u64) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM artist_blacklist WHERE artist_id = ?1",
                params![artist_id as i64],
            )
            .map_err(|e| format!("Failed to remove artist from blacklist: {}", e))?;

        // Update in-memory set
        if let Ok(mut set) = self.blacklisted_ids.write() {
            set.remove(&artist_id);
        }

        log::info!("[Blacklist] Removed artist id={}", artist_id);
        Ok(())
    }

    /// Get all blacklisted artists
    pub fn get_all(&self) -> Result<Vec<BlacklistedArtist>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT artist_id, artist_name, added_at, notes
                 FROM artist_blacklist
                 ORDER BY artist_name COLLATE NOCASE",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let artists = stmt
            .query_map([], |row| {
                Ok(BlacklistedArtist {
                    artist_id: row.get::<_, i64>(0)? as u64,
                    artist_name: row.get(1)?,
                    added_at: row.get(2)?,
                    notes: row.get(3)?,
                })
            })
            .map_err(|e| format!("Failed to query blacklist: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(artists)
    }

    /// Get count of blacklisted artists
    pub fn count(&self) -> usize {
        self.blacklisted_ids
            .read()
            .map(|set| set.len())
            .unwrap_or(0)
    }

    /// Set the enabled state
    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE blacklist_settings SET enabled = ?1 WHERE id = 1",
                params![if enabled { 1 } else { 0 }],
            )
            .map_err(|e| format!("Failed to update enabled setting: {}", e))?;

        self.enabled.store(enabled, Ordering::Relaxed);
        log::info!("[Blacklist] Feature enabled set to: {}", enabled);
        Ok(())
    }

    /// Check if the feature is enabled
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Get current settings
    pub fn get_settings(&self) -> BlacklistSettings {
        BlacklistSettings {
            enabled: self.is_enabled(),
        }
    }

    /// Clear all blacklisted artists
    pub fn clear_all(&self) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM artist_blacklist", [])
            .map_err(|e| format!("Failed to clear blacklist: {}", e))?;

        if let Ok(mut set) = self.blacklisted_ids.write() {
            set.clear();
        }

        log::info!("[Blacklist] Cleared all entries");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn create_test_service() -> BlacklistService {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_blacklist.db");
        BlacklistService::new(&db_path).unwrap()
    }

    #[test]
    fn test_add_and_check() {
        let service = create_test_service();

        assert!(!service.is_blacklisted(123));

        service.add(123, "Test Artist", None).unwrap();

        assert!(service.is_blacklisted(123));
        assert!(!service.is_blacklisted(456));
    }

    #[test]
    fn test_remove() {
        let service = create_test_service();

        service.add(123, "Test Artist", None).unwrap();
        assert!(service.is_blacklisted(123));

        service.remove(123).unwrap();
        assert!(!service.is_blacklisted(123));
    }

    #[test]
    fn test_enabled_toggle() {
        let service = create_test_service();

        service.add(123, "Test Artist", None).unwrap();

        // Enabled by default
        assert!(service.is_blacklisted(123));

        // Disable feature
        service.set_enabled(false).unwrap();
        assert!(!service.is_blacklisted(123));

        // Re-enable
        service.set_enabled(true).unwrap();
        assert!(service.is_blacklisted(123));
    }

    #[test]
    fn test_get_all() {
        let service = create_test_service();

        service.add(1, "Artist A", None).unwrap();
        service.add(2, "Artist B", Some("test note")).unwrap();

        let all = service.get_all().unwrap();
        assert_eq!(all.len(), 2);

        // Should be sorted by name
        assert_eq!(all[0].artist_name, "Artist A");
        assert_eq!(all[1].artist_name, "Artist B");
        assert_eq!(all[1].notes, Some("test note".to_string()));
    }
}
