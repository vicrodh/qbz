//! My QBZ controller — the Mixtapes & Collections index grids (read-only in
//! this slice). Mirrors `crate::playlist_manager`: it loads `MixtapeCollection`
//! rows from the per-user `library.db` via `qbz_mixtape::repo::list_collections`
//! (called through `crate::library_db::with_db` + `with_connection`), precomputes
//! every display string (eyebrow label, "N albums" ICU plural, pre-downscaled
//! mosaic cover URLs) and pushes ready-to-render `MixtapeCardItem`s into
//! `MyQbzState`. The views do NO per-row lookups.
//!
//! READ-ONLY SCOPE (Phase-2 Slice 2): create-new + open-detail are wired as
//! logging STUBS (`open_card` / `create_*`). The sidebar nav routes here and
//! loads real data; that is the testable path for this slice.
//!
//! The backend (`qbz-mixtape`) is reused directly — no Tauri command wrappers
//! (ADR-005), headless (ADR-006). The repo hydrates each collection's items so
//! counts + mosaic artwork are accurate (`repo.rs` `list_collections`).

use std::sync::{LazyLock, Mutex};

use qbz_models::mixtape::{CollectionKind, MixtapeCollection};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::artwork::{self, ArtworkJob, ArtworkTarget, ImageCache};
use crate::{AppWindow, ContentView, MixtapeCardItem, MyQbzState, NavState};

/// Last-loaded data per kind-group (so toolbar changes rebuild from cache,
/// no DB refetch). Mirrors `playlist_manager::CACHE`.
static MIXTAPES_CACHE: LazyLock<Mutex<Vec<MixtapeCollection>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));
static COLLECTIONS_CACHE: LazyLock<Mutex<Vec<MixtapeCollection>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Which grid a navigation targets.
#[derive(Clone, Copy, PartialEq)]
pub enum Grid {
    Mixtapes,
    Collections,
}

// ──────────────────────────── DB read path ────────────────────────────

/// List collections of the given kind filter from the per-user library.db.
/// `None` returns all kinds. Items come hydrated (the repo loads them) so
/// counts + mosaic artwork are accurate. Returns an empty Vec when the DB is
/// unavailable (logged by `with_db`).
pub fn list_collections(kind: Option<CollectionKind>) -> Vec<MixtapeCollection> {
    crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| {
            qbz_mixtape::repo::list_collections(conn, kind).unwrap_or_else(|e| {
                log::warn!("[qbz-slint] myqbz list_collections failed: {e}");
                Vec::new()
            })
        }))
    })
    .unwrap_or_default()
}

// ──────────────────────────── string helpers ──────────────────────────

fn kind_str(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Mixtape => "mixtape",
        CollectionKind::Collection => "collection",
        CollectionKind::ArtistCollection => "artist_collection",
    }
}

/// Eyebrow label, uppercase (Tauri `labelFor` / `mixtapes.label`).
fn label_for(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Mixtape => "MIXTAPE",
        CollectionKind::Collection => "COLLECTION",
        CollectionKind::ArtistCollection => "ARTIST",
    }
}

/// `mixtapes.albumCount` ICU plural — "1 album" / "N albums". Always "album(s)"
/// regardless of item_type (1:1 with the PSD).
fn album_count_label(count: usize) -> String {
    if count == 1 {
        "1 album".to_string()
    } else {
        format!("{count} albums")
    }
}

/// Pre-downscale a Qobuz cover URL to a per-cell target size, mirroring the
/// mosaic's `smallQobuzUrl` (regex-swap `_<old>.jpg` → `_<target>.jpg`). Used
/// before handing URLs to the image loader so we never pull 600px covers for
/// ~60-92px cells. Non-Qobuz URLs (local/plex) pass through unchanged.
fn small_qobuz_url(url: &str, target: u32) -> String {
    if url.is_empty() {
        return String::new();
    }
    // Lowercase scan for the size token; rewrite in place keeping original case
    // of the rest. Old tokens: 50|100|150|230|300|600|max|org.
    const TOKENS: [&str; 8] = ["_50.jpg", "_100.jpg", "_150.jpg", "_230.jpg", "_300.jpg", "_600.jpg", "_max.jpg", "_org.jpg"];
    let lower = url.to_lowercase();
    for tok in TOKENS {
        if let Some(pos) = lower.rfind(tok) {
            let mut out = String::with_capacity(url.len());
            out.push_str(&url[..pos]);
            out.push_str(&format!("_{target}.jpg"));
            out.push_str(&url[pos + tok.len()..]);
            return out;
        }
    }
    url.to_string()
}

/// Per-cell target size given the mosaic `size` and column count
/// (`cellPx = round(size/cols)`; `<=80 → 50`, `<=200 → 150`, else 300). The
/// grid card mosaic is 184px (2x2 → cell 92 → 150; 3x3 → cell ~61 → 50).
fn cell_target(size: u32, cols: u32) -> u32 {
    let cell_px = ((size as f32) / (cols as f32)).round() as u32;
    if cell_px <= 80 {
        50
    } else if cell_px <= 200 {
        150
    } else {
        300
    }
}

// ──────────────────────────── model builders ──────────────────────────

/// Build one ready-to-render card. Decides cover-count (0 / 4 / 9) per the
/// 2x2-vs-3x3 rule, and pre-downscales the up-to-9 cover URLs per cell.
fn card_item(c: &MixtapeCollection) -> MixtapeCardItem {
    let item_count = c.items.len();
    let has_custom = c.custom_artwork_path.is_some();

    // cols: 3x3 only for a Collection with >= 9 items; else 2x2.
    let cols: u32 = if c.kind == CollectionKind::Collection && item_count >= 9 {
        3
    } else {
        2
    };
    let cell_count = (cols * cols) as usize;
    // cover-count is the number of mosaic cells actually used (0 when empty or
    // when a custom cover full-bleeds; the view checks has-custom-cover first).
    let cover_count = if has_custom || item_count == 0 {
        0
    } else {
        cell_count
    };

    // Grid-card mosaic renders at 184px; size the downscale to that.
    let target = cell_target(184, cols);

    // Up-to-9 cell URLs: the first `cell_count` items' artwork, padded "".
    let url = |i: usize| -> slint::SharedString {
        if has_custom || item_count == 0 || i >= cell_count {
            return slint::SharedString::default();
        }
        match c.items.get(i).and_then(|it| it.artwork_url.as_deref()) {
            Some(u) if !u.is_empty() => small_qobuz_url(u, target).into(),
            _ => slint::SharedString::default(),
        }
    };

    let custom_image = slint::Image::default();

    MixtapeCardItem {
        id: c.id.clone().into(),
        name: c.name.clone().into(),
        kind: kind_str(c.kind).into(),
        label: label_for(c.kind).into(),
        meta: album_count_label(item_count).into(),
        item_count: item_count as i32,
        play_count: c.play_count,
        updated_at: c.updated_at as i32,
        custom_cover: custom_image,
        has_custom_cover: has_custom,
        cover_count: cover_count as i32,
        url1: url(0),
        url2: url(1),
        url3: url(2),
        url4: url(3),
        url5: url(4),
        url6: url(5),
        url7: url(6),
        url8: url(7),
        url9: url(8),
        cover1: slint::Image::default(),
        cover2: slint::Image::default(),
        cover3: slint::Image::default(),
        cover4: slint::Image::default(),
        cover5: slint::Image::default(),
        cover6: slint::Image::default(),
        cover7: slint::Image::default(),
        cover8: slint::Image::default(),
        cover9: slint::Image::default(),
    }
}

/// Set a decoded mosaic cover onto a card item by slot (0-8). Called from the
/// artwork apply arm.
pub fn set_mosaic_cover(item: &mut MixtapeCardItem, slot: usize, image: slint::Image) {
    match slot {
        0 => item.cover1 = image,
        1 => item.cover2 = image,
        2 => item.cover3 = image,
        3 => item.cover4 = image,
        4 => item.cover5 = image,
        5 => item.cover6 = image,
        6 => item.cover7 = image,
        7 => item.cover8 = image,
        8 => item.cover9 = image,
        _ => {}
    }
}

// ──────────────────────────── sort / filter ───────────────────────────

/// Sort a collection list by the active toolbar sort (mirrors `visibleX`):
/// name (locale-ish), items (count), updated (updated_at), position (default).
/// `dir` = "asc"/"desc".
fn sort_collections(list: &mut [MixtapeCollection], sort: &str, dir: &str) {
    let desc = dir == "desc";
    match sort {
        "name" => list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        "items" => list.sort_by(|a, b| a.items.len().cmp(&b.items.len())),
        "updated" => list.sort_by(|a, b| a.updated_at.cmp(&b.updated_at)),
        // default "position"
        _ => list.sort_by(|a, b| a.position.cmp(&b.position)),
    }
    if desc {
        list.reverse();
    }
}

/// Whether a collection passes the search filter (name OR description,
/// case-insensitive substring). Empty query = pass.
fn passes_search(c: &MixtapeCollection, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    if c.name.to_lowercase().contains(query) {
        return true;
    }
    c.description
        .as_deref()
        .map(|d| d.to_lowercase().contains(query))
        .unwrap_or(false)
}

// ──────────────────────────── apply / rebuild ─────────────────────────

pub fn set_loading(window: &AppWindow, loading: bool) {
    window.global::<MyQbzState>().set_loading(loading);
}

/// Store freshly-loaded rows for `grid` and render them through the active
/// toolbar state.
pub fn apply(window: &AppWindow, grid: Grid, rows: Vec<MixtapeCollection>) {
    match grid {
        Grid::Mixtapes => {
            if let Ok(mut c) = MIXTAPES_CACHE.lock() {
                *c = rows;
            }
        }
        Grid::Collections => {
            if let Ok(mut c) = COLLECTIONS_CACHE.lock() {
                *c = rows;
            }
        }
    }
    rebuild(window, grid);
}

/// Rebuild the visible card model for one grid from its cache, honoring the
/// active toolbar state (search / sort / kind-filter). UI thread only.
pub fn rebuild(window: &AppWindow, grid: Grid) {
    let state = window.global::<MyQbzState>();
    match grid {
        Grid::Mixtapes => {
            let data = MIXTAPES_CACHE.lock().map(|c| c.clone()).unwrap_or_default();
            let query = state.get_mix_search().trim().to_lowercase();
            let sort = state.get_mix_sort().to_string();
            let dir = state.get_mix_sort_dir().to_string();
            let mut filtered: Vec<MixtapeCollection> =
                data.into_iter().filter(|c| passes_search(c, &query)).collect();
            sort_collections(&mut filtered, &sort, &dir);
            let items: Vec<MixtapeCardItem> = filtered.iter().map(card_item).collect();
            state.set_mixtapes(ModelRc::new(VecModel::from(items)));
        }
        Grid::Collections => {
            let data = COLLECTIONS_CACHE.lock().map(|c| c.clone()).unwrap_or_default();
            let query = state.get_col_search().trim().to_lowercase();
            let sort = state.get_col_sort().to_string();
            let dir = state.get_col_sort_dir().to_string();
            let kind_filter = state.get_col_kind_filter().to_string();
            let mut filtered: Vec<MixtapeCollection> = data
                .into_iter()
                .filter(|c| match kind_filter.as_str() {
                    "collection" => c.kind == CollectionKind::Collection,
                    "artist_collection" => c.kind == CollectionKind::ArtistCollection,
                    _ => true,
                })
                .filter(|c| passes_search(c, &query))
                .collect();
            sort_collections(&mut filtered, &sort, &dir);
            let items: Vec<MixtapeCardItem> = filtered.iter().map(card_item).collect();
            state.set_collections(ModelRc::new(VecModel::from(items)));
        }
    }
    state.set_loading(false);
}

/// Re-clicking the active sort field flips direction; a new field resets to
/// asc. Mirrors `selectSort`.
pub fn set_sort(window: &AppWindow, grid: Grid, field: &str) {
    let state = window.global::<MyQbzState>();
    let (cur_sort, cur_dir) = match grid {
        Grid::Mixtapes => (state.get_mix_sort().to_string(), state.get_mix_sort_dir().to_string()),
        Grid::Collections => (state.get_col_sort().to_string(), state.get_col_sort_dir().to_string()),
    };
    let new_dir = if cur_sort == field {
        if cur_dir == "asc" { "desc" } else { "asc" }
    } else {
        "asc"
    };
    match grid {
        Grid::Mixtapes => {
            state.set_mix_sort(field.into());
            state.set_mix_sort_dir(new_dir.into());
        }
        Grid::Collections => {
            state.set_col_sort(field.into());
            state.set_col_sort_dir(new_dir.into());
        }
    }
    rebuild(window, grid);
}

/// Reset toolbar filters/sort (search too, like Tauri's `resetFilters`).
pub fn reset(window: &AppWindow, grid: Grid) {
    let state = window.global::<MyQbzState>();
    match grid {
        Grid::Mixtapes => {
            state.set_mix_sort("position".into());
            state.set_mix_sort_dir("asc".into());
            state.set_mix_search("".into());
        }
        Grid::Collections => {
            state.set_col_sort("position".into());
            state.set_col_sort_dir("asc".into());
            state.set_col_kind_filter("all".into());
            state.set_col_search("".into());
        }
    }
    rebuild(window, grid);
}

// ──────────────────────────── artwork jobs ────────────────────────────

/// Build mosaic-cover artwork jobs for every visible card of `grid`.
pub fn artwork_jobs(window: &AppWindow, grid: Grid) -> Vec<ArtworkJob> {
    let state = window.global::<MyQbzState>();
    let model = match grid {
        Grid::Mixtapes => state.get_mixtapes(),
        Grid::Collections => state.get_collections(),
    };
    let mut jobs = Vec::new();
    for index in 0..model.row_count() {
        let Some(card) = model.row_data(index) else { continue };
        let urls = [
            card.url1, card.url2, card.url3, card.url4, card.url5, card.url6, card.url7,
            card.url8, card.url9,
        ];
        for (slot, url) in urls.iter().enumerate() {
            if url.is_empty() {
                continue;
            }
            let target = match grid {
                Grid::Mixtapes => ArtworkTarget::MyQbzMixtapeCover { index, slot },
                Grid::Collections => ArtworkTarget::MyQbzCollectionCover { index, slot },
            };
            jobs.push(ArtworkJob {
                target,
                url: url.to_string(),
            });
        }
    }
    jobs
}

// ──────────────────────────── navigation ──────────────────────────────

/// Open a My QBZ grid (Mixtapes or Collections) and load its rows from the
/// per-user library.db on a blocking worker, then render + spawn mosaic
/// artwork. `kind` selects the grid (Mixtape → Mixtapes; Collection → the
/// Collections grid, which displays collection + artist_collection).
pub fn navigate(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    kind: CollectionKind,
) {
    let grid = match kind {
        CollectionKind::Mixtape => Grid::Mixtapes,
        _ => Grid::Collections,
    };
    let view = match grid {
        Grid::Mixtapes => ContentView::Mixtapes,
        Grid::Collections => ContentView::Collections,
    };

    handle.clone().spawn(async move {
        let _ = weak.upgrade_in_event_loop(move |w| {
            set_loading(&w, true);
            w.global::<NavState>().set_view(view);
        });

        // The Mixtapes grid wants kind == mixtape; the Collections grid loads
        // ALL kinds and filters locally (collection | artist_collection) so the
        // kind-filter dropdown can switch between them without a refetch.
        let kind_arg = match grid {
            Grid::Mixtapes => Some(CollectionKind::Mixtape),
            Grid::Collections => None,
        };
        let rows = tokio::task::spawn_blocking(move || list_collections(kind_arg))
            .await
            .unwrap_or_default();

        // For the Collections grid, drop mixtapes (load returned all kinds).
        let rows: Vec<MixtapeCollection> = match grid {
            Grid::Mixtapes => rows,
            Grid::Collections => rows
                .into_iter()
                .filter(|c| c.kind != CollectionKind::Mixtape)
                .collect(),
        };

        let _ = weak.upgrade_in_event_loop(move |w| {
            apply(&w, grid, rows);
            let jobs = artwork_jobs(&w, grid);
            artwork::spawn_loads(jobs, w.as_weak(), image_cache.clone());
        });
    });
}
