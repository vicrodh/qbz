//! Playlist detail view controller.
//!
//! Fetches a playlist through `QbzCore`, maps it to the shared
//! TrackItem rows + header metadata, and applies it to `PlaylistState`.
//! Mirrors `mix.rs`: a cached track list backs play-all / per-track
//! play, and an artwork-jobs pass resolves the row covers + header
//! cover off-thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{QueueTrack, Track};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::local_playlist::{LoadedRow, RowItem};
use crate::{AppWindow, PlaylistState, TrackItem};

/// The currently-loaded playlist's QOBUZ tracks (server order), for
/// play-all / per-track play of pure-Qobuz details AND for resolving a
/// catalog id to its `playlist_track_id` (removal) — the full `Track`
/// keeps what the `TrackItem` row model drops.
static CURRENT: LazyLock<Mutex<Vec<Track>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// True while the open ONLINE Qobuz detail carries sidecar (local/Plex)
/// rows — the play/shuffle/per-row-play paths then route through
/// `local_playlist`'s merged queue snapshot instead of the Qobuz-only
/// `CURRENT` cache. Set in `apply`, cleared in `reset`.
static MIXED: AtomicBool = AtomicBool::new(false);

/// Whether the open ONLINE Qobuz detail is a mixed ("carrete") playlist.
pub fn is_mixed() -> bool {
    MIXED.load(Ordering::Relaxed)
}

thread_local! {
    /// The full, original-order row list — the canonical source the
    /// search + sort derive the visible list from. UI thread only.
    static FULL_ITEMS: std::cell::RefCell<Vec<TrackItem>> = std::cell::RefCell::new(Vec::new());
    /// Active in-page search query.
    static QUERY: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    /// Active sort: (field, ascending). field "default" = playlist order.
    static SORT: std::cell::RefCell<(String, bool)> =
        std::cell::RefCell::new(("default".to_string(), true));
    /// Custom order positions keyed `(track id, is_local)` — the same
    /// keying `playlist_track_custom_order` uses (Seam E), so local/plex
    /// rows of a mixed playlist can hold an order without colliding with
    /// Qobuz catalog ids. Empty until the custom sort is entered
    /// (loaded/initialized from library.db).
    static CUSTOM_ORDER: std::cell::RefCell<std::collections::HashMap<(u64, bool), i32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The `(track_id, is_local)` custom-order key of a row, derived from the
/// row's source the way Tauri derives it from `isLocal` (§1.3):
/// - Qobuz rows -> `(catalog id, false)`.
/// - Local sidecar rows -> `(library row id, true)` — same value Tauri
///   stores (`local_tracks.id`, `is_local=1`).
/// - Plex rows -> `(synthetic display id, true)` — Tauri's abs(display
///   id):true wart replicated for table compatibility (E5); note the two
///   frontends derive DIFFERENT display ids for the same rating key, so
///   plex entries are read tolerantly and simply fall to the end when
///   unmapped (E6, Slint end-of-list rule).
/// - Rows without a stable numeric id (`plex:<key>` unresolved,
///   `file:`/`broken:` fallbacks) -> None: excluded from the order,
///   sorted to the end.
fn custom_key(item: &TrackItem) -> Option<(u64, bool)> {
    let is_local = matches!(item.source.as_str(), "local" | "plex");
    item.id.parse::<u64>().ok().map(|id| (id, is_local))
}

/// "m:ss" / "h:mm:ss" -> seconds, for duration sorting.
fn duration_secs(s: &str) -> u32 {
    s.split(':')
        .filter_map(|p| p.parse::<u32>().ok())
        .fold(0, |acc, n| acc * 60 + n)
}

/// Re-derive the visible track list from FULL_ITEMS by applying the
/// active search filter, then the active sort. Runs on the event loop.
fn refresh_view(window: &AppWindow) {
    let needle = QUERY.with(|q| q.borrow().trim().to_lowercase());
    let (field, asc) = SORT.with(|s| s.borrow().clone());
    let mut view: Vec<TrackItem> = FULL_ITEMS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|t| {
                needle.is_empty()
                    || t.title.as_str().to_lowercase().contains(&needle)
                    || t.artist.as_str().to_lowercase().contains(&needle)
                    || t.album.as_str().to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    });
    if field == "custom" {
        // Order by the local custom positions; rows not in the map sort
        // to the END in their natural relative order (E6 — deliberate
        // Slint rule; Tauri's addedIndex-0 fallback floats them to the
        // top by accident).
        let order = CUSTOM_ORDER.with(|c| c.borrow().clone());
        view.sort_by_key(|t| {
            custom_key(t)
                .and_then(|k| order.get(&k).copied())
                .unwrap_or(i32::MAX)
        });
    } else if field != "default" {
        view.sort_by(|a, b| {
            let ord = match field.as_str() {
                "title" => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
                "artist" => a.artist.to_lowercase().cmp(&b.artist.to_lowercase()),
                "album" => a.album.to_lowercase().cmp(&b.album.to_lowercase()),
                "duration" => {
                    duration_secs(a.duration.as_str()).cmp(&duration_secs(b.duration.as_str()))
                }
                _ => std::cmp::Ordering::Equal,
            };
            if asc {
                ord
            } else {
                ord.reverse()
            }
        });
    }
    window
        .global::<PlaylistState>()
        .set_tracks(ModelRc::new(VecModel::from(view)));
}

/// Resolve a row's artwork into BOTH the stable FULL_ITEMS list (so a
/// later sort/filter keeps it — they rebuild from FULL_ITEMS) and the
/// visible model row, matched by id (the displayed order may differ
/// from FULL_ITEMS after a sort). Called by the artwork pipeline.
pub fn set_track_artwork(window: &AppWindow, full_index: usize, image: slint::Image) {
    use slint::Model;
    let id = FULL_ITEMS.with(|c| {
        let mut b = c.borrow_mut();
        b.get_mut(full_index).map(|it| {
            it.artwork = image.clone();
            it.id.clone()
        })
    });
    let Some(id) = id else { return };
    let model = window.global::<PlaylistState>().get_tracks();
    for i in 0..model.row_count() {
        if let Some(mut it) = model.row_data(i) {
            if it.id == id {
                it.artwork = image;
                model.set_row_data(i, it);
                break;
            }
        }
    }
}

/// Update the search query and re-render.
pub fn filter_tracks(window: &AppWindow, query: &str) {
    QUERY.with(|q| *q.borrow_mut() = query.to_string());
    refresh_view(window);
}

/// Set the sort field. Re-selecting the active field toggles asc/desc;
/// "default" restores playlist order. Mirrors Tauri's behaviour.
pub fn set_sort(window: &AppWindow, field: &str) {
    SORT.with(|s| {
        let mut cur = s.borrow_mut();
        if field == "default" || field == "custom" {
            *cur = (field.to_string(), true);
        } else if cur.0 == field {
            cur.1 = !cur.1;
        } else {
            *cur = (field.to_string(), true);
        }
    });
    let (field, asc) = SORT.with(|s| s.borrow().clone());
    let state = window.global::<PlaylistState>();
    state.set_sort_field(field.into());
    state.set_sort_asc(asc);
    refresh_view(window);
}

// ==================== Custom (manual) order ====================

/// Custom-order SEED keys on first entry: Qobuz rows in natural order,
/// then LOCAL sidecar rows — Tauri parity (§1.3
/// `initCustomOrderFromCurrentTracks` covers `tracks` + `localTracks`
/// only; plex rows are NOT seeded — they only enter the table through an
/// explicit reorder write). Offline-copy sidecar rows render source
/// "qobuz" with the REAL catalog id here (Slint's queue-id row identity),
/// so they seed as Qobuz keys — a documented divergence from Tauri's
/// `(local_tracks.id, 1)`; unmapped rows just sort to the end (E6).
pub fn custom_seed_keys() -> Vec<(i64, bool)> {
    FULL_ITEMS.with(|cell| {
        let items = cell.borrow();
        let mut out: Vec<(i64, bool)> = Vec::new();
        for item in items.iter().filter(|t| t.source.as_str() == "qobuz") {
            if let Ok(id) = item.id.parse::<i64>() {
                out.push((id, false));
            }
        }
        for item in items.iter().filter(|t| t.source.as_str() == "local") {
            if let Ok(id) = item.id.parse::<i64>() {
                out.push((id, true));
            }
        }
        out
    })
}

/// The FULL (unfiltered, natural-order) row ids as strings. The LOCAL
/// detail's reorder works over these — its Plex rows (`plex:<key>`) don't
/// parse as u64, so the keyed custom-order helpers can't serve it. UI thread.
pub fn full_item_ids() -> Vec<String> {
    FULL_ITEMS.with(|cell| cell.borrow().iter().map(|t| t.id.to_string()).collect())
}

/// Swap the FULL_ITEMS entries at natural-order indexes `a` and `b`, then
/// re-render through the active search/sort. The LOCAL detail's optimistic
/// reorder move (B2) — under its "default" sort the visible order IS the
/// FULL order, so the swap shows immediately. UI thread.
pub fn swap_full_items(window: &AppWindow, a: usize, b: usize) {
    FULL_ITEMS.with(|cell| {
        let mut items = cell.borrow_mut();
        if a < items.len() && b < items.len() {
            items.swap(a, b);
        }
    });
    refresh_view(window);
}

/// Load the playlist's custom order from library.db, seeding it from
/// `seed` keys (see [`custom_seed_keys`]) if none exists. Returns
/// `((track_id, is_local), position)` rows — `is_local` is kept (Seam E;
/// the old reader dropped it, which collides once mixed rows exist).
/// Blocking — run on a worker thread.
pub fn load_or_init_custom(
    playlist_id: u64,
    seed: Vec<(i64, bool)>,
) -> Vec<((u64, bool), i32)> {
    crate::library_db::with_db(|db| {
        let has = db.has_playlist_custom_order(playlist_id)?;
        if !has {
            db.init_playlist_custom_order(playlist_id, &seed)?;
        }
        db.get_playlist_custom_order(playlist_id)
    })
    .unwrap_or_default()
    .into_iter()
    .map(|(id, is_local, pos)| ((id as u64, is_local), pos))
    .collect()
}

/// Persist the full custom order (DELETE + INSERT — self-healing),
/// `is_local` per row (Seam E — bidirectionally compatible with Tauri's
/// `playlist_track_custom_order`). Blocking.
pub fn persist_custom(playlist_id: u64, orders: Vec<(u64, bool, i32)>) {
    let rows: Vec<(i64, bool, i32)> = orders
        .into_iter()
        .map(|(id, is_local, pos)| (id as i64, is_local, pos))
        .collect();
    crate::library_db::with_db(|db| db.set_playlist_custom_order(playlist_id, &rows));
}

/// Store a freshly-loaded custom order + re-render. UI thread.
pub fn apply_custom_order(window: &AppWindow, orders: Vec<((u64, bool), i32)>) {
    CUSTOM_ORDER.with(|c| {
        let mut m = c.borrow_mut();
        m.clear();
        for (key, pos) in orders {
            m.insert(key, pos);
        }
    });
    refresh_view(window);
}

/// Move a track one slot up/down in the custom order. Rebuilds the
/// whole order with clean 0..N-1 positions (self-healing), re-renders,
/// and returns the new `(id, is_local, position)` rows to persist.
/// Like Tauri's `moveTrack` rewrite, the persisted set DOES include plex
/// rows (typed is_local=1 — the E5 wart, see [`custom_key`]); rows with
/// no stable key can't participate and stay at the end. UI thread.
pub fn move_track(window: &AppWindow, track_id: &str, up: bool) -> Vec<(u64, bool, i32)> {
    let target = FULL_ITEMS.with(|cell| {
        cell.borrow()
            .iter()
            .find(|t| t.id.as_str() == track_id)
            .and_then(custom_key)
    });
    let Some(target) = target else {
        return Vec::new();
    };
    // Current visible custom order of keyed rows.
    let order = CUSTOM_ORDER.with(|c| c.borrow().clone());
    let mut keys: Vec<(u64, bool)> =
        FULL_ITEMS.with(|cell| cell.borrow().iter().filter_map(custom_key).collect());
    keys.sort_by_key(|key| order.get(key).copied().unwrap_or(i32::MAX));
    let Some(idx) = keys.iter().position(|&key| key == target) else {
        return Vec::new();
    };
    let swap = if up {
        if idx == 0 {
            return Vec::new();
        }
        idx - 1
    } else {
        if idx + 1 >= keys.len() {
            return Vec::new();
        }
        idx + 1
    };
    keys.swap(idx, swap);
    // Rebuild contiguous positions.
    let orders: Vec<(u64, bool, i32)> = keys
        .iter()
        .enumerate()
        .map(|(i, &(id, is_local))| (id, is_local, i as i32))
        .collect();
    CUSTOM_ORDER.with(|c| {
        let mut m = c.borrow_mut();
        m.clear();
        for &(id, is_local, pos) in &orders {
            m.insert((id, is_local), pos);
        }
    });
    refresh_view(window);
    orders
}

/// Plain, `Send` playlist data produced on the worker thread.
pub struct PlaylistData {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub description: String,
    pub description_short: String,
    pub cover_url: String,
    /// Local custom artwork path (from playlist_settings), if the user
    /// set one — overrides the collage / server image.
    pub custom_artwork_path: Option<String>,
    /// The MERGED row list (Qobuz tracks interleaved with the local/Plex
    /// sidecar rows at their absolute slots — Seam A) in display order.
    /// Pure-Qobuz playlists are simply all `RowItem::Qobuz`.
    pub rows: Vec<LoadedRow>,
}

/// Tauri's absolute-slot interleave — the `displayTracks` contract
/// (spec §1.2): sidecar rows claim their STORED positions as slots in the
/// merged list; Qobuz tracks fill the remaining slots in server order;
/// `total = max(sum of rows, max stored position + 1)` so stale high slots
/// still render (E3); unclaimed slots with no Qobuz track left are skipped
/// (never a blank); leftover Qobuz tracks append. Same-slot collisions emit
/// ALL claimants — local first, then plex, in stable claim order — instead
/// of Tauri's Map collapse (E1/E2 fix-forward; healing repairs the stored
/// data separately). Display numbering is the emit order (contiguous).
pub(crate) fn interleave_rows(qobuz: Vec<Track>, sidecar: Vec<LoadedRow>) -> Vec<LoadedRow> {
    let qobuz_to_row = |(i, t): (usize, Track)| LoadedRow {
        position: i as i32,
        item: RowItem::Qobuz(Box::new(t)),
    };
    if sidecar.is_empty() {
        return qobuz.into_iter().enumerate().map(qobuz_to_row).collect();
    }
    let sidecar_len = sidecar.len();
    let mut max_pos: i32 = -1;
    let mut buckets: std::collections::HashMap<i32, Vec<LoadedRow>> =
        std::collections::HashMap::new();
    for row in sidecar {
        // Corrupt negative positions claim slot 0 rather than vanishing.
        let pos = row.position.max(0);
        max_pos = max_pos.max(pos);
        buckets.entry(pos).or_default().push(row);
    }
    let total = (qobuz.len() + sidecar_len).max((max_pos + 1) as usize);
    let mut out: Vec<LoadedRow> = Vec::with_capacity(qobuz.len() + sidecar_len);
    let mut qobuz_iter = qobuz.into_iter();
    for pos in 0..total as i32 {
        if let Some(rows) = buckets.remove(&pos) {
            out.extend(rows);
        } else if let Some(track) = qobuz_iter.next() {
            out.push(LoadedRow {
                position: pos,
                item: RowItem::Qobuz(Box::new(track)),
            });
        }
        // else: an unclaimed slot past the Qobuz tracks — a gap, skipped.
    }
    for track in qobuz_iter {
        out.push(LoadedRow {
            position: 0,
            item: RowItem::Qobuz(Box::new(track)),
        });
    }
    // Positions in the merged output are the contiguous display slots; the
    // stored sidecar positions did their job claiming the order.
    for (i, row) in out.iter_mut().enumerate() {
        row.position = i as i32;
    }
    out
}

pub async fn load<A>(runtime: &AppRuntime<A>, playlist_id: u64) -> Option<PlaylistData>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let pl = match runtime.core().get_playlist(playlist_id).await {
        Ok(pl) => pl,
        Err(e) => {
            log::error!("[qbz-slint] load playlist {playlist_id} failed: {e}");
            return None;
        }
    };
    let tracks = pl.tracks.map(|c| c.items).unwrap_or_default();
    // Header cover: the server-composed playlist image, else the first
    // track's album cover.
    let cover_url = pl
        .images
        .as_ref()
        .and_then(|imgs| imgs.first().cloned())
        .or_else(|| {
            tracks
                .first()
                .and_then(|t| t.album.as_ref())
                .and_then(|a| a.image.best().cloned())
        })
        .unwrap_or_default();
    // Local custom artwork (shared with the Tauri app via library.db).
    let custom_artwork_path = tokio::task::spawn_blocking(move || {
        crate::library_db::with_db(|db| db.get_playlist_settings(playlist_id))
            .flatten()
            .and_then(|s| s.custom_artwork_path)
            .filter(|p| !p.is_empty())
    })
    .await
    .ok()
    .flatten();
    let description = pl
        .description
        .map(|d| crate::strip_html::strip_html(&d))
        .unwrap_or_default();
    // B7 producer (membership): this fetch already returned the FULL track
    // list — full-replace the playlist's snapshot membership, detached (the
    // render never waits). No-ops for playlists outside the user's listed
    // set (no snapshot header), so merely-viewed public playlists stay out.
    // Qobuz membership only — sidecar rows never enter the snapshot (E10).
    crate::playlist_snapshot::record_detail_detached(
        playlist_id,
        pl.name.clone(),
        pl.owner.name.clone(),
        tracks.iter().map(|t| t.id).collect(),
    );
    // Seam A (merge-on-load): read the sidecar rows (healing + Plex cache
    // resolve inside the shared reader) and interleave them with the Qobuz
    // tracks at their absolute slots. Plex rows are always included online
    // (availability is connectivity-based, E13).
    let qobuz_count = tracks.len() as u32;
    let sidecar = tokio::task::spawn_blocking(move || {
        crate::local_playlist::read_sidecar_rows_blocking(playlist_id, qobuz_count, true)
    })
    .await
    .unwrap_or_default();
    let rows = interleave_rows(tracks, sidecar);
    Some(PlaylistData {
        id: pl.id.to_string(),
        name: pl.name,
        owner: pl.owner.name,
        description: description.clone(),
        description_short: truncate_words(&description, 160),
        cover_url,
        custom_artwork_path,
        rows,
    })
}

/// Word-boundary truncation for the 2-line header description (the
/// full text lives in the Read-more modal).
fn truncate_words(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max).collect();
    let cut = truncated.rfind(' ').unwrap_or(truncated.len());
    format!("{}…", truncated[..cut].trim_end())
}

fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub(crate) fn to_item(track: &Track) -> TrackItem {
    let mut title = track.title.clone();
    if let Some(v) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({v})");
    }
    // Blacklist key: the track's performer id (pure-Qobuz playlist rows;
    // local / Plex rows go through local_playlist::row_item, never stamped).
    let performer_id = track
        .performer
        .as_ref()
        .map(|p| p.id.to_string())
        .unwrap_or_default();
    TrackItem {
        is_blacklisted: crate::artist_blacklist::stamp_row("qobuz", &[performer_id.as_str()]),
        id: track.id.to_string().into(),
        number: "".into(),
        title: title.into(),
        artist: track
            .performer
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default()
            .into(),
        album: track
            .album
            .as_ref()
            .map(|a| a.title.clone())
            .unwrap_or_default()
            .into(),
        duration: mmss(track.duration).into(),
        quality_tier: match track.maximum_bit_depth {
            Some(d) if d >= 24 => "hires",
            Some(_) => "cd",
            None => "",
        }
        .into(),
        quality_detail: crate::quality::detail(
            track.maximum_bit_depth,
            track.maximum_sampling_rate,
        )
        .into(),
        explicit: track.parental_warning,
        selected: false,
        // Smallest variant — these are 40px row thumbnails; best()
        // would download mega/large covers (2000-row perf killer).
        artwork_url: track
            .album
            .as_ref()
            .and_then(|a| a.image.smallest().cloned())
            .unwrap_or_default()
            .into(),
        artwork: slint::Image::default(),
        is_favorite: crate::fav_cache::is_favorite(&track.id.to_string()),
        artist_id: track
            .performer
            .as_ref()
            .map(|p| p.id.to_string())
            .unwrap_or_default()
            .into(),
        album_id: track
            .album
            .as_ref()
            .map(|a| a.id.clone())
            .unwrap_or_default()
            .into(),
        removing: false,
        cache_status: if crate::offline_cache::is_cached(&track.id.to_string()) { 3 } else { 0 },
        cache_progress: 0.0,
        source: "qobuz".into(),
        unlocking: false,
        // Disc grouping is album-detail only; flat lists carry none.
        disc_header_number: 0,
    }
}

pub fn reset(window: &AppWindow) {
    FULL_ITEMS.with(|cell| cell.borrow_mut().clear());
    QUERY.with(|q| q.borrow_mut().clear());
    SORT.with(|s| *s.borrow_mut() = ("default".to_string(), true));
    CUSTOM_ORDER.with(|c| c.borrow_mut().clear());
    MIXED.store(false, Ordering::Relaxed);
    // Drop the previous detail's queue snapshot — the local/offline/mixed
    // applies repopulate it after this shared reset.
    crate::local_playlist::clear_open_snapshot();
    let state = window.global::<PlaylistState>();
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_track_count(0);
    state.set_total_duration("".into());
    state.set_cover(slint::Image::default());
    state.set_sort_field("default".into());
    state.set_sort_asc(true);
    // Local-playlist flags reset on every navigation; the local detail
    // path re-sets them after this shared reset. The offline-subset flag
    // (D11.a mixed-playlist offline rendering) resets the same way.
    state.set_is_local(false);
    state.set_offline_only(false);
    state.set_offline_subset(false);
    state.set_loading(true);
}

pub fn apply(window: &AppWindow, data: PlaylistData) {
    // One row-identity contract with the LOCAL/offline details (E11):
    // Qobuz rows keep catalog ids, local rows their library row id, plex
    // rows their synthetic id / `plex:<key>` — built by the shared
    // `build_row_models` so selection, drag, picker refs and scroll
    // restore behave identically across the connectivity flip.
    let (queue, items, positions) = crate::local_playlist::build_row_models(&data.rows);
    let qobuz_tracks: Vec<Track> = data
        .rows
        .iter()
        .filter_map(|r| match &r.item {
            RowItem::Qobuz(track) => Some((**track).clone()),
            _ => None,
        })
        .collect();
    let mixed = data.rows.len() != qobuz_tracks.len();
    // Merged header counts (Tauri shows qobuz + local + plex combined).
    let count = items.len() as i32;
    let duration = crate::local_playlist::total_duration_label(&data.rows);
    FULL_ITEMS.with(|cell| *cell.borrow_mut() = items.clone());
    if let Ok(mut cur) = CURRENT.lock() {
        *cur = qobuz_tracks;
    }
    MIXED.store(mixed, Ordering::Relaxed);
    if mixed {
        // Seam B: the mixed detail plays through local_playlist's queue
        // snapshot (source-aware QueueTracks); pure-Qobuz details keep the
        // CURRENT-cache path unchanged.
        crate::local_playlist::set_open_mixed_snapshot(&data.id, queue, positions);
    } else {
        crate::local_playlist::clear_open_snapshot();
    }
    // Custom artwork overrides the collage / server image. Load the
    // local file directly (it lives in the artwork cache on disk).
    let custom = data
        .custom_artwork_path
        .as_ref()
        .filter(|p| std::path::Path::new(p).exists())
        .and_then(|p| slint::Image::load_from_path(std::path::Path::new(p)).ok());
    let state = window.global::<PlaylistState>();
    state.set_id(data.id.into());
    state.set_name(data.name.into());
    state.set_owner(data.owner.into());
    state.set_description(data.description.into());
    state.set_description_short(data.description_short.into());
    if let Some(img) = custom {
        state.set_cover(img);
        state.set_cover_url(data.custom_artwork_path.clone().unwrap_or_default().into());
        state.set_has_custom(true);
    } else {
        state.set_cover_url(data.cover_url.into());
        state.set_has_custom(false);
    }
    state.set_tracks(ModelRc::new(VecModel::from(items)));
    state.set_track_count(count);
    state.set_total_duration(duration.into());
    state.set_loading(false);
}

/// Apply a prebuilt row list (the LOCAL-playlist detail path, which
/// resolves its rows from the local repo instead of a Qobuz fetch) into the
/// SAME per-view statics this module owns, so in-page search / sort /
/// multi-select / the artwork pipeline all work unchanged. Clears the Qobuz
/// `CURRENT` track cache — local playlists drive playback from
/// `crate::local_playlist`'s own queue snapshot. UI thread.
pub fn apply_local_items(window: &AppWindow, items: Vec<TrackItem>) {
    FULL_ITEMS.with(|cell| *cell.borrow_mut() = items.clone());
    if let Ok(mut cur) = CURRENT.lock() {
        cur.clear();
    }
    let state = window.global::<PlaylistState>();
    state.set_track_count(items.len() as i32);
    state.set_tracks(ModelRc::new(VecModel::from(items)));
    state.set_loading(false);
}

/// Artwork jobs for the loaded playlist — one per row plus the header
/// cover (resolved into PlaylistState.cover). Returns (http, local-file,
/// plex) job sets: Qobuz rows carry http URLs, local sidecar rows file
/// paths, plex rows raw `/library/...` thumb paths — the same loader split
/// the LOCAL detail uses.
pub fn artwork_jobs(data: &PlaylistData) -> (Vec<ArtworkJob>, Vec<ArtworkJob>, Vec<ArtworkJob>) {
    let (mut http, local, plex) = crate::local_playlist::artwork_jobs(&data.rows);
    // Skip the server-cover job when a local custom artwork is set
    // (it's already loaded in apply and cover_url holds a file path).
    if data.custom_artwork_path.is_none() && !data.cover_url.is_empty() {
        http.push(ArtworkJob {
            url: data.cover_url.clone(),
            target: ArtworkTarget::PlaylistCover,
        });
    }
    (http, local, plex)
}

pub fn current_tracks() -> Vec<Track> {
    CURRENT.lock().map(|c| c.clone()).unwrap_or_default()
}

/// The current playlist tracks in a fresh random order (Shuffle).
pub fn shuffled_tracks() -> Vec<Track> {
    let mut tracks = current_tracks();
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        | 1;
    for i in (1..tracks.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % (i as u64 + 1)) as usize;
        tracks.swap(i, j);
    }
    tracks
}

// Per-row "play from here" (visible order + clicked index) is handled
// centrally in `playback::play_track_in_context` / `order_by_visible`, which
// covers every tracklist view uniformly — see that function.

// ==================== Custom artwork ====================

/// Copy `src` into the artwork cache and store it as this playlist's
/// custom artwork (shared with Tauri via library.db). Returns the
/// stored path. Blocking — run on a worker thread.
pub fn set_custom_artwork(playlist_id: u64, src: &str) -> Option<String> {
    let cache = crate::library_db::artwork_cache_dir()?;
    std::fs::create_dir_all(&cache).ok()?;
    let ext = std::path::Path::new(src)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest = cache.join(format!("playlist_{playlist_id}_{ts}.{ext}"));
    if let Err(e) = std::fs::copy(src, &dest) {
        log::error!("[qbz-slint] copy custom artwork failed: {e}");
        return None;
    }
    let dest_str = dest.to_string_lossy().to_string();
    crate::library_db::with_db(|db| db.update_playlist_artwork(playlist_id, Some(&dest_str)))?;
    Some(dest_str)
}

/// Clear this playlist's custom artwork. Blocking.
pub fn clear_custom_artwork(playlist_id: u64) {
    crate::library_db::with_db(|db| db.update_playlist_artwork(playlist_id, None));
}

// ==================== Multi-select edit mode ====================

use slint::Model;

/// Recount selected rows into PlaylistState.selected-count.
pub fn recount_selected(window: &AppWindow) {
    let model = window.global::<PlaylistState>().get_tracks();
    let count = (0..model.row_count())
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count() as i32;
    window.global::<PlaylistState>().set_selected_count(count);
}

/// Enter/leave edit mode. Leaving clears any selection.
pub fn set_multi_select(window: &AppWindow, on: bool) {
    if !on {
        clear_selection(window);
    }
    window.global::<PlaylistState>().set_multi_select_mode(on);
}

/// Clear the selection WITHOUT leaving multi-select mode — the bulk
/// queueing actions keep the mode active (LocalLibrary bulk precedent).
pub fn clear_selection(window: &AppWindow) {
    let state = window.global::<PlaylistState>();
    let model = state.get_tracks();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.selected {
                item.selected = false;
                model.set_row_data(i, item);
            }
        }
    }
    state.set_selected_count(0);
}

/// Toggle select-all: select every row, or clear if all are selected.
pub fn select_all(window: &AppWindow) {
    let model = window.global::<PlaylistState>().get_tracks();
    let total = model.row_count();
    let selected = (0..total)
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    let target = selected != total; // if not all selected -> select all
    for i in 0..total {
        if let Some(mut item) = model.row_data(i) {
            if item.selected != target {
                item.selected = target;
                model.set_row_data(i, item);
            }
        }
    }
    recount_selected(window);
}

// ==================== Namespace-split removal (Seam D) ====================

/// A row reference for removal: the display id + the row's source — the
/// id namespace is source-dependent (catalog id / library row id /
/// synthetic plex id / `plex:<key>`). Built from the selection (bulk) or a
/// single row (the per-row "Remove from playlist" menu entry rides this
/// same seam when it lands).
pub struct SelectedRow {
    pub id: String,
    pub source: String,
}

/// The currently-selected rows with their sources.
pub fn selected_rows(window: &AppWindow) -> Vec<SelectedRow> {
    let model = window.global::<PlaylistState>().get_tracks();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|t| t.selected)
        .map(|t| SelectedRow {
            id: t.id.to_string(),
            source: t.source.to_string(),
        })
        .collect()
}

/// A single row (id + source) by display id — the per-row "Remove from
/// playlist" menu entry rides the same namespace-split seam as the bulk
/// selection, with a one-row set. UI thread.
pub fn row_for_id(id: &str) -> Option<SelectedRow> {
    FULL_ITEMS.with(|cell| {
        cell.borrow()
            .iter()
            .find(|item| item.id.as_str() == id)
            .map(|item| SelectedRow {
                id: item.id.to_string(),
                source: item.source.to_string(),
            })
    })
}

/// The selected rows as ready-to-enqueue, SOURCE-AWARE QueueTracks in
/// visible order — the bulk Play next / Add to queue (spec §1.5). Rows of
/// a snapshot-backed detail (local / offline subset / online mixed)
/// resolve through `local_playlist`'s merged queue snapshot, which keeps
/// each row's source (local/plex/cached — the T2 fix-forward: Tauri's
/// bulk path rebuilds catalog tracks and drops `source`); pure-Qobuz
/// details resolve through the loaded `CURRENT` Track cache. Unplayable
/// rows (file:/broken:/unresolved) drop out. UI thread.
pub fn selected_queue_tracks(window: &AppWindow) -> Vec<QueueTrack> {
    let model = window.global::<PlaylistState>().get_tracks();
    let qobuz = current_tracks();
    let mut out: Vec<QueueTrack> = Vec::new();
    for i in 0..model.row_count() {
        let Some(item) = model.row_data(i) else {
            continue;
        };
        if !item.selected {
            continue;
        }
        let id = item.id.to_string();
        // Snapshot first (covers ALL sources of the mixed/local detail;
        // empty for pure-Qobuz details, see clear_open_snapshot).
        if let Some(qt) = crate::local_playlist::queue_track_for_row(&id) {
            out.push(qt);
            continue;
        }
        // Pure-Qobuz detail: build from the loaded Track cache.
        if let Some(track) = id
            .parse::<u64>()
            .ok()
            .and_then(|tid| qobuz.iter().find(|t| t.id == tid))
        {
            let (album_id, album_title, album_artwork) = track
                .album
                .as_ref()
                .map(|a| {
                    (
                        a.id.clone(),
                        a.title.clone(),
                        a.image.best().cloned().unwrap_or_default(),
                    )
                })
                .unwrap_or_default();
            let album_artist = track
                .performer
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default();
            out.push(crate::playback::make_queue_track(
                track,
                &album_id,
                &album_title,
                &album_artist,
                &album_artwork,
            ));
        } else {
            log::warn!("[qbz-slint] bulk queue: row {id} not resolvable — skipped");
        }
    }
    out
}

/// The removal split of a row set, by id namespace.
#[derive(Default)]
pub struct RemovalSplit {
    /// Qobuz rows resolved to `playlist_track_id`s — what the
    /// `playlist/deleteTracks` API actually takes. ALL instances of a
    /// selected catalog id resolve (duplicates removed together — Tauri
    /// behavior).
    pub playlist_track_ids: Vec<u64>,
    /// Local sidecar rows: `local_tracks.id` (the row's display id).
    pub local_track_ids: Vec<i64>,
    /// Plex sidecar rows: rating keys (from the `plex:<key>` id of
    /// unresolved rows, or recovered from the open queue snapshot for
    /// resolved rows — the synthetic display id itself is useless).
    pub plex_keys: Vec<String>,
}

/// Split rows for removal by id namespace (Seam D — port Tauri's intent,
/// not its T1 plex-falls-into-the-Qobuz-call bug). Qobuz catalog ids
/// resolve to `playlist_track_id` through the `CURRENT` Track cache (the
/// loaded detail keeps it there; `TrackItem` drops it) — never ship a
/// TRACK id to `remove_tracks_from_playlist` (its parameter is
/// playlist_track_ids; the old bulk path did exactly that and silently
/// failed). Call on the UI thread while the detail is open (the plex key
/// recovery reads the open snapshot).
pub fn split_for_removal(rows: &[SelectedRow]) -> RemovalSplit {
    let mut split = RemovalSplit::default();
    let mut qobuz_ids: Vec<u64> = Vec::new();
    for row in rows {
        match row.source.as_str() {
            "local" => match row.id.parse::<i64>() {
                Ok(rid) => split.local_track_ids.push(rid),
                Err(_) => {
                    log::warn!("[qbz-slint] remove: unresolvable local row id {}", row.id)
                }
            },
            "plex" => {
                if let Some(key) = row.id.strip_prefix("plex:") {
                    split.plex_keys.push(key.to_string());
                } else if let Some(key) = crate::local_playlist::plex_key_for_row(&row.id) {
                    split.plex_keys.push(key);
                } else {
                    log::warn!("[qbz-slint] remove: no rating key for plex row {}", row.id);
                }
            }
            _ => match row.id.parse::<u64>() {
                Ok(tid) => qobuz_ids.push(tid),
                Err(_) => {
                    log::warn!("[qbz-slint] remove: unresolvable row id {}", row.id)
                }
            },
        }
    }
    if !qobuz_ids.is_empty() {
        let id_set: std::collections::HashSet<u64> = qobuz_ids.iter().copied().collect();
        let mut resolved: std::collections::HashSet<u64> = std::collections::HashSet::new();
        if let Ok(cur) = CURRENT.lock() {
            for track in cur.iter().filter(|t| id_set.contains(&t.id)) {
                match track.playlist_track_id {
                    Some(ptid) => {
                        split.playlist_track_ids.push(ptid);
                        resolved.insert(track.id);
                    }
                    None => {
                        log::warn!(
                            "[qbz-slint] remove: track {} has no playlist_track_id",
                            track.id
                        );
                    }
                }
            }
        }
        for tid in qobuz_ids {
            if !resolved.contains(&tid) {
                log::warn!(
                    "[qbz-slint] remove: track {tid} not resolvable to a playlist_track_id — skipped"
                );
            }
        }
    }
    split
}
