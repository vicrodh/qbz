//! My QBZ — Collection / Mixtape DETAIL **playback** (Phase-2 Slice 5).
//!
//! Wires the detail view's hero Play / Shuffle CTAs, the per-row Play action,
//! and the per-row context menu (play / play-next / add-to-queue) to the
//! shared `qbz-mixtape` ENQUEUE resolver, then drives the already-shared
//! `qbz-core` queue + `qbz-app` `RuntimeManager` queue-source stamp.
//!
//! Behavior is 1:1 with Tauri's `v2_enqueue_collection` /
//! `v2_enqueue_collection_item` (spec 40 §5/§6, gotchas §9):
//! - **Resolve all** uses the collection's persisted `play_mode`; the hero
//!   Shuffle forces `AlbumShuffle` ordering (time-seeded, whole-item shuffle).
//! - Failed items are logged + skipped (partial playback > total failure) —
//!   that is `resolve_collection_tracks`' own contract; the per-item path
//!   mirrors it manually.
//! - `play_next` inserts in **REVERSE** so the first resolved track lands
//!   immediately after the current track.
//! - The queue-source-collection stamp is set **only on replace** (hero
//!   play/shuffle + per-row replace-play); append/play_next preserve context.
//! - `touch_play` is best-effort and runs **only** on the whole-collection
//!   replace paths (hero play/shuffle), never per-row.
//!
//! Frontend-agnostic (ADR-005/006): the `qbz-mixtape` crate holds all the
//! resolution logic; this module only builds a `ProdItemResolver` (Qobuz client
//! + a `Send + Sync` local closure that runs `with_db` synchronously — no
//! `&LibraryDatabase` is ever held across an `.await`) and applies the result
//! to the queue.

use std::sync::Arc;

use qbz_models::mixtape::{CollectionPlayMode, MixtapeCollection, MixtapeCollectionItem};
use qbz_models::QueueTrack;
use qbz_mixtape::enqueue::{resolve_collection_tracks, ProdItemResolver};

use crate::adapter::SlintAdapter;
use crate::playback::{after_track_change, refresh_sidebar};
use crate::AppWindow;
use qbz_app::shell::AppRuntime;

/// Convenience alias for the runtime handle threaded through every call
/// (mirrors `playback::Runtime`).
type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// The per-row context-menu mode parsed from the Slint `action` string.
enum RowMode {
    /// Replace-play this single item (queue + start at 0). No queue-source
    /// stamp, no `touch_play` (per-row action, not "play the whole collection").
    Play,
    /// Insert the item's resolved tracks immediately after the current track.
    PlayNext,
    /// Append the item's resolved tracks at the end of the queue.
    AddToQueue,
}

impl RowMode {
    fn parse(action: &str) -> Option<Self> {
        match action {
            "play" => Some(Self::Play),
            "play-next" | "play_next" => Some(Self::PlayNext),
            "add-to-queue" | "add_to_queue" | "append" => Some(Self::AddToQueue),
            _ => None,
        }
    }
}

/// The synchronous local-item resolver closure handed to `ProdItemResolver`.
///
/// `with_db` is synchronous (it opens the per-user `library.db` fresh on the
/// current blocking thread), so `&LibraryDatabase` never crosses an `.await`.
/// Error semantics are preserved: the crate's `resolve_local_item` error (the
/// load-bearing user-meaningful messages — e.g. the plex "cache empty" hint,
/// the local-playlist hard error) is surfaced verbatim so
/// `resolve_collection_tracks` logs + skips the item exactly as it would for a
/// Qobuz failure. A DB-open failure becomes its own `Err` string (the item is
/// then skipped too, not silently dropped as success).
fn resolve_local(item: &MixtapeCollectionItem) -> Result<Vec<QueueTrack>, String> {
    // with_db -> Option<Result<.., String>>: Some(inner) when the DB opened
    // (inner carries the resolver's own Ok/Err); None when the DB could not be
    // opened at all. We map None to an Err so the item is skipped, not treated
    // as an empty success.
    crate::library_db::with_db(|db| Ok(qbz_mixtape::enqueue::resolve_local_item(db, item)))
        .unwrap_or_else(|| Err("library database unavailable".to_string()))
}

/// Resolve a whole collection's items into a flat queue.
///
/// Builds a `ProdItemResolver` over the shared Qobuz client (a clone taken
/// under the client `RwLock` so the value lives for the whole resolve — its
/// `&` reference must outlive the `.await`s the Qobuz arms perform) + the
/// `Send + Sync` `resolve_local` closure, then runs
/// `resolve_collection_tracks`. `force_shuffle` overrides the persisted mode
/// with `AlbumShuffle` (time-seeded whole-item shuffle) for the hero Shuffle
/// CTA; otherwise the collection's persisted `play_mode` is used.
pub(crate) async fn resolve_collection(
    runtime: &Runtime,
    collection: &MixtapeCollection,
    force_shuffle: bool,
) -> Vec<QueueTrack> {
    let play_mode = if force_shuffle {
        CollectionPlayMode::AlbumShuffle
    } else {
        collection.play_mode
    };

    // Snapshot the Qobuz client (mirrors v2_enqueue_collection step 3 /
    // playback.rs's prefetch path). The clone lives in `client`, so the `&`
    // handed to ProdItemResolver outlives every Qobuz `.await` in resolve.
    let client_lock = runtime.core().client();
    let client = {
        let guard = client_lock.read().await;
        match guard.as_ref() {
            Some(c) => c.clone(),
            None => {
                log::warn!("[qbz-slint] myqbz_play: no Qobuz client; resolving local items only");
                // Still build a resolver — local/Plex items resolve without the
                // client; Qobuz items will error+skip inside the resolver.
                // Cloning a missing client is impossible, so bail early with the
                // local-only subset is not feasible (the resolver needs a client
                // ref). Return empty: the caller toasts "0 playable tracks".
                return Vec::new();
            }
        }
    };

    let resolver = ProdItemResolver::new(&client, resolve_local);
    resolve_collection_tracks(collection.items.clone(), play_mode, &resolver).await
}

/// Resolve a SINGLE item (per-row actions). Mirrors `v2_enqueue_collection_item`
/// (spec 40 §6): resolve the one item directly, then **stamp
/// `source_item_id_hint = item.source_item_id` INLINE** (this path bypasses
/// `resolve_collection_tracks`, so the central stamp does not run). Failed
/// resolution logs + returns empty (the caller toasts "0 playable tracks").
async fn resolve_single_item(
    runtime: &Runtime,
    item: &MixtapeCollectionItem,
) -> Vec<QueueTrack> {
    use qbz_mixtape::enqueue::ItemResolver;

    let client_lock = runtime.core().client();
    let client = {
        let guard = client_lock.read().await;
        match guard.as_ref() {
            Some(c) => c.clone(),
            None => {
                log::warn!("[qbz-slint] myqbz_play: no Qobuz client; cannot resolve item");
                return Vec::new();
            }
        }
    };

    let resolver = ProdItemResolver::new(&client, resolve_local);
    match resolver.resolve(item).await {
        Ok(mut tracks) => {
            // Inline boundary stamp (resolve_collection_tracks isn't used here).
            let hint = item.source_item_id.clone();
            for track in &mut tracks {
                track.source_item_id_hint = Some(hint.clone());
            }
            tracks
        }
        Err(e) => {
            log::warn!(
                "[qbz-slint] myqbz_play: item {}/{} resolve failed: {}",
                item.source_item_id,
                item.title,
                e
            );
            Vec::new()
        }
    }
}

/// Resolve a single item's tracks for the **expanded view-mode inline track
/// expansion** (spec 12 §8). Same resolver path as `resolve_single_item`
/// (Qobuz album/track/playlist + local/Plex via `resolve_local`), but used for
/// DISPLAY only — no queue mutation, no `source_item_id_hint` stamping. The
/// per-(item_type, source) routing the spec's `fetchTracksForItem` matrix
/// describes already lives inside the shared `ProdItemResolver::resolve` /
/// `resolve_local_item` (qobuz album->tracks, local album->tracks, plex
/// cache->tracks; a local/Plex playlist returns its own resolver error → []),
/// so this stays a thin wrapper. Returns the resolved tracks (empty on any
/// resolver error, so the caller shows the per-item "no results" state).
pub(crate) async fn fetch_item_tracks(
    runtime: &Runtime,
    item: &MixtapeCollectionItem,
) -> Vec<QueueTrack> {
    use qbz_mixtape::enqueue::ItemResolver;

    let client_lock = runtime.core().client();
    let client = {
        let guard = client_lock.read().await;
        match guard.as_ref() {
            Some(c) => c.clone(),
            None => {
                log::warn!("[qbz-slint] myqbz_play: no Qobuz client; cannot fetch item tracks");
                // Local/Plex items resolve without the client, but the resolver
                // needs a client ref to build, so bail empty (the caller shows
                // the per-item empty state).
                return Vec::new();
            }
        }
    };

    let resolver = ProdItemResolver::new(&client, resolve_local);
    match resolver.resolve(item).await {
        Ok(tracks) => tracks,
        Err(e) => {
            log::warn!(
                "[qbz-slint] myqbz_play: fetch_item_tracks {}/{} failed: {}",
                item.source_item_id,
                item.title,
                e
            );
            Vec::new()
        }
    }
}

/// Best-effort `repo::touch_play` (bumps last_played_at + play_count). Errors
/// ignored, exactly like the Tauri command. Runs synchronously via `with_db` —
/// safe to call from the async context (no `&Connection` crosses an `.await`).
fn touch_play(collection_id: &str) {
    let _ = crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| {
            if let Err(e) = qbz_mixtape::repo::touch_play(conn, collection_id) {
                log::debug!("[qbz-slint] myqbz_play: touch_play({collection_id}) failed: {e}");
            }
        }))
    });
}

/// Replace the queue with `tracks`, start at index 0, stamp the queue-source
/// collection, and `touch_play`. Shared by hero Play + hero Shuffle (the two
/// whole-collection replace paths). Empty `tracks` → toast + no-op.
pub(crate) async fn play_all_tracks(
    runtime: &Runtime,
    weak: &slint::Weak<AppWindow>,
    collection_id: &str,
    tracks: Vec<QueueTrack>,
) {
    if tracks.is_empty() {
        crate::toast::error_weak(weak, "This collection resolved to 0 playable tracks");
        return;
    }
    let first_id = tracks[0].id;
    runtime.core().set_queue(tracks, Some(0)).await;
    // Queue-source stamp: ONLY on replace (spec §9.9) — this IS a replace.
    runtime
        .runtime()
        .set_queue_source_collection(Some(collection_id.to_string()))
        .await;
    after_track_change(runtime, weak, first_id).await;
    // touch_play is best-effort, replace-only.
    touch_play(collection_id);
    refresh_sidebar(true);
}

// ──────────────────────────── public entry points ─────────────────────

/// Hero **Play** (`on_play_all`): resolve the whole collection with its
/// persisted `play_mode`, then replace-play.
pub fn play_all(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    collection_id: String,
) {
    handle.spawn(async move {
        let Some(collection) = load_collection(&collection_id).await else {
            crate::toast::error_weak(&weak, "Couldn't load this collection");
            return;
        };
        let tracks = resolve_collection(&runtime, &collection, false).await;
        play_all_tracks(&runtime, &weak, &collection_id, tracks).await;
    });
}

/// Hero **Shuffle** (`on_shuffle`): resolve with forced `AlbumShuffle`
/// ordering, then replace-play (same queue-source stamp + touch_play as Play —
/// it is a replace). As a SIDE EFFECT (1:1 with Tauri `handleShuffle`, spec 12
/// §20), Shuffle also persists `play_mode='album_shuffle'` so the collection
/// remembers it; if THIS is the open collection, the detail is reloaded so the
/// overflow play-mode-toggle label flips to the other mode ("Play in order").
/// Persist runs ONLY on Shuffle — a normal hero Play never changes play_mode.
pub fn shuffle(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: crate::artwork::ImageCache,
    collection_id: String,
) {
    handle.clone().spawn(async move {
        let Some(collection) = load_collection(&collection_id).await else {
            crate::toast::error_weak(&weak, "Couldn't load this collection");
            return;
        };
        let tracks = resolve_collection(&runtime, &collection, true).await;
        play_all_tracks(&runtime, &weak, &collection_id, tracks).await;

        // Persist play_mode='album_shuffle' as a side effect (Tauri's
        // `setPlayMode` in handleShuffle). Best-effort: only persist when the
        // collection wasn't already album_shuffle, and reload the open detail so
        // the overflow toggle label reflects the new mode.
        if collection.play_mode != CollectionPlayMode::AlbumShuffle {
            persist_album_shuffle(&weak, &handle, &image_cache, &collection_id);
        }
    });
}

/// Best-effort `repo::set_play_mode(id, AlbumShuffle)` (spec 12 §20 shuffle
/// side effect), then reload the detail IF `id` is the currently-open
/// collection (so the hero overflow play-mode-toggle label flips). DB work runs
/// on a blocking worker (no `&Connection` crosses an `.await`); the reload hops
/// back to the event loop. Failures are logged, not surfaced — the shuffle
/// already played, so a failed persist must not toast an error.
fn persist_album_shuffle(
    weak: &slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: &crate::artwork::ImageCache,
    collection_id: &str,
) {
    let write_id = collection_id.to_string();
    let persisted = crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| {
            qbz_mixtape::repo::set_play_mode(conn, &write_id, CollectionPlayMode::AlbumShuffle)
        }))
    });
    match persisted {
        Some(Ok(())) => {}
        Some(Err(e)) => {
            log::warn!("[qbz-slint] myqbz_play: persist album_shuffle({collection_id}) failed: {e}");
            return;
        }
        None => {
            log::warn!("[qbz-slint] myqbz_play: persist album_shuffle: library db unavailable");
            return;
        }
    }

    // Reload the detail only when this collection is the one on screen, so the
    // overflow toggle label flips to the OTHER mode. Off-screen collections need
    // no reload — their next open restores from the DB.
    let handle = handle.clone();
    let image_cache = image_cache.clone();
    let id = collection_id.to_string();
    let _ = weak.upgrade_in_event_loop(move |w| {
        use slint::ComponentHandle;
        if w.global::<crate::MyQbzDetailState>().get_id().as_str() == id {
            crate::myqbz_detail::navigate(w.as_weak(), handle, image_cache, id);
        }
    });
}

/// Per-row default **Play** (`on_play_item`) and the context-menu **Play**
/// action: resolve the SINGLE item by `source_item_id`, then replace-play just
/// that item. No queue-source stamp, no touch_play (per-row, not whole
/// collection).
pub fn play_item(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    collection_id: String,
    source_item_id: String,
) {
    item_action(runtime, weak, handle, collection_id, source_item_id, "play".to_string());
}

/// Per-row context-menu action (`on_item_action`): play / play-next /
/// add-to-queue for the single item identified by `source_item_id`.
pub fn item_action(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    collection_id: String,
    source_item_id: String,
    action: String,
) {
    let Some(mode) = RowMode::parse(&action) else {
        log::warn!("[qbz-slint] myqbz_play: unknown item action {action}");
        return;
    };
    handle.spawn(async move {
        let Some(collection) = load_collection(&collection_id).await else {
            crate::toast::error_weak(&weak, "Couldn't load this collection");
            return;
        };
        let Some(item) = collection
            .items
            .iter()
            .find(|it| it.source_item_id == source_item_id)
            .cloned()
        else {
            log::warn!(
                "[qbz-slint] myqbz_play: item {source_item_id} not found in collection {collection_id}"
            );
            return;
        };

        let tracks = resolve_single_item(&runtime, &item).await;
        if tracks.is_empty() {
            crate::toast::error_weak(&weak, "This item resolved to 0 playable tracks");
            return;
        }

        match mode {
            RowMode::Play => {
                // Replace-play this single item — NO queue-source stamp, NO
                // touch_play (per-row).
                let first_id = tracks[0].id;
                runtime.core().set_queue(tracks, Some(0)).await;
                after_track_change(&runtime, &weak, first_id).await;
                refresh_sidebar(true);
            }
            RowMode::PlayNext => {
                // Insert in REVERSE so the first resolved track lands
                // immediately after the current track (spec §9.8).
                for track in tracks.into_iter().rev() {
                    runtime.core().add_track_next(track).await;
                }
                refresh_sidebar(false);
                crate::toast::success_weak(&weak, "Playing next");
            }
            RowMode::AddToQueue => {
                runtime.core().add_tracks(tracks).await;
                refresh_sidebar(false);
                crate::toast::success_weak(&weak, "Added to queue");
            }
        }
    });
}

/// **Bulk** enqueue for the detail select-mode bulk bar (spec 12 §13.1 Add to
/// queue / Play next). Resolves EACH selected `MixtapeCollectionItem` through
/// the same `ProdItemResolver` the per-row path uses (so Qobuz albums/tracks/
/// playlists + local/Plex all resolve), flattens them in selection order, then:
/// - **play_next = true**: insert the whole batch immediately after the current
///   track, in REVERSE so the first resolved track lands first (same rule as the
///   per-row `PlayNext`).
/// - **play_next = false**: append the batch at the end of the queue.
///
/// Never replaces the queue and never stamps the queue-source collection
/// (append/play-next preserve context, mirroring the per-row contract). Items
/// that resolve to nothing are logged + skipped; an all-empty batch toasts.
pub fn bulk_enqueue(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    items: Vec<MixtapeCollectionItem>,
    play_next: bool,
) {
    if items.is_empty() {
        return;
    }
    handle.spawn(async move {
        let client_lock = runtime.core().client();
        let client = {
            let guard = client_lock.read().await;
            match guard.as_ref() {
                Some(c) => c.clone(),
                None => {
                    log::warn!("[qbz-slint] myqbz_play: no Qobuz client; cannot bulk-enqueue");
                    crate::toast::error_weak(&weak, "These items resolved to 0 playable tracks");
                    return;
                }
            }
        };
        let resolver = ProdItemResolver::new(&client, resolve_local);

        // Resolve each item in selection order, stamping the per-item boundary
        // hint inline (this path bypasses resolve_collection_tracks).
        use qbz_mixtape::enqueue::ItemResolver;
        let mut tracks: Vec<QueueTrack> = Vec::new();
        for item in &items {
            match resolver.resolve(item).await {
                Ok(mut resolved) => {
                    let hint = item.source_item_id.clone();
                    for t in &mut resolved {
                        t.source_item_id_hint = Some(hint.clone());
                    }
                    tracks.extend(resolved);
                }
                Err(e) => {
                    log::warn!(
                        "[qbz-slint] myqbz_play: bulk item {}/{} resolve failed: {}",
                        item.source_item_id,
                        item.title,
                        e
                    );
                }
            }
        }

        if tracks.is_empty() {
            crate::toast::error_weak(&weak, "These items resolved to 0 playable tracks");
            return;
        }

        if play_next {
            // REVERSE so the first resolved track lands immediately after the
            // current track (spec §9.8).
            for track in tracks.into_iter().rev() {
                runtime.core().add_track_next(track).await;
            }
            refresh_sidebar(false);
            crate::toast::success_weak(&weak, "Playing next");
        } else {
            runtime.core().add_tracks(tracks).await;
            refresh_sidebar(false);
            crate::toast::success_weak(&weak, "Added to queue");
        }
    });
}

/// Resolve the selected items' Qobuz track IDs for the bulk "Add to playlist"
/// flow (spec 12 §13.1). Qobuz playlists only accept Qobuz track ids, so each
/// item is resolved and only `source == "qobuz"` tracks contribute their ids
/// (local/Plex tracks are skipped — same constraint the Local Library bulk
/// add-to-playlist applies). Returns the ids in resolution order; an empty
/// result means nothing playable-to-a-Qobuz-playlist was selected.
pub async fn resolve_bulk_qobuz_track_ids(
    runtime: &Runtime,
    items: &[MixtapeCollectionItem],
) -> Vec<String> {
    use qbz_mixtape::enqueue::ItemResolver;

    let client_lock = runtime.core().client();
    let client = {
        let guard = client_lock.read().await;
        match guard.as_ref() {
            Some(c) => c.clone(),
            None => {
                log::warn!("[qbz-slint] myqbz_play: no Qobuz client; cannot resolve bulk ids");
                return Vec::new();
            }
        }
    };
    let resolver = ProdItemResolver::new(&client, resolve_local);

    let mut ids: Vec<String> = Vec::new();
    for item in items {
        match resolver.resolve(item).await {
            Ok(tracks) => {
                for t in tracks {
                    // Qobuz-only: a local/Plex track id is not a Qobuz playlist
                    // member. `source` is the resolver's per-track stamp.
                    let is_qobuz = t.source.as_deref() == Some("qobuz")
                        || (t.source.is_none() && !t.is_local);
                    if is_qobuz {
                        ids.push(t.id.to_string());
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "[qbz-slint] myqbz_play: bulk add-to-playlist resolve {}/{} failed: {}",
                    item.source_item_id,
                    item.title,
                    e
                );
            }
        }
    }
    ids
}

/// The inline-track menu mode (expanded view-mode TrackRow actions, spec §8
/// `menuActions`): play-now / play-next / play-later for ONE track resolved
/// from its parent item. (go-to-album routes through `open-item` in main.rs,
/// not here.)
enum InlineTrackMode {
    Play,
    PlayNext,
    PlayLater,
}

impl InlineTrackMode {
    fn parse(action: &str) -> Option<Self> {
        match action {
            "play" => Some(Self::Play),
            "play-next" | "play_next" => Some(Self::PlayNext),
            "play-later" | "play_later" | "queue" | "add-to-queue" | "append" => {
                Some(Self::PlayLater)
            }
            _ => None,
        }
    }
}

/// Play / queue a SINGLE inline track from an expanded item (spec 12 §8
/// `onPlayTrackFromItem` / `onPlayTrackNext` / `onPlayTrackLater`). Re-resolves
/// the parent item's tracks (the inline view holds only display rows, not the
/// numeric `QueueTrack`s) and selects the one matching `track_id`:
/// - **Play**: replace-play just that track (no queue-source stamp, per-row).
/// - **PlayNext**: insert immediately after the current track.
/// - **PlayLater**: append at the end of the queue.
pub fn play_inline_track(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    collection_id: String,
    item_source_item_id: String,
    track_id: String,
    action: String,
) {
    let Some(mode) = InlineTrackMode::parse(&action) else {
        log::warn!("[qbz-slint] myqbz_play: unknown inline-track action {action}");
        return;
    };
    let Ok(track_id) = track_id.parse::<u64>() else {
        log::warn!("[qbz-slint] myqbz_play: inline-track non-numeric id {track_id}");
        return;
    };
    handle.spawn(async move {
        let Some(collection) = load_collection(&collection_id).await else {
            crate::toast::error_weak(&weak, "Couldn't load this collection");
            return;
        };
        let Some(item) = collection
            .items
            .iter()
            .find(|it| it.source_item_id == item_source_item_id)
            .cloned()
        else {
            log::warn!(
                "[qbz-slint] myqbz_play: inline-track item {item_source_item_id} not found"
            );
            return;
        };

        let tracks = fetch_item_tracks(&runtime, &item).await;
        let Some(track) = tracks.into_iter().find(|t| t.id == track_id) else {
            crate::toast::error_weak(&weak, "This track is no longer available");
            return;
        };

        match mode {
            InlineTrackMode::Play => {
                let first_id = track.id;
                runtime.core().set_queue(vec![track], Some(0)).await;
                after_track_change(&runtime, &weak, first_id).await;
                refresh_sidebar(true);
            }
            InlineTrackMode::PlayNext => {
                runtime.core().add_track_next(track).await;
                refresh_sidebar(false);
                crate::toast::success_weak(&weak, "Playing next");
            }
            InlineTrackMode::PlayLater => {
                runtime.core().add_tracks(vec![track]).await;
                refresh_sidebar(false);
                crate::toast::success_weak(&weak, "Added to queue");
            }
        }
    });
}

/// Load a collection (items hydrated) off the UI/event-loop thread, on a
/// blocking worker, reusing the detail module's read path. Returns `None` when
/// the DB is unavailable or the id is unknown.
pub(crate) async fn load_collection(collection_id: &str) -> Option<MixtapeCollection> {
    let id = collection_id.to_string();
    tokio::task::spawn_blocking(move || crate::myqbz_detail::get_collection(&id))
        .await
        .ok()
        .flatten()
}

// ─────────────────────── Part B: skip-to-ITEM (boundary nav) ───────────────
//
// Spec 40 §5.6 + §6 (`v2_skip_to_next_item` / `v2_skip_to_previous_item`):
// jump the queue cursor to the START of the next / previous ITEM (album /
// playlist / track group) rather than the next / previous TRACK. The boundary
// key per track = `source_item_id_hint` (stamped by the resolver), else the
// `album_id` fallback — both already on `QueueTrack`. The math is the PURE,
// already-shared `qbz_mixtape::enqueue::{next_item_index, previous_item_index}`
// (the 3-second prev rule lives there); these helpers only read the live queue
// from `qbz-core`, call that math, and `play_index` the target.
//
// **These NEVER touch the global `playback::next()` / `previous()`** — the
// normal transport stays track-by-track. They are headless entry points over
// `qbz-core` so a future UI trigger (or QConnect / CLI) can drive them without
// re-implementing the boundary detection.
//
// **UI trigger: DEFERRED.** Tauri registers both commands
// (`src-tauri/src/lib.rs`) but has ZERO frontend callsites — there is no
// skip-album button anywhere in the Tauri UI, so there is no faithful UI home
// to port 1:1. Forcing a button into the shared next/prev transport would risk
// the global transport for a behavior Tauri itself never surfaced. So the
// helpers land headless + tested-by-shared-crate; wiring a UI trigger waits
// until the product asks for one (then it calls these, no transport rewrite).

/// Skip to the START of the NEXT item in the live queue (spec 40 §6
/// `v2_skip_to_next_item`). Reads the current queue + cursor from `qbz-core`,
/// finds the first track whose item-boundary differs from the current item via
/// `next_item_index`, and `play_index`es it. No-op at the last item / empty
/// queue / no current cursor.
pub async fn skip_to_next_item(runtime: &Runtime, weak: &slint::Weak<AppWindow>) {
    let (queue, current) = runtime.core().get_all_queue_tracks().await;
    let Some(current) = current else {
        log::debug!("[qbz-slint] myqbz_play: skip_to_next_item — no current track");
        return;
    };
    match qbz_mixtape::enqueue::next_item_index(&queue, current) {
        Some(target) => {
            if let Some(track) = runtime.core().play_index(target).await {
                let track_id = track.id;
                after_track_change(runtime, weak, track_id).await;
                refresh_sidebar(true);
            }
        }
        None => {
            log::debug!("[qbz-slint] myqbz_play: skip_to_next_item — already at last item");
        }
    }
}

/// Skip to the START of the PREVIOUS item (or restart the current one) in the
/// live queue (spec 40 §6 `v2_skip_to_previous_item`). Reads the current queue
/// + cursor + elapsed position from `qbz-core`, applies the 3-second prev rule
/// via `previous_item_index` (elapsed > 3s OR mid-item → restart current item;
/// else jump to the previous item's start), and `play_index`es the target.
pub async fn skip_to_previous_item(runtime: &Runtime, weak: &slint::Weak<AppWindow>) {
    let (queue, current) = runtime.core().get_all_queue_tracks().await;
    let Some(current) = current else {
        log::debug!("[qbz-slint] myqbz_play: skip_to_previous_item — no current track");
        return;
    };
    // `PlaybackState.position` is in whole seconds (same unit the seek path
    // multiplies against `duration`); the boundary math wants elapsed ms.
    let elapsed_ms = runtime.core().get_playback_state().position * 1_000;
    match qbz_mixtape::enqueue::previous_item_index(&queue, current, elapsed_ms) {
        Some(target) => {
            if let Some(track) = runtime.core().play_index(target).await {
                let track_id = track.id;
                after_track_change(runtime, weak, track_id).await;
                refresh_sidebar(true);
            }
        }
        None => {
            log::debug!("[qbz-slint] myqbz_play: skip_to_previous_item — no previous item");
        }
    }
}
