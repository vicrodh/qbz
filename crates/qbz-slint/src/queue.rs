//! Queue sidebar controller.
//!
//! Ported from the Tauri `QueuePanel.svelte`. The core's `QueueManager`
//! owns the authoritative track list; this controller owns the
//! *sidebar-local* view state — which tab is shown, the search query, the
//! current paginator page — and the cached favorite-track IDs used to
//! light the NOW PLAYING heart.
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
use crate::{AppWindow, QueueItem, QueueState};

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
    /// Cached set of the user's favorite track IDs.
    favorites: std::collections::HashSet<u64>,
    /// Whether the favorites cache has been populated at least once.
    favorites_loaded: bool,
}

/// The Queue sidebar controller — see the module docs.
pub struct QueueController {
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    playback: qbz_app::settings::playback::PlaybackPreferencesState,
    view: Arc<Mutex<ViewState>>,
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

    /// Refresh the favorites cache, then refresh the queue view.
    pub fn refresh_with_favorites(&self) {
        let this = self.clone();
        self.handle.spawn(async move {
            this.reload_favorites().await;
            this.refresh_async().await;
        });
    }

    /// Fetch the user's favorite-track IDs from Qobuz into the cache.
    async fn reload_favorites(&self) {
        match self.runtime.core().favorite_track_ids().await {
            Ok(ids) => {
                if let Ok(mut view) = self.view.lock() {
                    view.favorites = ids;
                    view.favorites_loaded = true;
                }
            }
            Err(e) => {
                log::warn!("[qbz-slint] queue: favorite_track_ids failed: {e}");
            }
        }
    }

    async fn refresh_async(&self) {
        // Lazily populate the favorites cache the first time the queue is
        // shown so the NOW PLAYING heart is correct without a manual sync.
        let need_favorites = self
            .view
            .lock()
            .map(|v| !v.favorites_loaded)
            .unwrap_or(false);
        if need_favorites {
            self.reload_favorites().await;
        }

        let state = self.runtime.core().get_queue_state_full().await;

        // --- NOW PLAYING --------------------------------------------------
        let now_playing = state.current_track.as_ref().map(|t| row_from(t, true));
        let now_playing_favorite = state
            .current_track
            .as_ref()
            .map(|t| {
                self.view
                    .lock()
                    .map(|v| v.favorites.contains(&t.id))
                    .unwrap_or(false)
            })
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

        let weak = self.weak.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            let qs = w.global::<QueueState>();

            let np_item = now_playing
                .as_ref()
                .map(|r| to_item(r))
                .unwrap_or_default();
            qs.set_has_current(now_playing.is_some());
            qs.set_now_playing(np_item);
            qs.set_now_playing_favorite(now_playing_favorite);

            let page_items: Vec<QueueItem> = page_rows.iter().map(to_item).collect();
            qs.set_upcoming_page(slint::ModelRc::new(slint::VecModel::from(page_items)));
            qs.set_upcoming_total(upcoming_total as i32);
            qs.set_upcoming_remaining(remaining as i32);
            qs.set_page(page as i32);
            qs.set_page_count(page_count as i32);
            qs.set_page_start(page_start as i32);
            qs.set_page_end(page_end as i32);

            let history_items: Vec<QueueItem> = history_rows.iter().map(to_item).collect();
            qs.set_history(slint::ModelRc::new(slint::VecModel::from(history_items)));

            qs.set_infinite_play(infinite);
            // Keep the Slint tab property in sync with the view state so the
            // Queue/History body always matches the selected tab.
            qs.set_tab(tab);
        });

        // Plex creds for source-aware art: a Plex queue row carries a raw
        // `/library/...` thumb path that must resolve to a tokenized PlexThumb,
        // not a local-file read miss. Non-Plex paths ignore these.
        let plex = crate::plex_settings::get();
        load_artwork(self.weak.clone(), art_jobs, plex.base_url, plex.token);
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
            let state = this.runtime.core().get_queue_state_full().await;
            let Some(track) = state.current_track else {
                return;
            };
            let currently = this
                .view
                .lock()
                .map(|v| v.favorites.contains(&track.id))
                .unwrap_or(false);
            let make_favorite = !currently;
            match this
                .runtime
                .core()
                .set_track_favorite(track.id, make_favorite)
                .await
            {
                Ok(()) => {
                    if let Ok(mut view) = this.view.lock() {
                        if make_favorite {
                            view.favorites.insert(track.id);
                        } else {
                            view.favorites.remove(&track.id);
                        }
                    }
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
}

/// Build a `QueueItem` from plain row data (no artwork — that resolves
/// asynchronously and is set onto the row afterward).
fn to_item(row: &RowData) -> QueueItem {
    QueueItem {
        id: row.id.clone().into(),
        title: row.title.clone().into(),
        artist: row.artist.clone().into(),
        artwork: slint::Image::default(),
        playing: row.playing,
        duration: row.duration.clone().into(),
        explicit: row.explicit,
        is_ephemeral: row.is_ephemeral,
    }
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
                size: Some(96),
            }
        } else {
            qbz_models::ArtworkRef::LocalFile(url)
        };
        tokio::spawn(async move {
            let Some((pixels, w, h)) =
                crate::artwork::fetch_and_decode_ref(&art, &cache, 96).await
            else {
                return;
            };
            let _ = weak.upgrade_in_event_loop(move |win| {
                let img = crate::artwork::pixels_to_image(&pixels, w, h);
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
                }
            });
        });
    }
}
