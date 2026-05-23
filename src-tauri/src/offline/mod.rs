//! Offline mode detection and settings
//!
//! Handles:
//! - Network connectivity detection
//! - Login state checking
//! - Manual offline mode toggle
//! - Offline settings persistence
//! - Pending playlist sync queue (playlists created offline)

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    pub local_track_ids: Vec<i64>, // DEPRECATED: Use local_track_paths instead (kept for migration)
    pub local_track_paths: Vec<String>, // File paths - stable across re-scans
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
        Self::open_at(&db_path)
    }

    pub fn new_at(base_dir: &std::path::Path) -> Result<Self, String> {
        std::fs::create_dir_all(base_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;
        let db_path = base_dir.join("offline_settings.db");
        Self::open_at(&db_path)
    }

    fn open_at(db_path: &std::path::Path) -> Result<Self, String> {
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open offline settings database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to enable WAL for offline settings database: {}", e))?;

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
            CREATE INDEX IF NOT EXISTS idx_scrobble_queue_sent ON scrobble_queue(sent);",
        )
        .map_err(|e| format!("Failed to create offline settings table: {}", e))?;

        // Migration: Add new columns if they don't exist (run each separately to handle partial migrations)
        let migrations = [
            "ALTER TABLE offline_settings ADD COLUMN allow_cast_while_offline INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE offline_settings ADD COLUMN allow_immediate_scrobbling INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE offline_settings ADD COLUMN allow_accumulated_scrobbling INTEGER NOT NULL DEFAULT 1",
            "ALTER TABLE offline_settings ADD COLUMN show_network_folders_in_manual_offline INTEGER NOT NULL DEFAULT 0",
            // Snapshot of audio_settings.stream_first_track taken when manual
            // offline mode is enabled; used to restore the user's preference
            // when they exit offline mode. NULL means "no snapshot active".
            // See issue #279.
            "ALTER TABLE offline_settings ADD COLUMN pre_offline_stream_first_track INTEGER",
            "ALTER TABLE pending_playlist_sync ADD COLUMN local_track_ids TEXT",
            "ALTER TABLE pending_playlist_sync ADD COLUMN local_track_paths TEXT",
            // Persisted offline cache size limit in bytes.
            // NULL means "no explicit value set" — application layer falls
            // back to the 5 GB default (see OfflineCacheState seed at session
            // activation). Once the user sets a value through the UI, it is
            // persisted as a non-NULL integer.
            "ALTER TABLE offline_settings ADD COLUMN cache_limit_bytes INTEGER",
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

    /// Read the saved stream-first-track preference stashed when the user
    /// entered offline mode. Returns None when no snapshot is active.
    pub fn get_pre_offline_stream_first_track(&self) -> Result<Option<bool>, String> {
        self.conn
            .query_row(
                "SELECT pre_offline_stream_first_track FROM offline_settings WHERE id = 1",
                [],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map(|opt| opt.map(|v| v != 0))
            .map_err(|e| format!("Failed to read pre_offline_stream_first_track: {}", e))
    }

    /// Store or clear the pre-offline snapshot. Pass `Some(value)` when
    /// entering offline mode, `None` when exiting.
    pub fn set_pre_offline_stream_first_track(
        &self,
        value: Option<bool>,
    ) -> Result<(), String> {
        let param: Option<i64> = value.map(|v| v as i64);
        self.conn
            .execute(
                "UPDATE offline_settings SET pre_offline_stream_first_track = ?1 WHERE id = 1",
                params![param],
            )
            .map_err(|e| format!("Failed to set pre_offline_stream_first_track: {}", e))?;
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

    /// Read the persisted offline cache size limit in bytes.
    ///
    /// Returns:
    /// - `Some(bytes)` when a value has been persisted via the UI.
    /// - `None` when the column is NULL (legacy installs, never written).
    ///   The application layer interprets this as "fall back to default".
    pub fn get_cache_limit_bytes(&self) -> Result<Option<u64>, String> {
        self.conn
            .query_row(
                "SELECT cache_limit_bytes FROM offline_settings WHERE id = 1",
                [],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map(|opt| opt.map(|v| v as u64))
            .map_err(|e| format!("Failed to read cache_limit_bytes: {}", e))
    }

    /// Persist the offline cache size limit. `None` clears the column to NULL
    /// (no value set); `Some(bytes)` writes the value verbatim.
    pub fn set_cache_limit_bytes(&self, limit: Option<u64>) -> Result<(), String> {
        let param: Option<i64> = limit.map(|v| v as i64);
        self.conn
            .execute(
                "UPDATE offline_settings SET cache_limit_bytes = ?1 WHERE id = 1",
                params![param],
            )
            .map_err(|e| format!("Failed to set cache_limit_bytes: {}", e))?;
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
        local_track_paths: &[String],
    ) -> Result<i64, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let track_ids_json = serde_json::to_string(track_ids)
            .map_err(|e| format!("Failed to serialize track IDs: {}", e))?;

        let local_track_paths_json = serde_json::to_string(local_track_paths)
            .map_err(|e| format!("Failed to serialize local track paths: {}", e))?;

        // Keep local_track_ids as empty array for backward compatibility
        let empty_ids_json = "[]";

        self.conn
            .execute(
                "INSERT INTO pending_playlist_sync (name, description, is_public, track_ids, local_track_ids, local_track_paths, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![name, description, is_public as i64, track_ids_json, empty_ids_json, local_track_paths_json, now],
            )
            .map_err(|e| format!("Failed to create pending playlist: {}", e))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get all pending (unsynced) playlists
    pub fn get_pending_playlists(&self) -> Result<Vec<PendingPlaylist>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, description, is_public, track_ids,
                        COALESCE(local_track_ids, '[]'),
                        COALESCE(local_track_paths, '[]'),
                        created_at, synced, qobuz_playlist_id
                 FROM pending_playlist_sync WHERE synced = 0 ORDER BY created_at ASC",
            )
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let playlists = stmt
            .query_map([], |row| {
                let track_ids_json: String = row.get(4)?;
                let track_ids: Vec<u64> = serde_json::from_str(&track_ids_json).unwrap_or_default();

                let local_track_ids_json: String = row.get(5)?;
                let local_track_ids: Vec<i64> =
                    serde_json::from_str(&local_track_ids_json).unwrap_or_default();

                let local_track_paths_json: String = row.get(6)?;
                let local_track_paths: Vec<String> =
                    serde_json::from_str(&local_track_paths_json).unwrap_or_default();

                Ok(PendingPlaylist {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    is_public: row.get::<_, i64>(3)? != 0,
                    track_ids,
                    local_track_ids, // Kept for migration
                    local_track_paths,
                    created_at: row.get(7)?,
                    synced: row.get::<_, i64>(8)? != 0,
                    qobuz_playlist_id: row.get::<_, Option<i64>>(9)?.map(|id| id as u64),
                })
            })
            .map_err(|e| format!("Failed to query pending playlists: {}", e))?;

        playlists
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect pending playlists: {}", e))
    }

    /// Update the Qobuz playlist ID without marking as synced (for partial sync recovery)
    pub fn update_qobuz_playlist_id(
        &self,
        pending_id: i64,
        qobuz_playlist_id: u64,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE pending_playlist_sync SET qobuz_playlist_id = ?1 WHERE id = ?2",
                params![qobuz_playlist_id as i64, pending_id],
            )
            .map_err(|e| format!("Failed to update Qobuz playlist ID: {}", e))?;
        Ok(())
    }

    /// Mark a pending playlist as synced with its Qobuz ID
    pub fn mark_playlist_synced(
        &self,
        pending_id: i64,
        qobuz_playlist_id: u64,
    ) -> Result<(), String> {
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

    /// Add tracks to a pending playlist
    pub fn add_tracks_to_pending_playlist(
        &self,
        pending_id: i64,
        qobuz_track_ids: &[u64],
        local_track_paths: &[String],
    ) -> Result<(), String> {
        // Get current tracks
        let mut stmt = self.conn
            .prepare("SELECT track_ids, COALESCE(local_track_paths, '[]') FROM pending_playlist_sync WHERE id = ?1")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let (current_qobuz_json, current_local_paths_json): (String, String) = stmt
            .query_row(params![pending_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| format!("Failed to get pending playlist: {}", e))?;

        let mut current_qobuz: Vec<u64> =
            serde_json::from_str(&current_qobuz_json).unwrap_or_default();
        let mut current_local_paths: Vec<String> =
            serde_json::from_str(&current_local_paths_json).unwrap_or_default();

        // Append new tracks
        current_qobuz.extend_from_slice(qobuz_track_ids);
        current_local_paths.extend_from_slice(local_track_paths);

        // Serialize back
        let qobuz_json = serde_json::to_string(&current_qobuz)
            .map_err(|e| format!("Failed to serialize Qobuz tracks: {}", e))?;
        let local_paths_json = serde_json::to_string(&current_local_paths)
            .map_err(|e| format!("Failed to serialize local track paths: {}", e))?;

        // Update database
        self.conn
            .execute(
                "UPDATE pending_playlist_sync SET track_ids = ?1, local_track_paths = ?2 WHERE id = ?3",
                params![qobuz_json, local_paths_json, pending_id],
            )
            .map_err(|e| format!("Failed to update pending playlist: {}", e))?;

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
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, artist, track, album, timestamp, created_at, sent
                 FROM scrobble_queue WHERE sent = 0 ORDER BY timestamp ASC LIMIT ?1",
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

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let params: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
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

        let deleted = self
            .conn
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
    pub store: Arc<Mutex<Option<OfflineStore>>>,
}

impl OfflineState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(Some(OfflineStore::new()?))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }

    pub fn init_at(&self, base_dir: &std::path::Path) -> Result<(), String> {
        let new_store = OfflineStore::new_at(base_dir)?;
        let mut guard = self
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        *guard = Some(new_store);
        Ok(())
    }

    pub fn teardown(&self) {
        if let Ok(mut guard) = self.store.lock() {
            *guard = None;
        }
    }
}

/// Consecutive failed connectivity polls. The offline flag is only raised
/// once this reaches `OFFLINE_FAILURE_THRESHOLD`, so a single transient blip
/// (Wi-Fi micro-drop, DNS hiccup, or a saturated link losing the probe race
/// during a big Hi-Res download) can no longer flip the app offline. See
/// issue #467.
static CONSECUTIVE_FAILURES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Number of consecutive failed polls (~30 s apart) required before the app
/// is declared offline. 2 ≈ 60 s of confirmed loss — lax enough to ride out
/// blips, still reacts to a real outage within about a minute.
pub const OFFLINE_FAILURE_THRESHOLD: u32 = 2;

/// Record a connectivity-poll outcome and return the running consecutive
/// failure count. Any success resets the counter to 0.
pub fn record_connectivity_result(success: bool) -> u32 {
    use std::sync::atomic::Ordering;
    if success {
        CONSECUTIVE_FAILURES.store(0, Ordering::Relaxed);
        0
    } else {
        CONSECUTIVE_FAILURES.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Reset the consecutive-failure counter. Called on a strong positive signal
/// (active streaming liveness, manual-mode change) so the next real loss
/// starts counting from zero.
pub fn reset_connectivity_failures() {
    CONSECUTIVE_FAILURES.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Lightweight "generate_204"-style connectivity probes. A success is a
/// near-empty 204/200 response, so the probe barely competes for bandwidth
/// even while a large Hi-Res download is saturating the link — the exact
/// condition (long Dream Theater / Yes tracks) that timed the old
/// `HEAD https://1.1.1.1` probe out and produced a false offline (issue #467).
const PROBE_ENDPOINTS: &[&str] = &[
    "https://connectivitycheck.gstatic.com/generate_204",
    "https://www.gstatic.com/generate_204",
    "https://cloudflare.com/cdn-cgi/trace",
];

/// Check basic internet connectivity with a cheap, concurrent probe.
///
/// Races the lightweight endpoints above and returns `true` as soon as any
/// one succeeds. This is intentionally *not* a Qobuz-service check: a slow or
/// rate-limited Qobuz must never read as "offline" (issue #467). Actual Qobuz
/// API failures are handled with retries/backoff in the fetch path instead.
pub async fn check_network_connectivity() -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    let mut set = tokio::task::JoinSet::new();
    for endpoint in PROBE_ENDPOINTS {
        let client = client.clone();
        let endpoint = (*endpoint).to_string();
        set.spawn(async move {
            match client.get(&endpoint).send().await {
                Ok(response) => {
                    response.status().is_success() || response.status().is_redirection()
                }
                Err(_) => false,
            }
        });
    }

    while let Some(joined) = set.join_next().await {
        if matches!(joined, Ok(true)) {
            set.abort_all();
            return true;
        }
    }

    log::info!("[offline] Connectivity probe failed on all endpoints");
    false
}
