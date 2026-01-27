//! Update checks and "What's New" persistence
//!
//! Requirements:
//! - Durable persistence via SQLite (no JSON files)
//! - Network failures must fail silently
//! - Update checks must never block startup

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use log::{info, warn};
use reqwest::Client;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/vicrodh/qbz/releases";
const UPDATE_MIN_AGE_HOURS: i64 = 12;
const NETWORK_TIMEOUT_SECONDS: u64 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePreferences {
    pub check_on_launch: bool,
    pub show_whats_new_on_launch: bool,
}

impl Default for UpdatePreferences {
    fn default() -> Self {
        Self {
            check_on_launch: true,
            show_whats_new_on_launch: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: String,
    published_at: String,
    body: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub published_at_epoch: i64,
    pub body: Option<String>,
    pub html_url: String,
    pub is_old_enough: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCheckStatus {
    NoUpdates,
    UpdateAvailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub status: UpdateCheckStatus,
    pub current_version: String,
    pub release: Option<ReleaseInfo>,
}

impl UpdateCheckResult {
    fn no_updates(current_version: String) -> Self {
        Self {
            status: UpdateCheckStatus::NoUpdates,
            current_version,
            release: None,
        }
    }
}

/// SQLite-backed persistence for updates
pub struct UpdatesStore {
    conn: Connection,
}

impl UpdatesStore {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let db_path = data_dir.join("updates.db");
        let conn =
            Connection::open(&db_path).map_err(|e| format!("Failed to open updates database: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS update_preferences (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                check_on_launch INTEGER NOT NULL DEFAULT 1,
                show_whats_new_on_launch INTEGER NOT NULL DEFAULT 1
            );

            INSERT OR IGNORE INTO update_preferences (id, check_on_launch, show_whats_new_on_launch)
            VALUES (1, 1, 1);

            CREATE TABLE IF NOT EXISTS acknowledged_releases (
                version TEXT PRIMARY KEY,
                acknowledged_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS ignored_releases (
                version TEXT PRIMARY KEY,
                ignored_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS whats_new_shown (
                version TEXT PRIMARY KEY,
                shown_at INTEGER NOT NULL
            );",
        )
        .map_err(|e| format!("Failed to create updates tables: {}", e))?;

        // Migration guard: ensure both preference columns exist even on partial schemas
        let pref_migrations = [
            "ALTER TABLE update_preferences ADD COLUMN check_on_launch INTEGER NOT NULL DEFAULT 1",
            "ALTER TABLE update_preferences ADD COLUMN show_whats_new_on_launch INTEGER NOT NULL DEFAULT 1",
        ];
        for migration in pref_migrations {
            let _ = conn.execute(migration, []);
        }

        Ok(Self { conn })
    }

    pub fn get_preferences(&self) -> Result<UpdatePreferences, String> {
        self.conn
            .query_row(
                "SELECT check_on_launch, show_whats_new_on_launch FROM update_preferences WHERE id = 1",
                [],
                |row| {
                    Ok(UpdatePreferences {
                        check_on_launch: row.get::<_, i64>(0)? != 0,
                        show_whats_new_on_launch: row.get::<_, i64>(1)? != 0,
                    })
                },
            )
            .map_err(|e| format!("Failed to get update preferences: {}", e))
    }

    pub fn set_check_on_launch(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE update_preferences SET check_on_launch = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set check_on_launch: {}", e))?;
        Ok(())
    }

    pub fn set_show_whats_new_on_launch(&self, enabled: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE update_preferences SET show_whats_new_on_launch = ?1 WHERE id = 1",
                params![enabled as i64],
            )
            .map_err(|e| format!("Failed to set show_whats_new_on_launch: {}", e))?;
        Ok(())
    }

    pub fn acknowledge_release(&self, version: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO acknowledged_releases (version, acknowledged_at) VALUES (?1, ?2)",
                params![version, now_epoch_seconds()],
            )
            .map_err(|e| format!("Failed to acknowledge release: {}", e))?;
        Ok(())
    }

    pub fn ignore_release(&self, version: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO ignored_releases (version, ignored_at) VALUES (?1, ?2)",
                params![version, now_epoch_seconds()],
            )
            .map_err(|e| format!("Failed to ignore release: {}", e))?;
        Ok(())
    }

    pub fn is_release_acknowledged(&self, version: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM acknowledged_releases WHERE version = ?1 LIMIT 1",
                params![version],
                |_row| Ok(()),
            )
            .is_ok()
    }

    pub fn is_release_ignored(&self, version: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM ignored_releases WHERE version = ?1 LIMIT 1",
                params![version],
                |_row| Ok(()),
            )
            .is_ok()
    }

    pub fn has_whats_new_been_shown(&self, version: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM whats_new_shown WHERE version = ?1 LIMIT 1",
                params![version],
                |_row| Ok(()),
            )
            .is_ok()
    }

    pub fn mark_whats_new_shown(&self, version: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO whats_new_shown (version, shown_at) VALUES (?1, ?2)",
                params![version, now_epoch_seconds()],
            )
            .map_err(|e| format!("Failed to mark whats_new_shown: {}", e))?;
        Ok(())
    }
}

/// Thread-safe state wrapper
pub struct UpdatesState {
    store: Arc<Mutex<UpdatesStore>>,
    client: Client,
}

impl UpdatesState {
    pub fn new() -> Result<Self, String> {
        let store = UpdatesStore::new()?;
        let client = Client::builder()
            .timeout(Duration::from_secs(NETWORK_TIMEOUT_SECONDS))
            .user_agent("qbz-nix-updates")
            .build()
            .map_err(|e| format!("Failed to create updates HTTP client: {}", e))?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            client,
        })
    }

    fn with_store<T>(&self, f: impl FnOnce(&UpdatesStore) -> T) -> T {
        let guard = self.store.lock().expect("UpdatesStore mutex poisoned");
        f(&guard)
    }

    fn current_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

fn now_epoch_seconds() -> i64 {
    Utc::now().timestamp()
}

fn normalize_version_tag(tag: &str) -> String {
    tag.trim().trim_start_matches('v').to_string()
}

fn parse_version_parts(version: &str) -> Vec<u64> {
    normalize_version_tag(version)
        .split('.')
        .map(|segment| {
            let numeric: String = segment.chars().take_while(|c| c.is_ascii_digit()).collect();
            numeric.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

fn is_newer_version(current: &str, candidate: &str) -> bool {
    let current_parts = parse_version_parts(current);
    let candidate_parts = parse_version_parts(candidate);
    let max_len = current_parts.len().max(candidate_parts.len());
    for idx in 0..max_len {
        let cur = *current_parts.get(idx).unwrap_or(&0);
        let cand = *candidate_parts.get(idx).unwrap_or(&0);
        if cand > cur {
            return true;
        }
        if cand < cur {
            return false;
        }
    }
    false
}

fn parse_published_at_epoch(published_at: &str) -> Option<i64> {
    let parsed: DateTime<Utc> = DateTime::parse_from_rfc3339(published_at)
        .ok()?
        .with_timezone(&Utc);
    Some(parsed.timestamp())
}

fn is_old_enough(epoch_seconds: i64) -> bool {
    let published = DateTime::<Utc>::from_timestamp(epoch_seconds, 0);
    let Some(published) = published else {
        return false;
    };
    let age = Utc::now() - published;
    age >= ChronoDuration::hours(UPDATE_MIN_AGE_HOURS)
}

fn to_release_info(release: GithubRelease) -> Option<ReleaseInfo> {
    let epoch = parse_published_at_epoch(&release.published_at)?;
    let version = normalize_version_tag(&release.tag_name);
    let old_enough = is_old_enough(epoch);
    Some(ReleaseInfo {
        version,
        tag_name: release.tag_name,
        name: release.name,
        published_at: release.published_at,
        published_at_epoch: epoch,
        body: release.body,
        html_url: release.html_url,
        is_old_enough: old_enough,
    })
}

async fn fetch_releases(client: &Client) -> Result<Vec<GithubRelease>, String> {
    let response = client
        .get(GITHUB_RELEASES_URL)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch releases: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("Release fetch returned HTTP {}", response.status()));
    }
    response
        .json::<Vec<GithubRelease>>()
        .await
        .map_err(|e| format!("Failed to parse releases JSON: {}", e))
}

async fn fetch_release_by_tag(client: &Client, tag: &str) -> Result<GithubRelease, String> {
    let url = format!("{}/tags/{}", GITHUB_RELEASES_URL, tag);
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release by tag: {}", e))?;
    if !response.status().is_success() {
        return Err(format!(
            "Release-by-tag fetch returned HTTP {}",
            response.status()
        ));
    }
    response
        .json::<GithubRelease>()
        .await
        .map_err(|e| format!("Failed to parse release-by-tag JSON: {}", e))
}

fn select_latest_valid_update(
    current_version: &str,
    releases: Vec<GithubRelease>,
    state: &UpdatesState,
) -> Option<ReleaseInfo> {
    for release in releases {
        if release.draft || release.prerelease {
            continue;
        }
        let Some(info) = to_release_info(release) else {
            continue;
        };
        if !info.is_old_enough {
            continue;
        }
        if !is_newer_version(current_version, &info.version) {
            continue;
        }
        let is_blocked = state.with_store(|store| {
            store.is_release_acknowledged(&info.version) || store.is_release_ignored(&info.version)
        });
        if is_blocked {
            continue;
        }
        return Some(info);
    }
    None
}

// ==================== Tauri commands ====================

#[tauri::command]
pub fn get_update_preferences(state: tauri::State<UpdatesState>) -> Result<UpdatePreferences, String> {
    state.with_store(|store| store.get_preferences())
}

#[tauri::command]
pub fn set_update_check_on_launch(
    enabled: bool,
    state: tauri::State<UpdatesState>,
) -> Result<(), String> {
    state.with_store(|store| store.set_check_on_launch(enabled))
}

#[tauri::command]
pub fn set_show_whats_new_on_launch(
    enabled: bool,
    state: tauri::State<UpdatesState>,
) -> Result<(), String> {
    state.with_store(|store| store.set_show_whats_new_on_launch(enabled))
}

#[tauri::command]
pub fn acknowledge_release(version: String, state: tauri::State<UpdatesState>) -> Result<(), String> {
    state.with_store(|store| store.acknowledge_release(&version))
}

#[tauri::command]
pub fn ignore_release(version: String, state: tauri::State<UpdatesState>) -> Result<(), String> {
    state.with_store(|store| store.ignore_release(&version))
}

#[tauri::command]
pub fn is_release_acknowledged(version: String, state: tauri::State<UpdatesState>) -> bool {
    state.with_store(|store| store.is_release_acknowledged(&version))
}

#[tauri::command]
pub fn is_release_ignored(version: String, state: tauri::State<UpdatesState>) -> bool {
    state.with_store(|store| store.is_release_ignored(&version))
}

#[tauri::command]
pub fn has_whats_new_been_shown(version: String, state: tauri::State<UpdatesState>) -> bool {
    state.with_store(|store| store.has_whats_new_been_shown(&version))
}

#[tauri::command]
pub fn mark_whats_new_shown(version: String, state: tauri::State<UpdatesState>) -> Result<(), String> {
    state.with_store(|store| store.mark_whats_new_shown(&version))
}

#[tauri::command]
pub fn get_current_version(state: tauri::State<UpdatesState>) -> String {
    state.current_version()
}

#[tauri::command]
pub async fn check_for_updates(
    mode: String,
    state: tauri::State<'_, UpdatesState>,
) -> Result<UpdateCheckResult, String> {
    let current_version = state.current_version();

    if mode == "launch" {
        let prefs = match state.with_store(|store| store.get_preferences()) {
            Ok(p) => p,
            Err(e) => {
                warn!("[Updates] Failed to read preferences: {}", e);
                return Ok(UpdateCheckResult::no_updates(current_version));
            }
        };
        if !prefs.check_on_launch {
            info!("[Updates] Skipping launch check (disabled in preferences)");
            return Ok(UpdateCheckResult::no_updates(current_version));
        }
    }

    let releases = match fetch_releases(&state.client).await {
        Ok(r) => r,
        Err(e) => {
            // Silent failure: log and return "no updates"
            warn!("[Updates] Release fetch failed: {}", e);
            return Ok(UpdateCheckResult::no_updates(current_version));
        }
    };

    let maybe_update = select_latest_valid_update(&current_version, releases, &state);
    match maybe_update {
        Some(release) => Ok(UpdateCheckResult {
            status: UpdateCheckStatus::UpdateAvailable,
            current_version,
            release: Some(release),
        }),
        None => Ok(UpdateCheckResult::no_updates(current_version)),
    }
}

#[tauri::command]
pub async fn fetch_release_for_version(
    version: String,
    state: tauri::State<'_, UpdatesState>,
) -> Result<Option<ReleaseInfo>, String> {
    // Prefer exact tag lookup to avoid scanning all releases for the current version.
    let tag = if version.starts_with('v') {
        version.clone()
    } else {
        format!("v{}", version)
    };
    let release = match fetch_release_by_tag(&state.client, &tag).await {
        Ok(r) => r,
        Err(e) => {
            warn!("[Updates] Release-by-tag fetch failed for {}: {}", tag, e);
            return Ok(None);
        }
    };
    if release.draft || release.prerelease {
        return Ok(None);
    }
    Ok(to_release_info(release))
}
