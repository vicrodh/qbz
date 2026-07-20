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

/// One LOCAL playlist (library.db entity, id `local:<uuid>`) listed
/// alongside the Qobuz playlists. Always available — including offline.
#[derive(Clone)]
pub struct LocalSidebarPlaylist {
    pub id: String,
    pub name: String,
    pub description: String,
    pub offline_only: bool,
    /// Sidebar folder membership (shared `playlist_folders.id`); None = root.
    pub folder_id: Option<String>,
    /// Up to four cover refs for the micro-collage, resolved from the
    /// playlist's tracks' artwork (local file paths / Plex thumbs / cached
    /// Qobuz covers — no network). Empty = render the hard-drive glyph.
    pub cover_urls: Vec<String>,
}

#[derive(Clone, Default)]
pub struct SidebarData {
    pub playlists: Vec<SidebarPlaylist>,
    pub folders: Vec<FolderInfo>,
    pub folder_map: HashMap<u64, String>,
    /// Playlist ids the user has hidden from the sidebar (local settings).
    pub hidden_playlists: HashSet<u64>,
    /// First-class local playlists (offline-mode D7), appended as root rows.
    pub local_playlists: Vec<LocalSidebarPlaylist>,
    /// Qobuz playlist id -> local sidecar track count (library.db
    /// `playlist_local_tracks`). The D11.b offline filter keeps only the
    /// MIXED playlists (count > 0); unused while online.
    pub local_counts: HashMap<u64, u32>,
    /// B8: Qobuz playlists whose local SNAPSHOT membership has >= 1 track
    /// playable offline (snapshot ∩ cached, grace-gated). Extends the
    /// D11.b offline filter; empty while online.
    pub snapshot_available: HashSet<u64>,
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
        Ok(pls) => {
            // B7 producer (names): persist id+name(+owner, track_count) for
            // ALL listed playlists — data this load already fetched, written
            // detached so the render never waits. Offline the fetch is
            // gate-refused (Err), so the snapshot is never clobbered.
            crate::playlist_snapshot::record_names_detached(
                pls.iter()
                    .map(|p| crate::playlist_snapshot::SnapshotNameEntry {
                        qobuz_playlist_id: p.id,
                        name: p.name.clone(),
                        owner: Some(p.owner.name.clone()).filter(|o| !o.is_empty()),
                        track_count: Some(p.tracks_count),
                    })
                    .collect(),
            );
            pls.into_iter()
                .map(|p| SidebarPlaylist {
                    id: p.id,
                    name: p.name.clone(),
                    description: p.description.clone().unwrap_or_default(),
                    tracks_count: p.tracks_count,
                    cover_urls: playlist_cover_urls(&p),
                    position: 0,
                })
                .collect()
        }
        Err(e) => {
            log::warn!("[qbz-slint] sidebar playlists load failed: {e}");
            Vec::new()
        }
    };
    // Folders (hidden folders excluded) + folder membership +
    // per-playlist custom-sort positions + hidden-playlist set + the
    // first-class LOCAL playlists + the per-playlist local sidecar counts
    // (all local, library.db) + OFFLINE only: the playlist-snapshot names
    // and the snapshot-available set (B7/B8).
    let (
        folders,
        folder_map,
        positions,
        hidden_playlists,
        local_playlists,
        local_counts,
        snapshot_names,
        snapshot_available,
    ) =
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
            // Hidden locals drop from the sidebar (B3) the way hidden Qobuz
            // playlists do — they stay reachable via the manager's "hidden"
            // filter, which reads the repo list directly.
            let local_playlists: Vec<LocalSidebarPlaylist> =
                crate::local_playlist::list_blocking()
                    .into_iter()
                    .filter(|p| !p.hidden)
                    .map(|p| LocalSidebarPlaylist {
                        id: p.id,
                        name: p.name,
                        description: p.description.unwrap_or_default(),
                        offline_only: p.offline_only,
                        folder_id: p.folder_id,
                        // Resolved below (async, off the blocking DB closure).
                        cover_urls: Vec::new(),
                    })
                    .collect();
            // B7/B8 (offline only): the snapshot names replace the
            // synthesized "Playlist (N local)" fallback, and the
            // snapshot-available set extends the D11.b visibility filter.
            let (snapshot_names, snapshot_available) =
                if crate::offline_mode::engine().is_offline() {
                    (
                        crate::playlist_snapshot::headers_blocking(),
                        crate::playlist_snapshot::available_offline_blocking(),
                    )
                } else {
                    (HashMap::new(), HashSet::new())
                };
            (
                folders,
                crate::folders::playlist_folder_map(),
                crate::folders::playlist_positions(),
                hidden_playlists,
                local_playlists,
                crate::folders::playlist_local_counts(),
                snapshot_names,
                snapshot_available,
            )
        })
        .await
        .unwrap_or_default();
    // Resolve up to 4 cover refs per LOCAL playlist for the sidebar micro-collage
    // (no network — local/Plex/cached-Qobuz covers from the playlist's tracks).
    // Done here in the async load (off the blocking DB closure above) so each
    // resolved set is cached in SidebarData; rebuild() reuses it without
    // re-resolving. Empty result = the row keeps its hard-drive glyph.
    let mut local_playlists = local_playlists;
    for lp in local_playlists.iter_mut() {
        lp.cover_urls = crate::local_playlist::resolve_cover_urls(&lp.id, 4).await;
    }

    let mut playlists = playlists;
    // D11.b — OFFLINE: the Qobuz fetch above is gate-refused (empty), so the
    // reachable playlists are synthesized locally: the MIXED ones (>= 1
    // local sidecar row) plus — B8 — the snapshot-available ones (>= 1
    // cached snapshot track). Names come from the previous load's session
    // cache, else the persisted snapshot (B7 — survives a cold offline
    // start), else the synthesized "Playlist (N local)" fallback.
    if crate::offline_mode::engine().is_offline() {
        let known: HashSet<u64> = playlists.iter().map(|p| p.id).collect();
        let prior: HashMap<u64, (String, String)> =
            NAME_DESC.lock().map(|nd| nd.clone()).unwrap_or_default();
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
            let snapshot = snapshot_names.get(&id);
            let (name, description) = prior
                .get(&id)
                .cloned()
                .or_else(|| snapshot.map(|(name, _)| (name.clone(), String::new())))
                .unwrap_or_else(|| {
                    (
                        qbz_i18n::t_args("Playlist ({} local)", &[&count.to_string()]),
                        String::new(),
                    )
                });
            // Track count: the snapshot's point-in-time Qobuz total when
            // known (the "# of tracks" sort key), else the local count.
            let tracks_count = snapshot.and_then(|(_, tc)| *tc).unwrap_or(count);
            playlists.push(SidebarPlaylist {
                id,
                name,
                description,
                tracks_count,
                cover_urls: Vec::new(),
                position: 0,
            });
        }
    }
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
        local_playlists,
        local_counts,
        snapshot_available,
    }
}

/// Name + description + offline_only for a LOCAL playlist id, from the
/// last loaded sidebar cache (the local twin of `playlist_name_desc`).
pub fn local_playlist_meta(id: &str) -> Option<(String, String, bool)> {
    CACHE.lock().ok().and_then(|c| {
        c.local_playlists
            .iter()
            .find(|p| p.id == id)
            .map(|p| (p.name.clone(), p.description.clone(), p.offline_only))
    })
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

/// Optimistically patch a playlist's displayed NAME after a successful
/// rename (Qobuz numeric id or "local:<uuid>"), then re-render from the
/// patched cache. The edit-playlist handler still triggers a full
/// `load_sidebar_playlists` afterwards to reconcile — this patch exists
/// because the reload alone does not reliably show the new name right away
/// (Qobuz playlist/list read-after-write lag): the row must reflect the
/// edit the moment the modal closes.
pub fn rename_entry(window: &AppWindow, id: &str, name: &str) {
    if let Ok(mut cache) = CACHE.lock() {
        if let Ok(numeric) = id.parse::<u64>() {
            if let Some(p) = cache.playlists.iter_mut().find(|p| p.id == numeric) {
                p.name = name.to_string();
            }
        }
        if let Some(p) = cache.local_playlists.iter_mut().find(|p| p.id == id) {
            p.name = name.to_string();
        }
    }
    // Keep the session name/desc cache in sync too — the edit modal and the
    // offline name synthesis both prefill from it.
    if let (Ok(numeric), Ok(mut nd)) = (id.parse::<u64>(), NAME_DESC.lock()) {
        if let Some(entry) = nd.get_mut(&numeric) {
            entry.0 = name.to_string();
        }
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
        local_kind: "".into(),
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

/// Build a LOCAL playlist row. `indent`/`folder_id` place it under a folder
/// (root row when `folder_id` is ""). A micro-collage is shown when the
/// playlist resolved >= 1 track cover; otherwise the row falls back to the
/// hard-drive glyph (`local_kind` stays set so the glyph branch still applies
/// when there are no covers).
fn local_playlist_entry(p: &LocalSidebarPlaylist, indent: bool, folder_id: &str) -> SidebarEntry {
    let url = |i: usize| -> slint::SharedString {
        p.cover_urls.get(i).cloned().unwrap_or_default().into()
    };
    SidebarEntry {
        kind: "playlist".into(),
        id: p.id.clone().into(),
        name: p.name.clone().into(),
        expanded: false,
        count: 0,
        indent,
        folder_id: folder_id.into(),
        local_kind: if p.offline_only { "offline" } else { "local" }.into(),
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
    let offline = crate::offline_mode::engine().is_offline();
    let sorted = sort_playlists(&data.playlists);
    let mut entries: Vec<SidebarEntry> = sorted
        .iter()
        .filter(|p| {
            data.folder_map
                .get(&p.id)
                .map(|f| f.as_str() == folder_id)
                .unwrap_or(false)
        })
        .filter(|p| !data.hidden_playlists.contains(&p.id))
        .filter(|p| offline_visible(&data, offline, p))
        .map(|p| playlist_entry(p, true, folder_id))
        .collect();
    // Local playlists assigned to this folder, name-sorted, appended after.
    let mut local_members: Vec<&LocalSidebarPlaylist> = data
        .local_playlists
        .iter()
        .filter(|p| p.folder_id.as_deref() == Some(folder_id))
        .collect();
    local_members.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    for p in local_members {
        entries.push(local_playlist_entry(p, true, folder_id));
    }
    window
        .global::<SidebarFolderPopupState>()
        .set_playlists(ModelRc::new(VecModel::from(entries)));
}

/// D11.b visibility: ONLINE every playlist shows; OFFLINE the MIXED ones
/// (>= 1 local sidecar track) stay, plus — B8 — the snapshot-available ones
/// (>= 1 cached snapshot track). Everything else hides.
fn offline_visible(data: &SidebarData, offline: bool, p: &SidebarPlaylist) -> bool {
    !offline
        || data.local_counts.get(&p.id).copied().unwrap_or(0) > 0
        || data.snapshot_available.contains(&p.id)
}

/// Rebuild the flattened entries (+ the folders list for the
/// move-to-folder menu) from the cache + expand state, applying the
/// active sort and the playlist-name search filter.
pub fn rebuild(window: &AppWindow) {
    let data = CACHE.lock().map(|c| c.clone()).unwrap_or_default();
    let expanded = EXPANDED.lock().map(|e| e.clone()).unwrap_or_default();
    let query = SEARCH.lock().map(|q| q.clone()).unwrap_or_default();
    let searching = !query.is_empty();
    let offline = crate::offline_mode::engine().is_offline();
    let folder_ids: HashSet<&String> = data.folders.iter().map(|f| &f.id).collect();

    // Sort then filter by the playlist-name query (recursive — the same
    // filter applies to playlists nested in folders).
    let sorted = sort_playlists(&data.playlists);
    let matches = |p: &SidebarPlaylist| !searching || p.name.to_lowercase().contains(&query);

    // Local playlists matching the search, grouped by their folder membership
    // (the local twin of `folder_map`). Locals are never offline-gated — they
    // don't depend on a Qobuz fetch.
    let local_matches = |p: &LocalSidebarPlaylist| !searching || p.name.to_lowercase().contains(&query);

    let mut entries: Vec<SidebarEntry> = Vec::new();
    for folder in &data.folders {
        let members: Vec<&SidebarPlaylist> = sorted
            .iter()
            .filter(|p| data.folder_map.get(&p.id).map(|f| f == &folder.id).unwrap_or(false))
            .filter(|p| matches(p))
            .filter(|p| !data.hidden_playlists.contains(&p.id))
            .filter(|p| offline_visible(&data, offline, p))
            .collect();
        // Local playlists assigned to THIS folder, name-sorted.
        let mut local_members: Vec<&LocalSidebarPlaylist> = data
            .local_playlists
            .iter()
            .filter(|p| p.folder_id.as_deref() == Some(folder.id.as_str()))
            .filter(|p| local_matches(p))
            .collect();
        local_members.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        // While searching, skip folders with no matching playlists
        // (mirrors Tauri's `if (isSearching && folderPlaylists.length === 0) continue`).
        // Offline the same rule hides folders whose members are all filtered
        // out (D11.b) — an empty folder header carries no information there.
        // Locals count as members for both gates.
        if (searching || offline) && members.is_empty() && local_members.is_empty() {
            continue;
        }
        // When searching, force-expand so matches inside are visible.
        let is_exp = searching || expanded.contains(&folder.id);
        entries.push(SidebarEntry {
            kind: "folder".into(),
            id: folder.id.clone().into(),
            name: folder.name.clone().into(),
            expanded: is_exp,
            count: (members.len() + local_members.len()) as i32,
            indent: false,
            folder_id: "".into(),
            local_kind: "".into(),
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
            for p in local_members {
                entries.push(local_playlist_entry(p, true, &folder.id));
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
        if !in_folder
            && matches(p)
            && !data.hidden_playlists.contains(&p.id)
            && offline_visible(&data, offline, p)
        {
            entries.push(playlist_entry(p, false, ""));
        }
    }
    // LOCAL playlists (library.db, D7) NOT in a folder (or in one that no
    // longer exists) — root rows after the Qobuz set, name-sorted, honoring
    // the same search filter. Folder-assigned locals were already emitted
    // under their folder header above. Always present, online or offline.
    {
        let mut locals: Vec<&LocalSidebarPlaylist> = data
            .local_playlists
            .iter()
            .filter(|p| {
                let in_folder = p
                    .folder_id
                    .as_ref()
                    .map(|f| folder_ids.contains(f))
                    .unwrap_or(false);
                !in_folder && local_matches(p)
            })
            .collect();
        locals.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        for p in locals {
            entries.push(local_playlist_entry(p, false, ""));
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
/// Returns `(qobuz_jobs, local_jobs)`. Qobuz playlist covers are http(s) URLs
/// (HTTP cache loader); LOCAL playlist covers are filesystem paths / Plex thumb
/// paths (the local-or-Plex loader). They're split by the row's `local_kind` so
/// each set goes to the right loader — a file path sent through the HTTP loader
/// would silently fail to decode.
pub fn artwork_jobs(
    window: &AppWindow,
) -> (Vec<crate::artwork::ArtworkJob>, Vec<crate::artwork::ArtworkJob>) {
    use slint::Model;
    let mut qobuz_jobs = Vec::new();
    let mut local_jobs = Vec::new();
    let entries = window.global::<SidebarState>().get_entries();
    for idx in 0..entries.row_count() {
        let Some(e) = entries.row_data(idx) else { continue };
        if e.kind != "playlist" {
            continue;
        }
        let is_local = !e.local_kind.is_empty();
        let urls = [e.url1, e.url2, e.url3, e.url4];
        for (slot, url) in urls.iter().enumerate() {
            if !url.is_empty() {
                let job = crate::artwork::ArtworkJob {
                    target: crate::artwork::ArtworkTarget::SidebarPlaylistCover { idx, slot },
                    url: url.to_string(),
                };
                if is_local {
                    local_jobs.push(job);
                } else {
                    qobuz_jobs.push(job);
                }
            }
        }
    }
    (qobuz_jobs, local_jobs)
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

/// Optimistically move a LOCAL playlist (`local:<uuid>` id) into a folder
/// (`folder_id` "" = root) in the cache, then re-render. The DB write happens
/// separately. The local twin of `move_playlist_local`.
pub fn move_local_playlist_local(window: &AppWindow, id: &str, folder_id: &str) {
    if let Ok(mut cache) = CACHE.lock() {
        if let Some(p) = cache.local_playlists.iter_mut().find(|p| p.id == id) {
            p.folder_id = if folder_id.is_empty() {
                None
            } else {
                Some(folder_id.to_string())
            };
        }
    }
    rebuild(window);
}

/// Highlight the open playlist in the sidebar (or clear with "").
pub fn set_active(window: &AppWindow, id: &str) {
    window.global::<SidebarState>().set_active_id(id.into());
}

// (removed `contains` — playlist ownership/follow is now decided by the Qobuz
// owner id vs the current user + get_user_playlists membership, not by sidebar
// presence, which is only a CONSEQUENCE of following. See main.rs playlist load.)
