//! Playlist "Suggested Songs" section controller (T8).
//!
//! 1:1 port of the Svelte `PlaylistSuggestions.svelte` + the slice of
//! `PlaylistDetailView.svelte` that derives the seed artists / excludes and
//! mounts the component. All assembly logic lives in Rust (ADR-006): a
//! Rust-held pool + pagination feeds `PlaylistSuggestionsState`; the `.slint`
//! section only renders the projected rows + flags and fires the `Actions`.
//!
//! DISTINCT from the immersive `crate::suggestions` controller (different
//! surface, different engine). The backend engine is `qbz_reco` via
//! `core.generate_playlist_suggestions(...)`; the per-playlist dismiss store is
//! `crate::playlist_suggestions_dismiss` (T10).
//!
//! Pool sizing + pagination mirror the Svelte constants:
//!   VISIBLE_COUNT=6, INITIAL_POOL=30, EXPANDED_POOL=100, MAX_POOL=200,
//!   auto-expand when the filtered (available) pool falls below 12.
//! Filtering removes: dismissed ids (T10 store), excluded ids (already in the
//! playlist), suggestions that duplicate an existing track by `title|artist`,
//! and duplicates within the pool itself by the same key.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_models::Track;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AppWindow, PlaylistSuggestionRow, PlaylistSuggestionsState, PlaylistState};

type Runtime = std::sync::Arc<AppRuntime<SlintAdapter>>;
type Handle = tokio::runtime::Handle;
type Weak = slint::Weak<AppWindow>;

// --- Svelte parity constants -----------------------------------------------
const VISIBLE_COUNT: usize = 6;
const INITIAL_POOL: usize = 30;
const EXPANDED_POOL: usize = 100;
const MAX_POOL: usize = 200;
/// Auto-expand the pool once the available (filtered) tracks drop below this.
const MIN_AVAILABLE_THRESHOLD: usize = 12;

/// Which fetch we are running — drives the merge-vs-replace + error handling.
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// First load for the open playlist: replaces the pool, surfaces errors.
    Initial,
    /// Background pool growth (cycle-wrap load-more / variety): merges, silent.
    Merge,
}

/// The live suggestions session for the open playlist. Held in Rust (the UI
/// only ever sees the projected rows + flags on `PlaylistSuggestionsState`).
#[derive(Default)]
struct Session {
    /// Open playlist id (Qobuz catalog id). 0 = no active session.
    playlist_id: u64,
    /// Seed artists sent to the engine — stable across load-more within a
    /// session (Svelte: the `artists` prop only recomputes on track change).
    artists: Vec<(Option<u64>, String)>,
    /// Track ids already in the playlist (excluded from suggestions). Grows as
    /// the user adds suggested tracks.
    exclude_ids: HashSet<u64>,
    /// `title|artist` keys of existing playlist tracks (de-dupe vs the playlist).
    existing_keys: HashSet<String>,
    /// The full fetched pool (de-duped on merge by id).
    pool: Vec<qbz_reco::SuggestedTrack>,
    /// Current visible page (0-based; window of VISIBLE_COUNT).
    page: usize,
    /// How many full cycles through the pages the user has completed.
    completed_cycles: usize,
    /// True once the first fetch has returned.
    loaded_once: bool,
    /// A foreground (initial) fetch is in flight.
    loading: bool,
    /// A background pool expansion (load-more / variety) is in flight.
    loading_more: bool,
    /// True once a MAX_POOL request has been issued — prevents auto-expand from
    /// looping when the engine returns fewer than MAX_POOL tracks.
    max_requested: bool,
}

static SESSION: LazyLock<Mutex<Session>> = LazyLock::new(|| Mutex::new(Session::default()));

// --- string helpers (mirror the Svelte normalizeForComparison/makeTrackKey) -
fn normalize(s: &str) -> String {
    s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn make_key(title: &str, artist: &str) -> String {
    format!("{}|{}", normalize(title), normalize(artist))
}

fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Deterministic splitmix64 step (qbz-radio's RNG family) — used for the
/// adaptive-artist shuffle so the seed selection is varied but reproducible per
/// playlist (no `rand` pulled into this hot path).
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed ^ 0xD1B5_4A32_D192_ED03;
    for i in (1..items.len()).rev() {
        let j = (splitmix64(&mut state) % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

/// Adaptive seed-artist selection — 1:1 with the Svelte `extractAdaptiveArtists`
/// (quantity scales with playlist size; a 60/40 top-frequency/random mix for
/// coherence + discovery; the final selection shuffled). Keeps the engine's
/// per-artist MusicBrainz resolution bounded on large playlists.
fn extract_adaptive_artists(tracks: &[Track], playlist_id: u64) -> Vec<(Option<u64>, String)> {
    // Count tracks per artist name (first-seen qobuz id retained).
    let mut order: Vec<String> = Vec::new();
    let mut counts: std::collections::HashMap<String, (usize, Option<u64>)> =
        std::collections::HashMap::new();
    for track in tracks {
        let Some(performer) = track.performer.as_ref() else {
            continue;
        };
        let name = performer.name.trim();
        if name.is_empty() {
            continue;
        }
        let entry = counts.entry(name.to_string()).or_insert_with(|| {
            order.push(name.to_string());
            (0, Some(performer.id))
        });
        entry.0 += 1;
    }

    let unique = order.len();
    if unique == 0 {
        return Vec::new();
    }

    let n = tracks.len();
    let limit = if n < 15 {
        5.min(n).max(3)
    } else if n < 50 {
        10.min(((n as f64) * 0.3).ceil() as usize)
    } else if n < 100 {
        15.min(((n as f64) * 0.2).ceil() as usize)
    } else {
        20.min(((n as f64) * 0.15).ceil() as usize)
    };
    let actual = limit.min(unique);

    // Sorted (count desc, then first-seen order for stability).
    let mut sorted: Vec<(String, Option<u64>)> = order
        .iter()
        .map(|name| {
            let (_, qid) = counts[name];
            (name.clone(), qid)
        })
        .collect();
    let count_of = |name: &str| counts.get(name).map(|c| c.0).unwrap_or(0);
    sorted.sort_by(|a, b| count_of(b.0.as_str()).cmp(&count_of(a.0.as_str())));

    let to_pair = |(name, qid): (String, Option<u64>)| (qid, name);

    // Few artists: return all, shuffled.
    if unique <= actual {
        let mut all: Vec<(String, Option<u64>)> = sorted;
        shuffle(&mut all, playlist_id);
        return all.into_iter().map(to_pair).collect();
    }

    let top_count = 1.max(((actual as f64) * 0.6).floor() as usize);
    let random_count = actual - top_count;

    let top: Vec<(String, Option<u64>)> = sorted[..top_count].to_vec();
    let mut rest: Vec<(String, Option<u64>)> = sorted[top_count..].to_vec();
    shuffle(&mut rest, playlist_id ^ 0x5EED);
    let random: Vec<(String, Option<u64>)> = rest.into_iter().take(random_count).collect();

    let mut combined: Vec<(String, Option<u64>)> = top.into_iter().chain(random).collect();
    shuffle(&mut combined, playlist_id ^ 0xA5A5);
    combined.into_iter().map(to_pair).collect()
}

/// Compute the indices into `session.pool` that survive filtering, in pool
/// order: not dismissed, not excluded, not a duplicate of an existing playlist
/// track, and de-duplicated within the pool by `title|artist`.
fn filtered_indices(session: &Session) -> Vec<usize> {
    let dismissed = crate::playlist_suggestions_dismiss::dismissed_for_playlist(session.playlist_id);
    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (idx, item) in session.pool.iter().enumerate() {
        if dismissed.contains(&item.track_id) || session.exclude_ids.contains(&item.track_id) {
            continue;
        }
        let key = make_key(&item.title, &item.artist_name);
        if session.existing_keys.contains(&key) {
            continue;
        }
        if !seen_keys.insert(key) {
            continue;
        }
        out.push(idx);
    }
    out
}

fn to_row(track: &qbz_reco::SuggestedTrack) -> PlaylistSuggestionRow {
    PlaylistSuggestionRow {
        track_id: track.track_id.to_string().into(),
        title: track.title.clone().into(),
        artist_name: track.artist_name.clone().into(),
        artist_id: track
            .artist_id
            .map(|id| id.to_string())
            .unwrap_or_default()
            .into(),
        album_title: track.album_title.clone().into(),
        album_id: track.album_id.clone().into(),
        artwork_url: track.album_image_url.clone().unwrap_or_default().into(),
        artwork: slint::Image::default(),
        duration_label: mmss(track.duration).into(),
        reason: track.reason.clone().unwrap_or_default().into(),
        adding: false,
        added: false,
    }
}

/// Project the current session onto `PlaylistSuggestionsState` (visible page +
/// flags) and fire the row-cover artwork jobs. UI thread.
fn project(window: &AppWindow) {
    let (rows, has_more, is_empty, loading, loading_more, jobs): (
        Vec<PlaylistSuggestionRow>,
        bool,
        bool,
        bool,
        bool,
        Vec<ArtworkJob>,
    ) = {
        let mut session = SESSION.lock().unwrap();
        let filtered = filtered_indices(&session);
        let total_pages = filtered.len().div_ceil(VISIBLE_COUNT);
        // Clamp + persist the page so a dismiss/add that shrinks the pool below
        // the current window snaps back to the last real page (no empty view).
        session.page = session.page.min(total_pages.saturating_sub(1));
        let page = session.page;
        let start = page * VISIBLE_COUNT;
        let visible: Vec<&qbz_reco::SuggestedTrack> = filtered
            .iter()
            .skip(start)
            .take(VISIBLE_COUNT)
            .map(|&i| &session.pool[i])
            .collect();
        let mut jobs = Vec::new();
        let rows: Vec<PlaylistSuggestionRow> = visible
            .iter()
            .enumerate()
            .map(|(idx, track)| {
                if !track.album_image_url.as_deref().unwrap_or("").is_empty() {
                    jobs.push(ArtworkJob {
                        url: track.album_image_url.clone().unwrap_or_default(),
                        target: ArtworkTarget::PlaylistSuggestionCover { idx },
                    });
                }
                to_row(track)
            })
            .collect();
        let has_more = page + 1 < total_pages;
        let is_empty = filtered.is_empty() && !session.loading && session.loaded_once;
        (
            rows,
            has_more,
            is_empty,
            session.loading,
            session.loading_more,
            jobs,
        )
    };

    let state = window.global::<PlaylistSuggestionsState>();
    state.set_rows(ModelRc::new(VecModel::from(rows)));
    state.set_has_more(has_more);
    state.set_is_empty(is_empty);
    state.set_loading(loading);
    state.set_loading_more(loading_more);

    if !jobs.is_empty() {
        if let Some(cache) = crate::artwork::shared_cache() {
            crate::artwork::spawn_loads(jobs, window.as_weak(), cache);
        }
    }
}

/// Fetch a pool page and (Initial) replace or (Merge) merge it, then re-project.
fn spawn_fetch(runtime: Runtime, weak: Weak, handle: Handle, pool_size: usize, phase: Phase) {
    // Capture which playlist this fetch is for; a navigation / re-activate that
    // swaps the session before it returns must discard the stale result.
    let (pid, artists, exclude): (u64, Vec<(Option<u64>, String)>, Vec<u64>) = {
        let mut session = SESSION.lock().unwrap();
        match phase {
            Phase::Initial => session.loading = true,
            Phase::Merge => {
                session.loading_more = true;
                if pool_size >= MAX_POOL {
                    session.max_requested = true;
                }
            }
        }
        (
            session.playlist_id,
            session.artists.clone(),
            session.exclude_ids.iter().copied().collect(),
        )
    };

    let runtime2 = runtime.clone();
    let weak2 = weak.clone();
    let handle2 = handle.clone();
    handle.spawn(async move {
        let config = qbz_reco::SuggestionConfig {
            max_pool_size: pool_size,
            ..Default::default()
        };
        let result = runtime2
            .core()
            .generate_playlist_suggestions(artists, exclude, false, Some(config))
            .await;

        match result {
            Ok(result) => {
                let applied = {
                    let mut session = SESSION.lock().unwrap();
                    if session.playlist_id != pid {
                        false // superseded by a navigation / re-activate
                    } else {
                        match phase {
                            Phase::Initial => {
                                session.pool = result.tracks;
                                session.page = 0;
                                session.completed_cycles = 0;
                                session.loading = false;
                            }
                            Phase::Merge => {
                                let existing: HashSet<u64> =
                                    session.pool.iter().map(|t| t.track_id).collect();
                                for track in result.tracks {
                                    if !existing.contains(&track.track_id) {
                                        session.pool.push(track);
                                    }
                                }
                                session.loading_more = false;
                            }
                        }
                        session.loaded_once = true;
                        true
                    }
                };
                if applied {
                    let _ = weak2.upgrade_in_event_loop(|w| project(&w));
                    maybe_auto_expand(runtime2, weak2, handle2);
                }
            }
            Err(e) => {
                let surface = {
                    let mut session = SESSION.lock().unwrap();
                    if session.playlist_id != pid {
                        None // stale — leave the current session untouched
                    } else {
                        match phase {
                            Phase::Initial => {
                                session.loading = false;
                                session.pool.clear();
                                session.loaded_once = true;
                                Some(true)
                            }
                            Phase::Merge => {
                                session.loading_more = false;
                                Some(false)
                            }
                        }
                    }
                };
                let Some(surface) = surface else {
                    return;
                };
                log::warn!("[qbz-slint] playlist suggestions fetch failed: {e}");
                let _ = weak2.upgrade_in_event_loop(move |w| {
                    let state = w.global::<PlaylistSuggestionsState>();
                    state.set_loading(false);
                    state.set_loading_more(false);
                    if surface {
                        state.set_error(e.into());
                        project(&w);
                    }
                });
            }
        }
    });
}

/// Grow the pool to MAX_POOL when the filtered (available) tracks fall below
/// the threshold (Svelte's pool-exhaustion auto-refresh). One-shot per session
/// (guarded by `max_requested`) so a thin engine result never loops.
fn maybe_auto_expand(runtime: Runtime, weak: Weak, handle: Handle) {
    let should = {
        let session = SESSION.lock().unwrap();
        let available = filtered_indices(&session).len();
        available > 0
            && available < MIN_AVAILABLE_THRESHOLD
            && session.loaded_once
            && !session.loading
            && !session.loading_more
            && !session.max_requested
            && session.pool.len() < MAX_POOL
    };
    if should {
        let _ = weak.upgrade_in_event_loop(|w| {
            w.global::<PlaylistSuggestionsState>().set_loading_more(true);
        });
        spawn_fetch(runtime, weak, handle, MAX_POOL, Phase::Merge);
    }
}

/// Launch suggestions for the open playlist: gather the seed artists + excludes
/// off the loaded Qobuz tracks, then fetch the first pool page. UI thread.
pub fn activate(window: &AppWindow, runtime: Runtime, handle: Handle) {
    let playlist_id = window
        .global::<PlaylistState>()
        .get_id()
        .parse::<u64>()
        .unwrap_or(0);
    let tracks = crate::playlist::current_tracks();
    let artists = extract_adaptive_artists(&tracks, playlist_id);
    let exclude_ids: HashSet<u64> = tracks.iter().map(|t| t.id).collect();
    let existing_keys: HashSet<String> = tracks
        .iter()
        .map(|t| {
            let artist = t.performer.as_ref().map(|p| p.name.as_str()).unwrap_or("");
            make_key(&t.title, artist)
        })
        .collect();

    {
        let mut session = SESSION.lock().unwrap();
        *session = Session {
            playlist_id,
            artists: artists.clone(),
            exclude_ids,
            existing_keys,
            ..Default::default()
        };
    }

    let state = window.global::<PlaylistSuggestionsState>();
    state.set_activated(true);
    state.set_error("".into());
    state.set_is_empty(false);
    state.set_rows(ModelRc::new(VecModel::from(Vec::<PlaylistSuggestionRow>::new())));

    // No resolvable seed artists (e.g. a fully-local playlist) -> empty, hidden.
    if playlist_id == 0 || artists.is_empty() {
        let mut session = SESSION.lock().unwrap();
        session.loaded_once = true;
        drop(session);
        state.set_loading(false);
        state.set_is_empty(true);
        return;
    }

    state.set_loading(true);
    spawn_fetch(runtime, window.as_weak(), handle, INITIAL_POOL, Phase::Initial);
}

/// Advance the visible page; on a full cycle, wrap to page 0 and (first cycle)
/// kick the EXPANDED_POOL load-more. UI thread.
pub fn refresh(window: &AppWindow, runtime: Runtime, handle: Handle) {
    let expand = {
        let mut session = SESSION.lock().unwrap();
        if session.loading {
            return;
        }
        let total_pages = filtered_indices(&session).len().div_ceil(VISIBLE_COUNT);
        if session.page + 1 < total_pages {
            session.page += 1;
            false
        } else if total_pages > 0 {
            session.page = 0;
            session.completed_cycles += 1;
            session.completed_cycles == 1
                && !session.loading_more
                && session.pool.len() < EXPANDED_POOL
        } else {
            false
        }
    };
    project(window);
    if expand {
        window.global::<PlaylistSuggestionsState>().set_loading_more(true);
        spawn_fetch(
            runtime.clone(),
            window.as_weak(),
            handle.clone(),
            EXPANDED_POOL,
            Phase::Merge,
        );
    } else {
        maybe_auto_expand(runtime, window.as_weak(), handle);
    }
}

/// Add a suggested track to the open playlist, drop it from the pool, and
/// reload the detail so the new track appears in the list. UI thread.
pub fn add_track(window: &AppWindow, runtime: Runtime, handle: Handle, track_id: String) {
    let Ok(tid) = track_id.parse::<u64>() else {
        return;
    };
    let playlist_id = {
        let session = SESSION.lock().unwrap();
        session.playlist_id
    };
    if playlist_id == 0 {
        return;
    }

    // Optimistic "adding" flag on the visible row.
    set_row_flag(window, &track_id, true, false);

    let weak = window.as_weak();
    let runtime2 = runtime.clone();
    let handle2 = handle.clone();
    handle.spawn(async move {
        match runtime2
            .core()
            .add_tracks_to_playlist(playlist_id, &[tid])
            .await
        {
            Ok(()) => {
                {
                    let mut session = SESSION.lock().unwrap();
                    session.exclude_ids.insert(tid);
                    if let Some(track) = session.pool.iter().find(|t| t.track_id == tid) {
                        let key = make_key(&track.title, &track.artist_name);
                        session.existing_keys.insert(key);
                    }
                    session.pool.retain(|t| t.track_id != tid);
                }
                let runtime3 = runtime2.clone();
                let weak3 = weak.clone();
                let handle3 = handle2.clone();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    project(&w);
                    reload_open_playlist(&w, runtime3, handle3, playlist_id);
                    maybe_auto_expand(runtime2, weak3, handle2);
                });
            }
            Err(e) => {
                log::warn!("[qbz-slint] add suggested track {tid} failed: {e}");
                let _ = weak.upgrade_in_event_loop(move |w| {
                    set_row_flag(&w, &track_id, false, false);
                });
            }
        }
    });
}

/// Preview / play a single suggested track now. UI thread.
pub fn play_track(runtime: Runtime, weak: Weak, handle: Handle, track_id: String) {
    let Ok(tid) = track_id.parse::<u64>() else {
        return;
    };
    let handle2 = handle.clone();
    handle.spawn(async move {
        match runtime.core().get_track(tid).await {
            Ok(track) => {
                crate::playback::play_tracks(runtime, weak, handle2, vec![track], 0);
            }
            Err(e) => log::warn!("[qbz-slint] preview suggested track {tid} failed: {e}"),
        }
    });
}

/// Dismiss a suggestion (sticky per-playlist via the T10 store) and drop it from
/// the pool. UI thread.
pub fn dismiss_track(window: &AppWindow, runtime: Runtime, handle: Handle, track_id: String) {
    let Ok(tid) = track_id.parse::<u64>() else {
        return;
    };
    {
        let mut session = SESSION.lock().unwrap();
        if session.playlist_id == 0 {
            return;
        }
        crate::playlist_suggestions_dismiss::dismiss(session.playlist_id, tid);
        session.pool.retain(|t| t.track_id != tid);
    }
    project(window);
    maybe_auto_expand(runtime, window.as_weak(), handle);
}

/// Flip the `adding` flag on the visible row that matches `track_id` (so the
/// per-row add button can show its in-flight state).
fn set_row_flag(window: &AppWindow, track_id: &str, adding: bool, added: bool) {
    let model = window.global::<PlaylistSuggestionsState>().get_rows();
    for i in 0..model.row_count() {
        if let Some(mut row) = model.row_data(i) {
            if row.track_id.as_str() == track_id {
                row.adding = adding;
                row.added = added;
                model.set_row_data(i, row);
                break;
            }
        }
    }
}

/// Quietly reload the open Qobuz playlist detail after a mutation (no nav
/// record / view change — we are already on the playlist view). Refreshes the
/// track list + counts so an added suggestion shows immediately.
fn reload_open_playlist(window: &AppWindow, runtime: Runtime, handle: Handle, playlist_id: u64) {
    let weak = window.as_weak();
    handle.spawn(async move {
        if let Some(data) = crate::playlist::load(&runtime, playlist_id).await {
            let (http_jobs, local_jobs, plex_jobs) = crate::playlist::artwork_jobs(&data);
            let _ = weak.upgrade_in_event_loop(move |w| {
                crate::playlist::apply(&w, data);
            });
            if let Some(cache) = crate::artwork::shared_cache() {
                if !http_jobs.is_empty() {
                    crate::artwork::spawn_loads(http_jobs, weak.clone(), cache.clone());
                }
                if !local_jobs.is_empty() {
                    crate::artwork::spawn_local_loads(local_jobs, weak.clone(), cache.clone());
                }
                if !plex_jobs.is_empty() {
                    let plex = crate::plex_settings::get();
                    crate::artwork::spawn_local_or_plex_loads(
                        plex_jobs,
                        plex.base_url,
                        plex.token,
                        weak.clone(),
                        cache,
                    );
                }
            }
        }
    });
}

/// Reset the section to its pre-activation state. Called from
/// `crate::playlist::reset` on every playlist navigation so a new playlist
/// shows its own "Suggest songs" CTA instead of stale rows. UI thread.
pub fn reset(window: &AppWindow) {
    *SESSION.lock().unwrap() = Session::default();
    let state = window.global::<PlaylistSuggestionsState>();
    state.set_activated(false);
    state.set_loading(false);
    state.set_loading_more(false);
    state.set_has_more(false);
    state.set_is_empty(false);
    state.set_error("".into());
    state.set_rows(ModelRc::new(VecModel::from(Vec::<PlaylistSuggestionRow>::new())));
}
