//! Queue sidebar controller.
//!
//! Ported from the Tauri `QueuePanel.svelte`. The core's `QueueManager`
//! owns the authoritative track list; this controller owns the
//! *sidebar-local* view state — which tab is shown, the search query, the
//! current paginator page. The NOW PLAYING heart reads the shared
//! `fav_cache` set (disk-seeded, network-refreshed) like every other
//! track surface, so it stays correct offline.
//!
//! `refresh` pulls a fresh `get_queue_state_full()` snapshot, applies the
//! active search filter, slices out the current 40-track page, and pushes
//! everything onto the `QueueState` Slint global. Every queue mutation
//! (play / remove / clear / page change / search) calls back into
//! `refresh` so the UI and the core never drift.

use std::sync::{Arc, Mutex};

use qbz_app::settings::playback::AutoplayMode;
use qbz_models::QueueTrack;
use slint::{ComponentHandle, Model};

use crate::adapter::SlintAdapter;
use crate::{AppWindow, ImmersiveState, QueueItem, QueueState};

/// Upcoming tracks shown per paginator page — matches the Tauri sidebar.
pub const PAGE_SIZE: usize = 40;

type Runtime = Arc<qbz_app::shell::AppRuntime<SlintAdapter>>;

/// Sidebar-local view state. Wrapped in a `Mutex` and shared as an `Arc`
/// across every queue callback closure.
#[derive(Default)]
struct ViewState {
    /// Active tab: 0 = Queue, 1 = History.
    tab: i32,
    /// Live search query (filters the upcoming list, case-insensitive).
    search: String,
    /// Zero-based paginator page within the (filtered) upcoming list.
    page: usize,
}

/// The Queue sidebar controller — see the module docs.
pub struct QueueController {
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    playback: qbz_app::settings::playback::PlaybackPreferencesState,
    view: Arc<Mutex<ViewState>>,
    /// Last coverflow flat id-sequence fingerprint pushed to `QueueState`.
    /// `refresh_async` compares the freshly-computed hash to this: equal means a
    /// PURE ADVANCE/JUMP (same id-sequence, only the current pointer moved) — the
    /// flat model setter is SKIPPED and only `coverflow-index` is updated, so the
    /// Repeater never rebuilds and visible covers never re-decode. A different
    /// hash (new queue / shuffle / add / remove) triggers the one-time rebuild.
    /// `None` = nothing pushed yet (first refresh always rebuilds).
    last_coverflow_seq: Arc<Mutex<Option<u64>>>,
}

// `PlaybackPreferencesState` is not `Clone`, but its sole field is an
// `Arc`-shared store handle, so the controller can be cloned cheaply by
// sharing that handle. Every other field is already `Arc`/`Clone`.
impl Clone for QueueController {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            weak: self.weak.clone(),
            handle: self.handle.clone(),
            playback: qbz_app::settings::playback::PlaybackPreferencesState {
                store: Arc::clone(&self.playback.store),
            },
            view: Arc::clone(&self.view),
            last_coverflow_seq: Arc::clone(&self.last_coverflow_seq),
        }
    }
}

/// Plain `Send` row data built off the UI thread; the non-`Send`
/// `QueueItem` (holds a `slint::Image`) is constructed inside the event
/// loop from this.
struct RowData {
    id: String,
    title: String,
    artist: String,
    duration: String,
    explicit: bool,
    artwork_url: String,
    playing: bool,
    is_ephemeral: bool,
}

/// `M:SS` duration string.
fn fmt_duration(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Title with the Qobuz version suffix appended, matching the Tauri
/// `formatTrackTitle` behaviour.
fn display_title(track: &QueueTrack) -> String {
    match track.version.as_deref().filter(|v| !v.is_empty()) {
        Some(version) => format!("{} ({version})", track.title),
        None => track.title.clone(),
    }
}

/// One page's bounds within a `total`-length list, `PAGE_SIZE` per page.
struct PageBounds {
    /// Page index clamped into `[0, page_count)`.
    page: usize,
    /// Total number of pages (at least 1).
    page_count: usize,
    /// Zero-based slice start.
    start: usize,
    /// Zero-based slice end (exclusive).
    end: usize,
}

/// Compute the paginator bounds for `total` items at the requested
/// `page`. The page is clamped so a shrunk list never leaves the
/// paginator past the end. Extracted so the maths is unit-testable.
fn paginate(total: usize, requested_page: usize) -> PageBounds {
    let page_count = total.div_ceil(PAGE_SIZE).max(1);
    let page = requested_page.min(page_count - 1);
    let start = page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(total);
    PageBounds {
        page,
        page_count,
        start,
        end,
    }
}

fn row_from(track: &QueueTrack, playing: bool) -> RowData {
    RowData {
        id: track.id.to_string(),
        title: display_title(track),
        artist: track.artist.clone(),
        duration: fmt_duration(track.duration_secs),
        explicit: track.parental_warning,
        artwork_url: track.artwork_url.clone().unwrap_or_default(),
        playing,
        is_ephemeral: track.source.as_deref() == Some("ephemeral")
            || crate::ephemeral::is_ephemeral_id(track.id as i64),
    }
}

impl QueueController {
    pub fn new(
        runtime: Runtime,
        weak: slint::Weak<AppWindow>,
        handle: tokio::runtime::Handle,
        playback: qbz_app::settings::playback::PlaybackPreferencesState,
    ) -> Self {
        Self {
            runtime,
            weak,
            handle,
            playback,
            view: Arc::new(Mutex::new(ViewState::default())),
            last_coverflow_seq: Arc::new(Mutex::new(None)),
        }
    }

    /// Accessors so background flows (e.g. Plex quality hydration) reachable
    /// only through the global controller can re-push now-playing without
    /// threading the runtime through every detail-view entry point.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
    pub fn weak(&self) -> &slint::Weak<AppWindow> {
        &self.weak
    }
    pub fn handle(&self) -> &tokio::runtime::Handle {
        &self.handle
    }

    /// Pull a fresh full queue snapshot, re-apply the search filter and
    /// current page, and push the result onto `QueueState`. Spawns on the
    /// tokio runtime; safe to call from any thread.
    pub fn refresh(&self) {
        let this = self.clone();
        self.handle.spawn(async move {
            this.refresh_async().await;
        });
    }

    /// Refresh the queue view; when online, first re-pull the SHARED
    /// favorite cache from the network (used after a fresh play starts, so
    /// hearts reflect cross-device changes — same cadence as before, but
    /// now feeding `fav_cache` + its disk mirror instead of a queue-local
    /// set). Offline, the disk-seeded cache is used as-is.
    pub fn refresh_with_favorites(&self) {
        let this = self.clone();
        self.handle.spawn(async move {
            if !crate::offline_mode::engine().is_offline() {
                match this.runtime.core().favorite_track_ids().await {
                    Ok(ids) => {
                        // set_all mirrors to disk (blocking rusqlite).
                        let _ =
                            tokio::task::spawn_blocking(move || crate::fav_cache::set_all(ids))
                                .await;
                    }
                    Err(e) => {
                        log::warn!("[qbz-slint] queue: favorite_track_ids failed: {e}");
                    }
                }
            }
            this.refresh_async().await;
        });
    }

    async fn refresh_async(&self) {
        let perf_start = std::time::Instant::now();
        let state = self.runtime.core().get_queue_state_full().await;

        // --- NOW PLAYING --------------------------------------------------
        // Heart state comes from the shared fav_cache (disk-seeded at
        // session activation), so it is correct offline too.
        let now_playing = state.current_track.as_ref().map(|t| row_from(t, true));
        let now_playing_favorite = state
            .current_track
            .as_ref()
            .map(|t| crate::fav_cache::contains(t.id))
            .unwrap_or(false);

        // --- UP NEXT (search-filtered) -----------------------------------
        let (search, page, tab) = self
            .view
            .lock()
            .map(|v| (v.search.clone(), v.page, v.tab))
            .unwrap_or_default();
        let query = search.trim().to_lowercase();

        let filtered: Vec<&QueueTrack> = if query.is_empty() {
            state.upcoming.iter().collect()
        } else {
            state
                .upcoming
                .iter()
                .filter(|t| {
                    display_title(t).to_lowercase().contains(&query)
                        || t.artist.to_lowercase().contains(&query)
                })
                .collect()
        };

        let upcoming_total = filtered.len();
        let bounds = paginate(upcoming_total, page);
        let (page, page_count, start, end) =
            (bounds.page, bounds.page_count, bounds.start, bounds.end);
        // Persist the clamped page in case the filter shrank the list.
        if let Ok(mut view) = self.view.lock() {
            view.page = page;
        }

        let page_rows: Vec<RowData> = filtered[start..end]
            .iter()
            .map(|t| row_from(t, false))
            .collect();

        // page-start / page-end are 1-based for the human-readable counter.
        let page_start = if upcoming_total == 0 { 0 } else { start + 1 };
        let page_end = end;
        // "left" mirrors the Tauri queueRemainingTracks: tracks after the
        // current one across the whole (unfiltered) queue.
        let remaining = state
            .current_index
            .map(|idx| state.total_tracks.saturating_sub(idx + 1))
            .unwrap_or(state.total_tracks);

        // --- HISTORY ------------------------------------------------------
        let history_rows: Vec<RowData> =
            state.history.iter().map(|t| row_from(t, false)).collect();

        // --- COVERFLOW (ONE stable flat model) ----------------------------
        // Built from the UNFILTERED queue, oldest-first:
        //   [history.reversed (oldest..newest), NOW-PLAYING, upcoming...]
        // `QueueStateFull.history` is most-recent-first, so reverse it for the
        // oldest-first flat order. The flat index of NOW = number of history
        // entries (it sits right after the reversed history). On a PURE ADVANCE
        // the id-sequence is unchanged and only `coverflow_index` moves -> the
        // .slint `scroll` float animates, no model rebuild, no re-decode.
        let mut coverflow_rows: Vec<RowData> = Vec::with_capacity(
            state.history.len() + 1 + state.upcoming.len(),
        );
        for t in state.history.iter().rev() {
            coverflow_rows.push(row_from(t, false));
        }
        let coverflow_index: usize = if let Some(t) = state.current_track.as_ref() {
            let idx = coverflow_rows.len();
            coverflow_rows.push(row_from(t, true));
            idx
        } else {
            // No current track: index points at the first upcoming (or 0 when
            // the whole queue is empty). The flat list is history ++ upcoming.
            coverflow_rows.len()
        };
        for t in state.upcoming.iter() {
            coverflow_rows.push(row_from(t, false));
        }

        // Order-sensitive rolling fingerprint over the flat id-sequence. Used to
        // gate the model rebuild: equal hash => same membership+order => pure
        // advance/jump => skip set_coverflow_tracks. Hashing the ordered ids
        // (not a set) catches shuffle/reorder; folding the index-free id list
        // keeps a same-sequence-different-pointer advance hashing identical.
        let coverflow_seq_hash: u64 = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            coverflow_rows.len().hash(&mut h);
            for r in &coverflow_rows {
                r.id.hash(&mut h);
            }
            h.finish()
        };
        // Decide rebuild-vs-index-only BEFORE the event loop so the art-job set
        // can be narrowed to the ±4 window. `seq_changed` true => rebuild path.
        let seq_changed = {
            let mut last = self.last_coverflow_seq.lock().ok();
            let changed = match last.as_deref() {
                Some(Some(prev)) => *prev != coverflow_seq_hash,
                _ => true, // None lock or first push -> rebuild
            };
            if let Some(slot) = last.as_deref_mut() {
                *slot = Some(coverflow_seq_hash);
            }
            changed
        };

        // --- Infinite-play flag ------------------------------------------
        let infinite = self
            .playback
            .get_preferences()
            .map(|p| p.autoplay_mode == AutoplayMode::InfiniteRadio)
            .unwrap_or(false);

        // Collect artwork jobs before the rows move into the closure.
        // Indices: 0 = now-playing, 1.. = page rows, then history rows.
        let mut art_jobs: Vec<(ArtTarget, String)> = Vec::new();
        if let Some(np) = now_playing.as_ref() {
            if !np.artwork_url.is_empty() {
                art_jobs.push((ArtTarget::NowPlaying, np.artwork_url.clone()));
            }
        }
        for (idx, row) in page_rows.iter().enumerate() {
            if !row.artwork_url.is_empty() {
                art_jobs.push((ArtTarget::Upcoming(idx), row.artwork_url.clone()));
            }
        }
        for (idx, row) in history_rows.iter().enumerate() {
            if !row.artwork_url.is_empty() {
                art_jobs.push((ArtTarget::History(idx), row.artwork_url.clone()));
            }
        }
        // Coverflow art candidates. The COVERFLOW FAN only needs a ±4 window
        // around the current flat index (only ~9 rows can be near the visible ±3
        // fan). BUT the immersive QUEUE PANEL reuses this SAME flat model as a
        // full vertical UP-NEXT list, where EVERY visible row must show art — not
        // just the ±4 nearest (the reported "only now-playing + next 4 have art"
        // bug). So gather ALL rows with a cover here and let the event-loop
        // closure (which can read ImmersiveState) pick the range: the whole list
        // when the immersive queue panel is showing, else ±4. Either way the
        // closure decodes ONLY rows whose model cell still lacks a handle (lazy),
        // so a pure advance still decodes at most the one cover that just entered.
        const CF_WINDOW: usize = 4;
        let cf_lo = coverflow_index.saturating_sub(CF_WINDOW);
        let cf_hi = (coverflow_index + CF_WINDOW).min(coverflow_rows.len().saturating_sub(1));
        let mut coverflow_art_jobs: Vec<(usize, String)> = Vec::new();
        for (flat_idx, row) in coverflow_rows.iter().enumerate() {
            if !row.artwork_url.is_empty() {
                coverflow_art_jobs.push((flat_idx, row.artwork_url.clone()));
            }
        }

        let weak = self.weak.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            let qs = w.global::<QueueState>();

            // Snapshot prior decoded handles into ONE GLOBAL id -> artwork map
            // covering EVERY prior list (now-playing + upcoming + history +
            // both coverflow lists) BEFORE replacing the models. Coverflow
            // navigation shifts a cover ACROSS lists every click (now-playing ->
            // history, upcoming -> now-playing, ...), so a per-list diff misses
            // the moved covers -> they blank to default -> full re-decode ->
            // flicker + CPU spike. A global map reuses a cover's decoded handle
            // no matter which list it sat in before; net per click only the one
            // genuinely new track decodes.
            let mut prior_all: std::collections::HashMap<slint::SharedString, slint::Image> =
                std::collections::HashMap::new();
            {
                let np = qs.get_now_playing();
                if np.artwork.size().width > 0 {
                    prior_all.insert(np.id.clone(), np.artwork.clone());
                }
                for m in [
                    qs.get_upcoming_page(),
                    qs.get_history(),
                    qs.get_coverflow_tracks(),
                ] {
                    for i in 0..m.row_count() {
                        if let Some(it) = m.row_data(i) {
                            if it.artwork.size().width > 0 {
                                prior_all.entry(it.id.clone()).or_insert(it.artwork.clone());
                            }
                        }
                    }
                }
            }

            let np_item = now_playing
                .as_ref()
                .map(|r| to_item_reuse(r, &prior_all))
                .unwrap_or_default();
            qs.set_has_current(now_playing.is_some());
            qs.set_now_playing(np_item);
            qs.set_now_playing_favorite(now_playing_favorite);

            let page_items: Vec<QueueItem> =
                page_rows.iter().map(|r| to_item_reuse(r, &prior_all)).collect();
            qs.set_upcoming_page(slint::ModelRc::new(slint::VecModel::from(page_items)));
            qs.set_upcoming_total(upcoming_total as i32);
            qs.set_upcoming_remaining(remaining as i32);
            qs.set_page(page as i32);
            qs.set_page_count(page_count as i32);
            qs.set_page_start(page_start as i32);
            qs.set_page_end(page_end as i32);

            let history_items: Vec<QueueItem> =
                history_rows.iter().map(|r| to_item_reuse(r, &prior_all)).collect();
            qs.set_history(slint::ModelRc::new(slint::VecModel::from(history_items)));

            // --- COVERFLOW: gated flat-model update -----------------------
            // KEY INVARIANT. On a PURE ADVANCE/JUMP the id-sequence is unchanged
            // (`!seq_changed`) so we DO NOT call set_coverflow_tracks: the
            // Repeater model is untouched, every visible cover keeps its decoded
            // `slint::Image` handle (no source reassignment, no re-decode), and
            // only the int `coverflow-index` moves -> the .slint `scroll` float
            // animates the fan to the new position. The model is rebuilt ONLY
            // when the contents actually change (new queue / shuffle / add /
            // remove), reusing the global id->handle map so even then only the
            // genuinely-new covers decode.
            if seq_changed {
                let cf_items: Vec<QueueItem> = coverflow_rows
                    .iter()
                    .map(|r| to_item_reuse(r, &prior_all))
                    .collect();
                // The reversed model: same QueueItems (so a track's element holds
                // the SAME decoded handle on both sides — no extra decode), just
                // in reverse order. The RIGHT Repeater iterates THIS so it paints
                // far-upcoming -> near-upcoming, putting the nearer cover on top.
                // Rebuilt under the SAME seq gate as the forward model, so a pure
                // advance never touches it either.
                let mut cf_items_rev = cf_items.clone();
                cf_items_rev.reverse();
                qs.set_coverflow_tracks(slint::ModelRc::new(slint::VecModel::from(cf_items)));
                qs.set_coverflow_tracks_rev(slint::ModelRc::new(slint::VecModel::from(
                    cf_items_rev,
                )));
                qs.set_coverflow_seq_hash(coverflow_seq_hash as i32);
                log::debug!(
                    "[coverflow-perf] rebuild seq={coverflow_seq_hash} len={} idx={coverflow_index}",
                    coverflow_rows.len()
                );
            } else {
                log::debug!(
                    "[coverflow-perf] index-only idx={coverflow_index} (seq unchanged)"
                );
            }
            qs.set_coverflow_index(coverflow_index as i32);

            qs.set_infinite_play(infinite);
            // Keep the Slint tab property in sync with the view state so the
            // Queue/History body always matches the selected tab.
            qs.set_tab(tab);

            // --- COVERFLOW windowed lazy decode (inside the event loop so it
            // can read the live model and SKIP rows that already carry a decoded
            // handle). After a rebuild the to_item_reuse map already filled most
            // window covers; after an index-only update the model is the prior
            // one with handles intact. Either way we emit a decode job ONLY for a
            // window row whose model cell is still a default (0-width) image ->
            // at most ONE decode per advance (the cover that just entered ±4),
            // often zero. Visible covers are NEVER re-decoded (the invariant).
            let cf_model = qs.get_coverflow_tracks();
            // The immersive QUEUE panel (focus mode==5 or split-panel==3, while
            // immersive is open) shows the WHOLE up-next as a list, so every row
            // needs art — widen the window to the full list there. The coverflow
            // FAN (panel closed) keeps the cheap ±4 window. Same gate shape as
            // lyrics_sync's panel detection.
            let imm = w.global::<ImmersiveState>();
            let queue_panel_open = imm.get_open()
                && ((imm.get_view_mode() == 0 && imm.get_mode() == 5)
                    || (imm.get_view_mode() == 1 && imm.get_split_panel() == 3));
            let mut windowed_jobs: Vec<(ArtTarget, String)> = Vec::new();
            for (flat_idx, url) in coverflow_art_jobs.into_iter() {
                let in_window = queue_panel_open || (flat_idx >= cf_lo && flat_idx <= cf_hi);
                if !in_window {
                    continue;
                }
                let needs = cf_model
                    .row_data(flat_idx)
                    .map(|it| it.artwork.size().width == 0)
                    .unwrap_or(false);
                if needs {
                    windowed_jobs.push((ArtTarget::CoverflowFlat(flat_idx), url));
                }
            }
            if !windowed_jobs.is_empty() {
                let plex = crate::plex_settings::get();
                load_artwork(w.as_weak(), windowed_jobs, plex.base_url, plex.token);
            }
        });

        // Plex creds for source-aware art: a Plex queue row carries a raw
        // `/library/...` thumb path that must resolve to a tokenized PlexThumb,
        // not a local-file read miss. Non-Plex paths ignore these.
        let plex = crate::plex_settings::get();
        load_artwork(self.weak.clone(), art_jobs, plex.base_url, plex.token);

        log::debug!(
            "[coverflow-perf] refresh_async total={}ms",
            perf_start.elapsed().as_millis()
        );
    }

    // --- Callbacks --------------------------------------------------------

    /// Play the upcoming entry at `page_index` within the current page.
    /// Resolves the page-local index to a queue-wide upcoming index,
    /// honoring the active search filter, then jumps the core there.
    pub fn play_upcoming(&self, page_index: usize) {
        let this = self.clone();
        self.handle.spawn(async move {
            let Some(upcoming_index) = this.resolve_upcoming_index(page_index).await else {
                log::warn!("[qbz-slint] queue: play_upcoming {page_index} out of range");
                return;
            };
            let Some(track) = this
                .runtime
                .core()
                .play_upcoming_at(upcoming_index)
                .await
            else {
                log::warn!("[qbz-slint] queue: play_upcoming_at {upcoming_index} miss");
                return;
            };
            crate::playback::after_track_change(&this.runtime, &this.weak, track.id).await;
            this.refresh_async().await;
        });
    }

    /// Play an upcoming track by its QUEUE-WIDE (unfiltered) index. The immersive
    /// coverflow lists `state.upcoming.take(3)` regardless of the sidebar's page
    /// or search, so its cards must NOT go through `play_upcoming`'s page-local
    /// `resolve_upcoming_index` (that would play the wrong track when the sidebar
    /// is paged/filtered). History is already queue-wide via `play_history`.
    pub fn play_coverflow_upcoming(&self, upcoming_index: usize) {
        let this = self.clone();
        self.handle.spawn(async move {
            let Some(track) = this.runtime.core().play_upcoming_at(upcoming_index).await else {
                log::warn!(
                    "[qbz-slint] queue: play_coverflow_upcoming {upcoming_index} miss"
                );
                return;
            };
            crate::playback::after_track_change(&this.runtime, &this.weak, track.id).await;
            this.refresh_async().await;
        });
    }

    /// Remove the upcoming entry at `page_index` within the current page.
    pub fn remove_upcoming(&self, page_index: usize) {
        let this = self.clone();
        self.handle.spawn(async move {
            let Some(upcoming_index) = this.resolve_upcoming_index(page_index).await else {
                log::warn!("[qbz-slint] queue: remove_upcoming {page_index} out of range");
                return;
            };
            this.runtime
                .core()
                .remove_upcoming_track(upcoming_index)
                .await;
            this.refresh_async().await;
        });
    }

    /// Reorder the upcoming list: move the page-local row `from_page` to the
    /// insertion slot `to_slot` (0..=page_len). Resolves BOTH to queue-wide
    /// upcoming indices (honoring the search filter), then routes: connected ->
    /// the QConnect cloud (WS-authoritative, the cloud owns queue order); offline
    /// -> core `move_track` (already shuffle-aware) + refresh.
    pub fn reorder(&self, from_page: usize, to_slot: usize) {
        let this = self.clone();
        self.handle.spawn(async move {
            let Some(from_q) = this.resolve_upcoming_index(from_page).await else {
                log::warn!("[qbz-slint] queue: reorder from {from_page} out of range");
                return;
            };
            // `to_slot` is an insertion slot in [0, page_len]. Slot k<page_len sits
            // before page-local row k; slot==page_len appends after the last
            // visible row (one past it in queue-wide upcoming space).
            let page_len = this.current_page_len().await;
            let to_q = if to_slot >= page_len {
                match this.resolve_upcoming_index(page_len.saturating_sub(1)).await {
                    Some(last) => last + 1,
                    None => return,
                }
            } else {
                match this.resolve_upcoming_index(to_slot).await {
                    Some(idx) => idx,
                    None => return,
                }
            };
            if from_q == to_q {
                return;
            }

            // Connected -> the cloud reorders and echoes a QueueUpdated that
            // materialize applies; do NOT also reorder locally (would diverge).
            if let Some(svc) = crate::qconnect_service::service() {
                match svc.reorder_upcoming_if_remote(from_q, to_q).await {
                    Ok(true) => return,
                    Ok(false) => {} // not connected -> local path
                    Err(e) => {
                        log::warn!("[qbz-slint] queue: reorder handoff failed: {e}");
                        return; // connected but errored -> do NOT local-reorder
                    }
                }
            }

            this.runtime.core().move_track(from_q, to_q).await;
            this.refresh_async().await;
        });
    }

    /// Number of upcoming rows on the current (filtered) page — used to detect
    /// the "append after last" insertion slot in `reorder`.
    async fn current_page_len(&self) -> usize {
        let (search, page) = self
            .view
            .lock()
            .map(|v| (v.search.clone(), v.page))
            .unwrap_or_default();
        let query = search.trim().to_lowercase();
        let state = self.runtime.core().get_queue_state_full().await;
        let total = if query.is_empty() {
            state.upcoming.len()
        } else {
            state
                .upcoming
                .iter()
                .filter(|t| {
                    display_title(t).to_lowercase().contains(&query)
                        || t.artist.to_lowercase().contains(&query)
                })
                .count()
        };
        let bounds = paginate(total, page);
        bounds.end - bounds.start
    }

    /// Resolve a page-local upcoming index to a queue-wide upcoming index.
    /// When a search is active the queue-wide index is found by matching
    /// the filtered row back against the unfiltered upcoming list.
    async fn resolve_upcoming_index(&self, page_index: usize) -> Option<usize> {
        let (search, page) = self
            .view
            .lock()
            .map(|v| (v.search.clone(), v.page))
            .ok()?;
        let query = search.trim().to_lowercase();
        let absolute_in_filtered = page * PAGE_SIZE + page_index;

        if query.is_empty() {
            return Some(absolute_in_filtered);
        }

        // Walk the unfiltered upcoming list, counting matches until the
        // requested filtered position is reached.
        let state = self.runtime.core().get_queue_state_full().await;
        let mut matched = 0usize;
        for (idx, track) in state.upcoming.iter().enumerate() {
            let hit = display_title(track).to_lowercase().contains(&query)
                || track.artist.to_lowercase().contains(&query);
            if hit {
                if matched == absolute_in_filtered {
                    return Some(idx);
                }
                matched += 1;
            }
        }
        None
    }

    /// Play a History entry by its `index` in the history list.
    pub fn play_history(&self, index: usize) {
        let this = self.clone();
        self.handle.spawn(async move {
            let state = this.runtime.core().get_queue_state_full().await;
            let Some(track) = state.history.get(index).cloned() else {
                log::warn!("[qbz-slint] queue: play_history {index} out of range");
                return;
            };
            // History plays start a fresh single-track queue, matching the
            // Tauri handlePlayHistoryTrack path (the history list is not a
            // re-entry point into the existing queue order).
            this.runtime.core().set_queue(vec![track.clone()], Some(0)).await;
            crate::playback::after_track_change(&this.runtime, &this.weak, track.id).await;
            this.refresh_async().await;
        });
    }

    /// Empty the queue. When nothing is playing the now-playing slot is
    /// wiped too, mirroring the Tauri `handleClearQueue` behaviour.
    pub fn clear(&self) {
        let this = self.clone();
        self.handle.spawn(async move {
            let playing = this.runtime.core().get_playback_state().is_playing;
            // keep_current = playing: keep the slot only while audible.
            this.runtime.core().clear_queue(playing).await;
            if let Ok(mut view) = this.view.lock() {
                view.page = 0;
                view.search.clear();
            }
            this.refresh_async().await;
        });
    }

    /// Toggle the favorite state of the now-playing track.
    pub fn toggle_favorite(&self) {
        let this = self.clone();
        self.handle.spawn(async move {
            // Offline = read-only hearts (spec 4.3).
            if crate::offline_mode::engine().is_offline() {
                crate::toast::info_weak(&this.weak, "Not available offline");
                return;
            }
            let state = this.runtime.core().get_queue_state_full().await;
            let Some(track) = state.current_track else {
                return;
            };
            let make_favorite = !crate::fav_cache::contains(track.id);
            match this
                .runtime
                .core()
                .set_track_favorite(track.id, make_favorite)
                .await
            {
                Ok(()) => {
                    // Keep the shared cache (memory + disk) in sync so every
                    // other heart surface reflects the change immediately.
                    crate::fav_cache::set(track.id, make_favorite);
                }
                Err(e) => {
                    log::error!("[qbz-slint] queue: toggle favorite failed: {e}");
                }
            }
            this.refresh_async().await;
        });
    }

    /// Toggle infinite-play: flips the persisted autoplay mode between
    /// `InfiniteRadio` and `ContinueWithinSource`, mirroring the Tauri
    /// `handleToggleInfinitePlay` (which calls `setAutoplayMode`).
    pub fn toggle_infinite_play(&self) {
        let this = self.clone();
        self.handle.spawn(async move {
            let enabled = this
                .playback
                .get_preferences()
                .map(|p| p.autoplay_mode == AutoplayMode::InfiniteRadio)
                .unwrap_or(false);
            let next = if enabled {
                AutoplayMode::ContinueWithinSource
            } else {
                AutoplayMode::InfiniteRadio
            };
            if let Err(e) = this.playback.set_autoplay_mode(next) {
                log::error!("[qbz-slint] queue: set autoplay mode failed: {e}");
            }
            this.refresh_async().await;
        });
    }

    /// The current queue track IDs (current + upcoming), in play order —
    /// the set the "save as playlist" action would persist.
    pub fn save_as_playlist(&self) {
        let this = self.clone();
        self.handle.spawn(async move {
            let state = this.runtime.core().get_queue_state_full().await;
            let mut ids: Vec<u64> = Vec::new();
            if let Some(curr) = state.current_track.as_ref() {
                ids.push(curr.id);
            }
            ids.extend(state.upcoming.iter().map(|t| t.id));
            // The Slint MVP has no playlist-name input modal yet, so the
            // queue cannot be persisted into a named playlist here. Log the
            // resolved track set so the wiring is verifiable; the modal is
            // tracked as follow-up work.
            log::info!(
                "[qbz-slint] queue: save-as-playlist requested for {} tracks (modal pending)",
                ids.len()
            );
        });
    }

    /// Move to the previous paginator page.
    pub fn prev_page(&self) {
        if let Ok(mut view) = self.view.lock() {
            view.page = view.page.saturating_sub(1);
        }
        self.refresh();
    }

    /// Move to the next paginator page.
    pub fn next_page(&self) {
        if let Ok(mut view) = self.view.lock() {
            view.page += 1;
        }
        self.refresh();
    }

    /// Switch the active tab (0 = Queue, 1 = History). Pushes the new index
    /// onto the Slint `QueueState.tab` property right away — `refresh()` is
    /// async, so without this the body never switched (the History tab read
    /// `tab == 1` but the property stayed 0, so clicking History did nothing).
    pub fn set_tab(&self, tab: i32) {
        if let Ok(mut view) = self.view.lock() {
            view.tab = tab;
        }
        if let Some(w) = self.weak.upgrade() {
            w.global::<QueueState>().set_tab(tab);
        }
        self.refresh();
    }

    /// Update the search query and re-filter the upcoming list. Changing
    /// the query resets the paginator to the first page.
    pub fn search_changed(&self, query: String) {
        if let Ok(mut view) = self.view.lock() {
            view.search = query;
            view.page = 0;
        }
        self.refresh();
    }
}

/// Where a resolved cover image should land.
#[derive(Clone, Copy)]
enum ArtTarget {
    NowPlaying,
    Upcoming(usize),
    History(usize),
    /// A row in the single stable flat coverflow model, by flat index.
    CoverflowFlat(usize),
}

/// Build a `QueueItem` from plain row data, REUSING a prior decoded artwork
/// handle when the same track id was already on screen. This is the core of the
/// CPU-spike fix: a one-position queue shift keeps the decoded `slint::Image`
/// for every unchanged row instead of resetting it to `Image::default()` and
/// forcing a full re-decode (which also caused the empty-then-fill blink).
///
/// `prior` maps track id -> the decoded image from the model being replaced.
/// Unchanged rows reuse their handle; only genuinely-new rows fall back to the
/// default placeholder (their cover is decoded once by the artwork pipeline).
fn to_item_reuse(
    row: &RowData,
    prior: &std::collections::HashMap<slint::SharedString, slint::Image>,
) -> QueueItem {
    let id: slint::SharedString = row.id.clone().into();
    let artwork = prior.get(&id).cloned().unwrap_or_default();
    QueueItem {
        id: id.clone(),
        title: row.title.clone().into(),
        artist: row.artist.clone().into(),
        artwork,
        playing: row.playing,
        duration: row.duration.clone().into(),
        explicit: row.explicit,
        is_ephemeral: row.is_ephemeral,
    }
}

/// Build the miniplayer's self-contained NAVIGABLE queue model: the current
/// track first, then the FULL upcoming list (NOT capped at 20, NOT paginated —
/// the mini is scrollable). Artwork is left empty in v1 (rows show the
/// placeholder); id/title/artist/duration/explicit are 1:1. The miniplayer
/// owns its own model so it never collides with the sidebar's `QueueState`.
pub(crate) fn mini_queue_items(state: &qbz_models::QueueState) -> Vec<QueueItem> {
    let empty: std::collections::HashMap<slint::SharedString, slint::Image> =
        std::collections::HashMap::new();
    let mut out: Vec<QueueItem> = Vec::with_capacity(1 + state.upcoming.len());
    if let Some(t) = state.current_track.as_ref() {
        out.push(to_item_reuse(&row_from(t, true), &empty));
    }
    for t in state.upcoming.iter() {
        out.push(to_item_reuse(&row_from(t, false), &empty));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: u64, title: &str, version: Option<&str>) -> QueueTrack {
        QueueTrack {
            id,
            title: title.to_string(),
            version: version.map(|v| v.to_string()),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration_secs: 100,
            artwork_url: None,
            hires: false,
            bit_depth: None,
            sample_rate: None,
            is_local: false,
            album_id: None,
            artist_id: None,
            streamable: true,
            source: None,
            parental_warning: false,
            source_item_id_hint: None,
        }
    }

    #[test]
    fn fmt_duration_pads_seconds() {
        assert_eq!(fmt_duration(0), "0:00");
        assert_eq!(fmt_duration(9), "0:09");
        assert_eq!(fmt_duration(65), "1:05");
        assert_eq!(fmt_duration(3725), "62:05");
    }

    #[test]
    fn display_title_appends_version() {
        assert_eq!(display_title(&track(1, "Song", Some("Live"))), "Song (Live)");
        assert_eq!(display_title(&track(1, "Song", None)), "Song");
        // Empty version string is treated as no version.
        assert_eq!(display_title(&track(1, "Song", Some(""))), "Song");
    }

    #[test]
    fn row_from_marks_playing_and_explicit() {
        let mut t = track(7, "Song", Some("Mix"));
        t.parental_warning = true;
        t.duration_secs = 215;
        let row = row_from(&t, true);
        assert_eq!(row.id, "7");
        assert_eq!(row.title, "Song (Mix)");
        assert_eq!(row.duration, "3:35");
        assert!(row.playing);
        assert!(row.explicit);
    }

    #[test]
    fn paginate_single_page() {
        let b = paginate(10, 0);
        assert_eq!(b.page_count, 1);
        assert_eq!(b.page, 0);
        assert_eq!((b.start, b.end), (0, 10));
    }

    #[test]
    fn paginate_exact_page_boundary() {
        // Exactly PAGE_SIZE items -> one full page, not two.
        let b = paginate(PAGE_SIZE, 0);
        assert_eq!(b.page_count, 1);
        assert_eq!((b.start, b.end), (0, PAGE_SIZE));
    }

    #[test]
    fn paginate_spans_multiple_pages() {
        // 95 items, 40 per page -> 3 pages (40 / 40 / 15).
        let total = 95;
        let p0 = paginate(total, 0);
        assert_eq!(p0.page_count, 3);
        assert_eq!((p0.start, p0.end), (0, 40));
        let p1 = paginate(total, 1);
        assert_eq!((p1.start, p1.end), (40, 80));
        let p2 = paginate(total, 2);
        assert_eq!((p2.start, p2.end), (80, 95));
    }

    #[test]
    fn paginate_clamps_overshot_page() {
        // Requesting page 9 of a 2-page list clamps to the last page.
        let b = paginate(50, 9);
        assert_eq!(b.page_count, 2);
        assert_eq!(b.page, 1);
        assert_eq!((b.start, b.end), (40, 50));
    }

    #[test]
    fn paginate_empty_list_has_one_page() {
        let b = paginate(0, 0);
        assert_eq!(b.page_count, 1);
        assert_eq!((b.start, b.end), (0, 0));
    }
}

/// Resolve cover art for each job and apply it onto the matching row in
/// the `QueueState` global. One task per cover; misses are skipped.
fn load_artwork(
    weak: slint::Weak<AppWindow>,
    jobs: Vec<(ArtTarget, String)>,
    plex_base_url: String,
    plex_token: String,
) {
    /// Decode size for all queue/coverflow covers (matches the artwork pipeline).
    const QUEUE_DECODE: u32 = 96;

    let Some(cache) = crate::artwork::shared_cache() else {
        return;
    };
    for (target, url) in jobs {
        let weak = weak.clone();
        let cache = cache.clone();
        // Source-aware: queue covers may be remote (Qobuz) OR local file
        // paths (Local Library / offline) OR a raw Plex thumb path. Route file
        // paths through ArtworkRef::LocalFile (decode from disk), and a bare
        // `/library/`-or-`/photo/` Plex path through ArtworkRef::PlexThumb with
        // current creds (tokenized HTTP fetch) — a raw local read of a Plex
        // path 404s/misses, leaving Plex rows art-less.
        let is_plex_path = url.starts_with("/library/") || url.starts_with("/photo/");
        let art = if url.starts_with("http://") || url.starts_with("https://") {
            qbz_models::ArtworkRef::Remote(url)
        } else if let Some(p) = url.strip_prefix("file://") {
            qbz_models::ArtworkRef::LocalFile(p.to_string())
        } else if is_plex_path && !plex_base_url.is_empty() && !plex_token.is_empty() {
            qbz_models::ArtworkRef::PlexThumb {
                base_url: plex_base_url.clone(),
                token: plex_token.clone(),
                path: url,
                // Queue rows + now-playing item render small; request a
                // 96px server-side transcode (the decode size used below).
                size: Some(QUEUE_DECODE),
            }
        } else {
            qbz_models::ArtworkRef::LocalFile(url)
        };

        // Decoded-pixel fast path: if this exact cover was already decoded at
        // this size (true for the covers still on screen after a one-position
        // shift), upload the cached pixels on the event loop and SKIP the tokio
        // decode entirely. This is the bulk of the per-click CPU-spike fix.
        let cache_key = match &art {
            qbz_models::ArtworkRef::Remote(u) => Some(u.clone()),
            qbz_models::ArtworkRef::LocalFile(p) => Some(p.clone()),
            qbz_models::ArtworkRef::PlexThumb { base_url, token, path, size } => {
                Some(qbz_models::plex_thumb_url(base_url, token, path, *size))
            }
            _ => None,
        };
        if let Some(key) = cache_key.as_deref() {
            if let Some((pixels, w, h)) = crate::artwork::decoded_pixels(key, QUEUE_DECODE) {
                if let ArtTarget::CoverflowFlat(i) = target {
                    log::debug!("[coverflow-perf] cache-hit flat_idx={i}");
                }
                let weak = weak.clone();
                let _ = weak.upgrade_in_event_loop(move |win| {
                    let img = crate::artwork::pixels_to_image(&pixels, w, h);
                    apply_queue_art(&win, target, img);
                });
                continue;
            }
        }

        let perf_url = match target {
            ArtTarget::CoverflowFlat(i) => Some((i, cache_key.clone().unwrap_or_default())),
            _ => None,
        };
        tokio::spawn(async move {
            if let Some((i, u)) = perf_url.as_ref() {
                log::debug!("[coverflow-perf] decode flat_idx={i} url={u}");
            }
            let Some((pixels, w, h)) =
                crate::artwork::fetch_and_decode_ref(&art, &cache, QUEUE_DECODE).await
            else {
                return;
            };
            let _ = weak.upgrade_in_event_loop(move |win| {
                let img = crate::artwork::pixels_to_image(&pixels, w, h);
                apply_queue_art(&win, target, img);
            });
        });
    }
}

/// Apply a resolved cover onto its queue/coverflow row. Runs on the event loop.
fn apply_queue_art(win: &AppWindow, target: ArtTarget, img: slint::Image) {
    let qs = win.global::<QueueState>();
    match target {
        ArtTarget::NowPlaying => {
            let mut item = qs.get_now_playing();
            item.artwork = img;
            qs.set_now_playing(item);
        }
        ArtTarget::Upcoming(idx) => {
            let items = qs.get_upcoming_page();
            if let Some(mut item) = items.row_data(idx) {
                item.artwork = img;
                items.set_row_data(idx, item);
            }
        }
        ArtTarget::History(idx) => {
            let items = qs.get_history();
            if let Some(mut item) = items.row_data(idx) {
                item.artwork = img;
                items.set_row_data(idx, item);
            }
        }
        ArtTarget::CoverflowFlat(idx) => {
            let items = qs.get_coverflow_tracks();
            if let Some(mut item) = items.row_data(idx) {
                item.artwork = img.clone();
                // set_row_data on a SINGLE flat row — does NOT replace the
                // VecModel, so the Repeater is not rebuilt and the other covers'
                // sources are untouched. Only this one cell's image changes.
                items.set_row_data(idx, item);
            }
            // Mirror the decode onto the reversed model used by the RIGHT
            // Repeater. The reversed model is the forward list reversed, so the
            // same track sits at `len-1-idx`. Updating a single row here (not
            // replacing the VecModel) keeps that Repeater stable too — no rebuild,
            // no churn — it just fills in the same cover the forward side got.
            let rev = qs.get_coverflow_tracks_rev();
            let rev_len = rev.row_count();
            if rev_len > 0 && idx < rev_len {
                let rev_idx = rev_len - 1 - idx;
                if let Some(mut item) = rev.row_data(rev_idx) {
                    item.artwork = img;
                    rev.set_row_data(rev_idx, item);
                }
            }
        }
    }
}
