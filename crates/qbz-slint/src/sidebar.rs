//! Sidebar playlists + folders controller. Builds the flattened
//! left-nav list (folder headers with their playlists + root
//! playlists) from the user's Qobuz playlists and the local folder
//! organization (library.db). The loaded data is cached so expand /
//! move operations rebuild the list without re-hitting the network.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::folders::FolderInfo;
use crate::{AppWindow, SidebarEntry, SidebarFolderPopupState, SidebarPlaylistItem, SidebarState};

#[derive(Clone)]
pub struct SidebarPlaylist {
    pub id: u64,
    pub name: String,
    /// Playlist description (Qobuz). Empty when none. Used to prefill the
    /// edit-playlist modal opened from the sidebar context menu.
    pub description: String,
    /// Total track count (Qobuz). Used by the "# of tracks" sort.
    pub tracks_count: u32,
    /// Up to four cover-art URLs for the micro-collage, sourced from the
    /// `get_user_playlists()` payload (images300 / images150 / images).
    /// No extra fetch — same as Tauri's `images150 ?? images300 ?? images`.
    pub cover_urls: Vec<String>,
    /// Custom-sort position (from `playlist_settings.position`); the
    /// `Custom` sort orders by this ascending.
    pub position: i32,
}

/// Pick up to four de-duplicated cover URLs for a playlist, preferring the
/// largest available list (mirrors Tauri's `images150 ?? images300 ??
/// images`, but de-duplicated and capped at four for the 2x2 collage).
fn playlist_cover_urls(p: &qbz_models::Playlist) -> Vec<String> {
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

#[derive(Clone, Default)]
pub struct SidebarData {
    pub playlists: Vec<SidebarPlaylist>,
    pub folders: Vec<FolderInfo>,
    pub folder_map: HashMap<u64, String>,
    /// Playlist ids the user has hidden from the sidebar (local settings).
    pub hidden_playlists: HashSet<u64>,
}

/// Session-only folder expand state (matches Tauri — not persisted).
static EXPANDED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
/// Last loaded data, so expand/move rebuild without a refetch.
static CACHE: LazyLock<Mutex<SidebarData>> = LazyLock::new(|| Mutex::new(SidebarData::default()));
/// Active sort option, mirrored from `SidebarState.sort-option`. Session
/// scope (Tauri persists in localStorage; we have no equivalent store
/// here yet, so this matches the in-session behavior).
static SORT: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new("name".to_string()));
/// Active playlist-name search query (lowercased), mirrored from
/// `SidebarState.search-query`. Filters the rebuilt list recursively.
static SEARCH: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));
/// playlist id -> (name, description) from the last loaded payload, so the
/// sidebar context menu can prefill the edit-playlist modal without a
/// refetch.
static NAME_DESC: LazyLock<Mutex<HashMap<u64, (String, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn set_loading(window: &AppWindow, loading: bool) {
    window.global::<SidebarState>().set_loading(loading);
}

/// Update the active sort option and re-render (matches Tauri's five
/// options). Unknown values fall back to "name".
pub fn set_sort(window: &AppWindow, option: &str) {
    let opt = match option {
        "name" | "recent" | "tracks" | "playcount" | "custom" => option,
        _ => "name",
    };
    if let Ok(mut s) = SORT.lock() {
        *s = opt.to_string();
    }
    window.global::<SidebarState>().set_sort_option(opt.into());
    rebuild(window);
}

/// Update the playlist-name search filter and re-render. An empty query
/// shows everything.
pub fn set_search(window: &AppWindow, query: &str) {
    if let Ok(mut q) = SEARCH.lock() {
        *q = query.trim().to_lowercase();
    }
    rebuild(window);
}

/// Order playlists by the active sort option, mirroring Tauri's
/// comparators. `recent` keeps reverse insertion order (most-recently
/// added first); `playcount` has no per-playlist count source here, so it
/// stays stable like Tauri does when `play_count` is absent (0).
fn sort_playlists(playlists: &[SidebarPlaylist]) -> Vec<SidebarPlaylist> {
    let sort = SORT.lock().map(|s| s.clone()).unwrap_or_else(|_| "name".into());
    let mut out: Vec<SidebarPlaylist> = playlists.to_vec();
    match sort.as_str() {
        "name" => out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        "recent" => out.reverse(),
        "tracks" => out.sort_by(|a, b| b.tracks_count.cmp(&a.tracks_count)),
        "playcount" => { /* no play_count source — stable, like Tauri's absent field */ }
        "custom" => out.sort_by(|a, b| a.position.cmp(&b.position)),
        _ => out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
    }
    out
}

/// Fetch playlists (Qobuz) + folders + folder membership (local).
pub async fn load<A>(runtime: &AppRuntime<A>) -> SidebarData
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let playlists: Vec<SidebarPlaylist> = match runtime.core().get_user_playlists().await {
        Ok(pls) => pls
            .into_iter()
            .map(|p| SidebarPlaylist {
                id: p.id,
                name: p.name.clone(),
                description: p.description.clone().unwrap_or_default(),
                tracks_count: p.tracks_count,
                cover_urls: playlist_cover_urls(&p),
                position: 0,
            })
            .collect(),
        Err(e) => {
            log::warn!("[qbz-slint] sidebar playlists load failed: {e}");
            Vec::new()
        }
    };
    // Folders (hidden folders excluded) + folder membership +
    // per-playlist custom-sort positions + hidden-playlist set (all
    // local, library.db).
    let (folders, folder_map, positions, hidden_playlists) =
        tokio::task::spawn_blocking(|| {
            let folders: Vec<FolderInfo> = crate::folders::load_folders_full()
                .into_iter()
                .filter(|f| !f.is_hidden)
                .map(|f| FolderInfo {
                    id: f.id,
                    name: f.name,
                })
                .collect();
            let hidden_playlists: HashSet<u64> = crate::folders::playlist_settings_map()
                .into_iter()
                .filter(|(_, s)| s.hidden)
                .map(|(id, _)| id)
                .collect();
            (
                folders,
                crate::folders::playlist_folder_map(),
                crate::folders::playlist_positions(),
                hidden_playlists,
            )
        })
        .await
        .unwrap_or_default();
    let mut playlists = playlists;
    for p in &mut playlists {
        if let Some(pos) = positions.get(&p.id) {
            p.position = *pos;
        }
    }
    // Cache the loaded playlists' name+description for the sidebar
    // context-menu edit modal (no extra fetch on right-click).
    if let Ok(mut nd) = NAME_DESC.lock() {
        nd.clear();
        for p in &playlists {
            nd.insert(p.id, (p.name.clone(), p.description.clone()));
        }
    }
    SidebarData {
        playlists,
        folders,
        folder_map,
        hidden_playlists,
    }
}

/// Name + description for `id`, from the last loaded playlist payload.
/// Used by the sidebar context menu to prefill the edit-playlist modal
/// without a refetch. Returns None when the playlist is unknown.
pub fn playlist_name_desc(id: u64) -> Option<(String, String)> {
    NAME_DESC.lock().ok().and_then(|nd| nd.get(&id).cloned())
}

/// Total track count for `id`, from the last loaded playlist cache. Used by the
/// sidebar "Add to Mixtape/Collection" context action to populate the AddItem
/// `track_count` (the SidebarEntry struct doesn't carry it). Returns None when
/// the playlist is unknown.
pub fn playlist_track_count(id: u64) -> Option<u32> {
    CACHE
        .lock()
        .ok()
        .and_then(|c| c.playlists.iter().find(|p| p.id == id).map(|p| p.tracks_count))
}

/// Store the freshly-loaded data and render it.
pub fn apply(window: &AppWindow, data: SidebarData) {
    if let Ok(mut cache) = CACHE.lock() {
        *cache = data;
    }
    rebuild(window);
}

/// Build a playlist `SidebarEntry` (with its cover URLs for the
/// micro-collage). The decoded `cover*` images stay default here and are
/// filled asynchronously by the artwork pipeline (see `artwork_jobs`).
fn playlist_entry(p: &SidebarPlaylist, indent: bool, folder_id: &str) -> SidebarEntry {
    let url = |i: usize| -> slint::SharedString {
        p.cover_urls.get(i).cloned().unwrap_or_default().into()
    };
    SidebarEntry {
        kind: "playlist".into(),
        id: p.id.to_string().into(),
        name: p.name.clone().into(),
        expanded: false,
        count: 0,
        indent,
        folder_id: folder_id.into(),
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

/// Populate the collapsed-sidebar folder flyout with `folder_id`'s playlists,
/// built from the cache so it works even for collapsed folders (whose
/// children are absent from the flattened `entries`).
pub fn load_folder_popup(window: &AppWindow, folder_id: &str) {
    let data = CACHE.lock().map(|c| c.clone()).unwrap_or_default();
    let sorted = sort_playlists(&data.playlists);
    let entries: Vec<SidebarEntry> = sorted
        .iter()
        .filter(|p| {
            data.folder_map
                .get(&p.id)
                .map(|f| f.as_str() == folder_id)
                .unwrap_or(false)
        })
        .filter(|p| !data.hidden_playlists.contains(&p.id))
        .map(|p| playlist_entry(p, true, folder_id))
        .collect();
    window
        .global::<SidebarFolderPopupState>()
        .set_playlists(ModelRc::new(VecModel::from(entries)));
}

/// Rebuild the flattened entries (+ the folders list for the
/// move-to-folder menu) from the cache + expand state, applying the
/// active sort and the playlist-name search filter.
pub fn rebuild(window: &AppWindow) {
    let data = CACHE.lock().map(|c| c.clone()).unwrap_or_default();
    let expanded = EXPANDED.lock().map(|e| e.clone()).unwrap_or_default();
    let query = SEARCH.lock().map(|q| q.clone()).unwrap_or_default();
    let searching = !query.is_empty();
    let folder_ids: HashSet<&String> = data.folders.iter().map(|f| &f.id).collect();

    // Sort then filter by the playlist-name query (recursive — the same
    // filter applies to playlists nested in folders).
    let sorted = sort_playlists(&data.playlists);
    let matches = |p: &SidebarPlaylist| !searching || p.name.to_lowercase().contains(&query);

    let mut entries: Vec<SidebarEntry> = Vec::new();
    for folder in &data.folders {
        let members: Vec<&SidebarPlaylist> = sorted
            .iter()
            .filter(|p| data.folder_map.get(&p.id).map(|f| f == &folder.id).unwrap_or(false))
            .filter(|p| matches(p))
            .filter(|p| !data.hidden_playlists.contains(&p.id))
            .collect();
        // While searching, skip folders with no matching playlists
        // (mirrors Tauri's `if (isSearching && folderPlaylists.length === 0) continue`).
        if searching && members.is_empty() {
            continue;
        }
        // When searching, force-expand so matches inside are visible.
        let is_exp = searching || expanded.contains(&folder.id);
        entries.push(SidebarEntry {
            kind: "folder".into(),
            id: folder.id.clone().into(),
            name: folder.name.clone().into(),
            expanded: is_exp,
            count: members.len() as i32,
            indent: false,
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
        });
        if is_exp {
            for p in members {
                entries.push(playlist_entry(p, true, &folder.id));
            }
        }
    }
    // Root playlists — no folder, or a folder that no longer exists.
    for p in &sorted {
        let in_folder = data
            .folder_map
            .get(&p.id)
            .map(|f| folder_ids.contains(f))
            .unwrap_or(false);
        if !in_folder && matches(p) && !data.hidden_playlists.contains(&p.id) {
            entries.push(playlist_entry(p, false, ""));
        }
    }

    let folders: Vec<SidebarPlaylistItem> = data
        .folders
        .iter()
        .map(|f| SidebarPlaylistItem {
            id: f.id.clone().into(),
            name: f.name.clone().into(),
        })
        .collect();

    let state = window.global::<SidebarState>();
    state.set_entries(ModelRc::new(VecModel::from(entries)));
    state.set_folders(ModelRc::new(VecModel::from(folders.clone())));
    // The move-to-folder menu starts unfiltered (= the full folder set);
    // the search box narrows it via `search_menu_folders`.
    state.set_menu_folders(ModelRc::new(VecModel::from(folders)));
    state.set_loading(false);
}

/// Filter the move-to-folder menu's folder list by a case-insensitive
/// substring of the search query (Slint strings have no `contains`, so
/// this lives in Rust). An empty query restores the full list.
pub fn search_menu_folders(window: &AppWindow, query: &str) {
    let q = query.trim().to_lowercase();
    let data = CACHE.lock().map(|c| c.clone()).unwrap_or_default();
    let filtered: Vec<SidebarPlaylistItem> = data
        .folders
        .iter()
        .filter(|f| q.is_empty() || f.name.to_lowercase().contains(&q))
        .map(|f| SidebarPlaylistItem {
            id: f.id.clone().into(),
            name: f.name.clone().into(),
        })
        .collect();
    window
        .global::<SidebarState>()
        .set_menu_folders(ModelRc::new(VecModel::from(filtered)));
}

/// Build artwork-download jobs for every playlist row's collage covers,
/// targeting `SidebarState.entries` by row index. Call after `apply` /
/// `rebuild` updates the entries.
pub fn artwork_jobs(window: &AppWindow) -> Vec<crate::artwork::ArtworkJob> {
    use slint::Model;
    let mut jobs = Vec::new();
    let entries = window.global::<SidebarState>().get_entries();
    for idx in 0..entries.row_count() {
        let Some(e) = entries.row_data(idx) else { continue };
        if e.kind != "playlist" {
            continue;
        }
        let urls = [e.url1, e.url2, e.url3, e.url4];
        for (slot, url) in urls.iter().enumerate() {
            if !url.is_empty() {
                jobs.push(crate::artwork::ArtworkJob {
                    target: crate::artwork::ArtworkTarget::SidebarPlaylistCover { idx, slot },
                    url: url.to_string(),
                });
            }
        }
    }
    jobs
}

/// Toggle a folder's expanded state, then re-render from cache.
pub fn toggle_folder(window: &AppWindow, folder_id: &str) {
    if let Ok(mut exp) = EXPANDED.lock() {
        if !exp.remove(folder_id) {
            exp.insert(folder_id.to_string());
        }
    }
    rebuild(window);
}

/// Optimistically move a playlist in the cache (folder_id "" = root)
/// and re-render. The DB write happens separately.
pub fn move_playlist_local(window: &AppWindow, playlist_id: u64, folder_id: &str) {
    if let Ok(mut cache) = CACHE.lock() {
        if folder_id.is_empty() {
            cache.folder_map.remove(&playlist_id);
        } else {
            cache.folder_map.insert(playlist_id, folder_id.to_string());
        }
    }
    rebuild(window);
}

/// Highlight the open playlist in the sidebar (or clear with "").
pub fn set_active(window: &AppWindow, id: &str) {
    window.global::<SidebarState>().set_active_id(id.into());
}

/// Whether `id` is one of the user's own playlists — used to gate
/// playlist editing.
pub fn contains(window: &AppWindow, id: &str) -> bool {
    use slint::Model;
    let entries = window.global::<SidebarState>().get_entries();
    entries
        .iter()
        .any(|e| e.kind == "playlist" && e.id == id)
}
