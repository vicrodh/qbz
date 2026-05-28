//! Slint offline-cache controller.
//!
//! Triggers caching (single / batch) and removal, and drives the per-row
//! offline status + the unlock padlock through a `CacheEventSink` that
//! pushes updates onto the visible track models (mirrors the favorite-state
//! machinery: `set_row_cache_status` / `set_row_unlocking` in `main.rs`).
//!
//! The heavy lifting (download pipeline, CMAF store, vault) lives in the
//! shared `qbz-offline-cache` crate; this is the thin Slint orchestration.

use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use qbz_app::shell::AppRuntime;
use qbz_offline_cache::{CacheEvent, CacheEventSink, OfflineCacheStatus, TrackCacheInfo};

use crate::adapter::SlintAdapter;
use crate::AppWindow;

type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Session-wide set of track ids that have a READY offline copy. Seeded from
/// the index.db on login (`load_cached_ids`) and kept in sync as downloads
/// complete / copies are removed. Read at row-build time to seed each row's
/// cache-status (mirrors `fav_cache`), so a cached track shows its check when
/// the view is revisited without re-querying the DB per row.
static CACHED_IDS: OnceLock<StdMutex<HashSet<u64>>> = OnceLock::new();

fn cached_ids() -> &'static StdMutex<HashSet<u64>> {
    CACHED_IDS.get_or_init(|| StdMutex::new(HashSet::new()))
}

/// True if `track_id` (string form, as the row carries it) has a ready offline
/// copy. Used to seed `TrackItem.cache-status` when building track lists.
pub fn is_cached(track_id: &str) -> bool {
    let Ok(id) = track_id.parse::<u64>() else {
        return false;
    };
    cached_ids().lock().map(|s| s.contains(&id)).unwrap_or(false)
}

fn mark_cached(track_id: u64, cached: bool) {
    if let Ok(mut set) = cached_ids().lock() {
        if cached {
            set.insert(track_id);
        } else {
            set.remove(&track_id);
        }
    }
}

/// Seed the ready-set from the active offline cache's index.db. Called once
/// after the offline cache is activated (login / session restore).
pub async fn load_cached_ids() {
    let Some(off) = crate::offline::get().await else {
        return;
    };
    let ids: Vec<u64> = {
        let guard = off.db.lock().await;
        match guard.as_ref() {
            Some(db) => db
                .get_all_tracks()
                .map(|tracks| {
                    tracks
                        .into_iter()
                        .filter(|t| matches!(t.status, OfflineCacheStatus::Ready))
                        .map(|t| t.track_id)
                        .collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        }
    };
    if let Ok(mut set) = cached_ids().lock() {
        *set = ids.into_iter().collect();
        log::info!("[qbz-slint] offline: seeded {} cached track ids", set.len());
    }
}

/// Build a sink that reflects cache + unlock events onto every visible row
/// matching the event's track id (and surfaces terminal toasts). Shared by
/// the cache trigger AND the play path (UnlockStart/End → padlock).
pub fn row_sink(weak: slint::Weak<AppWindow>) -> CacheEventSink {
    Arc::new(move |ev: CacheEvent| match ev {
        CacheEvent::Started { track_id } => {
            push_status(&weak, track_id, 2, 0.0);
        }
        CacheEvent::Progress {
            track_id,
            progress_percent,
            ..
        } => {
            let p = (progress_percent as f32 / 100.0).clamp(0.0, 1.0);
            push_status(&weak, track_id, 2, p);
        }
        CacheEvent::Completed { track_id, .. } => {
            mark_cached(track_id, true);
            push_status(&weak, track_id, 3, 1.0);
            crate::toast::success_weak(&weak, "Cached for offline");
        }
        CacheEvent::Processed { .. } => {
            // Post-processing done; status already 'ready' from Completed.
        }
        CacheEvent::Failed { track_id, error } => {
            log::warn!("[qbz-slint] offline cache failed for {track_id}: {error}");
            push_status(&weak, track_id, 4, 0.0);
            crate::toast::error_weak(&weak, "Offline caching failed");
        }
        CacheEvent::UnlockStart { track_id } => {
            push_unlocking(&weak, track_id, true);
        }
        CacheEvent::UnlockEnd { track_id, .. } => {
            push_unlocking(&weak, track_id, false);
        }
    })
}

fn push_status(weak: &slint::Weak<AppWindow>, track_id: u64, status: i32, progress: f32) {
    let id = track_id.to_string();
    let _ = weak.upgrade_in_event_loop(move |w| {
        crate::set_row_cache_status(&w, &id, status, progress);
    });
}

fn push_unlocking(weak: &slint::Weak<AppWindow>, track_id: u64, unlocking: bool) {
    let id = track_id.to_string();
    let _ = weak.upgrade_in_event_loop(move |w| {
        crate::set_row_unlocking(&w, &id, unlocking);
    });
}

/// Build the DB row metadata from a catalog track. Offline copies are always
/// fetched at the top quality tier.
fn track_cache_info(track: &qbz_models::Track) -> TrackCacheInfo {
    TrackCacheInfo {
        track_id: track.id,
        title: track.title.clone(),
        artist: track
            .performer
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        album: track.album.as_ref().map(|a| a.title.clone()),
        album_id: track.album.as_ref().map(|a| a.id.clone()),
        duration_secs: track.duration as u64,
        quality: "UltraHiRes".to_string(),
        bit_depth: track.maximum_bit_depth,
        sample_rate: track.maximum_sampling_rate,
    }
}

/// Cache a single track for offline playback. Fetches the track metadata,
/// pre-flights the cache limit, inserts the queued row, and spawns the
/// download (CMAF-first) with a row-updating sink.
pub fn cache_track(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    id: u64,
) {
    handle.spawn(async move {
        let Some(off) = crate::offline::get().await else {
            crate::toast::error_weak(&weak, "Log in to cache tracks offline");
            return;
        };
        let track = match runtime.core().get_track(id).await {
            Ok(t) => t,
            Err(e) => {
                log::error!("[qbz-slint] cache: get_track {id} failed: {e}");
                crate::toast::error_weak(&weak, "Couldn't load that track");
                return;
            }
        };
        let info = track_cache_info(&track);
        let file_path = off.track_file_path(id, "flac");
        let file_path_str = file_path.to_string_lossy().to_string();

        // Pre-flight the cache limit, then insert the queued row.
        {
            let limit = *off.limit_bytes.lock().await;
            let guard = off.db.lock().await;
            let Some(db) = guard.as_ref() else {
                return;
            };
            let root = std::path::PathBuf::from(off.get_cache_path());
            if let Err(e) = qbz_offline_cache::maintenance::check_cache_limit(db, &root, limit) {
                log::warn!("[qbz-slint] cache limit reached: {e}");
                crate::toast::error_weak(
                    &weak,
                    "Offline cache is full — free space or raise the limit",
                );
                return;
            }
            if let Err(e) = db.insert_track(&info, &file_path_str) {
                log::error!("[qbz-slint] cache insert {id} failed: {e}");
                return;
            }
        }

        // Mark the row queued immediately, then spawn the download.
        push_status(&weak, id, 1, 0.0);
        qbz_offline_cache::spawn_track_cache_download(
            id,
            file_path,
            runtime.core().client(),
            off.fetcher.clone(),
            off.db.clone(),
            off.get_cache_path(),
            off.library_db.clone(),
            row_sink(weak.clone()),
            off.cache_semaphore.clone(),
        );
    });
}

/// Cache a batch of already-fetched catalog tracks (favorites bulk action).
pub fn cache_tracks(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    tracks: Vec<qbz_models::Track>,
) {
    if tracks.is_empty() {
        return;
    }
    handle.spawn(async move {
        let Some(off) = crate::offline::get().await else {
            crate::toast::error_weak(&weak, "Log in to cache tracks offline");
            return;
        };
        // Pre-flight once for the whole batch (mirrors Tauri).
        {
            let limit = *off.limit_bytes.lock().await;
            let guard = off.db.lock().await;
            let Some(db) = guard.as_ref() else {
                return;
            };
            let root = std::path::PathBuf::from(off.get_cache_path());
            if let Err(e) = qbz_offline_cache::maintenance::check_cache_limit(db, &root, limit) {
                log::warn!("[qbz-slint] batch cache limit reached: {e}");
                crate::toast::error_weak(
                    &weak,
                    "Offline cache is full — free space or raise the limit",
                );
                return;
            }
        }
        let count = tracks.len();
        for track in &tracks {
            let id = track.id;
            let info = track_cache_info(track);
            let file_path = off.track_file_path(id, "flac");
            let file_path_str = file_path.to_string_lossy().to_string();
            {
                let guard = off.db.lock().await;
                let Some(db) = guard.as_ref() else {
                    return;
                };
                if db.insert_track(&info, &file_path_str).is_err() {
                    continue;
                }
            }
            push_status(&weak, id, 1, 0.0);
            qbz_offline_cache::spawn_track_cache_download(
                id,
                file_path,
                runtime.core().client(),
                off.fetcher.clone(),
                off.db.clone(),
                off.get_cache_path(),
                off.library_db.clone(),
                row_sink(weak.clone()),
                off.cache_semaphore.clone(),
            );
        }
        crate::toast::success_weak(
            &weak,
            format!("Caching {count} track{} offline…", if count == 1 { "" } else { "s" }),
        );
    });
}

/// Cache a whole album for offline playback: fetch its tracks, then batch them.
pub fn cache_album(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
) {
    let inner = handle.clone();
    handle.spawn(async move {
        let album = match runtime.core().get_album(&album_id).await {
            Ok(a) => a,
            Err(e) => {
                log::error!("[qbz-slint] cache album {album_id} failed: {e}");
                crate::toast::error_weak(&weak, "Couldn't load that album");
                return;
            }
        };
        let tracks: Vec<qbz_models::Track> = album
            .tracks
            .as_ref()
            .map(|c| c.items.clone())
            .unwrap_or_default();
        if tracks.is_empty() {
            crate::toast::error_weak(&weak, "This album has no playable tracks");
            return;
        }
        cache_tracks(runtime, weak, inner, tracks);
    });
}

/// Cache a whole playlist for offline playback: fetch its tracks, then batch.
pub fn cache_playlist(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    playlist_id: u64,
) {
    let inner = handle.clone();
    handle.spawn(async move {
        let pl = match runtime.core().get_playlist(playlist_id).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("[qbz-slint] cache playlist {playlist_id} failed: {e}");
                crate::toast::error_weak(&weak, "Couldn't load that playlist");
                return;
            }
        };
        let tracks: Vec<qbz_models::Track> = pl.tracks.map(|c| c.items).unwrap_or_default();
        if tracks.is_empty() {
            crate::toast::error_weak(&weak, "This playlist has no playable tracks");
            return;
        }
        cache_tracks(runtime, weak, inner, tracks);
    });
}

/// Remove a track's offline copy (DB row + on-disk bundle/file + library row).
pub fn remove_cached(
    _runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    id: u64,
) {
    handle.spawn(async move {
        let Some(off) = crate::offline::get().await else {
            return;
        };
        let removed_path = {
            let guard = off.db.lock().await;
            match guard.as_ref() {
                Some(db) => db.delete_track(id).ok().flatten(),
                None => return,
            }
        };
        if let Some(p) = removed_path {
            let path = std::path::Path::new(&p);
            // v2 bundles live in `tracks-cmaf/<id>/` — remove the whole dir.
            let looks_v2 = path
                .parent()
                .and_then(|pp| pp.parent())
                .and_then(|r| r.file_name())
                .and_then(|n| n.to_str())
                == Some("tracks-cmaf");
            if looks_v2 {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::remove_dir_all(dir);
                }
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
        {
            let guard = off.library_db.lock().await;
            if let Some(db) = guard.as_ref() {
                let _ = db.remove_qobuz_cached_track(id);
            }
        }
        mark_cached(id, false);
        push_status(&weak, id, 0, 0.0);
        crate::toast::success_weak(&weak, "Removed from offline");
    });
}
