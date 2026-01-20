//! Offline mode detection and settings
//!
//! Handles:
//! - Network connectivity detection
//! - Login state checking
//! - Manual offline mode toggle
//! - Offline settings persistence
//! - Pending playlist sync queue (playlists created offline)

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::net::TcpStream;

const TCP_PROBES: [&str; 4] = [
    "1.1.1.1:53",
    "8.8.8.8:53",
    "9.9.9.9:53",
    "208.67.222.222:53",
];

const HTTP_PROBES: [&str; 2] = [
    "https://www.google.com/generate_204",
    "https://1.1.1.1/cdn-cgi/trace",
];

const QOBUZ_URL: &str = "https://www.qobuz.com";

const TCP_TIMEOUT: Duration = Duration::from_secs(3);
const HTTP_TIMEOUT: Duration = Duration::from_secs(4);
const QOBUZ_TIMEOUT: Duration = Duration::from_secs(8);

const OFFLINE_FAILURE_THRESHOLD: u32 = 2;
const ONLINE_SUCCESS_THRESHOLD: u32 = 2;
const QOBUZ_CHECK_EVERY: u32 = 10;
const QOBUZ_CHECK_INTERVAL: Duration = Duration::from_secs(300);

/// Reason why the app is in offline mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OfflineReason {
    NoNetwork,
    NotLoggedIn,
    ManualOverride,
}

/// Current offline status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineStatus {
    pub is_offline: bool,
    pub reason: Option<OfflineReason>,
    pub manual_mode_enabled: bool,
}

/// Persistent offline settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OfflineSettings {
    pub manual_offline_mode: bool,
    pub show_partial_playlists: bool,
    /// Allow Chromecast while in manual offline mode
    pub allow_cast_while_offline: bool,
    /// Allow immediate scrobbling to Last.fm in manual offline mode
    pub allow_immediate_scrobbling: bool,
    /// Queue scrobbles for later submission when back online
    pub allow_accumulated_scrobbling: bool,
    /// Show network folder content in manual offline mode
    pub show_network_folders_in_manual_offline: bool,
}

/// A playlist created offline, pending sync to Qobuz
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPlaylist {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_public: bool,
    pub track_ids: Vec<u64>,
    pub created_at: i64,
    pub synced: bool,
    pub qobuz_playlist_id: Option<u64>,
}

/// A scrobble queued while offline, pending sync to Last.fm
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedScrobble {
    pub id: i64,
    pub artist: String,
    pub track: String,
    pub album: Option<String>,
    pub timestamp: i64,
    pub created_at: i64,
    pub sent: bool,
}

/// SQLite-backed storage for offline settings
pub struct OfflineStore {
    conn: Connection,
}

impl OfflineStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("offline_settings.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open offline settings database: {}", e))?;

        // Create base tables (with original schema for backward compatibility)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS offline_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                manual_offline_mode INTEGER NOT NULL DEFAULT 0,
                show_partial_playlists INTEGER NOT NULL DEFAULT 1
            );
            INSERT OR IGNORE INTO offline_settings (id, manual_offline_mode, show_partial_playlists)
            VALUES (1, 0, 1);

            CREATE TABLE IF NOT EXISTS pending_playlist_sync (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                description TEXT,
                is_public INTEGER NOT NULL DEFAULT 0,
                track_ids TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                synced INTEGER NOT NULL DEFAULT 0,
                qobuz_playlist_id INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_pending_playlist_synced ON pending_playlist_sync(synced);

            CREATE TABLE IF NOT EXISTS scrobble_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                artist TEXT NOT NULL,
                track TEXT NOT NULL,
                album TEXT,
                timestamp INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                sent INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_scrobble_queue_sent ON scrobble_queue(sent);"
        ).map_err(|e| format!("Failed to create offline settings table: {}", e))?;

        // Migration: Add new columns if they don't exist (run each separately to handle partial migrations)
        let migrations = [
            "ALTER TABLE offline_settings ADD COLUMN allow_cast_while_offline INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE offline_settings ADD COLUMN allow_immediate_scrobbling INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE offline_settings ADD COLUMN allow_accumulated_scrobbling INTEGER NOT NULL DEFAULT 1",
            "ALTER TABLE offline_settings ADD COLUMN show_network_folders_in_manual_offline INTEGER NOT NULL DEFAULT 0",
        ];

        for migration in migrations {
            // Ignore errors (column may already exist)
            let _ = conn.execute(migration, []);
        }

        Ok(Self { conn })
    }

    pub fn get_settings(&self) -> Result<OfflineSettings, String> {
        self.conn
            .query_row(
                "SELECT manual_offline_mode, show_partial_playlists, allow_cast_while_offline, allow_immediate_scrobbling, allow_accumulated_scrobbling, COALESCE(show_network_folders_in_manual_offline, 0) FROM offline_settings WHERE id = 1",
                [],
                |row| {
                    Ok(OfflineSettings {
                        manual_offline_mode: row.get::<_, i64>(0)? != 0,
                        show_partial_playlists: row.get::<_, i64>(1)? != 0,
                        allow_cast_while_offline: row.get::<_, i64>(2)? != 0,
                        allow_immediate_scrobbling: row.get::<_, i64>(3)? != 0,
                        allow_accumulated_scrobbling: row.get::<_, i64>(4)? != 0,
                        show_network_folders_in_manual_offline: row.get::<_, i64>(5)? != 0,
                    })
                },
            )
            .map_err(|e| format!("Failed to get offline settings: {}", e))
    }

    pub fn set_manual_offline_mode(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE offline_settings SET manual_offline_mode = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set manual offline mode: {}", e))?;
        Ok(())
    }

    pub fn set_show_partial_playlists(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE offline_settings SET show_partial_playlists = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set show partial playlists: {}", e))?;
        Ok(())
    }

    pub fn set_allow_cast_while_offline(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE offline_settings SET allow_cast_while_offline = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set allow cast while offline: {}", e))?;
        Ok(())
    }

    pub fn set_allow_immediate_scrobbling(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE offline_settings SET allow_immediate_scrobbling = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set allow immediate scrobbling: {}", e))?;
        Ok(())
    }

    pub fn set_allow_accumulated_scrobbling(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE offline_settings SET allow_accumulated_scrobbling = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set allow accumulated scrobbling: {}", e))?;
        Ok(())
    }

    pub fn set_show_network_folders_in_manual_offline(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE offline_settings SET show_network_folders_in_manual_offline = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set show network folders in manual offline: {}", e))?;
        Ok(())
    }

    // === Pending Playlist Sync Methods ===

    /// Create a new pending playlist (created while offline)
    pub fn create_pending_playlist(
        &self,
        name: &str,
        description: Option<&str>,
        is_public: bool,
        track_ids: &[u64],
    ) -> Result<i64, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let track_ids_json = serde_json::to_string(track_ids)
            .map_err(|e| format!("Failed to serialize track IDs: {}", e))?;

        self.conn
            .execute(
                "INSERT INTO pending_playlist_sync (name, description, is_public, track_ids, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![name, description, is_public as i64, track_ids_json, now],
            )
            .map_err(|e| format!("Failed to create pending playlist: {}", e))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get all pending (unsynced) playlists
    pub fn get_pending_playlists(&self) -> Result<Vec<PendingPlaylist>, String> {
        let mut stmt = self.conn
            .prepare(
                "SELECT id, name, description, is_public, track_ids, created_at, synced, qobuz_playlist_id
                 FROM pending_playlist_sync WHERE synced = 0 ORDER BY created_at ASC"
            )
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let playlists = stmt
            .query_map([], |row| {
                let track_ids_json: String = row.get(4)?;
                let track_ids: Vec<u64> = serde_json::from_str(&track_ids_json).unwrap_or_default();

                Ok(PendingPlaylist {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    is_public: row.get::<_, i64>(3)? != 0,
                    track_ids,
                    created_at: row.get(5)?,
                    synced: row.get::<_, i64>(6)? != 0,
                    qobuz_playlist_id: row.get::<_, Option<i64>>(7)?.map(|id| id as u64),
                })
            })
            .map_err(|e| format!("Failed to query pending playlists: {}", e))?;

        playlists
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect pending playlists: {}", e))
    }

    /// Mark a pending playlist as synced with its Qobuz ID
    pub fn mark_playlist_synced(&self, pending_id: i64, qobuz_playlist_id: u64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE pending_playlist_sync SET synced = 1, qobuz_playlist_id = ?1 WHERE id = ?2",
                params![qobuz_playlist_id as i64, pending_id],
            )
            .map_err(|e| format!("Failed to mark playlist as synced: {}", e))?;
        Ok(())
    }

    /// Delete a pending playlist (e.g., if user cancels before sync)
    pub fn delete_pending_playlist(&self, pending_id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM pending_playlist_sync WHERE id = ?1",
                params![pending_id],
            )
            .map_err(|e| format!("Failed to delete pending playlist: {}", e))?;
        Ok(())
    }

    /// Get count of pending playlists
    pub fn get_pending_playlist_count(&self) -> Result<u32, String> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM pending_playlist_sync WHERE synced = 0",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as u32)
            .map_err(|e| format!("Failed to count pending playlists: {}", e))
    }

    // === Scrobble Queue Methods ===

    /// Queue a scrobble for later submission to Last.fm
    pub fn queue_scrobble(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
        timestamp: i64,
    ) -> Result<i64, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn
            .execute(
                "INSERT INTO scrobble_queue (artist, track, album, timestamp, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![artist, track, album, timestamp, now],
            )
            .map_err(|e| format!("Failed to queue scrobble: {}", e))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get all unsent scrobbles (up to 50 for Last.fm batch limit)
    pub fn get_queued_scrobbles(&self, limit: u32) -> Result<Vec<QueuedScrobble>, String> {
        let mut stmt = self.conn
            .prepare(
                "SELECT id, artist, track, album, timestamp, created_at, sent
                 FROM scrobble_queue WHERE sent = 0 ORDER BY timestamp ASC LIMIT ?1"
            )
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let scrobbles = stmt
            .query_map(params![limit], |row| {
                Ok(QueuedScrobble {
                    id: row.get(0)?,
                    artist: row.get(1)?,
                    track: row.get(2)?,
                    album: row.get(3)?,
                    timestamp: row.get(4)?,
                    created_at: row.get(5)?,
                    sent: row.get::<_, i64>(6)? != 0,
                })
            })
            .map_err(|e| format!("Failed to query queued scrobbles: {}", e))?;

        scrobbles
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect queued scrobbles: {}", e))
    }

    /// Mark scrobbles as sent
    pub fn mark_scrobbles_sent(&self, ids: &[i64]) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }

        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "UPDATE scrobble_queue SET sent = 1 WHERE id IN ({})",
            placeholders.join(",")
        );

        let mut stmt = self.conn
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        stmt.execute(params.as_slice())
            .map_err(|e| format!("Failed to mark scrobbles as sent: {}", e))?;

        Ok(())
    }

    /// Delete old sent scrobbles (cleanup)
    pub fn cleanup_sent_scrobbles(&self, older_than_days: u32) -> Result<u32, String> {
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
            - (older_than_days as i64 * 24 * 60 * 60);

        let deleted = self.conn
            .execute(
                "DELETE FROM scrobble_queue WHERE sent = 1 AND created_at < ?1",
                params![cutoff],
            )
            .map_err(|e| format!("Failed to cleanup sent scrobbles: {}", e))?;

        Ok(deleted as u32)
    }

    /// Get count of queued (unsent) scrobbles
    pub fn get_queued_scrobble_count(&self) -> Result<u32, String> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM scrobble_queue WHERE sent = 0",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as u32)
            .map_err(|e| format!("Failed to count queued scrobbles: {}", e))
    }
}

/// Thread-safe wrapper for OfflineStore
pub struct OfflineState {
    pub store: Arc<Mutex<OfflineStore>>,
    network_state: Arc<Mutex<NetworkCheckState>>,
}

impl OfflineState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(OfflineStore::new()?)),
            network_state: Arc::new(Mutex::new(NetworkCheckState::default())),
        })
    }
}

#[derive(Debug, Default)]
struct NetworkCheckState {
    consecutive_failures: u32,
    consecutive_successes: u32,
    is_offline: bool,
    checks_since_qobuz: u32,
    last_qobuz_check: Option<Instant>,
}

impl NetworkCheckState {
    fn should_check_qobuz(&self) -> bool {
        if self.last_qobuz_check.is_none() {
            return true;
        }
        if self.checks_since_qobuz >= QOBUZ_CHECK_EVERY {
            return true;
        }
        if let Some(last_check) = self.last_qobuz_check {
            return last_check.elapsed() >= QOBUZ_CHECK_INTERVAL;
        }
        false
    }

    fn note_qobuz_skipped(&mut self) {
        self.checks_since_qobuz = self.checks_since_qobuz.saturating_add(1);
    }

    fn record_qobuz_check(&mut self) {
        self.last_qobuz_check = Some(Instant::now());
        self.checks_since_qobuz = 0;
    }

    fn update_offline_state(&mut self, tier1_ok: bool) -> bool {
        if tier1_ok {
            self.consecutive_successes = self.consecutive_successes.saturating_add(1);
            self.consecutive_failures = 0;

            if self.is_offline && self.consecutive_successes >= ONLINE_SUCCESS_THRESHOLD {
                self.is_offline = false;
                self.consecutive_successes = 0;
            }
        } else {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
            self.consecutive_successes = 0;

            if !self.is_offline && self.consecutive_failures >= OFFLINE_FAILURE_THRESHOLD {
                self.is_offline = true;
                self.consecutive_failures = 0;
            }
        }

        self.is_offline
    }
}

async fn check_tcp_connectivity() -> bool {
    for address in TCP_PROBES {
        match tokio::time::timeout(TCP_TIMEOUT, TcpStream::connect(address)).await {
            Ok(Ok(_)) => return true,
            Ok(Err(_)) => continue,
            Err(_) => continue,
        }
    }

    log::warn!("TCP connectivity check failed for all endpoints");
    false
}

async fn check_http_fallback() -> bool {
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build();

    let client = match client {
        Ok(c) => c,
        Err(_) => return false,
    };

    for url in HTTP_PROBES {
        match client.get(url).send().await {
            Ok(response) => {
                if response.status().is_success() || response.status().is_redirection() {
                    return true;
                }
            }
            Err(e) => {
                log::warn!("HTTP connectivity check failed for {}: {}", url, e);
            }
        }
    }

    log::warn!("HTTP fallback connectivity check failed");
    false
}

async fn check_tier1_connectivity() -> bool {
    if check_tcp_connectivity().await {
        return true;
    }
    check_http_fallback().await
}

async fn check_qobuz_connectivity() -> bool {
    let client = reqwest::Client::builder()
        .timeout(QOBUZ_TIMEOUT)
        .build();

    let client = match client {
        Ok(c) => c,
        Err(_) => return false,
    };

    match client.head(QOBUZ_URL).send().await {
        Ok(response) => response.status().is_success() || response.status().is_redirection(),
        Err(e) => {
            log::warn!("Qobuz connectivity check failed: {}", e);
            false
        }
    }
}

// Tauri commands
pub mod commands {
    use super::*;
    use tauri::State;

    /// Get current offline status
    /// Note: Login state is not checked here because the frontend already handles
    /// showing LoginView when not logged in. This prevents race conditions on startup.
    #[tauri::command]
    pub async fn get_offline_status(
        offline_state: State<'_, OfflineState>,
    ) -> Result<OfflineStatus, String> {
        let settings = {
            let store = offline_state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
            store.get_settings()?
        };

        // If manual offline mode is enabled, return immediately
        if settings.manual_offline_mode {
            return Ok(OfflineStatus {
                is_offline: true,
                reason: Some(OfflineReason::ManualOverride),
                manual_mode_enabled: true,
            });
        }

        let tier1_ok = check_tier1_connectivity().await;

        let should_check_qobuz = if tier1_ok {
            let mut state = offline_state
                .network_state
                .lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            let should_check = state.should_check_qobuz();
            if !should_check {
                state.note_qobuz_skipped();
            }
            should_check
        } else {
            false
        };

        let qobuz_ok = if should_check_qobuz {
            Some(check_qobuz_connectivity().await)
        } else {
            None
        };

        if tier1_ok {
            if let Some(false) = qobuz_ok {
                log::warn!("Qobuz check failed while internet appears reachable");
            }
        }

        let is_offline = {
            let mut state = offline_state
                .network_state
                .lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            let offline = state.update_offline_state(tier1_ok);
            if qobuz_ok.is_some() {
                state.record_qobuz_check();
            }
            offline
        };

        if is_offline {
            return Ok(OfflineStatus {
                is_offline: true,
                reason: Some(OfflineReason::NoNetwork),
                manual_mode_enabled: false,
            });
        }

        Ok(OfflineStatus {
            is_offline: false,
            reason: None,
            manual_mode_enabled: false,
        })
    }

    /// Get offline settings
    #[tauri::command]
    pub fn get_offline_settings(
        state: State<'_, OfflineState>,
    ) -> Result<OfflineSettings, String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.get_settings()
    }

    /// Enable or disable manual offline mode
    #[tauri::command]
    pub async fn set_manual_offline(
        enabled: bool,
        state: State<'_, OfflineState>,
        app_handle: AppHandle,
    ) -> Result<OfflineStatus, String> {
        {
            let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
            store.set_manual_offline_mode(enabled)?;
        }

        // Get updated status
        let status = get_offline_status(state).await?;

        // Emit event to frontend
        let _ = app_handle.emit("offline-status-changed", &status);

        Ok(status)
    }

    /// Set whether to show playlists with partial local content in offline mode
    #[tauri::command]
    pub fn set_show_partial_playlists(
        enabled: bool,
        state: State<'_, OfflineState>,
    ) -> Result<(), String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.set_show_partial_playlists(enabled)
    }

    /// Set whether to allow Chromecast while in manual offline mode
    #[tauri::command]
    pub fn set_allow_cast_while_offline(
        enabled: bool,
        state: State<'_, OfflineState>,
    ) -> Result<(), String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.set_allow_cast_while_offline(enabled)
    }

    /// Set whether to allow immediate scrobbling to Last.fm in manual offline mode
    #[tauri::command]
    pub fn set_allow_immediate_scrobbling(
        enabled: bool,
        state: State<'_, OfflineState>,
    ) -> Result<(), String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.set_allow_immediate_scrobbling(enabled)
    }

    /// Set whether to queue scrobbles for later submission when back online
    #[tauri::command]
    pub fn set_allow_accumulated_scrobbling(
        enabled: bool,
        state: State<'_, OfflineState>,
    ) -> Result<(), String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.set_allow_accumulated_scrobbling(enabled)
    }

    /// Set whether to show network folder content in manual offline mode
    #[tauri::command]
    pub fn set_show_network_folders_in_manual_offline(
        enabled: bool,
        state: State<'_, OfflineState>,
    ) -> Result<(), String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.set_show_network_folders_in_manual_offline(enabled)
    }

    /// Check if network connectivity is available
    #[tauri::command]
    pub async fn check_network() -> bool {
        super::check_tier1_connectivity().await
    }

    // === Pending Playlist Sync Commands ===

    /// Create a playlist while offline (queued for sync)
    #[tauri::command]
    pub fn create_pending_playlist(
        name: String,
        description: Option<String>,
        is_public: bool,
        track_ids: Vec<u64>,
        state: State<'_, OfflineState>,
    ) -> Result<i64, String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.create_pending_playlist(&name, description.as_deref(), is_public, &track_ids)
    }

    /// Get all playlists pending sync
    #[tauri::command]
    pub fn get_pending_playlists(
        state: State<'_, OfflineState>,
    ) -> Result<Vec<PendingPlaylist>, String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.get_pending_playlists()
    }

    /// Get count of pending playlists
    #[tauri::command]
    pub fn get_pending_playlist_count(
        state: State<'_, OfflineState>,
    ) -> Result<u32, String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.get_pending_playlist_count()
    }

    /// Mark a pending playlist as synced after successful Qobuz creation
    #[tauri::command]
    pub fn mark_pending_playlist_synced(
        pending_id: i64,
        qobuz_playlist_id: u64,
        state: State<'_, OfflineState>,
    ) -> Result<(), String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.mark_playlist_synced(pending_id, qobuz_playlist_id)
    }

    /// Delete a pending playlist
    #[tauri::command]
    pub fn delete_pending_playlist(
        pending_id: i64,
        state: State<'_, OfflineState>,
    ) -> Result<(), String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.delete_pending_playlist(pending_id)
    }

    // === Scrobble Queue Commands ===

    /// Queue a scrobble for later submission (when offline)
    #[tauri::command]
    pub fn queue_scrobble(
        artist: String,
        track: String,
        album: Option<String>,
        timestamp: i64,
        state: State<'_, OfflineState>,
    ) -> Result<i64, String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.queue_scrobble(&artist, &track, album.as_deref(), timestamp)
    }

    /// Get queued scrobbles (up to limit, default 50 for Last.fm batch)
    #[tauri::command]
    pub fn get_queued_scrobbles(
        limit: Option<u32>,
        state: State<'_, OfflineState>,
    ) -> Result<Vec<QueuedScrobble>, String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.get_queued_scrobbles(limit.unwrap_or(50))
    }

    /// Mark scrobbles as sent after successful Last.fm submission
    #[tauri::command]
    pub fn mark_scrobbles_sent(
        ids: Vec<i64>,
        state: State<'_, OfflineState>,
    ) -> Result<(), String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.mark_scrobbles_sent(&ids)
    }

    /// Get count of queued (unsent) scrobbles
    #[tauri::command]
    pub fn get_queued_scrobble_count(
        state: State<'_, OfflineState>,
    ) -> Result<u32, String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.get_queued_scrobble_count()
    }

    /// Cleanup old sent scrobbles
    #[tauri::command]
    pub fn cleanup_sent_scrobbles(
        older_than_days: Option<u32>,
        state: State<'_, OfflineState>,
    ) -> Result<u32, String> {
        let store = state.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        store.cleanup_sent_scrobbles(older_than_days.unwrap_or(7))
    }
}
