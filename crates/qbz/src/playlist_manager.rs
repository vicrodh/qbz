//! Playlist Manager controller — the full playlist + folder organization
//! surface (Tauri's `PlaylistManagerView`). Local organization layer over
//! the user's Qobuz playlists: folders (icon + color + hidden), per-playlist
//! favorite / hidden / custom-order / folder membership, search / filter /
//! sort, and three view modes (grid / list / tree).
//!
//! The backend is 100% reusable: playlists come from
//! `QbzCore::get_user_playlists`, and folders / settings / stats / local
//! counts come from the per-user `library.db` via `crate::folders` (the
//! same data the Tauri `v2_*` commands back). All DB ops are blocking, so
//! the loader runs them on `spawn_blocking`.
//!
//! Merged row structs are precomputed in Rust (Send) and pushed as
//! ready-to-render Slint models — the view does NO per-row map lookups.
//! Toolbar state (filter / sort / view / folder-mode) is session-scoped,
//! mirrored in this module's statics so rebuilds don't re-hit the network.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::artwork::{self, ArtworkJob, ArtworkTarget, ImageCache};
use crate::folders::FolderFull;
use crate::{
    AppWindow, ContentView, NavState, PlaylistManagerState, PmFolderItem, PmPlaylistItem, PmTreeRow,
};

/// Send view of one playlist merged with its local settings + stats.
#[derive(Clone)]
struct PmPlaylist {
    id: u64,
    name: String,
    /// Remote (Qobuz) track count.
    tracks_count: u32,
    /// Total playlist duration in seconds (Qobuz `duration`).
    duration: u32,
    /// Local (non-Qobuz) track count.
    local_count: u32,
    play_count: u32,
    is_favorite: bool,
    is_hidden: bool,
    folder_id: Option<String>,
    position: i32,
    /// Up to four de-duplicated cover URLs (same scheme as the sidebar).
    cover_urls: Vec<String>,
    /// B8: >= 1 snapshot track playable offline (snapshot ∩ cached,
    /// grace-gated). Extends the D11.b offline filter; false while online.
    offline_available: bool,
}

impl PmPlaylist {
    fn total_count(&self) -> u32 {
        self.tracks_count + self.local_count
    }
}

/// Send view of one LOCAL playlist (library.db entity, id `local:<uuid>`).
/// Listed alongside the Qobuz set with a hard-drive marker. Favorite /
/// hidden live on the `local_playlists` row itself (B3) and participate in
/// the manager's filter + card actions; folder membership stays a
/// Qobuz-side concept (the folder tables are u64-keyed) and doesn't apply.
#[derive(Clone)]
struct PmLocalPlaylist {
    id: String,
    name: String,
    offline_only: bool,
    track_count: u32,
    is_favorite: bool,
    is_hidden: bool,
}

#[derive(Clone, Default)]
pub struct PmData {
    playlists: Vec<PmPlaylist>,
    folders: Vec<FolderFull>,
    locals: Vec<PmLocalPlaylist>,
}

/// Last-loaded data (so toolbar changes rebuild from cache, no refetch).
static CACHE: LazyLock<Mutex<PmData>> = LazyLock::new(|| Mutex::new(PmData::default()));
/// Session folder-expand state for the tree view (Tauri: not persisted).
static EXPANDED: LazyLock<Mutex<std::collections::HashSet<String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));
/// True once the tree has auto-expanded folders on first open.
static TREE_INIT: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

/// Pick up to four de-duplicated cover URLs (images150 > images300 >
/// images), mirroring `crate::sidebar::playlist_cover_urls`.
fn cover_urls(p: &qbz_models::Playlist) -> Vec<String> {
    let source = [&p.images300, &p.images150, &p.images]
        .into_iter()
        .flatten()
        .find(|v| !v.is_empty());
    let mut out: Vec<String> = Vec::new();
    if let Some(list) = source {
        for url in list {
            if !url.is_empty() && !out.contains(url) {
                out.push(url.clone());
            }
            if out.len() == 4 {
                break;
            }
        }
    }
    out
}

/// Fetch playlists (Qobuz) + folders + settings + stats + local counts
/// (local, library.db) and merge into the Send `PmData`.
pub async fn load<A>(runtime: &AppRuntime<A>) -> PmData
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let remote = runtime.core().get_user_playlists().await.unwrap_or_else(|e| {
        log::warn!("[qbz-slint] playlist-manager playlists load failed: {e}");
        Vec::new()
    });
    // B7 producer (names): persist id+name(+owner, track_count) for ALL
    // listed playlists — data this load already fetched, written detached.
    // Offline the fetch is gate-refused (empty), so nothing is written.
    crate::playlist_snapshot::record_names_detached(
        remote
            .iter()
            .map(|p| crate::playlist_snapshot::SnapshotNameEntry {
                qobuz_playlist_id: p.id,
                name: p.name.clone(),
                owner: Some(p.owner.name.clone()).filter(|o| !o.is_empty()),
                track_count: Some(p.tracks_count),
            })
            .collect(),
    );

    let (folders, settings, play_counts, local_counts, locals, snapshot_names, snapshot_available) =
        tokio::task::spawn_blocking(|| {
            let locals: Vec<PmLocalPlaylist> = crate::local_playlist::list_blocking()
                .into_iter()
                .map(|p| PmLocalPlaylist {
                    id: p.id,
                    name: p.name,
                    offline_only: p.offline_only,
                    track_count: p.track_count,
                    is_favorite: p.favorite,
                    is_hidden: p.hidden,
                })
                .collect();
            // B7/B8 (offline only): snapshot names for the synthesized
            // entries + the snapshot-available visibility set.
            let (snapshot_names, snapshot_available) =
                if crate::offline_mode::engine().is_offline() {
                    (
                        crate::playlist_snapshot::headers_blocking(),
                        crate::playlist_snapshot::available_offline_blocking(),
                    )
                } else {
                    (HashMap::new(), std::collections::HashSet::new())
                };
            (
                crate::folders::load_folders_full(),
                crate::folders::playlist_settings_map(),
                crate::folders::playlist_play_counts(),
                crate::folders::playlist_local_counts(),
                locals,
                snapshot_names,
                snapshot_available,
            )
        })
        .await
        .unwrap_or_else(|_| {
            (
                Vec::new(),
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
                Vec::new(),
                HashMap::new(),
                std::collections::HashSet::new(),
            )
        });

    let folder_ids: std::collections::HashSet<&String> = folders.iter().map(|f| &f.id).collect();

    let mut playlists: Vec<PmPlaylist> = remote
        .iter()
        .map(|p| {
            let s = settings.get(&p.id).cloned().unwrap_or_default();
            // A folder that no longer exists falls back to root (matches the
            // sidebar's `folder_ids.contains` guard).
            let folder_id = s
                .folder_id
                .filter(|fid| folder_ids.contains(fid));
            PmPlaylist {
                id: p.id,
                name: p.name.clone(),
                tracks_count: p.tracks_count,
                duration: p.duration,
                local_count: local_counts.get(&p.id).copied().unwrap_or(0),
                play_count: play_counts.get(&p.id).copied().unwrap_or(0),
                is_favorite: s.is_favorite,
                is_hidden: s.hidden,
                folder_id,
                position: s.position,
                cover_urls: cover_urls(p),
                offline_available: snapshot_available.contains(&p.id),
            }
        })
        .collect();

    // Surface INTERNAL favorites (hearted playlists) that are neither owned nor
    // subscribed — they don't come back from get_user_playlists, so without this
    // a favorited-but-not-followed Qobuz playlist would be invisible here.
    // Online only (offline can't fetch them); mirrors Favorites>Playlists.
    if !crate::offline_mode::engine().is_offline() {
        let fav_ids =
            crate::library_db::with_db(|db| db.get_favorite_playlist_ids()).unwrap_or_default();
        let known: std::collections::HashSet<u64> = playlists.iter().map(|p| p.id).collect();
        for fid in fav_ids {
            if known.contains(&fid) {
                continue;
            }
            if let Ok(p) = runtime.core().get_playlist(fid).await {
                let s = settings.get(&fid).cloned().unwrap_or_default();
                playlists.push(PmPlaylist {
                    id: fid,
                    name: p.name.clone(),
                    tracks_count: p.tracks_count,
                    duration: p.duration,
                    local_count: local_counts.get(&fid).copied().unwrap_or(0),
                    play_count: play_counts.get(&fid).copied().unwrap_or(0),
                    is_favorite: true,
                    is_hidden: s.hidden,
                    folder_id: s.folder_id.filter(|f| folder_ids.contains(f)),
                    position: s.position,
                    cover_urls: cover_urls(&p),
                    offline_available: snapshot_available.contains(&fid),
                });
            }
        }
    }

    // D11.b — OFFLINE: the Qobuz fetch is gate-refused (empty), so the
    // reachable playlists are synthesized locally: the MIXED ones (>= 1
    // local sidecar row) plus — B8 — the snapshot-available ones (>= 1
    // cached snapshot track). Names: the sidebar's session cache (loaded
    // while online), else the persisted snapshot (B7 — survives a cold
    // offline start), else the "Playlist (N local)" fallback.
    if crate::offline_mode::engine().is_offline() {
        let known: std::collections::HashSet<u64> = playlists.iter().map(|p| p.id).collect();
        let mut ids: Vec<u64> = local_counts
            .iter()
            .filter(|&(_, &count)| count > 0)
            .map(|(&id, _)| id)
            .collect();
        for &id in &snapshot_available {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        for id in ids {
            if known.contains(&id) {
                continue;
            }
            let count = local_counts.get(&id).copied().unwrap_or(0);
            let s = settings.get(&id).cloned().unwrap_or_default();
            let snapshot = snapshot_names.get(&id);
            let name = crate::sidebar::playlist_name_desc(id)
                .map(|(name, _)| name)
                .or_else(|| snapshot.map(|(name, _)| name.clone()))
                .unwrap_or_else(|| qbz_i18n::t_args("Playlist ({} local)", &[&count.to_string()]));
            playlists.push(PmPlaylist {
                id,
                name,
                // The snapshot's point-in-time Qobuz total when known
                // ("# of tracks" sort + the card count line).
                tracks_count: snapshot.and_then(|(_, tc)| *tc).unwrap_or(0),
                duration: 0,
                local_count: count,
                play_count: play_counts.get(&id).copied().unwrap_or(0),
                is_favorite: s.is_favorite,
                is_hidden: s.hidden,
                folder_id: s.folder_id.filter(|fid| folder_ids.contains(fid)),
                position: s.position,
                cover_urls: Vec::new(),
                offline_available: snapshot_available.contains(&id),
            });
        }
    }

    PmData {
        playlists,
        folders,
        locals,
    }
}

pub fn set_loading(window: &AppWindow, loading: bool) {
    window.global::<PlaylistManagerState>().set_loading(loading);
}

/// Store freshly-loaded data and render it.
pub fn apply(window: &AppWindow, data: PmData) {
    if let Ok(mut c) = CACHE.lock() {
        *c = data;
    }
    rebuild(window);
}

/// Reset the per-session tree-expand init so a fresh navigation
/// re-expands folders on first tree open.
pub fn reset_session(_window: &AppWindow) {
    if let Ok(mut init) = TREE_INIT.lock() {
        *init = false;
    }
}

// --- sort / filter ------------------------------------------------------

/// Order playlists by the active sort (mirrors `applySortToList`):
/// name (locale-ish), playcount desc, tracks (remote+local) desc, custom
/// (position asc); `recent` keeps the API order.
fn sort_playlists(list: &mut Vec<PmPlaylist>, sort: &str) {
    match sort {
        "name" => list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        "playcount" => list.sort_by(|a, b| b.play_count.cmp(&a.play_count)),
        "tracks" => list.sort_by(|a, b| b.total_count().cmp(&a.total_count())),
        "custom" => list.sort_by(|a, b| a.position.cmp(&b.position)),
        // "recent" — keep insertion (API) order.
        _ => {}
    }
}

/// Display-stage union of a Qobuz playlist and a LOCAL (library.db)
/// playlist, so locals INTERLEAVE into the active sort instead of being
/// appended after the Qobuz set (B4). The u64-keyed mutators (custom-order
/// reorder, move-to-folder) still parse the model ids and skip `local:`
/// ones — the folder tables are Qobuz-keyed; favorite / hide route to the
/// local repo instead (B3, `toggle_local_favorite` / `toggle_local_hidden`).
///
/// Sort keys locals don't have:
/// - playcount: no local play stat — locals sort as ZERO, which under the
///   descending playcount sort puts them last (after any played Qobuz set);
/// - custom: positions are a Qobuz-side concept — locals sort as MAX, i.e.
///   after the positioned Qobuz set;
/// - recent: no recency signal — kept after the API-ordered Qobuz set.
/// Ties keep the pre-sort order (stable sort): API order for Qobuz rows,
/// name order among the locals.
enum PmEntry<'a> {
    Qobuz(&'a PmPlaylist),
    Local(&'a PmLocalPlaylist),
}

impl PmEntry<'_> {
    fn name_lower(&self) -> String {
        match self {
            Self::Qobuz(p) => p.name.to_lowercase(),
            Self::Local(p) => p.name.to_lowercase(),
        }
    }

    fn total_count(&self) -> u32 {
        match self {
            Self::Qobuz(p) => p.total_count(),
            Self::Local(p) => p.track_count,
        }
    }

    fn play_count(&self) -> u32 {
        match self {
            Self::Qobuz(p) => p.play_count,
            Self::Local(_) => 0,
        }
    }

    fn position(&self) -> i64 {
        match self {
            Self::Qobuz(p) => p.position as i64,
            Self::Local(_) => i64::MAX,
        }
    }

    fn item(&self) -> PmPlaylistItem {
        match self {
            Self::Qobuz(p) => playlist_item(p),
            Self::Local(p) => local_playlist_item(p),
        }
    }
}

/// `sort_playlists`, over the merged Qobuz + local display set (same
/// comparators; see `PmEntry` for the missing-stat rules).
fn sort_entries(list: &mut [PmEntry], sort: &str) {
    match sort {
        "name" => list.sort_by_key(|e| e.name_lower()),
        "playcount" => list.sort_by(|a, b| b.play_count().cmp(&a.play_count())),
        "tracks" => list.sort_by(|a, b| b.total_count().cmp(&a.total_count())),
        "custom" => list.sort_by_key(|e| e.position()),
        // "recent" — Qobuz keeps API order, locals stay after it.
        _ => {}
    }
}

/// The LOCAL playlists that pass the toolbar filters, name-sorted (their
/// tie/no-stat order inside `sort_entries`). The visibility filter applies
/// to their own hidden flag (B3, `local_playlists.hidden`); folder
/// filtering N/A (root-only). `query` must already be lowercased.
fn local_entries<'a>(data: &'a PmData, query: &str, filter: &str) -> Vec<&'a PmLocalPlaylist> {
    let mut locals: Vec<&PmLocalPlaylist> = data
        .locals
        .iter()
        .filter(|p| query.is_empty() || p.name.to_lowercase().contains(query))
        .filter(|p| match filter {
            "visible" => !p.is_hidden,
            "hidden" => p.is_hidden,
            _ => true,
        })
        .collect();
    locals.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    locals
}

/// Whether a playlist passes the search + visibility + folder filters.
/// `current_folder` is None for the flat / root view (we never enter a
/// folder in the Slint port — folder navigation is via the tree).
fn passes(p: &PmPlaylist, query: &str, filter: &str, folder_mode: bool, view_mode: &str) -> bool {
    if !query.is_empty() && !p.name.to_lowercase().contains(query) {
        return false;
    }
    // Folder filter: in folder mode (non-tree), the grid/list shows ONLY
    // root playlists (folders own their members; opening a folder is the
    // tree's job in this port).
    if folder_mode && view_mode != "tree" && p.folder_id.is_some() {
        return false;
    }
    match filter {
        "visible" => !p.is_hidden,
        "hidden" => p.is_hidden,
        _ => true,
    }
}

// --- model builders -----------------------------------------------------

/// Parse a stored folder color into a Slint color. Only solid `#rgb` /
/// `#rrggbb` hex is representable; gradients ("linear-gradient(...)") and
/// CSS vars ("var(--accent-primary)") and empty values return None so the
/// card falls back to the accent.
fn parse_color(s: &str) -> Option<slint::Color> {
    let hex = s.strip_prefix('#')?;
    let (r, g, b) = match hex.len() {
        3 => {
            let v = u32::from_str_radix(hex, 16).ok()?;
            let r = ((v >> 8) & 0xf) as u8;
            let g = ((v >> 4) & 0xf) as u8;
            let b = (v & 0xf) as u8;
            (r * 17, g * 17, b * 17)
        }
        6 => {
            let v = u32::from_str_radix(hex, 16).ok()?;
            (((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8)
        }
        _ => return None,
    };
    Some(slint::Color::from_rgb_u8(r, g, b))
}

/// Total-playtime label, e.g. "1h 43m" or "12m" (mirrors Tauri's
/// `formatDuration`). Empty when the duration is zero.
fn format_duration(seconds: u32) -> String {
    if seconds == 0 {
        return String::new();
    }
    let hours = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    if hours > 0 {
        qbz_i18n::t_args("{} h {} min", &[&hours.to_string(), &mins.to_string()])
    } else {
        qbz_i18n::t_args("{} min", &[&mins.to_string()])
    }
}

fn folder_item(f: &FolderFull, count: usize) -> PmFolderItem {
    let color = parse_color(&f.icon_color);
    PmFolderItem {
        id: f.id.clone().into(),
        name: f.name.clone().into(),
        count: count as i32,
        icon_type: f.icon_type.clone().into(),
        icon_preset: f.icon_preset.clone().into(),
        icon_color: color.unwrap_or_default(),
        has_color: color.is_some(),
        is_hidden: f.is_hidden,
        custom_image: slint::Image::default(),
        has_custom_image: f.icon_type == "custom" && f.custom_image_path.is_some(),
    }
}

fn playlist_item(p: &PmPlaylist) -> PmPlaylistItem {
    let url = |i: usize| -> slint::SharedString {
        p.cover_urls.get(i).cloned().unwrap_or_default().into()
    };
    let local_status = if p.local_count == 0 {
        "no"
    } else if p.tracks_count == 0 {
        "all_local"
    } else {
        "some_local"
    };
    let local_line = if p.local_count > 0 {
        qbz_i18n::t_args("({} local)", &[&p.local_count.to_string()])
    } else {
        String::new()
    };
    PmPlaylistItem {
        id: p.id.to_string().into(),
        name: p.name.clone().into(),
        tracks_line: { let n = p.total_count(); qbz_i18n::tf("{} track", "{} tracks", n as i64, &[&n.to_string()]).into() },
        duration_line: format_duration(p.duration).into(),
        local_line: local_line.into(),
        local_count: p.local_count as i32,
        total_count: p.total_count() as i32,
        play_count: p.play_count as i32,
        local_status: local_status.into(),
        is_favorite: p.is_favorite,
        is_hidden: p.is_hidden,
        is_local_playlist: false,
        offline_only: false,
        folder_id: p.folder_id.clone().unwrap_or_default().into(),
        cover_count: p.cover_urls.len().min(4) as i32,
        url1: url(0),
        url2: url(1),
        url3: url(2),
        url4: url(3),
        cover1: slint::Image::default(),
        cover2: slint::Image::default(),
        cover3: slint::Image::default(),
        cover4: slint::Image::default(),
    }
}

fn local_playlist_item(p: &PmLocalPlaylist) -> PmPlaylistItem {
    PmPlaylistItem {
        id: p.id.clone().into(),
        name: p.name.clone().into(),
        tracks_line: qbz_i18n::tf("{} track", "{} tracks", p.track_count as i64, &[&p.track_count.to_string()]).into(),
        duration_line: "".into(),
        local_line: "".into(),
        local_count: 0,
        total_count: p.track_count as i32,
        play_count: 0,
        local_status: "".into(),
        is_favorite: p.is_favorite,
        is_hidden: p.is_hidden,
        is_local_playlist: true,
        offline_only: p.offline_only,
        folder_id: "".into(),
        cover_count: 0,
        url1: Default::default(),
        url2: Default::default(),
        url3: Default::default(),
        url4: Default::default(),
        cover1: slint::Image::default(),
        cover2: slint::Image::default(),
        cover3: slint::Image::default(),
        cover4: slint::Image::default(),
    }
}

/// Rebuild the visible grid/list model + the folder model + the tree
/// from the cache, honoring the active toolbar state. UI thread only.
pub fn rebuild(window: &AppWindow) {
    let data = CACHE.lock().map(|c| c.clone()).unwrap_or_default();
    let state = window.global::<PlaylistManagerState>();
    let query = state.get_search_query().trim().to_lowercase();
    let filter = state.get_filter().to_string();
    let sort = state.get_sort().to_string();
    let view_mode = state.get_view_mode().to_string();
    let folder_mode = state.get_folder_mode();

    // Folder counts (members regardless of search/visibility, like Tauri).
    let mut folder_counts: HashMap<String, usize> = HashMap::new();
    for p in &data.playlists {
        if let Some(fid) = &p.folder_id {
            *folder_counts.entry(fid.clone()).or_insert(0) += 1;
        }
    }

    let folders: Vec<PmFolderItem> = data
        .folders
        .iter()
        .map(|f| folder_item(f, folder_counts.get(&f.id).copied().unwrap_or(0)))
        .collect();

    // Filtered + sorted playlists for the grid / list. While OFFLINE only
    // the MIXED (>= 1 local sidecar track) and snapshot-available (>= 1
    // cached snapshot track, B8) playlists stay (D11.b).
    let offline = crate::offline_mode::engine().is_offline();
    let filtered: Vec<PmPlaylist> = data
        .playlists
        .iter()
        .filter(|p| !offline || p.local_count > 0 || p.offline_available)
        .filter(|p| passes(p, &query, &filter, folder_mode, &view_mode))
        .cloned()
        .collect();
    // LOCAL playlists (library.db, D7) interleave into the SAME sort as the
    // Qobuz set (B4) — see `PmEntry` for the missing-stat sort rules.
    let mut entries: Vec<PmEntry> = filtered.iter().map(PmEntry::Qobuz).collect();
    entries.extend(local_entries(&data, &query, &filter).into_iter().map(PmEntry::Local));
    sort_entries(&mut entries, &sort);
    let playlist_items: Vec<PmPlaylistItem> = entries.iter().map(|e| e.item()).collect();
    let visible_count = playlist_items.len();

    // Tree rows (folder headers + nested + root playlists). Built only
    // when the tree view is active; otherwise an empty model.
    let tree = if folder_mode && view_mode == "tree" {
        build_tree(&data, &query, &filter, &sort)
    } else {
        Vec::new()
    };

    // The list-row move-to-folder menu starts unfiltered (= the full set);
    // its search box narrows it via `search_menu_folders`.
    let menu_folders: Vec<PmFolderItem> = data
        .folders
        .iter()
        .map(|f| folder_item(f, folder_counts.get(&f.id).copied().unwrap_or(0)))
        .collect();

    state.set_folders(ModelRc::new(VecModel::from(folders)));
    state.set_menu_folders(ModelRc::new(VecModel::from(menu_folders)));
    state.set_playlists(ModelRc::new(VecModel::from(playlist_items)));
    state.set_tree(ModelRc::new(VecModel::from(tree)));
    state.set_folder_count(data.folders.len() as i32);
    state.set_playlist_count(visible_count as i32);
    state.set_can_reorder(sort == "custom" && query.is_empty());
    state.set_loading(false);
}

/// Filter the list-row move-to-folder menu's folder list by a
/// case-insensitive substring (Slint strings have no `contains`, so this
/// lives in Rust). An empty query restores the full list. Counts mirror the
/// `rebuild` computation. UI thread only.
pub fn search_menu_folders(window: &AppWindow, query: &str) {
    let q = query.trim().to_lowercase();
    let data = CACHE.lock().map(|c| c.clone()).unwrap_or_default();

    let mut folder_counts: HashMap<String, usize> = HashMap::new();
    for p in &data.playlists {
        if let Some(fid) = &p.folder_id {
            *folder_counts.entry(fid.clone()).or_insert(0) += 1;
        }
    }

    let filtered: Vec<PmFolderItem> = data
        .folders
        .iter()
        .filter(|f| q.is_empty() || f.name.to_lowercase().contains(&q))
        .map(|f| folder_item(f, folder_counts.get(&f.id).copied().unwrap_or(0)))
        .collect();

    window
        .global::<PlaylistManagerState>()
        .set_menu_folders(ModelRc::new(VecModel::from(filtered)));
}

/// Flatten folders + their (expanded) playlists + root playlists into the
/// tree model. Auto-expands all folders the first time the tree opens.
fn build_tree(data: &PmData, query: &str, filter: &str, sort: &str) -> Vec<PmTreeRow> {
    // Auto-expand on first tree open (Tauri's treeInitialized).
    {
        let mut init = TREE_INIT.lock().unwrap_or_else(|e| e.into_inner());
        if !*init {
            if let Ok(mut exp) = EXPANDED.lock() {
                for f in &data.folders {
                    exp.insert(f.id.clone());
                }
            }
            *init = true;
        }
    }
    let expanded = EXPANDED.lock().map(|e| e.clone()).unwrap_or_default();
    let searching = !query.is_empty();
    let offline = crate::offline_mode::engine().is_offline();

    let matches = |p: &PmPlaylist| -> bool {
        // D11.b: offline only the MIXED and snapshot-available (B8)
        // playlists stay.
        if offline && p.local_count == 0 && !p.offline_available {
            return false;
        }
        if searching && !p.name.to_lowercase().contains(query) {
            return false;
        }
        match filter {
            "visible" => !p.is_hidden,
            "hidden" => p.is_hidden,
            _ => true,
        }
    };

    let mut rows: Vec<PmTreeRow> = Vec::new();
    for f in &data.folders {
        let mut members: Vec<PmPlaylist> = data
            .playlists
            .iter()
            .filter(|p| p.folder_id.as_deref() == Some(f.id.as_str()))
            .filter(|p| matches(p))
            .cloned()
            .collect();
        // While searching — and offline, where the D11.b filter may empty a
        // folder — skip folders with no visible members.
        if (searching || offline) && members.is_empty() {
            continue;
        }
        sort_playlists(&mut members, sort);
        let is_exp = searching || expanded.contains(&f.id);
        rows.push(PmTreeRow {
            kind: "folder".into(),
            expanded: is_exp,
            folder: folder_item(f, members.len()),
            playlist: PmPlaylistItem::default(),
            indent: false,
        });
        if is_exp {
            for p in &members {
                rows.push(PmTreeRow {
                    kind: "playlist".into(),
                    expanded: false,
                    folder: PmFolderItem::default(),
                    playlist: playlist_item(p),
                    indent: true,
                });
            }
        }
    }
    // Root playlists (no folder), with the LOCAL playlists (never in
    // folders) interleaved into the SAME sort (B4) — see `PmEntry` for the
    // missing-stat sort rules.
    let root: Vec<PmPlaylist> = data
        .playlists
        .iter()
        .filter(|p| p.folder_id.is_none())
        .filter(|p| matches(p))
        .cloned()
        .collect();
    let mut entries: Vec<PmEntry> = root.iter().map(PmEntry::Qobuz).collect();
    entries.extend(local_entries(data, query, filter).into_iter().map(PmEntry::Local));
    sort_entries(&mut entries, sort);
    for e in &entries {
        rows.push(PmTreeRow {
            kind: "playlist".into(),
            expanded: false,
            folder: PmFolderItem::default(),
            playlist: e.item(),
            indent: false,
        });
    }
    rows
}

/// Toggle a tree folder's expand state, then rebuild (cheap, from cache).
pub fn toggle_tree_folder(window: &AppWindow, folder_id: &str) {
    if let Ok(mut exp) = EXPANDED.lock() {
        if !exp.remove(folder_id) {
            exp.insert(folder_id.to_string());
        }
    }
    rebuild(window);
}

// --- artwork ------------------------------------------------------------

/// Build artwork jobs for every visible grid/list playlist card's collage
/// covers (targeting `PlaylistManagerState.playlists` by row index) plus
/// the tree rows' playlists. Plus the folders' decoded custom images.
pub fn artwork_jobs(window: &AppWindow) -> Vec<ArtworkJob> {
    let state = window.global::<PlaylistManagerState>();
    let mut jobs = Vec::new();

    let playlists = state.get_playlists();
    for index in 0..playlists.row_count() {
        let Some(p) = playlists.row_data(index) else {
            continue;
        };
        for (slot, url) in [p.url1, p.url2, p.url3, p.url4].iter().enumerate() {
            if !url.is_empty() {
                jobs.push(ArtworkJob {
                    target: ArtworkTarget::PmPlaylistCover { index, slot },
                    url: url.to_string(),
                });
            }
        }
    }

    let tree = state.get_tree();
    for index in 0..tree.row_count() {
        let Some(row) = tree.row_data(index) else {
            continue;
        };
        if row.kind != "playlist" {
            continue;
        }
        let p = row.playlist;
        for (slot, url) in [p.url1, p.url2, p.url3, p.url4].iter().enumerate() {
            if !url.is_empty() {
                jobs.push(ArtworkJob {
                    target: ArtworkTarget::PmTreeCover { index, slot },
                    url: url.to_string(),
                });
            }
        }
    }
    jobs
}

/// Decode the folder cards' custom images (local files) on a worker and
/// push them into the folder model. Folder custom images come from
/// `library.db`; the URL pipeline only handles http(s), so these are read
/// + decoded directly here.
pub fn load_folder_custom_images(weak: slint::Weak<AppWindow>, handle: &tokio::runtime::Handle) {
    let data = CACHE.lock().map(|c| c.clone()).unwrap_or_default();
    let with_images: Vec<(String, String)> = data
        .folders
        .iter()
        .filter(|f| f.icon_type == "custom")
        .filter_map(|f| f.custom_image_path.clone().map(|p| (f.id.clone(), p)))
        .collect();
    if with_images.is_empty() {
        return;
    }
    handle.spawn(async move {
        for (folder_id, path) in with_images {
            let path2 = path.clone();
            let decoded =
                tokio::task::spawn_blocking(move || decode_local_image(&path2, 160)).await;
            if let Ok(Some((pixels, w, h))) = decoded {
                let fid = folder_id.clone();
                let _ = weak.upgrade_in_event_loop(move |win| {
                    set_folder_image(&win, &fid, &pixels, w, h);
                });
            }
        }
    });
}

/// Read + decode a local image file to RGBA8, downscaled to `size`.
fn decode_local_image(path: &str, size: u32) -> Option<(Vec<u8>, u32, u32)> {
    let img = image::open(path).ok()?.thumbnail(size, size).to_rgba8();
    let (w, h) = img.dimensions();
    Some((img.into_raw(), w, h))
}

/// Look up a folder's full record (from the cache) for the editor.
pub fn folder_for_edit(folder_id: &str) -> Option<FolderFull> {
    CACHE
        .lock()
        .ok()?
        .folders
        .iter()
        .find(|f| f.id == folder_id)
        .cloned()
}

/// Decode a local image file and push it into the folder-editor preview
/// (FolderEditState.custom-image). Used when opening the editor on a
/// folder with an existing custom image, and after the user picks one.
pub fn load_editor_custom_image(weak: slint::Weak<AppWindow>, path: String) {
    std::thread::spawn(move || {
        if let Some((pixels, w, h)) = decode_local_image(&path, 160) {
            let _ = weak.upgrade_in_event_loop(move |win| {
                let image = artwork::pixels_to_image(&pixels, w, h);
                win.global::<crate::FolderEditState>().set_custom_image(image);
            });
        }
    });
}

fn set_folder_image(window: &AppWindow, folder_id: &str, pixels: &[u8], w: u32, h: u32) {
    let image = artwork::pixels_to_image(pixels, w, h);
    let model = window.global::<PlaylistManagerState>().get_folders();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.id == folder_id {
                item.custom_image = image.clone();
                model.set_row_data(i, item);
            }
        }
    }
    // Mirror into the tree's folder rows.
    let tree = window.global::<PlaylistManagerState>().get_tree();
    for i in 0..tree.row_count() {
        if let Some(mut row) = tree.row_data(i) {
            if row.kind == "folder" && row.folder.id == folder_id {
                row.folder.custom_image = image.clone();
                tree.set_row_data(i, row);
            }
        }
    }
}

// --- optimistic local mutations ----------------------------------------

/// Flip a playlist's favorite flag in the cache + rebuild.
pub fn toggle_favorite_local(window: &AppWindow, playlist_id: u64) -> bool {
    let mut new_val = false;
    if let Ok(mut c) = CACHE.lock() {
        if let Some(p) = c.playlists.iter_mut().find(|p| p.id == playlist_id) {
            p.is_favorite = !p.is_favorite;
            new_val = p.is_favorite;
        }
    }
    rebuild(window);
    new_val
}

/// Flip a playlist's hidden flag in the cache + rebuild.
pub fn toggle_hidden_local(window: &AppWindow, playlist_id: u64) -> bool {
    let mut new_val = false;
    if let Ok(mut c) = CACHE.lock() {
        if let Some(p) = c.playlists.iter_mut().find(|p| p.id == playlist_id) {
            p.is_hidden = !p.is_hidden;
            new_val = p.is_hidden;
        }
    }
    rebuild(window);
    new_val
}

/// Flip a LOCAL playlist's favorite flag in the cache + rebuild (B3).
/// Returns the new value for the repo write.
pub fn toggle_local_favorite(window: &AppWindow, id: &str) -> bool {
    let mut new_val = false;
    if let Ok(mut c) = CACHE.lock() {
        if let Some(p) = c.locals.iter_mut().find(|p| p.id == id) {
            p.is_favorite = !p.is_favorite;
            new_val = p.is_favorite;
        }
    }
    rebuild(window);
    new_val
}

/// Flip a LOCAL playlist's hidden flag in the cache + rebuild (B3).
/// Returns the new value for the repo write.
pub fn toggle_local_hidden(window: &AppWindow, id: &str) -> bool {
    let mut new_val = false;
    if let Ok(mut c) = CACHE.lock() {
        if let Some(p) = c.locals.iter_mut().find(|p| p.id == id) {
            p.is_hidden = !p.is_hidden;
            new_val = p.is_hidden;
        }
    }
    rebuild(window);
    new_val
}

/// Move a playlist into a folder ("" = root) in the cache + rebuild.
pub fn move_to_folder_local(window: &AppWindow, playlist_id: u64, folder_id: &str) {
    if let Ok(mut c) = CACHE.lock() {
        if let Some(p) = c.playlists.iter_mut().find(|p| p.id == playlist_id) {
            p.folder_id = if folder_id.is_empty() {
                None
            } else {
                Some(folder_id.to_string())
            };
        }
    }
    rebuild(window);
}

/// Move a playlist one slot up (custom sort): swap with its predecessor in
/// the current visible order, write the new positions back to the cache,
/// rebuild, and return the new full id order for persistence (empty when
/// the move is a no-op, e.g. already first).
pub fn move_up(window: &AppWindow, playlist_id: u64) -> Vec<u64> {
    reorder_step(window, playlist_id, -1)
}

/// Move a playlist one slot down (custom sort).
pub fn move_down(window: &AppWindow, playlist_id: u64) -> Vec<u64> {
    reorder_step(window, playlist_id, 1)
}

/// Shared up/down logic: reorder the currently-visible list (root, custom
/// sort) and assign fresh positions to the *full* playlist set so the
/// `position` field stays a total order. Returns the new id order for the
/// DB write, or empty on a no-op.
fn reorder_step(window: &AppWindow, playlist_id: u64, delta: i32) -> Vec<u64> {
    let model = window.global::<PlaylistManagerState>().get_playlists();
    let mut ids: Vec<u64> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter_map(|it| it.id.parse::<u64>().ok())
        .collect();
    let Some(pos) = ids.iter().position(|&id| id == playlist_id) else {
        return Vec::new();
    };
    let target = pos as i32 + delta;
    if target < 0 || target as usize >= ids.len() {
        return Vec::new();
    }
    ids.swap(pos, target as usize);

    // Write fresh positions back into the cache for the reordered ids, then
    // rebuild so the move is reflected immediately under the custom sort.
    if let Ok(mut c) = CACHE.lock() {
        for (i, id) in ids.iter().enumerate() {
            if let Some(p) = c.playlists.iter_mut().find(|p| p.id == *id) {
                p.position = i as i32;
            }
        }
    }
    rebuild(window);
    ids
}

// --- navigation ---------------------------------------------------------

/// Open the Playlist Manager and load its data. Mirrors `navigate_favorites`.
pub fn navigate(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: ImageCache,
) {
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            reset_session(&w);
            set_loading(&w, true);
            w.global::<NavState>().set_view(ContentView::PlaylistManager);
        });
        let data = load(&runtime).await;
        let handle2 = tokio::runtime::Handle::current();
        let weak2 = weak.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            apply(&w, data);
            let jobs = artwork_jobs(&w);
            artwork::spawn_loads(jobs, weak2.clone(), image_cache.clone());
            load_folder_custom_images(weak2.clone(), &handle2);
        });
    });
}
