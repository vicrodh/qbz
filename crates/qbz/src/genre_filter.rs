//! Filter-by-genre controller.
//!
//! Loads the parent genres for the popup's simple grid and owns the genre
//! selection. The selection is **per context** ("discover" for the three
//! Discover tabs, "favorites" for the favorites tabs) so the two surfaces
//! filter independently (Tauri keeps them separate too). The popup edits
//! whatever context is `current` (set when it opens). The selection
//! persists to `<data-dir>/qbz/genre_filter.json` when "Remember
//! selection" is on, and feeds `genre_ids` into the discover-index fetch /
//! the favorites client-side genre filter.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AppWindow, GenreChip, GenreFilterState, GenreTreeRow};

#[derive(Clone)]
struct GenreItem {
    id: u64,
    name: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Persisted {
    /// Per-context selections ("discover" / "favorites" / ...).
    #[serde(default)]
    contexts: HashMap<String, Vec<u64>>,
    /// Legacy single-list selection — migrated into the "discover" context.
    #[serde(default)]
    selected: Vec<u64>,
    #[serde(default = "default_true")]
    remember: bool,
}

fn default_true() -> bool {
    true
}

struct State {
    parents: Vec<GenreItem>,
    /// Lazily loaded children, keyed by parent id (levels 2 and 3).
    children: HashMap<u64, Vec<GenreItem>>,
    /// Selected genre ids per context.
    selected: HashMap<String, Vec<u64>>,
    /// The context the popup is currently editing.
    current: String,
    expanded: HashSet<u64>,
    search: String,
    remember: bool,
}

impl State {
    /// Mutable handle to the current context's selection (created if absent).
    fn cur_mut(&mut self) -> &mut Vec<u64> {
        let key = self.current.clone();
        self.selected.entry(key).or_default()
    }
    fn is_selected(&self, id: u64) -> bool {
        self.selected
            .get(&self.current)
            .map(|v| v.contains(&id))
            .unwrap_or(false)
    }
    fn cur_len(&self) -> usize {
        self.selected
            .get(&self.current)
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        parents: Vec::new(),
        children: HashMap::new(),
        selected: HashMap::new(),
        current: "discover".to_string(),
        expanded: HashSet::new(),
        search: String::new(),
        remember: true,
    })
});

fn store_path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("qbz").join("genre_filter.json"))
}

fn load_persisted() -> Persisted {
    let Some(path) = store_path() else {
        return Persisted::default();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Persisted::default(),
    }
}

fn save_persisted(contexts: &HashMap<String, Vec<u64>>, remember: bool) {
    let Some(path) = store_path() else {
        return;
    };
    if !remember {
        // Remember off — drop any persisted selection.
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let data = Persisted {
        contexts: contexts.clone(),
        selected: Vec::new(),
        remember,
    };
    if let Ok(json) = serde_json::to_vec_pretty(&data) {
        let _ = std::fs::write(&path, json);
    }
}

/// Set the context the popup edits (call when opening it for a surface).
pub fn set_context(ctx: &str) {
    if let Ok(mut s) = STATE.lock() {
        s.current = ctx.to_string();
        s.selected.entry(ctx.to_string()).or_default();
    }
}

/// The context the popup is currently editing.
pub fn current_context() -> String {
    STATE
        .lock()
        .map(|s| s.current.clone())
        .unwrap_or_else(|_| "discover".to_string())
}

/// The explicitly-selected genre ids in the current popup context.
pub fn selected_ids() -> Vec<u64> {
    STATE
        .lock()
        .map(|s| s.selected.get(&s.current).cloned().unwrap_or_default())
        .unwrap_or_default()
}

/// The RAW genre selection for `ctx` (no expansion, no ancestor mapping):
/// the exact ids the user toggled, parent or sub-genre. This is what gets
/// sent to /discover/* in `genre_ids` — Qobuz honors sub-genre ids
/// server-side (1:1 with Tauri discovery-v2, which sent the raw selection
/// straight through and did no client-side narrowing).
pub fn selected_ids_for(ctx: &str) -> Vec<u64> {
    STATE
        .lock()
        .map(|s| s.selected.get(ctx).cloned().unwrap_or_default())
        .unwrap_or_default()
}

/// Selected genre NAMES (+ descendant names) for `ctx` — for the
/// client-side album / track genre filter used by favorites.
pub fn selected_names(ctx: &str) -> Vec<String> {
    let Ok(s) = STATE.lock() else {
        return Vec::new();
    };
    let mut ids: HashSet<u64> = HashSet::new();
    if let Some(sel) = s.selected.get(ctx) {
        for id in sel {
            ids.insert(*id);
            collect_descendants(&s.children, *id, &mut ids);
        }
    }
    let mut names: Vec<String> = Vec::new();
    for id in ids {
        if let Some(g) = s.parents.iter().find(|g| g.id == id) {
            names.push(g.name.clone());
        } else if let Some(g) = s.children.values().flatten().find(|g| g.id == id) {
            names.push(g.name.clone());
        }
    }
    names
}

fn collect_descendants(
    children: &HashMap<u64, Vec<GenreItem>>,
    id: u64,
    out: &mut HashSet<u64>,
) {
    if let Some(kids) = children.get(&id) {
        for kid in kids {
            if out.insert(kid.id) {
                collect_descendants(children, kid.id, out);
            }
        }
    }
}

pub fn children_loaded(id: u64) -> bool {
    STATE.lock().map(|s| s.children.contains_key(&id)).unwrap_or(false)
}

fn store_children(parent_id: u64, kids: Vec<GenreItem>) {
    if let Ok(mut s) = STATE.lock() {
        s.children.insert(parent_id, kids);
    }
}

/// Fetch the parent genres (if not already loaded) and seed the persisted
/// selection. Runs on a worker; call apply_state afterwards on the UI
/// thread.
pub async fn load_parents<A>(runtime: &AppRuntime<A>)
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    {
        let already = STATE.lock().map(|s| !s.parents.is_empty()).unwrap_or(false);
        if already {
            return;
        }
    }
    let persisted = load_persisted();
    let mut parents: Vec<GenreItem> = match runtime.core().get_genres(None).await {
        Ok(list) => list
            .into_iter()
            .map(|g| GenreItem {
                id: g.id,
                name: g.name,
            })
            .collect(),
        Err(e) => {
            log::warn!("[qbz-slint] genre filter: get_genres failed: {e}");
            Vec::new()
        }
    };
    parents.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Keep persisted selections as-is — they may reference child genres
    // not yet loaded (advanced view), so validating against parents only
    // would wrongly drop them.
    if let Ok(mut s) = STATE.lock() {
        s.parents = parents;
        let mut contexts = persisted.contexts;
        // Migrate a legacy flat selection into the discover context.
        if contexts.is_empty() && !persisted.selected.is_empty() {
            contexts.insert("discover".to_string(), persisted.selected);
        }
        s.selected = contexts;
        s.remember = persisted.remember;
    }
}

/// Load one genre level (children of `parent_id`) and store it. No-op if
/// already loaded.
pub async fn load_children<A>(runtime: &AppRuntime<A>, parent_id: u64)
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    if children_loaded(parent_id) {
        return;
    }
    let kids: Vec<GenreItem> = match runtime.core().get_genres(Some(parent_id)).await {
        Ok(list) => list
            .into_iter()
            .map(|g| GenreItem {
                id: g.id,
                name: g.name,
            })
            .collect(),
        Err(e) => {
            log::warn!("[qbz-slint] genre filter: get_genres({parent_id}) failed: {e}");
            Vec::new()
        }
    };
    store_children(parent_id, kids);
}

fn child_ids(parent_id: u64) -> Vec<u64> {
    STATE
        .lock()
        .ok()
        .and_then(|s| s.children.get(&parent_id).map(|k| k.iter().map(|c| c.id).collect()))
        .unwrap_or_default()
}

/// Eager-load every parent's children (level 2) so the advanced tree can
/// show child counts up front. Grandchildren stay lazy.
pub async fn load_all_parent_children<A>(runtime: &AppRuntime<A>)
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let parents: Vec<u64> = STATE
        .lock()
        .map(|s| s.parents.iter().map(|p| p.id).collect())
        .unwrap_or_default();
    for parent_id in parents {
        load_children(runtime, parent_id).await;
    }
}

/// Eager-load a genre's full descendant subtree (children + grandchildren),
/// so a selection expands correctly in selected_names (favorites) and the
/// tree shows counts.
pub async fn load_descendants<A>(runtime: &AppRuntime<A>, id: u64)
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    load_children(runtime, id).await;
    for kid in child_ids(id) {
        load_children(runtime, kid).await;
    }
}

/// Toggle a tree node's expanded state. Returns true if it is now expanded
/// (so the caller can lazy-load its children).
pub fn toggle_expand(id_str: &str) -> bool {
    let Ok(id) = id_str.parse::<u64>() else {
        return false;
    };
    let Ok(mut s) = STATE.lock() else {
        return false;
    };
    if s.expanded.contains(&id) {
        s.expanded.remove(&id);
        false
    } else {
        s.expanded.insert(id);
        true
    }
}

pub fn set_search(query: &str) {
    if let Ok(mut s) = STATE.lock() {
        s.search = query.to_string();
    }
}

/// Push the current parents + selection + tree into GenreFilterState (for
/// the current context). UI thread.
pub fn apply_state(window: &AppWindow) {
    let (chips, rows, count, remember) = {
        let Ok(s) = STATE.lock() else {
            return;
        };
        let chips: Vec<GenreChip> = s
            .parents
            .iter()
            .map(|g| GenreChip {
                id: g.id.to_string().into(),
                name: g.name.clone().into(),
                selected: s.is_selected(g.id),
            })
            .collect();
        (chips, build_tree_rows(&s), s.cur_len() as i32, s.remember)
    };
    let state = window.global::<GenreFilterState>();
    state.set_genres(ModelRc::new(VecModel::from(chips)));
    state.set_tree(ModelRc::new(VecModel::from(rows)));
    state.set_selected_count(count);
    state.set_remember(remember);
}

fn tree_row(item: &GenreItem, level: i32, s: &State) -> GenreTreeRow {
    let loaded = s.children.get(&item.id);
    let count = loaded.map(|c| c.len()).unwrap_or(0);
    // Parents always have children; deeper levels show an expand arrow
    // optimistically until a load proves them empty.
    let has_children = if level == 0 {
        true
    } else if level == 1 {
        count > 0 || loaded.is_none()
    } else {
        false
    };
    GenreTreeRow {
        id: item.id.to_string().into(),
        name: item.name.clone().into(),
        level,
        selected: s.is_selected(item.id),
        expanded: s.expanded.contains(&item.id),
        has_children,
        count: count as i32,
    }
}

/// Flatten the genre tree into the currently-visible rows. With a search
/// query, returns a flat list of all loaded genres matching the query
/// (ignoring expansion); otherwise honors per-node expansion down three
/// levels.
fn build_tree_rows(s: &State) -> Vec<GenreTreeRow> {
    let query = s.search.trim().to_lowercase();
    let mut rows: Vec<GenreTreeRow> = Vec::new();

    if !query.is_empty() {
        // Search rows are a flat list that ignores expansion — never show
        // an expand chevron (level 0 would otherwise force has_children on
        // child matches).
        let matches = |g: &GenreItem| g.name.to_lowercase().contains(&query);
        let flat_row = |g: &GenreItem| {
            let mut row = tree_row(g, 0, s);
            row.has_children = false;
            row
        };
        for p in &s.parents {
            if matches(p) {
                rows.push(flat_row(p));
            }
        }
        for kids in s.children.values() {
            for k in kids {
                if matches(k) {
                    rows.push(flat_row(k));
                }
            }
        }
        return rows;
    }

    for parent in &s.parents {
        rows.push(tree_row(parent, 0, s));
        if !s.expanded.contains(&parent.id) {
            continue;
        }
        let Some(children) = s.children.get(&parent.id) else {
            continue;
        };
        for child in children {
            rows.push(tree_row(child, 1, s));
            if !s.expanded.contains(&child.id) {
                continue;
            }
            if let Some(grandchildren) = s.children.get(&child.id) {
                for gc in grandchildren {
                    rows.push(tree_row(gc, 2, s));
                }
            }
        }
    }
    rows
}

/// Toggle a genre id in the current context's selection. Returns true if
/// the selection changed (so the caller can re-fetch / re-derive).
pub fn toggle(id_str: &str) -> bool {
    let Ok(id) = id_str.parse::<u64>() else {
        return false;
    };
    let Ok(mut s) = STATE.lock() else {
        return false;
    };
    {
        let sel = s.cur_mut();
        if let Some(pos) = sel.iter().position(|x| *x == id) {
            sel.remove(pos);
        } else {
            sel.push(id);
        }
    }
    let (contexts, rem) = (s.selected.clone(), s.remember);
    drop(s);
    save_persisted(&contexts, rem);
    true
}

pub fn clear() {
    let Ok(mut s) = STATE.lock() else {
        return;
    };
    s.cur_mut().clear();
    let (contexts, rem) = (s.selected.clone(), s.remember);
    drop(s);
    save_persisted(&contexts, rem);
}

pub fn set_remember(remember: bool) {
    let Ok(mut s) = STATE.lock() else {
        return;
    };
    s.remember = remember;
    let contexts = s.selected.clone();
    drop(s);
    save_persisted(&contexts, remember);
}
