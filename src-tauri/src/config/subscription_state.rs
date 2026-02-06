//! Subscription validity tracking (for offline download compliance)
//!
//! Tracks when a user was first observed without a valid subscription. If the
//! invalid state persists for more than a grace period, offline downloads are
//! purged.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

const GRACE_PERIOD_SECS: i64 = 3 * 24 * 60 * 60; // 3 days

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionState {
    pub invalid_since: Option<i64>,
    pub last_invalid_at: Option<i64>,
    pub last_valid_at: Option<i64>,
    pub last_checked_at: Option<i64>,
    pub downloads_purged_at: Option<i64>,
}

impl Default for SubscriptionState {
    fn default() -> Self {
        Self {
            invalid_since: None,
            last_invalid_at: None,
            last_valid_at: None,
            last_checked_at: None,
            downloads_purged_at: None,
        }
    }
}

pub struct SubscriptionStateStore {
    conn: Connection,
}

impl SubscriptionStateStore {
    fn open_at(dir: &Path, db_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = dir.join(db_name);
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open subscription state database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS subscription_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                invalid_since INTEGER,
                last_invalid_at INTEGER,
                last_valid_at INTEGER,
                last_checked_at INTEGER,
                downloads_purged_at INTEGER
            );
            INSERT OR IGNORE INTO subscription_state (id) VALUES (1);"
        ).map_err(|e| format!("Failed to create subscription state table: {}", e))?;

        Ok(Self { conn })
    }

    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");
        Self::open_at(&data_dir, "subscription_state.db")
    }

    pub fn new_at(base_dir: &Path) -> Result<Self, String> {
        Self::open_at(base_dir, "subscription_state.db")
    }

    pub fn get_state(&self) -> Result<SubscriptionState, String> {
        self.conn
            .query_row(
                "SELECT invalid_since, last_invalid_at, last_valid_at, last_checked_at, downloads_purged_at
                 FROM subscription_state WHERE id = 1",
                [],
                |row| {
                    Ok(SubscriptionState {
                        invalid_since: row.get(0)?,
                        last_invalid_at: row.get(1)?,
                        last_valid_at: row.get(2)?,
                        last_checked_at: row.get(3)?,
                        downloads_purged_at: row.get(4)?,
                    })
                },
            )
            .map_err(|e| format!("Failed to read subscription state: {}", e))
    }

    pub fn mark_valid(&self, now: i64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE subscription_state
                 SET invalid_since = NULL,
                     last_valid_at = ?1,
                     last_checked_at = ?1
                 WHERE id = 1",
                params![now],
            )
            .map_err(|e| format!("Failed to update subscription state: {}", e))?;
        Ok(())
    }

    pub fn mark_invalid(&self, now: i64) -> Result<(), String> {
        // Only set invalid_since if it's not already set (first observation).
        self.conn
            .execute(
                "UPDATE subscription_state
                 SET invalid_since = COALESCE(invalid_since, ?1),
                     last_invalid_at = ?1,
                     last_checked_at = ?1
                 WHERE id = 1",
                params![now],
            )
            .map_err(|e| format!("Failed to update subscription state: {}", e))?;
        Ok(())
    }

    pub fn mark_offline_cache_purged(&self, now: i64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE subscription_state SET downloads_purged_at = ?1 WHERE id = 1",
                params![now],
            )
            .map_err(|e| format!("Failed to update purge timestamp: {}", e))?;
        Ok(())
    }

    pub fn should_purge_offline_cache(&self, now: i64) -> Result<bool, String> {
        let state = self.get_state()?;
        let Some(invalid_since) = state.invalid_since else { return Ok(false); };
        if now - invalid_since < GRACE_PERIOD_SECS {
            return Ok(false);
        }
        // Purge at most once per invalid period.
        if let Some(purged_at) = state.downloads_purged_at {
            if purged_at >= invalid_since {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

pub type SubscriptionStateState = Arc<Mutex<Option<SubscriptionStateStore>>>;

pub fn create_subscription_state() -> Result<SubscriptionStateState, String> {
    let store = SubscriptionStateStore::new()?;
    Ok(Arc::new(Mutex::new(Some(store))))
}

pub fn create_empty_subscription_state() -> SubscriptionStateState {
    Arc::new(Mutex::new(None))
}
