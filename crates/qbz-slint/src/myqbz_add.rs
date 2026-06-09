//! "Add to Mixtape/Collection" controller — the Rust side of the global
//! `AddToMixtapeModal` (spec 21/50). Every app-wide "Add to Mixtape/Collection"
//! trigger builds an [`AddItem`] payload (or a batch) and calls [`open`]; this
//! module holds the pending items in a process-global, loads the picker list
//! (kind-restricted + recency-sorted + `item_exists`-resolved), and on pick /
//! create-and-add writes the items into the chosen collection via the shared
//! `qbz_mixtape::repo` (reached through `crate::library_db::with_db` +
//! `with_connection` — no Tauri command wrappers, ADR-005/006).
//!
//! Dedup is the backend's job: `add_item_with(allow_duplicate=false)` returns
//! `false` for an exact `(collection_id, source, source_item_id)` duplicate
//! (not an error). We count `added` vs `skipped` and surface the outcome via a
//! toast ("Added N to {name}" / "Already in {name}"), mirroring Tauri's
//! `toastBatchResult` + the dup flow's net result.

use std::sync::{LazyLock, Mutex};

use qbz_models::mixtape::{AlbumSource, CollectionKind, ItemType, MixtapeCollection};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AppWindow, MyQbzAddRow, MyQbzAddState, ToastKind};

/// One pending item to add. Built by each callsite from its row/album/playlist
/// data (spec 50 §0.2). `source_item_id` is ALWAYS a string (numeric track ids
/// are stringified by the caller).
#[derive(Clone)]
pub struct AddItem {
    /// "album" | "track" | "playlist".
    pub item_type: String,
    /// "qobuz" | "local" (Plex rows pass "local" — there is no "plex" source).
    pub source: String,
    pub source_item_id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub artwork_url: Option<String>,
    pub year: Option<i32>,
    pub track_count: Option<i32>,
}

/// Pending items for the currently-open picker. Set by [`open`], read by the
/// add/create handlers. Cleared on close.
static PENDING: LazyLock<Mutex<Vec<AddItem>>> = LazyLock::new(|| Mutex::new(Vec::new()));

fn pending_snapshot() -> Vec<AddItem> {
    PENDING.lock().map(|p| p.clone()).unwrap_or_default()
}

// ──────────────────────────── enum mapping ────────────────────────────

fn item_type_from_str(s: &str) -> ItemType {
    match s {
        "track" => ItemType::Track,
        "playlist" => ItemType::Playlist,
        _ => ItemType::Album,
    }
}

fn source_from_str(s: &str) -> AlbumSource {
    match s {
        "local" => AlbumSource::Local,
        _ => AlbumSource::Qobuz,
    }
}

// ──────────────────────────── open / payload ──────────────────────────

/// Open the picker for one or more items. Empty input is a no-op (mirrors
/// `openAddToMixtape([])`). Stores the payload, computes the header strings +
/// kind-restriction, marks loading, and shows the modal. UI thread.
///
/// The caller is responsible for spawning the row load afterwards (it needs the
/// tokio handle + a worker thread for the DB read); the wiring in `main.rs`
/// does `open(...)` then spawns [`load_rows`] → [`apply_rows`].
pub fn open(window: &AppWindow, items: Vec<AddItem>) {
    if items.is_empty() {
        return;
    }

    let bulk = items.len() > 1;
    // restrict to mixtapes if ANY pending item is a track/playlist (collections
    // hold whole albums only).
    let restrict = items.iter().any(|it| it.item_type != "album");

    let first = &items[0];
    let header_title: String = if bulk {
        format!("{} items", items.len())
    } else {
        first.title.clone()
    };
    let header_subtitle: String = if bulk {
        // "{first title} + N more" — hardcoded English suffix (1:1 with PSD).
        format!("{} + {} more", first.title, items.len() - 1)
    } else {
        first.subtitle.clone().unwrap_or_default()
    };

    if let Ok(mut p) = PENDING.lock() {
        *p = items;
    }

    let state = window.global::<MyQbzAddState>();
    state.set_rows(ModelRc::new(VecModel::from(Vec::<MyQbzAddRow>::new())));
    state.set_header_title(header_title.into());
    state.set_header_subtitle(header_subtitle.into());
    state.set_bulk_mode(bulk);
    state.set_restrict_to_mixtape(restrict);
    state.set_search("".into());
    state.set_busy_id("".into());
    state.set_creating(false);
    state.set_create_name("".into());
    state.set_create_kind("mixtape".into());
    state.set_loading(true);
    state.set_open(true);
}

/// Close the picker + clear the pending payload. UI thread.
pub fn close(window: &AppWindow) {
    if let Ok(mut p) = PENDING.lock() {
        p.clear();
    }
    let state = window.global::<MyQbzAddState>();
    state.set_open(false);
    state.set_creating(false);
    state.set_busy_id("".into());
}

// ──────────────────────────── row loading ─────────────────────────────

/// A loaded picker row (the collection + whether it already contains every
/// pending item). Built on a worker thread by [`load_rows`].
pub struct LoadedRow {
    pub id: String,
    pub name: String,
    pub kind: CollectionKind,
    pub item_count: usize,
    /// True when EVERY pending item already exists in this collection.
    pub already_has: bool,
}

/// Load the collections offered as targets, kind-restricted + recency-sorted +
/// `item_exists`-resolved. Blocking (DB) — run on a worker thread.
///
/// - `restrict_to_mixtape` → only `kind == mixtape` (excludes collections AND
///   artist_collections, the latter never a user target).
/// - sort = `last_played_at ?? updated_at` DESC (most-recently-played, then
///   most-recently-updated), matching `sortedCollections` in the PSD.
/// - `already_has` = every pending item's `(source, source_item_id)` already in
///   the collection (so the row can show an "already added" hint).
pub fn load_rows(restrict_to_mixtape: bool, items: &[AddItem]) -> Vec<LoadedRow> {
    crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| {
            let mut cols: Vec<MixtapeCollection> =
                qbz_mixtape::repo::list_collections(conn, None).unwrap_or_else(|e| {
                    log::warn!("[qbz-slint] myqbz_add list_collections failed: {e}");
                    Vec::new()
                });

            // Kind restriction.
            if restrict_to_mixtape {
                cols.retain(|c| c.kind == CollectionKind::Mixtape);
            } else {
                // An album can be added to ANY collection kind — Mixtape,
                // Collection, OR Artist Collection. Tauri allows adding an album
                // to an artist_collection (the user can augment a built
                // discography), so no kind restriction here.
            }

            // Sort by last_played_at ?? updated_at DESC.
            cols.sort_by(|a, b| {
                let ra = a.last_played_at.unwrap_or(a.updated_at);
                let rb = b.last_played_at.unwrap_or(b.updated_at);
                rb.cmp(&ra)
            });

            cols.into_iter()
                .map(|c| {
                    // already_has = every pending item is already present.
                    let already_has = !items.is_empty()
                        && items.iter().all(|it| {
                            qbz_mixtape::repo::item_exists(
                                conn,
                                &c.id,
                                source_from_str(&it.source),
                                &it.source_item_id,
                            )
                            .unwrap_or(false)
                        });
                    LoadedRow {
                        id: c.id,
                        name: c.name,
                        kind: c.kind,
                        item_count: c.items.len(),
                        already_has,
                    }
                })
                .collect::<Vec<_>>()
        }))
    })
    .unwrap_or_default()
}

fn kind_icon(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Mixtape => "cassette",
        CollectionKind::ArtistCollection => "user",
        CollectionKind::Collection => "library-big",
    }
}

fn kind_label(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Mixtape => "MIXTAPE",
        CollectionKind::Collection => "COLLECTION",
        CollectionKind::ArtistCollection => "ARTIST",
    }
}

/// "N albums" / "1 album" (always "album(s)" regardless of item_type — 1:1 PSD).
fn album_count_label(count: usize) -> String {
    if count == 1 {
        "1 album".to_string()
    } else {
        format!("{count} albums")
    }
}

/// Render loaded rows into `MyQbzAddState`, applying the active search filter.
/// UI thread.
pub fn apply_rows(window: &AppWindow, rows: Vec<LoadedRow>) {
    // Stash so a later search re-filters without a DB refetch.
    if let Ok(mut c) = ROWS_CACHE.lock() {
        *c = rows;
    }
    rebuild(window);
    window.global::<MyQbzAddState>().set_loading(false);
}

/// Last-loaded rows (so search filters client-side, no refetch).
static ROWS_CACHE: LazyLock<Mutex<Vec<LoadedRow>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Rebuild the visible row model from the cache honoring the search filter.
pub fn rebuild(window: &AppWindow) {
    let state = window.global::<MyQbzAddState>();
    let query = state.get_search().trim().to_lowercase();
    let cache = ROWS_CACHE.lock();
    let items: Vec<MyQbzAddRow> = cache
        .as_ref()
        .map(|rows| {
            rows.iter()
                .filter(|r| query.is_empty() || r.name.to_lowercase().contains(&query))
                .map(|r| MyQbzAddRow {
                    id: r.id.clone().into(),
                    name: r.name.clone().into(),
                    kind: match r.kind {
                        CollectionKind::Mixtape => "mixtape",
                        CollectionKind::Collection => "collection",
                        CollectionKind::ArtistCollection => "artist_collection",
                    }
                    .into(),
                    icon: kind_icon(r.kind).into(),
                    kind_label: kind_label(r.kind).into(),
                    meta: album_count_label(r.item_count).into(),
                    already_has: r.already_has,
                })
                .collect()
        })
        .unwrap_or_default();
    state.set_rows(ModelRc::new(VecModel::from(items)));
}

// ──────────────────────────── add / create ────────────────────────────

/// Result of a batch insert: how many were inserted and how many were skipped
/// as duplicates.
pub struct AddOutcome {
    pub added: usize,
    pub skipped: usize,
}

/// Insert every pending item into `collection_id` with `allow_duplicate=false`.
/// Blocking (DB). Returns the added/skipped tally (a `false` return from the
/// repo = a dedup-rejected duplicate, NOT an error).
pub fn add_items(collection_id: &str, items: &[AddItem]) -> AddOutcome {
    let mut added = 0usize;
    let mut skipped = 0usize;
    crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| {
            for it in items {
                match qbz_mixtape::repo::add_item_with(
                    conn,
                    collection_id,
                    item_type_from_str(&it.item_type),
                    source_from_str(&it.source),
                    &it.source_item_id,
                    &it.title,
                    it.subtitle.as_deref(),
                    it.artwork_url.as_deref(),
                    it.year,
                    it.track_count,
                    false,
                ) {
                    Ok(true) => added += 1,
                    Ok(false) => skipped += 1,
                    Err(e) => {
                        log::warn!("[qbz-slint] myqbz_add add_item failed: {e}");
                    }
                }
            }
        }))
    });
    AddOutcome { added, skipped }
}

/// Surface the add outcome via a toast (matches Tauri's `toastBatchResult` net
/// behavior). `name` is the collection name.
pub fn toast_outcome(window: &AppWindow, name: &str, outcome: &AddOutcome) {
    let msg = if outcome.added == 0 {
        // Nothing inserted -> everything was a duplicate ("Already in {name}").
        format!("Already in {name}")
    } else if outcome.skipped > 0 {
        format!(
            "Added {} to {name} ({} duplicate(s) skipped)",
            outcome.added, outcome.skipped
        )
    } else {
        format!("Added {} to {name}", outcome.added)
    };
    let kind = if outcome.added == 0 {
        ToastKind::Info
    } else {
        ToastKind::Success
    };
    crate::toast::show(window, msg, kind);
}

/// Take a snapshot of the pending items (clone). Used by the action handlers in
/// `main.rs` to hand the payload to a blocking worker.
pub fn take_pending() -> Vec<AddItem> {
    pending_snapshot()
}

/// Build `track` payloads from a batch of LocalTracks (source "local" — Plex
/// rows included, per spec 50: there is no "plex" source). Subtitle =
/// "artist · album"; no artwork_url / year / track_count (1:1 PSD §R).
pub fn track_items_from_local(tracks: &[qbz_library::LocalTrack]) -> Vec<AddItem> {
    tracks
        .iter()
        .map(|t| {
            let subtitle = [t.artist.clone(), t.album.clone()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            AddItem {
                item_type: "track".into(),
                source: "local".into(),
                source_item_id: t.id.to_string(),
                title: t.title.clone(),
                subtitle: (!subtitle.is_empty()).then_some(subtitle),
                artwork_url: None,
                year: None,
                track_count: None,
            }
        })
        .collect()
}

/// Create a new manual collection of `kind` named `name`, returning
/// `(id, name)` on success. Blocking (DB). `kind` is "mixtape" | "collection".
pub fn create_collection(kind: &str, name: &str) -> Option<(String, String)> {
    let kind = match kind {
        "collection" => CollectionKind::Collection,
        _ => CollectionKind::Mixtape,
    };
    crate::myqbz::create_collection(kind, name).map(|c| (c.id, c.name))
}
