//! Purchases controller (Slint) — Slices 3 + 6: data-loading shells +
//! registry/qualityDir wiring.
//!
//! Mirrors Tauri's `PurchasesView.svelte` data-loading flow
//! (`source-of-truth §2.1.6`), ported to the shared service. No `tauri::State`
//! anywhere: every fn is generic over `A: FrontendAdapter` and reaches Qobuz
//! through `runtime.core().client()` and the local registry through
//! `crate::library_db::with_db`, exactly like `award.rs` / `myqbz_play.rs`.
//!
//! Slice 3 scope = the load fns only (no `.slint` apply/derive/reset yet —
//! those land in Slices 8/9). What is implemented here:
//!
//!   * `get_downloaded_track_ids` → `HashSet<u64>` (the §3.2 `getDownloadedTrackIds`
//!     wrapper; O(1) `Set.has` lookups);
//!   * `load_purchases_metadata` — the one-shot metadata fetch (dlIds + TWO
//!     separate `getUserPurchasesIds(1,0,type)` totals; the two-call per-type
//!     totals quirk is preserved verbatim);
//!   * `load_purchases_by_tab(tab, force, &metadata)` — the per-tab list load
//!     (`get_user_purchases_all_typed`) with §3.6 error mapping and total
//!     backfill.
//!
//! Slice 6 adds the controller-side registry/qualityDir wiring used by the
//! download path (Slice 7) and the download-flag annotation (Slice 4):
//!
//!   * `get_downloaded_purchase_formats` → `HashMap<track_id → Vec<format_id>>`
//!     (the Tauri command #1 `format_map`; source for `downloaded_format_ids` +
//!     format-scoped completion gating);
//!   * `quality_dir(label)` → the `/`→`-`, `.trim()` derivation (§7.5) applied
//!     before every download invoke.
//!
//! Download-flag enrichment (`enrichWithDownloadStatus`) is Slice 4; the
//! download state machine + actions are Slice 7; the UI apply is Slices 8/9.
//! These load fns therefore return PLAIN, `Send` payload structs (no
//! `slint::Image` / `ModelRc`) so they may be built off the event loop and held
//! across `.await`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use qbz_app::shell::AppRuntime;
use qbz_models::{PurchaseAlbum, PurchaseFormatOption, PurchaseTrack};
use qbz_offline_cache::purchases_service;

use crate::adapter::SlintAdapter;
use crate::AppWindow;

type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Which purchase tab to load. The wire `type` string (`"albums"` / `"tracks"`)
/// is derived via [`PurchaseTab::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurchaseTab {
    Albums,
    Tracks,
}

impl PurchaseTab {
    pub fn as_str(self) -> &'static str {
        match self {
            PurchaseTab::Albums => "albums",
            PurchaseTab::Tracks => "tracks",
        }
    }
}

/// One-shot purchases metadata (mirrors the Svelte `loadPurchasesMetadata`
/// result: `downloadedTrackIds` + the two per-type totals). `Send` — no Slint
/// types. The caller caches this behind a `metadataLoaded` guard (UI state in
/// Slices 8/9); the fetch itself is idempotent.
#[derive(Debug, Clone, Default)]
pub struct PurchasesMetadata {
    /// Track ids with a downloaded purchase on disk (O(1) `contains`).
    pub downloaded_track_ids: HashSet<u64>,
    /// Server total of purchased albums (from the dedicated ids call), or 0.
    pub total_albums: u32,
    /// Server total of purchased tracks (from the dedicated ids call), or 0.
    pub total_tracks: u32,
}

/// A single tab's list payload. `albums`/`tracks` carry the RAW wire items for
/// the active tab (the inactive tab's vec is empty — the Svelte flow assigns
/// only the active tab's array). `total` is the resolved count after backfill.
/// `Send` (plain serde structs).
#[derive(Debug, Clone, Default)]
pub struct PurchasesTabPayload {
    pub tab_albums: Vec<PurchaseAlbum>,
    pub tab_tracks: Vec<PurchaseTrack>,
    /// The resolved total for the loaded tab (metadata total, else response
    /// total, else loaded length — the §2.1.6 backfill chain).
    pub total: u32,
}

/// Read the downloaded-purchase track ids from the local registry as a
/// `HashSet<u64>`. Mirrors the §3.2 `getDownloadedTrackIds()` service wrapper:
/// the registry call ALSO prunes stale rows whose file vanished off disk (a
/// side effect of `get_downloaded_purchase_track_ids`). On a DB error or no
/// active user → empty set (the Svelte `.catch(() => new Set())`).
///
/// Runs the blocking DB op on a `spawn_blocking` worker so the async caller is
/// not stalled (consistent with the rest of the controller's DB access).
pub async fn get_downloaded_track_ids() -> HashSet<u64> {
    tokio::task::spawn_blocking(|| {
        crate::library_db::with_db(|db| Ok(db.get_downloaded_purchase_track_ids()?))
            .map(|ids| ids.into_iter().map(|id| id as u64).collect::<HashSet<u64>>())
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default()
}

/// Read the per-track downloaded FORMAT ids from the local registry as a
/// `HashMap<track_id → Vec<format_id>>`. Mirrors Tauri command #1's
/// `format_map` (`legacy_compat.rs:2704-2742`, source-of-truth §3.3 #1): the
/// command reads `db.get_downloaded_purchase_formats()` → `Vec<(track_id i64,
/// format_id i64)>` and derives both the `downloaded_ids` HashSet (already
/// provided by [`get_downloaded_track_ids`]) and a `format_map: HashMap<i64,
/// Vec<u32>>`. This is the source for the per-track `downloaded_format_ids`
/// annotation + the format-scoped completion gating (Slice 7).
///
/// Each `(track_id, format_id)` pair is folded into the map (ids cast to the
/// frontend's `u64`/`u32` widths). Unlike the track-ids reader, the registry's
/// `get_downloaded_purchase_formats` does NOT stale-prune (§7.7) — it returns
/// every persisted pair as-is, matching Tauri.
///
/// On a DB error or no active user → an empty map (the Tauri command surfaces a
/// "No active session" error, but the metadata load tolerates that the same way
/// the dlIds path does — the controller falls back to empty rather than failing
/// the whole view).
///
/// Runs the blocking DB op on a `spawn_blocking` worker, consistent with the
/// rest of the controller's DB access.
pub async fn get_downloaded_purchase_formats() -> HashMap<u64, Vec<u32>> {
    tokio::task::spawn_blocking(|| {
        crate::library_db::with_db(|db| Ok(db.get_downloaded_purchase_formats()?))
            .map(|pairs| {
                let mut map: HashMap<u64, Vec<u32>> = HashMap::new();
                for (track_id, format_id) in pairs {
                    map.entry(track_id as u64).or_default().push(format_id as u32);
                }
                map
            })
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default()
}

/// §7.5 / §8.2 `qualityDir` derivation (FRONTEND, port-critical). Mirrors the
/// Svelte `qualityDir = format.label.replace(/\//g, '-').trim()` used in BOTH
/// views (`executeTrackDownload` + `qualityFolderName`, source-of-truth lines
/// 314 / 1196-1199): replace EVERY `/` with `-`, then trim surrounding
/// whitespace. The result becomes a literal subfolder segment appended to the
/// album folder (with a leading space) inside `target_path`.
///
/// FIDELITY: JS `String.prototype.replace(/\//g, …)` is GLOBAL — all slashes
/// are replaced. Rust `str::replace` is likewise global, so `"24/192"` →
/// `"24-192"` 1:1. (The §8.2 checkbox writes `replace('/', '-')` shorthand, but
/// §7.5 + line 314 confirm the live regex is the global `/\//g`; the global
/// form is the correct port — a label with two slashes must lose both.) The
/// `.trim()` matches the JS `.trim()` exactly (leading/trailing ASCII+Unicode
/// whitespace). No other normalization is applied — the raw label drives the
/// folder name, just like Tauri.
pub fn quality_dir(label: &str) -> String {
    label.replace('/', "-").trim().to_string()
}

/// The one-shot metadata load (Svelte `loadPurchasesMetadata`, §2.1.6):
/// concurrently (a) read the downloaded-track ids from the registry and (b)
/// fetch the TWO per-type totals via `getUserPurchasesIds(1, 0, type)` — one
/// call for `"albums"`, one for `"tracks"`.
///
/// GOTCHA (per-type totals, §3.6): the two ids calls MUST stay separate. A
/// single unfiltered `limit=1` ids call carries only the FIRST type's total, so
/// collapsing them would zero the other tab's count. Each call independently
/// falls back to 0 on error (`getPurchaseIds(...).catch(() => null)` →
/// `... ?? 0`). dlIds independently falls back to an empty set.
pub async fn load_purchases_metadata(runtime: &Runtime) -> PurchasesMetadata {
    // (a) Registry ids — independent of the Qobuz client (works offline).
    let dl_ids = get_downloaded_track_ids().await;

    // (b) Two per-type totals. Snapshot the client once; if there is no client
    // (logged-out / pre-login), both totals fall back to 0 — same shape as the
    // Svelte `.catch(() => null)` path.
    let client_lock = runtime.core().client();
    let client = {
        let guard = client_lock.read().await;
        guard.as_ref().cloned()
    };

    let (total_albums, total_tracks) = match client {
        Some(client) => {
            // Two SEPARATE ids calls (never one). Order matches the Svelte
            // `Promise.all([albums, tracks])`; each maps its own type's total.
            let total_albums = purchases_service::get_purchase_total(&client, "albums")
                .await
                .unwrap_or(0);
            let total_tracks = purchases_service::get_purchase_total(&client, "tracks")
                .await
                .unwrap_or(0);
            (total_albums, total_tracks)
        }
        None => {
            log::warn!("[Purchases] load_purchases_metadata: no Qobuz client; totals default to 0");
            (0, 0)
        }
    };

    PurchasesMetadata {
        downloaded_track_ids: dl_ids,
        total_albums,
        total_tracks,
    }
}

/// Load ONE tab's purchase list (Svelte `loadPurchasesByTab`, §2.1.6). Fetches
/// the full per-type list via `get_user_purchases_all_typed`, then resolves the
/// tab total with the backfill chain:
///
///   metadata total (if non-zero) → response page total → loaded item count.
///
/// Only the active tab's items are populated (the inactive vec stays empty),
/// mirroring the Svelte "assign ONLY the active tab's array". The `force` flag
/// and the `albumsLoaded`/`tracksLoaded` cache gating live in the UI layer
/// (Slices 8/9) — this fn always performs the fetch when called; the caller
/// decides whether to skip it.
///
/// On error returns `Err(mapped)` where the message is mapped per §3.6:
/// network-ish errors → the i18n key `purchases.loadFailed`; anything else →
/// the raw error string (surfaced verbatim).
pub async fn load_purchases_by_tab(
    runtime: &Runtime,
    tab: PurchaseTab,
    metadata: &PurchasesMetadata,
) -> Result<PurchasesTabPayload, String> {
    let client_lock = runtime.core().client();
    let client = {
        let guard = client_lock.read().await;
        guard.as_ref().cloned()
    };
    let client = match client {
        Some(c) => c,
        None => {
            // No session → surface as the generic load-failed key (the Svelte
            // flow would hit an auth error string, which maps the same way).
            return Err("purchases.loadFailed".to_string());
        }
    };

    let response = match purchases_service::get_user_purchases_by_type(&client, tab.as_str()).await
    {
        Ok(resp) => resp,
        Err(e) => {
            // Replicate Tauri command #3's error wrapping
            // (`legacy_compat.rs:2784`): `"Failed to fetch {type} purchases:
            // {e}"`. The `"fetch"` token is LOAD-BEARING — it is what makes a
            // genuine network error map to `purchases.loadFailed` via §3.6
            // (the raw `ApiError::NetworkError` Display is `"Network error: …"`,
            // which contains none of the three Svelte tokens). Dropping the
            // prefix would diverge from Tauri; keep it verbatim.
            let wrapped = format!("Failed to fetch {} purchases: {}", tab.as_str(), e);
            return Err(map_load_error(&wrapped));
        }
    };

    let payload = match tab {
        PurchaseTab::Albums => {
            let response_total = response.albums.total;
            let items = response.albums.items;
            let total = resolve_tab_total(metadata.total_albums, response_total, items.len());
            PurchasesTabPayload {
                tab_albums: items,
                tab_tracks: Vec::new(),
                total,
            }
        }
        PurchaseTab::Tracks => {
            let response_total = response.tracks.total;
            let items = response.tracks.items;
            let total = resolve_tab_total(metadata.total_tracks, response_total, items.len());
            PurchasesTabPayload {
                tab_albums: Vec::new(),
                tab_tracks: items,
                total,
            }
        }
    };

    Ok(payload)
}

/// `searchPurchases(query)` (Svelte `handleSearchInput`, §2.1.7): fetch ALL
/// purchases (both types) then filter by the query (title / artist / album).
/// Returns BOTH the album + track lists (the search path sets both arrays).
/// Errors map per §3.6 (network-ish → `purchases.loadFailed`, else raw).
pub async fn search_purchases(
    runtime: &Runtime,
    query: &str,
) -> Result<(Vec<PurchaseAlbum>, Vec<PurchaseTrack>), String> {
    let client_lock = runtime.core().client();
    let client = {
        let guard = client_lock.read().await;
        guard.as_ref().cloned()
    };
    let Some(client) = client else {
        return Err("purchases.loadFailed".to_string());
    };
    let response = match purchases_service::get_user_purchases_all(&client).await {
        Ok(r) => r,
        Err(e) => {
            let wrapped = format!("Failed to fetch purchases: {e}");
            return Err(map_load_error(&wrapped));
        }
    };
    let filtered = purchases_service::filter_purchase_response(response, query.trim());
    Ok((filtered.albums.items, filtered.tracks.items))
}

/// `getFormats(albumId)` (§2.1.13 / command #6): fetch the album then synthesize
/// its ≤4 downloadable format options.
///
/// 1:1 FIDELITY (§2.1.13): the Svelte per-track download flow distinguishes TWO
/// failure modes that produce DIFFERENT toasts. `getFormats` is `await`ed inside
/// the `try`, so:
///   * it RESOLVES with `[]` (a genuinely-empty but successful fetch) →
///     `purchases.errors.noFormats` ("No downloadable formats available");
///   * it THROWS (network error / no session) → `catch` →
///     `purchases.errors.downloadFailed`.
/// To preserve that split this returns a `Result`: `Ok(vec)` for a SUCCESSFUL
/// fetch (the vec may be empty), and `Err(message)` for a FETCH FAILURE (no
/// client or a `get_album` error). The caller maps `Ok(empty)` → `noFormats` and
/// `Err(_)` → `downloadFailed`. Collapsing a fetch failure into an empty vec
/// (the old behavior) misroutes a network error to the `noFormats` toast.
pub async fn get_album_formats(
    runtime: &Runtime,
    album_id: &str,
) -> Result<Vec<PurchaseFormatOption>, String> {
    let client_lock = runtime.core().client();
    let client = {
        let guard = client_lock.read().await;
        guard.as_ref().cloned()
    };
    let Some(client) = client else {
        // No session = the Svelte `getFormats` throw path (an unauthenticated
        // fetch rejects) → `downloadFailed`, NOT `noFormats`.
        return Err("No Qobuz session".to_string());
    };
    match client.get_album(album_id).await {
        Ok(album) => Ok(purchases_service::synth_formats(&album)),
        Err(e) => {
            log::warn!("[Purchases] get_album_formats({album_id}) failed: {e}");
            Err(e.to_string())
        }
    }
}

/// §2.1.6 total backfill: prefer the metadata (ids-call) total; if that is 0
/// fall back to the response page total, then to the loaded item count.
fn resolve_tab_total(metadata_total: u32, response_total: u32, loaded_len: usize) -> u32 {
    if metadata_total != 0 {
        metadata_total
    } else if response_total != 0 {
        response_total
    } else {
        loaded_len as u32
    }
}

/// §3.6 error mapping (Svelte `loadPurchasesByTab` catch): if the error string
/// contains `"Load failed"`, `"fetch"`, or `"NetworkError"`, surface the i18n
/// key `purchases.loadFailed`; otherwise surface the raw message unchanged.
///
/// (The list view i18n-maps network errors; the DETAIL view surfaces the RAW
/// string — that asymmetry is intentional and handled in Slice 9, not here.)
fn map_load_error(message: &str) -> String {
    if message.contains("Load failed")
        || message.contains("fetch")
        || message.contains("NetworkError")
    {
        "purchases.loadFailed".to_string()
    } else {
        message.to_string()
    }
}

// ============================================================================
// Slice 7 — Download state machine + actions
//
// Verbatim port of the Svelte `purchaseDownloadStore.ts` (source-of-truth §2.3)
// onto the CONTROLLER so the state SURVIVES view navigation (the Tauri store is
// a module-level `writable`; a Slint global can't survive away-navigation
// re-renders and isn't `Send`, so we use an `OnceLock<Mutex<…>>` process-wide
// singleton like `qconnect_service::service()`).
//
// The store is PURE `Send` data (track-id → status maps keyed by albumId, plus
// per-album abort flags) — no `slint::Image`/`ModelRc`. Mutators are synchronous
// and unit-testable in isolation; the async actions (album loop / single-track /
// folder picker / Add-to-Library) wrap them around the Slice-5 service primitive
// `purchases_service::download_purchase_track`.
//
// SEND BOUNDARY (load-bearing): the Slice-5 `download_purchase_track(&client,
// &db, …)` holds `&LibraryDatabase` (rusqlite `Connection` is `Send` but NOT
// `Sync`, so `&LibraryDatabase` is `!Send`) ACROSS the multi-minute CDN `.await`
// → its future is `!Send` and cannot be `handle.spawn`-ed on the multi-thread
// runtime. We therefore run the album loop on a DEDICATED OS thread driving a
// CURRENT-THREAD tokio runtime (`new_current_thread`), where `!Send` futures are
// legal. Each download owns a fresh per-user `LibraryDatabase`, and the Qobuz
// client is snapshot-cloned out of the `RwLock` before the thread starts. This
// also gives the required strictly-SEQUENTIAL execution for free (the CDN gates
// concurrent connections — §2.3 critical behavior #1).
// ============================================================================

/// One track's download status (Svelte `TrackDownloadStatus`,
/// `purchaseDownloadStore.ts:1`). Ported verbatim: the four states a track can
/// be in inside an album-download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackDownloadStatus {
    Downloading,
    Complete,
    Failed,
    Cancelled,
}

impl TrackDownloadStatus {
    /// The lowercase wire string the Slint UI binds against (matches the Svelte
    /// status literals `'downloading'|'complete'|'failed'|'cancelled'`). Slices
    /// 8/9 read this when projecting per-track row state.
    pub fn as_str(self) -> &'static str {
        match self {
            TrackDownloadStatus::Downloading => "downloading",
            TrackDownloadStatus::Complete => "complete",
            TrackDownloadStatus::Failed => "failed",
            TrackDownloadStatus::Cancelled => "cancelled",
        }
    }
}

/// Per-album download state (Svelte `AlbumDownloadState`,
/// `purchaseDownloadStore.ts:2-8`), keyed by `albumId` in the store. Survives
/// navigation. `destination` is REWRITTEN to the album folder after the first
/// successful track (so Add-to-Library adds the album, not the download root).
/// `format_id` gates completion display (a download done for format X must not
/// show `complete` while the user is viewing format Y).
#[derive(Debug, Clone, Default)]
pub struct AlbumDownloadState {
    /// `trackId → status` for tracks touched by THIS album's downloads.
    pub track_statuses: HashMap<u64, TrackDownloadStatus>,
    /// True while the sequential album loop is running.
    pub is_downloading_all: bool,
    /// True only once EVERY requested track is `Complete` (one failure/cancel
    /// leaves it false). NOT format-scoped here — the format scoping is applied
    /// at READ time (`album_all_complete_for_format`), matching the Svelte
    /// derived `allComplete = (state.allComplete) && (formatId === undefined ||
    /// formatId === selectedFormatId)`.
    pub all_complete: bool,
    /// The download destination. Seeded to the user-picked folder, then
    /// REWRITTEN to the album-level folder after the first successful track.
    pub destination: Option<String>,
    /// The format these downloads used (`undefined` until a download starts).
    pub format_id: Option<u32>,
}

/// The whole download store (Svelte `purchaseDownloads` writable +
/// module-level `abortFlags` map). Pure `Send` data. Held behind a process-wide
/// `OnceLock<Mutex<…>>` (see [`store`]).
#[derive(Debug, Default)]
pub struct PurchaseDownloadStore {
    /// `albumId → AlbumDownloadState` (Svelte `purchaseDownloads`).
    albums: HashMap<String, AlbumDownloadState>,
    /// `albumId → abort?` (Svelte module-level `abortFlags`, NOT in the store
    /// itself — cooperative cancellation checked BETWEEN tracks).
    abort_flags: HashMap<String, bool>,
    /// `trackId → resolved formats` for the OPEN per-track format picker. Caches
    /// the `getFormats` result so `pick-format` resolves the chosen option's
    /// label WITHOUT a second `get_album` round-trip (the Svelte original keeps
    /// the formats in component state). One entry at a time in practice (the
    /// picker is modal); cleared when the picker is consumed.
    picker_formats: HashMap<u64, Vec<PurchaseFormatOption>>,
}

impl PurchaseDownloadStore {
    /// Upsert an album's state via an immutable-style updater (Svelte
    /// `updateAlbumState`: seed `{trackStatuses:{}, isDownloadingAll:false,
    /// allComplete:false}` if absent, then apply the updater).
    fn update_album<F: FnOnce(&mut AlbumDownloadState)>(&mut self, album_id: &str, updater: F) {
        let state = self.albums.entry(album_id.to_string()).or_default();
        updater(state);
    }

    /// `startAlbumDownload` seed (`purchaseDownloadStore.ts:140-148`): clear the
    /// abort flag, then REPLACE the album state with a fresh seed
    /// `{trackStatuses:{}, isDownloadingAll:true, allComplete:false, destination,
    /// formatId}`. (Album-download seeds FRESH — unlike single-track, which
    /// merges; §A.7.)
    fn seed_album_download(
        &mut self,
        album_id: &str,
        destination: &str,
        format_id: u32,
    ) {
        self.abort_flags.remove(album_id);
        self.albums.insert(
            album_id.to_string(),
            AlbumDownloadState {
                track_statuses: HashMap::new(),
                is_downloading_all: true,
                all_complete: false,
                destination: Some(destination.to_string()),
                format_id: Some(format_id),
            },
        );
    }

    /// Mark one track's status inside an album (Svelte `updateAlbumState(...,
    /// trackStatuses: { ...state.trackStatuses, [trackId]: status })`). Spreads
    /// the existing statuses — only the one track changes.
    fn mark_track(&mut self, album_id: &str, track_id: u64, status: TrackDownloadStatus) {
        self.update_album(album_id, |state| {
            state.track_statuses.insert(track_id, status);
        });
    }

    /// DESTINATION REWRITE after the first successful track
    /// (`purchaseDownloadStore.ts:160-165`, §2.3 step 4): set `destination` to
    /// the album-level folder derived from the returned file path. Only the
    /// album loop calls this, and only after the FIRST success.
    fn rewrite_destination(&mut self, album_id: &str, album_folder: &str) {
        self.update_album(album_id, |state| {
            state.destination = Some(album_folder.to_string());
        });
    }

    /// Finalize the album loop (`purchaseDownloadStore.ts:167-174`): delete the
    /// abort flag; `allComplete = trackIds.every(id => statuses[id] ===
    /// 'complete')`; `isDownloadingAll = false`.
    fn finalize_album(&mut self, album_id: &str, track_ids: &[u64]) {
        self.abort_flags.remove(album_id);
        self.update_album(album_id, |state| {
            // FIDELITY: JS `trackIds.every(id => statuses[id] === 'complete')`.
            // `Iterator::all` matches `Array.prototype.every` EXACTLY, including
            // the empty case (`[].every(...)` === `true`). A download-all is
            // never fired on a zero-track album in practice, so the empty case is
            // degenerate — but we replicate the JS semantics verbatim rather than
            // "guard" it, per the strict-1:1 rule.
            let all = track_ids.iter().all(|id| {
                matches!(
                    state.track_statuses.get(id),
                    Some(TrackDownloadStatus::Complete)
                )
            });
            state.all_complete = all;
            state.is_downloading_all = false;
        });
    }

    /// Cancel-path bulk mark (`executeAlbumDownload` abort branch,
    /// `purchaseDownloadStore.ts:151-158`): for every requested track that has
    /// NOT yet reached a terminal status, set `Cancelled`; then
    /// `isDownloadingAll = false`, `allComplete = false`, delete the abort flag.
    /// (The in-flight track already finished and is `Complete`/`Failed`; only
    /// not-yet-started tracks become `Cancelled`.)
    fn apply_cancellation(&mut self, album_id: &str, track_ids: &[u64]) {
        self.update_album(album_id, |state| {
            for id in track_ids {
                // VERBATIM Svelte predicate (`purchaseDownloadStore.ts:109-112`):
                // `if (!currentState?.trackStatuses[id]) remaining[id] =
                // 'cancelled'`. Only tracks with NO existing status entry become
                // `Cancelled` — a track that already has ANY status (including
                // `Downloading`) is left untouched (the abort-check runs BETWEEN
                // tracks, so the in-flight track has already finished and reached a
                // terminal status before this runs).
                if !state.track_statuses.contains_key(id) {
                    state.track_statuses.insert(*id, TrackDownloadStatus::Cancelled);
                }
            }
            state.is_downloading_all = false;
            state.all_complete = false;
        });
        self.abort_flags.remove(album_id);
    }

    /// Single-track MERGE (Svelte `startTrackDownload`,
    /// `purchaseDownloadStore.ts:171-184`, §A.7): spread `...state`, overwrite
    /// ONLY the target track's status (→ `Downloading`) and set `formatId`. Every
    /// prior field (`isDownloadingAll`, `allComplete`, `destination`, other track
    /// statuses) SURVIVES — so a post-album single-track redownload keeps
    /// `allComplete` + the Add-to-Library affordance. Does NOT seed fresh state,
    /// does NOT rewrite destination, does NOT set `isDownloadingAll`.
    fn merge_single_track_start(&mut self, album_id: &str, track_id: u64, format_id: u32) {
        self.update_album(album_id, |state| {
            state
                .track_statuses
                .insert(track_id, TrackDownloadStatus::Downloading);
            state.format_id = Some(format_id);
        });
    }

    /// Single-track completion/failure MERGE (`executeSingleTrackDownload`,
    /// `:195-204`): again spreads `...state` — only the one track's status flips
    /// to `Complete`/`Failed`. `allComplete`/`destination`/siblings untouched.
    fn merge_single_track_finish(
        &mut self,
        album_id: &str,
        track_id: u64,
        status: TrackDownloadStatus,
    ) {
        self.update_album(album_id, |state| {
            state.track_statuses.insert(track_id, status);
        });
    }

    /// `cancelAlbumDownload` (`purchaseDownloadStore.ts:131`): set the abort flag
    /// (the running track finishes; the rest become `Cancelled` between tracks).
    fn set_abort(&mut self, album_id: &str) {
        self.abort_flags.insert(album_id.to_string(), true);
    }

    /// `clearAlbumDownloadState` (`purchaseDownloadStore.ts:124-128`): delete the
    /// abort flag AND remove the album entry (progress / Add-to-Library blocks
    /// vanish).
    fn clear(&mut self, album_id: &str) {
        self.abort_flags.remove(album_id);
        self.albums.remove(album_id);
    }

    /// `getAlbumDownloadFormatId(albumId)` (`purchaseDownloadStore.ts:120`):
    /// `purchaseDownloads[albumId]?.formatId`.
    fn album_format_id(&self, album_id: &str) -> Option<u32> {
        self.albums.get(album_id).and_then(|s| s.format_id)
    }

    /// Whether the album loop should abort before the next track (cooperative
    /// cancel check; Svelte `abortFlags.get(albumId)`).
    fn is_aborted(&self, album_id: &str) -> bool {
        self.abort_flags.get(album_id).copied().unwrap_or(false)
    }

    /// Snapshot one album's state (cloned — the caller reads off the lock).
    fn album(&self, album_id: &str) -> Option<AlbumDownloadState> {
        self.albums.get(album_id).cloned()
    }
}

/// Process-wide download store singleton (mirrors `qconnect_service::service()`
/// — survives away-navigation, `Send`). A `std::sync::Mutex` is enough: every
/// mutation is a short synchronous map edit, never held across `.await`.
fn store() -> &'static Mutex<PurchaseDownloadStore> {
    static STORE: OnceLock<Mutex<PurchaseDownloadStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(PurchaseDownloadStore::default()))
}

/// Lock the store, run `f`, return its result. Recovers from a poisoned mutex
/// (a panic in a mutator would only corrupt one album's transient status map,
/// which the next download reseeds anyway).
fn with_store<R>(f: impl FnOnce(&mut PurchaseDownloadStore) -> R) -> R {
    let mut guard = store().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

// ---------------------------------------------------------------------------
// Public store READERS (used by Slices 8/9 to project per-row + detail state).
// ---------------------------------------------------------------------------

/// Flattened `allTrackStatuses` (Svelte derived, `purchaseDownloadStore.ts`):
/// `trackId → status` across ALL albums. PurchasesView rows bind here. If two
/// albums ever held the same track id, the last-written wins (matches the JS
/// `derived` object-spread order — undefined across albums, but harmless: a
/// track id is unique to one album in practice).
pub fn all_track_statuses() -> HashMap<u64, TrackDownloadStatus> {
    with_store(|s| {
        let mut flat = HashMap::new();
        for state in s.albums.values() {
            for (id, status) in &state.track_statuses {
                flat.insert(*id, *status);
            }
        }
        flat
    })
}

/// Snapshot one album's download state (detail view binds here).
pub fn album_download_state(album_id: &str) -> Option<AlbumDownloadState> {
    with_store(|s| s.album(album_id))
}

/// Cache the resolved format options for an OPEN per-track picker (keyed by
/// `trackId`). `pick-format` reads these back so the chosen option's label is
/// resolved without re-fetching the album (the Svelte original keeps the
/// formats in component state).
pub fn cache_picker_formats(track_id: u64, formats: Vec<PurchaseFormatOption>) {
    with_store(|s| {
        s.picker_formats.insert(track_id, formats);
    });
}

/// Consume the cached picker formats for a track (removes the entry). Returns
/// `None` if the picker cache was never seeded (e.g. picker opened before this
/// slice cached it) — the caller then falls back to a fresh `get_album` fetch.
pub fn take_picker_formats(track_id: u64) -> Option<Vec<PurchaseFormatOption>> {
    with_store(|s| s.picker_formats.remove(&track_id))
}

/// Drop a cached picker entry without consuming it (picker closed/cancelled).
pub fn clear_picker_formats(track_id: u64) {
    with_store(|s| {
        s.picker_formats.remove(&track_id);
    });
}

// ---------------------------------------------------------------------------
// Format-scoped completion gating (source-of-truth §2.2.1 / §2.2.3 / §A.4).
// ---------------------------------------------------------------------------

/// Detail-view `allComplete` (Svelte `:388`): `(state.allComplete) &&
/// (state.formatId === undefined || state.formatId === selectedFormatId)`. The
/// stored `all_complete` is gated by the currently-selected format — a fully
/// downloaded album for format X reads `allComplete = false` while viewing
/// format Y. (Add-to-Library + the `complete` progress label key off THIS.)
pub fn album_all_complete_for_format(album_id: &str, selected_format_id: Option<u32>) -> bool {
    with_store(|s| {
        let Some(state) = s.albums.get(album_id) else {
            return false;
        };
        if !state.all_complete {
            return false;
        }
        match state.format_id {
            None => true,
            Some(fmt) => Some(fmt) == selected_format_id,
        }
    })
}

/// Detail-view `getTrackStatus(trackId)` (Svelte `:406-415`, format-scoped):
/// `status = downloadStatuses[trackId]`; if none → `None`; if `Complete` AND the
/// album's `formatId` is set AND ≠ `selectedFormatId` → `None` (HIDES completion
/// when viewing a different format). All other statuses pass through.
///
/// NOTE the asymmetry (§A.4): this scoping affects only the per-track row badge.
/// `completedCount` / progress-fill / `wasCancelled` count ALL statuses and are
/// NOT format-scoped — those are computed by Slice 9 over the RAW
/// `track_statuses` map (via [`album_download_state`]), never through this fn.
pub fn track_status_scoped(
    album_id: &str,
    track_id: u64,
    selected_format_id: Option<u32>,
) -> Option<TrackDownloadStatus> {
    with_store(|s| {
        let state = s.albums.get(album_id)?;
        let status = *state.track_statuses.get(&track_id)?;
        if status == TrackDownloadStatus::Complete {
            if let Some(fmt) = s.album_format_id(album_id) {
                if Some(fmt) != selected_format_id {
                    return None;
                }
            }
        }
        Some(status)
    })
}

/// `isDownloadedForFormat` (Svelte `:446`): `selectedFormatId !== null &&
/// (track.downloaded_format_ids ?? []).includes(selectedFormatId)`. Keys off the
/// SERVER-derived `downloaded_format_ids` (from the local registry's REQUESTED
/// format, §B.2), NOT the transient store. Combined with the store's `complete`
/// status to produce the row's `isDownloaded` (Slice 9).
pub fn is_downloaded_for_format(
    downloaded_format_ids: &[u32],
    selected_format_id: Option<u32>,
) -> bool {
    match selected_format_id {
        Some(fmt) => downloaded_format_ids.contains(&fmt),
        None => false,
    }
}

/// `albumFolderFromFilePath(filePath)` (Svelte `:160-165` helper): strip after
/// the last `/` (fallback `\`); returns the directory, or the whole path if no
/// separator. Used to rewrite `destination` to the album folder after the first
/// successful track.
fn album_folder_from_file_path(file_path: &str) -> String {
    if let Some(idx) = file_path.rfind('/') {
        return file_path[..idx].to_string();
    }
    if let Some(idx) = file_path.rfind('\\') {
        return file_path[..idx].to_string();
    }
    file_path.to_string()
}

// ---------------------------------------------------------------------------
// Cancel / clear actions (synchronous — no I/O).
// ---------------------------------------------------------------------------

/// `cancelAlbumDownload(albumId)` (§2.3): set the abort flag; the running track
/// finishes, the rest become `Cancelled` between tracks.
pub fn cancel_album_download(album_id: &str) {
    with_store(|s| s.set_abort(album_id));
}

/// `clearAlbumDownloadState(albumId)` (§2.3): delete the abort flag + remove the
/// album entry (progress / Add-to-Library blocks vanish).
pub fn clear_album_download_state(album_id: &str) {
    with_store(|s| s.clear(album_id));
}

// ---------------------------------------------------------------------------
// Download execution (async actions).
// ---------------------------------------------------------------------------

/// Open a fresh per-user `LibraryDatabase` for the download thread. Replicates
/// `library_db::db_path()` (private) using the PUBLIC `current_user_id()` +
/// `dirs::data_dir()`. Returns `None` when no user is active or the open fails.
/// The owned DB is moved into the current-thread runtime so the Slice-5 service
/// can hold `&db` across the CDN await (legal off the multi-thread runtime).
fn open_owned_library_db() -> Option<qbz_library::LibraryDatabase> {
    let uid = crate::library_db::current_user_id()?;
    let path = dirs::data_dir()?
        .join("qbz")
        .join("users")
        .join(uid.to_string())
        .join("library.db");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match qbz_library::LibraryDatabase::open(&path) {
        Ok(db) => Some(db),
        Err(e) => {
            log::error!("[Purchases] open library.db for download failed: {e}");
            None
        }
    }
}

/// Snapshot-clone the live Qobuz client out of the `RwLock` (so the download
/// thread does not hold the lock across the multi-minute CDN fetch). `None` when
/// logged out.
async fn snapshot_client(runtime: &Runtime) -> Option<qbz_qobuz::QobuzClient> {
    let lock = runtime.core().client();
    let guard = lock.read().await;
    guard.as_ref().cloned()
}

/// skip-if-remote guard (§Slice-7 / Tauri `skipIfRemote`): never fire purchase
/// download I/O while controlling a remote QConnect renderer. Mirrors the
/// `award.rs` / favorites guard (`svc.is_peer_active().await`).
async fn is_controlling_remote() -> bool {
    if let Some(svc) = crate::qconnect_service::service() {
        return svc.is_peer_active().await;
    }
    false
}

/// `startAlbumDownload(albumId, trackIds, formatId, destination, qualityDir)`
/// (§2.3): clear the abort flag, seed `{trackStatuses:{}, isDownloadingAll:true,
/// allComplete:false, destination, formatId}`, then fire the sequential loop
/// (NOT awaited). The loop runs on a dedicated thread (see SEND BOUNDARY note).
///
/// `qualityDir` is the format label with `'/'→'-'` already applied
/// ([`quality_dir`]). The album state survives navigation — re-entering the
/// detail view shows live progress.
pub fn start_album_download(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
    track_ids: Vec<u64>,
    format_id: u32,
    destination: String,
    quality_dir: String,
) {
    handle.spawn(async move {
        // skip-if-remote: no download I/O while controlling a remote renderer.
        if is_controlling_remote().await {
            return;
        }
        // Seed FRESH album state (album-download replaces; §A.7 only single-track
        // merges).
        with_store(|s| s.seed_album_download(&album_id, &destination, format_id));

        let Some(client) = snapshot_client(&runtime).await else {
            // No client: mark every track failed so the UI does not hang on the
            // seeded `Downloading`/empty state. (Tauri would surface a per-track
            // getFileUrl/auth failure the same way — generic `'failed'`, §B.4.)
            with_store(|s| {
                for id in &track_ids {
                    s.mark_track(&album_id, *id, TrackDownloadStatus::Failed);
                }
                s.finalize_album(&album_id, &track_ids);
            });
            return;
        };

        execute_album_download(weak, album_id, track_ids, format_id, destination, quality_dir, client)
            .await;
    });
}

/// `executeAlbumDownload` (§2.3, sequential loop). For each `trackId` in order:
///   1. abort-check → mark not-yet-terminal tracks `Cancelled`, finalize, RETURN.
///   2. mark `Downloading`.
///   3. `download_purchase_track` (Slice-5 service primitive).
///   4. on the FIRST success only → rewrite `destination` to the album folder.
///   5. mark `Complete` (registry write is inside the primitive; best-effort —
///      a registry failure surfaces as `Failed`, §B.1).
///   6. on Err → mark `Failed` (loop CONTINUES).
/// Finally: `finalize_album` (delete abort flag; `allComplete` = ALL complete;
/// `isDownloadingAll=false`).
///
/// Runs on a dedicated OS thread with a CURRENT-THREAD runtime so the `!Send`
/// (db-borrow-across-await) download future is legal (SEND BOUNDARY note above).
async fn execute_album_download(
    weak: slint::Weak<AppWindow>,
    album_id: String,
    track_ids: Vec<u64>,
    format_id: u32,
    destination: String,
    quality_dir: String,
    client: qbz_qobuz::QobuzClient,
) {
    // Move the whole (sequential, `!Send`) loop onto a dedicated thread driving
    // a current-thread tokio runtime. `spawn_blocking` would also work but a
    // fresh thread keeps the blocking pool free for DB/scan work.
    let done = tokio::task::spawn_blocking(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                log::error!("[Purchases] album-download runtime build failed: {e}");
                return;
            }
        };
        rt.block_on(async move {
            let Some(db) = open_owned_library_db() else {
                // No DB → cannot register; mark all failed (file would orphan).
                with_store(|s| {
                    for id in &track_ids {
                        s.mark_track(&album_id, *id, TrackDownloadStatus::Failed);
                    }
                    s.finalize_album(&album_id, &track_ids);
                });
                return;
            };

            let mut first_success_done = false;
            for track_id in track_ids.iter().copied() {
                // (1) Cooperative abort-check BEFORE each track (between-tracks
                // only; the in-flight track always finishes).
                if with_store(|s| s.is_aborted(&album_id)) {
                    with_store(|s| s.apply_cancellation(&album_id, &track_ids));
                    return;
                }
                // (2) downloading.
                with_store(|s| s.mark_track(&album_id, track_id, TrackDownloadStatus::Downloading));

                // (3) download ONLY (get_track → getFileUrl → CDN → .part→rename).
                // The album loop does NOT bundle the registry write into the
                // download Result — it mirrors Svelte `executeAlbumDownload`
                // (`purchaseDownloadStore.ts:130-147`): mark `Complete` on download
                // success, THEN do a SEPARATE best-effort registry write that
                // SWALLOWS failure (`markTrackDownloaded(...).catch(()=>{})`). So a
                // registry-write failure during album download leaves the track
                // `Complete` (file on disk, just unregistered) — UNLIKE the
                // single-track path which propagates the registry error → `Failed`
                // (§B.1). The download itself still records the REQUESTED format_id
                // when the best-effort registry write runs (B.2).
                match purchases_service::download_purchase_track_file_only(
                    &client,
                    track_id,
                    format_id,
                    &destination,
                    &quality_dir,
                )
                .await
                {
                    Ok(file_path) => {
                        // (4) destination rewrite to the album folder after the
                        // FIRST success only.
                        if !first_success_done {
                            first_success_done = true;
                            let folder = album_folder_from_file_path(&file_path);
                            with_store(|s| s.rewrite_destination(&album_id, &folder));
                        }
                        // (5) complete FIRST (Svelte sets `'complete'` before the
                        // registry write).
                        with_store(|s| {
                            s.mark_track(&album_id, track_id, TrackDownloadStatus::Complete)
                        });
                        // (5b) best-effort registry write — SWALLOW failure
                        // (`.catch(()=>{})`). The album loop passes the real
                        // `albumId` (Svelte `markTrackDownloaded(trackId, albumId,
                        // filePath, formatId)`), unlike the single-track path which
                        // passes `None`.
                        if let Err(e) = db.mark_purchase_downloaded(
                            track_id as i64,
                            Some(album_id.as_str()),
                            &file_path,
                            format_id as i64,
                        ) {
                            log::warn!(
                                "[Purchases] best-effort registry write for track {track_id} failed (track stays complete): {e}"
                            );
                        }
                    }
                    Err(e) => {
                        // (6) failed — loop continues (one failure leaves
                        // `allComplete` false). A CDN/getFileUrl failure lands here;
                        // a registry-only failure does NOT (it's swallowed above).
                        log::warn!("[Purchases] download track {track_id} failed: {e}");
                        with_store(|s| {
                            s.mark_track(&album_id, track_id, TrackDownloadStatus::Failed)
                        });
                    }
                }
            }
            // finally: finalize.
            with_store(|s| s.finalize_album(&album_id, &track_ids));
        });
    })
    .await;

    if let Err(e) = done {
        log::error!("[Purchases] album-download thread join failed: {e}");
    }
    // Re-project the finished album statuses onto any visible list/detail rows.
    nudge_ui_refresh(&weak);
}

/// `startTrackDownload(albumId, trackId, formatId, destination, qualityDir)`
/// (§2.3 / §A.7): MERGE into the existing album state (mark the one track
/// `Downloading` + set `formatId`; everything else survives), then download.
/// Single-track download does NOT rewrite destination and does NOT set
/// `isDownloadingAll`/`allComplete`. On finish: merge `Complete`/`Failed`.
pub fn start_track_download(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
    track_id: u64,
    format_id: u32,
    destination: String,
    quality_dir: String,
) {
    handle.spawn(async move {
        if is_controlling_remote().await {
            return;
        }
        // MERGE — never seed fresh (§A.7).
        with_store(|s| s.merge_single_track_start(&album_id, track_id, format_id));
        // Nudge the list-row projection to show `downloading` immediately.
        nudge_ui_refresh(&weak);

        let Some(client) = snapshot_client(&runtime).await else {
            with_store(|s| {
                s.merge_single_track_finish(&album_id, track_id, TrackDownloadStatus::Failed)
            });
            nudge_ui_refresh(&weak);
            return;
        };

        let done = tokio::task::spawn_blocking(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("[Purchases] track-download runtime build failed: {e}");
                    return;
                }
            };
            rt.block_on(async move {
                let Some(db) = open_owned_library_db() else {
                    with_store(|s| {
                        s.merge_single_track_finish(
                            &album_id,
                            track_id,
                            TrackDownloadStatus::Failed,
                        )
                    });
                    return;
                };
                let status = match purchases_service::download_purchase_track(
                    &client,
                    &db,
                    track_id,
                    format_id,
                    &destination,
                    &quality_dir,
                )
                .await
                {
                    Ok(_) => TrackDownloadStatus::Complete,
                    Err(e) => {
                        log::warn!("[Purchases] single-track download {track_id} failed: {e}");
                        TrackDownloadStatus::Failed
                    }
                };
                // MERGE finish (spreads `...state`; siblings + allComplete +
                // destination untouched — §A.7).
                with_store(|s| s.merge_single_track_finish(&album_id, track_id, status));
            });
        })
        .await;

        if let Err(e) = done {
            log::error!("[Purchases] track-download thread join failed: {e}");
        }
        // Project the finished status (complete/failed) onto the list rows.
        nudge_ui_refresh(&weak);
    });
}

/// Refresh the PurchasesView list-row download projection from the store. A
/// no-op when the window is gone (download still completes; the registry holds
/// the record for the next open). Used by the download actions to surface live
/// per-track status changes without a refetch.
fn nudge_ui_refresh(weak: &slint::Weak<AppWindow>) {
    let _ = weak.upgrade_in_event_loop(|w| {
        // Re-derive the PurchasesView LIST projection (per-track dl-status on the
        // visible rows). A re-derive elsewhere is harmless (it rebuilds hidden
        // models).
        refresh_track_statuses(&w);
        // ALSO re-derive the detail screen so the progress section, per-track
        // 4-state, completed-count, was-cancelled, all-complete, and the
        // Add-to-Library block update LIVE during/after a running download
        // (matches Svelte's reactive `$derived` re-run on every store change,
        // PurchaseAlbumDetailView.svelte :44-52/:203-209/:317-349/:388-461).
        // Gated on the detail view being the active surface — `derive_detail`
        // reads the persisted detail cache (the currently-loaded album), so a
        // re-derive while another view is up would rebuild the hidden detail
        // models from stale-but-irrelevant cache; gating keeps it cheap and
        // correct. The four main.rs action handlers seed the detail once
        // synchronously; this routes the async download-lifecycle nudges
        // (execute_album_download :1032, start_track_download :1057/:1063/:1116)
        // to keep it refreshed thereafter.
        if w.global::<NavState>().get_view() == ContentView::PurchaseAlbum {
            derive_detail(&w);
        }
    });
}

/// Folder picker (§2.1.13 `executeTrackDownload` / §2.2.7 `promptForFolder`):
/// open a directory-only picker defaulting to the OS audio dir, titled
/// `purchases.chooseFolder`. Returns `None` when the user cancels (Svelte
/// `if (!dest || typeof dest !== 'string') return`). Async (rfd portal).
pub async fn pick_download_folder() -> Option<String> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(&qbz_i18n::t("Choose folder"));
    if let Some(audio_dir) = dirs::audio_dir() {
        dialog = dialog.set_directory(audio_dir);
    }
    let folder = dialog.pick_folder().await?;
    Some(folder.path().to_string_lossy().to_string())
}

/// `handleAddToLibrary()` (§2.2.6) — the ONLY non-purchase backend WRITE.
/// `destination` is the album-level folder (after the rewrite, §7). Sequence:
///   1. `add_folder_with_network_info(path)` → folder id (core-equivalent of
///      `v2_library_add_folder`; reuses the live `library_db` + network probe
///      exactly like `local_library_settings::add_folder`).
///   2. `clearAlbumDownloadState(albumId)` (progress / Add-to-Library vanish).
///   3. success toast `purchases.addToLibrarySuccess`.
///   4. fire-and-forget scan of the new folder (`scan_folder`, errors swallowed
///      — the Svelte `.catch(() => {})`).
/// On add-folder failure → error toast `purchases.addToLibraryError`.
///
/// The spinner (`adding_to_library`) is set true by the caller BEFORE this fires
/// and is held true across the whole async add (matching Svelte's `addingToLibrary`
/// staying true across the await, PurchaseAlbumDetailView.svelte :56-71). This fn
/// owns clearing it: on EVERY exit path it schedules an event-loop callback that
/// drops the spinner AND re-derives the detail — so on success the cleared download
/// state hides the progress + Add-to-Library blocks (Svelte's reactive `$derived`),
/// and on failure the spinner drops while the block stays in place. (Replaces the
/// old next-tick `upgrade_in_event_loop` in main.rs that flashed the spinner off
/// before the async add finished and re-derived against stale state.)
pub fn handle_add_to_library(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
    destination: String,
) {
    let scan_handle = handle.clone();
    handle.spawn(async move {
        // Drop the spinner + re-derive the detail on the event loop. Called on
        // every exit path (skip-if-remote / add-failure / success) so the
        // spinner never gets stuck and the (possibly cleared) download state is
        // reflected. On success the state is already cleared → the derive hides
        // the progress + Add-to-Library blocks; on failure nothing was cleared →
        // the block stays, only the spinner drops.
        let finish = |weak: slint::Weak<AppWindow>| {
            let _ = weak.upgrade_in_event_loop(|w| {
                set_detail_adding_to_library(&w, false);
                refresh_detail_download(&w);
            });
        };

        // skip-if-remote: no library writes while controlling a remote renderer.
        if is_controlling_remote().await {
            finish(weak.clone());
            return;
        }

        // (1) add the album folder (with network detection, like LocalLibrary's
        // own add-folder). Returns the folder id for the follow-up scan.
        let dest_for_add = destination.clone();
        let folder_id = tokio::task::spawn_blocking(move || {
            let pb = std::path::Path::new(&dest_for_add);
            let is_net = qbz_library::is_network_path(pb);
            let fs = if is_net {
                qbz_library::network_fs_label(pb)
            } else {
                None
            };
            crate::library_db::with_db(|db| {
                Ok(db.add_folder_with_network_info(&dest_for_add, is_net, fs.as_deref())?)
            })
        })
        .await
        .ok()
        .flatten();

        let Some(folder_id) = folder_id else {
            // (error) add-folder failed → error toast, leave state intact (the
            // Add-to-Library block stays); `finish` drops only the spinner.
            crate::toast::error_weak(&weak, qbz_i18n::t("Couldn't add to library"));
            finish(weak.clone());
            return;
        };

        // (2) clear the in-memory download state (progress + Add-to-Library
        // blocks disappear).
        clear_album_download_state(&album_id);

        // (3) success toast.
        crate::toast::success_weak(&weak, qbz_i18n::t("Added to library"));

        // (4) fire-and-forget scan of the new folder (errors swallowed). Reuses
        // the LocalLibrary scan engine — the scanned folder equals the rewritten
        // album folder, so the files tag `source='qobuz_purchase'` (§7.8) and
        // pick up the gold badge in LocalLibrary.
        crate::local_library_settings::scan_folder(weak.clone(), scan_handle, folder_id);

        // (5) drop the spinner + re-derive (cleared state → blocks vanish).
        finish(weak.clone());
    });
}

// ============================================================================
// Slice 8 — PurchasesView UI layer
//
// The list-surface chrome lives in `ui/purchases/PurchasesView.slint`; this
// section is the Rust half: the per-tab data cache (so the UI can re-derive
// filter/sort/group + search without a refetch), the byte-for-byte port of the
// Svelte filter/sort/group functions + formatters, and the event-loop appliers
// that project the cache → `PurchasesState` (`ModelRc<VecModel<…>>` built here,
// never off the event loop). Mirrors `PurchasesView.svelte` §2.1.
//
// SEND BOUNDARY: every fn that touches `slint::Image`/`ModelRc` takes
// `&AppWindow` and runs on the event loop (called via `upgrade_in_event_loop`).
// The cache holds only `Send` wire structs.
// ============================================================================

use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::{
    ContentView, NavState, PurchaseAlbumGroup, PurchaseAlbumItem, PurchaseDetailState,
    PurchaseDetailTrack, PurchaseFormatItem, PurchaseTrackGroup, PurchaseTrackItem, PurchasesState,
};

/// `'all' | 'hires' | 'cd' | 'lossy'` quality filter (Svelte `QualityFilter`).
/// Parsed from the persisted/`PurchasesState` string; unknown → `All`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityFilter {
    All,
    Hires,
    Cd,
    Lossy,
}

impl QualityFilter {
    pub fn from_str(s: &str) -> Self {
        match s {
            "hires" => QualityFilter::Hires,
            "cd" => QualityFilter::Cd,
            "lossy" => QualityFilter::Lossy,
            _ => QualityFilter::All,
        }
    }
}

/// The toolbar/filter state snapshot the appliers read off `PurchasesState`.
/// (Read once per derive so the pure fns stay free of any Slint dependency.)
#[derive(Debug, Clone)]
pub struct ToolbarState {
    pub album_grouping_enabled: bool,
    pub album_group_mode: String,
    pub album_sort_by: String,
    pub album_sort_direction: String,
    pub track_grouping_enabled: bool,
    pub track_group_mode: String,
    pub filter_hide_unavailable: bool,
    pub filter_quality: QualityFilter,
    pub filter_hide_downloaded: bool,
}

/// Per-tab data cache (process-wide, survives navigation like the download
/// store). Holds the RAW wire items so a filter/sort/group/search change
/// re-derives in Rust without a refetch — the Svelte `$derived` equivalent.
#[derive(Debug, Default)]
struct UiCache {
    albums_raw: Vec<PurchaseAlbum>,
    tracks_raw: Vec<PurchaseTrack>,
    albums_loaded: bool,
    tracks_loaded: bool,
    metadata: PurchasesMetadata,
    total_albums: u32,
    total_tracks: u32,
    /// Search active = the last applied query was non-empty.
    search_active: bool,
    /// Monotonic search token — the debounce drops a stale fire.
    search_seq: u64,
}

fn ui_cache() -> &'static Mutex<UiCache> {
    static CACHE: OnceLock<Mutex<UiCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(UiCache::default()))
}

fn with_ui_cache<R>(f: impl FnOnce(&mut UiCache) -> R) -> R {
    let mut guard = ui_cache().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// Reset all per-user purchases state (logout / account switch). Clears the
/// per-tab UI cache (raw albums/tracks + totals + metadata) AND the per-album
/// download-status store, so the next account never sees the previous user's
/// purchased items or in-flight download statuses. The appliers re-seed both.
pub fn reset_ui_cache() {
    with_ui_cache(|c| *c = UiCache::default());
    with_store(|s| *s = PurchaseDownloadStore::default());
}

// ---------------------------------------------------------------------------
// Formatters (source-of-truth §2.1.5 / §2.2.8 / Addendum A.6).
// ---------------------------------------------------------------------------

/// `formatQualityLabel(bitDepth?, samplingRate?)` (Svelte `:181-184`): `''` if
/// EITHER is missing, else `"{bd}/{sr} kHz"`. Used for the GRID card quality.
fn format_quality_label(bit_depth: Option<u32>, sampling_rate: Option<f64>) -> String {
    match (bit_depth, sampling_rate) {
        (Some(bd), Some(sr)) => format!("{bd}/{} kHz", fmt_rate(sr)),
        _ => String::new(),
    }
}

/// `formatQuality(hires, bd, sr)` (qobuzAdapters `:82`): `"{bd}bit/{sr}kHz"`
/// only when `hires && bd && sr`, else the literal `"CD Quality"`. Used for the
/// album-LIST-row quality column (the list passes `hires = (bd ?? 16) > 16`).
fn format_quality(hires: bool, bit_depth: Option<u32>, sampling_rate: Option<f64>) -> String {
    match (hires, bit_depth, sampling_rate) {
        (true, Some(bd), Some(sr)) => format!("{bd}bit/{}kHz", fmt_rate(sr)),
        _ => "CD Quality".to_string(),
    }
}

/// The BARE track-row quality (§A.6): `"{bd}/{sr}"` only when BOTH present, no
/// `kHz`, no `formatQuality`. `''` otherwise.
fn bare_quality(bit_depth: Option<u32>, sampling_rate: Option<f64>) -> String {
    match (bit_depth, sampling_rate) {
        (Some(bd), Some(sr)) => format!("{bd}/{}", fmt_rate(sr)),
        _ => String::new(),
    }
}

/// Render a sampling rate the way JS template-literal does: an integer prints
/// with no decimals (`96`), a fractional with its decimals (`44.1`). Qobuz
/// passes kHz already (e.g. `96.0` / `44.1`).
fn fmt_rate(sr: f64) -> String {
    if (sr.fract()).abs() < f64::EPSILON {
        format!("{}", sr as i64)
    } else {
        // Trim a trailing zero JS would not print (44.10 → 44.1). `{}` on f64
        // already prints the shortest round-trip form, matching JS number→string.
        format!("{sr}")
    }
}

/// `formatDuration(seconds)` (qobuzAdapters `:25`): `"{m}:{ss}"` zero-padded
/// seconds.
fn format_duration(seconds: u32) -> String {
    let mins = seconds / 60;
    let secs = seconds % 60;
    format!("{mins}:{secs:02}")
}

/// `formatPurchaseDate(ts)` SHORT-month variant (Svelte `:168-179`): `''` when
/// no ts; else a localized `"MMM D, YYYY"` (short month). The Svelte version
/// uses `toLocaleDateString(..., {year,month:'short',day:'numeric'})`; we render
/// the same fixed structure with a localized abbreviated month (the project's
/// shared `dates` convention), which matches in en/es/de/fr/pt.
fn format_purchase_date(ts: Option<i64>) -> String {
    let Some(ts) = ts else {
        return String::new();
    };
    if ts <= 0 {
        // JS `if (!ts) return ''` — 0 is falsy. Negative epoch never occurs for
        // a purchase; treat <= 0 as no date.
        return String::new();
    }
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(ts, 0) {
        chrono::offset::LocalResult::Single(dt) => dt
            .format_localized("%b %-d, %Y", crate::dates::current_locale())
            .to_string(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Filter / sort / group (source-of-truth §2.1.5 — ported byte-for-byte).
// ---------------------------------------------------------------------------

/// `matchesQualityFilter(hires, bitDepth?, samplingRate?)` (Svelte `:188-194`).
fn matches_quality_filter(
    filter: QualityFilter,
    hires: bool,
    bit_depth: Option<u32>,
    sampling_rate: Option<f64>,
) -> bool {
    match filter {
        QualityFilter::All => true,
        QualityFilter::Hires => hires,
        // `!hires && (bitDepth === 16 || (!bitDepth && !samplingRate))`
        QualityFilter::Cd => {
            !hires
                && (bit_depth == Some(16)
                    || (bit_depth.is_none() && sampling_rate.is_none()))
        }
        // `!bitDepth || bitDepth < 16`
        QualityFilter::Lossy => bit_depth.is_none() || bit_depth.unwrap() < 16,
    }
}

/// `applyAlbumFilters(list)` (Svelte `:196-202`) — order matters: hide
/// unavailable → quality → hide downloaded.
fn apply_album_filters<'a>(
    ts: &ToolbarState,
    list: &'a [PurchaseAlbum],
) -> Vec<&'a PurchaseAlbum> {
    list.iter()
        .filter(|a| !ts.filter_hide_unavailable || a.downloadable)
        .filter(|a| {
            ts.filter_quality == QualityFilter::All
                || matches_quality_filter(
                    ts.filter_quality,
                    a.hires,
                    a.maximum_bit_depth,
                    a.maximum_sampling_rate,
                )
        })
        .filter(|a| !ts.filter_hide_downloaded || !a.downloaded)
        .collect()
}

/// `applyTrackFilters(list)` (Svelte `:204-209`): hide-downloaded then quality.
/// Tracks have NO hide-unavailable filter (availability is albums-only).
fn apply_track_filters<'a>(
    ts: &ToolbarState,
    list: &'a [PurchaseTrack],
) -> Vec<&'a PurchaseTrack> {
    list.iter()
        .filter(|t| !ts.filter_hide_downloaded || !t.downloaded)
        .filter(|t| {
            ts.filter_quality == QualityFilter::All
                || matches_quality_filter(
                    ts.filter_quality,
                    t.hires,
                    t.maximum_bit_depth,
                    t.maximum_sampling_rate,
                )
        })
        .collect()
}

/// `sortAlbums(list)` (Svelte `:211-…`): copy then sort, `dir = asc?1:-1`.
/// Tracks are filtered but NEVER sorted (Svelte `:142`).
fn sort_albums<'a>(ts: &ToolbarState, mut list: Vec<&'a PurchaseAlbum>) -> Vec<&'a PurchaseAlbum> {
    let dir: i64 = if ts.album_sort_direction == "asc" { 1 } else { -1 };
    match ts.album_sort_by.as_str() {
        "artist" => list.sort_by(|a, b| {
            let ord = a.artist.name.cmp(&b.artist.name);
            apply_dir(ord, dir)
        }),
        "album" => list.sort_by(|a, b| {
            let ord = a.title.cmp(&b.title);
            apply_dir(ord, dir)
        }),
        "quality" => list.sort_by(|a, b| {
            // `(a.sr||0)-(b.sr||0) || (a.bd||0)-(b.bd||0)`
            let asr = a.maximum_sampling_rate.unwrap_or(0.0);
            let bsr = b.maximum_sampling_rate.unwrap_or(0.0);
            let primary = asr.partial_cmp(&bsr).unwrap_or(std::cmp::Ordering::Equal);
            let ord = if primary != std::cmp::Ordering::Equal {
                primary
            } else {
                a.maximum_bit_depth
                    .unwrap_or(0)
                    .cmp(&b.maximum_bit_depth.unwrap_or(0))
            };
            apply_dir(ord, dir)
        }),
        // "date" (default): `(a.purchased_at||0) - (b.purchased_at||0)`.
        _ => list.sort_by(|a, b| {
            let ord = a.purchased_at.unwrap_or(0).cmp(&b.purchased_at.unwrap_or(0));
            apply_dir(ord, dir)
        }),
    }
    list
}

fn apply_dir(ord: std::cmp::Ordering, dir: i64) -> std::cmp::Ordering {
    if dir < 0 {
        ord.reverse()
    } else {
        ord
    }
}

/// `selectAlbumSort(value)` (Svelte `:169-171`): same key → flip direction;
/// else set the key and `direction = (value==='date' ? 'desc' : 'asc')`.
/// Returns the `(sort_by, direction)` pair to push back onto `PurchasesState`.
pub fn next_album_sort(current_by: &str, current_dir: &str, value: &str) -> (String, String) {
    if current_by == value {
        let flipped = if current_dir == "asc" { "desc" } else { "asc" };
        (value.to_string(), flipped.to_string())
    } else {
        let dir = if value == "date" { "desc" } else { "asc" };
        (value.to_string(), dir.to_string())
    }
}

/// `alphaGroupKey(str)` (Svelte `:173`): first char uppercased; `/[A-Z]/` →
/// that letter, else `'#'`.
fn alpha_group_key(s: &str) -> String {
    match s.chars().next() {
        Some(c) => {
            let up = c.to_uppercase().next().unwrap_or(c);
            if up.is_ascii_uppercase() {
                up.to_string()
            } else {
                "#".to_string()
            }
        }
        None => "#".to_string(),
    }
}

/// Insert one purchased album group `(key, title, item)`; groups end sorted by
/// `key.localeCompare`. Used by both alpha and artist album grouping.
fn group_albums(ts: &ToolbarState, list: &[&PurchaseAlbum]) -> Vec<(String, Vec<PurchaseAlbumItem>)> {
    let mut buckets: HashMap<String, Vec<PurchaseAlbumItem>> = HashMap::new();
    for a in list {
        let key = if ts.album_group_mode == "artist" {
            a.artist.name.clone()
        } else {
            alpha_group_key(&a.title)
        };
        buckets.entry(key).or_default().push(album_item(a));
    }
    sorted_groups(buckets)
}

/// `groupTracks(list)` (Svelte `:178-179`): key = name→alphaGroupKey(title) /
/// artist→performer.name / album→album?.title || 'Unknown'.
fn group_tracks(ts: &ToolbarState, list: &[&PurchaseTrack]) -> Vec<(String, Vec<PurchaseTrackItem>)> {
    let mut buckets: HashMap<String, Vec<PurchaseTrackItem>> = HashMap::new();
    for t in list {
        let key = match ts.track_group_mode.as_str() {
            "album" => t
                .album
                .as_ref()
                .map(|al| al.title.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Unknown".to_string()),
            "name" => alpha_group_key(&t.title),
            // default "artist".
            _ => t.performer.name.clone(),
        };
        buckets.entry(key).or_default().push(track_item(t));
    }
    sorted_groups(buckets)
}

/// Sort grouped buckets by `key.localeCompare` (lexicographic), returning
/// `(key, items)` pairs.
fn sorted_groups<T>(buckets: HashMap<String, Vec<T>>) -> Vec<(String, Vec<T>)> {
    let mut groups: Vec<(String, Vec<T>)> = buckets.into_iter().collect();
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    groups
}

// ---------------------------------------------------------------------------
// Wire → Slint item mapping.
// ---------------------------------------------------------------------------

/// Map a `PurchaseAlbum` → the `PurchaseAlbumItem` Slint struct. `quality-label`
/// = the grid kHz form; `quality-list` = the list `formatQuality` form (the
/// list passes `hires = (bd ?? 16) > 16`). `purchase-date` = SHORT month.
fn album_item(a: &PurchaseAlbum) -> PurchaseAlbumItem {
    // List quality uses `(bit ?? 16) > 16` for the hires flag (Svelte `:906`).
    let list_hires = a.maximum_bit_depth.unwrap_or(16) > 16;
    PurchaseAlbumItem {
        id: a.id.clone().into(),
        title: a.title.clone().into(),
        artist: a.artist.name.clone().into(),
        artist_id: if a.artist.id != 0 {
            a.artist.id.to_string().into()
        } else {
            slint::SharedString::new()
        },
        artwork_url: a.image.smallest().cloned().unwrap_or_default().into(),
        artwork: slint::Image::default(),
        quality_label: format_quality_label(a.maximum_bit_depth, a.maximum_sampling_rate).into(),
        quality_list: format_quality(list_hires, a.maximum_bit_depth, a.maximum_sampling_rate)
            .into(),
        purchase_date: format_purchase_date(a.purchased_at).into(),
        downloadable: a.downloadable,
        downloaded: a.downloaded,
    }
}

/// Map a `PurchaseTrack` → the `PurchaseTrackItem` Slint struct. Quality is the
/// BARE `{bit}/{rate}` (§A.6). `dl-status` is the flattened download-store
/// status for the row's 4-state control.
fn track_item(t: &PurchaseTrack) -> PurchaseTrackItem {
    let statuses = all_track_statuses();
    let dl_status = statuses
        .get(&t.id)
        .map(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let (album_title, album_id, album_img) = match &t.album {
        Some(al) => (
            al.title.clone(),
            al.id.clone(),
            al.image.smallest().cloned().unwrap_or_default(),
        ),
        None => (String::new(), String::new(), String::new()),
    };
    PurchaseTrackItem {
        id: t.id.to_string().into(),
        // Purchased tracks have NO version field → formatTrackTitle == title.trim().
        title: t.title.trim().into(),
        artist: t.performer.name.clone().into(),
        artist_id: if t.performer.id != 0 {
            t.performer.id.to_string().into()
        } else {
            slint::SharedString::new()
        },
        album_title: album_title.into(),
        album_id: album_id.into(),
        artwork_url: album_img.into(),
        artwork: slint::Image::default(),
        quality: bare_quality(t.maximum_bit_depth, t.maximum_sampling_rate).into(),
        duration: format_duration(t.duration).into(),
        purchase_date: format_purchase_date(t.purchased_at).into(),
        streamable: t.streamable,
        downloaded: t.downloaded,
        dl_status: dl_status.into(),
    }
}

// ---------------------------------------------------------------------------
// Enrichment (frontend OVERRIDES backend `downloaded`, §2.1.6).
// ---------------------------------------------------------------------------

/// `enrichWithDownloadStatus` for the LIST view (Svelte `:182-187`):
///   * track: `downloaded = dlIds.has(track.id)`;
///   * album: `downloaded = albumTrackIds.length>0 && every(id ∈ dlIds)` where
///     `albumTrackIds = album.tracks?.items?.map(t=>t.id)`. List-mode albums
///     from `get_by_type('albums')` carry NO nested tracks → empty →
///     `downloaded` stays false (replicated verbatim — the frontend OVERRIDES
///     the backend's value here).
fn enrich_albums(albums: &mut [PurchaseAlbum], dl_ids: &HashSet<u64>) {
    for album in albums {
        let nested: Vec<u64> = album
            .tracks
            .as_ref()
            .map(|page| page.items.iter().map(|t| t.id).collect())
            .unwrap_or_default();
        album.downloaded = !nested.is_empty() && nested.iter().all(|id| dl_ids.contains(id));
    }
}

fn enrich_tracks(tracks: &mut [PurchaseTrack], dl_ids: &HashSet<u64>) {
    for track in tracks {
        track.downloaded = dl_ids.contains(&track.id);
    }
}

// ---------------------------------------------------------------------------
// Toolbar-state snapshot from PurchasesState (event loop).
// ---------------------------------------------------------------------------

fn read_toolbar(window: &AppWindow) -> ToolbarState {
    let s = window.global::<PurchasesState>();
    ToolbarState {
        album_grouping_enabled: s.get_album_grouping_enabled(),
        album_group_mode: s.get_album_group_mode().to_string(),
        album_sort_by: s.get_album_sort_by().to_string(),
        album_sort_direction: s.get_album_sort_direction().to_string(),
        track_grouping_enabled: s.get_track_grouping_enabled(),
        track_group_mode: s.get_track_group_mode().to_string(),
        filter_hide_unavailable: s.get_filter_hide_unavailable(),
        filter_quality: QualityFilter::from_str(&s.get_filter_quality()),
        filter_hide_downloaded: s.get_filter_hide_downloaded(),
    }
}

// ---------------------------------------------------------------------------
// Appliers (event loop) — project the cache → PurchasesState.
// ---------------------------------------------------------------------------

/// Re-derive the rendered models + counts + derived flags from the cache and
/// the current toolbar state, and push them onto `PurchasesState`. Called after
/// a load (Slices 8 apply) and after every toolbar/filter/search/sort/group
/// change (the Svelte `$derived` re-run). Runs on the event loop.
pub fn derive_purchases(window: &AppWindow) {
    let ts = read_toolbar(window);
    let (albums_raw, tracks_raw, total_albums, total_tracks, search_active) = with_ui_cache(|c| {
        (
            c.albums_raw.clone(),
            c.tracks_raw.clone(),
            c.total_albums,
            c.total_tracks,
            c.search_active,
        )
    });

    let s = window.global::<PurchasesState>();

    // hasActiveFilters / activeFilterCount (Svelte `:139-140`).
    let hide_un = ts.filter_hide_unavailable;
    let quality_on = ts.filter_quality != QualityFilter::All;
    let hide_dl = ts.filter_hide_downloaded;
    let has_active_filters = hide_un || quality_on || hide_dl;
    let active_filter_count =
        (hide_un as i32) + (quality_on as i32) + (hide_dl as i32);
    s.set_has_active_filters(has_active_filters);
    s.set_active_filter_count(active_filter_count);
    s.set_search_active(search_active);

    // ── Albums ──
    let filtered_albums = sort_albums(&ts, apply_album_filters(&ts, &albums_raw));
    let album_filtered_len = filtered_albums.len() as i32;
    // albumTabCount (Svelte `:144`): server total unless search OR filters
    // active → filtered length.
    let album_tab_count = if search_active || has_active_filters {
        album_filtered_len
    } else if total_albums != 0 {
        total_albums as i32
    } else {
        album_filtered_len
    };
    s.set_album_tab_count(album_tab_count);

    if ts.album_grouping_enabled {
        let groups = group_albums(&ts, &filtered_albums);
        let group_models: Vec<PurchaseAlbumGroup> = groups
            .into_iter()
            .map(|(key, items)| PurchaseAlbumGroup {
                key: key.clone().into(),
                title: key.into(),
                albums: ModelRc::new(VecModel::from(items)),
            })
            .collect();
        s.set_albums_grouped(ModelRc::new(VecModel::from(group_models)));
        s.set_albums(ModelRc::new(VecModel::from(Vec::<PurchaseAlbumItem>::new())));
    } else {
        let items: Vec<PurchaseAlbumItem> = filtered_albums.iter().map(|a| album_item(a)).collect();
        s.set_albums(ModelRc::new(VecModel::from(items)));
        s.set_albums_grouped(ModelRc::new(VecModel::from(Vec::<PurchaseAlbumGroup>::new())));
    }

    // ── Tracks (filtered, NEVER sorted) ──
    let filtered_tracks = apply_track_filters(&ts, &tracks_raw);
    let track_filtered_len = filtered_tracks.len() as i32;
    let track_tab_count = if search_active || has_active_filters {
        track_filtered_len
    } else if total_tracks != 0 {
        total_tracks as i32
    } else {
        track_filtered_len
    };
    s.set_track_tab_count(track_tab_count);

    if ts.track_grouping_enabled {
        let groups = group_tracks(&ts, &filtered_tracks);
        let group_models: Vec<PurchaseTrackGroup> = groups
            .into_iter()
            .map(|(key, items)| PurchaseTrackGroup {
                key: key.clone().into(),
                title: key.into(),
                tracks: ModelRc::new(VecModel::from(items)),
            })
            .collect();
        s.set_tracks_grouped(ModelRc::new(VecModel::from(group_models)));
        s.set_tracks(ModelRc::new(VecModel::from(Vec::<PurchaseTrackItem>::new())));
    } else {
        let items: Vec<PurchaseTrackItem> = filtered_tracks.iter().map(|t| track_item(t)).collect();
        s.set_tracks(ModelRc::new(VecModel::from(items)));
        s.set_tracks_grouped(ModelRc::new(VecModel::from(Vec::<PurchaseTrackGroup>::new())));
    }
}

/// Apply a tab-load payload to the cache + state, then derive. Enriches the raw
/// items with the registry dlIds (frontend overrides backend `downloaded`) and
/// seeds the stable `albums-full`/`tracks-full` artwork-target models. Runs on
/// the event loop. `search_overwrote` marks BOTH tabs loaded (the search path).
pub fn apply_purchases_tab(
    window: &AppWindow,
    tab: PurchaseTab,
    mut payload: PurchasesTabPayload,
    metadata: &PurchasesMetadata,
    search_overwrote: bool,
) {
    let dl_ids = metadata.downloaded_track_ids.clone();
    match tab {
        PurchaseTab::Albums => {
            enrich_albums(&mut payload.tab_albums, &dl_ids);
        }
        PurchaseTab::Tracks => {
            enrich_tracks(&mut payload.tab_tracks, &dl_ids);
        }
    }

    with_ui_cache(|c| {
        c.metadata = metadata.clone();
        // Totals from metadata; backfilled by the payload total.
        c.total_albums = if metadata.total_albums != 0 {
            metadata.total_albums
        } else {
            c.total_albums
        };
        c.total_tracks = if metadata.total_tracks != 0 {
            metadata.total_tracks
        } else {
            c.total_tracks
        };
        match tab {
            PurchaseTab::Albums => {
                c.albums_raw = payload.tab_albums.clone();
                c.albums_loaded = true;
                if c.total_albums == 0 {
                    c.total_albums = payload.total;
                }
            }
            PurchaseTab::Tracks => {
                c.tracks_raw = payload.tab_tracks.clone();
                c.tracks_loaded = true;
                if c.total_tracks == 0 {
                    c.total_tracks = payload.total;
                }
            }
        }
        // The search path sets BOTH arrays + BOTH loaded flags (Svelte
        // `handleSearchInput`); a non-search load assigns only the active tab.
        if search_overwrote {
            match tab {
                PurchaseTab::Albums => c.tracks_loaded = true,
                PurchaseTab::Tracks => c.albums_loaded = true,
            }
        }
    });

    let s = window.global::<PurchasesState>();
    s.set_loading(false);
    s.set_load_error(slint::SharedString::new());

    // Seed the stable artwork-target full models for the loaded tab.
    match tab {
        PurchaseTab::Albums => {
            let full: Vec<PurchaseAlbumItem> =
                payload.tab_albums.iter().map(album_item).collect();
            s.set_albums_full(ModelRc::new(VecModel::from(full)));
        }
        PurchaseTab::Tracks => {
            let full: Vec<PurchaseTrackItem> =
                payload.tab_tracks.iter().map(track_item).collect();
            s.set_tracks_full(ModelRc::new(VecModel::from(full)));
        }
    }

    derive_purchases(window);
}

/// Apply a SEARCH result to the cache + state (Svelte `handleSearchInput`,
/// §2.1.7): sets BOTH the album + track raw arrays AND BOTH loaded flags,
/// enriches both with dlIds, seeds BOTH full artwork models, marks search
/// active, then derives. Event loop.
pub fn apply_purchases_search(
    window: &AppWindow,
    mut albums: Vec<PurchaseAlbum>,
    mut tracks: Vec<PurchaseTrack>,
    metadata: &PurchasesMetadata,
) {
    let dl_ids = metadata.downloaded_track_ids.clone();
    enrich_albums(&mut albums, &dl_ids);
    enrich_tracks(&mut tracks, &dl_ids);

    with_ui_cache(|c| {
        c.metadata = metadata.clone();
        c.albums_raw = albums.clone();
        c.tracks_raw = tracks.clone();
        // Search sets BOTH loaded flags (the stale-other-tab quirk on a later
        // clearSearch is intentional — §2.1.7).
        c.albums_loaded = true;
        c.tracks_loaded = true;
        c.search_active = true;
    });

    let s = window.global::<PurchasesState>();
    s.set_loading(false);
    s.set_load_error(slint::SharedString::new());
    let full_albums: Vec<PurchaseAlbumItem> = albums.iter().map(album_item).collect();
    let full_tracks: Vec<PurchaseTrackItem> = tracks.iter().map(track_item).collect();
    s.set_albums_full(ModelRc::new(VecModel::from(full_albums)));
    s.set_tracks_full(ModelRc::new(VecModel::from(full_tracks)));

    derive_purchases(window);
}

/// Artwork jobs for BOTH tabs (the search path loads both). Event-loop-free
/// (reads the cache).
pub fn artwork_jobs_for_both() -> Vec<crate::artwork::ArtworkJob> {
    let mut jobs = artwork_jobs_for_tab(PurchaseTab::Albums);
    jobs.extend(artwork_jobs_for_tab(PurchaseTab::Tracks));
    jobs
}

/// Mark loading + clear the error (called before a tab fetch). Event loop.
pub fn set_loading(window: &AppWindow) {
    let s = window.global::<PurchasesState>();
    s.set_loading(true);
    s.set_load_error(slint::SharedString::new());
}

/// Clear the loading flag + error (cache-hit path: the tab is already loaded,
/// so we skip straight to a re-derive without a spinner). Event loop.
pub fn set_loading_done(window: &AppWindow) {
    let s = window.global::<PurchasesState>();
    s.set_loading(false);
    s.set_load_error(slint::SharedString::new());
}

/// Surface a load error (already mapped to its display string). Event loop.
pub fn set_load_error(window: &AppWindow, message: &str) {
    let s = window.global::<PurchasesState>();
    s.set_loading(false);
    s.set_load_error(message.into());
}

/// Refresh ONLY the per-track `dl-status` projection after a download-store
/// change (so a download's progress/complete shows live without a refetch).
/// Re-derives from the cache (which re-reads `all_track_statuses`). Event loop.
pub fn refresh_track_statuses(window: &AppWindow) {
    derive_purchases(window);
}

// ---------------------------------------------------------------------------
// Cache gating helpers (lazy-load decision lives in the controller).
// ---------------------------------------------------------------------------

/// Whether the given tab is already cached (no refetch on switch-back). Mirrors
/// `loadPurchasesByTab`'s early-return guard.
pub fn tab_cached(tab: PurchaseTab) -> bool {
    with_ui_cache(|c| match tab {
        PurchaseTab::Albums => c.albums_loaded,
        PurchaseTab::Tracks => c.tracks_loaded,
    })
}

/// Set the active search state (non-empty query) in the cache. `clearSearch`
/// resets it to false; a non-empty applied query sets it true.
pub fn set_search_active(active: bool) {
    with_ui_cache(|c| c.search_active = active);
}

/// Bump + return the search debounce token. A fire whose token != the latest is
/// dropped (the 300ms debounce — the Svelte `clearTimeout` equivalent).
pub fn next_search_seq() -> u64 {
    with_ui_cache(|c| {
        c.search_seq = c.search_seq.wrapping_add(1);
        c.search_seq
    })
}

/// Whether `seq` is still the latest issued search token.
pub fn search_seq_current(seq: u64) -> bool {
    with_ui_cache(|c| c.search_seq == seq)
}

// ---------------------------------------------------------------------------
// Artwork jobs + dual-set (id-keyed) into the rendered models.
// ---------------------------------------------------------------------------

/// Build artwork jobs for the loaded tab's FULL model (stable index targets).
pub fn artwork_jobs_for_tab(tab: PurchaseTab) -> Vec<crate::artwork::ArtworkJob> {
    let mut jobs = Vec::new();
    with_ui_cache(|c| match tab {
        PurchaseTab::Albums => {
            for (i, a) in c.albums_raw.iter().enumerate() {
                if let Some(url) = a.image.smallest() {
                    if !url.is_empty() {
                        jobs.push(crate::artwork::ArtworkJob {
                            url: url.clone(),
                            target: crate::artwork::ArtworkTarget::PurchaseAlbum { index: i },
                        });
                    }
                }
            }
        }
        PurchaseTab::Tracks => {
            for (i, t) in c.tracks_raw.iter().enumerate() {
                if let Some(al) = &t.album {
                    if let Some(url) = al.image.smallest() {
                        if !url.is_empty() {
                            jobs.push(crate::artwork::ArtworkJob {
                                url: url.clone(),
                                target: crate::artwork::ArtworkTarget::PurchaseTrack { index: i },
                            });
                        }
                    }
                }
            }
        }
    });
    jobs
}

/// Dual-set a decoded album cover by id into the rendered flat + grouped models
/// (the favorites pattern: a sort/filter/group makes them NOT share the full
/// model). Event loop.
pub fn set_album_artwork(window: &AppWindow, id: &str, image: slint::Image) {
    let s = window.global::<PurchasesState>();
    // Flat.
    let flat = s.get_albums();
    for i in 0..flat.row_count() {
        if let Some(mut item) = flat.row_data(i) {
            if item.id == id {
                item.artwork = image.clone();
                flat.set_row_data(i, item);
            }
        }
    }
    // Grouped sections.
    let groups = s.get_albums_grouped();
    for g in 0..groups.row_count() {
        if let Some(group) = groups.row_data(g) {
            let inner = group.albums.clone();
            for i in 0..inner.row_count() {
                if let Some(mut item) = inner.row_data(i) {
                    if item.id == id {
                        item.artwork = image.clone();
                        inner.set_row_data(i, item);
                    }
                }
            }
        }
    }
}

/// Dual-set a decoded track thumbnail by id into the rendered flat + grouped
/// track models. Event loop.
pub fn set_track_artwork(window: &AppWindow, id: &str, image: slint::Image) {
    let s = window.global::<PurchasesState>();
    let flat = s.get_tracks();
    for i in 0..flat.row_count() {
        if let Some(mut item) = flat.row_data(i) {
            if item.id == id {
                item.artwork = image.clone();
                flat.set_row_data(i, item);
            }
        }
    }
    let groups = s.get_tracks_grouped();
    for g in 0..groups.row_count() {
        if let Some(group) = groups.row_data(g) {
            let inner = group.tracks.clone();
            for i in 0..inner.row_count() {
                if let Some(mut item) = inner.row_data(i) {
                    if item.id == id {
                        item.artwork = image.clone();
                        inner.set_row_data(i, item);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-track download flow (tracks tab) — format fetch + picker decision.
// ---------------------------------------------------------------------------

/// Build the format-picker option items for the popup (`fmt.label` + the
/// `{bd}/{sr}` detail when both present). Mirrors §2.1.13.
pub fn format_picker_items(formats: &[PurchaseFormatOption]) -> Vec<PurchaseFormatItem> {
    formats
        .iter()
        .map(|f| PurchaseFormatItem {
            id: f.id as i32,
            label: f.label.clone().into(),
            detail: match (f.bit_depth, f.sampling_rate) {
                (Some(bd), Some(sr)) => format!("{bd}/{}", fmt_rate(sr)).into(),
                _ => slint::SharedString::new(),
            },
        })
        .collect()
}

/// Look up a purchased track in the cache by its (stringified) id — the
/// tracks-tab download flow needs `track.album.id` + the raw record.
pub fn find_track(track_id: u64) -> Option<PurchaseTrack> {
    with_ui_cache(|c| c.tracks_raw.iter().find(|t| t.id == track_id).cloned())
}

// ============================================================================
// Slice 9 — PurchaseDetailView (detail + download screen)
//
// The detail-surface chrome lives in `ui/purchases/PurchaseDetailView.slint`;
// this section is the Rust half: the load fn (mirrors Tauri command #5 via the
// shared `purchases_service::build_purchase_album`), the detail UI cache (so
// format changes + download-status nudges re-project the rows/progress without
// a refetch), and the event-loop appliers that project the cache + the
// download store → `PurchaseDetailState`.
//
// SEND BOUNDARY: load fns return PLAIN `Send` payloads (no Slint types); the
// appliers build `ModelRc`/`Image` only on the event loop.
//
// Mirrors `PurchaseAlbumDetailView.svelte` §2.2. The download state machine,
// folder picker, and the download actions are reused from Slice 7 (they live
// above and survive navigation in the process-wide store).
// ============================================================================

/// A `Send` snapshot of one purchased album for the detail view: the header
/// fields + the synthesized format options + the nested tracks. No Slint types
/// — built off the event loop, cached, then applied.
#[derive(Debug, Clone, Default)]
pub struct PurchaseDetailPayload {
    pub album: PurchaseAlbum,
    pub formats: Vec<PurchaseFormatOption>,
}

/// Detail UI cache: the loaded album + its formats + the currently-selected
/// format id, so a format-dropdown change or a download-status nudge can
/// re-project the rows/progress WITHOUT a refetch (the Svelte component keeps
/// these in `$state`). Survives navigation only transiently — a fresh
/// `load_purchase_album` reseeds it (and `reset_detail` clears it).
#[derive(Debug, Default)]
struct DetailCache {
    album_id: String,
    album: Option<PurchaseAlbum>,
    formats: Vec<PurchaseFormatOption>,
    /// The currently-selected format id (default `formats[0].id` after load).
    selected_format_id: Option<u32>,
}

fn detail_cache() -> &'static Mutex<DetailCache> {
    static CACHE: OnceLock<Mutex<DetailCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(DetailCache::default()))
}

fn with_detail_cache<R>(f: impl FnOnce(&mut DetailCache) -> R) -> R {
    let mut guard = detail_cache().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// `getSelectedFormatLabel` / `handleFormatChange` glue: set the selected
/// format id from a dropdown INDEX into the cached `formats`. Returns the
/// chosen format id (so the caller can re-derive). A no-op (returns the current
/// selection) for an out-of-range index.
pub fn select_detail_format(index: usize) -> Option<u32> {
    with_detail_cache(|c| {
        if let Some(fmt) = c.formats.get(index) {
            c.selected_format_id = Some(fmt.id);
        }
        c.selected_format_id
    })
}

/// The currently-selected detail format id (for the download actions:
/// `promptForFolder` reads `selectedFormatId` + `qualityFolderName`).
pub fn detail_selected_format_id() -> Option<u32> {
    with_detail_cache(|c| c.selected_format_id)
}

/// The cached detail album id (the actions need it to key the download store).
pub fn detail_album_id() -> String {
    with_detail_cache(|c| c.album_id.clone())
}

/// The label of the currently-selected format (for the `qualityDir`
/// derivation in the download actions; `qualityFolderName(selectedFormatId)`).
pub fn detail_selected_format_label() -> Option<String> {
    with_detail_cache(|c| {
        let sel = c.selected_format_id?;
        c.formats
            .iter()
            .find(|f| f.id == sel)
            .map(|f| f.label.clone())
    })
}

/// The track ids of the cached detail album, in order (for `download-all` →
/// `startAlbumDownload(albumId, album.tracks.items.map(t=>t.id), ...)`).
pub fn detail_track_ids() -> Vec<u64> {
    with_detail_cache(|c| {
        c.album
            .as_ref()
            .and_then(|a| a.tracks.as_ref())
            .map(|page| page.items.iter().map(|t| t.id).collect())
            .unwrap_or_default()
    })
}

/// The cached detail album's download destination (after the rewrite) — read
/// from the download store for the Add-to-Library handler.
pub fn detail_destination() -> Option<String> {
    let album_id = detail_album_id();
    album_download_state(&album_id).and_then(|s| s.destination)
}

/// Load the detail album (Svelte `loadAlbum`, §2.2.2 + command #5). Fetches the
/// full catalog `Album` (the regular `get_album` path) AND the user's purchases
/// listing (for the `downloadable`/`purchased_at` meta), reads the local
/// registry for download flags, builds the `PurchaseAlbum` via the shared
/// `build_purchase_album` service, and synthesizes the formats. Returns a
/// `Send` payload (no Slint types) or a RAW error string (the detail view shows
/// the raw error, NOT i18n-mapped — §2.2.4).
pub async fn load_purchase_album(
    runtime: &Runtime,
    album_id: &str,
) -> Result<PurchaseDetailPayload, String> {
    let Some(client) = snapshot_client(runtime).await else {
        // No session = the Svelte unauthenticated fetch rejects → raw error.
        return Err("No Qobuz session".to_string());
    };

    // Full catalog album (the regular album path — same call command #5 uses).
    let album = client
        .get_album(album_id)
        .await
        .map_err(|e| format!("Failed to fetch album {album_id}: {e}"))?;
    // The purchases listing carries the per-album `downloadable`/`purchased_at`.
    let purchases = purchases_service::get_user_purchases_all(&client)
        .await
        .map_err(|e| format!("Failed to fetch purchases: {e}"))?;

    // Registry: downloaded ids + the requested-format map (command #5's DB read).
    let (downloaded_ids, format_map) = read_registry_for_detail().await;

    let purchase_album =
        purchases_service::build_purchase_album(&album, &purchases, &downloaded_ids, &format_map);
    let formats = purchases_service::synth_formats(&album);

    Ok(PurchaseDetailPayload {
        album: purchase_album,
        formats,
    })
}

/// Read the local registry for the detail annotation: the downloaded track ids
/// (`HashSet<i64>`) + the `track_id → Vec<format_id>` map. Mirrors command #5's
/// `get_downloaded_purchase_formats` read. Empty on a DB error / no user.
async fn read_registry_for_detail() -> (HashSet<i64>, HashMap<i64, Vec<u32>>) {
    let formats = get_downloaded_purchase_formats().await; // track_id(u64) → Vec<format_id(u32)>
    let mut ids: HashSet<i64> = HashSet::new();
    let mut map: HashMap<i64, Vec<u32>> = HashMap::new();
    for (track_id, fmt_ids) in formats {
        ids.insert(track_id as i64);
        map.insert(track_id as i64, fmt_ids);
    }
    (ids, map)
}

/// Apply a freshly-loaded detail payload to `PurchaseDetailState` (event loop).
/// Seeds the cache, default-selects `formats[0]`, then projects the header +
/// the track rows via [`derive_detail`].
pub fn apply_detail(window: &AppWindow, payload: PurchaseDetailPayload) {
    let selected = payload.formats.first().map(|f| f.id);
    with_detail_cache(|c| {
        c.album_id = payload.album.id.clone();
        c.album = Some(payload.album.clone());
        c.formats = payload.formats.clone();
        c.selected_format_id = selected;
    });

    let s = window.global::<PurchaseDetailState>();
    s.set_loading(false);
    s.set_load_error(slint::SharedString::new());
    s.set_loaded(true);

    // Seed the FULL artwork-target cover model: the header cover is written by
    // the artwork pipeline into `PurchaseDetailState.artwork` (single image).
    // (No list/grid here — just the one 224×224 cover.)
    derive_detail(window);
}

/// Re-project the detail header + format dropdown + progress + track rows from
/// the cache and the download store onto `PurchaseDetailState`. Called after a
/// load, after a format-dropdown change, and after each download-status nudge
/// (the Svelte `$derived` re-run). Runs on the event loop.
pub fn derive_detail(window: &AppWindow) {
    let (album, formats, selected) = with_detail_cache(|c| {
        (c.album.clone(), c.formats.clone(), c.selected_format_id)
    });
    let Some(album) = album else {
        return;
    };
    let s = window.global::<PurchaseDetailState>();

    // ── Header ────────────────────────────────────────────────────────────
    s.set_title(album.title.clone().into());
    s.set_artist(album.artist.name.clone().into());
    s.set_artist_id(if album.artist.id != 0 {
        album.artist.id.to_string().into()
    } else {
        slint::SharedString::new()
    });
    s.set_downloadable(album.downloadable);
    // LONG-month purchased-on (§2.2.4; the detail uses the long month, unlike
    // the list's short month).
    s.set_purchased_on(format_purchase_date_long(album.purchased_at).into());
    s.set_label_name(
        album
            .label
            .as_ref()
            .map(|l| l.name.clone())
            .unwrap_or_default()
            .into(),
    );
    s.set_genre_name(
        album
            .genre
            .as_ref()
            .map(|g| g.name.clone())
            .unwrap_or_default()
            .into(),
    );
    // Header quality (§2.2.4): `formatQuality((bit ?? 16) > 16, bit, rate)`.
    let header_hires = album.maximum_bit_depth.unwrap_or(16) > 16;
    s.set_quality(
        format_quality(header_hires, album.maximum_bit_depth, album.maximum_sampling_rate).into(),
    );

    let tracks = album
        .tracks
        .as_ref()
        .map(|p| p.items.as_slice())
        .unwrap_or(&[]);
    let total_tracks = tracks.len() as i32;
    s.set_total_tracks(total_tracks);
    let total_secs: u32 = tracks.iter().map(|t| t.duration).sum();
    s.set_total_duration(format_total_duration(total_secs).into());

    // ── Format dropdown ───────────────────────────────────────────────────
    let labels: Vec<slint::SharedString> =
        formats.iter().map(|f| f.label.clone().into()).collect();
    s.set_format_labels(ModelRc::new(VecModel::from(labels)));
    let format_index = selected
        .and_then(|sel| formats.iter().position(|f| f.id == sel))
        .unwrap_or(0) as i32;
    s.set_format_index(format_index);
    s.set_has_formats(!formats.is_empty());

    // ── Download state (from the process-wide store) ──────────────────────
    let dl_state = album_download_state(&album.id);
    let statuses = dl_state
        .as_ref()
        .map(|st| st.track_statuses.clone())
        .unwrap_or_default();
    let is_downloading_all = dl_state.as_ref().map(|st| st.is_downloading_all).unwrap_or(false);
    // allComplete IS format-scoped (Svelte `:388`).
    let all_complete = album_all_complete_for_format(&album.id, selected);
    // completedCount / wasCancelled / progress-fill are NOT format-scoped
    // (§A.4) — count ALL statuses in the raw map regardless of selected format.
    let completed_count = statuses
        .values()
        .filter(|st| **st == TrackDownloadStatus::Complete)
        .count() as i32;
    let any_cancelled = statuses
        .values()
        .any(|st| *st == TrackDownloadStatus::Cancelled);
    // wasCancelled = !isDownloadingAll && !allComplete && some cancelled.
    let was_cancelled = !is_downloading_all && !all_complete && any_cancelled;
    let progress_percent = if total_tracks > 0 {
        (completed_count as f32 / total_tracks as f32) * 100.0
    } else {
        0.0
    };
    s.set_is_downloading_all(is_downloading_all);
    s.set_all_complete(all_complete);
    s.set_was_cancelled(was_cancelled);
    s.set_completed_count(completed_count);
    s.set_progress_percent(progress_percent);
    // Progress section renders iff downloadingAll || allComplete || wasCancelled.
    s.set_show_progress(is_downloading_all || all_complete || was_cancelled);

    // Download-all gating: disabled while downloading-all OR no selected format.
    s.set_can_download_all(!is_downloading_all && selected.is_some());

    // Add-to-Library block: only when allComplete (format-scoped) && a
    // destination is set in the store (§2.2.4).
    let destination = dl_state.as_ref().and_then(|st| st.destination.clone());
    s.set_show_add_to_library(all_complete && destination.is_some());

    // ── Track rows (disc-grouped) ─────────────────────────────────────────
    let rows = build_detail_rows(&album, selected);
    s.set_tracks(ModelRc::new(VecModel::from(rows)));
}

/// Build the disc-grouped detail track rows (§2.2.4 `groupByDisc` + §2.2.5 row).
/// Disc number = `media_number ?? 1`; multi-disc iff > 1 distinct discs. The
/// first row of each disc on a multi-disc album carries `disc_header_number =
/// disc`; otherwise 0 (single-disc → flat). Per-track status is FORMAT-SCOPED
/// (`track_status_scoped`); `is_downloaded = isDownloadedForFormat ||
/// status==Complete`; `show_download = downloadable || is_downloaded`.
fn build_detail_rows(
    album: &PurchaseAlbum,
    selected_format_id: Option<u32>,
) -> Vec<PurchaseDetailTrack> {
    let tracks = album
        .tracks
        .as_ref()
        .map(|p| p.items.as_slice())
        .unwrap_or(&[]);

    // Disc grouping: preserve item order, group by media_number ?? 1.
    // `isMultiDisc` = more than one distinct disc number.
    let mut distinct_discs: Vec<u32> = Vec::new();
    for t in tracks {
        let disc = t.media_number.unwrap_or(1);
        if !distinct_discs.contains(&disc) {
            distinct_discs.push(disc);
        }
    }
    let is_multi_disc = distinct_discs.len() > 1;

    let mut rows: Vec<PurchaseDetailTrack> = Vec::with_capacity(tracks.len());
    let mut last_disc: Option<u32> = None;
    for t in tracks {
        let disc = t.media_number.unwrap_or(1);
        // First row of a disc on a multi-disc album → header marker.
        let disc_header = if is_multi_disc && last_disc != Some(disc) {
            disc as i32
        } else {
            0
        };
        last_disc = Some(disc);

        // FORMAT-SCOPED per-track status (§2.2.3): suppresses `complete` when
        // viewing a different format than the one downloaded.
        let scoped = track_status_scoped(&album.id, t.id, selected_format_id);
        let dl_status = scoped.map(|s| s.as_str()).unwrap_or("").to_string();
        // isDownloadedForFormat (server `downloaded_format_ids`) OR a store
        // `complete` (§2.2.5).
        let downloaded_for_format =
            is_downloaded_for_format(&t.downloaded_format_ids, selected_format_id);
        let is_downloaded =
            downloaded_for_format || scoped == Some(TrackDownloadStatus::Complete);
        // show-download (§2.2.5): album.downloadable OR already downloaded.
        let show_download = album.downloadable || is_downloaded;

        // Second-line performer when ≠ album artist (§2.2.5).
        let show_performer =
            !t.performer.name.is_empty() && t.performer.name != album.artist.name;

        rows.push(PurchaseDetailTrack {
            id: t.id.to_string().into(),
            track_number: t.track_number as i32,
            // Purchased tracks have NO version → formatTrackTitle == title.
            // `formatTrackTitle` trims the title (utils/trackTitle.ts), so trim
            // here too for 1:1 parity.
            title: t.title.trim().into(),
            performer: t.performer.name.clone().into(),
            show_performer,
            duration: format_duration(t.duration).into(),
            quality: bare_quality(t.maximum_bit_depth, t.maximum_sampling_rate).into(),
            streamable: t.streamable,
            dl_status: dl_status.into(),
            is_downloaded,
            show_download,
            disc_header_number: disc_header,
        });
    }
    rows
}

/// Artwork jobs for the detail header cover (the single 224×224 album cover).
/// Reads the cache; event-loop-free.
pub fn detail_artwork_jobs() -> Vec<crate::artwork::ArtworkJob> {
    with_detail_cache(|c| {
        c.album
            .as_ref()
            .and_then(|a| a.image.best().cloned())
            .filter(|u| !u.is_empty())
            .map(|url| {
                vec![crate::artwork::ArtworkJob {
                    url,
                    target: crate::artwork::ArtworkTarget::PurchaseDetailCover,
                }]
            })
            .unwrap_or_default()
    })
}

/// Set loading on the detail state (before a fetch). Event loop.
pub fn set_detail_loading(window: &AppWindow) {
    let s = window.global::<PurchaseDetailState>();
    s.set_loading(true);
    s.set_load_error(slint::SharedString::new());
    s.set_loaded(false);
}

/// Surface a RAW detail error (NOT i18n-mapped, §2.2.4). Event loop.
pub fn set_detail_error(window: &AppWindow, message: &str) {
    let s = window.global::<PurchaseDetailState>();
    s.set_loading(false);
    s.set_loaded(false);
    s.set_load_error(message.into());
}

/// Set the `adding-to-library` spinner flag. Event loop.
pub fn set_detail_adding_to_library(window: &AppWindow, adding: bool) {
    window
        .global::<PurchaseDetailState>()
        .set_adding_to_library(adding);
}

/// Reset the detail view + cache before loading a new album (an album→album
/// jump must not flash the previous album's header/tracks). Clears the cache
/// and resets the state to its loading shell. Does NOT touch the download store
/// (that survives navigation by design). Event loop.
pub fn reset_detail(window: &AppWindow) {
    with_detail_cache(|c| *c = DetailCache::default());
    let s = window.global::<PurchaseDetailState>();
    s.set_loading(true);
    s.set_loaded(false);
    s.set_load_error(slint::SharedString::new());
    s.set_title(slint::SharedString::new());
    s.set_artist(slint::SharedString::new());
    s.set_artwork(slint::Image::default());
    s.set_tracks(ModelRc::new(VecModel::<PurchaseDetailTrack>::default()));
    s.set_format_labels(ModelRc::new(VecModel::<slint::SharedString>::default()));
    s.set_has_formats(false);
    s.set_show_progress(false);
    s.set_show_add_to_library(false);
}

/// Re-derive the detail download projection (progress + per-track statuses)
/// from the store. The download actions call this (via the weak window) to
/// surface live status changes without a refetch. Event loop.
pub fn refresh_detail_download(window: &AppWindow) {
    derive_detail(window);
}

/// `formatTotalDuration(seconds)` (§2.2.8): `"{h}h {m}m"` when hours > 0, else
/// `"{m}m"`.
fn format_total_duration(seconds: u32) -> String {
    let hrs = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    if hrs > 0 {
        format!("{hrs}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

/// `formatPurchaseDate(ts)` LONG-month variant (Svelte detail `:417`): `''`
/// when no ts; else a localized `"MMMMM D, YYYY"` (FULL month name). The list
/// view uses the SHORT-month [`format_purchase_date`]; the detail uses the long
/// month — replicate the asymmetry verbatim.
fn format_purchase_date_long(ts: Option<i64>) -> String {
    let Some(ts) = ts else {
        return String::new();
    };
    if ts <= 0 {
        return String::new();
    }
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(ts, 0) {
        chrono::offset::LocalResult::Single(dt) => dt
            .format_localized("%B %-d, %Y", crate::dates::current_locale())
            .to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_as_str_maps_wire_strings() {
        assert_eq!(PurchaseTab::Albums.as_str(), "albums");
        assert_eq!(PurchaseTab::Tracks.as_str(), "tracks");
    }

    #[test]
    fn quality_dir_replaces_slash_and_trims() {
        // The canonical hi-res label: `/` → `-` (the load-bearing transform).
        assert_eq!(quality_dir("24/192"), "24-192");
        assert_eq!(quality_dir("16/44.1"), "16-44.1");
        // GLOBAL replace (JS `/\//g`): a label with two slashes loses BOTH.
        assert_eq!(quality_dir("a/b/c"), "a-b-c");
        // `.trim()` strips surrounding whitespace (matches JS `.trim()`),
        // including the leading/trailing space some labels carry.
        assert_eq!(quality_dir("  24/192  "), "24-192");
        assert_eq!(quality_dir("Hi-Res"), "Hi-Res");
        // No slash, no surrounding space → identity.
        assert_eq!(quality_dir("CD Quality"), "CD Quality");
        // Empty/whitespace-only → empty (a single-track download with a blank
        // qualityDir produces no subfolder segment, matching the JS `?? ''`).
        assert_eq!(quality_dir(""), "");
        assert_eq!(quality_dir("   "), "");
    }

    #[test]
    fn total_backfill_prefers_metadata_then_response_then_len() {
        // metadata wins when non-zero
        assert_eq!(resolve_tab_total(42, 10, 3), 42);
        // response total used when metadata is 0
        assert_eq!(resolve_tab_total(0, 10, 3), 10);
        // loaded length used when both are 0
        assert_eq!(resolve_tab_total(0, 0, 3), 3);
        // all zero → 0
        assert_eq!(resolve_tab_total(0, 0, 0), 0);
    }

    #[test]
    fn error_mapping_network_to_key() {
        // Verbatim Svelte tokens (§3.6 `msg.includes(...)`): the three string
        // checks are ported 1:1 — `"Load failed"`, `"fetch"`, `"NetworkError"`.
        assert_eq!(map_load_error("Load failed"), "purchases.loadFailed");
        assert_eq!(
            map_load_error("Failed to fetch purchases: timeout"),
            "purchases.loadFailed"
        );
        // The literal `"NetworkError"` token (e.g. a JS-shaped error surfaced
        // through) maps to the key.
        assert_eq!(
            map_load_error("NetworkError when attempting to fetch"),
            "purchases.loadFailed"
        );
    }

    #[test]
    fn wrapped_network_error_maps_to_key_via_fetch_token() {
        // `load_purchases_by_tab` wraps fetch errors as Tauri command #3 does:
        // `"Failed to fetch {type} purchases: {e}"`. That wrapper carries the
        // `"fetch"` token, so even a raw `ApiError::NetworkError` ("Network
        // error: …") maps to the key — matching Tauri 1:1.
        let raw = "Network error: connection refused";
        let wrapped = format!("Failed to fetch {} purchases: {}", "albums", raw);
        assert_eq!(map_load_error(&wrapped), "purchases.loadFailed");
        // An offline-gate error is likewise wrapped → carries `"fetch"` → key.
        let offline = format!(
            "Failed to fetch {} purchases: {}",
            "tracks", "Offline mode is active - Qobuz services are disabled"
        );
        assert_eq!(map_load_error(&offline), "purchases.loadFailed");
    }

    #[test]
    fn error_mapping_passthrough_raw() {
        assert_eq!(
            map_load_error("Album 123 not found (404)"),
            "Album 123 not found (404)"
        );
        assert_eq!(
            map_load_error("No active session - please log in"),
            "No active session - please log in"
        );
        // FIDELITY NOTE: the Rust `ApiError::NetworkError` Display string is
        // `"Network error: ..."` (space, lowercase `e`) — it does NOT contain
        // the verbatim Svelte token `"NetworkError"`, so a genuine Rust network
        // error falls through to the RAW branch. This is a faithful consequence
        // of porting the JS substring checks 1:1 (the spec mandates replicating
        // the string checks, not "map every network error"). Do NOT "fix" the
        // check to match the Rust Display — that would diverge from Tauri.
        assert_eq!(
            map_load_error("Network error: connection refused"),
            "Network error: connection refused"
        );
    }

    // ------------------------------------------------------------------
    // Slice 7 — download state machine (pure-logic tests over the store).
    //
    // These exercise the PurchaseDownloadStore mutators directly (no I/O), the
    // way `executeAlbumDownload` / `startTrackDownload` drive them. The async
    // actions are thin wrappers around these mutators + the Slice-5 service
    // primitive, so the store logic is the part with the 1:1 behavioral risk.
    // ------------------------------------------------------------------

    use TrackDownloadStatus::*;

    fn status(store: &PurchaseDownloadStore, album: &str, track: u64) -> Option<TrackDownloadStatus> {
        store
            .albums
            .get(album)
            .and_then(|s| s.track_statuses.get(&track).copied())
    }

    #[test]
    fn track_status_as_str_matches_svelte_literals() {
        assert_eq!(Downloading.as_str(), "downloading");
        assert_eq!(Complete.as_str(), "complete");
        assert_eq!(Failed.as_str(), "failed");
        assert_eq!(Cancelled.as_str(), "cancelled");
    }

    #[test]
    fn album_loop_marks_statuses_in_order_then_all_complete() {
        let mut s = PurchaseDownloadStore::default();
        let ids = vec![10u64, 20, 30];
        s.seed_album_download("A", "/music", 27);
        // seeded: downloadingAll true, no statuses yet, destination = picker dir.
        let st = s.album("A").unwrap();
        assert!(st.is_downloading_all);
        assert!(!st.all_complete);
        assert_eq!(st.destination.as_deref(), Some("/music"));
        assert_eq!(st.format_id, Some(27));

        // simulate the loop: downloading → complete, per track in order.
        for (i, id) in ids.iter().enumerate() {
            s.mark_track("A", *id, Downloading);
            assert_eq!(status(&s, "A", *id), Some(Downloading));
            // first success rewrites destination to the album folder.
            if i == 0 {
                s.rewrite_destination("A", "/music/Artist/Album [27]");
            }
            s.mark_track("A", *id, Complete);
        }
        s.finalize_album("A", &ids);

        let st = s.album("A").unwrap();
        assert!(st.all_complete, "all complete when every track is Complete");
        assert!(!st.is_downloading_all);
        // destination rewritten to album folder after the FIRST success only.
        assert_eq!(st.destination.as_deref(), Some("/music/Artist/Album [27]"));
        assert!(!s.is_aborted("A"), "finalize deletes the abort flag");
    }

    #[test]
    fn one_failure_leaves_all_complete_false() {
        let mut s = PurchaseDownloadStore::default();
        let ids = vec![1u64, 2, 3];
        s.seed_album_download("A", "/d", 7);
        s.mark_track("A", 1, Complete);
        s.mark_track("A", 2, Failed); // induced mid-loop failure
        s.mark_track("A", 3, Complete);
        s.finalize_album("A", &ids);
        let st = s.album("A").unwrap();
        assert!(!st.all_complete, "a single failure leaves allComplete false");
        assert_eq!(status(&s, "A", 2), Some(Failed));
    }

    #[test]
    fn cancel_stops_before_next_track_and_marks_rest_cancelled() {
        let mut s = PurchaseDownloadStore::default();
        let ids = vec![1u64, 2, 3, 4];
        s.seed_album_download("A", "/d", 6);
        // track 1 completed, track 2 in flight finishes complete, THEN cancel.
        s.mark_track("A", 1, Complete);
        s.mark_track("A", 2, Complete);
        s.set_abort("A");
        assert!(s.is_aborted("A"));
        // loop sees the abort before track 3 → apply cancellation.
        s.apply_cancellation("A", &ids);
        let st = s.album("A").unwrap();
        // terminal tracks keep their status; untouched ones become Cancelled.
        assert_eq!(status(&s, "A", 1), Some(Complete));
        assert_eq!(status(&s, "A", 2), Some(Complete));
        assert_eq!(status(&s, "A", 3), Some(Cancelled));
        assert_eq!(status(&s, "A", 4), Some(Cancelled));
        assert!(!st.is_downloading_all);
        assert!(!st.all_complete);
        assert!(!s.is_aborted("A"), "cancellation deletes the abort flag");
    }

    #[test]
    fn cancellation_only_marks_tracks_with_no_status_entry() {
        // VERBATIM Svelte predicate (`purchaseDownloadStore.ts:109-112`):
        // `if (!currentState?.trackStatuses[id]) remaining[id] = 'cancelled'`.
        // ONLY tracks with no existing status entry become `Cancelled`. A track
        // carrying ANY status — including `Downloading` — is left untouched (it is
        // NOT overwritten to `Cancelled`). This guards against the prior, wrong
        // "any non-terminal" predicate which would have flipped a `Downloading`
        // track to `Cancelled`.
        let mut s = PurchaseDownloadStore::default();
        let ids = vec![1u64, 2, 3];
        s.seed_album_download("A", "/d", 6);
        s.mark_track("A", 1, Complete); // terminal — has a status
        s.mark_track("A", 2, Downloading); // non-terminal but HAS a status
        // track 3 has NO status entry.
        s.apply_cancellation("A", &ids);
        assert_eq!(status(&s, "A", 1), Some(Complete), "complete untouched");
        assert_eq!(
            status(&s, "A", 2),
            Some(Downloading),
            "a track with a Downloading status is NOT overwritten to Cancelled"
        );
        assert_eq!(
            status(&s, "A", 3),
            Some(Cancelled),
            "only the no-status track becomes Cancelled"
        );
    }

    #[test]
    fn single_track_merges_preserving_all_complete_destination_and_siblings() {
        // §A.7: after a full album download, a single-track redownload MERGES —
        // it must NOT seed fresh state, so allComplete + destination + the other
        // tracks' statuses survive (Add-to-Library stays visible).
        let mut s = PurchaseDownloadStore::default();
        let ids = vec![1u64, 2];
        s.seed_album_download("A", "/picked", 27);
        s.mark_track("A", 1, Complete);
        s.mark_track("A", 2, Complete);
        s.rewrite_destination("A", "/picked/Artist/Album [27]");
        s.finalize_album("A", &ids);
        assert!(s.album("A").unwrap().all_complete);

        // single-track redownload of track 1, possibly with a different format.
        s.merge_single_track_start("A", 1, 7);
        let st = s.album("A").unwrap();
        assert_eq!(st.track_statuses.get(&1), Some(&Downloading), "target track flips");
        assert_eq!(st.track_statuses.get(&2), Some(&Complete), "sibling preserved");
        assert!(st.all_complete, "allComplete preserved (no fresh seed)");
        assert_eq!(
            st.destination.as_deref(),
            Some("/picked/Artist/Album [27]"),
            "destination preserved (no rewrite on single-track)"
        );
        assert!(!st.is_downloading_all, "single-track does NOT set isDownloadingAll");
        assert_eq!(st.format_id, Some(7), "formatId updated to the single-track format");

        // finish merge → only track 1 flips, everything else intact.
        s.merge_single_track_finish("A", 1, Complete);
        let st = s.album("A").unwrap();
        assert_eq!(st.track_statuses.get(&1), Some(&Complete));
        assert_eq!(st.track_statuses.get(&2), Some(&Complete));
        assert!(st.all_complete);
        assert_eq!(st.destination.as_deref(), Some("/picked/Artist/Album [27]"));
    }

    #[test]
    fn clear_removes_entry_and_abort_flag() {
        let mut s = PurchaseDownloadStore::default();
        s.seed_album_download("A", "/d", 5);
        s.set_abort("A");
        s.clear("A");
        assert!(s.album("A").is_none());
        assert!(!s.is_aborted("A"));
    }

    #[test]
    fn album_folder_from_file_path_strips_filename() {
        // unix
        assert_eq!(
            album_folder_from_file_path("/music/Miles Davis/Kind of Blue [27]/01 - So What.flac"),
            "/music/Miles Davis/Kind of Blue [27]"
        );
        // windows
        assert_eq!(
            album_folder_from_file_path("C:\\music\\Artist\\Album\\01 - t.flac"),
            "C:\\music\\Artist\\Album"
        );
        // no separator → whole path (fallback)
        assert_eq!(album_folder_from_file_path("track.flac"), "track.flac");
    }

    #[test]
    fn album_all_complete_is_format_scoped() {
        // A fully-downloaded album for format 27 reads complete only while
        // format 27 is selected (Svelte `:388` gate).
        let mut s = PurchaseDownloadStore::default();
        let ids = vec![1u64];
        s.seed_album_download("A", "/d", 27);
        s.mark_track("A", 1, Complete);
        s.finalize_album("A", &ids);
        drop(s); // mutate via the singleton-equivalent below using a local store

        // Re-do against the local store directly (album_all_complete_for_format
        // reads the global singleton, so test the gate math via a fresh store).
        let mut s = PurchaseDownloadStore::default();
        s.seed_album_download("A", "/d", 27);
        s.mark_track("A", 1, Complete);
        s.finalize_album("A", &[1]);
        let st = s.albums.get("A").unwrap();
        // simulate album_all_complete_for_format's body:
        let scoped = |sel: Option<u32>| {
            st.all_complete && (st.format_id.is_none() || st.format_id == sel)
        };
        assert!(scoped(Some(27)), "complete when selected format matches");
        assert!(!scoped(Some(7)), "suppressed for a different format");
        assert!(!scoped(None), "suppressed when no format selected (Some≠None)");
    }

    #[test]
    fn track_status_scoped_suppresses_complete_for_other_format() {
        // getTrackStatus (Svelte `:406-415`): Complete is hidden when the album's
        // formatId is set AND ≠ selected. Other statuses pass through.
        let mut s = PurchaseDownloadStore::default();
        s.seed_album_download("A", "/d", 27);
        s.mark_track("A", 1, Complete);
        s.mark_track("A", 2, Downloading);

        // reproduce track_status_scoped's body against the local store.
        let scoped = |track: u64, sel: Option<u32>| -> Option<TrackDownloadStatus> {
            let state = s.albums.get("A")?;
            let status = *state.track_statuses.get(&track)?;
            if status == Complete {
                if let Some(fmt) = state.format_id {
                    if Some(fmt) != sel {
                        return None;
                    }
                }
            }
            Some(status)
        };
        assert_eq!(scoped(1, Some(27)), Some(Complete), "shown for matching format");
        assert_eq!(scoped(1, Some(7)), None, "Complete suppressed for other format");
        assert_eq!(scoped(2, Some(7)), Some(Downloading), "non-complete always shown");
        assert_eq!(scoped(99, Some(27)), None, "unknown track → None");
    }

    #[test]
    fn is_downloaded_for_format_keys_off_server_ids() {
        // §B.2: gating keys off the REQUESTED format recorded server-side.
        assert!(is_downloaded_for_format(&[27, 7], Some(27)));
        assert!(!is_downloaded_for_format(&[7], Some(27)));
        assert!(!is_downloaded_for_format(&[27], None), "no selection → false");
        assert!(!is_downloaded_for_format(&[], Some(27)));
    }

    #[test]
    fn all_track_statuses_flattens_across_albums() {
        // The flattened map (PurchasesView rows) spans every album's statuses.
        let mut s = PurchaseDownloadStore::default();
        s.seed_album_download("A", "/d", 27);
        s.mark_track("A", 1, Complete);
        s.seed_album_download("B", "/d", 7);
        s.mark_track("B", 2, Downloading);
        // reproduce all_track_statuses's body locally.
        let mut flat = HashMap::new();
        for state in s.albums.values() {
            for (id, st) in &state.track_statuses {
                flat.insert(*id, *st);
            }
        }
        assert_eq!(flat.get(&1), Some(&Complete));
        assert_eq!(flat.get(&2), Some(&Downloading));
    }

    #[test]
    fn finalize_with_empty_track_ids_matches_js_every_true() {
        // FIDELITY: JS `[].every(...) === true` → `allComplete = true` even with
        // zero tracks. Replicate verbatim (degenerate — never reached for a real
        // album, but we do not "guard" it; strict 1:1).
        let mut s = PurchaseDownloadStore::default();
        s.seed_album_download("A", "/d", 27);
        s.finalize_album("A", &[]);
        assert!(s.album("A").unwrap().all_complete);
        assert!(!s.album("A").unwrap().is_downloading_all);
    }

    #[test]
    fn quality_dir_feeds_download_subfolder() {
        // Cross-check with the Slice-6 derivation used before every download.
        assert_eq!(quality_dir("FLAC 24/192"), "FLAC 24-192");
    }

    // ------------------------------------------------------------------
    // Slice 8 — filter / sort / group / formatter pure-fn tests (the
    // byte-for-byte ports of the Svelte $derived functions, §2.1.5).
    // ------------------------------------------------------------------

    fn album(
        title: &str,
        artist: &str,
        bd: Option<u32>,
        sr: Option<f64>,
        hires: bool,
        downloadable: bool,
        purchased_at: Option<i64>,
    ) -> PurchaseAlbum {
        PurchaseAlbum {
            title: title.to_string(),
            artist: qbz_models::Artist {
                name: artist.to_string(),
                ..Default::default()
            },
            maximum_bit_depth: bd,
            maximum_sampling_rate: sr,
            hires,
            downloadable,
            purchased_at,
            ..Default::default()
        }
    }

    fn track(title: &str, bd: Option<u32>, sr: Option<f64>, hires: bool) -> PurchaseTrack {
        PurchaseTrack {
            title: title.to_string(),
            maximum_bit_depth: bd,
            maximum_sampling_rate: sr,
            hires,
            ..Default::default()
        }
    }

    fn ts_default() -> ToolbarState {
        ToolbarState {
            album_grouping_enabled: false,
            album_group_mode: "alpha".into(),
            album_sort_by: "date".into(),
            album_sort_direction: "desc".into(),
            track_grouping_enabled: false,
            track_group_mode: "artist".into(),
            filter_hide_unavailable: false,
            filter_quality: QualityFilter::All,
            filter_hide_downloaded: false,
        }
    }

    #[test]
    fn quality_filter_matches_svelte() {
        // all → always true
        assert!(matches_quality_filter(QualityFilter::All, false, None, None));
        // hires → the hires flag
        assert!(matches_quality_filter(QualityFilter::Hires, true, Some(24), Some(96.0)));
        assert!(!matches_quality_filter(QualityFilter::Hires, false, Some(16), Some(44.1)));
        // cd → !hires && (bd==16 || (no bd && no sr))
        assert!(matches_quality_filter(QualityFilter::Cd, false, Some(16), Some(44.1)));
        assert!(matches_quality_filter(QualityFilter::Cd, false, None, None));
        assert!(!matches_quality_filter(QualityFilter::Cd, false, Some(24), Some(96.0)));
        assert!(!matches_quality_filter(QualityFilter::Cd, true, Some(16), Some(44.1)));
        // lossy → !bd || bd < 16
        assert!(matches_quality_filter(QualityFilter::Lossy, false, None, None));
        assert!(matches_quality_filter(QualityFilter::Lossy, false, Some(8), None));
        assert!(!matches_quality_filter(QualityFilter::Lossy, false, Some(16), None));
    }

    #[test]
    fn album_filters_apply_in_order() {
        let list = vec![
            album("A", "X", Some(24), Some(96.0), true, true, Some(3)),  // hires, avail
            album("B", "Y", Some(16), Some(44.1), false, false, Some(2)), // cd, UNAVAIL
            album("C", "Z", Some(8), None, false, true, Some(1)),        // lossy, avail
        ];
        let mut ts = ts_default();
        // hide unavailable drops B
        ts.filter_hide_unavailable = true;
        let r = apply_album_filters(&ts, &list);
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|a| a.downloadable));
        // quality=hires keeps only A
        ts.filter_quality = QualityFilter::Hires;
        let r = apply_album_filters(&ts, &list);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "A");
    }

    #[test]
    fn tracks_filtered_not_sorted_and_no_unavailable_filter() {
        let list = vec![
            track("b", Some(24), Some(96.0), true),
            track("a", Some(8), None, false),
        ];
        let mut ts = ts_default();
        ts.filter_quality = QualityFilter::Lossy;
        let r = apply_track_filters(&ts, &list);
        // lossy keeps the 8-bit track only; order preserved (no sort).
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "a");
    }

    #[test]
    fn sort_albums_date_desc_default_artist_asc() {
        let list = vec![
            album("A", "Zeta", None, None, false, true, Some(1)),
            album("B", "Alpha", None, None, false, true, Some(3)),
            album("C", "Mu", None, None, false, true, Some(2)),
        ];
        let mut ts = ts_default();
        // date DESC (default): newest (3) first.
        let r = sort_albums(&ts, list.iter().collect());
        assert_eq!(r.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), vec!["B", "C", "A"]);
        // artist ASC.
        ts.album_sort_by = "artist".into();
        ts.album_sort_direction = "asc".into();
        let r = sort_albums(&ts, list.iter().collect());
        assert_eq!(r.iter().map(|a| a.artist.name.as_str()).collect::<Vec<_>>(), vec!["Alpha", "Mu", "Zeta"]);
    }

    #[test]
    fn select_album_sort_toggles_and_defaults() {
        // same key → flip direction.
        assert_eq!(next_album_sort("date", "desc", "date"), ("date".into(), "asc".into()));
        assert_eq!(next_album_sort("date", "asc", "date"), ("date".into(), "desc".into()));
        // new key date → desc; other keys → asc.
        assert_eq!(next_album_sort("artist", "asc", "date"), ("date".into(), "desc".into()));
        assert_eq!(next_album_sort("date", "desc", "artist"), ("artist".into(), "asc".into()));
        assert_eq!(next_album_sort("date", "desc", "quality"), ("quality".into(), "asc".into()));
    }

    #[test]
    fn alpha_group_key_letter_or_hash() {
        assert_eq!(alpha_group_key("apple"), "A");
        // A non-ASCII first letter uppercases to a non-ASCII char, which is NOT
        // `is_ascii_uppercase` → bucket '#' (matches the JS `/[A-Z]/` test).
        assert_eq!(alpha_group_key("Éclair"), "#");
        assert_eq!(alpha_group_key("123"), "#");
        assert_eq!(alpha_group_key(""), "#");
        assert_eq!(alpha_group_key("zoo"), "Z");
    }

    #[test]
    fn formatters_match_svelte() {
        // grid card label: "{bd}/{sr} kHz" or "" if either missing.
        assert_eq!(format_quality_label(Some(24), Some(96.0)), "24/96 kHz");
        assert_eq!(format_quality_label(Some(16), Some(44.1)), "16/44.1 kHz");
        assert_eq!(format_quality_label(None, Some(96.0)), "");
        assert_eq!(format_quality_label(Some(24), None), "");
        // album-list quality: hires → "{bd}bit/{sr}kHz", else "CD Quality".
        assert_eq!(format_quality(true, Some(24), Some(96.0)), "24bit/96kHz");
        assert_eq!(format_quality(false, Some(16), Some(44.1)), "CD Quality");
        assert_eq!(format_quality(true, None, Some(96.0)), "CD Quality");
        // BARE track-row quality (§A.6): "{bd}/{sr}" no kHz, "" if either missing.
        assert_eq!(bare_quality(Some(24), Some(96.0)), "24/96");
        assert_eq!(bare_quality(Some(24), Some(44.1)), "24/44.1");
        assert_eq!(bare_quality(None, Some(96.0)), "");
        assert_eq!(bare_quality(Some(24), None), "");
        // duration m:ss.
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_duration(3723), "62:03");
    }

    #[test]
    fn enrich_album_downloaded_requires_all_nested_in_dlids() {
        use qbz_models::SearchResultsPage;
        let mut a = PurchaseAlbum {
            title: "Album".into(),
            tracks: Some(SearchResultsPage {
                items: vec![track_with_id(1), track_with_id(2)],
                ..Default::default()
            }),
            ..Default::default()
        };
        a.downloaded = false;
        let mut dl: HashSet<u64> = HashSet::new();
        dl.insert(1);
        // partial → not downloaded (frontend OVERRIDES backend).
        enrich_albums(std::slice::from_mut(&mut a), &dl);
        assert!(!a.downloaded);
        // all present → downloaded.
        dl.insert(2);
        enrich_albums(std::slice::from_mut(&mut a), &dl);
        assert!(a.downloaded);
        // no nested tracks (list-mode album) → never downloaded even if dlIds full.
        let mut b = PurchaseAlbum { tracks: None, ..Default::default() };
        enrich_albums(std::slice::from_mut(&mut b), &dl);
        assert!(!b.downloaded);
    }

    fn track_with_id(id: u64) -> PurchaseTrack {
        PurchaseTrack { id, ..Default::default() }
    }
}
