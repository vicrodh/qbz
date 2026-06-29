//! Local Library controller (Slint) — greenfield port of Tauri's
//! `LocalLibraryView`.
//!
//! This module owns the per-tab navigation and (slice by slice) the data
//! loading for the four browse tabs: Albums / Artists / Folders / Tracks.
//! It reads the shared per-user `library.db` through the already
//! frontend-agnostic `qbz-library` crate (see `library_db::with_db`).
//!
//! Plex (2026-06-07): the Plex port is landing slice by slice — spec at
//! `qbz-nix-docs/plex-integration/2026-06-07-plex-slint-build-spec.md`.
//! WIRED so far: creds + PIN auth (Settings > Local Library), the Albums tab
//! (grid union + album-detail tracks + covers), the Plex play path, and (slice
//! 3c) the Artists rail (client-side artist aggregation + Plex-union album
//! cache + source-aware portraits) and the flat Tracks tab (full Plex set
//! merged once on page 1, source-aware playback). NOT yet wired: quality
//! hydration (cached values only), and Plex track-row covers (the Tracks tab
//! keeps the documented anti-freeze design of no per-row decode jobs).
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
    AlbumCardItem, AlphaJump, AppWindow, DiscoverSection, EphemeralAlbum, FolderNode,
    FolderSubcardItem, LocalArtistItem, LocalArtistSection, LocalLibraryState, TrackItem,
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
// The Albums tab browses the metadata-grouped albums via
// `get_albums_metadata_page(…, Some(plex_cache_path))` — the `plex_aggregated`
// union surfaces Plex albums alongside local ones when Plex is enabled
// (`plex_cache_db_path()` returns `None` when disabled → local-only). Sort,
// search, group + filter are derived client-side over the full-loaded set.
// Covers load via the source-aware artwork pipeline
// (`spawn_local_or_plex_loads`): local files from disk, Plex `/library/...`
// thumbs via `ArtworkRef::PlexThumb`.

/// Generation guard, bumped on every (re)load. A stale in-flight
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
    // Format-first classification (mirrors Tauri): a lossy format (MP3) gets
    // the dedicated MP3 badge tier, never CD.
    // One shared classifier (see crate::quality::badge) so the card, the
    // album-detail header and the track rows can never disagree — and so an
    // un-hydrated lossless Plex album shows a generic "FLAC" badge instead of
    // nothing. `a.sample_rate` is Hz; `badge` normalizes it to kHz (guarded).
    let (tier, quality_detail, quality_label) =
        crate::quality::badge(&a.format.to_string(), a.bit_depth, Some(a.sample_rate));
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

/// The full metadata-grouped LocalAlbum set — the FILTER SOURCE (the quality/
/// format/source filters need raw bit_depth/format/source, which AlbumCardItem
/// doesn't carry). Loaded once; `derive_albums` filters it by id.
static LOCAL_ALBUMS: LazyLock<Mutex<Vec<qbz_library::LocalAlbum>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

fn local_albums() -> std::sync::MutexGuard<'static, Vec<qbz_library::LocalAlbum>> {
    LOCAL_ALBUMS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Active quality/format/source filter (read once per derive from the global).
#[derive(Clone, Copy, Default)]
struct AlbumFilter {
    hires: bool,
    cd: bool,
    lossy: bool,
    flac: bool,
    alac: bool,
    ape: bool,
    wav: bool,
    mp3: bool,
    aac: bool,
    other: bool,
    local: bool,
    offline: bool,
    plex: bool,
}

fn read_album_filter(window: &AppWindow) -> AlbumFilter {
    let f = window.global::<crate::LibAlbumFilterState>();
    AlbumFilter {
        hires: f.get_hires(),
        cd: f.get_cd(),
        lossy: f.get_lossy(),
        flac: f.get_flac(),
        alac: f.get_alac(),
        ape: f.get_ape(),
        wav: f.get_wav(),
        mp3: f.get_mp3(),
        aac: f.get_aac(),
        other: f.get_other(),
        local: f.get_local(),
        offline: f.get_offline(),
        plex: f.get_plex(),
    }
}

fn album_filter_count(f: &AlbumFilter) -> i32 {
    [
        f.hires, f.cd, f.lossy, f.flac, f.alac, f.ape, f.wav, f.mp3, f.aac, f.other, f.local,
        f.offline, f.plex,
    ]
    .iter()
    .filter(|b| **b)
    .count() as i32
}

/// 1:1 with Tauri `matchesQualityFilters`: OR within each group, AND between
/// groups; an empty group passes everything.
fn album_matches_filters(a: &qbz_library::LocalAlbum, f: &AlbumFilter) -> bool {
    let format = a.format.to_string().to_lowercase();
    let lossless = matches!(
        format.as_str(),
        "flac" | "wav" | "aiff" | "alac" | "ape" | "dsd" | "dsf" | "dff"
    );
    let lossy = matches!(format.as_str(), "mp3" | "aac" | "m4a" | "ogg" | "opus" | "wma");
    let bit_depth = a.bit_depth.unwrap_or(16);

    let q_active = f.hires || f.cd || f.lossy;
    let passes_q = !q_active
        || (f.hires && lossless && (bit_depth >= 24 || a.sample_rate > 48000.0))
        || (f.cd && lossless && bit_depth <= 16 && a.sample_rate <= 48000.0)
        || (f.lossy && lossy);

    let fmt_active = f.flac || f.alac || f.ape || f.wav || f.mp3 || f.aac || f.other;
    let passes_f = !fmt_active
        || (f.flac && format == "flac")
        || (f.alac && (format == "alac" || format == "m4a"))
        || (f.ape && format == "ape")
        || (f.wav && (format == "wav" || format == "wave"))
        || (f.mp3 && format == "mp3")
        || (f.aac && (format == "aac" || format == "m4a"))
        || (f.other
            && !matches!(
                format.as_str(),
                "flac" | "alac" | "ape" | "wav" | "wave" | "mp3" | "aac" | "m4a"
            ));

    let s_active = f.local || f.offline || f.plex;
    let src = a.source.as_str();
    let passes_s = !s_active
        || (f.local && (src == "user" || src.is_empty()))
        || (f.offline && src == "qobuz_download")
        || (f.plex && src == "plex");

    passes_q && passes_f && passes_s
}

/// Build LocalAlbumCard artwork jobs for the full `albums` set (gen-stamped).
fn album_artwork_jobs(cards: &[crate::album_map::AlbumCard], gen: u64) -> Vec<ArtworkJob> {
    cards
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.artwork_url.is_empty())
        .map(|(i, c)| ArtworkJob {
            target: ArtworkTarget::LocalAlbumCard { index: i, gen },
            url: c.artwork_url.clone(),
        })
        .collect()
}

/// Set a freshly-decoded local-album cover (by id) on every rendered model:
/// the full `albums` set + `albums-visible` + each `albums-grouped` section.
pub fn set_local_album_artwork(window: &AppWindow, id: &str, image: slint::Image) {
    let s = window.global::<LocalLibraryState>();
    let set_in = |m: &ModelRc<AlbumCardItem>| {
        for i in 0..m.row_count() {
            if let Some(mut it) = m.row_data(i) {
                if it.id.as_str() == id {
                    it.artwork = image.clone();
                    m.set_row_data(i, it);
                    break;
                }
            }
        }
    };
    set_in(&s.get_albums());
    set_in(&s.get_albums_visible());
    let grouped = s.get_albums_grouped();
    for gi in 0..grouped.row_count() {
        if let Some(sec) = grouped.row_data(gi) {
            set_in(&sec.albums);
        }
    }
}

/// Same, for the Folders tab: full `folders` set + `folders-visible` + each
/// `folders-grouped` section. Without this the cover only lands on the source
/// `folders` model and the rendered (visible/grouped) views miss it on first
/// load (the same bug the Albums/Artists tabs had).
pub fn set_local_folder_artwork(window: &AppWindow, id: &str, image: slint::Image) {
    let s = window.global::<LocalLibraryState>();
    let set_in = |m: &ModelRc<AlbumCardItem>| {
        for i in 0..m.row_count() {
            if let Some(mut it) = m.row_data(i) {
                if it.id.as_str() == id {
                    it.artwork = image.clone();
                    m.set_row_data(i, it);
                    break;
                }
            }
        }
    };
    set_in(&s.get_folders());
    set_in(&s.get_folders_visible());
    let grouped = s.get_folders_grouped();
    for gi in 0..grouped.row_count() {
        if let Some(sec) = grouped.row_data(gi) {
            set_in(&sec.albums);
        }
    }
}

/// Re-derive the rendered Albums sets (search + quality/format/source filter +
/// sort + group + A-Z) from the full `albums` card set, filtered by id against
/// the raw LocalAlbum cache. Mirrors `derive_folders` plus the filter.
pub fn derive_albums(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    let query_owned = s.get_albums_search().to_lowercase();
    let query = query_owned.trim();
    let sort = s.get_albums_sort().to_string();
    let group = s.get_albums_group().to_string();
    let filter = read_album_filter(window);
    window
        .global::<crate::LibAlbumFilterState>()
        .set_count(album_filter_count(&filter));

    let matching: std::collections::HashSet<String> = {
        let cache = local_albums();
        cache
            .iter()
            .filter(|a| {
                (query.is_empty()
                    || a.title.to_lowercase().contains(query)
                    || a.artist.to_lowercase().contains(query)
                    || a.all_artists.to_lowercase().contains(query))
                    && album_matches_filters(a, &filter)
            })
            .map(|a| a.id.clone())
            .collect()
    };

    let all = s.get_albums();
    let mut filtered: Vec<AlbumCardItem> = (0..all.row_count())
        .filter_map(|i| all.row_data(i))
        .filter(|c| matching.contains(&c.id.to_string()))
        .collect();
    crate::album_map::sort_album_items(&mut filtered, &sort);
    s.set_albums_shown(filtered.len() as i32);

    let empty_sections = || ModelRc::new(VecModel::from(Vec::<DiscoverSection>::new()));
    if group == "off" {
        s.set_albums_visible(ModelRc::new(VecModel::from(filtered)));
        s.set_albums_grouped(empty_sections());
        s.set_albums_alpha(ModelRc::new(VecModel::from(Vec::<AlphaJump>::new())));
        return;
    }

    let mut map: Vec<(String, Vec<AlbumCardItem>)> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for item in filtered {
        let key = if group == "artist" {
            let a = item.artist.to_string();
            if a.is_empty() {
                qbz_i18n::t("Unknown")
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
    let jumps: Vec<AlphaJump> = map
        .iter()
        .enumerate()
        .map(|(i, (k, _))| AlphaJump {
            letter: k.clone().into(),
            index: i as i32,
        })
        .collect();
    let sections: Vec<DiscoverSection> = map
        .into_iter()
        .map(|(key, items)| DiscoverSection {
            title: key.into(),
            endpoint: "".into(),
            albums: ModelRc::new(VecModel::from(items)),
        })
        .collect();
    s.set_albums_grouped(ModelRc::new(VecModel::from(sections)));
    s.set_albums_alpha(ModelRc::new(VecModel::from(jumps)));
    s.set_albums_visible(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
}

/// Clear all quality/format/source filters, then re-derive.
pub fn clear_album_filter(window: &AppWindow) {
    let f = window.global::<crate::LibAlbumFilterState>();
    f.set_hires(false);
    f.set_cd(false);
    f.set_lossy(false);
    f.set_flac(false);
    f.set_alac(false);
    f.set_ape(false);
    f.set_wav(false);
    f.set_mp3(false);
    f.set_aac(false);
    f.set_other(false);
    f.set_local(false);
    f.set_offline(false);
    f.set_plex(false);
    derive_albums(window);
}

/// Resolve the Plex cache DB path (`<data_dir>/qbz/plex_cache.db`), gated on
/// the user's Plex master toggle being ON. Returns `None` when Plex is
/// disabled OR the data dir is unavailable, so the Albums union degrades to
/// local-only (identical behaviour to before Plex existed). Mirrors the Tauri
/// command's resolution (`commands_v2/library.rs`).
fn plex_cache_db_path() -> Option<std::path::PathBuf> {
    if !crate::plex_settings::get().enabled {
        return None;
    }
    dirs::data_dir().map(|d| d.join("qbz").join("plex_cache.db"))
}

// NETWORK-FOLDER VISIBILITY (owner verdict 2026-06-10, refined same day):
// hiding network-folder content is keyed on RAW CONNECTIVITY, never on the
// offline MODE. A logged-out session or induced offline with the link up
// says nothing about LAN mounts — content stays visible there (offline mode
// exists precisely to use the local library). Only a CONFIRMED-down link
// (hard offline: no default route / probes dead) hides network folders, the
// one state where LAN mounts are gone too. An unmounted-while-online path is
// handled at PLAYBACK time instead (existence guard + friendly toast in
// playback.rs), not by hiding library content.
//
// Known approximation, accepted with the model: an ISP outage with a live
// LAN reads as hard offline and hides NAS content; per-mount accessibility
// checks in every browse query would be the exact-but-costly alternative.

/// True only under HARD offline (connectivity confirmed down). See the
/// NETWORK-FOLDER VISIBILITY note above.
pub(crate) fn exclude_network_folders_now() -> bool {
    crate::offline_mode::engine().status().connectivity
        == qbz_app::offline_mode::Connectivity::Down
}

/// Reset the four browse-tab models so each tab re-fetches on its next visit
/// (the `ensure_*_loaded` guards key on an empty model). Used after a scan,
/// after the danger-zone clear, and on offline-mode flips (where the
/// connectivity-keyed network-folder gate may change the browse SET — see
/// the NETWORK-FOLDER VISIBILITY note above).
pub fn reset_browse_models(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    let empty_albums = ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new()));
    let empty_tracks = ModelRc::new(VecModel::from(Vec::<TrackItem>::new()));
    s.set_albums(empty_albums.clone());
    s.set_folders(empty_albums);
    s.set_tracks(empty_tracks);
    s.set_artists(ModelRc::new(VecModel::from(Vec::<LocalArtistItem>::new())));
}

/// Upper bound for the full-load page. The Albums tab loads the entire set in
/// one shot (search/sort/filter/group are all derived client-side over the
/// cached set in `derive_albums`), so we request a single large page rather
/// than truly paginating. `total` from the page is informational here.
const ALBUMS_FULL_LOAD_LIMIT: u64 = 1_000_000;

/// Full-load the metadata-grouped albums off the UI thread (mapping + cover
/// fallback resolution all happen on the blocking thread), store the raw cache
/// + the mapped card set, then derive + spawn covers on the UI thread.
///
/// Uses the Plex-aware paginated query so that when the Plex master toggle is
/// ON, the `plex_aggregated` union surfaces Plex albums in the grid (with
/// covers via the source-aware artwork pipeline). When Plex is OFF the path is
/// `None` → the query runs local-only, exactly as before.
fn spawn_albums_load(
    window: &AppWindow,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    gen: u64,
) {
    let weak = window.as_weak();
    let plex_path = plex_cache_db_path();
    let plex = crate::plex_settings::get();
    handle.spawn(async move {
        let loaded: Option<(Vec<qbz_library::LocalAlbum>, Vec<crate::album_map::AlbumCard>)> =
            tokio::task::spawn_blocking(move || {
                // include_qobuz_downloads: true (offline copies belong in the
                // grid — the toolbar's "Offline" source filter selects them).
                // exclude_network_folders: connectivity-keyed — see the
                // NETWORK-FOLDER VISIBILITY note.
                let exclude_network = exclude_network_folders_now();
                crate::library_db::with_db(|db| {
                    let page = db.get_albums_metadata_page(
                        0,
                        ALBUMS_FULL_LOAD_LIMIT,
                        None,
                        "artist",
                        "asc",
                        true,
                        exclude_network,
                        plex_path.as_deref(),
                    )?;
                    let albums = page.albums;
                    let cards: Vec<crate::album_map::AlbumCard> = albums
                        .iter()
                        .map(|a| {
                            let mut card = map_local_album(a.clone());
                            // Local-cover fallback scans the on-disk folder; it
                            // never applies to Plex rows (their artwork_url is a
                            // non-empty /library/... path, so this no-ops).
                            if card.artwork_url.is_empty() {
                                if let Some(cover) = db.resolve_album_cover_fallback(&card.id) {
                                    card.artwork_url = cover;
                                }
                            }
                            card
                        })
                        .collect();
                    Ok((albums, cards))
                })
            })
            .await
            .ok()
            .flatten();

        let _ = weak.upgrade_in_event_loop(move |w| {
            if ALBUMS_GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            let s = w.global::<LocalLibraryState>();
            match loaded {
                Some((albums, cards)) => {
                    *local_albums() = albums;
                    let jobs = album_artwork_jobs(&cards, gen);
                    let items: Vec<AlbumCardItem> =
                        cards.into_iter().map(crate::album_map::to_item).collect();
                    s.set_albums(ModelRc::new(VecModel::from(items.clone())));
                    s.set_album_count(items.len() as i32);
                    s.set_albums_loading(false);
                    s.set_albums_load_failed(false);
                    derive_albums(&w);
                    // Route covers through the Plex-aware spawn: Plex rows carry
                    // a /library/... path → PlexThumb; local rows → LocalFile.
                    crate::artwork::spawn_local_or_plex_loads(
                        jobs,
                        plex.base_url.clone(),
                        plex.token.clone(),
                        w.as_weak(),
                        image_cache,
                    );
                }
                None => {
                    s.set_albums_loading(false);
                    s.set_albums_load_failed(true);
                }
            }
        });
    });
}

/// (Re)load the full album set, bumping the generation guard.
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
        spawn_albums_load(&w, handle, image_cache, gen);
    });
}

/// Seed all four tab-badge counts up front (mirrors Favorites' seeded
/// counts) so the nav shows numbers without visiting each tab. Cheap:
/// bounded album/folder/artist reads + a `COUNT(*)` for the (potentially
/// huge) tracks table. Album/artist counts match each tab's own loader
/// exactly (same `get_albums_metadata_page` set incl. the Plex union; same
/// `normalize_artist` grouping the rail uses), so badges never jump when a
/// tab is opened.
pub fn seed_counts(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let plex_path = plex_cache_db_path();
    let plex_enabled = crate::plex_settings::get().enabled;
    handle.spawn(async move {
        let counts: Option<(usize, usize, usize, usize)> = tokio::task::spawn_blocking(move || {
            // Same include-qobuz / network flags as the Albums tab loader, so
            // the badge always matches the grid (network content always in —
            // see the NETWORK-FOLDER VISIBILITY note).
            let exclude_network = exclude_network_folders_now();
            crate::library_db::with_db(|db| {
                // Total under the same filter the Albums tab uses, incl. the
                // Plex union when enabled, so the badge matches the grid.
                let albums = db
                    .get_albums_metadata_page(
                        0,
                        ALBUMS_FULL_LOAD_LIMIT,
                        None,
                        "artist",
                        "asc",
                        true,
                        exclude_network,
                        plex_path.as_deref(),
                    )
                    .map(|p| p.total as usize)
                    .unwrap_or(0);
                let folders = db
                    .get_folders_with_metadata()
                    .map(|v| v.len())
                    .unwrap_or(0);
                let mut tracks = db.count_all_local_tracks().unwrap_or(0) as usize;
                // With Plex ON, include the Plex track count so the Tracks badge
                // matches the now-Plex-inclusive Tracks tab (and the albums/artists
                // badges, which already fold Plex in).
                if plex_enabled {
                    tracks += qbz_plex::plex_cache_count_tracks().unwrap_or(0);
                }
                // Exact rail count = distinct non-empty normalized names
                // (mirrors merge_artists' grouping key). With Plex ON, fold the
                // aggregated Plex artist names into the same set so the badge
                // matches the now-Plex-inclusive rail (a local + Plex artist of
                // the same normalized name counts once).
                let artists_raw = db.get_artists().unwrap_or_default();
                let mut seen = std::collections::HashSet::new();
                for a in &artists_raw {
                    let n = normalize_artist(&a.name);
                    if !n.is_empty() {
                        seen.insert(n);
                    }
                }
                if plex_enabled {
                    if let Ok(plex_artists) = qbz_plex::plex_cache_get_artists() {
                        for pa in &plex_artists {
                            let n = normalize_artist(&pa.name);
                            if !n.is_empty() {
                                seen.insert(n);
                            }
                        }
                    }
                }
                Ok((albums, seen.len(), folders, tracks))
            })
        })
        .await
        .ok()
        .flatten();

        if let Some((albums, artists, folders, tracks)) = counts {
            let _ = weak.upgrade_in_event_loop(move |w| {
                let s = w.global::<LocalLibraryState>();
                s.set_album_count(albums as i32);
                s.set_artist_count(artists as i32);
                s.set_folder_count(folders as i32);
                s.set_track_count(tracks as i32);
            });
        }
    });
}

/// Load on first visit only (re-entry keeps the set + derived views).
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
            spawn_albums_load(&w, handle, image_cache, gen);
        }
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

/// Snapshot of the currently-loaded Tracks-tab rows (already carry their
/// covers). Used to build the play queue instantly on a row click — avoiding
/// the full DB re-query + cover-fill that delayed queue population.
pub fn tracks_current_snapshot() -> Vec<qbz_library::LocalTrack> {
    tracks_current().clone()
}

fn fmt_duration(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Map one local track row to the rendered `TrackItem` (UI thread — holds a
/// non-Send `slint::Image`). Local tracks aren't Qobuz-linkable, so the
/// artist/album link ids are empty (the row renders them as plain text).
fn map_local_track(t: qbz_library::LocalTrack) -> TrackItem {
    // One shared classifier (crate::quality::badge) — same source the album
    // card + header use, so all surfaces agree; an un-hydrated lossless track
    // shows a generic "FLAC" detail. `t.sample_rate` is Hz; badge normalizes.
    let (tier, quality_detail, _) =
        crate::quality::badge(&t.format.to_string(), t.bit_depth, Some(t.sample_rate));
    TrackItem {
        // Local Library rows are local assets — never blacklisted (protected).
        is_blacklisted: false,
        id: t.id.to_string().into(),
        number: t.track_number.map(|n| n.to_string()).unwrap_or_default().into(),
        title: t.title.into(),
        artist: t.artist.into(),
        album: t.album.into(),
        duration: fmt_duration(t.duration_secs).into(),
        quality_tier: tier.into(),
        quality_detail: quality_detail.into(),
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
        // Source indicator: offline copies read as Qobuz, user files as local,
        // ephemeral tracks tagged so the UI can gate persistence actions.
        source: match t.source.as_deref() {
            Some("qobuz_download") => "qobuz",
            Some("plex") => "plex",
            Some("ephemeral") => "ephemeral",
            _ => "local",
        }
        .into(),
        unlocking: false,
        // Default: no disc header. The flat Library Tracks tab never groups by
        // disc; the local-album DETAIL view stamps this afterwards (see
        // apply_album_version) for multi-disc local albums.
        disc_header_number: 0,
    }
}

/// Fetch one tracks page off the UI thread. `LocalTrack` is Send, so it
/// crosses the `spawn_blocking` boundary; the conversion to `TrackItem`
/// happens on the UI thread. has_more = the LOCAL page came back full.
///
/// Plex (2026-06-07): `search_with_filter_page` is `local_tracks`-only (no
/// ATTACH/union), and there is no paginated Plex track query — the Plex cache
/// is a bounded set (≤5000), not 16K-row scale. So when `plex` is ON we merge
/// the FULL Plex search set ONCE on page 1 (`offset == 0`) and keep all later
/// pages pure-local. This preserves the local `LIMIT/OFFSET` perf path exactly
/// (offsets stay aligned to `local_tracks`) and mirrors how the Albums tab
/// full-loads its Plex union. `has_more` is driven by the LOCAL page only, so
/// the merged Plex rows never make pagination over-report. When `plex` is OFF
/// the path is byte-for-byte the pre-Plex behaviour.
fn fetch_tracks_page(
    query: String,
    offset: u64,
    plex: bool,
) -> Option<(Vec<qbz_library::LocalTrack>, bool)> {
    // exclude_network_folders: connectivity-keyed — see the NETWORK-FOLDER
    // VISIBILITY note.
    let exclude_network = exclude_network_folders_now();
    let mut rows = crate::library_db::with_db(|db| {
        db.search_with_filter_page(query.trim(), offset, TRACKS_PAGE, true, exclude_network)
    })?;
    let has_more = rows.len() as u64 == TRACKS_PAGE;
    if plex && offset == 0 {
        // Full Plex set (default cap 5000), mapped to the LocalTrack shape so it
        // flows through the existing map_local_track -> TrackItem pipeline and
        // the source-aware playback path (file_path = rating_key, source=plex).
        if let Ok(plex_rows) = qbz_plex::plex_cache_search_tracks(query.trim().to_string(), None) {
            let mapped = plex_rows.into_iter().map(map_plex_cached_to_local_track);
            // Prepend Plex rows so they are visible without scrolling past a full
            // local page; client-side sort/group in derive_tracks reorders when a
            // group mode is active.
            let mut merged: Vec<qbz_library::LocalTrack> = mapped.collect();
            merged.append(&mut rows);
            rows = merged;
        }
    }
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
    crate::selection::clear_anchor();
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

/// Toggle one row's selection (by id) in the visible model. Plain/Ctrl+Click =
/// single toggle; Shift+Click = additive range from the anchor (1:1 with the
/// central track arm — LocalLibrary routes its own toggle, not that arm).
pub fn toggle_track_select(window: &AppWindow, id: &str) {
    let model = window.global::<LocalLibraryState>().get_tracks_visible();
    if let Some(vm) = model.as_any().downcast_ref::<slint::VecModel<TrackItem>>() {
        let clicked = (0..vm.row_count())
            .find(|&i| vm.row_data(i).map(|t| t.id.as_str() == id).unwrap_or(false));
        if let Some(clicked) = clicked {
            let shift = crate::keybindings::mods().2;
            let anchor = if shift {
                crate::selection::resolve_anchor(
                    crate::selection::SURFACE_LOCAL_TRACKS,
                    vm,
                    |t| t.id.to_string(),
                )
            } else {
                None
            };
            match anchor {
                Some(anchor) => crate::selection::apply_shift_range(
                    vm,
                    anchor,
                    clicked,
                    |t, v| t.selected = v,
                ),
                None => {
                    if let Some(mut item) = vm.row_data(clicked) {
                        item.selected = !item.selected;
                        vm.set_row_data(clicked, item);
                    }
                }
            }
            crate::selection::set_anchor(crate::selection::SURFACE_LOCAL_TRACKS, clicked, id);
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
    // Snapshot the Plex gate on the UI thread (the setting read is cheap but we
    // never want to read it off the event loop). Page 1 only merges Plex.
    let plex = crate::plex_settings::get().enabled;
    let weak = window.as_weak();
    handle.spawn(async move {
        let result = tokio::task::spawn_blocking(move || fetch_tracks_page(query, 0, plex))
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
            // Plex rows are merged once on page 1; later pages stay pure-local so
            // the local LIMIT/OFFSET stays aligned (no duplicate Plex rows).
            let result = tokio::task::spawn_blocking(move || fetch_tracks_page(query, offset, false))
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
        let h = (mins / 60).to_string();
        let m = (mins % 60).to_string();
        qbz_i18n::t_args("{} h {} min", &[&h, &m])
    } else {
        qbz_i18n::t_args("{} min", &[&mins.to_string()])
    }
}

// The open local album's versions (label, tracks) — a "version" is a distinct
// physical copy (= a distinct source directory). Cached so the version picker
// switches without a DB round-trip. Splitting by directory is what stops two
// copies of the same album from merging into a duplicated track list.
static ALBUM_VERSIONS: LazyLock<Mutex<Vec<(String, Vec<qbz_library::LocalTrack>)>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

fn album_versions() -> std::sync::MutexGuard<'static, Vec<(String, Vec<qbz_library::LocalTrack>)>> {
    ALBUM_VERSIONS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Client-side track filter for the open local album (mirrors the Qobuz album
/// view's track search). Applied over the current version's tracks at render.
static ALBUM_QUERY: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));

fn album_query() -> String {
    ALBUM_QUERY.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Set the album track filter and re-render the current version in place.
pub fn search_album(weak: slint::Weak<AppWindow>, query: String) {
    *ALBUM_QUERY.lock().unwrap_or_else(|e| e.into_inner()) = query;
    let _ = weak.upgrade_in_event_loop(|w| {
        let index = w.global::<crate::LocalAlbumState>().get_version_index();
        apply_album_version(&w, index);
    });
}

/// Quality rank for ordering versions (hi-res first).
fn version_rank(t: &qbz_library::LocalTrack) -> (u32, u64) {
    (t.bit_depth.unwrap_or(0), t.sample_rate as u64)
}

/// A version's picker label: "24-bit / 96 kHz · FLAC" (quality + format).
fn version_label(tracks: &[qbz_library::LocalTrack]) -> String {
    match tracks.first() {
        Some(t) => {
            let (detail, _) = local_quality(t.bit_depth, t.sample_rate);
            let fmt = t.format.to_string();
            if detail.is_empty() {
                fmt
            } else {
                format!("{detail} · {fmt}")
            }
        }
        None => String::new(),
    }
}

/// A version's source ("user" | "qobuz_download" | "plex" | "") — drives the
/// picker's source icon.
fn version_source(tracks: &[qbz_library::LocalTrack]) -> String {
    tracks
        .first()
        .and_then(|t| t.source.clone())
        .unwrap_or_default()
}

/// Load a local album (dedicated LocalAlbumView), splitting its tracks into
/// VERSIONS by source directory so multiple copies don't merge into a
/// duplicate-track list. Applies the best-quality version first; the picker
/// switches versions in place. Does NOT touch nav — the caller sets the view.
pub fn open_local_album(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    group_key: String,
) {
    // Fresh album → clear any leftover track filter from the previous one.
    *ALBUM_QUERY.lock().unwrap_or_else(|e| e.into_inner()) = String::new();
    let _ = weak.upgrade_in_event_loop(|w| {
        let s = w.global::<crate::LocalAlbumState>();
        s.set_loading(true);
        s.set_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
        s.set_versions(ModelRc::new(VecModel::from(Vec::<crate::LocalAlbumVersion>::new())));
        s.set_version_index(0);
        s.set_cover(slint::Image::default());
    });
    let gk = group_key.clone();
    let hydrate_handle = handle.clone();
    handle.spawn(async move {
        let tracks = tokio::task::spawn_blocking(move || {
            let mut t = fetch_album_tracks_blocking(&gk);
            // Backfill covers from cover.jpg/folder.jpg on disk (the DB may not
            // have an artwork_path even when a cover sits in the folder).
            crate::playback::fill_missing_covers(&mut t);
            t
        })
        .await
        .unwrap_or_default();
        // Group by source directory (LocalTrack.album_group_key = the dir key).
        let mut groups: std::collections::HashMap<String, Vec<qbz_library::LocalTrack>> =
            std::collections::HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for t in tracks {
            let key = t.album_group_key.clone();
            if !groups.contains_key(&key) {
                order.push(key.clone());
            }
            groups.entry(key).or_default().push(t);
        }
        let mut versions: Vec<(String, Vec<qbz_library::LocalTrack>)> = order
            .into_iter()
            .filter_map(|k| {
                groups.remove(&k).map(|mut v| {
                    v.sort_by_key(|t| (t.disc_number.unwrap_or(1), t.track_number.unwrap_or(0)));
                    (k, v)
                })
            })
            .collect();
        // Best quality first (so the default selection is the highest-res copy).
        versions.sort_by(|a, b| {
            let qa = a.1.iter().map(version_rank).max().unwrap_or((0, 0));
            let qb = b.1.iter().map(version_rank).max().unwrap_or((0, 0));
            qb.cmp(&qa)
        });
        // (label, source) per version — best-quality first (already sorted).
        let infos: Vec<(String, String)> = versions
            .iter()
            .map(|(_, v)| (version_label(v), version_source(v)))
            .collect();
        // Album cover = the FIRST version (best quality first) that has a cover
        // on disk; fall through versions; else empty (placeholder). Album-level
        // + stable across version switches.
        let album_cover = versions
            .iter()
            .find_map(|(_, v)| {
                v.iter()
                    .find_map(|t| t.artwork_path.clone().filter(|p| !p.is_empty()))
            })
            .unwrap_or_default();
        *album_versions() = versions;
        let _ = weak.upgrade_in_event_loop(move |w| {
            let s = w.global::<crate::LocalAlbumState>();
            s.set_id(group_key.into());
            let vlist: Vec<crate::LocalAlbumVersion> = infos
                .into_iter()
                .map(|(label, source)| crate::LocalAlbumVersion {
                    label: label.into(),
                    source: source.into(),
                })
                .collect();
            s.set_versions(ModelRc::new(VecModel::from(vlist)));
            s.set_loading(false);
            s.set_cover_url(album_cover.clone().into());
            apply_album_version(&w, 0);
            // Plex quality hydration (slice 6): if this is a Plex album, the
            // cached rows may carry NULL/incomplete quality (the bulk `/all`
            // list omits bitDepth/samplingRate). The cached badge is already
            // painted above (KEEP it — a partial badge beats nothing); now
            // hydrate the real per-track quality in the background and fan the
            // result out to every badge surface so they agree.
            let gk_for_hydrate = w.global::<crate::LocalAlbumState>().get_id().to_string();
            if gk_for_hydrate.starts_with("plex:") {
                spawn_plex_quality_hydration(w.as_weak(), hydrate_handle.clone(), gk_for_hydrate);
            }
            // Decode the album cover once (stable across version switches).
            // Plex-aware: a Plex album's cover is a raw `/library/...` thumb
            // path that needs the token (PlexThumb), not a local file read.
            if !album_cover.is_empty() {
                let plex = crate::plex_settings::get();
                crate::artwork::spawn_local_or_plex_loads(
                    vec![ArtworkJob {
                        target: ArtworkTarget::LocalAlbumViewCover,
                        url: album_cover,
                    }],
                    plex.base_url,
                    plex.token,
                    w.as_weak(),
                    image_cache,
                );
            }
        });
    });
}

/// Apply version `index` of the open album to LocalAlbumState (tracks, header,
/// quality). Reads the cached versions; no DB round-trip. The cover is
/// album-level (set once by `open_local_album`), so it is NOT touched here.
pub fn apply_album_version(window: &AppWindow, index: i32) {
    let versions = album_versions();
    let Some((_, tracks)) = versions.get(index as usize) else {
        return;
    };
    let s = window.global::<crate::LocalAlbumState>();
    let group_key = s.get_id().to_string();
    let title = tracks
        .first()
        .map(|t| t.album_group_title.clone())
        .unwrap_or_default();
    let artist_of =
        |t: &qbz_library::LocalTrack| t.album_artist.clone().unwrap_or_else(|| t.artist.clone());
    let artist = match tracks.first() {
        Some(first) => {
            let name = artist_of(first);
            if tracks.iter().all(|t| artist_of(t) == name) {
                name
            } else {
                qbz_i18n::t("Various Artists")
            }
        }
        None => String::new(),
    };
    let total_secs: u64 = tracks.iter().map(|t| t.duration_secs).sum();
    let track_count = qbz_i18n::tf("{} track", "{} tracks", tracks.len() as i64, &[&tracks.len().to_string()]);
    let info_line = format!("{} · {}", track_count, fmt_album_duration(total_secs));
    let (tier, detail) = match tracks.iter().max_by_key(|t| t.bit_depth.unwrap_or(0)) {
        Some(t) => {
            // Same shared classifier as the card + rows (badge), so the header
            // matches them; un-hydrated lossless → generic "FLAC".
            let (tier, detail, _) =
                crate::quality::badge(&t.format.to_string(), t.bit_depth, Some(t.sample_rate));
            (tier.to_string(), detail)
        }
        None => (String::new(), String::new()),
    };
    // Client-side filter (Qobuz album view parity): match title/artist; the
    // header badge/info stay album-level (computed from the full version above).
    let q = album_query().to_lowercase();
    let shown: Vec<&qbz_library::LocalTrack> = tracks
        .iter()
        .filter(|t| {
            q.is_empty()
                || t.title.to_lowercase().contains(&q)
                || t.artist.to_lowercase().contains(&q)
        })
        .collect();
    // Multi-disc grouping (mirrors the Qobuz album view): the album is
    // multi-disc when its shown tracks span more than one distinct disc
    // number, and only then do we stamp "Disc N" headers on the first row of
    // each disc run. Local tracks are already disc-then-track sorted upstream
    // (album_versions sorts by (disc_number, track_number)).
    let is_multi_disc = {
        let mut seen: Option<u32> = None;
        let mut multi = false;
        for t in &shown {
            let disc = t.disc_number.unwrap_or(1);
            match seen {
                Some(d) if d != disc => {
                    multi = true;
                    break;
                }
                _ => seen = Some(disc),
            }
        }
        multi
    };
    let mut prev_disc: Option<u32> = None;
    let items: Vec<TrackItem> = shown
        .into_iter()
        .map(|t| {
            let disc = t.disc_number.unwrap_or(1);
            let disc_header_number = if is_multi_disc && prev_disc != Some(disc) {
                disc as i32
            } else {
                0
            };
            prev_disc = Some(disc);
            let mut it = map_local_track(t.clone());
            it.album_id = group_key.clone().into();
            it.disc_header_number = disc_header_number;
            it
        })
        .collect();
    s.set_title(title.into());
    s.set_artist(artist.into());
    s.set_info_line(info_line.into());
    s.set_quality_tier(tier.into());
    s.set_quality_detail(detail.into());
    s.set_tracks(ModelRc::new(VecModel::from(items)));
    s.set_version_index(index);
}

// ======================= Plex quality hydration (slice 6) =================
// Album-open trigger: a Plex album's cached rows often carry NULL/incomplete
// quality (the bulk `/library/sections/.../all` list omits bitDepth and
// samplingRate). We fetch the real per-track quality on open, AWAIT the SQLite
// write-back (qbz_plex persists via COALESCE so a NULL incoming value never
// erases an existing one), then fan the result out to every badge surface so
// they agree:
//   (1) album-CARD grid badge  — re-read the album aggregate, patch the 3 album
//                                 models + the raw LOCAL_ALBUMS filter source.
//   (2) album-DETAIL header + per-row + audio-specs — rebuild album_versions()
//                                 from the hydrated cache, re-run apply_album_version.
//   (3) flat Tracks-tab rows    — patch matching rows in `tracks`/`tracks-visible`
//                                 + the tracks_current() selection cache by id.
//   (4) now-playing STAMP       — if a hydrated track is the current queue track,
//                                 patch its frozen snapshot and re-push.
//
// "Needs hydration" is the DB-NULL definition (bit_depth IS NULL OR sample_rate
// is 0) — NOT a value heuristic; a genuine 16/44.1 FLAC is written once and
// never re-probed (fixes the Tauri isLikelyFallbackPlexQuality bug).

/// Spawn the album-open Plex quality hydration. Reads the just-loaded version
/// tracks to find which Plex rating_keys still need hydration, hydrates them
/// (awaiting the persist), then refreshes all four badge surfaces.
fn spawn_plex_quality_hydration(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    group_key: String,
) {
    // Collect rating_keys whose cached quality is still incomplete. The Plex
    // LocalTrack carries the rating_key in `file_path` and the cached quality in
    // `bit_depth` / `sample_rate` (0.0 == unknown).
    let needing: Vec<String> = {
        let versions = album_versions();
        let mut keys: Vec<String> = versions
            .iter()
            .flat_map(|(_, tracks)| tracks.iter())
            .filter(|t| t.source.as_deref() == Some("plex"))
            .filter(|t| t.bit_depth.is_none() || t.sample_rate <= 0.0)
            .map(|t| t.file_path.clone())
            .collect();
        keys.sort();
        keys.dedup();
        keys
    };
    if needing.is_empty() {
        return;
    }

    let plex = crate::plex_settings::get();
    if plex.base_url.is_empty() || plex.token.is_empty() {
        return;
    }

    handle.spawn(async move {
        let updates = match qbz_plex::plex_hydrate_album_quality(
            plex.base_url.clone(),
            plex.token.clone(),
            needing,
        )
        .await
        {
            Ok(u) if !u.is_empty() => u,
            _ => return, // nothing fetched/persisted → leave the cached badge as-is
        };

        // Surface 4 (now-playing): if any hydrated track is the current queue
        // track, patch its frozen snapshot and re-push the stamp. Fire-and-forget
        // through the global queue controller.
        let queue_updates: Vec<(String, Option<u32>, Option<f64>)> = updates
            .iter()
            .map(|u| {
                let khz = u.sampling_rate_hz.map(|hz| hz as f64 / 1000.0);
                (u.rating_key.clone(), u.bit_depth, khz)
            })
            .collect();
        crate::playback::apply_plex_quality_to_queue(queue_updates);

        // Re-read the now-hydrated album tracks + the album aggregate off the UI
        // thread (both are blocking SQLite reads).
        let gk = group_key.clone();
        let plex_path = plex_cache_db_path();
        let reread = tokio::task::spawn_blocking(move || {
            let tracks = qbz_plex::plex_cache_get_album_tracks(gk.clone())
                .unwrap_or_default()
                .into_iter()
                .map(map_plex_cached_to_local_track)
                .collect::<Vec<_>>();
            // The album aggregate (MAX over tracks) is a pure runtime query; the
            // refreshed Plex track quality flows into it automatically.
            let album = plex_path.as_deref().and_then(|p| {
                // Same flags as the Albums tab loader so the looked-up
                // aggregate comes from the identical set (network content
                // always in — see the NETWORK-FOLDER VISIBILITY note).
                let exclude_network = exclude_network_folders_now();
                crate::library_db::with_db(|db| {
                    let page = db.get_albums_metadata_page(
                        0,
                        ALBUMS_FULL_LOAD_LIMIT,
                        None,
                        "artist",
                        "asc",
                        true,
                        exclude_network,
                        Some(p),
                    )?;
                    Ok(page.albums.into_iter().find(|a| a.id == gk))
                })
                .flatten()
            });
            (tracks, album)
        })
        .await
        .unwrap_or_else(|_| (Vec::new(), None));

        let (hydrated_tracks, hydrated_album) = reread;
        if hydrated_tracks.is_empty() {
            return;
        }

        let updated_rows: Vec<(String, Option<u32>, u32)> = updates
            .iter()
            .map(|u| (u.rating_key.clone(), u.bit_depth, u.sampling_rate_hz.unwrap_or(0)))
            .collect();

        let _ = weak.upgrade_in_event_loop(move |w| {
            refresh_open_album_after_hydration(&w, &group_key, hydrated_tracks);
            if let Some(album) = hydrated_album {
                refresh_album_card_quality(&w, &album);
            }
            refresh_track_rows_quality(&w, &updated_rows);
        });
    });
}

/// Surface 2 — rebuild the open album's version tracks from the hydrated cache
/// rows and re-run `apply_album_version` so the header MAX badge, the audio
/// specs, and the per-row badges all recompute from the real quality. No-op if
/// the open album changed while we were hydrating.
fn refresh_open_album_after_hydration(
    window: &AppWindow,
    group_key: &str,
    hydrated_tracks: Vec<qbz_library::LocalTrack>,
) {
    let s = window.global::<crate::LocalAlbumState>();
    if s.get_id().to_string() != group_key {
        return; // user navigated away; the album-detail surface is no longer this album
    }
    // Re-split into versions by source dir, mirroring open_local_album.
    let mut groups: std::collections::HashMap<String, Vec<qbz_library::LocalTrack>> =
        std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for t in hydrated_tracks {
        let key = t.album_group_key.clone();
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(t);
    }
    let mut versions: Vec<(String, Vec<qbz_library::LocalTrack>)> = order
        .into_iter()
        .filter_map(|k| {
            groups.remove(&k).map(|mut v| {
                v.sort_by_key(|t| (t.disc_number.unwrap_or(1), t.track_number.unwrap_or(0)));
                (k, v)
            })
        })
        .collect();
    versions.sort_by(|a, b| {
        let qa = a.1.iter().map(version_rank).max().unwrap_or((0, 0));
        let qb = b.1.iter().map(version_rank).max().unwrap_or((0, 0));
        qb.cmp(&qa)
    });
    let index = s.get_version_index().max(0);
    *album_versions() = versions;
    apply_album_version(window, index);
}

/// Surface 1 — patch the album-CARD grid badge for one album across all three
/// rendered album models (`albums`, `albums-visible`, each `albums-grouped`
/// section) AND the raw `LOCAL_ALBUMS` filter source, recomputing tier/label/
/// detail from the freshly aggregated quality. Mirrors `set_local_album_artwork`
/// (refresh-one-row across every collection — the Tauri bug was patching only
/// one collection, leaving the grid badge stale).
fn refresh_album_card_quality(window: &AppWindow, album: &qbz_library::LocalAlbum) {
    let card = map_local_album(album.clone());
    let tier: slint::SharedString = card.quality_tier.clone().into();
    let label: slint::SharedString = card.quality_label.clone().into();
    let detail: slint::SharedString = card.quality_detail.clone().into();
    let id = album.id.clone();

    let s = window.global::<LocalLibraryState>();
    let patch_in = |m: &ModelRc<AlbumCardItem>| {
        for i in 0..m.row_count() {
            if let Some(mut it) = m.row_data(i) {
                if it.id.as_str() == id {
                    it.quality_tier = tier.clone();
                    it.quality_label = label.clone();
                    it.quality_detail = detail.clone();
                    m.set_row_data(i, it);
                    break;
                }
            }
        }
    };
    patch_in(&s.get_albums());
    patch_in(&s.get_albums_visible());
    let grouped = s.get_albums_grouped();
    for gi in 0..grouped.row_count() {
        if let Some(sec) = grouped.row_data(gi) {
            patch_in(&sec.albums);
        }
    }
    // Keep the raw filter source consistent so a re-derive (search/filter/sort)
    // doesn't snap the badge back to the stale MAX.
    {
        let mut cache = local_albums();
        if let Some(a) = cache.iter_mut().find(|a| a.id == id) {
            a.bit_depth = album.bit_depth;
            a.sample_rate = album.sample_rate;
            a.format = album.format.clone();
        }
    }
}

/// Surface 3 — patch the flat Tracks-tab row badges (and the `tracks_current()`
/// selection cache) for the hydrated Plex tracks, matched by their namespaced
/// row id. Patches in place (no model rebuild) to avoid re-grouping the 16K-row
/// set — the same reason Tauri used an override map instead of a `tracks`
/// reassignment.
fn refresh_track_rows_quality(window: &AppWindow, updates: &[(String, Option<u32>, u32)]) {
    if updates.is_empty() {
        return;
    }
    // Map rating_key -> namespaced TrackItem id string (matches map_plex_cached_to_local_track).
    let by_id: std::collections::HashMap<String, (Option<u32>, u32)> = {
        let cache = tracks_current();
        let mut m = std::collections::HashMap::new();
        for t in cache.iter() {
            if t.source.as_deref() != Some("plex") {
                continue;
            }
            if let Some((_, bd, sr)) = updates.iter().find(|(rk, _, _)| *rk == t.file_path) {
                m.insert(t.id.to_string(), (*bd, *sr));
            }
        }
        m
    };
    if by_id.is_empty() {
        return;
    }

    // Patch the in-memory selection cache so a later derive_tracks keeps quality.
    {
        let mut cache = tracks_current();
        for t in cache.iter_mut() {
            if t.source.as_deref() != Some("plex") {
                continue;
            }
            if let Some((_, bd, sr)) = updates.iter().find(|(rk, _, _)| *rk == t.file_path) {
                if bd.is_some() {
                    t.bit_depth = *bd;
                }
                if *sr > 0 {
                    t.sample_rate = *sr as f64;
                }
            }
        }
    }

    let recompute = |bd: Option<u32>, sr: u32| -> (slint::SharedString, slint::SharedString) {
        let tier = match bd {
            Some(b) if b >= 24 => "hires",
            Some(_) => "cd",
            None => "",
        };
        let detail = if tier.is_empty() {
            String::new()
        } else {
            crate::quality::detail(bd, Some(sr as f64))
        };
        (tier.into(), detail.into())
    };

    let s = window.global::<LocalLibraryState>();
    let patch_in = |m: &ModelRc<TrackItem>| {
        for i in 0..m.row_count() {
            if let Some(mut it) = m.row_data(i) {
                if it.source.as_str() != "plex" {
                    continue;
                }
                if let Some((bd, sr)) = by_id.get(it.id.as_str()) {
                    let (tier, detail) = recompute(*bd, *sr);
                    it.quality_tier = tier;
                    it.quality_detail = detail;
                    m.set_row_data(i, it);
                }
            }
        }
    };
    patch_in(&s.get_tracks());
    patch_in(&s.get_tracks_visible());
}

/// The source directory of version `index` (for the tag editor — a real dir).
pub fn album_version_dir(index: i32) -> Option<String> {
    album_versions().get(index as usize).map(|(dir, _)| dir.clone())
}

/// The currently-selected album version's tracks (play / shuffle / add / edit).
pub fn current_album_version_tracks(window: &AppWindow) -> Vec<qbz_library::LocalTrack> {
    let idx = window.global::<crate::LocalAlbumState>().get_version_index();
    album_versions()
        .get(idx as usize)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// The current version's tracks for one disc (for the per-disc "Disc N" header
/// menu), filtered by `disc_number` (defaulting to 1 — exactly as
/// `apply_album_version` stamps the "Disc N" header). Preserves the upstream
/// (disc, track) order.
pub fn current_album_disc_tracks(
    window: &AppWindow,
    disc: i32,
) -> Vec<qbz_library::LocalTrack> {
    current_album_version_tracks(window)
        .into_iter()
        .filter(|t| t.disc_number.unwrap_or(1) as i32 == disc)
        .collect()
}

// ============================ Folders (flat) ==============================
//
// The Folders tab (flat mode) is the album grid grouped by directory rather
// than by metadata. Full-load (folder counts are bounded; the freeze risk is
// the Tracks table). Reuses AlbumCollectionView; covers load per card.

fn fetch_folder_albums() -> Vec<crate::album_map::AlbumCard> {
    // exclude_network_folders: connectivity-keyed — an unmounted-but-online
    // folder stays visible (the index is the source); only hard offline
    // hides it (see the NETWORK-FOLDER VISIBILITY note).
    let exclude_network = exclude_network_folders_now();
    crate::library_db::with_db(move |db| {
        db.get_albums_with_full_filter(
            /* include_hidden */ false,
            /* include_qobuz_downloads */ true,
            exclude_network,
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
                qbz_i18n::t("Unknown")
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
            selected: false,
            select_state: 0,
        },
        qbz_library::FolderTreeEntry::Track { path, segment } => FolderNode {
            path: path.clone().into(),
            segment: segment.clone().into(),
            depth,
            is_folder: false,
            expanded: false,
            can_expand: false,
            track_count: 0,
            selected: false,
            select_state: 0,
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
                        selected: false,
                        select_state: 0,
                    })
                    .collect();
                let s = w.global::<LocalLibraryState>();
                apply_tree(&s, nodes);
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
                apply_tree(&s, nodes);
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
                apply_tree(&s, nodes);
            }
        });
    });
}

// ---- Tree rail: search filter + multi-select ----

use std::collections::HashMap;

// Selected tracks in the tree (path -> record), for the bulk bar.
static TREE_SELECTED: LazyLock<Mutex<HashMap<String, qbz_library::LocalTrack>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn tree_selected() -> std::sync::MutexGuard<'static, HashMap<String, qbz_library::LocalTrack>> {
    TREE_SELECTED.lock().unwrap_or_else(|e| e.into_inner())
}

/// Apply the current selection to `nodes` (track `selected`, folder tri-state),
/// commit the tree model, refresh the selected count, and derive the visible
/// (search-filtered) set. The single sink for every tree mutation.
fn apply_tree(s: &LocalLibraryState, mut nodes: Vec<FolderNode>) {
    {
        let sel = tree_selected();
        for n in nodes.iter_mut() {
            if n.is_folder {
                let prefix = format!("{}/", n.path);
                let under = sel.keys().filter(|p| p.starts_with(&prefix)).count();
                n.select_state = if under == 0 {
                    0
                } else if n.track_count > 0 && under as i32 >= n.track_count {
                    2
                } else {
                    1
                };
                n.selected = false;
            } else {
                n.selected = sel.contains_key(n.path.as_str());
                n.select_state = 0;
            }
        }
        s.set_tree_selected_count(sel.len() as i32);
    }
    s.set_folder_tree(ModelRc::new(VecModel::from(nodes)));
    derive_folder_tree_visible(s);
}

/// Derive `folder-tree-visible` from `folder-tree`, filtered by the rail search
/// (keeps matching nodes AND their ancestors so the tree stays navigable).
fn derive_folder_tree_visible(s: &LocalLibraryState) {
    let full = s.get_folder_tree();
    let nodes: Vec<FolderNode> = (0..full.row_count()).filter_map(|i| full.row_data(i)).collect();
    let q = s.get_folders_tree_search().as_str().trim().to_lowercase();
    if q.is_empty() {
        s.set_folder_tree_visible(ModelRc::new(VecModel::from(nodes)));
        return;
    }
    let matches: Vec<String> = nodes
        .iter()
        .filter(|n| n.segment.as_str().to_lowercase().contains(&q))
        .map(|n| n.path.to_string())
        .collect();
    let kept: Vec<FolderNode> = nodes
        .into_iter()
        .filter(|n| {
            let p = n.path.as_str();
            n.segment.as_str().to_lowercase().contains(&q)
                || matches.iter().any(|m| m.starts_with(&format!("{p}/")))
        })
        .collect();
    s.set_folder_tree_visible(ModelRc::new(VecModel::from(kept)));
}

/// Re-run the visible filter after a search-text change.
pub fn folders_tree_search(window: &AppWindow, query: &str) {
    let s = window.global::<LocalLibraryState>();
    s.set_folders_tree_search(query.into());
    derive_folder_tree_visible(&s);
}

/// Collapse every expanded folder — keep only the depth-0 roots.
pub fn collapse_all_tree(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    let mut nodes = collect_tree(&s);
    nodes.retain(|n| n.depth == 0);
    for n in nodes.iter_mut() {
        n.expanded = false;
    }
    apply_tree(&s, nodes);
}

/// Toggle multi-select mode; leaving it clears the selection.
pub fn toggle_tree_select_mode(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    let on = !s.get_tree_select_mode();
    s.set_tree_select_mode(on);
    if !on {
        tree_selected().clear();
        let nodes = collect_tree(&s);
        apply_tree(&s, nodes);
    }
}

/// Toggle every track under a folder (recursive). 'all' → deselect; else select.
pub fn toggle_tree_folder_select(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    path: String,
) {
    handle.spawn(async move {
        let p = path.clone();
        let tracks = tokio::task::spawn_blocking(move || {
            crate::library_db::with_db(|db| db.list_folder_tracks_recursive(&p, false))
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        if tracks.is_empty() {
            return;
        }
        let all_selected = {
            let sel = tree_selected();
            tracks.iter().all(|t| sel.contains_key(&t.file_path))
        };
        {
            let mut sel = tree_selected();
            if all_selected {
                for t in &tracks {
                    sel.remove(&t.file_path);
                }
            } else {
                for t in tracks {
                    sel.insert(t.file_path.clone(), t);
                }
            }
        }
        let _ = weak.upgrade_in_event_loop(|w| {
            let s = w.global::<LocalLibraryState>();
            let nodes = collect_tree(&s);
            apply_tree(&s, nodes);
        });
    });
}

/// Toggle a single track row by path.
pub fn toggle_tree_track_select(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    path: String,
) {
    handle.spawn(async move {
        let was_selected = tree_selected().contains_key(&path);
        if was_selected {
            tree_selected().remove(&path);
        } else {
            // Resolve the track record from its parent folder listing.
            let parent = std::path::Path::new(&path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let p = parent.clone();
            let tracks = tokio::task::spawn_blocking(move || {
                crate::library_db::with_db(|db| db.list_folder_tracks(&p, false)).unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            if let Some(t) = tracks.into_iter().find(|t| t.file_path == path) {
                tree_selected().insert(path.clone(), t);
            }
        }
        let _ = weak.upgrade_in_event_loop(|w| {
            let s = w.global::<LocalLibraryState>();
            let nodes = collect_tree(&s);
            apply_tree(&s, nodes);
        });
    });
}

/// Snapshot the currently-selected tree tracks (scan order by path).
pub fn tree_selected_snapshot() -> Vec<qbz_library::LocalTrack> {
    let sel = tree_selected();
    let mut v: Vec<qbz_library::LocalTrack> = sel.values().cloned().collect();
    v.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    v
}

/// Toggle "select all": if every track under the roots is already selected,
/// clear the selection; otherwise select them all. Two-way (the bulk button
/// flips select-all / un-select-all).
pub fn tree_select_all(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        // Roots = the registered library folders.
        let paths = tokio::task::spawn_blocking(|| {
            crate::library_db::with_db(|db| db.get_folders()).unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        let all = tokio::task::spawn_blocking(move || {
            let mut acc: Vec<qbz_library::LocalTrack> = Vec::new();
            for p in paths {
                let mut t =
                    crate::library_db::with_db(|db| db.list_folder_tracks_recursive(&p, false))
                        .unwrap_or_default();
                acc.append(&mut t);
            }
            acc
        })
        .await
        .unwrap_or_default();
        {
            let mut sel = tree_selected();
            let all_selected =
                !all.is_empty() && all.iter().all(|t| sel.contains_key(&t.file_path));
            if all_selected {
                // Un-select everything under the roots.
                for t in &all {
                    sel.remove(&t.file_path);
                }
            } else {
                for t in all {
                    sel.insert(t.file_path.clone(), t);
                }
            }
        }
        let _ = weak.upgrade_in_event_loop(|w| {
            let s = w.global::<LocalLibraryState>();
            let nodes = collect_tree(&s);
            apply_tree(&s, nodes);
        });
    });
}

/// Clear the tree selection.
pub fn tree_clear_selection(window: &AppWindow) {
    tree_selected().clear();
    let s = window.global::<LocalLibraryState>();
    let nodes = collect_tree(&s);
    apply_tree(&s, nodes);
}

/// Select a folder in the tree: load its detail pane — direct child tracks
/// plus immediate subfolders (for in-pane drill-down).
pub fn select_folder(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    path: String,
    segment: String,
) {
    let _ = weak.upgrade_in_event_loop({
        let path = path.clone();
        // Drill-in from a subfolder card passes an empty segment — derive the
        // display name from the path so the detail header is never blank.
        let segment = if segment.is_empty() {
            path_basename(&path)
        } else {
            segment.clone()
        };
        move |w| {
            let s = w.global::<LocalLibraryState>();
            s.set_folders_selected_path(path.clone().into());
            s.set_folders_selected_name(segment.clone().into());
            s.set_folder_detail_loading(true);
            // Reset the per-folder subfolder filter on navigation.
            s.set_folder_detail_search("".into());
        }
    });
    let path_for_fetch = path.clone();
    handle.spawn(async move {
        let (tracks, subfolders) = tokio::task::spawn_blocking(move || {
            let tracks = crate::library_db::with_db(|db| {
                db.list_folder_tracks(&path_for_fetch, false)
            })
            .unwrap_or_default();
            // Resolve a real on-disk cover for each subfolder whose indexed
            // artwork_path is empty (no embedded art / never backfilled) — the
            // image can sit under any of a dozen names (cover/folder/front/art/
            // <album>.jpg, …). Off-thread, so the fs scan is fine here.
            let children = crate::library_db::with_db(|db| {
                db.list_folder_children(&path_for_fetch, false)
            })
            .unwrap_or_default()
            .into_iter()
            .map(|e| match e {
                qbz_library::FolderTreeEntry::Folder {
                    path,
                    segment,
                    track_count_under,
                    artwork,
                } => {
                    let artwork = artwork.filter(|a| !a.is_empty()).or_else(|| {
                        find_folder_cover(std::path::Path::new(&path))
                    });
                    qbz_library::FolderTreeEntry::Folder {
                        path,
                        segment,
                        track_count_under,
                        artwork,
                    }
                }
                other => other,
            })
            .collect::<Vec<_>>();
            (tracks, children)
        })
        .await
        .unwrap_or_default();
        let _ = weak.upgrade_in_event_loop(move |w| {
            let track_items: Vec<TrackItem> = tracks.into_iter().map(map_local_track).collect();
            // Subfolders become cover cards (1:1 with Tauri). The cover comes from
            // FolderTreeEntry::Folder.artwork (resolved async below).
            let cards: Vec<FolderSubcardItem> = subfolders
                .iter()
                .filter_map(|e| match e {
                    qbz_library::FolderTreeEntry::Folder {
                        path,
                        segment,
                        track_count_under,
                        artwork,
                    } => Some(FolderSubcardItem {
                        path: path.clone().into(),
                        name: segment.clone().into(),
                        track_count: *track_count_under as i32,
                        artwork: slint::Image::default(),
                        artwork_url: artwork.clone().unwrap_or_default().into(),
                    }),
                    _ => None,
                })
                .collect();
            // Recursive count = sum of subfolder counts + this folder's direct tracks.
            let recursive: i32 =
                cards.iter().map(|c| c.track_count).sum::<i32>() + track_items.len() as i32;
            let s = w.global::<LocalLibraryState>();
            s.set_folder_detail_tracks(ModelRc::new(VecModel::from(track_items)));
            s.set_folder_detail_track_count(recursive);
            s.set_folder_detail_subfolders(ModelRc::new(VecModel::from(cards)));
            s.set_folder_detail_loading(false);
            derive_folder_detail(&w);

            // Spawn cover artwork jobs over the full subfolder set.
            let full = s.get_folder_detail_subfolders();
            let mut jobs: Vec<ArtworkJob> = Vec::new();
            for i in 0..full.row_count() {
                if let Some(it) = full.row_data(i) {
                    let url = it.artwork_url.to_string();
                    if !url.is_empty() {
                        jobs.push(ArtworkJob {
                            target: ArtworkTarget::LocalFolderDetailCard { index: i },
                            url,
                        });
                    }
                }
            }
            if !jobs.is_empty() {
                crate::artwork::spawn_local_loads(jobs, w.as_weak(), image_cache.clone());
            }
        });
    });
}

/// Re-derive the rendered subfolder set (`-visible`) from the full set, filtered
/// by the subfolder name search. Mirrors `derive_folders`.
fn derive_folder_detail(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    let full = s.get_folder_detail_subfolders();
    let q = s.get_folder_detail_search().as_str().trim().to_lowercase();
    let rows: Vec<FolderSubcardItem> = (0..full.row_count())
        .filter_map(|i| full.row_data(i))
        .filter(|it| q.is_empty() || it.name.as_str().to_lowercase().contains(&q))
        .collect();
    s.set_folder_detail_subfolders_visible(ModelRc::new(VecModel::from(rows)));
}

/// Set the search filter for the subfolder cards and re-derive.
pub fn folder_detail_search(window: &AppWindow, query: &str) {
    window
        .global::<LocalLibraryState>()
        .set_folder_detail_search(query.into());
    derive_folder_detail(window);
}

/// Dual-set a resolved cover onto the full + visible subfolder sets by path.
pub fn set_folder_detail_subfolder_artwork(window: &AppWindow, path: &str, image: slint::Image) {
    let s = window.global::<LocalLibraryState>();
    let set_in = |m: &ModelRc<FolderSubcardItem>| {
        for i in 0..m.row_count() {
            if let Some(mut it) = m.row_data(i) {
                if it.path.as_str() == path {
                    it.artwork = image.clone();
                    m.set_row_data(i, it);
                    break;
                }
            }
        }
    };
    set_in(&s.get_folder_detail_subfolders());
    set_in(&s.get_folder_detail_subfolders_visible());
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

/// An artist name to auto-select once the Artists tab finishes loading — set
/// when navigating to a local artist from outside the tab (LocalAlbum header
/// link, now-playing "Go to artist", a track's context menu). Consumed once.
static PENDING_ARTIST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Queue an artist to be selected as soon as the Artists tab is ready.
pub fn set_pending_artist(name: String) {
    *PENDING_ARTIST.lock().unwrap_or_else(|e| e.into_inner()) = Some(name);
}

fn take_pending_artist() -> Option<String> {
    PENDING_ARTIST.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Best-effort on-disk cover for a folder. The index often has no
/// `artwork_path` (no embedded art + no backfill), yet a cover image sits in
/// the folder under any of a dozen names. Priority: a known cover stem
/// (cover/folder/front/art/album/…), then `<foldername>.<ext>` (a file named
/// after the album), then the first image file as a last resort. Case- and
/// extension-insensitive. Returns an absolute path; must run off the UI thread.
pub fn find_folder_cover(folder: &std::path::Path) -> Option<String> {
    const STEMS: &[&str] = &[
        "cover",
        "folder",
        "front",
        "art",
        "album",
        "albumart",
        "albumartsmall",
        "thumb",
        "artwork",
        "scan",
        "booklet",
        "title",
    ];
    const EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff"];
    let is_img = |p: &std::path::Path| {
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
            .unwrap_or(false)
    };
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_img(p))
        .collect();
    if entries.is_empty() {
        return None;
    }
    entries.sort();
    let stem_lower = |p: &std::path::Path| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default()
    };
    let folder_name = folder
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let by_stem = entries
        .iter()
        .find(|p| STEMS.contains(&stem_lower(p).as_str()));
    let by_name = entries
        .iter()
        .find(|p| !folder_name.is_empty() && stem_lower(p) == folder_name);
    by_stem
        .or(by_name)
        .cloned()
        .or_else(|| entries.into_iter().next())
        .map(|p| p.to_string_lossy().into_owned())
}

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
    plex_portraits: &std::collections::HashMap<String, String>,
    album_thumb_fallback: bool,
) -> Vec<ArtistRow> {
    let album_ids = build_artist_album_ids(albums);
    let norm_imgs: std::collections::HashMap<String, String> = custom_images
        .iter()
        .map(|(k, v)| (normalize_artist(k), v.clone()))
        .collect();
    // Per-normalized-artist representative album cover (first non-empty), used
    // as a last-resort portrait fallback when the artist has neither a custom
    // image nor a Plex thumb — so Plex-only artists show a cover instead of the
    // mic placeholder. GATED behind `album_thumb_fallback` (only set with Plex
    // ON): with Plex OFF the map stays empty, so a local artist with no custom
    // portrait keeps `image_path = ""` and triggers the Qobuz fetch exactly as
    // before — no behavioural change to the pre-Plex path.
    let mut album_thumbs: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if album_thumb_fallback {
        for al in albums {
            if let Some(path) = al.artwork_path.as_deref().filter(|p| !p.is_empty()) {
                album_thumbs
                    .entry(normalize_artist(&al.artist))
                    .or_insert_with(|| path.to_string());
            }
        }
    }

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
        // Portrait fallback chain: custom/cached (incl. previously-fetched
        // Qobuz) -> representative Plex thumb -> representative album cover.
        // A `/library/...` Plex path decodes via the PlexThumb artwork arm;
        // a filesystem album cover decodes as a local file. Both are routed
        // through `spawn_local_or_plex_loads` at dispatch time.
        let image_path = norm_imgs
            .get(&n)
            .or_else(|| plex_portraits.get(&n))
            .or_else(|| album_thumbs.get(&n))
            .cloned()
            .unwrap_or_default();
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
            // Already loaded → satisfy a pending open-artist immediately.
            if s.get_artists().row_count() != 0 {
                if let Some(name) = take_pending_artist() {
                    select_local_artist(w.as_weak(), handle.clone(), image_cache.clone(), name);
                }
            }
            return;
        }
        s.set_artists_loading(true);
        s.set_artists_load_failed(false);
        let gen = ARTISTS_IMG_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        let weak2 = w.as_weak();
        let handle_inner = handle.clone();
        // Snapshot the Plex gate + cache path on the UI thread. When Plex is ON
        // we (a) union Plex albums into the right-pane/album-count cache via the
        // ATTACH query (same set the Albums tab shows), and (b) aggregate Plex
        // artists client-side and fold them into the merge. When OFF, every Plex
        // branch is skipped and the path is byte-for-byte the pre-Plex flow.
        let plex_enabled = crate::plex_settings::get().enabled;
        let plex_path = plex_cache_db_path();
        handle.spawn(async move {
            let items = tokio::task::spawn_blocking(move || {
                // Same network flag as every browse tab: connectivity-keyed
                // — see the NETWORK-FOLDER VISIBILITY note.
                let exclude_network = exclude_network_folders_now();
                let artists = crate::library_db::with_db(|db| {
                    db.get_artists_with_filter(true, exclude_network)
                })
                .unwrap_or_default();
                // Album cache for the right pane + album_count. With Plex ON,
                // use the Plex-aware ATTACH/union query so the artist-detail grid
                // and counts include Plex albums (1:1 with the Albums tab set);
                // with Plex OFF, the local-only full filter — unchanged.
                let albums = if plex_enabled {
                    crate::library_db::with_db(|db| {
                        db.get_albums_metadata_page(
                            0,
                            ALBUMS_FULL_LOAD_LIMIT,
                            None,
                            "artist",
                            "asc",
                            true,
                            exclude_network,
                            plex_path.as_deref(),
                        )
                        .map(|p| p.albums)
                    })
                    .unwrap_or_default()
                } else {
                    crate::library_db::with_db(|db| {
                        db.get_albums_with_full_filter(false, true, exclude_network)
                    })
                    .unwrap_or_default()
                };
                // Seed custom AND previously-cached Qobuz portraits (fixes the
                // Tauri headline bug: its batch load command was never wired).
                let custom = crate::library_db::with_db(|db| db.get_all_artist_image_urls())
                    .unwrap_or_default();
                // Aggregate Plex artists (no plex_cache_artists table — derived
                // from the track cache) and fold them into the local set BEFORE
                // merge_artists, which de-dupes by normalize_artist: a local and
                // a Plex "Radiohead" collapse into one row with summed tracks.
                let mut all_artists = artists;
                let mut plex_portraits: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                if plex_enabled {
                    if let Ok(plex_artists) = qbz_plex::plex_cache_get_artists() {
                        for pa in plex_artists {
                            let n = normalize_artist(&pa.name);
                            if n.is_empty() {
                                continue;
                            }
                            if let Some(path) =
                                pa.artwork_path.filter(|p| !p.is_empty())
                            {
                                plex_portraits.entry(n).or_insert(path);
                            }
                            all_artists.push(qbz_library::LocalArtist {
                                name: pa.name,
                                album_count: pa.album_count,
                                track_count: pa.track_count,
                            });
                        }
                    }
                }
                let merged =
                    merge_artists(all_artists, &albums, &custom, &plex_portraits, plex_enabled);
                if let Ok(mut cache) = ARTIST_ALBUMS.lock() {
                    *cache = albums;
                }
                merged
            })
            .await
            .unwrap_or_default();
            let _ = weak2.upgrade_in_event_loop(move |w| {
                apply_artists(&w, items);
                // Satisfy a pending open-artist now that the set + ARTIST_ALBUMS
                // cache are loaded (navigated here from a "Go to artist" link).
                if let Some(name) = take_pending_artist() {
                    select_local_artist(
                        w.as_weak(),
                        handle_inner.clone(),
                        image_cache.clone(),
                        name,
                    );
                }
                // Seed decode jobs for rows that already carry an image-path.
                // Non-http paths now split into local files vs Plex `/library/`
                // thumbs: both go through the source-aware dispatcher (which
                // tokenizes Plex thumbs and reads local covers as files), so a
                // borrowed Plex artist portrait decodes correctly.
                let s = w.global::<LocalLibraryState>();
                let plex = crate::plex_settings::get();
                let artists = s.get_artists();
                let mut local_or_plex_jobs = Vec::new();
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
                            local_or_plex_jobs.push(job);
                        }
                    }
                }
                crate::artwork::spawn_local_or_plex_loads(
                    local_or_plex_jobs,
                    plex.base_url.clone(),
                    plex.token.clone(),
                    w.as_weak(),
                    image_cache.clone(),
                );
                crate::artwork::spawn_loads(http_jobs, w.as_weak(), image_cache.clone());
                // Kick the capped Qobuz portrait fetch for missing rows. Snapshot
                // the names HERE (UI thread, sync) — fetch_missing_artist_images
                // must NOT block the event loop to read the model.
                if s.get_artists_fetch_images() {
                    let mut names = Vec::new();
                    for i in 0..artists.row_count() {
                        if let Some(a) = artists.row_data(i) {
                            if a.image_path.is_empty()
                                && normalize_artist(&a.name.to_string()) != "various artists"
                            {
                                names.push(a.name.to_string());
                            }
                        }
                    }
                    s.set_artists_images_fetching(true);
                    s.set_artists_images_fetched(0);
                    fetch_missing_artist_images(
                        runtime,
                        w.as_weak(),
                        handle_inner,
                        image_cache,
                        gen,
                        names,
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
    mut names: Vec<String>,
) {
    // `names` is snapshotted by the caller on the UI thread (NEVER block the
    // event loop here — this can be invoked from inside an event-loop closure).
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
            // Source-aware: a selected artist's Plex albums carry a `/library/`
            // artwork path (PlexThumb); local albums carry filesystem paths.
            let plex = crate::plex_settings::get();
            crate::artwork::spawn_local_or_plex_loads(
                jobs,
                plex.base_url.clone(),
                plex.token.clone(),
                w.as_weak(),
                image_cache,
            );
        });
    });
}

/// Map a format string (Plex container/codec, e.g. "flac", "alac") to the
/// `AudioFormat` enum the UI quality badge expects. Mirrors the private
/// `Database::parse_format`, but case-insensitive on the lowercase Plex value.
fn parse_audio_format(s: &str) -> qbz_library::AudioFormat {
    use qbz_library::AudioFormat;
    match s.to_ascii_lowercase().as_str() {
        "flac" => AudioFormat::Flac,
        "alac" => AudioFormat::Alac,
        "wav" | "wave" => AudioFormat::Wav,
        "aiff" | "aif" => AudioFormat::Aiff,
        "ape" => AudioFormat::Ape,
        "mp3" => AudioFormat::Mp3,
        _ => AudioFormat::Unknown,
    }
}

/// Synthetic-id namespace floor for Plex track rows: `2^40`. Plex rating-key
/// ids (`PlexCachedTrack.id`, a parsed/hashed rating_key) share the SMALL
/// integer space with `local_tracks.id`, so without namespacing a Plex row and
/// a local row can collide on `id` — and the Tracks tab merges both, so any
/// "first match by id" lookup (queue start, selection, play-next) would resolve
/// to the wrong row. Offsetting Plex ids into `[2^40, 2^41)` keeps them clear of
/// local ids (`< 2^40`) AND of ephemeral ids (`>= 2^48 = EPHEMERAL_ID_FLOOR`),
/// so `is_ephemeral_id` still returns false. The string rating_key is preserved
/// separately in `file_path`, so playback resolution is unaffected.
pub(crate) const PLEX_TRACK_ID_FLOOR: u64 = 1 << 40;

/// Map a Plex-cache track row to the `LocalTrack` shape the album-detail view
/// and the Tracks tab render. The `file_path` carries the Plex `rating_key` (the
/// playback slice resolves the stream URL from it); `artwork_path` is the raw
/// `/library/...` thumb path (tokenized at decode time by the PlexThumb artwork
/// arm); `source` is `"plex"` so the UI classifies the row correctly. The `id`
/// is namespaced (see `PLEX_TRACK_ID_FLOOR`) to avoid colliding with local rows.
pub(crate) fn map_plex_cached_to_local_track(t: qbz_plex::PlexCachedTrack) -> qbz_library::LocalTrack {
    qbz_library::LocalTrack {
        id: (PLEX_TRACK_ID_FLOOR | (t.id & (PLEX_TRACK_ID_FLOOR - 1))) as i64,
        file_path: t.rating_key,
        title: t.title,
        artist: t.artist,
        album: t.album.clone(),
        // Version key: open_local_album groups tracks into selectable versions
        // by album_group_key. `album_key` is a title+artist hash, so two
        // distinct Plex albums with the same name collapse into one group and
        // their tracks interleave (1,1,2,2,...). Key on the Plex album
        // (parent_rating_key) instead so each edition is its own version; fall
        // back to album_key on pre-resync rows that lack it (one merged group).
        album_group_key: t
            .parent_rating_key
            .as_deref()
            .filter(|k| !k.is_empty())
            .map(|k| format!("plex:album:{k}"))
            .unwrap_or_else(|| t.album_key.clone()),
        // Feeds the album-detail header title (apply_album_version reads
        // tracks.first().album_group_title) — use the Plex album name.
        album_group_title: t.album.clone(),
        track_number: t.track_number,
        disc_number: t.disc_number,
        duration_secs: t.duration_secs,
        format: parse_audio_format(&t.format),
        bit_depth: t.bit_depth,
        sample_rate: t.sample_rate as f64,
        artwork_path: t.artwork_path,
        source: Some("plex".to_string()),
        ..Default::default()
    }
}

/// Fetch an album's tracks by group key, trying the metadata grouping first
/// (Albums tab) then the folder grouping (Folders tab). Blocking. Plex albums
/// (`plex:<hash>` group keys) are served from the Plex cache DB instead.
pub fn fetch_album_tracks_blocking(group_key: &str) -> Vec<qbz_library::LocalTrack> {
    if group_key.starts_with("plex:") {
        return qbz_plex::plex_cache_get_album_tracks(group_key.to_string())
            .unwrap_or_default()
            .into_iter()
            .map(map_plex_cached_to_local_track)
            .collect();
    }
    crate::library_db::with_db(|db| {
        let meta = db.get_album_tracks_metadata(group_key)?;
        if !meta.is_empty() {
            return Ok(meta);
        }
        db.get_album_tracks(group_key)
    })
    .unwrap_or_default()
}

// ======================= Ephemeral folder =========================
// Open a folder OUTSIDE the indexed library, browse + play it without writing
// to library.db. The scan/metadata logic is shared (`qbz_library::ephemeral`);
// here we drive the picker, build the album-grouped pane, and persist the path.

/// Last path segment (folder name) for the header.
fn folder_display_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Load a cover from a cached artwork path (strips an optional `file://`).
fn load_cover(path: &Option<String>) -> slint::Image {
    let Some(p) = path.as_deref().filter(|s| !s.is_empty()) else {
        return slint::Image::default();
    };
    let p = p.strip_prefix("file://").unwrap_or(p);
    slint::Image::load_from_path(std::path::Path::new(p)).unwrap_or_default()
}

/// Group ephemeral tracks into album blocks (sorted by title), each with its
/// cover + tracks. Returns the blocks and whether the session spans >1 album.
/// MUST run on the UI thread — it loads `slint::Image`s (not Send).
fn build_ephemeral_albums(tracks: &[qbz_library::LocalTrack]) -> (Vec<EphemeralAlbum>, bool) {
    use std::collections::BTreeMap;
    // Preserve scan order within a group; key order is stabilized by title sort.
    let mut groups: BTreeMap<String, Vec<qbz_library::LocalTrack>> = BTreeMap::new();
    for t in tracks {
        groups
            .entry(crate::ephemeral::ephemeral_album_key(t))
            .or_default()
            .push(t.clone());
    }
    let multi = groups.len() > 1;
    let mut albums: Vec<EphemeralAlbum> = groups
        .into_iter()
        .map(|(key, group)| {
            let first = &group[0];
            let title = if first.album_group_title.is_empty() {
                first.album.clone()
            } else {
                first.album_group_title.clone()
            };
            let artist = first
                .album_artist
                .clone()
                .unwrap_or_else(|| first.artist.clone());
            let count = group.len();
            let track_count_label =
                qbz_i18n::tf("{} track", "{} tracks", count as i64, &[&count.to_string()]);
            let meta = match first.year {
                Some(y) if y > 0 => format!("{y} · {track_count_label}"),
                _ => track_count_label,
            };
            let tier = if first.format.to_string().eq_ignore_ascii_case("mp3") {
                "mp3"
            } else {
                match first.bit_depth {
                    Some(b) if b >= 24 => "hires",
                    Some(_) => "cd",
                    None => "",
                }
            };
            let is_cue = first.cue_file_path.is_some() || first.cue_start_secs.is_some();
            let artwork = load_cover(&first.artwork_path);
            let items: Vec<TrackItem> = group.into_iter().map(map_local_track).collect();
            EphemeralAlbum {
                group_key: key.into(),
                title: title.into(),
                artist: artist.into(),
                meta: meta.into(),
                quality_tier: tier.into(),
                is_cue,
                artwork,
                tracks: ModelRc::new(VecModel::from(items)),
            }
        })
        .collect();
    albums.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    (albums, multi)
}

/// Push a scanned ephemeral result onto the UI (UI thread). `focus` switches to
/// the Folders tab (true for an explicit open; false on startup rehydrate so we
/// don't hijack the landing view).
fn apply_ephemeral(
    window: &AppWindow,
    name: &str,
    path: &str,
    tracks: &[qbz_library::LocalTrack],
    focus: bool,
) {
    let (albums, multi) = build_ephemeral_albums(tracks);
    let s = window.global::<LocalLibraryState>();
    s.set_ephemeral_active(true);
    s.set_ephemeral_loading(false);
    s.set_ephemeral_name(name.into());
    s.set_ephemeral_path(path.into());
    s.set_ephemeral_track_count(tracks.len() as i32);
    s.set_ephemeral_multi_album(multi);
    s.set_ephemeral_albums(ModelRc::new(VecModel::from(albums)));
    if focus {
        s.set_active_tab("folders".into());
    }
}

type EphRuntime = Arc<AppRuntime<SlintAdapter>>;

/// Open the native folder picker, then scan + show the ephemeral pane.
pub fn open_ephemeral(
    runtime: EphRuntime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    handle.spawn(async move {
        let Some(dir) = rfd::AsyncFileDialog::new()
            .set_title(&qbz_i18n::t("Choose a folder to play"))
            .pick_folder()
            .await
        else {
            return;
        };
        let path = dir.path().to_string_lossy().to_string();
        scan_ephemeral(Some(runtime), weak, path, true).await;
    });
}

/// Re-open a previously-persisted ephemeral path on startup (no picker). Skips
/// silently if the path is gone, clearing the stale pref.
pub fn rehydrate_ephemeral(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let Some(path) = crate::locallibrary_prefs::ephemeral_path() else {
        return;
    };
    handle.spawn(async move {
        scan_ephemeral(None, weak, path, false).await;
    });
}

/// Shared scan path used by both the picker and rehydrate. When a `runtime` is
/// given (explicit open), any ephemeral track currently playing is wiped first
/// so it can't bleed into the freshly-loaded session (its synthetic id would be
/// reused). Rehydrate passes `None` (startup — nothing is playing).
async fn scan_ephemeral(
    runtime: Option<EphRuntime>,
    weak: slint::Weak<AppWindow>,
    path: String,
    from_picker: bool,
) {
    if let Some(rt) = &runtime {
        crate::playback::wipe_ephemeral_if_playing(rt, &weak).await;
    }
    let name = folder_display_name(&path);
    {
        let nm = name.clone();
        let p = path.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            let s = w.global::<LocalLibraryState>();
            s.set_ephemeral_active(true);
            s.set_ephemeral_loading(true);
            s.set_ephemeral_name(nm.into());
            s.set_ephemeral_path(p.into());
            s.set_ephemeral_albums(ModelRc::new(VecModel::from(Vec::<EphemeralAlbum>::new())));
            if from_picker {
                s.set_active_tab("folders".into());
            }
        });
    }
    let scan_path = path.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::ephemeral::open(std::path::Path::new(&scan_path))
    })
    .await;

    match result {
        Ok(Ok(res)) => {
            let tracks = res.tracks;
            let skipped = res.skipped_files;
            let nm = name.clone();
            let p = path.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                apply_ephemeral(&w, &nm, &p, &tracks, from_picker);
            });
            crate::locallibrary_prefs::save_ephemeral_path(Some(&path));
            if from_picker {
                if skipped > 0 {
                    crate::toast::success_weak(
                        &weak,
                        qbz_i18n::t_args(
                            "Opened folder ({} files skipped)",
                            &[&skipped.to_string()],
                        ),
                    );
                } else {
                    crate::toast::success_weak(&weak, qbz_i18n::t("Folder opened"));
                }
            }
        }
        Ok(Err(e)) => {
            log::warn!("[qbz-slint] ephemeral open failed: {e}");
            let _ = weak.upgrade_in_event_loop(|w| {
                reset_ephemeral_state(&w);
            });
            crate::ephemeral::clear();
            crate::locallibrary_prefs::save_ephemeral_path(None);
            if from_picker {
                crate::toast::error_weak(&weak, qbz_i18n::t("Couldn't open that folder"));
            }
        }
        Err(_) => {
            let _ = weak.upgrade_in_event_loop(|w| {
                reset_ephemeral_state(&w);
            });
        }
    }
}

/// Reset the ephemeral UI state to its closed defaults.
fn reset_ephemeral_state(window: &AppWindow) {
    let s = window.global::<LocalLibraryState>();
    s.set_ephemeral_active(false);
    s.set_ephemeral_loading(false);
    s.set_ephemeral_name("".into());
    s.set_ephemeral_path("".into());
    s.set_ephemeral_track_count(0);
    s.set_ephemeral_multi_album(false);
    s.set_ephemeral_albums(ModelRc::new(VecModel::from(Vec::<EphemeralAlbum>::new())));
}

/// Clear the ephemeral session: drop the pane, the in-memory store, and the
/// persisted path.
pub fn clear_ephemeral(window: &AppWindow) {
    reset_ephemeral_state(window);
    crate::ephemeral::clear();
    crate::locallibrary_prefs::save_ephemeral_path(None);
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
