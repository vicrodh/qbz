//! Per-playlist "Suggested Songs" dismiss store (T10).
//!
//! The playlist suggestions section lets the user dismiss ("not interested")
//! a suggested track. The rejection is sticky for that playlist, so the same
//! track never re-appears in future suggestion runs for it. A single JSON
//! file at the shared QBZ data path holds the dismissals — small enough for a
//! full read on each suggestions call, no SQLite needed at this layer.
//!
//! This is the Slint-side replacement for the Svelte service's
//! `getDismissedTrackIds` / `dismissTrack` localStorage pair
//! (`playlist_suggestions_dismissed_<playlistId>`). Modeled on
//! `discovery_dismiss.rs`; the only shape change is the value type
//! (track-id `u64` lists, keyed by playlist id string).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Map of playlist id (as string) -> dismissed Qobuz track ids.
#[derive(Default, Serialize, Deserialize)]
struct DismissStore {
    #[serde(flatten)]
    by_playlist: HashMap<String, Vec<u64>>,
}

fn store_path() -> Option<PathBuf> {
    Some(
        dirs::data_dir()?
            .join("qbz")
            .join("playlist_suggestions_dismiss.json"),
    )
}

fn load_store() -> DismissStore {
    let Some(path) = store_path() else {
        return DismissStore::default();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => DismissStore::default(),
    }
}

fn write_store(store: &DismissStore) {
    let Some(path) = store_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("[qbz-slint] playlist-suggestions-dismiss dir failed: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(store) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!("[qbz-slint] playlist-suggestions-dismiss write failed: {e}");
            }
        }
        Err(e) => log::warn!("[qbz-slint] playlist-suggestions-dismiss serialize failed: {e}"),
    }
}

/// Return the set of dismissed track ids for `playlist_id`. Used as a filter
/// input by the suggestions controller's pool filter.
pub fn dismissed_for_playlist(playlist_id: u64) -> HashSet<u64> {
    let store = load_store();
    store
        .by_playlist
        .get(&playlist_id.to_string())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Record a dismissal of `track_id` for `playlist_id` (idempotent).
pub fn dismiss(playlist_id: u64, track_id: u64) {
    let mut store = load_store();
    let entry = store.by_playlist.entry(playlist_id.to_string()).or_default();
    if !entry.contains(&track_id) {
        entry.push(track_id);
    }
    write_store(&store);
}
