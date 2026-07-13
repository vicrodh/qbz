//! B9 — offline Favorites "playable favorites" rail.
//!
//! While OFFLINE the Favorites view mounts the shared OfflinePlaceholder;
//! this module fills the rail under the placeholder copy with the favorite
//! tracks that are still playable. Three local id/metadata sources:
//!
//!   favorites      — `fav_cache` (disk-first seeded favorites_cache.db,
//!                    so the set is correct with zero network)
//!   offline cache  — qbz-offline-cache index rows with status READY
//!   library copies — library.db `local_tracks` rows with
//!                    `source = 'qobuz_download'` (the Local Library
//!                    "Offline" source-filter set)
//!
//! rail = favorites ∩ (ready ∪ qobuz_download). Metadata comes from the
//! index row when present (title/artist columns + the offline cover chain
//! via [`CachedTrackInfo::resolve_cover_path`]), else from the library row;
//! ids with no local metadata are skipped (count logged). Zero schema
//! changes — both stores are read as-is.
//!
//! A row click replaces the player queue with the WHOLE rail starting at
//! the clicked row; the tracks carry the real Qobuz id +
//! `source = "qobuz_download"` (the `local_queue_track` offline-copy
//! shape), so playback runs through the existing offline cache tier.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use qbz_models::QueueTrack;
use qbz_offline_cache::{CachedTrackInfo, OfflineCacheStatus};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::{AppWindow, OfflineFavoritesState, SlimItem};

type Runtime = std::sync::Arc<qbz_app::shell::AppRuntime<SlintAdapter>>;

/// The rail's queue, in display order — rebuilt by [`load`], consumed by
/// [`play`] (clicking a row plays the rail from that row, mirroring the
/// `play_tracks` track-list semantics).
static RAIL_QUEUE: LazyLock<Mutex<Vec<QueueTrack>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Worker-built, `Send` row data; the cover is pre-decoded to size on the
/// worker (`DecodedPixels` is `Send`) and the `slint::Image` is built on the
/// UI thread — the offline_manager pattern.
struct RowData {
    id: String,
    title: String,
    artist: String,
    cover: Option<crate::artwork::DecodedPixels>,
}

/// Rail rows (SlimCard) render their cover at 44px; decode to the rows tier
/// so the model never holds full-resolution sources.
const COVER_DECODE_SIZE: u32 = 96;

/// kHz normalization: Qobuz metadata carries kHz (96.0), library rows Hz
/// (96000.0) — same defensive rule as `local_queue_track`.
fn khz(rate: Option<f64>) -> Option<f64> {
    rate.map(|r| if r >= 1000.0 { r / 1000.0 } else { r })
}

/// QueueTrack from an offline-cache index row — mirrors
/// `playback::local_queue_track`'s offline-copy arm: the real Qobuz id,
/// `source = "qobuz_download"`, `is_local = true` (playback then routes
/// through the offline cache tier), `file://` artwork from the offline
/// cover chain.
fn index_queue_track(row: &CachedTrackInfo, cover: &str) -> QueueTrack {
    QueueTrack {
        id: row.track_id,
        title: row.title.clone(),
        version: None,
        artist: row.artist.clone(),
        album: row.album.clone().unwrap_or_default(),
        album_version: None,
        duration_secs: row.duration_secs,
        artwork_url: (!cover.is_empty()).then(|| format!("file://{cover}")),
        hires: row.bit_depth.map(|d| d > 16).unwrap_or(false),
        bit_depth: row.bit_depth,
        sample_rate: khz(row.sample_rate),
        is_local: true,
        album_id: row.album_id.clone(),
        artist_id: None,
        streamable: true,
        source: Some("qobuz_download".to_string()),
        parental_warning: false,
        source_item_id_hint: None,
        context_kind: None,
        context_id: None,
    }
}

/// Rebuild the rail. Fired by the Slint rail's `init` on every mount of
/// the Favorites offline placeholder (ADR-010 conditional mount, so each
/// entry re-reads the three local stores — all cheap local reads).
pub fn load(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        let favorites: HashSet<u64> = crate::fav_cache::all();

        // Offline-cache index: READY rows (most-recently-accessed first,
        // the DB's order) + the cache root for the cover chain.
        let (index_rows, cache_path): (Vec<CachedTrackInfo>, String) =
            match crate::offline::get().await {
                Some(off) => {
                    let cp = off.get_cache_path();
                    let guard = off.db.lock().await;
                    let rows: Vec<CachedTrackInfo> = guard
                        .as_ref()
                        .and_then(|db| db.get_all_tracks().ok())
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|t| matches!(t.status, OfflineCacheStatus::Ready))
                        .collect();
                    (rows, cp)
                }
                None => (Vec::new(), String::new()),
            };

        // library.db qobuz_download rows (the LocalLibrary "Offline"
        // source-filter set). `with_db` opens the file on the current
        // thread, so it runs inside spawn_blocking.
        let mut lib_rows: Vec<qbz_library::LocalTrack> = tokio::task::spawn_blocking(|| {
            crate::library_db::with_db(|db| db.get_qobuz_download_tracks()).unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        // Folder-cover backfill: the downloaders write cover.jpg next to
        // the file without always backfilling artwork_path.
        crate::playback::fill_missing_covers(&mut lib_rows);

        // Intersection, in display order: index rows first (recency), then
        // library-only copies. The index row wins metadata when an id is in
        // both (richer quality columns + the offline cover chain).
        let mut seen: HashSet<u64> = HashSet::new();
        let mut rows: Vec<RowData> = Vec::new();
        let mut queue: Vec<QueueTrack> = Vec::new();
        for t in &index_rows {
            if !favorites.contains(&t.track_id) || !seen.insert(t.track_id) {
                continue;
            }
            let cover = t.resolve_cover_path(&cache_path).unwrap_or_default();
            queue.push(index_queue_track(t, &cover));
            rows.push(RowData {
                id: t.track_id.to_string(),
                title: t.title.clone(),
                artist: t.artist.clone(),
                cover: crate::artwork::decode_local_pixels(
                    &cover,
                    crate::artwork::scaled_decode(COVER_DECODE_SIZE),
                ),
            });
        }
        for lt in &lib_rows {
            let Some(qid) = lt.qobuz_track_id.and_then(|v| u64::try_from(v).ok()) else {
                continue;
            };
            if !favorites.contains(&qid) || !seen.insert(qid) {
                continue;
            }
            queue.push(crate::playback::local_queue_track(lt));
            rows.push(RowData {
                id: qid.to_string(),
                title: lt.title.clone(),
                artist: lt.artist.clone(),
                cover: crate::artwork::decode_local_pixels(
                    lt.artwork_path.as_deref().unwrap_or_default(),
                    crate::artwork::scaled_decode(COVER_DECODE_SIZE),
                ),
            });
        }

        // Playable favorites with no local metadata row are skipped by
        // construction (membership comes FROM the metadata rows); keep the
        // count observable per the backlog contract.
        let ready_ids: HashSet<u64> = index_rows.iter().map(|t| t.track_id).collect();
        let lib_ids: HashSet<u64> = lib_rows
            .iter()
            .filter_map(|t| t.qobuz_track_id.and_then(|v| u64::try_from(v).ok()))
            .collect();
        let playable = favorites
            .iter()
            .filter(|id| ready_ids.contains(id) || lib_ids.contains(id))
            .count();
        let skipped = playable.saturating_sub(rows.len());
        log::info!(
            "[qbz-slint] offline favorites rail: {} playable of {} favorites ({} skipped — no local metadata)",
            rows.len(),
            favorites.len(),
            skipped
        );

        if let Ok(mut q) = RAIL_QUEUE.lock() {
            *q = queue;
        }
        let _ = weak.upgrade_in_event_loop(move |w| {
            let items: Vec<SlimItem> = rows
                .into_iter()
                .map(|rd| SlimItem {
                    id: rd.id.into(),
                    title: rd.title.into(),
                    subtitle: rd.artist.into(),
                    rank: "".into(),
                    artwork_url: "".into(),
                    artwork: rd
                        .cover
                        .map(|(px, pw, ph)| crate::artwork::pixels_to_image(&px, pw, ph))
                        .unwrap_or_default(),
                    following: false,
                })
                .collect();
            w.global::<OfflineFavoritesState>()
                .set_tracks(ModelRc::new(VecModel::from(items)));
        });
    });
}

/// Play the rail starting at the clicked track id: the rail becomes the
/// queue (replace), playback starts at the clicked row and continues down
/// the list through the existing offline-capable play path.
pub fn play(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    id: String,
) {
    let queue: Vec<QueueTrack> = RAIL_QUEUE
        .lock()
        .map(|q| q.clone())
        .unwrap_or_default();
    if queue.is_empty() {
        return;
    }
    let start = id
        .parse::<u64>()
        .ok()
        .and_then(|tid| queue.iter().position(|t| t.id == tid))
        .unwrap_or(0);
    let first_id = queue[start].id;
    handle.spawn(async move {
        runtime.core().set_queue(queue, Some(start)).await;
        crate::playback::after_track_change(&runtime, &weak, first_id).await;
        crate::playback::refresh_sidebar(true);
    });
}
