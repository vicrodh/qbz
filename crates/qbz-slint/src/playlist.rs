//! Playlist detail view controller.
//!
//! Fetches a playlist through `QbzCore`, maps it to the shared
//! TrackItem rows + header metadata, and applies it to `PlaylistState`.
//! Mirrors `mix.rs`: a cached track list backs play-all / per-track
//! play, and an artwork-jobs pass resolves the row covers + header
//! cover off-thread.

use std::sync::{LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::Track;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AppWindow, PlaylistState, TrackItem};

/// The currently-loaded playlist tracks, for play-all / per-track play.
static CURRENT: LazyLock<Mutex<Vec<Track>>> = LazyLock::new(|| Mutex::new(Vec::new()));

thread_local! {
    /// The full, original-order row list — the canonical source the
    /// search + sort derive the visible list from. UI thread only.
    static FULL_ITEMS: std::cell::RefCell<Vec<TrackItem>> = std::cell::RefCell::new(Vec::new());
    /// Active in-page search query.
    static QUERY: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    /// Active sort: (field, ascending). field "default" = playlist order.
    static SORT: std::cell::RefCell<(String, bool)> =
        std::cell::RefCell::new(("default".to_string(), true));
    /// Custom order positions: track id -> position. Empty until the
    /// custom sort is entered (loaded/initialized from library.db).
    static CUSTOM_ORDER: std::cell::RefCell<std::collections::HashMap<u64, i32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
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
        // Order by the local custom positions; tracks not yet in the
        // map fall back to their natural index (newly-added tracks).
        let order = CUSTOM_ORDER.with(|c| c.borrow().clone());
        view.sort_by_key(|t| {
            let id = t.id.parse::<u64>().unwrap_or(0);
            order.get(&id).copied().unwrap_or(i32::MAX)
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

/// The current full track ids in natural (playlist) order — used to
/// seed the custom order on first entry.
pub fn current_track_ids() -> Vec<u64> {
    FULL_ITEMS.with(|cell| {
        cell.borrow()
            .iter()
            .filter_map(|t| t.id.parse::<u64>().ok())
            .collect()
    })
}

/// Load the playlist's custom order from library.db, seeding it from
/// `current_ids` if none exists. Blocking — run on a worker thread.
pub fn load_or_init_custom(playlist_id: u64, current_ids: Vec<u64>) -> Vec<(u64, i32)> {
    crate::library_db::with_db(|db| {
        let has = db.has_playlist_custom_order(playlist_id)?;
        if !has {
            let seed: Vec<(i64, bool)> = current_ids.iter().map(|&id| (id as i64, false)).collect();
            db.init_playlist_custom_order(playlist_id, &seed)?;
        }
        db.get_playlist_custom_order(playlist_id)
    })
    .unwrap_or_default()
    .into_iter()
    .map(|(id, _is_local, pos)| (id as u64, pos))
    .collect()
}

/// Persist the full custom order (DELETE + INSERT — self-healing).
/// Blocking.
pub fn persist_custom(playlist_id: u64, orders: Vec<(u64, i32)>) {
    let rows: Vec<(i64, bool, i32)> =
        orders.into_iter().map(|(id, pos)| (id as i64, false, pos)).collect();
    crate::library_db::with_db(|db| db.set_playlist_custom_order(playlist_id, &rows));
}

/// Store a freshly-loaded custom order + re-render. UI thread.
pub fn apply_custom_order(window: &AppWindow, orders: Vec<(u64, i32)>) {
    CUSTOM_ORDER.with(|c| {
        let mut m = c.borrow_mut();
        m.clear();
        for (id, pos) in orders {
            m.insert(id, pos);
        }
    });
    refresh_view(window);
}

/// Move a track one slot up/down in the custom order. Rebuilds the
/// whole order with clean 0..N-1 positions (self-healing), re-renders,
/// and returns the new orders to persist. UI thread.
pub fn move_track(window: &AppWindow, track_id: &str, up: bool) -> Vec<(u64, i32)> {
    let Ok(target) = track_id.parse::<u64>() else {
        return Vec::new();
    };
    // Current visible custom order of ids.
    let order = CUSTOM_ORDER.with(|c| c.borrow().clone());
    let mut ids: Vec<u64> = FULL_ITEMS.with(|cell| {
        cell.borrow()
            .iter()
            .filter_map(|t| t.id.parse::<u64>().ok())
            .collect()
    });
    ids.sort_by_key(|id| order.get(id).copied().unwrap_or(i32::MAX));
    let Some(idx) = ids.iter().position(|&id| id == target) else {
        return Vec::new();
    };
    let swap = if up {
        if idx == 0 {
            return Vec::new();
        }
        idx - 1
    } else {
        if idx + 1 >= ids.len() {
            return Vec::new();
        }
        idx + 1
    };
    ids.swap(idx, swap);
    // Rebuild contiguous positions.
    let orders: Vec<(u64, i32)> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i as i32))
        .collect();
    CUSTOM_ORDER.with(|c| {
        let mut m = c.borrow_mut();
        m.clear();
        for &(id, pos) in &orders {
            m.insert(id, pos);
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
    pub tracks: Vec<Track>,
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
    Some(PlaylistData {
        id: pl.id.to_string(),
        name: pl.name,
        owner: pl.owner.name,
        description: description.clone(),
        description_short: truncate_words(&description, 160),
        cover_url,
        custom_artwork_path,
        tracks,
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

fn total_duration(tracks: &[Track]) -> String {
    let secs: u64 = tracks.iter().map(|t| t.duration as u64).sum();
    let mins = secs / 60;
    if mins >= 60 {
        format!("{} h {} min", mins / 60, mins % 60)
    } else {
        format!("{} min", mins)
    }
}

fn to_item(track: &Track) -> TrackItem {
    let mut title = track.title.clone();
    if let Some(v) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({v})");
    }
    TrackItem {
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
    }
}

pub fn reset(window: &AppWindow) {
    FULL_ITEMS.with(|cell| cell.borrow_mut().clear());
    QUERY.with(|q| q.borrow_mut().clear());
    SORT.with(|s| *s.borrow_mut() = ("default".to_string(), true));
    CUSTOM_ORDER.with(|c| c.borrow_mut().clear());
    let state = window.global::<PlaylistState>();
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_track_count(0);
    state.set_total_duration("".into());
    state.set_cover(slint::Image::default());
    state.set_sort_field("default".into());
    state.set_sort_asc(true);
    state.set_loading(true);
}

pub fn apply(window: &AppWindow, data: PlaylistData) {
    let items: Vec<TrackItem> = data.tracks.iter().map(to_item).collect();
    let count = data.tracks.len() as i32;
    let duration = total_duration(&data.tracks);
    FULL_ITEMS.with(|cell| *cell.borrow_mut() = items.clone());
    if let Ok(mut cur) = CURRENT.lock() {
        *cur = data.tracks;
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

/// Artwork jobs for the loaded playlist — one per row plus the header
/// cover (resolved into PlaylistState.cover).
pub fn artwork_jobs(data: &PlaylistData) -> Vec<ArtworkJob> {
    let mut jobs: Vec<ArtworkJob> = data
        .tracks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.album
                .as_ref()
                .and_then(|a| a.image.smallest().cloned())
                .map(|url| ArtworkJob {
                    url,
                    target: ArtworkTarget::PlaylistTrack { index: i },
                })
        })
        .collect();
    // Skip the server-cover job when a local custom artwork is set
    // (it's already loaded in apply and cover_url holds a file path).
    if data.custom_artwork_path.is_none() && !data.cover_url.is_empty() {
        jobs.push(ArtworkJob {
            url: data.cover_url.clone(),
            target: ArtworkTarget::PlaylistCover,
        });
    }
    jobs
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

pub fn index_of(track_id: &str) -> usize {
    CURRENT
        .lock()
        .ok()
        .and_then(|c| c.iter().position(|t| t.id.to_string() == track_id))
        .unwrap_or(0)
}

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
    let state = window.global::<PlaylistState>();
    if !on {
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
    state.set_multi_select_mode(on);
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

/// The ids of the currently-selected rows.
pub fn selected_ids(window: &AppWindow) -> Vec<u64> {
    let model = window.global::<PlaylistState>().get_tracks();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|t| t.selected)
        .filter_map(|t| t.id.parse::<u64>().ok())
        .collect()
}
