//! Per-user pinned-items lifecycle + access wrapper.
//!
//! A process-global singleton over the headless
//! `qbz_app::settings::pinned_items::PinnedItemsService` (ADR-006: all model
//! logic — schema, O(1) key set, mutations — lives in `qbz-app`; this module
//! only owns the per-user store lifecycle and the thin accessors the Slint
//! surfaces call).
//!
//! Lifecycle mirrors `artist_blacklist` / `fav_cache` / `discover_prefs`: a
//! process-global `Mutex<Option<Service>>` bound per session via
//! [`init_for_user`] / [`teardown`], next to the other per-user stores. The
//! service keeps its own in-memory `(kind, id)` set, so reads never round-trip
//! SQLite; matching the family, there is no change-notify mechanism — mutation
//! sites re-run the consumer's reload / re-push path.
//!
//! Fail-open everywhere: with no session bound (`None`), checks behave as "not
//! pinned", the list/snapshot are empty, and mutations return the exact error
//! string the sibling stores use so the UI shows the same message.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use qbz_app::settings::pinned_items::{PinnedItemsService, DB_FILE_NAME};

pub use qbz_app::settings::pinned_items::PinnedItem;

/// Per-user pinned-items service. `None` outside an active session (online or
/// offline); pure fail-open behavior in that window.
static SERVICE: Mutex<Option<PinnedItemsService>> = Mutex::new(None);

/// The exact error string the sibling per-user stores return for a mutation
/// attempted with no active session. Kept verbatim so the UI surfaces the
/// same message.
const NO_SESSION_ERR: &str = "No active session - please log in";

// ---------------------------------------------------------------------------
// Lifecycle (mirrors artist_blacklist::{init_for_user, teardown})
// ---------------------------------------------------------------------------

/// Bind the per-user store from `<dir>/pinned_items.db`. Called on every
/// session activation — login, restore, AND offline entry — next to
/// `artist_blacklist::init_for_user`. Best-effort: a store-open failure logs
/// and leaves the singleton `None` (fail-open: nothing is pinned, never
/// blocks entry). The offline binding matters — pinned items are local-only
/// and must render offline.
pub fn init_for_user(base_dir: &Path) {
    let db_path = base_dir.join(DB_FILE_NAME);
    match PinnedItemsService::new(&db_path) {
        Ok(service) => {
            if let Ok(mut guard) = SERVICE.lock() {
                *guard = Some(service);
            }
        }
        Err(e) => log::error!("[qbz-slint] pinned items store open failed: {e}"),
    }
}

/// Drop the per-user store on logout. Mirrors `artist_blacklist::teardown`.
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
fn with_service<T>(default: T, f: impl FnOnce(&PinnedItemsService) -> T) -> T {
    SERVICE
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(f))
        .unwrap_or(default)
}

/// True when the `(kind, id)` item is pinned. Fail-open `false` when no
/// session is bound.
pub fn is_pinned(kind: &str, id: &str) -> bool {
    with_service(false, |s| s.is_pinned(kind, id))
}

/// All pinned items, newest first, for the Pinned section loader. Empty on no
/// session or query error.
pub fn list() -> Vec<PinnedItem> {
    with_service(Vec::new(), |s| s.list().unwrap_or_default())
}

/// Count of pinned items. `0` when no session is bound.
#[allow(dead_code)] // family-API parity (blacklist::count); no consumer wired yet
pub fn count() -> usize {
    with_service(0, |s| s.count())
}

/// Snapshot of the full `(kind, id)` key set, for bulk card stamping (mirrors
/// `artist_blacklist::ids_snapshot`). Empty when no session is bound.
#[allow(dead_code)] // converters stamp per-row via is_pinned today; kept for bulk maps
pub fn keys_snapshot() -> HashSet<(String, String)> {
    with_service(HashSet::new(), |s| s.keys_snapshot())
}

// ---------------------------------------------------------------------------
// Mutations (Err with the "no active session" string when unbound)
// ---------------------------------------------------------------------------

/// Run a mutation against the bound service, returning the "no active
/// session" error string when there is none / the lock is poisoned.
fn mutate(f: impl FnOnce(&PinnedItemsService) -> Result<(), String>) -> Result<(), String> {
    match SERVICE.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(service) => f(service),
            None => Err(NO_SESSION_ERR.into()),
        },
        Err(_) => Err(NO_SESSION_ERR.into()),
    }
}

/// Pin an item (upsert; `pinned_at` is stamped by the service).
pub fn pin(item: &PinnedItem) -> Result<(), String> {
    mutate(|s| s.pin(item))
}

/// Unpin an item. Absent rows are Ok, not an error.
pub fn unpin(kind: &str, id: &str) -> Result<(), String> {
    mutate(|s| s.unpin(kind, id))
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
        let dir = std::env::temp_dir().join(format!("qbz-slint-pinned-test-{nanos}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn item(kind: &str, id: &str, title: &str) -> PinnedItem {
        PinnedItem {
            kind: kind.to_string(),
            id: id.to_string(),
            title: title.to_string(),
            subtitle: String::new(),
            artwork_url: String::new(),
            pinned_at: 0, // ignored on write; the service stamps now
        }
    }

    /// One combined test: the singleton is process-global, so splitting into
    /// parallel tests would let them clobber each other. Covers the full
    /// round-trip: empty state after init, pin reflected in check + list +
    /// snapshot, unpin, then teardown restores the fail-open state.
    #[test]
    fn lifecycle_roundtrip() {
        let dir = unique_temp_dir();

        init_for_user(&dir);
        assert!(!is_pinned("album", "abc"), "nothing pinned yet");
        assert!(list().is_empty(), "fresh store lists nothing");
        assert!(keys_snapshot().is_empty(), "fresh snapshot is empty");
        assert_eq!(count(), 0);

        pin(&item("album", "abc", "An Album")).expect("pin succeeds with a bound store");
        assert!(is_pinned("album", "abc"), "pinned item is pinned");
        assert!(!is_pinned("playlist", "abc"), "kinds are isolated");
        assert_eq!(count(), 1);
        assert!(
            keys_snapshot().contains(&("album".to_string(), "abc".to_string())),
            "snapshot contains the pinned key"
        );
        let all = list();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title, "An Album");

        unpin("album", "abc").expect("unpin succeeds");
        assert!(!is_pinned("album", "abc"), "unpinned item is gone");
        unpin("album", "nope").expect("absent unpin is Ok");
        assert_eq!(count(), 0);

        // Rebind persists across sessions: pin, teardown, re-init, still pinned.
        pin(&item("playlist", "7", "P")).expect("pin");
        teardown();
        assert!(!is_pinned("playlist", "7"), "fail-open after teardown");
        assert!(list().is_empty(), "empty list after teardown");
        assert!(keys_snapshot().is_empty(), "empty snapshot after teardown");
        assert_eq!(count(), 0);
        assert!(
            pin(&item("album", "x", "X")).is_err(),
            "mutation with no session returns the error string"
        );
        assert!(
            unpin("album", "x").is_err(),
            "unpin with no session returns the error string"
        );
        init_for_user(&dir);
        assert!(is_pinned("playlist", "7"), "pin persisted across rebind");

        teardown();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
