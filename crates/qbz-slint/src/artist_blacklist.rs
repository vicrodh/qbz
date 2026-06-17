//! Per-user artist-blacklist lifecycle + access wrapper.
//!
//! A process-global singleton over the headless
//! `qbz_app::settings::artist_blacklist::BlacklistService` (ADR-006: all model
//! logic — schema, O(1) lookup set, enable flag, mutations — lives in
//! `qbz-app`; this module only owns the per-user store lifecycle and the thin
//! accessors the Slint surfaces call).
//!
//! Lifecycle mirrors `fav_cache` / `discover_prefs`: a process-global
//! `Mutex<Option<Service>>` bound per session via [`init_for_user`] /
//! [`teardown`], next to the other per-user stores. The service keeps its own
//! in-memory `HashSet` + enabled flag, so reads never round-trip SQLite; there
//! is no separate cache here and — matching `fav_cache` — no change-notify
//! mechanism: callers re-read after mutating (later UI tasks re-push Slint state
//! after a mutation).
//!
//! Fail-open everywhere: with no session bound (`None`), checks behave as "not
//! blacklisted" / "enabled", snapshots are empty, and mutations return the exact
//! Tauri error string so the UI shows the same message.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use qbz_app::settings::artist_blacklist::{BlacklistService, BlacklistedArtist, DB_FILE_NAME};

/// Per-user blacklist service. `None` outside an active session (online or
/// offline); pure fail-open behavior in that window.
static SERVICE: Mutex<Option<BlacklistService>> = Mutex::new(None);

/// The exact error string the Tauri build returns for a mutation attempted
/// with no active session. Kept verbatim so the UI surfaces the same message.
const NO_SESSION_ERR: &str = "No active session - please log in";

// ---------------------------------------------------------------------------
// Lifecycle (mirrors fav_cache::{init_for_user, teardown})
// ---------------------------------------------------------------------------

/// Bind the per-user store from `<dir>/artist_blacklist.db`. Called on every
/// session activation — login, restore, AND offline entry — next to
/// `fav_cache::init_for_user`. Best-effort: a store-open failure logs and
/// leaves the singleton `None` (fail-open: nothing is blacklisted, the feature
/// reads as enabled, never blocks entry). The offline binding is the fix for
/// the Tauri gap where the blacklist was never initialized in offline mode.
pub fn init_for_user(base_dir: &Path) {
    let db_path = base_dir.join(DB_FILE_NAME);
    match BlacklistService::new(&db_path) {
        Ok(service) => {
            if let Ok(mut guard) = SERVICE.lock() {
                *guard = Some(service);
            }
        }
        Err(e) => log::error!("[qbz-slint] artist blacklist store open failed: {e}"),
    }
}

/// Drop the per-user store on logout. Mirrors `fav_cache::teardown`.
pub fn teardown() {
    if let Ok(mut guard) = SERVICE.lock() {
        *guard = None;
    }
}

// ---------------------------------------------------------------------------
// Accessors (fail-open when no session is bound)
// ---------------------------------------------------------------------------

/// Run a closure against the bound service, or `default` when there is none /
/// the lock is poisoned.
fn with_service<T>(default: T, f: impl FnOnce(&BlacklistService) -> T) -> T {
    SERVICE
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(f))
        .unwrap_or(default)
}

/// True when the artist id is blacklisted (and the feature is enabled).
/// Fail-open `false` when no session is bound.
pub fn is_blacklisted(artist_id: u64) -> bool {
    with_service(false, |s| s.is_blacklisted(artist_id))
}

/// True when the string-form artist id parses and is blacklisted. Non-numeric
/// ids (local artists) are never blacklisted. For row code that carries string
/// ids.
pub fn is_blacklisted_id_str(artist_id: &str) -> bool {
    let Ok(id) = artist_id.parse::<u64>() else {
        return false;
    };
    is_blacklisted(id)
}

/// Stamp value for a `TrackItem.is-blacklisted` cell (Task 6). The single
/// rule every in-scope track controller (album / playlist / favorites / the
/// four Q-mixes) reuses so render and the Task 7 queue filters agree on what
/// "blacklisted" means per row:
///
/// - **HARD local/Plex guard** — a non-Qobuz `source` is NEVER blacklisted
///   (Codex guardrail; local copies with a numeric Qobuz id must still stay
///   playable). `qobuz_download` rows render `source == "qobuz"`, so they are
///   treated as Qobuz here — that matches Tauri (VTL keys on `!isLocal`).
/// - Resolve the artist from the candidate string ids in order; the first
///   non-empty, numeric, blacklisted id wins (D-FEAT: performer OR composer;
///   album rows that lack a performer fall back to the album's primary artist).
/// - Missing / zero / non-numeric ids => fail-open (`false`).
///
/// The enabled-flag gate and the no-session fail-open live in
/// [`is_blacklisted`], so this never blocks when the feature is off or no
/// session is bound.
///
/// Live re-stamp contract (Step B): there is no change-notify here (the
/// fav_cache pattern). Every in-scope controller already calls this at LOAD
/// time, so navigating to a view always shows correct state. To refresh the
/// CURRENTLY-loaded lists after a blacklist mutation (Task 9 artist toggle /
/// Task 11 manager), the mutation site re-runs that controller's existing
/// reload / re-push path (which re-invokes `stamp_row` per row) — same as how
/// favorites re-push after a `fav_cache` change. There is intentionally no
/// global listener/observer.
pub fn stamp_row(source: &str, artist_ids: &[&str]) -> bool {
    // Local / Plex / ephemeral rows are protected — never blacklisted.
    if source != "qobuz" {
        return false;
    }
    artist_ids.iter().any(|id| is_blacklisted_id_str(id))
}

/// True when the blacklist feature is enabled. Default-enabled (`true`) when no
/// session is bound.
pub fn is_enabled() -> bool {
    with_service(true, |s| s.is_enabled())
}

/// Snapshot of the full blacklisted-id set, for `qbz_core::search_all`-style
/// filtering. Empty when no session is bound. Derived from `get_all` so it
/// reflects the persisted rows (ignores the enabled flag — callers gate on
/// [`is_enabled`] separately).
pub fn ids_snapshot() -> HashSet<u64> {
    with_service(HashSet::new(), |s| {
        s.get_all()
            .map(|list| list.into_iter().map(|a| a.artist_id).collect())
            .unwrap_or_default()
    })
}

/// All blacklisted artists (name-sorted), for the manager view. Empty on no
/// session or query error.
pub fn get_all() -> Vec<BlacklistedArtist> {
    with_service(Vec::new(), |s| s.get_all().unwrap_or_default())
}

/// Count of blacklisted artists (ignores the enabled flag). `0` when no session
/// is bound.
pub fn count() -> usize {
    with_service(0, |s| s.count())
}

// ---------------------------------------------------------------------------
// Mutations (Err with the Tauri "no active session" string when unbound)
// ---------------------------------------------------------------------------

/// Run a mutation against the bound service, returning the Tauri "no active
/// session" error string when there is none / the lock is poisoned.
fn mutate(f: impl FnOnce(&BlacklistService) -> Result<(), String>) -> Result<(), String> {
    match SERVICE.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(service) => f(service),
            None => Err(NO_SESSION_ERR.into()),
        },
        Err(_) => Err(NO_SESSION_ERR.into()),
    }
}

/// Add an artist to the blacklist.
pub fn add(artist_id: u64, artist_name: &str, notes: Option<&str>) -> Result<(), String> {
    mutate(|s| s.add(artist_id, artist_name, notes))
}

/// Remove an artist from the blacklist.
pub fn remove(artist_id: u64) -> Result<(), String> {
    mutate(|s| s.remove(artist_id))
}

/// Toggle the global enable flag.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    mutate(|s| s.set_enabled(enabled))
}

/// Clear all blacklisted artists (leaves the enabled flag untouched).
pub fn clear_all() -> Result<(), String> {
    mutate(|s| s.clear_all())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique temp dir under the system temp root (no `tempfile` dev-dep on
    /// qbz-slint). Created here, removed at the end of the test.
    fn unique_temp_dir() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("qbz-slint-blacklist-test-{nanos}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// One combined test: the singleton is process-global, so splitting into
    /// parallel tests would let them clobber each other. Covers the full
    /// round-trip: empty snapshot + default-enabled after init, add reflected in
    /// both check + snapshot, then teardown restores the fail-open state.
    #[test]
    fn lifecycle_roundtrip() {
        let dir = unique_temp_dir();

        init_for_user(&dir);
        assert!(ids_snapshot().is_empty(), "fresh store has no ids");
        assert!(is_enabled(), "blacklist defaults to enabled");
        assert!(!is_blacklisted(42), "nothing blacklisted yet");

        add(42, "X", None).expect("add succeeds with a bound store");
        assert!(is_blacklisted(42), "added id is blacklisted");
        assert!(is_blacklisted_id_str("42"), "string-id check matches");
        assert!(ids_snapshot().contains(&42), "snapshot contains the added id");
        assert_eq!(count(), 1);

        teardown();
        assert!(!is_blacklisted(42), "fail-open after teardown");
        assert!(ids_snapshot().is_empty(), "empty snapshot after teardown");
        assert_eq!(count(), 0);
        assert!(is_enabled(), "default-enabled after teardown");
        assert!(
            add(1, "Y", None).is_err(),
            "mutation with no session returns the Tauri error string"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
