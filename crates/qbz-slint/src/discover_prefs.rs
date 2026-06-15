//! Discover section-configurator controller (Slice 5).
//!
//! Thin frontend binding over the headless `qbz_app::settings::discover_prefs`
//! store (ADR-006: all model logic — defaults, reconcile, toggle, move, reset —
//! lives in `qbz-app`; this module only owns the per-user store lifecycle, the
//! in-memory authoritative copy, and the push helpers that map prefs into the
//! `DiscoverState` Slint global).
//!
//! Lifecycle mirrors `fav_cache`: a process-global `Mutex<Option<Store>>`
//! (persistence) + `Mutex<Option<Prefs>>` (in-memory authoritative, so a UI
//! toggle never round-trips SQLite on the event loop), bound per session via
//! [`init_for_user`] / [`teardown`] next to the other per-user stores.
//!
//! The render driver: Rust recomputes `prefs.enabled_ordered(tab)` on every
//! mutation and on tab switch, then pushes the ordered descriptor lists. For You
//! descriptors are built here (the data lives in `ForYouState`, dispatched by
//! id); Home / Editor's Picks descriptors are built in `crate::home` (it owns
//! the cached `SectionData` the album-carousel arms embed). The configurator
//! modal reads `config-rows` (the FULL ordered list, enabled + disabled).

use std::path::Path;
use std::sync::Mutex;

use qbz_app::settings::discover_prefs::{
    default_prefs, DiscoverPrefs, DiscoverPrefsStore, DiscoverySectionId, DiscoveryTab,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::artwork::{ImageCache, spawn_loads};
use crate::{AppWindow, ConfigRow, DiscoverSection, DiscoverState, SectionDescriptor};

/// Per-user persistent store. `None` outside an active session.
static STORE: Mutex<Option<DiscoverPrefsStore>> = Mutex::new(None);
/// In-memory authoritative prefs — the source of truth for the UI thread.
static PREFS: Mutex<Option<DiscoverPrefs>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Lifecycle (mirrors fav_cache::{init_for_user, teardown})
// ---------------------------------------------------------------------------

/// Bind the per-user store and load the persisted prefs into memory. Called on
/// every session activation (login / restore / offline entry), next to
/// `fav_cache::init_for_user`. Best-effort: a store-open failure logs and falls
/// back to in-memory defaults (the configurator still works, just non-persistent).
pub fn init_for_user(base_dir: &Path) {
    match DiscoverPrefsStore::new_at(base_dir) {
        Ok(store) => {
            *PREFS.lock().unwrap() = Some(store.load());
            *STORE.lock().unwrap() = Some(store);
        }
        Err(e) => {
            log::error!("[qbz-slint] discover prefs store open failed: {e}");
            *PREFS.lock().unwrap() = Some(default_prefs());
        }
    }
}

/// Drop the per-user store and in-memory prefs on logout.
pub fn teardown() {
    *STORE.lock().unwrap() = None;
    *PREFS.lock().unwrap() = None;
}

/// A clone of the current in-memory prefs (defaults if no session yet).
fn current() -> DiscoverPrefs {
    PREFS.lock().unwrap().clone().unwrap_or_else(default_prefs)
}

fn persist() {
    if let (Some(p), Some(s)) = (
        PREFS.lock().unwrap().as_ref(),
        STORE.lock().unwrap().as_ref(),
    ) {
        if let Err(e) = s.save(p) {
            log::error!("[qbz-slint] discover prefs save failed: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// id -> render kind / i18n label key
// ---------------------------------------------------------------------------

/// Coarse render family. Used by the one tab-dependent Home arm: `mostStreamed`
/// renders as a slim grid on Home but an album carousel on Editor's Picks. The
/// rest of the dispatch is by id; this is harmless metadata for those.
pub fn render_kind(id: DiscoverySectionId) -> &'static str {
    use DiscoverySectionId::*;
    match id {
        // Album carousels (Home / Editor share the Carousel component).
        NewReleases | PressAwards | IdealDiscography | EditorPicks | Qobuzissimes => "albumCarousel",
        // mostStreamed is overridden per tab in `home::tab_descriptors`; this is
        // its Home default.
        MostStreamed => "slimGrid",
        QobuzPlaylists => "playlistCarousel",
        RecentlyPlayedAlbums => "albumCarousel",
        ContinueListening => "slimGrid",
        QobuzMixes => "qobuzMixes",
        RadioStations => "radio",
        TopArtists | ArtistsToFollow => "artistCarousel",
        ArtistSpotlight => "spotlight",
        ReleaseWatch | FavoriteAlbums | SimilarAlbums | RediscoverLibrary | EssentialsByGenre => {
            "albumCarousel"
        }
    }
}

/// id -> Tauri i18n key (frontend concern, NOT in the headless prefs crate per
/// ADR-006). Resolved to a string in Rust because Slint `@tr` needs a literal
/// key. Returns the English label today (the Slint gettext pipeline is unwired,
/// so labels are plain Rust strings — consistent with every other Slint section
/// title). When gettext lands this swaps to an MO lookup with NO `.slint` change.
/// The keys are kept verbatim (with their real, mixed `home.*` / `discover.*` /
/// `discovery.*` namespaces) so the lookup ports 1:1 when the pipeline arrives.
pub fn label_for(id: DiscoverySectionId) -> &'static str {
    use DiscoverySectionId::*;
    match id {
        NewReleases => "New Releases",            // home.newReleases
        PressAwards => "Press Accolades",         // home.pressAwards
        QobuzPlaylists => "Qobuz Playlists",      // home.qobuzPlaylists
        RecentlyPlayedAlbums => "Recently Played", // home.recentlyPlayed
        ContinueListening => "Continue Listening", // home.continueListening
        IdealDiscography => "Ideal Discography",  // discover.idealDiscography
        MostStreamed => "Most Streamed",          // home.mostStreamed
        ReleaseWatch => "Release Watch",          // home.releaseWatch
        EditorPicks => "Albums of the Week",      // home.editorPicks
        Qobuzissimes => "Qobuzissimes",           // home.qobuzissimes
        TopArtists => "Your Top Artists",         // home.yourTopArtists
        FavoriteAlbums => "Favorite Albums",      // home.favoriteAlbums
        QobuzMixes => "Qobuz Mixes",              // home.qobuzMixes
        RadioStations => "Radio Stations",        // home.radioStations
        SimilarAlbums => "More From Your Library", // discovery.similarAlbums
        RediscoverLibrary => "Rediscover Your Library", // discovery.rediscoverLibrary
        EssentialsByGenre => "Essentials by Genre", // discovery.essentialsByGenre
        ArtistsToFollow => "Artists to Follow",   // discovery.artistsToFollow
        ArtistSpotlight => "Artist Spotlight",    // discovery.artistSpotlight
    }
}

// ---------------------------------------------------------------------------
// Descriptor builders
// ---------------------------------------------------------------------------

/// A descriptor with no embedded album payload (For You arms dispatch on id and
/// read the typed ForYouState fields). `section` is an empty default.
fn bare_descriptor(id: DiscoverySectionId) -> SectionDescriptor {
    SectionDescriptor {
        id: SharedString::from(id.as_str()),
        kind: SharedString::from(render_kind(id)),
        section: DiscoverSection::default(),
    }
}

/// For You ordered ENABLED descriptors. The For You delegate reads ForYouState
/// by id and keeps its own `length > 0` self-hide gate (qobuzMixes excepted), so
/// the descriptor list is the pure visibility+order driver and carries no album
/// payload. `essentialsByGenre` is DROPPED here: it is Slice-2c-blocked (no
/// `ForYouState.essentials` field exists yet), so emitting it would mount a
/// delegate with no matching arm. It re-appears automatically once Slice 2c adds
/// the field and an arm.
fn foryou_descriptors(prefs: &DiscoverPrefs) -> Vec<SectionDescriptor> {
    prefs
        .enabled_ordered(DiscoveryTab::ForYou)
        .into_iter()
        .filter(|id| *id != DiscoverySectionId::EssentialsByGenre)
        .map(bare_descriptor)
        .collect()
}

/// Push the descriptor lists for ALL three tabs. Home / Editor's Picks lists are
/// built by `crate::home` from the cached section data (the album-carousel arms
/// embed it); For You is built here. When the active tab is For You the Home /
/// Editor lists are pushed EMPTY so the Home repeater renders nothing for that
/// tab — content is controlled purely via the model, avoiding a conditional
/// repeater (preferred unconditional-repeater form; see HomeView).
pub fn push_descriptors(window: &AppWindow, prefs: &DiscoverPrefs) {
    let state = window.global::<DiscoverState>();
    let active = state.get_active_tab().to_string();

    // For You list (always pushed; the For You view is mounted only when active).
    state.set_foryou_sections(ModelRc::new(VecModel::from(foryou_descriptors(prefs))));

    if active == "forYou" {
        // Drive the Home repeater empty for the For You tab.
        state.set_home_sections(ModelRc::new(VecModel::from(Vec::<SectionDescriptor>::new())));
        state.set_editor_sections(ModelRc::new(VecModel::from(Vec::<SectionDescriptor>::new())));
    } else {
        // Home + Editor descriptors come from the cached section data.
        let (home, editor) = crate::home::tab_descriptors(prefs);
        state.set_home_sections(ModelRc::new(VecModel::from(home)));
        state.set_editor_sections(ModelRc::new(VecModel::from(editor)));
    }
}

/// Push the configurator modal payload for one tab: the FULL ordered list
/// (enabled AND disabled), with labels resolved in Rust, plus the enabled/total
/// counts. `can-move-up/down` are NOT struct fields — the modal computes boundary
/// state from the row index, so the struct stays minimal.
pub fn push_config_rows(window: &AppWindow, prefs: &DiscoverPrefs, tab: DiscoveryTab) {
    let rows: Vec<ConfigRow> = prefs
        .tab(tab)
        .iter()
        .map(|p| ConfigRow {
            id: SharedString::from(p.id.as_str()),
            label: SharedString::from(label_for(p.id)),
            enabled: p.enabled,
        })
        .collect();
    let total = rows.len() as i32;
    let enabled = prefs.enabled_count(tab) as i32;
    let state = window.global::<DiscoverState>();
    state.set_config_rows(ModelRc::new(VecModel::from(rows)));
    state.set_enabled_count(enabled);
    state.set_total_count(total);
}

/// Seed the descriptor lists at shell entry so the render loop has data before
/// the first `apply_home`. Mirrors `myqbz_prefs::seed`.
pub fn seed(window: &AppWindow) {
    let prefs = current();
    push_descriptors(window, &prefs);
}

// ---------------------------------------------------------------------------
// Mutation handlers (mutate -> persist -> re-push -> re-render)
// ---------------------------------------------------------------------------

/// After any mutation: re-push the descriptor lists (visibility + order),
/// refresh the live modal rows for the active tab, and re-render Home / Editor
/// from the cache (For You's data already lives in ForYouState — descriptors are
/// its sole driver). Returns artwork jobs to re-fire for newly-shown Home album
/// sections (mirrors `select_tab`'s job return); empty for For You.
fn apply_after_mutation(window: &AppWindow, mutated: DiscoveryTab) -> Vec<crate::artwork::ArtworkJob> {
    let prefs = current();
    push_descriptors(window, &prefs);
    if let Some(active) =
        DiscoveryTab::from_key(window.global::<DiscoverState>().get_active_tab().as_str())
    {
        push_config_rows(window, &prefs, active);
    }
    match mutated {
        DiscoveryTab::Home | DiscoveryTab::EditorPicks => {
            crate::home::rerender_active_tab(window, &prefs)
        }
        // For You: descriptor list is the sole driver; data already in ForYouState.
        DiscoveryTab::ForYou => Vec::new(),
    }
}

pub fn on_open_configurator(window: &AppWindow) {
    let prefs = current();
    if let Some(active) =
        DiscoveryTab::from_key(window.global::<DiscoverState>().get_active_tab().as_str())
    {
        push_config_rows(window, &prefs, active);
    }
    window.global::<DiscoverState>().set_configurator_open(true);
}

pub fn on_close_configurator(window: &AppWindow) {
    window.global::<DiscoverState>().set_configurator_open(false);
}

pub fn on_toggle(window: &AppWindow, tab: &str, id: &str, cache: &ImageCache) {
    let (Some(tab), Some(id)) = (DiscoveryTab::from_key(tab), DiscoverySectionId::from_str(id))
    else {
        return;
    };
    if let Some(p) = PREFS.lock().unwrap().as_mut() {
        p.toggle(tab, id);
    }
    persist();
    let jobs = apply_after_mutation(window, tab);
    spawn_loads(jobs, window.as_weak(), cache.clone());
}

pub fn on_move(window: &AppWindow, tab: &str, id: &str, dir: i32, cache: &ImageCache) {
    let (Some(tab), Some(id)) = (DiscoveryTab::from_key(tab), DiscoverySectionId::from_str(id))
    else {
        return;
    };
    let dir = dir.clamp(-1, 1) as i8;
    if let Some(p) = PREFS.lock().unwrap().as_mut() {
        p.move_section(tab, id, dir);
    }
    persist();
    let jobs = apply_after_mutation(window, tab);
    spawn_loads(jobs, window.as_weak(), cache.clone());
}

pub fn on_reset(window: &AppWindow, tab: &str, cache: &ImageCache) {
    let Some(tab) = DiscoveryTab::from_key(tab) else {
        return;
    };
    if let Some(p) = PREFS.lock().unwrap().as_mut() {
        p.reset_tab(tab);
    }
    persist();
    let jobs = apply_after_mutation(window, tab);
    spawn_loads(jobs, window.as_weak(), cache.clone());
}

/// Read-through used by `crate::home::select_tab` so a tab switch recomputes the
/// active tab's descriptors from the same prefs the controller owns.
pub fn prefs_snapshot() -> DiscoverPrefs {
    current()
}
