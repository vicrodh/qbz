//! Offline Cache Manager controller — loads the artist→album→track rollup +
//! stats into `OfflineManagerState`. Per-item actions reuse `offline_cache::*`
//! (Slice 3); this module owns the data load, the toolbar filters (artist
//! rail / sort / show-only-failed), the album covers, and the size-limit edit.

use std::collections::BTreeMap;
use std::sync::{Mutex as StdMutex, OnceLock};

use qbz_offline_cache::{CachedTrackInfo, OfflineCacheStatus};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::{AppWindow, OfflineArtist, OfflineManagerState, OfflineRow};

const GB: u64 = 1024 * 1024 * 1024;

// --- Toolbar filter state (Rust-side source of truth) -------------------
// The rebuild runs on a tokio task and can't read Slint props cross-thread,
// so the artist selection / sort / show-only-failed live here. The UI
// actions update these + trigger a rebuild; the rebuild mirrors them back
// onto OfflineManagerState for the dropdown / toggle / rail display.

#[derive(Clone)]
struct Filters {
    selected_artist: String, // "" = all
    sort: i32,               // 0 alpha / 1 recent / 2 largest / 3 smallest
    show_only_failed: bool,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            selected_artist: String::new(),
            sort: 0,
            show_only_failed: false,
        }
    }
}

static FILTERS: OnceLock<StdMutex<Filters>> = OnceLock::new();

fn filters() -> &'static StdMutex<Filters> {
    FILTERS.get_or_init(|| StdMutex::new(Filters::default()))
}

fn current_filters() -> Filters {
    filters().lock().map(|f| f.clone()).unwrap_or_default()
}

// --- Formatting ---------------------------------------------------------

// pub(crate): also formats the lyrics-cache size in the Settings row
// (crate::lyrics::refresh_cache_stats).
pub(crate) fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", b / GB as f64)
    } else if bytes >= 1024 * 1024 {
        format!("{:.0} MB", b / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} KB", b / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn track_status_int(s: &OfflineCacheStatus) -> i32 {
    match s {
        OfflineCacheStatus::Ready => 3,
        OfflineCacheStatus::Failed => 4,
        _ => 2,
    }
}

fn fmt_duration(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn album_size(group: &[CachedTrackInfo]) -> u64 {
    group.iter().map(|t| t.file_size_bytes).sum()
}

/// Path of an album's on-disk cover thumbnail, or "" when none exists.
/// Resolution order (B5): the index's `artwork_path` when set, the CMAF
/// bundle's `tracks-cmaf/<id>/cover.jpg`, then the `cover.jpg` sibling of
/// the audio file (v1-format rows). Computed on the worker (a `String` is
/// `Send`); the image itself is loaded on the UI thread (`slint::Image` is
/// NOT `Send`).
fn cover_path(cache_path: &str, track: &CachedTrackInfo) -> String {
    track.resolve_cover_path(cache_path).unwrap_or_default()
}

/// Load a cover image from a path on the UI thread (empty path / missing -> default).
fn load_cover(path: &str) -> slint::Image {
    if path.is_empty() {
        return slint::Image::default();
    }
    slint::Image::load_from_path(std::path::Path::new(path)).unwrap_or_default()
}

/// Worker-built, `Send` row data. Converted to the (non-`Send`) `OfflineRow`
/// on the UI thread, where the cover image is decoded.
struct RowData {
    kind: &'static str,
    album_id: String,
    track_id: String,
    title: String,
    subtitle: String,
    meta: String,
    status: i32,
    progress: f32,
    cover_path: String,
    number: String,
}

// --- Data load ----------------------------------------------------------

/// Read the index.db, build the artist→album→track rollup + stats (applying
/// the current toolbar filters), and push them to `OfflineManagerState`.
/// `pub` so the cache mutation fns refresh the manager after their DB op.
pub async fn rebuild(weak: slint::Weak<AppWindow>) {
    let f = current_filters();
    let off = crate::offline::get().await;
    let (tracks, limit, cache_path): (Vec<CachedTrackInfo>, Option<u64>, String) = match off {
        Some(ref o) => {
            let limit = *o.limit_bytes.lock().await;
            let cp = o.get_cache_path();
            let guard = o.db.lock().await;
            let tracks = guard
                .as_ref()
                .and_then(|db| db.get_all_tracks().ok())
                .unwrap_or_default();
            (tracks, limit, cp)
        }
        None => (Vec::new(), None, String::new()),
    };

    let total_size: u64 = tracks.iter().map(|t| t.file_size_bytes).sum();
    let tracks_count = tracks.len() as i32;

    // album_id -> (artist, album_title, tracks), first-seen order (the DB
    // already returns rows most-recently-accessed first, so this order is
    // the "recent" sort).
    let mut album_order: Vec<String> = Vec::new();
    let mut albums: BTreeMap<String, (String, String, Vec<CachedTrackInfo>)> = BTreeMap::new();
    for t in tracks {
        let aid = t.album_id.clone().unwrap_or_else(|| "__singles__".to_string());
        if !albums.contains_key(&aid) {
            album_order.push(aid.clone());
        }
        let title = t.album.clone().unwrap_or_else(|| "Singles".to_string());
        albums
            .entry(aid)
            .or_insert_with(|| (t.artist.clone(), title, Vec::new()))
            .2
            .push(t);
    }

    // Artist rail: name -> (album_count, track_count), A-Z (BTreeMap order).
    let mut artist_stats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for aid in &album_order {
        let (artist, _title, group) = &albums[aid];
        let e = artist_stats.entry(artist.clone()).or_insert((0, 0));
        e.0 += 1;
        e.1 += group.len();
    }
    let artists: Vec<OfflineArtist> = artist_stats
        .iter()
        .map(|(name, (albums_n, tracks_n))| OfflineArtist {
            name: name.clone().into(),
            meta: qbz_i18n::t_args(
                "{} albums · {} tracks",
                &[&albums_n.to_string(), &tracks_n.to_string()],
            )
            .into(),
            selected: *name == f.selected_artist,
        })
        .collect();

    // Album display order per the sort.
    let mut order = album_order.clone();
    match f.sort {
        0 => order.sort_by(|a, b| albums[a].1.to_lowercase().cmp(&albums[b].1.to_lowercase())),
        2 => order.sort_by(|a, b| album_size(&albums[b].2).cmp(&album_size(&albums[a].2))),
        3 => order.sort_by(|a, b| album_size(&albums[a].2).cmp(&album_size(&albums[b].2))),
        _ => {} // 1 recent — keep the DB's last_accessed_at DESC order
    }

    let mut rows: Vec<RowData> = Vec::new();
    for aid in &order {
        let (artist, title, group) = &albums[aid];
        if !f.selected_artist.is_empty() && *artist != f.selected_artist {
            continue;
        }
        let any_failed = group
            .iter()
            .any(|t| matches!(t.status, OfflineCacheStatus::Failed));
        if f.show_only_failed && !any_failed {
            continue;
        }
        let any_active = group.iter().any(|t| {
            matches!(
                t.status,
                OfflineCacheStatus::Queued | OfflineCacheStatus::Downloading
            )
        });
        let all_ready = group
            .iter()
            .all(|t| matches!(t.status, OfflineCacheStatus::Ready));
        let album_status = if any_failed {
            4
        } else if any_active {
            2
        } else if all_ready {
            3
        } else {
            0
        };
        // First track whose cover resolves — within an album only some
        // tracks may carry one (per-track CMAF folders, mixed v1/v2 rows).
        let cover_path = group
            .iter()
            .map(|t| cover_path(&cache_path, t))
            .find(|p| !p.is_empty())
            .unwrap_or_default();
        rows.push(RowData {
            kind: "album",
            album_id: aid.clone(),
            track_id: String::new(),
            title: title.clone(),
            subtitle: artist.clone(),
            meta: qbz_i18n::t_args(
                "{} tracks · {}",
                &[&group.len().to_string(), &human_size(album_size(group))],
            ),
            status: album_status,
            progress: 0.0,
            cover_path,
            number: String::new(),
        });
        for (i, t) in group.iter().enumerate() {
            if f.show_only_failed && !matches!(t.status, OfflineCacheStatus::Failed) {
                continue;
            }
            rows.push(RowData {
                kind: "track",
                album_id: aid.clone(),
                track_id: t.track_id.to_string(),
                title: t.title.clone(),
                subtitle: t.artist.clone(),
                meta: fmt_duration(t.duration_secs),
                status: track_status_int(&t.status),
                progress: t.progress_percent as f32 / 100.0,
                cover_path: String::new(),
                number: (i + 1).to_string(),
            });
        }
    }

    let (limit_text, usage, limit_gb) = match limit {
        Some(l) if l > 0 => (
            qbz_i18n::t_args("· of {}", &[&human_size(l)]),
            (total_size as f32 / l as f32).clamp(0.0, 1.0),
            (l / GB).max(1) as i32,
        ),
        _ => (qbz_i18n::t("· Unlimited"), 0.0, 5),
    };
    let size_text = human_size(total_size);

    let _ = weak.upgrade_in_event_loop(move |w| {
        let st = w.global::<OfflineManagerState>();
        // Build OfflineRow on the UI thread (decodes the cover images here —
        // slint::Image is not Send, so it can't be built on the worker).
        let offline_rows: Vec<OfflineRow> = rows
            .into_iter()
            .map(|rd| OfflineRow {
                kind: rd.kind.into(),
                album_id: rd.album_id.into(),
                track_id: rd.track_id.into(),
                title: rd.title.into(),
                subtitle: rd.subtitle.into(),
                meta: rd.meta.into(),
                status: rd.status,
                progress: rd.progress,
                cover: load_cover(&rd.cover_path),
                number: rd.number.into(),
                selected: false,
            })
            .collect();
        st.set_rows(ModelRc::new(VecModel::from(offline_rows)));
        st.set_selected_count(0);
        st.set_artists(ModelRc::new(VecModel::from(artists)));
        st.set_tracks_count(tracks_count);
        st.set_size_text(SharedString::from(size_text));
        st.set_limit_text(SharedString::from(limit_text));
        st.set_usage(usage);
        st.set_limit_gb(limit_gb);
        st.set_selected_artist(SharedString::from(f.selected_artist));
        st.set_sort_index(f.sort);
        st.set_show_only_failed(f.show_only_failed);
        st.set_loading(false);
    });
}

/// Load (or refresh) the manager. Marks loading, then rebuilds.
pub fn load(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<OfflineManagerState>().set_loading(true);
    });
    handle.spawn(rebuild(weak));
}

// --- Toolbar actions ----------------------------------------------------

pub fn select_artist(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle, name: String) {
    if let Ok(mut f) = filters().lock() {
        f.selected_artist = name;
    }
    handle.spawn(rebuild(weak));
}

pub fn set_sort(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle, index: i32) {
    if let Ok(mut f) = filters().lock() {
        f.sort = index;
    }
    handle.spawn(rebuild(weak));
}

pub fn toggle_failed(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    if let Ok(mut f) = filters().lock() {
        f.show_only_failed = !f.show_only_failed;
    }
    handle.spawn(rebuild(weak));
}

// --- Multi-select (in-place model edits on the UI thread) ---------------

fn recount(st: &OfflineManagerState) {
    let model = st.get_rows();
    let mut n = 0;
    for i in 0..model.row_count() {
        if let Some(r) = model.row_data(i) {
            if r.kind == "track" && r.selected {
                n += 1;
            }
        }
    }
    st.set_selected_count(n);
}

/// Flip a single track row's checkbox in place. Plain/Ctrl+Click = single
/// toggle; Shift+Click = additive range from the anchor (always-on selection,
/// so the range is always available). The model interleaves artist-header rows,
/// so the range setter only touches `kind == "track"` rows.
pub fn toggle_select(w: &AppWindow, track_id: &str) {
    let st = w.global::<OfflineManagerState>();
    let model = st.get_rows();
    if let Some(vm) = model.as_any().downcast_ref::<VecModel<OfflineRow>>() {
        let clicked = (0..vm.row_count()).find(|&i| {
            vm.row_data(i)
                .map(|r| r.kind == "track" && r.track_id == track_id)
                .unwrap_or(false)
        });
        if let Some(clicked) = clicked {
            let shift = crate::keybindings::mods().2;
            let anchor = if shift {
                crate::selection::resolve_anchor(
                    crate::selection::SURFACE_OFFLINE,
                    vm,
                    |r| r.track_id.to_string(),
                )
            } else {
                None
            };
            match anchor {
                Some(anchor) => crate::selection::apply_shift_range(vm, anchor, clicked, |r, v| {
                    if r.kind == "track" {
                        r.selected = v;
                    }
                }),
                None => {
                    if let Some(mut r) = vm.row_data(clicked) {
                        r.selected = !r.selected;
                        vm.set_row_data(clicked, r);
                    }
                }
            }
            crate::selection::set_anchor(crate::selection::SURFACE_OFFLINE, clicked, track_id);
        }
    }
    recount(&st);
}

/// Check (or uncheck) every track row.
pub fn set_all_selected(w: &AppWindow, selected: bool) {
    let st = w.global::<OfflineManagerState>();
    let model = st.get_rows();
    if let Some(vm) = model.as_any().downcast_ref::<VecModel<OfflineRow>>() {
        for i in 0..vm.row_count() {
            if let Some(mut r) = vm.row_data(i) {
                if r.kind == "track" && r.selected != selected {
                    r.selected = selected;
                    vm.set_row_data(i, r);
                }
            }
        }
    }
    recount(&st);
}

/// The Qobuz ids of the checked track rows (for the bulk actions).
pub fn selected_track_ids(w: &AppWindow) -> Vec<u64> {
    let model = w.global::<OfflineManagerState>().get_rows();
    let mut ids = Vec::new();
    for i in 0..model.row_count() {
        if let Some(r) = model.row_data(i) {
            if r.kind == "track" && r.selected {
                if let Ok(id) = r.track_id.parse::<u64>() {
                    ids.push(id);
                }
            }
        }
    }
    ids
}

/// Set the cache size limit (GB), persist it to disk, and refresh.
pub fn set_limit(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle, gb: i32) {
    handle.spawn(async move {
        let bytes = (gb.max(1) as u64) * GB;
        if let Some(off) = crate::offline::get().await {
            *off.limit_bytes.lock().await = Some(bytes);
        }
        crate::offline::persist_limit(bytes).await;
        rebuild(weak).await;
    });
}
