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

use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget, ImageCache};
use crate::{AlbumCardItem, AppWindow, LocalArtistItem, LocalLibraryState, TrackItem};

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
    let items: Vec<TrackItem> = rows.into_iter().map(map_local_track).collect();
    let s = window.global::<LocalLibraryState>();
    let n = items.len() as i32;
    s.set_tracks(ModelRc::new(VecModel::from(items)));
    s.set_tracks_next_offset(n);
    s.set_tracks_has_more(has_more);
    s.set_tracks_loading(false);
    s.set_tracks_loading_more(false);
    s.set_tracks_load_failed(false);
}

fn append_tracks(window: &AppWindow, rows: Vec<qbz_library::LocalTrack>, has_more: bool) {
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

// ============================== Artists tab ===============================
//
// A full-loaded grid of artists (name + album/track counts). Images +
// click-to-detail are a later slice; merge/dedup of normalized-equal names
// (the Tauri artistMergeResult) is deferred — v1 shows the DB's distinct
// album-artist rows.

fn apply_artists(window: &AppWindow, artists: Vec<qbz_library::LocalArtist>) {
    let items: Vec<LocalArtistItem> = artists
        .into_iter()
        .map(|a| LocalArtistItem {
            name: a.name.into(),
            subtitle: format!("{} albums · {} tracks", a.album_count, a.track_count).into(),
        })
        .collect();
    let s = window.global::<LocalLibraryState>();
    s.set_artists(ModelRc::new(VecModel::from(items)));
    s.set_artists_loading(false);
    s.set_artists_load_failed(false);
}

/// Load the artists grid on first visit (re-entry keeps it).
pub fn ensure_artists_loaded(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<LocalLibraryState>();
        if s.get_artists().row_count() != 0 || s.get_artists_loading() {
            return;
        }
        s.set_artists_loading(true);
        s.set_artists_load_failed(false);
        let weak2 = w.as_weak();
        handle.spawn(async move {
            let artists = tokio::task::spawn_blocking(|| {
                crate::library_db::with_db(|db| db.get_artists_with_filter(true, false))
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            let _ = weak2.upgrade_in_event_loop(move |w| apply_artists(&w, artists));
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
