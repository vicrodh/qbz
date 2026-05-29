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
use crate::{AlbumCardItem, AppWindow, LocalLibraryState};

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
    let page = crate::library_db::with_db(|db| {
        db.get_albums_metadata_page(
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
        )
    })?;
    let total = page.total;
    let has_more = offset + (page.albums.len() as u64) < total;
    let cards = page.albums.into_iter().map(map_local_album).collect();
    Some((cards, total, has_more))
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
