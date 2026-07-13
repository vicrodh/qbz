//! Session persistence (queue + playback state) — the Slint wiring for the
//! `persist_session` / `resume_playback_position` playback preferences.
//!
//! The on-disk store is the frontend-agnostic [`SessionStore`] in
//! `qbz-app` (a per-user `session.db`, already built + tested). This module is
//! the thin frontend glue: it owns a process-global store handle (bound at
//! session activation), captures the live queue + playback state into the
//! persisted snapshot at meaningful edges, and restores it at startup.
//!
//! Gating: `persist_session` (restore the queue/session) and
//! `resume_playback_position` (also restore the exact position) are per-user
//! playback preferences. Both are cached here so the hot save path never reopens
//! the prefs DB; [`set_gates`] refreshes the cache when the toggles change, and
//! [`init_for_user`] seeds them synchronously when the store opens (no race with
//! the async settings snapshot load).
//!
//! Phase A (this module) restores the queue + current track PAUSED — it touches
//! NO protected-audio code beyond threading an existing `start_position_secs`.
//! The saved position rides along via [`take_resume_for`] and is consumed on the
//! first play of the restored track, reusing the player's session-resume offset.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use qbz_app::session_store::{
    PersistedPlaybackSession, PersistedQueueTrack, PersistedSessionSnapshot,
    PersistedShellViewState, SessionStore,
};
use qbz_app::settings::playback::PlaybackPreferencesStore;
use qbz_app::shell::AppRuntime;
use qbz_models::{QueueTrack, RepeatMode};

use crate::adapter::SlintAdapter;

type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Per-user session store, bound at activation (None before login / on failure).
static STORE: Mutex<Option<SessionStore>> = Mutex::new(None);
/// Cached `persist_session` gate — captured/restored only when true.
static PERSIST_SESSION: AtomicBool = AtomicBool::new(false);
/// Cached `resume_playback_position` gate — primes [`PENDING_RESUME`] at restore.
static RESUME_POSITION: AtomicBool = AtomicBool::new(false);
/// Position (secs) to resume the restored current track at on the first play.
/// 0 = none. Written at restore (when resume is on); consumed on first play.
static PENDING_RESUME: AtomicU64 = AtomicU64::new(0);
/// Track id the pending resume position applies to, so ONLY the restored current
/// track resumes — playing any other track first starts from 0. 0 = none.
static PENDING_RESUME_TRACK: AtomicU64 = AtomicU64::new(0);
/// Runtime + tokio handle captured at shell entry, so the synchronous window
/// close handlers can flush a final full snapshot before the loop quits.
static EXIT_CTX: OnceLock<(Runtime, tokio::runtime::Handle)> = OnceLock::new();

/// Bind the runtime + tokio handle for [`save_on_exit`]. Called once at shell
/// entry (idempotent — later calls are ignored by `OnceLock`).
pub fn bind_exit_ctx(runtime: Runtime, handle: tokio::runtime::Handle) {
    let _ = EXIT_CTX.set((runtime, handle));
}

/// Flush a final full snapshot synchronously (the window close handlers run on
/// the UI thread, off the tokio runtime, so we `block_on`). No-op until the exit
/// context is bound or unless `persist_session` is on.
pub fn save_on_exit() {
    if !persist_enabled() {
        return;
    }
    if let Some((runtime, handle)) = EXIT_CTX.get() {
        handle.block_on(capture_and_save(runtime));
    }
}

/// Open the per-user session store and seed the gate flags synchronously from
/// the playback preferences. Called at session activation alongside the other
/// `init_for_user` stores. Failures degrade to "no persistence" (logged).
pub fn init_for_user(base_dir: &Path) {
    let opened = match SessionStore::new_at(base_dir) {
        Ok(store) => {
            *STORE.lock().unwrap() = Some(store);
            true
        }
        Err(e) => {
            log::warn!("[qbz-slint] session_persist: open failed: {e}");
            *STORE.lock().unwrap() = None;
            false
        }
    };
    // Seed the gates from the per-user playback prefs so capture/restore work
    // before the async settings snapshot has had a chance to call set_gates.
    match PlaybackPreferencesStore::new_at(base_dir).and_then(|s| s.get_preferences()) {
        Ok(prefs) => set_gates(prefs.persist_session, prefs.resume_playback_position),
        Err(e) => {
            log::warn!("[qbz-slint] session_persist: prefs read failed, gates off: {e}");
            set_gates(false, false);
        }
    }
    log::info!(
        "[qbz-slint] session_persist: init at {} (store_open={opened}, persist={}, resume={})",
        base_dir.display(),
        PERSIST_SESSION.load(Ordering::Relaxed),
        RESUME_POSITION.load(Ordering::Relaxed)
    );
}

/// Refresh the cached gate flags (called by the Settings toggle handlers and the
/// settings snapshot load so the cache tracks live preference changes).
pub fn set_gates(persist_session: bool, resume_position: bool) {
    PERSIST_SESSION.store(persist_session, Ordering::Relaxed);
    RESUME_POSITION.store(resume_position, Ordering::Relaxed);
}

/// Whether session persistence is currently enabled.
pub fn persist_enabled() -> bool {
    PERSIST_SESSION.load(Ordering::Relaxed)
}

/// Take + clear the pending resume position IF it applies to `track_id` (the
/// restored current track). Returns the saved position once, then 0 forever —
/// and 0 for any other track, so playing something else first never resumes.
pub fn take_resume_for(track_id: u64) -> u64 {
    if track_id != 0 && PENDING_RESUME_TRACK.swap(0, Ordering::Relaxed) == track_id {
        PENDING_RESUME.swap(0, Ordering::Relaxed)
    } else {
        0
    }
}

/// Peek the pending resume position WITHOUT consuming it (so the seek bar can be
/// seeded at restore while the actual resume still fires on first play). 0 = none.
pub fn pending_resume_position() -> u64 {
    if PENDING_RESUME_TRACK.load(Ordering::Relaxed) != 0 {
        PENDING_RESUME.load(Ordering::Relaxed)
    } else {
        0
    }
}

fn repeat_to_str(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "off",
        RepeatMode::All => "all",
        RepeatMode::One => "one",
    }
}

fn repeat_from_str(s: &str) -> RepeatMode {
    match s {
        "all" => RepeatMode::All,
        "one" => RepeatMode::One,
        _ => RepeatMode::Off,
    }
}

fn to_persisted(t: &QueueTrack) -> PersistedQueueTrack {
    PersistedQueueTrack {
        id: t.id,
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        duration_secs: t.duration_secs,
        artwork_url: t.artwork_url.clone(),
        hires: t.hires,
        bit_depth: t.bit_depth,
        sample_rate: t.sample_rate,
        is_local: t.is_local,
        album_id: t.album_id.clone(),
        artist_id: t.artist_id,
        streamable: t.streamable,
        source: t.source.clone(),
        parental_warning: t.parental_warning,
        source_item_id_hint: t.source_item_id_hint.clone(),
    }
}

fn from_persisted(t: PersistedQueueTrack) -> QueueTrack {
    QueueTrack {
        id: t.id,
        title: t.title,
        // The persisted schema predates `version` (Tauri parity): the edition
        // subtitle is not stored, so a restored track simply has no version.
        version: None,
        artist: t.artist,
        album: t.album,
        // Album-version is cosmetic (now-playing/MPRIS); not persisted in the
        // session schema, so a restored track shows the clean album until the
        // next album-play repopulates it.
        album_version: None,
        duration_secs: t.duration_secs,
        artwork_url: t.artwork_url,
        hires: t.hires,
        bit_depth: t.bit_depth,
        sample_rate: t.sample_rate,
        is_local: t.is_local,
        album_id: t.album_id,
        artist_id: t.artist_id,
        streamable: t.streamable,
        source: t.source,
        parental_warning: t.parental_warning,
        source_item_id_hint: t.source_item_id_hint,
        // Not persisted in the session schema — a restored track carries no
        // container origin, so the "playing from" button falls back to the
        // track's own album until the next container play re-stamps the queue.
        context_kind: None,
        context_id: None,
    }
}

/// Capture the live queue + playback state and persist it. No-op unless
/// `persist_session` is on and the store is open. Async (reads the queue lock).
pub async fn capture_and_save(runtime: &Runtime) {
    if !persist_enabled() {
        return;
    }
    let (tracks, current_index) = runtime.core().get_all_queue_tracks().await;
    // Crash-chain level >=3 bypassed the restore this boot, so the queue on
    // disk is the GOOD copy the user wants back on a healthy start — don't
    // clobber it with this session's empty queue at exit. A queue the user
    // actually built during the recovered boot still saves normally.
    if tracks.is_empty() && crate::crash_chain_level() >= 3 {
        log::info!(
            "[qbz-slint] session_persist: crash-chain recovery boot with empty queue — \
             keeping the preserved snapshot on disk"
        );
        return;
    }
    let full = runtime.core().get_queue_state_full().await;
    let pb = runtime.core().get_playback_state();
    let snapshot = PersistedSessionSnapshot {
        playback: PersistedPlaybackSession {
            queue_tracks: tracks.iter().map(to_persisted).collect(),
            current_index,
            current_position_secs: pb.position,
            volume: pb.volume,
            shuffle_enabled: full.shuffle,
            repeat_mode: repeat_to_str(full.repeat).to_string(),
            was_playing: pb.is_playing,
            saved_at: 0, // set inside save_session
        },
        // Shell-view restoration is handled separately (ui_prefs startup_page);
        // keep the Tauri view columns at their defaults so the schema round-trips.
        shell_view: PersistedShellViewState::default(),
    };
    let track_count = snapshot.playback.queue_tracks.len();
    if let Some(store) = STORE.lock().unwrap().as_ref() {
        match store.save_session(&snapshot) {
            Ok(()) => log::info!(
                "[qbz-slint] session_persist: saved {track_count} queue tracks (pos {}s, playing {})",
                snapshot.playback.current_position_secs,
                snapshot.playback.was_playing
            ),
            Err(e) => log::warn!("[qbz-slint] session_persist: save failed: {e}"),
        }
    } else {
        log::warn!("[qbz-slint] session_persist: capture skipped (store not open)");
    }
}

/// Quick position-only save (a single cheap UPDATE) — for the poll loop's
/// throttled tick and the pause edge, so a crash keeps a near-current position.
pub fn save_position(position_secs: u64) {
    if !persist_enabled() {
        return;
    }
    if let Some(store) = STORE.lock().unwrap().as_ref() {
        let _ = store.save_position(position_secs);
    }
}

/// Restore the persisted queue at startup. Returns true if a non-empty queue was
/// restored (so the caller refreshes the now-playing bar). Restores PAUSED; when
/// `resume_playback_position` is on, primes [`PENDING_RESUME`] for Phase B.
pub async fn restore(runtime: &Runtime) -> bool {
    if !persist_enabled() {
        log::info!("[qbz-slint] session_persist: restore skipped (persist_session off)");
        return false;
    }
    let snapshot = {
        let guard = STORE.lock().unwrap();
        let Some(store) = guard.as_ref() else {
            log::warn!("[qbz-slint] session_persist: restore skipped (store not open)");
            return false;
        };
        match store.load_session() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[qbz-slint] session_persist: load failed: {e}");
                return false;
            }
        }
    };
    let pb_sess = snapshot.playback;
    if pb_sess.queue_tracks.is_empty() {
        log::info!("[qbz-slint] session_persist: nothing to restore (saved queue is empty)");
        return false;
    }
    let position = pb_sess.current_position_secs;
    let count = pb_sess.queue_tracks.len();
    let index = pb_sess.current_index;
    let tracks: Vec<QueueTrack> = pb_sess
        .queue_tracks
        .into_iter()
        .map(from_persisted)
        .collect();
    // The current track's id, so the resume position is applied ONLY when this
    // exact track is the first one played after the restore.
    let current_track_id = index.and_then(|i| tracks.get(i)).map(|t| t.id).unwrap_or(0);
    runtime
        .core()
        .set_queue_with_order(tracks, index, pb_sess.shuffle_enabled, None)
        .await;
    runtime
        .core()
        .set_repeat_mode(repeat_from_str(&pb_sess.repeat_mode))
        .await;
    // The queue session carries the authoritative last volume; apply it to the
    // player (the slider also seeds from ui_prefs, but this keeps them in step).
    let _ = runtime.core().set_volume(pb_sess.volume);
    if RESUME_POSITION.load(Ordering::Relaxed) && position > 0 && current_track_id != 0 {
        PENDING_RESUME.store(position, Ordering::Relaxed);
        PENDING_RESUME_TRACK.store(current_track_id, Ordering::Relaxed);
    }
    log::info!(
        "[qbz-slint] session_persist: restored {count} queue tracks (index {index:?}), paused; \
         resume position {position}s (consumed on first play when enabled)"
    );
    true
}
