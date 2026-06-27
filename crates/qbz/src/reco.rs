//! Per-user `RecoStore` runtime wrapper (mirrors `fav_cache.rs`).
//!
//! Holds the ported, headless `qbz_app` [`RecoStore`] behind a process-global
//! `Mutex<Option<…>>`; all access goes through typed helpers so the source
//! gating and the `spawn_blocking` discipline live in ONE place. Every helper
//! degrades to a no-op when no session is active (the store is `None`), so
//! callers never branch on "is reco enabled" — reco simply contributes
//! nothing until a session opens it.
//!
//! Lifecycle mirrors `fav_cache`: [`init_for_user`] on every session
//! activation (login, restore, offline entry), [`teardown`] on logout. The DB
//! file is shared with Tauri (`<base_dir>/reco/events.db`), so a user's
//! existing recommendation history carries across frontends.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use qbz_app::settings::reco_store::{
    HomeSeedLimits, HomeSeeds, RecoEventInput, RecoEventType, RecoItemType, RecoStore, TrainParams,
};

/// Per-user reco event store. `None` until a session (online or offline) is
/// activated; every helper is a no-op in that window.
static RECO: Mutex<Option<RecoStore>> = Mutex::new(None);

/// Open the per-user reco store (`<base_dir>/reco/events.db`). Best-effort: a
/// failure logs and leaves reco disabled (every helper then degrades to
/// no-op). Called on every session activation — login, restore, offline
/// entry — next to `fav_cache::init_for_user`.
pub fn init_for_user(base_dir: &Path) {
    match RecoStore::new_at(base_dir) {
        Ok(store) => {
            log::info!("[reco] event store opened for session");
            if let Ok(mut guard) = RECO.lock() {
                *guard = Some(store);
            }
        }
        Err(e) => log::warn!("[reco] init failed, reco disabled: {e}"),
    }
}

/// Drop the per-user store on logout (mirrors `fav_cache::teardown`).
pub fn teardown() {
    if let Ok(mut guard) = RECO.lock() {
        *guard = None;
    }
}

/// Read the home/Discover ID seeds. `None` when reco is disabled (no session)
/// or the read fails — callers fall back to their existing local source so a
/// cold reco store never empties a surface.
pub fn home_seeds(limits: HomeSeedLimits) -> Option<HomeSeeds> {
    let guard = RECO.lock().ok()?;
    let store = guard.as_ref()?;
    store.get_home_seeds(limits).ok()
}

// ---------------------------------------------------------------------------
// Play events (W2)
// ---------------------------------------------------------------------------

/// CRITICAL source gate: only Qobuz-catalog plays may enter reco. `None`
/// defaults to `"qobuz"` (the queue's own normalization in
/// `playback::record_recent`); only `local` / `plex` / `ephemeral` carry
/// non-catalog ids that don't resolve against Qobuz and would poison the home
/// seeds. A `qobuz_download` (a purchased Qobuz track) keeps a resolvable
/// Qobuz id, so it counts. Same exclusion the mix seeder uses (`mix.rs`).
pub fn is_qobuz_source(source: Option<&str>) -> bool {
    !matches!(source.unwrap_or("qobuz"), "local" | "plex" | "ephemeral")
}

/// Log a Qobuz play event. Blocking SQLite — call from `spawn_blocking`.
/// Returns whether it was logged (`false` = gated out as non-Qobuz, or reco
/// disabled). `genre_id` is `None`: a `QueueTrack` carries no genre, exactly
/// as in Tauri (genre is supplied later via the album-meta write-back).
pub fn log_play_gated(
    track_id: u64,
    album_id: Option<String>,
    artist_id: Option<u64>,
    source: Option<&str>,
) -> bool {
    if !is_qobuz_source(source) {
        return false;
    }
    if let Ok(guard) = RECO.lock() {
        if let Some(store) = guard.as_ref() {
            if let Err(e) = store.log_play_event(track_id, album_id, artist_id, None) {
                log::warn!("[reco] log_play failed: {e}");
            }
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Favorite events (W3) — log ONLY a successful ADD (make==true && network ok);
// the caller applies that gate. Logging an un-favorite or a failed add would
// corrupt the taste signal. Favorited items are inherently Qobuz catalog (the
// add_favorite API only succeeds for Qobuz ids), so no extra source gate.
// ---------------------------------------------------------------------------

/// Log a favorite of a Qobuz track.
pub fn log_favorite_track(track_id: u64, album_id: Option<String>, artist_id: Option<u64>) {
    if let Ok(guard) = RECO.lock() {
        if let Some(store) = guard.as_ref() {
            if let Err(e) = store.log_favorite_event(track_id, album_id, artist_id, None) {
                log::warn!("[reco] log_favorite_track failed: {e}");
            }
        }
    }
}

/// Log a favorite of a Qobuz album.
pub fn log_favorite_album(album_id: String, artist_id: Option<u64>) {
    insert_favorite(RecoItemType::Album, None, Some(album_id), artist_id);
}

/// Log a favorite of a Qobuz artist.
pub fn log_favorite_artist(artist_id: u64) {
    insert_favorite(RecoItemType::Artist, None, None, Some(artist_id));
}

fn insert_favorite(
    item_type: RecoItemType,
    track_id: Option<u64>,
    album_id: Option<String>,
    artist_id: Option<u64>,
) {
    if let Ok(guard) = RECO.lock() {
        if let Some(store) = guard.as_ref() {
            let ev = RecoEventInput {
                event_type: RecoEventType::Favorite,
                item_type,
                track_id,
                album_id,
                artist_id,
                playlist_id: None,
                genre_id: None,
            };
            if let Err(e) = store.insert_event(&ev) {
                log::warn!("[reco] log_favorite failed: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Playlist-add events (W4) — log the FULL requested Qobuz track ids (Tauri
// parity; the deduped/inserted subset under-weights train()). Qobuz tracks
// only — callers skip local-ref adds, whose ids don't resolve against Qobuz.
// `playlist_id` is the Qobuz playlist id for a Qobuz target, else None (a new
// or local playlist has no Qobuz id; the per-track signal is what scores).
// ---------------------------------------------------------------------------

/// Log a playlist-add, one PlaylistAdd/Track event per requested id.
pub fn log_playlist_add(playlist_id: Option<u64>, track_ids: Vec<u64>) {
    if track_ids.is_empty() {
        return;
    }
    if let Ok(guard) = RECO.lock() {
        if let Some(store) = guard.as_ref() {
            for tid in track_ids {
                let ev = RecoEventInput {
                    event_type: RecoEventType::PlaylistAdd,
                    item_type: RecoItemType::Track,
                    track_id: Some(tid),
                    album_id: None,
                    artist_id: None,
                    playlist_id,
                    genre_id: None,
                };
                if let Err(e) = store.insert_event(&ev) {
                    log::warn!("[reco] log_playlist_add failed: {e}");
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Surfaces (W7/W8)
// ---------------------------------------------------------------------------

/// Forgotten-favorite album ids (favorited, not played within `recency_days`).
/// `None` when reco is disabled or the read fails — the caller falls back to
/// its local heuristic so the Rediscover row never empties on a cold store.
pub fn forgotten_favorite_album_ids(limit: u32, recency_days: u32) -> Option<Vec<String>> {
    let guard = RECO.lock().ok()?;
    let store = guard.as_ref()?;
    store
        .get_forgotten_favorite_album_ids(limit, recency_days)
        .ok()
}

/// The reco-scored favorite album ids in taste order (highest first) when the
/// store is warm (trained); `None` when reco is cold/disabled so the caller
/// keeps its original ordering. Bounded by `limit`.
pub fn scored_favorite_album_ids(limit: u32) -> Option<Vec<String>> {
    let limits = HomeSeedLimits {
        recent_albums: 0,
        continue_tracks: 0,
        top_artists: 0,
        favorites: limit,
    };
    let seeds = home_seeds(limits)?;
    if seeds.favorite_album_ids.is_empty() {
        None
    } else {
        Some(seeds.favorite_album_ids)
    }
}

/// Backfill genres `(album_id, genre_id, genre_name)` onto reco events +
/// album-meta once albums are resolved. Best-effort, blocking SQLite — call
/// from `spawn_blocking`. Plays carry no genre, so this is what feeds
/// `get_top_genres`; idempotent (only fills still-NULL event genres).
pub fn backfill_album_genres(entries: Vec<(String, u64, String)>) {
    if entries.is_empty() {
        return;
    }
    if let Ok(guard) = RECO.lock() {
        if let Some(store) = guard.as_ref() {
            for (album_id, genre_id, genre_name) in entries {
                let _ = store.update_genre_for_album(&album_id, genre_id);
                let _ = store.set_album_genre_name(&album_id, &genre_name);
            }
        }
    }
}

/// Artist ids the user "knows" — played more than `play_threshold` times OR
/// favorited — to augment the discovery "skip artists I already know" filter
/// with the reco signal (plays + favorites, not plays alone). `None` when reco
/// is cold/disabled, so discovery falls back to the play_history set only.
pub fn known_artist_ids(play_threshold: u32) -> Option<HashSet<u64>> {
    let guard = RECO.lock().ok()?;
    let store = guard.as_ref()?;
    store.get_known_artist_ids(play_threshold).ok()
}

// ---------------------------------------------------------------------------
// Training (W5)
// ---------------------------------------------------------------------------

/// Recompute reco scores off-thread, fire-and-forget (mirrors Tauri's
/// non-awaited `trainScores` after login). Never blocks the caller; uses the
/// engine's default decay/weight params. No-op when reco is disabled.
pub fn train_async() {
    tokio::task::spawn_blocking(|| {
        if let Ok(mut guard) = RECO.lock() {
            if let Some(store) = guard.as_mut() {
                if let Err(e) = store.train(TrainParams::default()) {
                    log::warn!("[reco] train failed: {e}");
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_are_noop_when_uninitialized() {
        teardown(); // ensure no store is open
        // Logging with no open store must not panic and must report "not logged".
        assert!(!log_play_gated(123, None, None, Some("qobuz")));
        // Reading seeds with no open store yields None (caller falls back to local).
        assert!(home_seeds(HomeSeedLimits::default()).is_none());
    }

    #[test]
    fn qobuz_source_gate_excludes_local_plex_ephemeral() {
        assert!(is_qobuz_source(None)); // queue default = "qobuz"
        assert!(is_qobuz_source(Some("qobuz")));
        assert!(is_qobuz_source(Some("qobuz_download"))); // purchased Qobuz track, resolvable id
        assert!(!is_qobuz_source(Some("local")));
        assert!(!is_qobuz_source(Some("plex")));
        assert!(!is_qobuz_source(Some("ephemeral")));
    }
}
