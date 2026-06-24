//! Purchases controller (Slint) — Slice 3: data-loading shells.
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
//! Download-flag enrichment (`enrichWithDownloadStatus`) is Slice 4; the
//! download state machine + actions are Slice 7; the UI apply is Slices 8/9.
//! These load fns therefore return PLAIN, `Send` payload structs (no
//! `slint::Image` / `ModelRc`) so they may be built off the event loop and held
//! across `.await`.

use std::collections::HashSet;
use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_models::{PurchaseAlbum, PurchaseTrack};
use qbz_offline_cache::purchases_service;

use crate::adapter::SlintAdapter;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_as_str_maps_wire_strings() {
        assert_eq!(PurchaseTab::Albums.as_str(), "albums");
        assert_eq!(PurchaseTab::Tracks.as_str(), "tracks");
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
}
