//! Shared favorite-track cache.
//!
//! A single process-wide set of the user's favorite track IDs, so every
//! track-list surface (album, artist, search, playlist, mix, favorites)
//! can stamp `is-favorite` on each row without re-fetching, and the row
//! heart can toggle optimistically.
//!
//! The set is loaded once from `core().favorite_track_ids()` (the same
//! source the Queue sidebar uses) and kept in sync by the favorite
//! media-action handler, which inserts/removes locally the moment the
//! user clicks — the network call follows in the background.

use std::collections::HashSet;
use std::sync::{LazyLock, RwLock};

static FAVORITES: LazyLock<RwLock<HashSet<u64>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

/// Replace the cache with a freshly-fetched set.
pub fn set_all(ids: HashSet<u64>) {
    if let Ok(mut guard) = FAVORITES.write() {
        *guard = ids;
    }
}

/// True when the given track id (string form) is in the favorite set.
/// Non-numeric ids (local tracks) are never favorites.
pub fn is_favorite(track_id: &str) -> bool {
    let Ok(id) = track_id.parse::<u64>() else {
        return false;
    };
    FAVORITES.read().map(|g| g.contains(&id)).unwrap_or(false)
}

/// Insert / remove a single id, keeping the cache consistent with an
/// optimistic UI toggle. Returns the new favorite state.
pub fn set(track_id: u64, favorite: bool) {
    if let Ok(mut guard) = FAVORITES.write() {
        if favorite {
            guard.insert(track_id);
        } else {
            guard.remove(&track_id);
        }
    }
}
