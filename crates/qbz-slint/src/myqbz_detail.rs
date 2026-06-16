//! My QBZ — Collection / Mixtape DETAIL view controller (read-only slice).
//!
//! Mirrors `crate::playlist` (a cached full item list backs a client-side
//! filter -> search -> sort that re-derives the visible model) and reuses the
//! grid controller's mosaic + URL-downscale helpers from `crate::myqbz`. It
//! loads ONE `MixtapeCollection` (items come hydrated) via
//! `qbz_mixtape::repo::get_collection` through `library_db::with_db` +
//! `with_connection`, precomputes every display string (type label, source
//! kind, quality detail, tracks/year columns, downscaled `_50` row artwork
//! URL, up-to-9 hero-mosaic URLs), and pushes ready-to-render
//! `MixtapeDetailItem`s into `MyQbzDetailState`. The view does NO per-row
//! lookups.
//!
//! READ-ONLY SCOPE (Phase-2 Slice 3): nav-in (the grid card click) routes here
//! and loads real data — that is the testable path. The hero CTAs
//! (play/shuffle/dj-mix/edit/delete/sync), per-row context-menu items, and the
//! select-mode bulk bar are VISIBLE 1:1 but their handlers are logging stubs
//! (wired in main.rs). DEFERRED to a later slice: the live source/quality
//! `resolveItems` resolution (so quality badges + plex/local source kinds are
//! placeholders here, derived only from the stored `source`), the per-item
//! inline track expansion (the "expanded" view-mode renders its toggle + shell
//! only), the rename/description/delete/cover/DJ-mix modals, and persisted
//! per-collection view-prefs.
//!
//! The backend (`qbz-mixtape`) is reused directly — no Tauri command wrappers
//! (ADR-005), headless (ADR-006).

use qbz_models::mixtape::{
    AlbumSource, CollectionKind, CollectionPlayMode, ItemType, MixtapeCollection,
    MixtapeCollectionItem,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::artwork::{self, ArtworkJob, ArtworkTarget, ImageCache};
use crate::{AppWindow, ContentView, MixtapeDetailItem, MyQbzDetailState, NavState, TrackItem};

/// Process-global runtime handle, set ONCE during startup wiring. The
/// mutation-reload paths (cover upload/remove, rename/description/convert/
/// remove-selected) re-run `navigate` to refresh the open detail, and
/// `navigate`'s resolveItems pass needs the runtime; rather than thread the
/// `Arc<AppRuntime>` through every one of those entry points + their main.rs
/// callsites, they pull it from here. The primary nav-in path still passes the
/// runtime explicitly. Set by `set_runtime` at wiring time.
static GLOBAL_RUNTIME: std::sync::OnceLock<
    std::sync::Arc<qbz_app::shell::AppRuntime<crate::adapter::SlintAdapter>>,
> = std::sync::OnceLock::new();

/// Store the shared runtime for the global reload paths (idempotent — a second
/// call is ignored). Called once during startup wiring.
pub fn set_runtime(
    runtime: std::sync::Arc<qbz_app::shell::AppRuntime<crate::adapter::SlintAdapter>>,
) {
    let _ = GLOBAL_RUNTIME.set(runtime);
}

/// The shared runtime for the reload paths. `None` only before wiring (never in
/// practice, since reloads happen after the UI is up).
pub fn global_runtime(
) -> Option<std::sync::Arc<qbz_app::shell::AppRuntime<crate::adapter::SlintAdapter>>> {
    GLOBAL_RUNTIME.get().cloned()
}

thread_local! {
    /// The full, original-order item list for the open collection — the
    /// canonical source the toolbar derives the visible list from. UI thread
    /// only (mirrors `playlist::FULL_ITEMS`).
    static FULL_ITEMS: std::cell::RefCell<Vec<MixtapeCollectionItem>> =
        const { std::cell::RefCell::new(Vec::new()) };

    /// Expanded-mode inline-tracks cache, keyed by a STABLE per-item key
    /// (`source|source_item_id`). Populated ONCE per item when its inline tracks
    /// are first resolved (spec 12 §8). It is the durable home of the resolved
    /// tracks: `refresh_view` rebuilds the `MixtapeDetailItem` render rows on
    /// every filter/sort/search (so the per-row `inline_tracks` would be wiped),
    /// so after each re-derive we re-hydrate the rows from THIS cache instead of
    /// re-fetching. The cached `Vec<TrackItem>` carries `slint::Image`s (`!Send`)
    /// — safe here because the cache lives on the UI thread only. Cleared on
    /// `reset` (a fresh collection open). Mirrors the Tauri per-item track cache
    /// that survives the client-side re-derive.
    static INLINE_CACHE: std::cell::RefCell<std::collections::HashMap<String, Vec<TrackItem>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());

    /// Per-collection view-prefs "hydrated" gate (mirrors Tauri's
    /// `prefsHydrated`). `false` from `reset` until `apply` has restored the
    /// stored prefs; while `false` every toolbar persist is suppressed so an
    /// early setter can't clobber the about-to-be-restored prefs. UI thread.
    static PREFS_HYDRATED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// resolveItems cache, keyed by the STABLE per-item key
    /// (`source|source_item_id`). Holds the LIVE-resolved per-row display values
    /// that `MixtapeCollectionItem` alone can't carry: the resolved source kind
    /// (qobuz / plex / local), the album-level quality tier + detail (derived
    /// from the item's first resolved track), and the resolved TYPE label
    /// (album -> EP/Single/Album by track count). Populated once per item by the
    /// `resolve_items` pass (spawned after `apply`), re-hydrated in `to_item` on
    /// every filter/sort/search re-derive so the columns stay populated without
    /// re-fetching. Cleared on `reset`. UI thread only.
    static RESOLVE_CACHE: std::cell::RefCell<std::collections::HashMap<String, ResolvedItem>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The live-resolved per-row display values cached by `RESOLVE_CACHE`.
#[derive(Clone, Default)]
struct ResolvedItem {
    /// "qobuz" | "plex" | "local".
    source_kind: String,
    /// "hires" | "cd" | "" — the row's `QualityBadgeFull` tier.
    quality_tier: String,
    /// "24-bit / 96 kHz" etc; "" when tier is "".
    quality_detail: String,
    /// Uppercased TYPE-column label (ALBUM / EP / SINGLE / TRACK / PLAYLIST).
    type_label: String,
    /// First resolved track's artwork (bare local file path / Plex
    /// `/library/...` thumb / Qobuz URL — the `file://` prefix is stripped).
    /// Backfills rows whose stored `artwork_url` was empty (e.g. disco-builder
    /// local items saved with NULL art before the builder carried the cover).
    artwork_url: String,
    /// First resolved track's numeric Qobuz artist id ("" when the track
    /// carries none — local/Plex items). The stored `MixtapeCollectionItem`
    /// only has the artist NAME (subtitle), so this is what lets a Qobuz
    /// item's artist link open the QOBUZ artist page instead of falling back
    /// to the LocalLibrary Artists tab.
    artist_id: String,
}

/// Persist the open collection's current toolbar prefs (spec 12 §18), gated on
/// the hydrated flag so a setter firing before `apply` restores the stored
/// prefs cannot clobber them. The persisted fields are the five §18 fields
/// (view-mode / sort / sort-dir / type-filter / source flags); search +
/// select-mode stay transient. UI thread.
pub fn persist_prefs(window: &AppWindow) {
    if !PREFS_HYDRATED.with(|c| c.get()) {
        return;
    }
    let state = window.global::<MyQbzDetailState>();
    let id = state.get_id().to_string();
    if id.is_empty() {
        return;
    }
    let prefs = crate::myqbz_view_prefs::Prefs {
        view_mode: state.get_view_mode().to_string(),
        sort_by: state.get_sort().to_string(),
        sort_dir: state.get_sort_dir().to_string(),
        type_filter: state.get_type_filter().to_string(),
        src_qobuz: state.get_src_qobuz(),
        src_plex: state.get_src_plex(),
        src_local: state.get_src_local(),
    };
    crate::myqbz_view_prefs::save(&id, &prefs);
}

/// Stable per-item key for the inline-tracks cache (`source|source_item_id`).
/// `source_item_id` alone is the row's logical key, but pairing it with the
/// source keeps qobuz-vs-local collisions impossible.
fn inline_cache_key(source: &str, source_item_id: &str) -> String {
    format!("{source}|{source_item_id}")
}

// ──────────────────────────── DB read path ────────────────────────────

/// Load one collection (items hydrated by the repo) from the per-user
/// library.db. Returns `None` when the DB is unavailable or the id is unknown.
pub fn get_collection(id: &str) -> Option<MixtapeCollection> {
    crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| {
            qbz_mixtape::repo::get_collection(conn, id).unwrap_or_else(|e| {
                log::warn!("[qbz-slint] myqbz_detail get_collection({id}) failed: {e}");
                None
            })
        }))
    })
    .flatten()
}

// ──────────────────────────── string helpers ──────────────────────────

fn kind_str(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Mixtape => "mixtape",
        CollectionKind::Collection => "collection",
        CollectionKind::ArtistCollection => "artist_collection",
    }
}

/// Eyebrow label (Tauri `kindLabel`): mixtapes.label / collections.artistLabel
/// / collections.label, uppercased to match the grid card eyebrow.
fn kind_label(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Mixtape => "MIXTAPE",
        CollectionKind::ArtistCollection => "ARTIST",
        CollectionKind::Collection => "COLLECTION",
    }
}

fn play_mode_str(mode: CollectionPlayMode) -> &'static str {
    match mode {
        CollectionPlayMode::InOrder => "in_order",
        CollectionPlayMode::AlbumShuffle => "album_shuffle",
    }
}

pub fn source_str(source: AlbumSource) -> &'static str {
    match source {
        AlbumSource::Qobuz => "qobuz",
        AlbumSource::Local => "local",
    }
}

pub fn item_type_str(t: ItemType) -> &'static str {
    match t {
        ItemType::Album => "album",
        ItemType::Track => "track",
        ItemType::Playlist => "playlist",
    }
}

/// `mixtapes.albumCount` ICU plural — always "album(s)" regardless of
/// item_type (1:1 with the PSD / the grid card meta).
fn album_count_label(count: usize) -> String {
    if count == 1 {
        "1 album".to_string()
    } else {
        format!("{count} albums")
    }
}

/// Type-cell label, uppercase (spec 12 §6.3 col-3 `itemTypeLabel`). Release-type
/// overrides (album rows showing EP/Single/…) are a later slice — albums render
/// "ALBUM" here.
fn type_label(t: ItemType) -> &'static str {
    match t {
        ItemType::Album => "ALBUM",
        ItemType::Track => "TRACK",
        ItemType::Playlist => "PLAYLIST",
    }
}

/// TRACKS column (spec 12 §6.3 col-6 `itemTracks`): "1" for a track, else the
/// count or an em-dash.
fn tracks_text(item: &MixtapeCollectionItem) -> String {
    match item.item_type {
        ItemType::Track => "1".to_string(),
        _ => match item.track_count {
            Some(n) => n.to_string(),
            None => "—".to_string(),
        },
    }
}

/// YEAR column (spec 12 §6.3 col-7 `itemYear`): the year or "".
fn year_text(item: &MixtapeCollectionItem) -> String {
    item.year.map(|y| y.to_string()).unwrap_or_default()
}

// ──────────────────────────── model builder ───────────────────────────

/// Build one ready-to-render row. The `_50` row-artwork downscale reuses the
/// grid controller's `small_qobuz_url`. Source kind defaults from the stored
/// `source` (the live plex-vs-local-vs-qobuz `resolveItems` resolution is
/// DEFERRED, so quality badge inputs stay empty here).
fn to_item(item: &MixtapeCollectionItem) -> MixtapeDetailItem {
    let source = source_str(item.source);
    // `small_qobuz_url` only rewrites Qobuz CDN `_<size>.jpg` URLs; running it on
    // a LOCAL filesystem path (or a Plex `/library/...` path) corrupts/no-ops it.
    // Gate the rewrite to Qobuz items; local/plex artwork passes through raw so
    // the source-aware artwork dispatch can read it as a file/Plex thumb.
    let mut artwork_url = item
        .artwork_url
        .as_deref()
        .filter(|u| !u.is_empty())
        .map(|u| {
            if item.source == AlbumSource::Qobuz {
                crate::myqbz::small_qobuz_url(u, 50)
            } else {
                u.to_string()
            }
        })
        .unwrap_or_default();

    // Re-hydrate the live-resolved display values (source kind, quality
    // tier/detail, type label, backfilled artwork) from the resolveItems cache
    // so a filter/sort/search re-derive keeps the columns populated without
    // re-fetching. A miss falls back to the stored-source defaults (qobuz/local
    // + no quality), which the `resolve_items` pass then fills in. On a miss the
    // row is flagged `quality_resolving` so the quality cell shows a skeleton
    // until the async pass lands.
    let cache_key = inline_cache_key(source, &item.source_item_id);
    let resolved = RESOLVE_CACHE.with(|cell| cell.borrow().get(&cache_key).cloned());
    let (source_kind, quality_tier, quality_detail, type_label, artist_id, quality_resolving) =
        match resolved {
            Some(r) => {
                // Backfill the row cover from the resolved track when the stored
                // `artwork_url` was empty (disco-builder local items, older saves).
                if artwork_url.is_empty() && !r.artwork_url.is_empty() {
                    artwork_url = r.artwork_url.clone();
                }
                (r.source_kind, r.quality_tier, r.quality_detail, r.type_label, r.artist_id, false)
            }
            None => (
                source.to_string(),
                String::new(),
                String::new(),
                type_label(item.item_type).to_string(),
                String::new(),
                true,
            ),
        };

    // Re-hydrate inline tracks from the controller-level cache (keyed
    // `source|source_item_id`) so a filter/sort/search re-derive does NOT lose
    // already-resolved tracks or trigger a re-fetch (spec 12 §8 — the cache
    // survives the re-derive). A cache hit marks the row loaded.
    let cache_key = inline_cache_key(source, &item.source_item_id);
    let (cached_tracks, tracks_loaded) = INLINE_CACHE.with(|cell| {
        match cell.borrow().get(&cache_key) {
            Some(tracks) => (tracks.clone(), true),
            None => (Vec::new(), false),
        }
    });

    MixtapeDetailItem {
        position: item.position,
        item_type: item_type_str(item.item_type).into(),
        source: source.into(),
        source_item_id: item.source_item_id.clone().into(),
        title: item.title.clone().into(),
        subtitle: item.subtitle.clone().unwrap_or_default().into(),
        // Only qobuz items get a clickable artist subtitle (spec 12 §6.3).
        subtitle_is_link: item.source == AlbumSource::Qobuz
            && item.subtitle.as_deref().map(|s| !s.is_empty()).unwrap_or(false),
        // Resolved Qobuz artist id ("" until resolveItems lands / for
        // local-Plex items) — routes the artist link to the Qobuz artist page.
        artist_id: artist_id.into(),
        // Resolved source kind / quality / type label — from the resolveItems
        // cache (above) when resolved; else the stored-source defaults.
        source_kind: source_kind.into(),
        type_label: type_label.into(),
        quality_tier: quality_tier.into(),
        quality_detail: quality_detail.into(),
        quality_resolving,
        tracks_text: tracks_text(item).into(),
        year_text: year_text(item).into(),
        artwork_url: artwork_url.into(),
        artwork: slint::Image::default(),
        selected: false,
        // Expanded-mode inline tracks (spec 12 §8): albums and playlists can
        // host inline tracks; a bare track item is itself (no expansion).
        can_expand: matches!(item.item_type, ItemType::Album | ItemType::Playlist),
        // Loaded/tracks come from the per-item cache (above) so the re-derive
        // keeps previously-resolved tracks instead of re-fetching.
        tracks_loaded,
        expand_loading: false,
        inline_tracks: ModelRc::new(VecModel::from(cached_tracks)),
    }
}

// ──────────────────────────── hero mosaic ─────────────────────────────

/// Decide the hero-mosaic cover-count (0 / 4 / 9) + downscaled cell URLs, and
/// push them into `MyQbzDetailState`. Mirrors the grid card's mosaic rule
/// (3x3 only for a Collection with >= 9 items; else 2x2) but at the hero
/// `size = 186` (so the downscale target differs: 2x2 -> 150, 3x3 -> 50).
fn apply_hero_mosaic(state: &MyQbzDetailState, c: &MixtapeCollection) {
    let item_count = c.items.len();
    let has_custom = c.custom_artwork_path.is_some();

    let cols: usize = if c.kind == CollectionKind::Collection && item_count >= 9 {
        3
    } else {
        2
    };
    let cell_count = cols * cols;
    let cover_count = if has_custom || item_count == 0 {
        0
    } else {
        cell_count
    };
    // Hero renders at 186px; cell ~93 (2x2) -> 150, ~62 (3x3) -> 50.
    let target: u32 = if cols == 3 { 50 } else { 150 };

    let url = |i: usize| -> slint::SharedString {
        if has_custom || item_count == 0 || i >= cell_count {
            return slint::SharedString::default();
        }
        let Some(it) = c.items.get(i) else {
            return slint::SharedString::default();
        };
        match it.artwork_url.as_deref() {
            // `small_qobuz_url` is Qobuz-CDN-specific; only rewrite Qobuz cells.
            // Local/Plex artwork paths pass through raw for the source-aware
            // dispatch.
            Some(u) if !u.is_empty() && it.source == AlbumSource::Qobuz => {
                crate::myqbz::small_qobuz_url(u, target).into()
            }
            Some(u) if !u.is_empty() => u.to_string().into(),
            _ => slint::SharedString::default(),
        }
    };

    state.set_cover_count(cover_count as i32);
    state.set_url1(url(0));
    state.set_url2(url(1));
    state.set_url3(url(2));
    state.set_url4(url(3));
    state.set_url5(url(4));
    state.set_url6(url(5));
    state.set_url7(url(6));
    state.set_url8(url(7));
    state.set_url9(url(8));
    // Reset the decoded covers so a re-open does not show stale tiles.
    state.set_cover1(slint::Image::default());
    state.set_cover2(slint::Image::default());
    state.set_cover3(slint::Image::default());
    state.set_cover4(slint::Image::default());
    state.set_cover5(slint::Image::default());
    state.set_cover6(slint::Image::default());
    state.set_cover7(slint::Image::default());
    state.set_cover8(slint::Image::default());
    state.set_cover9(slint::Image::default());
}

// ──────────────────────────── sort / filter / search ──────────────────

/// Apply the active toolbar (type filter -> source filter -> search -> sort)
/// over `FULL_ITEMS` and push the resulting render model. Non-destructive (the
/// persisted order is untouched). UI thread only. Mirrors spec 12 §19.
pub fn refresh_view(window: &AppWindow) {
    let state = window.global::<MyQbzDetailState>();
    let query = state.get_search().trim().to_lowercase();
    let type_filter = state.get_type_filter().to_string();
    let (sq, sp, sl) = (
        state.get_src_qobuz(),
        state.get_src_plex(),
        state.get_src_local(),
    );
    let any_source = sq || sp || sl;
    let sort = state.get_sort().to_string();
    let desc = state.get_sort_dir().to_string() == "desc";

    let mut view: Vec<MixtapeCollectionItem> = FULL_ITEMS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|it| {
                // Type filter (single-select).
                type_filter == "all" || item_type_str(it.item_type) == type_filter
            })
            .filter(|it| {
                // Source filter (multi-select). source_kind currently equals
                // the raw source (resolveItems deferred) — qobuz / local.
                if !any_source {
                    return true;
                }
                let kind = source_str(it.source);
                (sq && kind == "qobuz") || (sp && kind == "plex") || (sl && kind == "local")
            })
            .filter(|it| {
                if query.is_empty() {
                    return true;
                }
                it.title.to_lowercase().contains(&query)
                    || it
                        .subtitle
                        .as_deref()
                        .map(|s| s.to_lowercase().contains(&query))
                        .unwrap_or(false)
            })
            .cloned()
            .collect()
    });

    match sort.as_str() {
        "name" => view.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        "year" => view.sort_by(|a, b| a.year.unwrap_or(0).cmp(&b.year.unwrap_or(0))),
        "tracks" => {
            view.sort_by(|a, b| a.track_count.unwrap_or(0).cmp(&b.track_count.unwrap_or(0)))
        }
        // default "position"
        _ => view.sort_by(|a, b| a.position.cmp(&b.position)),
    }
    if desc {
        view.reverse();
    }

    let items: Vec<MixtapeDetailItem> = view.iter().map(to_item).collect();
    state.set_items(ModelRc::new(VecModel::from(items)));

    // Derived toolbar badges (Rust-computed; the view only reads them).
    let source_count = (sq as i32) + (sp as i32) + (sl as i32);
    state.set_filter_count(source_count + if type_filter != "all" { 1 } else { 0 });
    state.set_has_any_filter(
        type_filter != "all" || any_source || sort != "position" || desc,
    );
}

/// Update the search query and re-render.
pub fn search(window: &AppWindow, query: &str) {
    window.global::<MyQbzDetailState>().set_search(query.into());
    refresh_view(window);
}

/// Set the sort field. Re-selecting the active field flips asc/desc; a new
/// field resets to asc (spec 12 §5.4 `selectSort`).
pub fn set_sort(window: &AppWindow, field: &str) {
    let state = window.global::<MyQbzDetailState>();
    if state.get_sort() == field {
        let dir = if state.get_sort_dir() == "asc" { "desc" } else { "asc" };
        state.set_sort_dir(dir.into());
    } else {
        state.set_sort(field.into());
        state.set_sort_dir("asc".into());
    }
    persist_prefs(window);
    refresh_view(window);
}

/// Single-select the type filter.
pub fn set_type_filter(window: &AppWindow, value: &str) {
    window.global::<MyQbzDetailState>().set_type_filter(value.into());
    persist_prefs(window);
    refresh_view(window);
}

/// Toggle one source-filter flag (multi-select; menu stays open in the view).
pub fn toggle_source_filter(window: &AppWindow, kind: &str) {
    let state = window.global::<MyQbzDetailState>();
    match kind {
        "qobuz" => state.set_src_qobuz(!state.get_src_qobuz()),
        "plex" => state.set_src_plex(!state.get_src_plex()),
        "local" => state.set_src_local(!state.get_src_local()),
        _ => {}
    }
    persist_prefs(window);
    refresh_view(window);
}

/// Reset filters + sort (spec 12 §5.6 reset: type 'all', no sources, sort
/// 'position' asc). Search query is left intact (Tauri's reset doesn't clear
/// it; `hasAnyFilter` excludes search).
pub fn reset_filters(window: &AppWindow) {
    let state = window.global::<MyQbzDetailState>();
    state.set_type_filter("all".into());
    state.set_src_qobuz(false);
    state.set_src_plex(false);
    state.set_src_local(false);
    state.set_sort("position".into());
    state.set_sort_dir("asc".into());
    persist_prefs(window);
    refresh_view(window);
}

/// Set the view-mode (list|grid|expanded) + persist it (spec 12 §18). The
/// expanded-mode inline-track fetch stays in `main.rs` (it needs the runtime +
/// handle); this only updates state + persists so the per-collection prefs
/// remember the chosen mode.
pub fn set_view_mode(window: &AppWindow, mode: &str) {
    window.global::<MyQbzDetailState>().set_view_mode(mode.into());
    persist_prefs(window);
}

/// Toggle multi-select edit mode. Leaving clears any selection.
pub fn toggle_select_mode(window: &AppWindow) {
    let state = window.global::<MyQbzDetailState>();
    let on = !state.get_select_mode();
    if !on {
        let model = state.get_items();
        for i in 0..model.row_count() {
            if let Some(mut it) = model.row_data(i) {
                if it.selected {
                    it.selected = false;
                    model.set_row_data(i, it);
                }
            }
        }
        state.set_selected_count(0);
    }
    state.set_select_mode(on);
}

/// Toggle one row's selection by position. Recounts the selection.
pub fn toggle_item_select(window: &AppWindow, position: i32) {
    let state = window.global::<MyQbzDetailState>();
    let model = state.get_items();
    for i in 0..model.row_count() {
        if let Some(mut it) = model.row_data(i) {
            if it.position == position {
                it.selected = !it.selected;
                model.set_row_data(i, it);
                break;
            }
        }
    }
    let count = (0..model.row_count())
        .filter(|&i| model.row_data(i).map(|it| it.selected).unwrap_or(false))
        .count() as i32;
    state.set_selected_count(count);
}

/// The set of currently-selected row positions (select-mode), read off the
/// rendered item model. UI thread.
pub fn selected_positions(window: &AppWindow) -> Vec<i32> {
    let model = window.global::<MyQbzDetailState>().get_items();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|it| it.selected)
        .map(|it| it.position)
        .collect()
}

/// The full `MixtapeCollectionItem`s (with year / track_count) for the
/// currently-selected positions, in ascending position order. Sourced from
/// `FULL_ITEMS` (the slint `MixtapeDetailItem` carries only display text, not
/// the numeric year/track_count the add payload needs). UI thread.
pub fn selected_full_items(window: &AppWindow) -> Vec<MixtapeCollectionItem> {
    let mut positions = selected_positions(window);
    positions.sort_unstable();
    FULL_ITEMS.with(|cell| {
        let items = cell.borrow();
        positions
            .iter()
            .filter_map(|p| items.iter().find(|it| it.position == *p).cloned())
            .collect()
    })
}

// ──────────────────────── expanded-mode inline tracks ─────────────────────

/// The full `MixtapeCollectionItem` for one `source_item_id` (the row's stable
/// key). Sourced from `FULL_ITEMS` so the resolver gets the numeric
/// year/track_count + the typed item_type/source. UI thread.
fn full_item_by_source_id(source_item_id: &str) -> Option<MixtapeCollectionItem> {
    FULL_ITEMS.with(|cell| {
        cell.borrow()
            .iter()
            .find(|it| it.source_item_id == source_item_id)
            .cloned()
    })
}

/// "m:ss" track duration (spec 12 §8 `formatSec`). A zero/missing duration
/// renders the placeholder "--:--" (NOT "0:00") so an unresolved length reads as
/// unknown, matching the Tauri formatter. (`duration_secs` is `u64`, so the
/// "negative" case collapses to the zero case.)
fn track_duration_str(secs: u64) -> String {
    if secs == 0 {
        "--:--".to_string()
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// Title + parenthesized Qobuz version suffix (spec 12 §8 `formatTrackTitle`).
fn inline_track_title(track: &qbz_models::QueueTrack) -> String {
    match track.version.as_deref().filter(|v| !v.is_empty()) {
        Some(version) => format!("{} ({version})", track.title),
        None => track.title.clone(),
    }
}

/// Map one resolved `QueueTrack` into the shared `TrackItem` the inline
/// `TrackRow`s render. Quality tier/detail are derived the same way as the
/// now-playing + album-row badges (24-bit+ = Hi-Res), so the inline badge
/// matches every other surface. `source` drives the per-source `TrackRow`
/// affordances (Plex/local rows hide the favorite + offline columns).
///
/// `resolver_index` is the 0-based position of this track in the resolver's
/// output. The resolved `QueueTrack` carries no explicit album track number, so
/// the displayed number is the resolver's order (1-based) — i.e. "use the
/// resolver's track number when present", which for this model is the resolved
/// sequence position. (`TrackRow` would otherwise index-fall-back, but baking
/// the number here keeps the row number correct regardless of the caller.)
fn track_to_item(track: &qbz_models::QueueTrack, resolver_index: usize) -> TrackItem {
    let quality_tier = match track.bit_depth {
        Some(d) if d >= 24 => "hires",
        Some(_) => "cd",
        None if track.hires => "hires",
        None => "",
    };
    let quality_detail = if quality_tier.is_empty() {
        String::new()
    } else {
        crate::quality::detail(track.bit_depth, track.sample_rate)
    };
    let source = track
        .source
        .clone()
        .unwrap_or_else(|| if track.is_local { "local".into() } else { "qobuz".into() });

    TrackItem {
        id: track.id.to_string().into(),
        number: (resolver_index + 1).to_string().into(),
        title: inline_track_title(track).into(),
        artist: track.artist.clone().into(),
        album: String::new().into(),
        duration: track_duration_str(track.duration_secs).into(),
        quality_tier: quality_tier.into(),
        quality_detail: quality_detail.into(),
        explicit: track.parental_warning,
        selected: false,
        artwork_url: String::new().into(),
        artwork: slint::Image::default(),
        is_favorite: false,
        artist_id: track.artist_id.map(|id| id.to_string()).unwrap_or_default().into(),
        album_id: track.album_id.clone().unwrap_or_default().into(),
        source: source.into(),
        removing: false,
        cache_status: 0,
        cache_progress: 0.0,
        unlocking: false,
        // Disc grouping is album-detail only; flat lists carry no header.
        disc_header_number: 0,
    }
}

/// Find the rendered row for `source_item_id` and mutate it in place. UI thread.
fn with_row_by_source_id<F: FnOnce(&mut MixtapeDetailItem)>(
    window: &AppWindow,
    source_item_id: &str,
    f: F,
) {
    let model = window.global::<MyQbzDetailState>().get_items();
    for i in 0..model.row_count() {
        if let Some(mut it) = model.row_data(i) {
            if it.source_item_id == source_item_id {
                f(&mut it);
                model.set_row_data(i, it);
                break;
            }
        }
    }
}

/// Ensure every expandable item's inline tracks are loaded (spec 12 §8). Fired
/// when the "expanded" view-mode becomes active. For each rendered row that
/// `can_expand` and is not already loaded / loading, flips `expand-loading` on
/// and spawns a per-item fetch via the shared enqueue resolver
/// (`myqbz_play::fetch_item_tracks`); on completion it populates that row's
/// inline-tracks model + marks it loaded. Idempotent: already-cached rows are
/// skipped, so re-entering expanded mode is instant (and re-deriving the model
/// after a filter/sort resets `tracks_loaded`, so the new rows re-fetch).
pub fn ensure_expanded(
    runtime: std::sync::Arc<qbz_app::shell::AppRuntime<crate::adapter::SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    let Some(window) = weak.upgrade() else { return };
    let model = window.global::<MyQbzDetailState>().get_items();

    // Snapshot the rows that still need a fetch (source + source-item-id), then
    // mark them loading in one pass (mutating the model while iterating is fine
    // — we set_row_data the same index we read). `tracks_loaded` rows are
    // skipped: the cache already re-hydrated them in `to_item`, so a re-derive
    // is instant (no re-fetch).
    let mut pending: Vec<(String, String)> = Vec::new();
    for i in 0..model.row_count() {
        if let Some(mut it) = model.row_data(i) {
            if it.can_expand && !it.tracks_loaded && !it.expand_loading {
                it.expand_loading = true;
                let source = it.source.to_string();
                let id = it.source_item_id.to_string();
                model.set_row_data(i, it);
                pending.push((source, id));
            }
        }
    }

    for (source, source_item_id) in pending {
        let Some(full_item) = full_item_by_source_id(&source_item_id) else {
            // No backing item (shouldn't happen) — clear the spinner.
            with_row_by_source_id(&window, &source_item_id, |it| it.expand_loading = false);
            continue;
        };
        let runtime = runtime.clone();
        let weak = weak.clone();
        handle.spawn(async move {
            // `Vec<QueueTrack>` is `Send`; the mapped `Vec<TrackItem>` carries
            // a `slint::Image` (!Send), so it must be built INSIDE the event
            // loop, not moved across the thread boundary.
            let tracks = crate::myqbz_play::fetch_item_tracks(&runtime, &full_item).await;
            let _ = weak.upgrade_in_event_loop(move |w| {
                let items: Vec<TrackItem> = tracks
                    .iter()
                    .enumerate()
                    .map(|(i, t)| track_to_item(t, i))
                    .collect();
                // Persist into the controller-level cache (keyed
                // `source|source_item_id`) so a later filter/sort/search
                // re-derive re-hydrates this row from the cache instead of
                // re-fetching (spec 12 §8 — cache survives the re-derive).
                let cache_key = inline_cache_key(&source, &source_item_id);
                INLINE_CACHE.with(|cell| {
                    cell.borrow_mut().insert(cache_key, items.clone());
                });
                with_row_by_source_id(&w, &source_item_id, |it| {
                    it.expand_loading = false;
                    it.tracks_loaded = true;
                    it.inline_tracks = ModelRc::new(VecModel::from(items));
                });
            });
        });
    }
}

/// Derive a row's resolved display values from its resolved tracks (spec §17
/// resolveItems). Album-level quality = the item's first resolved track's
/// quality (24-bit+ = Hi-Res); the same tier/detail rule every other surface
/// uses. Source kind = the first track's `source` (or `is_local` fallback) —
/// this is how a Plex item (stored as `AlbumSource::Local`) is finally
/// classified `"plex"`. Type label: non-album rows keep their stored type;
/// album rows resolve to ALBUM/EP/SINGLE by the resolved track count (the
/// `QueueTrack` payload carries no release_type, so the track-count heuristic
/// — the same one favorites/labels use — applies).
fn resolve_from_tracks(
    item: &MixtapeCollectionItem,
    tracks: &[qbz_models::QueueTrack],
) -> ResolvedItem {
    let stored = source_str(item.source);
    let first = tracks.first();

    let source_kind = match first {
        Some(t) => t
            .source
            .clone()
            .unwrap_or_else(|| if t.is_local { "local".into() } else { "qobuz".into() }),
        None => stored.to_string(),
    };

    let quality_tier = match first {
        Some(t) => match t.bit_depth {
            Some(d) if d >= 24 => "hires",
            Some(_) => "cd",
            None if t.hires => "hires",
            None => "",
        },
        None => "",
    };
    let quality_detail = match (first, quality_tier.is_empty()) {
        (Some(t), false) => crate::quality::detail(t.bit_depth, t.sample_rate),
        _ => String::new(),
    };

    // Type label: albums resolve their release type from the resolved track
    // count; tracks/playlists keep their stored type. Uppercased to match the
    // column eyebrow.
    let type_label = match item.item_type {
        ItemType::Album => {
            crate::album_map::classify_release_type(Some(tracks.len() as u32)).to_uppercase()
        }
        other => type_label(other).to_string(),
    };

    // First resolved track's artwork — backfills rows whose stored
    // `artwork_url` was empty (disco-builder local items saved with NULL art).
    // Strip the `file://` prefix that `local_queue_track` adds: the source-aware
    // artwork dispatch reads a bare filesystem path (a raw `tokio::fs::read` of a
    // `file://…` URI fails). Plex `/library/...` thumbs and Qobuz CDN urls have
    // no prefix and pass through unchanged.
    let artwork_url = first
        .and_then(|t| t.artwork_url.clone())
        .map(|u| u.strip_prefix("file://").map(str::to_string).unwrap_or(u))
        .unwrap_or_default();

    // First resolved track's Qobuz artist id — empty for local/Plex tracks
    // (QueueTrack.artist_id is None there). Feeds the row's artist link.
    let artist_id = first
        .and_then(|t| t.artist_id)
        .map(|id| id.to_string())
        .unwrap_or_default();

    ResolvedItem {
        source_kind,
        quality_tier: quality_tier.to_string(),
        quality_detail,
        type_label,
        artwork_url,
        artist_id,
    }
}

/// B10 — OFFLINE: derive a cached Qobuz item's badge values from LOCAL sources
/// instead of the Qobuz API (which the offline gate refuses, leaving the badge
/// empty). The offline index (`index.db` `cached_tracks` rows, surfaced as
/// `CachedTrackInfo`) carries `quality` / `bit_depth` / `sample_rate` per
/// track; the tier/detail rule is the SAME one `resolve_from_tracks` and the
/// LocalLibrary "Offline"-source rows use (24-bit+ = Hi-Res, detail via
/// `crate::quality::detail` — the library.db `qobuz_download` rows derive their
/// badge from these very columns at sync time), so the offline badge matches
/// every other surface. Returns `None` for non-Qobuz items, Qobuz playlists
/// (membership is API-side), and uncached/non-ready items — the caller then
/// falls through to the normal resolver path.
async fn resolve_offline_cached(item: &MixtapeCollectionItem) -> Option<ResolvedItem> {
    use qbz_offline_cache::OfflineCacheStatus;

    if item.source != AlbumSource::Qobuz {
        return None;
    }
    let off = crate::offline::get().await?;
    let guard = off.db.lock().await;
    let db = guard.as_ref()?;

    let (info, ready_count) = match item.item_type {
        ItemType::Album => {
            let ready: Vec<qbz_offline_cache::CachedTrackInfo> = db
                .get_album_tracks(&item.source_item_id)
                .ok()?
                .into_iter()
                .filter(|cached| matches!(cached.status, OfflineCacheStatus::Ready))
                .collect();
            let count = ready.len();
            (ready.into_iter().next()?, count)
        }
        ItemType::Track => {
            let id = item.source_item_id.parse::<u64>().ok()?;
            let info = db.get_track(id).ok().flatten()?;
            if !matches!(info.status, OfflineCacheStatus::Ready) {
                return None;
            }
            (info, 1)
        }
        // Qobuz playlist membership lives in the API — not derivable locally.
        ItemType::Playlist => return None,
    };

    // Quality tier/detail — the resolve_from_tracks rule over the index
    // columns. The stored quality string ("UltraHiRes" etc.) stands in for the
    // missing hires flag when bit_depth is NULL.
    let quality_tier = match info.bit_depth {
        Some(d) if d >= 24 => "hires",
        Some(_) => "cd",
        None if info.quality.to_ascii_lowercase().contains("hires") => "hires",
        None => "",
    };
    let quality_detail = if quality_tier.is_empty() {
        String::new()
    } else {
        crate::quality::detail(info.bit_depth, info.sample_rate)
    };

    // Type label: albums classify by the stored track_count (the album's REAL
    // count, captured at add time); the cached READY count is the fallback for
    // older rows saved without one (a floor — partial caches undercount).
    let type_label = match item.item_type {
        ItemType::Album => {
            let count = item
                .track_count
                .filter(|n| *n > 0)
                .map(|n| n as u32)
                .unwrap_or(ready_count as u32);
            crate::album_map::classify_release_type(Some(count)).to_uppercase()
        }
        other => type_label(other).to_string(),
    };

    Some(ResolvedItem {
        source_kind: "qobuz".to_string(),
        quality_tier: quality_tier.to_string(),
        quality_detail,
        type_label,
        // No artwork backfill offline: the empty-stored-art backfill is a
        // local-item concern (those still resolve through the normal path),
        // and a Qobuz CDN url could not be fetched offline anyway.
        artwork_url: String::new(),
        // The offline index carries no artist id — and the Qobuz artist page
        // is gate-blocked offline anyway, so the link stays a no-op here.
        artist_id: String::new(),
    })
}

/// resolveItems pass (spec §17): resolve every rendered row's tracks via the
/// shared enqueue resolver (`myqbz_play::fetch_item_tracks` — the SAME
/// qobuz/local/plex backends), derive the row's source kind + album-level
/// quality + type label from the first resolved track, push the values into the
/// row, and cache them (keyed `source|source_item_id`) so a later filter/sort/
/// search re-derive re-hydrates instead of re-fetching. Spawned once after
/// `apply` (alongside the artwork jobs); already-cached rows are skipped, so a
/// re-derive is instant. Fire-and-forget: failures leave the stored-source
/// defaults in place.
///
/// OFFLINE (B10): a cached Qobuz item's badges resolve from the LOCAL offline
/// index via `resolve_offline_cached` — the API path would be gate-refused and
/// the badge would stay empty. Online the branch is never taken, so that path
/// is byte-identical.
pub fn resolve_items(
    runtime: std::sync::Arc<qbz_app::shell::AppRuntime<crate::adapter::SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
) {
    let Some(window) = weak.upgrade() else { return };

    // Snapshot the items needing resolution (every full item not already
    // cached). Sourced from FULL_ITEMS so the resolver gets the typed
    // item_type/source + numeric fields.
    let pending: Vec<MixtapeCollectionItem> = FULL_ITEMS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|it| {
                let key = inline_cache_key(source_str(it.source), &it.source_item_id);
                !RESOLVE_CACHE.with(|c| c.borrow().contains_key(&key))
            })
            .cloned()
            .collect()
    });
    drop(window);

    for full_item in pending {
        let runtime = runtime.clone();
        let weak = weak.clone();
        let image_cache = image_cache.clone();
        handle.spawn(async move {
            // B10 — OFFLINE: cached Qobuz items resolve their badges locally
            // (offline index); everything else (online, local/plex, uncached)
            // takes the existing resolver path unchanged.
            let offline_resolved = if crate::offline_mode::engine().is_offline() {
                resolve_offline_cached(&full_item).await
            } else {
                None
            };
            let resolved = match offline_resolved {
                Some(r) => r,
                None => {
                    let tracks =
                        crate::myqbz_play::fetch_item_tracks(&runtime, &full_item).await;
                    resolve_from_tracks(&full_item, &tracks)
                }
            };
            let source = source_str(full_item.source).to_string();
            let source_item_id = full_item.source_item_id.clone();
            let stored_artwork_empty = full_item
                .artwork_url
                .as_deref()
                .map(|u| u.is_empty())
                .unwrap_or(true);
            let _ = weak.upgrade_in_event_loop(move |w| {
                let key = inline_cache_key(&source, &source_item_id);
                RESOLVE_CACHE.with(|cell| {
                    cell.borrow_mut().insert(key, resolved.clone());
                });
                // Push the resolved values into the currently-rendered row (if
                // still present after any re-derive). Clear `quality_resolving`
                // (the skeleton) and backfill the row cover when the stored
                // `artwork_url` was empty (disco-builder local items, older
                // saves) so the disc placeholder is replaced by the real art —
                // the album-view pattern applied to the detail rows.
                let mut backfilled_pos: Option<i32> = None;
                with_row_by_source_id(&w, &source_item_id, |it| {
                    it.source_kind = resolved.source_kind.clone().into();
                    it.quality_tier = resolved.quality_tier.clone().into();
                    it.quality_detail = resolved.quality_detail.clone().into();
                    it.type_label = resolved.type_label.clone().into();
                    it.artist_id = resolved.artist_id.clone().into();
                    it.quality_resolving = false;
                    if it.artwork_url.is_empty() && !resolved.artwork_url.is_empty() {
                        it.artwork_url = resolved.artwork_url.clone().into();
                        backfilled_pos = Some(it.position);
                    }
                });
                // Dispatch the one backfilled cover through the source-aware
                // path (qobuz CDN -> HTTP; local/plex -> source-aware decode).
                // Only when the stored art was empty AND a row was actually
                // backfilled (skips the common already-had-art case).
                if stored_artwork_empty {
                    if let Some(pos) = backfilled_pos {
                        let job = ArtworkJob {
                            target: ArtworkTarget::MyQbzDetailRow { position: pos },
                            url: resolved.artwork_url.clone(),
                        };
                        let split = if resolved.source_kind == "qobuz" {
                            ArtworkJobSplit { remote: vec![job], ..Default::default() }
                        } else {
                            ArtworkJobSplit { local_or_plex: vec![job], ..Default::default() }
                        };
                        dispatch_artwork(split, w.as_weak(), image_cache.clone());
                    }
                }
            });
        });
    }
}

/// Clear the current selection (uncheck every row + zero the count), staying in
/// select-mode. Used after a bulk action completes. UI thread.
pub fn clear_selection(window: &AppWindow) {
    let state = window.global::<MyQbzDetailState>();
    let model = state.get_items();
    for i in 0..model.row_count() {
        if let Some(mut it) = model.row_data(i) {
            if it.selected {
                it.selected = false;
                model.set_row_data(i, it);
            }
        }
    }
    state.set_selected_count(0);
}

// ──────────────────────────── reset / apply ───────────────────────────

/// Clear the view to its loading state before a fresh load (so a re-open does
/// not flash the previous collection's hero + rows).
pub fn reset(window: &AppWindow) {
    FULL_ITEMS.with(|cell| cell.borrow_mut().clear());
    // Drop the inline-tracks cache — a different collection's tracks must not
    // leak into the freshly-opened one.
    INLINE_CACHE.with(|cell| cell.borrow_mut().clear());
    // Drop the resolveItems cache too (same reason — the resolved source/
    // quality/type of a different collection's items must not leak).
    RESOLVE_CACHE.with(|cell| cell.borrow_mut().clear());
    // Close the persist gate until `apply` restores this collection's prefs —
    // any toolbar setter that fires meanwhile must NOT overwrite stored prefs
    // with the in-flight defaults (mirrors Tauri's prefsHydrated).
    PREFS_HYDRATED.with(|c| c.set(false));
    let state = window.global::<MyQbzDetailState>();
    state.set_loading(true);
    state.set_found(true);
    state.set_items(ModelRc::new(VecModel::from(Vec::<MixtapeDetailItem>::new())));
    state.set_name("".into());
    state.set_description("".into());
    state.set_meta("".into());
    state.set_item_count(0);
    state.set_has_custom_cover(false);
    state.set_custom_cover(slint::Image::default());
    state.set_cover_count(0);
    state.set_selected_count(0);
    state.set_select_mode(false);
    // Toolbar -> defaults during load; `apply` then restores this collection's
    // persisted view-prefs (spec 12 §18) over these. Search + select-mode stay
    // transient (never persisted) so they always start fresh.
    state.set_search("".into());
    state.set_sort("position".into());
    state.set_sort_dir("asc".into());
    state.set_type_filter("all".into());
    state.set_src_qobuz(false);
    state.set_src_plex(false);
    state.set_src_local(false);
    state.set_view_mode("list".into());
    state.set_filter_count(0);
    state.set_has_any_filter(false);
}

/// Apply a freshly-loaded collection: header strings, hero mosaic, the full
/// item list (-> FULL_ITEMS), then render through the (reset) toolbar.
pub fn apply(window: &AppWindow, c: MixtapeCollection) {
    let state = window.global::<MyQbzDetailState>();
    let item_count = c.items.len();

    state.set_id(c.id.clone().into());
    state.set_kind(kind_str(c.kind).into());
    state.set_kind_label(kind_label(c.kind).into());
    state.set_name(c.name.clone().into());
    state.set_description(c.description.clone().unwrap_or_default().into());
    state.set_meta(album_count_label(item_count).into());
    state.set_item_count(item_count as i32);
    state.set_play_mode(play_mode_str(c.play_mode).into());
    state.set_found(true);

    // Custom cover (overrides the mosaic) — load the local file directly (it
    // lives in the artwork cache on disk; same as the playlist controller).
    let has_custom = c
        .custom_artwork_path
        .as_ref()
        .filter(|p| !p.is_empty())
        .filter(|p| std::path::Path::new(p).exists())
        .and_then(|p| slint::Image::load_from_path(std::path::Path::new(p)).ok());
    if let Some(img) = has_custom {
        state.set_has_custom_cover(true);
        state.set_custom_cover(img);
    } else {
        state.set_has_custom_cover(false);
        state.set_custom_cover(slint::Image::default());
    }

    apply_hero_mosaic(&state, &c);

    // Restore this collection's persisted view-prefs over the reset defaults
    // (spec 12 §18). `load` returns the §18 defaults when nothing is stored, so
    // a never-opened collection lands on list/position/asc/all/empty exactly as
    // before. Open the persist gate AFTER applying so the restore itself isn't
    // re-persisted (and so subsequent setter-driven persists are live).
    let prefs = crate::myqbz_view_prefs::load(c.id.as_str());
    state.set_view_mode(prefs.view_mode.into());
    state.set_sort(prefs.sort_by.into());
    state.set_sort_dir(prefs.sort_dir.into());
    state.set_type_filter(prefs.type_filter.into());
    state.set_src_qobuz(prefs.src_qobuz);
    state.set_src_plex(prefs.src_plex);
    state.set_src_local(prefs.src_local);
    PREFS_HYDRATED.with(|cell| cell.set(true));

    FULL_ITEMS.with(|cell| *cell.borrow_mut() = c.items);
    refresh_view(window);
    state.set_loading(false);
}

/// Mark the load as not-found (the id resolved to no collection).
pub fn apply_not_found(window: &AppWindow) {
    let state = window.global::<MyQbzDetailState>();
    state.set_loading(false);
    state.set_found(false);
}

// ──────────────────────────── artwork jobs ────────────────────────────

/// The artwork jobs for the loaded collection, SPLIT by source so each is
/// dispatched through the correct decoder (spec §17 fallback chain): Qobuz items
/// carry an HTTP CDN url → the Remote/HTTP path (`spawn_loads`); local/Plex
/// items carry a filesystem path or a Plex `/library/...` path → the
/// source-aware path (`spawn_local_or_plex_loads`). Mixing them (the old single
/// `spawn_loads`) broke local/Plex covers — a filesystem path was fetched as an
/// HTTP url and failed silently, leaving the row/hero cell blank.
#[derive(Default)]
pub struct ArtworkJobSplit {
    /// Qobuz CDN urls — HTTP fetch via the disk cache.
    pub remote: Vec<ArtworkJob>,
    /// Local filesystem paths + Plex thumb paths — source-aware decode.
    pub local_or_plex: Vec<ArtworkJob>,
}

/// Build the (remote, local/plex) artwork jobs for the loaded collection: the
/// up-to-9 hero-mosaic cells (only when no custom cover) + one thumbnail per
/// visible item row. Each job is routed to the `remote` bucket for Qobuz items
/// and the `local_or_plex` bucket otherwise.
pub fn artwork_jobs(window: &AppWindow) -> ArtworkJobSplit {
    let state = window.global::<MyQbzDetailState>();
    let mut split = ArtworkJobSplit::default();

    // Hero mosaic cells: classify each cell by the corresponding FULL_ITEMS
    // item's source (the cells map 1:1 to the first N items in original order).
    if !state.get_has_custom_cover() {
        let urls = [
            state.get_url1(),
            state.get_url2(),
            state.get_url3(),
            state.get_url4(),
            state.get_url5(),
            state.get_url6(),
            state.get_url7(),
            state.get_url8(),
            state.get_url9(),
        ];
        let cell_sources: Vec<AlbumSource> =
            FULL_ITEMS.with(|cell| cell.borrow().iter().map(|it| it.source).collect());
        for (slot, url) in urls.iter().enumerate() {
            if url.is_empty() {
                continue;
            }
            let job = ArtworkJob {
                target: ArtworkTarget::MyQbzDetailCover { slot },
                url: url.to_string(),
            };
            match cell_sources.get(slot) {
                Some(AlbumSource::Qobuz) => split.remote.push(job),
                _ => split.local_or_plex.push(job),
            }
        }
    }

    // Row thumbnails (the rendered model — matched back by position on apply).
    // Route by the row's resolved source-kind (qobuz -> remote; plex/local ->
    // source-aware). A not-yet-resolved row defaults to its stored kind.
    let model = state.get_items();
    for i in 0..model.row_count() {
        let Some(item) = model.row_data(i) else { continue };
        if item.artwork_url.is_empty() {
            continue;
        }
        let job = ArtworkJob {
            target: ArtworkTarget::MyQbzDetailRow { position: item.position },
            url: item.artwork_url.to_string(),
        };
        if item.source_kind == "qobuz" {
            split.remote.push(job);
        } else {
            split.local_or_plex.push(job);
        }
    }
    split
}

/// Dispatch a built `ArtworkJobSplit` through the correct decoders: Qobuz CDN
/// urls via the HTTP path (`spawn_loads`), local/Plex paths via the source-aware
/// path (`spawn_local_or_plex_loads`, threading the live Plex creds). The single
/// entry point both `navigate` (initial load) and the toolbar re-derive
/// (`refresh_row_covers`) use, so the source-split routing lives in ONE place.
pub fn dispatch_artwork(
    split: ArtworkJobSplit,
    weak: slint::Weak<AppWindow>,
    image_cache: ImageCache,
) {
    if !split.remote.is_empty() {
        artwork::spawn_loads(split.remote, weak.clone(), image_cache.clone());
    }
    if !split.local_or_plex.is_empty() {
        let plex = crate::plex_settings::get();
        artwork::spawn_local_or_plex_loads(
            split.local_or_plex,
            plex.base_url,
            plex.token,
            weak,
            image_cache,
        );
    }
}

/// Set a decoded row thumbnail by item position (the rendered model order may
/// differ from FULL_ITEMS after a sort, so match by the stable position).
pub fn set_row_artwork(window: &AppWindow, position: i32, image: slint::Image) {
    let model = window.global::<MyQbzDetailState>().get_items();
    for i in 0..model.row_count() {
        if let Some(mut it) = model.row_data(i) {
            if it.position == position {
                it.artwork = image;
                model.set_row_data(i, it);
                break;
            }
        }
    }
}

/// Set a decoded hero-mosaic cover by slot (0-8).
pub fn set_hero_cover(window: &AppWindow, slot: usize, image: slint::Image) {
    let state = window.global::<MyQbzDetailState>();
    match slot {
        0 => state.set_cover1(image),
        1 => state.set_cover2(image),
        2 => state.set_cover3(image),
        3 => state.set_cover4(image),
        4 => state.set_cover5(image),
        5 => state.set_cover6(image),
        6 => state.set_cover7(image),
        7 => state.set_cover8(image),
        8 => state.set_cover9(image),
        _ => {}
    }
}

// ──────────────────────────── navigation ──────────────────────────────

/// Open the collection-detail view for `id`: switch the ContentView + loading
/// state immediately, fetch the collection on a blocking worker, then apply +
/// render + spawn (source-split) artwork + the resolveItems pass. Mirrors
/// `myqbz::navigate` (load/apply/render) and the album/playlist detail
/// navigators. The `runtime` drives the resolveItems backend resolution
/// (quality / source-kind / type per item).
pub fn navigate(
    runtime: std::sync::Arc<qbz_app::shell::AppRuntime<crate::adapter::SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    id: String,
) {
    handle.clone().spawn(async move {
        {
            let weak = weak.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                reset(&w);
                w.global::<NavState>().set_view(ContentView::MixtapeDetail);
            });
        }

        let fetch_id = id.clone();
        let collection =
            tokio::task::spawn_blocking(move || get_collection(&fetch_id)).await.ok().flatten();

        // D11.c — OFFLINE: drop the items failing the availability rule
        // (qobuz not-cached / grace-expired; plex under real offline) before
        // the rows render. Online: untouched.
        let collection = match collection {
            Some(mut c) if crate::offline_mode::engine().is_offline() => {
                let items: Vec<&qbz_models::mixtape::MixtapeCollectionItem> =
                    c.items.iter().collect();
                let avail = crate::myqbz::offline_availability(&items).await;
                drop(items);
                let before = c.items.len();
                c.items.retain(|it| avail.item_available(it));
                if c.items.len() < before {
                    log::info!(
                        "[qbz-slint] myqbz_detail {}: {} item(s) unavailable offline, hidden (D11)",
                        c.id,
                        before - c.items.len()
                    );
                }
                Some(c)
            }
            other => other,
        };

        let resolve_handle = handle.clone();
        let _ = weak.upgrade_in_event_loop(move |w| match collection {
            Some(c) => {
                apply(&w, c);
                let split = artwork_jobs(&w);
                dispatch_artwork(split, w.as_weak(), image_cache.clone());
                // resolveItems (spec §17): resolve each item's quality / source
                // kind / type from the backends and hydrate the rows (also
                // backfills + dispatches covers for rows stored with empty art).
                resolve_items(runtime, w.as_weak(), resolve_handle, image_cache.clone());
            }
            None => {
                log::warn!("[qbz-slint] myqbz_detail navigate({id}): collection not found");
                apply_not_found(&w);
            }
        });
    });
}
