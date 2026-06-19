//! Shared favorite-track cache.
//!
//! A single process-wide set of the user's favorite track IDs, so every
//! track-list surface (album, artist, search, playlist, mix, favorites,
//! queue) can stamp `is-favorite` on each row without re-fetching, and the
//! row heart can toggle optimistically.
//!
//! Disk-first seeding: [`init_for_user`] binds the per-user persistent
//! store (`favorites_cache.db`, same file + schema as Tauri) on session
//! activation and loads the IDs from disk — so hearts are correct offline.
//! The online shell entry then refreshes the set from the network and
//! writes it back via [`set_all`]. Toggles keep memory and disk in sync
//! through [`set`].

use std::collections::HashSet;
use std::path::Path;
use std::sync::{LazyLock, Mutex, RwLock};

use qbz_app::settings::favorites_cache::FavoritesCacheStore;

static FAVORITES: LazyLock<RwLock<HashSet<u64>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

/// Process-wide set of the user's favorite ALBUM ids (string catalog ids).
/// Same disk-first + network-refresh lifecycle as [`FAVORITES`], so the
/// album header heart renders the right state from first paint and stays
/// live across toggles. Mirrors Tauri's `albumFavoritesStore`.
static FAV_ALBUMS: LazyLock<RwLock<HashSet<String>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

/// Process-wide set of the user's followed AWARD ids (string ids — Qobuz
/// types them inconsistently, stored TEXT). Same disk-first + network-refresh
/// lifecycle as the album set, so the AwardView follow heart renders the
/// right state from first paint and stays live across toggles. Mirrors
/// Tauri's `awardFavoritesStore`.
static FAV_AWARDS: LazyLock<RwLock<HashSet<String>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

/// Per-user persistent ID store. `None` until a session (online or offline)
/// is activated; pure in-memory behavior in that window.
static STORE: Mutex<Option<FavoritesCacheStore>> = Mutex::new(None);

/// Bind the per-user store and seed the in-memory set from disk (works
/// offline). Called on every session activation — login, restore, and
/// offline entry — next to `offline_mode::init_for_user`. Best-effort:
/// failures are logged and leave the set empty (hearts render unfavorited,
/// never block entry).
pub fn init_for_user(base_dir: &Path) {
    let store = match FavoritesCacheStore::new_at(base_dir) {
        Ok(store) => store,
        Err(e) => {
            log::error!("[qbz-slint] favorites cache store open failed: {e}");
            return;
        }
    };
    match store.get_favorite_track_ids() {
        Ok(ids) => {
            let set: HashSet<u64> = ids
                .into_iter()
                .filter_map(|id| u64::try_from(id).ok())
                .collect();
            log::info!(
                "[qbz-slint] favorites cache: {} track ids seeded from disk",
                set.len()
            );
            if let Ok(mut guard) = FAVORITES.write() {
                *guard = set;
            }
        }
        Err(e) => log::warn!("[qbz-slint] favorites cache disk seed failed: {e}"),
    }
    match store.get_favorite_album_ids() {
        Ok(ids) => {
            let set: HashSet<String> = ids.into_iter().collect();
            log::info!(
                "[qbz-slint] favorites cache: {} album ids seeded from disk",
                set.len()
            );
            if let Ok(mut guard) = FAV_ALBUMS.write() {
                *guard = set;
            }
        }
        Err(e) => log::warn!("[qbz-slint] favorites cache album disk seed failed: {e}"),
    }
    match store.get_favorite_award_ids() {
        Ok(ids) => {
            let set: HashSet<String> = ids.into_iter().collect();
            log::info!(
                "[qbz-slint] favorites cache: {} award ids seeded from disk",
                set.len()
            );
            if let Ok(mut guard) = FAV_AWARDS.write() {
                *guard = set;
            }
        }
        Err(e) => log::warn!("[qbz-slint] favorites cache award disk seed failed: {e}"),
    }
    if let Ok(mut guard) = STORE.lock() {
        *guard = Some(store);
    }
}

/// Drop the per-user store and the in-memory set on logout.
pub fn teardown() {
    if let Ok(mut guard) = STORE.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = FAVORITES.write() {
        guard.clear();
    }
    if let Ok(mut guard) = FAV_ALBUMS.write() {
        guard.clear();
    }
    if let Ok(mut guard) = FAV_AWARDS.write() {
        guard.clear();
    }
}

/// Replace the cache with a freshly-fetched set and mirror it to the
/// per-user store — full replace, the same semantics as Tauri's
/// `v2_sync_cached_favorite_tracks`. Blocking disk write; call off the
/// UI thread.
pub fn set_all(ids: HashSet<u64>) {
    if let Ok(mut guard) = FAVORITES.write() {
        *guard = ids.clone();
    }
    if let Ok(guard) = STORE.lock() {
        if let Some(store) = guard.as_ref() {
            let disk: Vec<i64> = ids.iter().map(|&id| id as i64).collect();
            if let Err(e) = store.sync_favorite_tracks(&disk) {
                log::warn!("[qbz-slint] favorites cache disk sync failed: {e}");
            }
        }
    }
}

/// True when the given track id (string form) is in the favorite set.
/// Non-numeric ids (local tracks) are never favorites.
pub fn is_favorite(track_id: &str) -> bool {
    let Ok(id) = track_id.parse::<u64>() else {
        return false;
    };
    contains(id)
}

/// Snapshot of the full favorite-track id set. Powers the offline
/// favorites rail (B9): the disk-first seeding makes this correct while
/// offline, right after session activation.
pub fn all() -> HashSet<u64> {
    FAVORITES.read().map(|g| g.clone()).unwrap_or_default()
}

/// True when the given numeric track id is in the favorite set.
pub fn contains(track_id: u64) -> bool {
    FAVORITES
        .read()
        .map(|g| g.contains(&track_id))
        .unwrap_or(false)
}

/// Insert / remove a single id, keeping the cache consistent with an
/// optimistic UI toggle, and mirror the change to the per-user store so
/// hearts survive a restart.
pub fn set(track_id: u64, favorite: bool) {
    if let Ok(mut guard) = FAVORITES.write() {
        if favorite {
            guard.insert(track_id);
        } else {
            guard.remove(&track_id);
        }
    }
    if let Ok(guard) = STORE.lock() {
        if let Some(store) = guard.as_ref() {
            let res = if favorite {
                store.add_favorite_track(track_id as i64)
            } else {
                store.remove_favorite_track(track_id as i64)
            };
            if let Err(e) = res {
                log::warn!("[qbz-slint] favorites cache disk update failed: {e}");
            }
        }
    }
}

// ==================== Favorite albums ====================

/// True when the album catalog id is in the user's favorite-album set.
pub fn is_album_favorite(album_id: &str) -> bool {
    FAV_ALBUMS
        .read()
        .map(|g| g.contains(album_id))
        .unwrap_or(false)
}

/// Replace the favorite-album set with a freshly-fetched id list and mirror
/// it to the per-user store (full replace — Tauri's
/// `v2_sync_cached_favorite_albums`). Blocking disk write; call off the UI
/// thread.
pub fn set_all_albums(ids: HashSet<String>) {
    if let Ok(guard) = STORE.lock() {
        if let Some(store) = guard.as_ref() {
            let disk: Vec<String> = ids.iter().cloned().collect();
            if let Err(e) = store.sync_favorite_albums(&disk) {
                log::warn!("[qbz-slint] favorites cache album sync failed: {e}");
            }
        }
    }
    if let Ok(mut guard) = FAV_ALBUMS.write() {
        *guard = ids;
    }
}

/// Insert / remove a single album id (optimistic toggle) and mirror the
/// change to the per-user store so the heart survives a restart.
pub fn set_album(album_id: &str, favorite: bool) {
    if let Ok(mut guard) = FAV_ALBUMS.write() {
        if favorite {
            guard.insert(album_id.to_string());
        } else {
            guard.remove(album_id);
        }
    }
    if let Ok(guard) = STORE.lock() {
        if let Some(store) = guard.as_ref() {
            let res = if favorite {
                store.add_favorite_album(album_id)
            } else {
                store.remove_favorite_album(album_id)
            };
            if let Err(e) = res {
                log::warn!("[qbz-slint] favorites cache album disk update failed: {e}");
            }
        }
    }
}

// ==================== Followed awards ====================

/// True when the award id is in the user's followed-award set.
pub fn is_award_favorite(award_id: &str) -> bool {
    FAV_AWARDS
        .read()
        .map(|g| g.contains(award_id))
        .unwrap_or(false)
}

/// Replace the followed-award set with a freshly-fetched id list and mirror
/// it to the per-user store (full replace — Tauri's
/// `v2_sync_cached_favorite_awards`). Blocking disk write; call off the UI
/// thread.
pub fn set_all_awards(ids: HashSet<String>) {
    if let Ok(guard) = STORE.lock() {
        if let Some(store) = guard.as_ref() {
            let disk: Vec<String> = ids.iter().cloned().collect();
            if let Err(e) = store.sync_favorite_awards(&disk) {
                log::warn!("[qbz-slint] favorites cache award sync failed: {e}");
            }
        }
    }
    if let Ok(mut guard) = FAV_AWARDS.write() {
        *guard = ids;
    }
}

/// Insert / remove a single award id (optimistic toggle) and mirror the
/// change to the per-user store so the follow survives a restart.
pub fn set_award(award_id: &str, favorite: bool) {
    if let Ok(mut guard) = FAV_AWARDS.write() {
        if favorite {
            guard.insert(award_id.to_string());
        } else {
            guard.remove(award_id);
        }
    }
    if let Ok(guard) = STORE.lock() {
        if let Some(store) = guard.as_ref() {
            let res = if favorite {
                store.add_favorite_award(award_id)
            } else {
                store.remove_favorite_award(award_id)
            };
            if let Err(e) = res {
                log::warn!("[qbz-slint] favorites cache award disk update failed: {e}");
            }
        }
    }
}
