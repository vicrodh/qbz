//! Local Library controller (Slint) — greenfield port of Tauri's
//! `LocalLibraryView`.
//!
//! This module owns the per-tab navigation and (slice by slice) the data
//! loading for the four browse tabs: Albums / Artists / Folders / Tracks.
//! It reads the shared per-user `library.db` through the already
//! frontend-agnostic `qbz-library` crate (see `library_db::with_db`), and
//! Plex through the `qbz-plex` core crate.
//!
//! Folder management, scan, maintenance, and the danger zone do NOT live in
//! this view — they belong under Settings > Local Library. The view's gear
//! button routes there.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::artwork::{ArtworkJob, ArtworkTarget, ImageCache};
use crate::{
    AlbumCardItem, AlphaJump, AppWindow, DiscoverSection, FolderNode, LocalArtistItem,
    LocalArtistSection, LocalLibraryState, TrackItem,
};

/// The four browse tabs. Order mirrors Tauri's default tab order
/// (`tracks / folders / albums / artists`); the visible order is a user
/// preference layered on top later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibTab {
    Tracks,
    Folders,
    Albums,
    Artists,
}

impl LibTab {
    /// Parse a HeaderBar menu route (`"local-albums"` etc.).
    pub fn from_route(route: &str) -> Option<Self> {
        match route {
            "local-tracks" => Some(Self::Tracks),
            "local-folders" => Some(Self::Folders),
            "local-albums" => Some(Self::Albums),
            "local-artists" => Some(Self::Artists),
            _ => None,
        }
    }

    /// Parse a tab id (`"albums"` etc.) as carried by a `NavEntry` and by
    /// `LocalLibraryState.active-tab`.
    pub fn from_tab_id(id: &str) -> Option<Self> {
        match id {
            "tracks" => Some(Self::Tracks),
            "folders" => Some(Self::Folders),
            "albums" => Some(Self::Albums),
            "artists" => Some(Self::Artists),
            _ => None,
        }
    }

    /// The canonical tab id used in nav entries + `LocalLibraryState.active-tab`.
    pub fn tab_id(self) -> &'static str {
        match self {
            Self::Tracks => "tracks",
            Self::Folders => "folders",
            Self::Albums => "albums",
            Self::Artists => "artists",
        }
    }
}

// =============================== Albums tab ===============================
//
// The Albums tab browses the metadata-grouped local albums via the
// server-paginated `get_albums_metadata_page` (the performant path that
// scales past the documented 16K-row freeze). Sort + search are pushed to
// SQL; the grid pages on scroll. The shared `AlbumCollectionView` renders
// the result, covers load from the local filesystem via the source-aware
// artwork pipeline.

/// Albums fetched per page (chunk). Mirrors Tauri's chunked store (100).
const ALBUMS_PAGE: u64 = 100;

/// Generation guard, bumped on every page-1 (re)load. A stale in-flight
/// fetch (older search/sort) is discarded on apply, and an in-flight
/// load-more is dropped once a reload supersedes it.
static ALBUMS_GEN: AtomicU64 = AtomicU64::new(0);

/// True if `gen` is still the current albums generation. The artwork
/// pipeline calls this before applying a decoded cover so an in-flight job
/// from a superseded page (a search/sort/retry replaced the model) doesn't
/// land on a stale row index.
pub fn albums_gen_current(gen: u64) -> bool {
    ALBUMS_GEN.load(Ordering::SeqCst) == gen
}

/// Map one local album row to the shared plain `AlbumCard`. Kept as the
/// Send-safe plain struct (NOT `AlbumCardItem`, which holds a non-Send
/// `slint::Image`) so it can cross the `spawn_blocking` boundary; the
/// conversion to `AlbumCardItem` happens on the UI thread via
/// `album_map::to_item`. Genre is intentionally empty (the local DB carries
/// genre per-track, not per-album); the cover PATH rides on `artwork_url`
/// as the artwork-job carrier (the grid renders `artwork`, not the url).
pub fn map_local_album(a: qbz_library::LocalAlbum) -> crate::album_map::AlbumCard {
    let tier = match a.bit_depth {
        Some(b) if b >= 24 => "hires",
        Some(_) => "cd",
        None => "",
    };
    let (quality_detail, quality_label) = local_quality(a.bit_depth, a.sample_rate);
    let year = a.year.map(|y| y.to_string()).unwrap_or_default();
    let track_count = if a.track_count > 0 {
        a.track_count.to_string()
    } else {
        String::new()
    };
    // Real source for the SOURCE column + the always-visible card badge:
    // user files -> local, offline copies -> qobuz_download, Plex -> plex.
    let source = match a.source.as_str() {
        "plex" => "plex",
        "qobuz_download" => "qobuz_download",
        _ => "local",
    }
    .to_string();
    crate::album_map::AlbumCard {
        id: a.id,
        title: a.title,
        artist: a.artist,
        artist_id: String::new(),
        genre: String::new(),
        year: year.clone(),
        quality_tier: tier.to_string(),
        quality_label,
        artwork_url: a.artwork_path.unwrap_or_default(),
        release_type: crate::album_map::classify_release_type(Some(a.track_count)).to_string(),
        source,
        quality_detail,
        track_count,
        plain_year: year,
    }
}

/// Format a local album's quality. `sample_rate_hz` is Hz (44100.0); the
/// detail is the bare "24-bit / 96 kHz" (QualityBadgeFull) and the label is
/// the grid badge tooltip "Hi-Res: 24-bit / 96 kHz".
fn local_quality(bit_depth: Option<u32>, sample_rate_hz: f64) -> (String, String) {
    let Some(bd) = bit_depth else {
        return (String::new(), String::new());
    };
    let khz = if sample_rate_hz >= 1000.0 {
        sample_rate_hz / 1000.0
    } else {
        sample_rate_hz
    };
    let khz_str = if khz.fract().abs() < 0.05 {
        format!("{}", khz.round() as i64)
    } else {
        format!("{khz:.1}")
    };
    let prefix = if bd >= 24 { "Hi-Res" } else { "CD" };
    (
        format!("{bd}-bit / {khz_str} kHz"),
        format!("{prefix}: {bd}-bit / {khz_str} kHz"),
    )
}

/// Map the toolbar sort key to the DB's validated (sort_by, sort_dir).
fn sort_params(sort: &str) -> (&'static str, &'static str) {
    match sort {
        "title-asc" => ("title", "asc"),
        "title-desc" => ("title", "desc"),
        "year-desc" => ("year", "desc"),
        "year-asc" => ("year", "asc"),
        "artist-desc" => ("artist", "desc"),
        // Covers all six toolbar options; unknown input defaults to artist-asc.
        _ => ("artist", "asc"),
    }
}

/// Fetch one albums page off the UI thread (rusqlite is blocking). Returns
/// (cards, total, has_more), or None if the DB can't be opened. Returns the
/// plain `AlbumCard` (Send) — conversion to `AlbumCardItem` is UI-thread.
fn fetch_albums_page(
    offset: u64,
    search: String,
    sort: String,
) -> Option<(Vec<crate::album_map::AlbumCard>, u64, bool)> {
    let (sort_by, sort_dir) = sort_params(&sort);
    let trimmed = search.trim();
    let search_opt = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    };
    crate::library_db::with_db(|db| {
        let page = db.get_albums_metadata_page(
            offset,
            ALBUMS_PAGE,
            search_opt,
            sort_by,
            sort_dir,
            // include_qobuz_downloads: mirrors the `show_in_library`
            // download-setting default (true). The user toggle lands with
            // the Settings > Local Library integration.
            true,
            // exclude_network_folders / plex_cache_path: offline-network
            // filtering and Plex are their own later slices.
            false,
            None,
        )?;
        let total = page.total;
        let has_more = offset + (page.albums.len() as u64) < total;
        let cards: Vec<crate::album_map::AlbumCard> = page
            .albums
            .into_iter()
            .map(|a| {
                let mut card = map_local_album(a);
                // Offline-cached / folder albums sometimes have no backfilled
                // artwork_path though a cover.jpg sits in the track folder —
                // resolve it so the cover that exists on disk actually shows.
                if card.artwork_url.is_empty() {
                    if let Some(cover) = db.resolve_album_cover_fallback(&card.id) {
                        card.artwork_url = cover;
                    }
                }
                card
            })
            .collect();
        Ok((cards, total, has_more))
    })
}

/// Build local-cover artwork jobs for a page of cards. The job url carries
/// the local file path; `spawn_local_loads` decodes via `ArtworkRef::LocalFile`.
/// `gen` is the albums generation at fetch time — a reload bumps it, so a
/// cover decoded after the model was replaced is dropped on apply.
fn album_artwork_jobs(
    cards: &[crate::album_map::AlbumCard],
    offset: usize,
    gen: u64,
) -> Vec<ArtworkJob> {
    cards
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.artwork_url.is_empty())
        .map(|(i, c)| ArtworkJob {
            target: ArtworkTarget::LocalAlbumCard {
                index: offset + i,
                gen,
            },
            url: c.artwork_url.clone(),
        })
        .collect()
}

/// Replace the albums set with page 1 (UI thread). Converts the plain cards
/// to `AlbumCardItem` here, where the non-Send `slint::Image` is allowed.
fn apply_albums(
    window: &AppWindow,
    cards: Vec<crate::album_map::AlbumCard>,
    total: u64,
    has_more: bool,
) {
    let items: Vec<AlbumCardItem> = cards.into_iter().map(crate::album_map::to_item).collect();
    let s = window.global::<LocalLibraryState>();
    let n = items.len() as i32;
    s.set_albums(ModelRc::new(VecModel::from(items)));
    s.set_albums_total(total as i32);
    s.set_album_count(total as i32);
    s.set_albums_next_offset(n);
    s.set_albums_has_more(has_more);
    s.set_albums_loading(false);
    s.set_albums_loading_more(false);
    s.set_albums_load_failed(false);
}

/// Append a fetched page onto the existing grid (UI thread).
fn append_albums(
    window: &AppWindow,
    cards: Vec<crate::album_map::AlbumCard>,
    total: u64,
    has_more: bool,
) {
    let new_items: Vec<AlbumCardItem> =
        cards.into_iter().map(crate::album_map::to_item).collect();
    let s = window.global::<LocalLibraryState>();
    let model = s.get_albums();
    let mut combined: Vec<AlbumCardItem> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .collect();
    let added = new_items.len() as i32;
    combined.extend(new_items);
    s.set_albums(ModelRc::new(VecModel::from(combined)));
    s.set_albums_next_offset(s.get_albums_next_offset() + added);
    s.set_albums_total(total as i32);
    s.set_album_count(total as i32);
    s.set_albums_has_more(has_more);
    s.set_albums_loading_more(false);
}

/// Spawn the page-1 fetch and apply it (gen-guarded). Reads search/sort +
/// spawns from the UI thread; the caller has set `albums-loading` + `gen`.
fn spawn_albums_page_load(
    window: &AppWindow,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    gen: u64,
) {
    let s = window.global::<LocalLibraryState>();
    let search = s.get_albums_search().to_string();
    let sort = s.get_albums_sort().to_string();
    let weak = window.as_weak();
    handle.spawn(async move {
        let result = tokio::task::spawn_blocking(move || fetch_albums_page(0, search, sort))
            .await
            .ok()
            .flatten();
        let _ = weak.upgrade_in_event_loop(move |w| {
            if ALBUMS_GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            match result {
                Some((cards, total, has_more)) => {
                    let jobs = album_artwork_jobs(&cards, 0, gen);
                    apply_albums(&w, cards, total, has_more);
                    crate::artwork::spawn_local_loads(jobs, w.as_weak(), image_cache);
                }
                None => {
                    let s = w.global::<LocalLibraryState>();
                    s.set_albums_loading(false);
                    s.set_albums_load_failed(true);
                }
            }
        });
    });
}

/// (Re)load page 1 with the current search + sort. Bumps the generation so
/// any older in-flight fetch is discarded on arrival.
pub fn reload_albums(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let gen = ALBUMS_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        let s = w.global::<LocalLibraryState>();
        s.set_albums_loading(true);
        s.set_albums_load_failed(false);
        spawn_albums_page_load(&w, handle, image_cache, gen);
    });
}

/// Load page 1 only if the albums set is empty and not already loading — the
/// lazy per-tab fetch on first visit (re-entry keeps the loaded set + scroll).
pub fn ensure_albums_loaded(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<LocalLibraryState>();
        if s.get_albums().row_count() == 0 && !s.get_albums_loading() {
            let gen = ALBUMS_GEN.fetch_add(1, Ordering::SeqCst) + 1;
            s.set_albums_loading(true);
            s.set_albums_load_failed(false);
            spawn_albums_page_load(&w, handle, image_cache, gen);
        }
    });
}

/// Fetch + append the next page (driven by scroll-near-bottom). No-ops while
/// a page is in flight or the catalog is exhausted; the in-flight result is
/// dropped if a reload supersedes it.
pub fn load_more_albums(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<LocalLibraryState>();
        if !s.get_albums_has_more() || s.get_albums_loading_more() || s.get_albums_loading() {
            return;
        }
        let offset = s.get_albums_next_offset().max(0) as u64;
        let search = s.get_albums_search().to_string();
        let sort = s.get_albums_sort().to_string();
        let gen = ALBUMS_GEN.load(Ordering::SeqCst);
        s.set_albums_loading_more(true);
        let weak2 = w.as_weak();
        handle.spawn(async move {
            let result =
                tokio::task::spawn_blocking(move || fetch_albums_page(offset, search, sort))
                    .await
                    .ok()
                    .flatten();
            let _ = weak2.upgrade_in_event_loop(move |w| {
                let s = w.global::<LocalLibraryState>();
                if ALBUMS_GEN.load(Ordering::SeqCst) != gen {
                    s.set_albums_loading_more(false);
                    return;
                }
                match result {
                    Some((cards, total, has_more)) => {
                        let base = s.get_albums().row_count();
                        let jobs = album_artwork_jobs(&cards, base, gen);
                        append_albums(&w, cards, total, has_more);
                        crate::artwork::spawn_local_loads(jobs, w.as_weak(), image_cache);
                    }
                    None => {
                        s.set_albums_loading_more(false);
                    }
                }
            });
        });
    });
}

// =============================== Tracks tab ===============================
//
// Server-paginated flat list (the perf path that avoids the documented ~16K
// freeze): each page is a `search_with_filter_page` query, appended on
// scroll. Track artwork is off by default (Tauri's perf default), so there
// are no per-row artwork jobs here. Group-by / multi-select / per-row
// playback land with the source-aware playback slice.

const TRACKS_PAGE: u64 = 200;

static TRACKS_GEN: AtomicU64 = AtomicU64::new(0);

/// The loaded `LocalTrack` rows backing `tracks`, kept in lockstep with the
/// paged model in apply/append. The selection-source for bulk queue/play-next/
/// add-to-playlist (resolves ids -> LocalTrack with no DB round-trip).
static TRACKS_CURRENT: LazyLock<Mutex<Vec<qbz_library::LocalTrack>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

fn tracks_current() -> std::sync::MutexGuard<'static, Vec<qbz_library::LocalTrack>> {
    TRACKS_CURRENT.lock().unwrap_or_else(|e| e.into_inner())
}

fn fmt_duration(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Map one local track row to the rendered `TrackItem` (UI thread — holds a
/// non-Send `slint::Image`). Local tracks aren't Qobuz-linkable, so the
/// artist/album link ids are empty (the row renders them as plain text).
fn map_local_track(t: qbz_library::LocalTrack) -> TrackItem {
    let tier = match t.bit_depth {
        Some(b) if b >= 24 => "hires",
        Some(_) => "cd",
        None => "",
    };
    TrackItem {
        id: t.id.to_string().into(),
        number: t.track_number.map(|n| n.to_string()).unwrap_or_default().into(),
        title: t.title.into(),
        artist: t.artist.into(),
        album: t.album.into(),
        duration: fmt_duration(t.duration_secs).into(),
        quality_tier: tier.into(),
        explicit: false,
        selected: false,
        artwork_url: t.artwork_path.unwrap_or_default().into(),
        artwork: slint::Image::default(),
        is_favorite: false,
        artist_id: "".into(),
        album_id: "".into(),
        removing: false,
        cache_status: 0,
        cache_progress: 0.0,
        // Source indicator: offline copies read as Qobuz, user files as local.
        source: match t.source.as_deref() {
            Some("qobuz_download") => "qobuz",
            Some("plex") => "plex",
            _ => "local",
        }
        .into(),
        unlocking: false,
    }
}

/// Fetch one tracks page off the UI thread. `LocalTrack` is Send, so it
/// crosses the `spawn_blocking` boundary; the conversion to `TrackItem`
/// happens on the UI thread. has_more = the page came back full.
fn fetch_tracks_page(query: String, offset: u64) -> Option<(Vec<qbz_library::LocalTrack>, bool)> {
    let rows = crate::library_db::with_db(|db| {
        db.search_with_filter_page(query.trim(), offset, TRACKS_PAGE, true, false)
    })?;
    let has_more = rows.len() as u64 == TRACKS_PAGE;
    Some((rows, has_more))
}

fn apply_tracks(window: &AppWindow, rows: Vec<qbz_library::LocalTrack>, has_more: bool) {
    // Keep the selection-source cache in lockstep (clone BEFORE the move).
    *tracks_current() = rows.clone();
    let items: Vec<TrackItem> = rows.into_iter().map(map_local_track).collect();
    let s = window.global::<LocalLibraryState>();
    let n = items.len() as i32;
    s.set_tracks(ModelRc::new(VecModel::from(items)));
    s.set_tracks_next_offset(n);
    s.set_tracks_has_more(has_more);
    s.set_tracks_loading(false);
    s.set_tracks_loading_more(false);
    s.set_tracks_load_failed(false);
    derive_tracks(window);
}

fn append_tracks(window: &AppWindow, rows: Vec<qbz_library::LocalTrack>, has_more: bool) {
    tracks_current().extend(rows.clone());
    let new_items: Vec<TrackItem> = rows.into_iter().map(map_local_track).collect();
    let s = window.global::<LocalLibraryState>();
    let model = s.get_tracks();
    let mut combined: Vec<TrackItem> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .collect();
    let added = new_items.len() as i32;
    combined.extend(new_items);
    s.set_tracks(ModelRc::new(VecModel::from(combined)));
    s.set_tracks_next_offset(s.get_tracks_next_offset() + added);
    s.set_tracks_has_more(has_more);
    s.set_tracks_loading_more(false);
    derive_tracks(window);
}

// --- Tracks group-by + multi-select (reduced port of favorites helpers) ---

/// Re-derive the group-ordered + search-filtered `tracks-visible` render model
/// from the loaded `tracks`, plus the A-Z jump strip for name grouping. Uses
/// the local `folder_alpha_key`; no genre filter (no local genre surface).
pub fn derive_tracks(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    let query_owned = s.get_tracks_search().to_lowercase();
    let query = query_owned.trim();
    let group = s.get_tracks_group_mode().to_string();
    let all = s.get_tracks();
    s.set_tracks_alpha(ModelRc::new(VecModel::from(Vec::<AlphaJump>::new())));

    // Fast path: no search + no grouping -> share the loaded model.
    if query.is_empty() && group == "off" {
        s.set_tracks_visible(all);
        return;
    }
    let mut filtered: Vec<TrackItem> = (0..all.row_count())
        .filter_map(|i| all.row_data(i))
        .filter(|t| {
            query.is_empty()
                || t.title.to_lowercase().contains(query)
                || t.artist.to_lowercase().contains(query)
                || t.album.to_lowercase().contains(query)
        })
        .collect();
    let lc = |s: &slint::SharedString| s.to_lowercase();
    match group.as_str() {
        "album" => filtered
            .sort_by(|a, b| lc(&a.album).cmp(&lc(&b.album)).then(lc(&a.title).cmp(&lc(&b.title)))),
        "artist" => filtered.sort_by(|a, b| {
            lc(&a.artist)
                .cmp(&lc(&b.artist))
                .then(lc(&a.album).cmp(&lc(&b.album)))
                .then(lc(&a.title).cmp(&lc(&b.title)))
        }),
        "name" => filtered.sort_by(|a, b| lc(&a.title).cmp(&lc(&b.title))),
        _ => {}
    }
    if group == "name" {
        let mut jumps: Vec<AlphaJump> = Vec::new();
        let mut last = String::new();
        for (i, t) in filtered.iter().enumerate() {
            let key = folder_alpha_key(t.title.as_str());
            if key != last {
                jumps.push(AlphaJump {
                    letter: key.clone().into(),
                    index: i as i32,
                });
                last = key;
            }
        }
        s.set_tracks_alpha(ModelRc::new(VecModel::from(jumps)));
    }
    s.set_tracks_visible(ModelRc::new(VecModel::from(filtered)));
}

/// Set the group mode, persist it, re-derive.
pub fn set_tracks_group(window: &AppWindow, mode: &str) {
    window.global::<LocalLibraryState>().set_tracks_group_mode(mode.into());
    crate::locallibrary_prefs::save(window);
    derive_tracks(window);
}

/// Enter/leave multi-select; leaving clears the selection.
pub fn set_tracks_multi_select(window: &AppWindow, on: bool) {
    window.global::<LocalLibraryState>().set_tracks_multi_select(on);
    if !on {
        clear_tracks_selection(window);
    }
}

/// Recount selected visible rows into `tracks-selected-count`.
pub fn recount_tracks_selected(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    let model = s.get_tracks_visible();
    let count = (0..model.row_count())
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    s.set_tracks_selected_count(count as i32);
}

/// Toggle one row's selection (by id) in the visible model.
pub fn toggle_track_select(window: &AppWindow, id: &str) {
    let model = window.global::<LocalLibraryState>().get_tracks_visible();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.id.as_str() == id {
                item.selected = !item.selected;
                model.set_row_data(i, item);
                break;
            }
        }
    }
    recount_tracks_selected(window);
}

/// Select-all toggle: select every visible row, or clear if all selected.
pub fn select_all_tracks(window: &AppWindow) {
    let model = window.global::<LocalLibraryState>().get_tracks_visible();
    let total = model.row_count();
    let selected = (0..total)
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    let target = selected != total;
    for i in 0..total {
        if let Some(mut item) = model.row_data(i) {
            if item.selected != target {
                item.selected = target;
                model.set_row_data(i, item);
            }
        }
    }
    recount_tracks_selected(window);
}

/// Deselect every visible row (multi-select mode stays on).
pub fn clear_tracks_selection(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    let model = s.get_tracks_visible();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.selected {
                item.selected = false;
                model.set_row_data(i, item);
            }
        }
    }
    s.set_tracks_selected_count(0);
}

/// The selected rows resolved to `LocalTrack`, in DISPLAY order (iterate the
/// visible model, look each selected id up in `TRACKS_CURRENT`). Deviates from
/// favorites' load-order resolution so the queue matches what the user sees.
pub fn selected_local_tracks(window: &AppWindow) -> Vec<qbz_library::LocalTrack> {
    let model = window.global::<LocalLibraryState>().get_tracks_visible();
    let cache = tracks_current();
    let mut out = Vec::new();
    for i in 0..model.row_count() {
        if let Some(item) = model.row_data(i) {
            if item.selected {
                let id = item.id.to_string();
                if let Some(t) = cache.iter().find(|t| t.id.to_string() == id) {
                    out.push(t.clone());
                }
            }
        }
    }
    out
}

/// Resolve a single row id (display) to its `LocalTrack` from the cache.
pub fn local_track_by_id(id: &str) -> Option<qbz_library::LocalTrack> {
    tracks_current().iter().find(|t| t.id.to_string() == id).cloned()
}

fn spawn_tracks_page_load(window: &AppWindow, handle: tokio::runtime::Handle, gen: u64) {
    let s = window.global::<LocalLibraryState>();
    let query = s.get_tracks_search().to_string();
    let weak = window.as_weak();
    handle.spawn(async move {
        let result = tokio::task::spawn_blocking(move || fetch_tracks_page(query, 0))
            .await
            .ok()
            .flatten();
        let _ = weak.upgrade_in_event_loop(move |w| {
            if TRACKS_GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            match result {
                Some((rows, has_more)) => apply_tracks(&w, rows, has_more),
                None => {
                    let s = w.global::<LocalLibraryState>();
                    s.set_tracks_loading(false);
                    s.set_tracks_load_failed(true);
                }
            }
        });
    });
}

/// (Re)load page 1 of the tracks list with the current search.
pub fn reload_tracks(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let gen = TRACKS_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        let s = w.global::<LocalLibraryState>();
        s.set_tracks_loading(true);
        s.set_tracks_load_failed(false);
        spawn_tracks_page_load(&w, handle, gen);
    });
}

/// Lazy load on first visit (re-entry keeps the loaded set + scroll).
pub fn ensure_tracks_loaded(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<LocalLibraryState>();
        if s.get_tracks().row_count() == 0 && !s.get_tracks_loading() {
            let gen = TRACKS_GEN.fetch_add(1, Ordering::SeqCst) + 1;
            s.set_tracks_loading(true);
            s.set_tracks_load_failed(false);
            spawn_tracks_page_load(&w, handle, gen);
        }
    });
}

/// Fetch + append the next tracks page (scroll-near-bottom).
pub fn load_more_tracks(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<LocalLibraryState>();
        if !s.get_tracks_has_more() || s.get_tracks_loading_more() || s.get_tracks_loading() {
            return;
        }
        let offset = s.get_tracks_next_offset().max(0) as u64;
        let query = s.get_tracks_search().to_string();
        let gen = TRACKS_GEN.load(Ordering::SeqCst);
        s.set_tracks_loading_more(true);
        let weak2 = w.as_weak();
        handle.spawn(async move {
            let result = tokio::task::spawn_blocking(move || fetch_tracks_page(query, offset))
                .await
                .ok()
                .flatten();
            let _ = weak2.upgrade_in_event_loop(move |w| {
                let s = w.global::<LocalLibraryState>();
                if TRACKS_GEN.load(Ordering::SeqCst) != gen {
                    s.set_tracks_loading_more(false);
                    return;
                }
                match result {
                    Some((rows, has_more)) => append_tracks(&w, rows, has_more),
                    None => s.set_tracks_loading_more(false),
                }
            });
        });
    });
}

// ============================ Album detail ================================
//
// Local albums reuse the shared `AlbumPageView` + `AlbumState`: we just
// populate the state from the album's local tracks and flag `is-local` so
// the media-action dispatcher routes play to local playback.

fn fmt_album_duration(secs: u64) -> String {
    let mins = secs / 60;
    if mins >= 60 {
        format!("{} h {} min", mins / 60, mins % 60)
    } else {
        format!("{mins} min")
    }
}

/// Populate `AlbumState` from a local album's tracks (UI thread). The cover
/// is loaded separately by the caller; `is-local` is set so playback routes
/// to local. `group_key` is the metadata group key.
pub fn apply_local_album(
    window: &AppWindow,
    group_key: &str,
    tracks: Vec<qbz_library::LocalTrack>,
) {
    let title = tracks
        .first()
        .map(|t| t.album_group_title.clone())
        .unwrap_or_default();
    // Album artist: the common album-artist, else "Various Artists".
    let artist_of = |t: &qbz_library::LocalTrack| {
        t.album_artist.clone().unwrap_or_else(|| t.artist.clone())
    };
    let artist = match tracks.first() {
        Some(first) => {
            let name = artist_of(first);
            if tracks.iter().all(|t| artist_of(t) == name) {
                name
            } else {
                "Various Artists".to_string()
            }
        }
        None => String::new(),
    };
    let cover = tracks
        .iter()
        .find_map(|t| t.artwork_path.clone())
        .unwrap_or_default();
    let cover_url = if cover.is_empty() {
        String::new()
    } else {
        format!("file://{cover}")
    };
    // Quality from the highest-resolution track in the album.
    let (tier, detail) = match tracks.iter().max_by_key(|t| t.bit_depth.unwrap_or(0)) {
        Some(t) => {
            let tier = match t.bit_depth {
                Some(b) if b >= 24 => "hires",
                Some(_) => "cd",
                None => "",
            };
            (tier.to_string(), local_quality(t.bit_depth, t.sample_rate).0)
        }
        None => (String::new(), String::new()),
    };
    let total_secs: u64 = tracks.iter().map(|t| t.duration_secs).sum();
    let info_line = format!("{} tracks · {}", tracks.len(), fmt_album_duration(total_secs));
    // Album source for the tag-editor pencil gate (hidden for Plex). Computed
    // before `tracks` is consumed below.
    let source = tracks
        .first()
        .and_then(|t| t.source.clone())
        .unwrap_or_default();
    let items: Vec<TrackItem> = tracks
        .into_iter()
        .map(|t| {
            let mut it = map_local_track(t);
            it.album_id = group_key.into();
            it
        })
        .collect();

    let s = window.global::<crate::AlbumState>();
    s.set_id(group_key.into());
    s.set_is_local(true);
    s.set_source(source.into());
    s.set_title(title.into());
    s.set_artist(artist.into());
    s.set_artist_id("".into());
    s.set_artwork_url(cover_url.into());
    s.set_has_custom_cover(false);
    s.set_quality_tier(tier.into());
    s.set_quality_detail(detail.into());
    s.set_info_line(info_line.into());
    s.set_label("".into());
    s.set_description("".into());
    s.set_description_short("".into());
    s.set_awards(ModelRc::new(VecModel::from(Vec::<slint::SharedString>::new())));
    s.set_tracks(ModelRc::new(VecModel::from(items)));
    s.set_loading(false);
}

// ============================ Folders (flat) ==============================
//
// The Folders tab (flat mode) is the album grid grouped by directory rather
// than by metadata. Full-load (folder counts are bounded; the freeze risk is
// the Tracks table). Reuses AlbumCollectionView; covers load per card.

fn fetch_folder_albums() -> Vec<crate::album_map::AlbumCard> {
    crate::library_db::with_db(|db| {
        db.get_albums_with_full_filter(
            /* include_hidden */ false,
            /* include_qobuz_downloads */ true,
            /* exclude_network_folders */ false,
        )
    })
    .unwrap_or_default()
    .into_iter()
    .map(map_local_album)
    .collect()
}

fn folder_artwork_jobs(cards: &[crate::album_map::AlbumCard]) -> Vec<ArtworkJob> {
    cards
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.artwork_url.is_empty())
        .map(|(i, c)| ArtworkJob {
            target: ArtworkTarget::LocalFolderCard { index: i },
            url: c.artwork_url.clone(),
        })
        .collect()
}

fn apply_folders(window: &AppWindow, cards: Vec<crate::album_map::AlbumCard>) {
    let items: Vec<AlbumCardItem> = cards.into_iter().map(crate::album_map::to_item).collect();
    let s = window.global::<LocalLibraryState>();
    s.set_folders(ModelRc::new(VecModel::from(items)));
    s.set_folders_loading(false);
    s.set_folders_load_failed(false);
    derive_folders(window);
}

/// First-letter bucket key for alpha grouping (`#` for non-alphabetic).
/// Mirrors favorites' `album_alpha_key` so the two surfaces sort identically.
fn folder_alpha_key(title: &str) -> String {
    title
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| {
            let up = c.to_uppercase().next().unwrap_or(c);
            if up.is_ascii_digit() {
                "#".to_string()
            } else {
                up.to_string()
            }
        })
        .unwrap_or_else(|| "#".to_string())
}

/// Re-derive the flat-mode visible / grouped folder models from the full
/// `folders` set, applying the toolbar's search query, sort key and group
/// mode. Mirrors `favorites::derive_albums` so behaviour is identical to the
/// metadata Albums tab; the only difference is the source model (directory
/// grouping instead of metadata grouping).
pub fn derive_folders(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    let query_owned = s.get_folders_search().to_lowercase();
    let query = query_owned.trim();
    let sort = s.get_folders_sort().to_string();
    let group = s.get_folders_group().to_string();
    let all = s.get_folders();
    let empty_sections = || ModelRc::new(VecModel::from(Vec::<DiscoverSection>::new()));

    let mut filtered: Vec<AlbumCardItem> = (0..all.row_count())
        .filter_map(|i| all.row_data(i))
        .filter(|a| {
            query.is_empty()
                || a.title.to_lowercase().contains(query)
                || a.artist.to_lowercase().contains(query)
        })
        .collect();
    crate::album_map::sort_album_items(&mut filtered, &sort);

    if group == "off" {
        s.set_folders_visible(ModelRc::new(VecModel::from(filtered)));
        s.set_folders_grouped(empty_sections());
        return;
    }

    // Grouped: bucket by artist name, or by the title's first letter
    // (`#` for non-alphabetic). Sections ordered alphabetically, `#` last.
    let mut map: Vec<(String, Vec<AlbumCardItem>)> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for item in filtered {
        let key = if group == "artist" {
            let a = item.artist.to_string();
            if a.is_empty() {
                "Unknown".to_string()
            } else {
                a
            }
        } else {
            folder_alpha_key(item.title.as_str())
        };
        let idx = *index.entry(key.clone()).or_insert_with(|| {
            map.push((key.clone(), Vec::new()));
            map.len() - 1
        });
        map[idx].1.push(item);
    }
    map.sort_by(|(a, _), (b, _)| match (a == "#", b == "#") {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    });
    let sections: Vec<DiscoverSection> = map
        .into_iter()
        .map(|(key, items)| DiscoverSection {
            title: key.into(),
            endpoint: "".into(),
            albums: ModelRc::new(VecModel::from(items)),
        })
        .collect();
    s.set_folders_grouped(ModelRc::new(VecModel::from(sections)));
    s.set_folders_visible(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
}

/// Load the folder-grouped album grid on first visit (re-entry keeps it).
pub fn ensure_folders_loaded(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<LocalLibraryState>();
        if s.get_folders().row_count() != 0 || s.get_folders_loading() {
            return;
        }
        s.set_folders_loading(true);
        s.set_folders_load_failed(false);
        let weak2 = w.as_weak();
        handle.spawn(async move {
            let cards = tokio::task::spawn_blocking(fetch_folder_albums)
                .await
                .unwrap_or_default();
            let _ = weak2.upgrade_in_event_loop(move |w| {
                let jobs = folder_artwork_jobs(&cards);
                apply_folders(&w, cards);
                crate::artwork::spawn_local_loads(jobs, w.as_weak(), image_cache);
            });
        });
    });
}

// ============================ Folders (tree) ==============================
//
// Slint has no native TreeView and no self-recursive component, so the tree
// is rendered as a flattened list of visible nodes (`FolderNode`) in a
// ListView: each node carries its `depth` (drives the indent) and an
// `expanded` flag. Expanding fetches one level lazily via
// `list_folder_children`; collapsing drops the contiguous descendant block.

/// Last path component for display (the registered root paths are absolute).
fn path_basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Map a backend tree entry to a flattened `FolderNode` at the given depth.
fn entry_to_node(entry: &qbz_library::FolderTreeEntry, depth: i32) -> FolderNode {
    match entry {
        qbz_library::FolderTreeEntry::Folder {
            path,
            segment,
            track_count_under,
            ..
        } => FolderNode {
            path: path.clone().into(),
            segment: segment.clone().into(),
            depth,
            is_folder: true,
            expanded: false,
            can_expand: *track_count_under > 0,
            track_count: *track_count_under as i32,
        },
        qbz_library::FolderTreeEntry::Track { path, segment } => FolderNode {
            path: path.clone().into(),
            segment: segment.clone().into(),
            depth,
            is_folder: false,
            expanded: false,
            can_expand: false,
            track_count: 0,
        },
    }
}

/// Load the tree roots (registered library folders) on first switch to tree
/// mode. Each root gets a recursive track count so the rail can show totals
/// and gate the expand affordance.
pub fn ensure_folder_tree_loaded(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<LocalLibraryState>();
        if s.get_folder_tree().row_count() != 0 || s.get_folder_tree_loading() {
            return;
        }
        s.set_folder_tree_loading(true);
        let weak2 = w.as_weak();
        handle.spawn(async move {
            let roots = tokio::task::spawn_blocking(|| {
                crate::library_db::with_db(|db| {
                    let paths = db.get_folders()?;
                    let mut out: Vec<(String, u32)> = Vec::with_capacity(paths.len());
                    for p in paths {
                        let cnt = db.count_folder_tracks_recursive(&p, false)?;
                        out.push((p, cnt));
                    }
                    Ok::<_, qbz_library::LibraryError>(out)
                })
                .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            let _ = weak2.upgrade_in_event_loop(move |w| {
                let nodes: Vec<FolderNode> = roots
                    .into_iter()
                    .map(|(p, cnt)| FolderNode {
                        segment: path_basename(&p).into(),
                        path: p.into(),
                        depth: 0,
                        is_folder: true,
                        expanded: false,
                        can_expand: cnt > 0,
                        track_count: cnt as i32,
                    })
                    .collect();
                let s = w.global::<LocalLibraryState>();
                s.set_folder_tree(ModelRc::new(VecModel::from(nodes)));
                s.set_folder_tree_loading(false);
            });
        });
    });
}

/// Collect the current flattened tree into a plain vec for splicing.
fn collect_tree(s: &LocalLibraryState) -> Vec<FolderNode> {
    let m = s.get_folder_tree();
    (0..m.row_count()).filter_map(|i| m.row_data(i)).collect()
}

/// Expand or collapse a folder node. Collapsing is pure UI (drop the
/// contiguous descendant block); expanding fetches one child level lazily.
pub fn toggle_folder_node(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    path: String,
    expand: bool,
) {
    if !expand {
        let _ = weak.upgrade_in_event_loop(move |w| {
            let s = w.global::<LocalLibraryState>();
            let mut nodes = collect_tree(&s);
            if let Some(pos) = nodes.iter().position(|n| n.path == path) {
                let depth = nodes[pos].depth;
                nodes[pos].expanded = false;
                let mut end = pos + 1;
                while end < nodes.len() && nodes[end].depth > depth {
                    end += 1;
                }
                nodes.drain(pos + 1..end);
                s.set_folder_tree(ModelRc::new(VecModel::from(nodes)));
            }
        });
        return;
    }
    // Expand: fetch this level's children off-thread, then splice them in.
    let path_for_fetch = path.clone();
    handle.spawn(async move {
        let children = tokio::task::spawn_blocking(move || {
            crate::library_db::with_db(|db| db.list_folder_children(&path_for_fetch, false))
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        let _ = weak.upgrade_in_event_loop(move |w| {
            let s = w.global::<LocalLibraryState>();
            let mut nodes = collect_tree(&s);
            if let Some(pos) = nodes.iter().position(|n| n.path == path) {
                let depth = nodes[pos].depth;
                nodes[pos].expanded = true;
                let child_nodes: Vec<FolderNode> = children
                    .iter()
                    .map(|e| entry_to_node(e, depth + 1))
                    .collect();
                for (i, cn) in child_nodes.into_iter().enumerate() {
                    nodes.insert(pos + 1 + i, cn);
                }
                s.set_folder_tree(ModelRc::new(VecModel::from(nodes)));
            }
        });
    });
}

/// Select a folder in the tree: load its detail pane — direct child tracks
/// plus immediate subfolders (for in-pane drill-down).
pub fn select_folder(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    path: String,
    segment: String,
) {
    let _ = weak.upgrade_in_event_loop({
        let path = path.clone();
        let segment = segment.clone();
        move |w| {
            let s = w.global::<LocalLibraryState>();
            s.set_folders_selected_path(path.clone().into());
            s.set_folders_selected_name(segment.clone().into());
            s.set_folder_detail_loading(true);
        }
    });
    let path_for_fetch = path.clone();
    handle.spawn(async move {
        let (tracks, subfolders) = tokio::task::spawn_blocking(move || {
            let tracks = crate::library_db::with_db(|db| {
                db.list_folder_tracks(&path_for_fetch, false)
            })
            .unwrap_or_default();
            let children = crate::library_db::with_db(|db| {
                db.list_folder_children(&path_for_fetch, false)
            })
            .unwrap_or_default();
            (tracks, children)
        })
        .await
        .unwrap_or_default();
        let _ = weak.upgrade_in_event_loop(move |w| {
            let track_items: Vec<TrackItem> = tracks.into_iter().map(map_local_track).collect();
            let sub_nodes: Vec<FolderNode> = subfolders
                .iter()
                .filter(|e| matches!(e, qbz_library::FolderTreeEntry::Folder { .. }))
                .map(|e| entry_to_node(e, 0))
                .collect();
            let s = w.global::<LocalLibraryState>();
            s.set_folder_detail_tracks(ModelRc::new(VecModel::from(track_items)));
            s.set_folder_detail_subfolders(ModelRc::new(VecModel::from(sub_nodes)));
            s.set_folder_detail_loading(false);
        });
    });
}

// ============================== Artists tab ===============================
//
// Two-column master/detail, 1:1 with Tauri's Artists tab (NOT a card grid —
// the Svelte VirtualizedArtistGrid imports there are dead). Left rail: a
// merged/deduped, alpha-grouped master list of compact rows (round avatar +
// name + "N albums · M tracks"). Right pane: the selected artist's albums,
// filtered IN PLACE from the loaded album set (no new backend call). The
// name-merge collapses normalized-equal spellings into one canonical row.
//
// The Qobuz background image fetch (capped/sequential in Tauri, and whose DB
// batch path is broken there) is the remaining follow-up; this pass wires the
// DB custom-image path + the mic placeholder.

/// The loaded album set, cached so the right-pane filter (select) doesn't
/// re-hit the DB — mirrors Tauri filtering its in-memory `albums` array.
static ARTIST_ALBUMS: std::sync::Mutex<Vec<qbz_library::LocalAlbum>> =
    std::sync::Mutex::new(Vec::new());

/// Fold a common Latin accented char to its ASCII base (best-effort, no
/// `unicode-normalization` dep). Covers Spanish/European music metadata; the
/// uncovered tail just won't merge across diacritics. Mirrors the intent of
/// Tauri's NFKD + combining-mark strip in `normalizeArtistName`.
fn fold_diacritic(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ė' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'ī' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' | 'ø' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'ū' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        'ý' | 'ÿ' => 'y',
        'ß' => 's',
        _ => c,
    }
}

/// Normalize an artist name for merge/match: lowercase, fold diacritics,
/// collapse every run of non-alphanumerics to a single space, trim. So
/// "Alice In Chains" and "alice  in chains" both -> "alice in chains".
fn normalize_artist(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_space = false;
    for ch in name.to_lowercase().chars() {
        let c = fold_diacritic(ch);
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

/// Split a credit string into individual artist names on the usual
/// separators (comma already handled by the caller for `all_artists`).
fn split_credit(s: &str) -> Vec<String> {
    s.split([',', '&', '/', ';'])
        .flat_map(|p| {
            p.split(" feat ")
                .flat_map(|q| q.split(" ft "))
                .flat_map(|q| q.split(" featuring "))
                .flat_map(|q| q.split(" with "))
                .map(|q| q.to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Build the per-normalized-artist set of album ids, so merged rows get an
/// accurate unique album count independent of per-track spelling. Mirrors
/// Tauri's `artistAlbumIds`.
fn build_artist_album_ids(
    albums: &[qbz_library::LocalAlbum],
) -> std::collections::HashMap<String, std::collections::HashSet<String>> {
    let mut map: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for al in albums {
        if !al.all_artists.is_empty() {
            for part in al.all_artists.split(',') {
                let n = normalize_artist(part);
                if n.is_empty() || n == "various artists" {
                    continue;
                }
                map.entry(n).or_default().insert(al.id.clone());
            }
        } else {
            let n = normalize_artist(&al.artist);
            if !n.is_empty() && n != "various artists" {
                map.entry(n).or_default().insert(al.id.clone());
            }
        }
    }
    map
}

/// Send-safe merged-artist row (no `slint::Image`, so it can cross the
/// `spawn_blocking` boundary). Converted to `LocalArtistItem` on the UI thread
/// in `apply_artists` (where the non-Send decoded `image` is added).
struct ArtistRow {
    name: String,
    display_name: String,
    album_count: i32,
    track_count: i32,
    image_path: String,
}

/// Collapse normalized-equal artist spellings into one canonical row and
/// attach accurate album counts + a custom-image path. Mirrors Tauri's
/// `artistMergeResult`: canonical = the variant with most albums (tie: most
/// tracks); merged track count = sum across variants.
fn merge_artists(
    artists: Vec<qbz_library::LocalArtist>,
    albums: &[qbz_library::LocalAlbum],
    custom_images: &std::collections::HashMap<String, String>,
) -> Vec<ArtistRow> {
    let album_ids = build_artist_album_ids(albums);
    let norm_imgs: std::collections::HashMap<String, String> = custom_images
        .iter()
        .map(|(k, v)| (normalize_artist(k), v.clone()))
        .collect();

    let mut groups: std::collections::HashMap<String, Vec<qbz_library::LocalArtist>> =
        std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for a in artists {
        let n = normalize_artist(&a.name);
        if n.is_empty() {
            continue;
        }
        if !groups.contains_key(&n) {
            order.push(n.clone());
        }
        groups.entry(n).or_default().push(a);
    }

    let mut out: Vec<ArtistRow> = Vec::with_capacity(order.len());
    for n in order {
        let variants = match groups.remove(&n) {
            Some(v) => v,
            None => continue,
        };
        let album_set_len = album_ids.get(&n).map(|s| s.len()).unwrap_or(0) as i32;
        let (canonical, album_count, track_count) = if variants.len() == 1 {
            let v = &variants[0];
            let ac = if album_set_len > 0 {
                album_set_len
            } else {
                v.album_count as i32
            };
            (v.name.clone(), ac, v.track_count as i32)
        } else {
            let canon = variants
                .iter()
                .max_by(|a, b| {
                    a.album_count
                        .cmp(&b.album_count)
                        .then(a.track_count.cmp(&b.track_count))
                })
                .unwrap();
            let total_tracks: u32 = variants.iter().map(|v| v.track_count).sum();
            let ac = if album_set_len > 0 {
                album_set_len
            } else {
                canon.album_count as i32
            };
            (canon.name.clone(), ac, total_tracks as i32)
        };
        let image_path = norm_imgs.get(&n).cloned().unwrap_or_default();
        out.push(ArtistRow {
            name: canonical.clone(),
            display_name: canonical,
            album_count,
            track_count,
            image_path,
        });
    }
    out.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
    out
}

/// Does this album credit the (normalized) selected artist — as primary, in
/// `all_artists`, or as one part of a multi-artist credit? Mirrors Tauri's
/// `selectedArtistAlbums` predicate.
fn album_matches_artist(al: &qbz_library::LocalAlbum, nsel: &str) -> bool {
    if nsel == "various artists" {
        return normalize_artist(&al.artist) == "various artists";
    }
    if normalize_artist(&al.artist) == nsel {
        return true;
    }
    for part in al.all_artists.split(',') {
        if normalize_artist(part) == nsel {
            return true;
        }
    }
    for part in split_credit(&al.artist) {
        if normalize_artist(&part) == nsel {
            return true;
        }
    }
    false
}

/// Re-derive the left-rail render sets (search filter + A-Z grouping + jump
/// strip) from the merged `artists` master list.
pub fn derive_artists(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    let query_owned = s.get_artists_search().to_lowercase();
    let query = query_owned.trim();
    let all = s.get_artists();
    let filtered: Vec<LocalArtistItem> = (0..all.row_count())
        .filter_map(|i| all.row_data(i))
        .filter(|a| query.is_empty() || a.display_name.to_lowercase().contains(query))
        .collect();
    s.set_artists_shown(filtered.len() as i32);

    let mut map: Vec<(String, Vec<LocalArtistItem>)> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for item in filtered {
        let key = folder_alpha_key(item.display_name.as_str());
        let idx = *index.entry(key.clone()).or_insert_with(|| {
            map.push((key.clone(), Vec::new()));
            map.len() - 1
        });
        map[idx].1.push(item);
    }
    map.sort_by(|(a, _), (b, _)| match (a == "#", b == "#") {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    });
    let jumps: Vec<AlphaJump> = map
        .iter()
        .enumerate()
        .map(|(i, (k, _))| AlphaJump {
            letter: k.clone().into(),
            index: i as i32,
        })
        .collect();
    let sections: Vec<LocalArtistSection> = map
        .into_iter()
        .map(|(letter, artists)| LocalArtistSection {
            letter: letter.into(),
            artists: ModelRc::new(VecModel::from(artists)),
        })
        .collect();
    s.set_artists_grouped(ModelRc::new(VecModel::from(sections)));
    s.set_artists_alpha(ModelRc::new(VecModel::from(jumps)));
}

fn apply_artists(window: &AppWindow, rows: Vec<ArtistRow>) {
    // Build the Slint items here (UI thread) — `LocalArtistItem.image` holds a
    // non-Send `slint::Image`, so the rows crossed `spawn_blocking` as the
    // Send-safe `ArtistRow` and gain the (default-empty) decoded image now.
    let items: Vec<LocalArtistItem> = rows
        .into_iter()
        .map(|r| LocalArtistItem {
            name: r.name.into(),
            display_name: r.display_name.into(),
            album_count: r.album_count,
            track_count: r.track_count,
            image_path: r.image_path.into(),
            image: slint::Image::default(),
        })
        .collect();
    let s = window.global::<LocalLibraryState>();
    s.set_artists(ModelRc::new(VecModel::from(items)));
    s.set_artists_loading(false);
    s.set_artists_load_failed(false);
    derive_artists(window);
}

/// Generation guard for the artist-image fetch: bumped on every artists load
/// so a stale in-flight fetch/decode (from a superseded list) is dropped.
static ARTISTS_IMG_GEN: AtomicU64 = AtomicU64::new(0);

/// True if `gen` is still the current artist-image generation (the apply arm
/// checks this before painting a portrait).
pub fn artists_img_gen_current() -> u64 {
    ARTISTS_IMG_GEN.load(Ordering::SeqCst)
}

/// Set a freshly-decoded portrait (by artist `name`) on BOTH the flat master
/// (`artists`, so a later `derive_artists` carries it forward) and every
/// rendered grouped section (`artists-grouped[*].artists`). Mirrors
/// `favorites::set_album_artwork`. The `String` name lives here, not in the
/// `Copy` artwork target.
pub fn set_artist_row_image(window: &AppWindow, name: &str, image: slint::Image) {
    let s = window.global::<LocalLibraryState>();
    let flat = s.get_artists();
    for i in 0..flat.row_count() {
        if let Some(mut it) = flat.row_data(i) {
            if it.name.as_str() == name {
                it.image = image.clone();
                flat.set_row_data(i, it);
                break;
            }
        }
    }
    let grouped = s.get_artists_grouped();
    for sx in 0..grouped.row_count() {
        if let Some(sec) = grouped.row_data(sx) {
            for r in 0..sec.artists.row_count() {
                if let Some(mut it) = sec.artists.row_data(r) {
                    if it.name.as_str() == name {
                        it.image = image.clone();
                        sec.artists.set_row_data(r, it);
                        break;
                    }
                }
            }
        }
    }
}

/// Load + merge the artists master list on first visit (re-entry keeps it).
/// Also caches the album set for the right-pane filter, seeds decode jobs for
/// rows that already have an image (custom or previously-cached Qobuz), and
/// kicks the capped background fetch for the rest.
pub fn ensure_artists_loaded(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<LocalLibraryState>();
        if s.get_artists().row_count() != 0 || s.get_artists_loading() {
            return;
        }
        s.set_artists_loading(true);
        s.set_artists_load_failed(false);
        let gen = ARTISTS_IMG_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        let weak2 = w.as_weak();
        let handle_inner = handle.clone();
        handle.spawn(async move {
            let items = tokio::task::spawn_blocking(|| {
                let artists = crate::library_db::with_db(|db| db.get_artists_with_filter(true, false))
                    .unwrap_or_default();
                let albums = crate::library_db::with_db(|db| {
                    db.get_albums_with_full_filter(false, true, false)
                })
                .unwrap_or_default();
                // Seed custom AND previously-cached Qobuz portraits (fixes the
                // Tauri headline bug: its batch load command was never wired).
                let custom = crate::library_db::with_db(|db| db.get_all_artist_image_urls())
                    .unwrap_or_default();
                let merged = merge_artists(artists, &albums, &custom);
                if let Ok(mut cache) = ARTIST_ALBUMS.lock() {
                    *cache = albums;
                }
                merged
            })
            .await
            .unwrap_or_default();
            let _ = weak2.upgrade_in_event_loop(move |w| {
                apply_artists(&w, items);
                // Seed decode jobs for rows that already carry an image-path.
                let s = w.global::<LocalLibraryState>();
                let artists = s.get_artists();
                let mut local_jobs = Vec::new();
                let mut http_jobs = Vec::new();
                for i in 0..artists.row_count() {
                    if let Some(a) = artists.row_data(i) {
                        let p = a.image_path.to_string();
                        if p.is_empty() {
                            continue;
                        }
                        let job = ArtworkJob {
                            target: ArtworkTarget::LocalArtistRowImage { index: i, gen },
                            url: p.clone(),
                        };
                        if p.starts_with("http") {
                            http_jobs.push(job);
                        } else {
                            local_jobs.push(job);
                        }
                    }
                }
                crate::artwork::spawn_local_loads(local_jobs, w.as_weak(), image_cache.clone());
                crate::artwork::spawn_loads(http_jobs, w.as_weak(), image_cache.clone());
                // Kick the capped Qobuz portrait fetch for missing rows.
                if s.get_artists_fetch_images() {
                    fetch_missing_artist_images(
                        runtime,
                        w.as_weak(),
                        handle_inner,
                        image_cache,
                        gen,
                    );
                }
            });
        });
    });
}

/// Capped, sequential background fetch of missing artist portraits from Qobuz
/// (max 50/session, 1s apart, exact-normalized match only). 1:1 with Tauri's
/// `fetchMissingArtistImages`, with per-image immediate paint + a generation
/// guard. Names with an image already are skipped (snapshotted on the UI
/// thread; the worker never touches the Slint model except via event-loop hops).
pub fn fetch_missing_artist_images(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    gen: u64,
) {
    // Snapshot the names missing a portrait, on the UI thread.
    let (tx, rx) = std::sync::mpsc::channel::<Vec<String>>();
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<LocalLibraryState>();
        s.set_artists_images_fetching(true);
        s.set_artists_images_fetched(0);
        let flat = s.get_artists();
        let mut names = Vec::new();
        for i in 0..flat.row_count() {
            if let Some(a) = flat.row_data(i) {
                if !a.image_path.is_empty() {
                    continue;
                }
                let name = a.name.to_string();
                if normalize_artist(&name) == "various artists" {
                    continue;
                }
                names.push(name);
            }
        }
        let _ = tx.send(names);
    });
    let mut names = rx.recv().unwrap_or_default();
    names.truncate(50);
    if names.is_empty() {
        let _ = weak.upgrade_in_event_loop(|w| {
            w.global::<LocalLibraryState>().set_artists_images_fetching(false);
        });
        return;
    }

    handle.spawn(async move {
        let mut painted = 0i32;
        for name in names {
            if artists_img_gen_current() != gen {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            if artists_img_gen_current() != gen {
                break;
            }
            let page = match runtime.core().search_artists(&name, 3, 0, None).await {
                Ok(p) => p,
                Err(qbz_core::CoreError::NotInitialized) => break,
                Err(e) => {
                    log::debug!("[locallibrary] artist image search failed for {name}: {e}");
                    continue;
                }
            };
            let nsel = normalize_artist(&name);
            let matched = page
                .items
                .into_iter()
                .find(|a| normalize_artist(&a.name) == nsel);
            let Some(artist) = matched else {
                continue; // no exact match -> skip (no wrong-artist persist)
            };
            let Some(url) = artist.image.as_ref().and_then(|i| i.best().cloned()) else {
                continue;
            };

            // Persist the fetched portrait (best-effort; paints regardless).
            let name_c = name.clone();
            let url_c = url.clone();
            let canon = artist.name.clone();
            let _ = tokio::task::spawn_blocking(move || {
                crate::library_db::with_db(|db| {
                    db.cache_artist_image_with_canonical(
                        &name_c,
                        Some(&url_c),
                        "qobuz",
                        None,
                        Some(&canon),
                    )
                })
            })
            .await;

            // Paint now: resolve the current flat-master index on the UI thread.
            let url_p = url.clone();
            let name_p = name.clone();
            let cache = image_cache.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                let s = w.global::<LocalLibraryState>();
                let flat = s.get_artists();
                let mut idx = None;
                for i in 0..flat.row_count() {
                    if let Some(a) = flat.row_data(i) {
                        if a.name.as_str() == name_p {
                            idx = Some(i);
                            break;
                        }
                    }
                }
                if let Some(i) = idx {
                    crate::artwork::spawn_loads(
                        vec![ArtworkJob {
                            target: ArtworkTarget::LocalArtistRowImage { index: i, gen },
                            url: url_p,
                        }],
                        w.as_weak(),
                        cache,
                    );
                }
            });
            painted += 1;
        }
        let _ = weak.upgrade_in_event_loop(move |w| {
            let s = w.global::<LocalLibraryState>();
            s.set_artists_images_fetching(false);
            s.set_artists_images_fetched(painted);
        });
    });
}

/// Select an artist: filter their albums (in place, from the cached album
/// set) into the right pane and kick cover loads.
pub fn select_local_artist(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    name: String,
) {
    let _ = weak.upgrade_in_event_loop({
        let name = name.clone();
        move |w| {
            let s = w.global::<LocalLibraryState>();
            s.set_artists_selected_name(name.clone().into());
            // Display name = the merged row's display, else the raw name.
            let all = s.get_artists();
            let display = (0..all.row_count())
                .filter_map(|i| all.row_data(i))
                .find(|a| a.name == name)
                .map(|a| a.display_name.to_string())
                .unwrap_or_else(|| name.clone());
            s.set_artists_selected_display(display.into());
            s.set_artists_selected_loading(true);
        }
    });
    handle.spawn(async move {
        let cards = tokio::task::spawn_blocking(move || {
            let albums = ARTIST_ALBUMS
                .lock()
                .map(|c| c.clone())
                .unwrap_or_default();
            let nsel = normalize_artist(&name);
            albums
                .into_iter()
                .filter(|al| album_matches_artist(al, &nsel))
                .map(map_local_album)
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let _ = weak.upgrade_in_event_loop(move |w| {
            let jobs: Vec<ArtworkJob> = cards
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.artwork_url.is_empty())
                .map(|(i, c)| ArtworkJob {
                    target: ArtworkTarget::LocalArtistAlbumCard { index: i },
                    url: c.artwork_url.clone(),
                })
                .collect();
            let items: Vec<AlbumCardItem> =
                cards.into_iter().map(crate::album_map::to_item).collect();
            let s = w.global::<LocalLibraryState>();
            s.set_artists_selected_albums(ModelRc::new(VecModel::from(items)));
            s.set_artists_selected_loading(false);
            crate::artwork::spawn_local_loads(jobs, w.as_weak(), image_cache);
        });
    });
}

/// Fetch an album's tracks by group key, trying the metadata grouping first
/// (Albums tab) then the folder grouping (Folders tab). Blocking.
pub fn fetch_album_tracks_blocking(group_key: &str) -> Vec<qbz_library::LocalTrack> {
    crate::library_db::with_db(|db| {
        let meta = db.get_album_tracks_metadata(group_key)?;
        if !meta.is_empty() {
            return Ok(meta);
        }
        db.get_album_tracks(group_key)
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_and_tab_id_round_trip() {
        for (route, id, tab) in [
            ("local-tracks", "tracks", LibTab::Tracks),
            ("local-folders", "folders", LibTab::Folders),
            ("local-albums", "albums", LibTab::Albums),
            ("local-artists", "artists", LibTab::Artists),
        ] {
            assert_eq!(LibTab::from_route(route), Some(tab));
            assert_eq!(LibTab::from_tab_id(id), Some(tab));
            assert_eq!(tab.tab_id(), id);
        }
        assert_eq!(LibTab::from_route("favorites-albums"), None);
        assert_eq!(LibTab::from_tab_id("bogus"), None);
    }
}
