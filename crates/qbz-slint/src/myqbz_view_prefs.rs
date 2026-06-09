//! Per-collection view-prefs persistence for the My QBZ DETAIL view
//! (spec 12 §18).
//!
//! Mirrors the Tauri `userStorage` key `collection-view-prefs.{collectionId}`:
//! each collection remembers its own toolbar state across opens. The persisted
//! shape (five fields) is exactly the Tauri set:
//!
//!   { viewMode, sortBy, sortDir, typeFilter, sourceFilter:[SourceKind] }
//!
//! `searchQuery` and `selectMode` are intentionally TRANSIENT — never persisted
//! (same as Tauri).
//!
//! Storage is per-user JSON (so different Qobuz accounts keep independent
//! prefs), scoped the same way as `myqbz_prefs.rs`:
//!
//!   <data_dir>/qbz/users/<user_id>/collection_view_prefs.json
//!
//! Rather than one file per collection, the whole map lives in one tiny JSON
//! (`{ "<collection-id>": { … } }`) — read-modify-write on every set. The store
//! is keyed by collection id, which is the §18 contract.
//!
//! Lifecycle (driven from `myqbz_detail` + `myqbz_edit`):
//!  - **restore on open**: `load(id)` → apply each field, else defaults.
//!  - **persist on change**: `save(id, prefs)` after a toolbar setter mutates a
//!    persisted field — gated behind a `hydrated` flag so the restore is not
//!    clobbered by an early persist (mirrors Tauri's `prefsHydrated`).
//!  - **clear on delete**: `remove(id)` drops the orphaned key.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

/// The active user id, set by `init_for_user`. `None` before login (the store
/// degrades to defaults — there is no pre-login detail view).
static USER_ID: LazyLock<Mutex<Option<u64>>> = LazyLock::new(|| Mutex::new(None));

/// The five persisted view-pref fields for one collection (spec 12 §18). Source
/// filter is the three independent flags the Slint toolbar uses (Slint has no
/// Set); together they round-trip the Tauri `sourceFilter:[SourceKind]` array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default = "d_list")]
    pub view_mode: String,
    #[serde(default = "d_position")]
    pub sort_by: String,
    #[serde(default = "d_asc")]
    pub sort_dir: String,
    #[serde(default = "d_all")]
    pub type_filter: String,
    #[serde(default)]
    pub src_qobuz: bool,
    #[serde(default)]
    pub src_plex: bool,
    #[serde(default)]
    pub src_local: bool,
}

fn d_list() -> String {
    "list".to_string()
}
fn d_position() -> String {
    "position".to_string()
}
fn d_asc() -> String {
    "asc".to_string()
}
fn d_all() -> String {
    "all".to_string()
}

impl Default for Prefs {
    /// The §18 defaults: list / position / asc / all / empty source set.
    fn default() -> Self {
        Self {
            view_mode: d_list(),
            sort_by: d_position(),
            sort_dir: d_asc(),
            type_filter: d_all(),
            src_qobuz: false,
            src_plex: false,
            src_local: false,
        }
    }
}

/// `<data_dir>/qbz/users/<user_id>/collection_view_prefs.json` for the active
/// user. `None` before login or when the data dir is unavailable.
fn store_path() -> Option<PathBuf> {
    let user_id = (*USER_ID.lock().ok()?)?;
    Some(
        dirs::data_dir()?
            .join("qbz")
            .join("users")
            .join(user_id.to_string())
            .join("collection_view_prefs.json"),
    )
}

/// Read the whole `{ collection-id -> Prefs }` map. A missing / unreadable /
/// unparseable file degrades to an empty map.
fn read_all() -> HashMap<String, Prefs> {
    let Some(path) = store_path() else {
        return HashMap::new();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// Persist the whole map (best-effort — failures are logged).
fn write_all(map: &HashMap<String, Prefs>) {
    let Some(path) = store_path() else {
        log::warn!("[qbz-slint] collection view-prefs: no active user, not saving");
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::error!("[qbz-slint] collection view-prefs: create dir failed: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(map) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("[qbz-slint] collection view-prefs: write failed: {e}");
            }
        }
        Err(e) => log::error!("[qbz-slint] collection view-prefs: serialize failed: {e}"),
    }
}

/// Bind the store to `user_id` on shell entry. Subsequent reads/writes target
/// that user's JSON file.
pub fn init_for_user(user_id: u64) {
    if let Ok(mut guard) = USER_ID.lock() {
        *guard = Some(user_id);
    }
}

/// Load the stored prefs for `id`, or the §18 defaults when none are stored.
pub fn load(id: &str) -> Prefs {
    read_all().remove(id).unwrap_or_default()
}

/// Persist the prefs for `id` (read-modify-write the whole map). Writing the
/// default set is harmless (re-open restores the same defaults), so the caller
/// need not special-case it.
pub fn save(id: &str, prefs: &Prefs) {
    if id.is_empty() {
        return;
    }
    let mut map = read_all();
    map.insert(id.to_string(), prefs.clone());
    write_all(&map);
}

/// Remove the stored prefs for `id` (cleanup on collection delete, spec §18 /
/// §11.3). No-op when the key is absent.
pub fn remove(id: &str) {
    if id.is_empty() {
        return;
    }
    let mut map = read_all();
    if map.remove(id).is_some() {
        write_all(&map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec_18() {
        let p = Prefs::default();
        assert_eq!(p.view_mode, "list");
        assert_eq!(p.sort_by, "position");
        assert_eq!(p.sort_dir, "asc");
        assert_eq!(p.type_filter, "all");
        assert!(!p.src_qobuz && !p.src_plex && !p.src_local);
    }

    #[test]
    fn legacy_json_without_fields_deserializes_to_defaults() {
        let p: Prefs = serde_json::from_str("{}").expect("empty object deserializes");
        assert_eq!(p, Prefs::default());
    }

    #[test]
    fn partial_json_keeps_present_fields() {
        let p: Prefs = serde_json::from_str(r#"{"view_mode":"grid","src_plex":true}"#)
            .expect("partial object deserializes");
        assert_eq!(p.view_mode, "grid");
        assert!(p.src_plex);
        // Absent fields fall back to defaults.
        assert_eq!(p.sort_by, "position");
        assert_eq!(p.type_filter, "all");
        assert!(!p.src_qobuz);
    }
}
