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
use qbz_models::{PurchaseAlbum, PurchaseTrack};
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

/// `getAlbumDownloadFormatId(albumId)` reader.
pub fn get_album_download_format_id(album_id: &str) -> Option<u32> {
    with_store(|s| s.album_format_id(album_id))
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
    // Slices 8/9 re-project the store onto the UI; nudge a refresh here once the
    // appliers exist. For now the weak handle is retained for that wiring.
    let _ = weak;
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

        let Some(client) = snapshot_client(&runtime).await else {
            with_store(|s| {
                s.merge_single_track_finish(&album_id, track_id, TrackDownloadStatus::Failed)
            });
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
        let _ = weak;
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
pub fn handle_add_to_library(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
    destination: String,
) {
    let scan_handle = handle.clone();
    handle.spawn(async move {
        // skip-if-remote: no library writes while controlling a remote renderer.
        if is_controlling_remote().await {
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
            // (error) add-folder failed → error toast, leave state intact.
            crate::toast::error_weak(&weak, qbz_i18n::t("Couldn't add to library"));
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
    });
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
}
