// crates/qbz-app/src/playback_driver.rs — the headless playback orchestrator.
//
// This re-hosts, as a PURE decision function plus a thin IO shell, the playback
// bookkeeping that today only exists inside the desktop's 450 ms poll loop
// (`crates/qbz/src/playback.rs::start_poll_loop`): end-of-track detection,
// auto-advance, gapless pre-queue, stop-after, seamless-transition cursor sync
// and the periodic session-position save. Every branch below cites the exact
// desktop line it mirrors — the desktop is the reference for tie-breaks, the
// unit tests pin the observable contract (01-architecture.md §3.2).
//
// Split of concerns:
//   * `plan_tick`      — side-effect-free: (state, event, queue, error) → actions
//   * `advance_state`  — the pure state-update rule the shell applies each tick
//   * `next_playable`  — bounded unstreamable skip-walk (mirrors
//                        `playback.rs::advance_to_playable`, capped at
//                        `MAX_OFFLINE_SKIPS`)
//   * `run_driver`     — the 450 ms IO shell that reads the player, calls
//                        `plan_tick`, and executes the resulting actions
//   * `advance_and_play` — the full advance ritual (skip-walk → play → prefetch
//                        → persist), reused verbatim by the CLI next/prev routes

use std::sync::Arc;
use std::time::Duration;

use qbz_core::{FrontendAdapter, QbzCore};
use qbz_models::{Quality, QueueTrack, RepeatMode};
use qbz_player::PlaybackEvent;

use crate::session_store::{
    PersistedPlaybackSession, PersistedQueueTrack, PersistedSessionSnapshot,
    PersistedShellViewState,
};
use crate::shell::AppRuntime;

/// Poll cadence — the same 450 ms the desktop loop uses
/// (`playback.rs:4088`).
const TICK_MS: u64 = 450;

/// Session-position save cadence: every ~11 ticks ≈ 5 s
/// (`playback.rs:4306`, `save_pos_tick % 11 == 0`).
const SAVE_POSITION_EVERY_N_TICKS: u64 = 11;

/// QConnect report cadence while playing: every ~4 ticks ≈ 2 s
/// (`playback.rs:4069`, `QCONNECT_REPORT_EVERY_N_TICKS`).
const QCONNECT_REPORT_EVERY_N_TICKS: u64 = 4;

/// Bounded skip-walk ceiling for unavailable tracks (Tauri #467 parity;
/// `playback.rs:226`, `MAX_OFFLINE_SKIPS = 5`).
const MAX_OFFLINE_SKIPS: usize = 5;

/// A side effect the shell must perform this tick. Produced by [`plan_tick`],
/// executed by [`run_driver`]. Consumed by later tasks (T7 next/prev reuses the
/// advance ritual, T10 wires `ReportEdge`, T11 the settings reload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverAction {
    /// Reconcile the queue cursor to the id the engine is actually playing
    /// (a seamless gapless hand-off; `playback.rs:4340`).
    SyncCursorTo(u64),
    /// Pre-queue this upcoming track's bytes for a gapless transition
    /// (`playback.rs:4387`).
    ArmGapless(u64),
    /// The current track ended and there is a next playable track — run the
    /// full advance ritual (`playback.rs:4743`).
    AdvanceAndPlay,
    /// The ended track was stop-after-marked — halt (pause), never advance
    /// (`playback.rs:4720`).
    PauseStopAfter,
    /// Persist the live position (throttled ~5 s; `playback.rs:4307`).
    SavePosition(u64),
    /// Latch a drained stream-error message so `status` stays diagnosable
    /// (`playback.rs:4111`).
    LatchError(String),
    /// Emit an outbound QConnect renderer-state report on a transition or the
    /// ~2 s periodic cadence (`playback.rs:4648`).
    ReportEdge,
    /// The current track ended and nothing is playable — stop
    /// (`playback.rs:4751`).
    QueueFinished,
}

/// The previous tick's snapshot: the desktop loop's `last_track_id` /
/// `seen_position` / `was_playing` (plus duration for [`DriverState::after`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LastTick {
    pub track_id: u64,
    pub position: u64,
    pub duration: u64,
    pub is_playing: bool,
}

impl LastTick {
    fn from_event(ev: &PlaybackEvent) -> Self {
        Self {
            track_id: ev.track_id,
            position: ev.position,
            duration: ev.duration,
            is_playing: ev.is_playing,
        }
    }
}

/// The driver's carried-over state between ticks — the loop-local `mut`
/// variables of `start_poll_loop`, hoisted into a value so the decision is a
/// pure function of it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DriverState {
    /// Previous tick (`last_track_id`, `seen_position`, `was_playing`).
    pub last: LastTick,
    /// ~11-tick throttle counter for the periodic position save.
    pub save_pos_tick: u64,
    /// Track id an `ArmGapless` already fired for, so the ticker does not
    /// re-request it every tick (`gapless_requested_for`).
    pub gapless_requested_for: u64,
    /// ~4-tick throttle counter for the periodic QConnect report.
    pub report_tick: u64,
    /// Last track id we emitted a `ReportEdge` for (`last_reported_track_id`).
    pub last_reported_track_id: u64,
    /// Last play-state we emitted a `ReportEdge` for (`last_reported_playing`).
    pub last_reported_playing: bool,
}

impl DriverState {
    /// A state whose `last` snapshot (and report trackers) come from `ev` — the
    /// "the previous tick looked like this" constructor used by the tests and by
    /// the shell to seed a baseline.
    pub fn after(ev: &PlaybackEvent) -> DriverState {
        DriverState {
            last: LastTick::from_event(ev),
            save_pos_tick: 0,
            gapless_requested_for: 0,
            report_tick: 0,
            last_reported_track_id: ev.track_id,
            last_reported_playing: ev.is_playing,
        }
    }
}

/// The queue shape the decision needs, projected from `QbzCore::get_queue_state`
/// each tick: the current track id, the `(id, streamable)` upcoming list, the
/// repeat key and the stop-after marker.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueueSnapshot {
    pub current: u64,
    pub upcoming: Vec<(u64, bool)>,
    pub repeat: String,
    pub stop_after: Option<u64>,
    /// True when autoplay mode is "infinite" — logged-unsupported in P0 and
    /// treated as queue-finished (01 §3.1-5c).
    pub autoplay_infinite: bool,
}

/// Bounded skip-walk to the first streamable upcoming track. Returns
/// `(index, track_id)` of the first `playable == true` entry within `max_walk`
/// steps, or `None` when none is found inside the bound (never walks forever).
/// Mirrors `playback.rs::advance_to_playable`'s `MAX_OFFLINE_SKIPS` cap.
pub fn next_playable(upcoming: &[(u64, bool)], max_walk: usize) -> Option<(usize, u64)> {
    for (i, &(id, playable)) in upcoming.iter().enumerate() {
        if i >= max_walk {
            break;
        }
        if playable {
            return Some((i, id));
        }
    }
    None
}

/// The pure per-tick decision. Side-effect-free: given the carried state, the
/// live player event, the queue projection and any drained stream error, decide
/// which [`DriverAction`]s the shell must perform. Order mirrors the desktop
/// loop so the shell executes effects in the same sequence.
pub fn plan_tick(
    state: &DriverState,
    ev: &PlaybackEvent,
    queue: &QueueSnapshot,
    stream_error: Option<&str>,
) -> Vec<DriverAction> {
    let mut actions = Vec::new();
    let last = &state.last;

    // 1. Stream-error latch (playback.rs:4111): the player records a
    //    user-readable message drained exactly once per failure.
    if let Some(msg) = stream_error {
        actions.push(DriverAction::LatchError(msg.to_string()));
    }

    // 2. Periodic session-position save (playback.rs:4305-4308): ~11 ticks ≈ 5 s
    //    while a track is actually playing.
    let next_save_tick = state.save_pos_tick.wrapping_add(1);
    if ev.is_playing && ev.track_id != 0 && next_save_tick % SAVE_POSITION_EVERY_N_TICKS == 0 {
        actions.push(DriverAction::SavePosition(ev.position));
    }

    // 3. Seamless gapless transition (playback.rs:4324-4371): the engine advanced
    //    to a new track WITHOUT a stop (track-id change while still playing).
    //    Sync the cursor and STOP — the desktop `continue`s before every block
    //    below, so no other action fires this tick.
    let seamless_change = ev.track_id != 0
        && last.track_id != 0
        && ev.track_id != last.track_id
        && ev.is_playing
        && last.is_playing;
    if seamless_change {
        actions.push(DriverAction::SyncCursorTo(ev.track_id));
        return actions;
    }

    // 4. Gapless prefetch trigger (playback.rs:4387-4401): the engine wants the
    //    next track pre-queued and none is armed; arm the first playable upcoming
    //    exactly once per current track, suppressed when the current track is
    //    stop-after-marked (so it ends naturally and the marker can fire).
    if ev.gapless_ready
        && ev.gapless_next_track_id == 0
        && ev.track_id != 0
        && state.gapless_requested_for != ev.track_id
        && queue.stop_after != Some(ev.track_id)
    {
        if let Some(&(next_id, playable)) = queue.upcoming.first() {
            if next_id != ev.track_id && playable {
                actions.push(DriverAction::ArmGapless(next_id));
            }
        }
    }

    // 5. End-of-track edge (playback.rs:4489-4496): the previous tick was
    //    playing, this tick is not, the track id held (or went to 0), and the
    //    previous position was within 2 s of the (current) duration. Uses the
    //    live `ev.duration` guard + previous `last.position` exactly as the
    //    desktop's `duration > 0 && seen_position + 2 >= duration`.
    let track_ended = last.is_playing
        && !ev.is_playing
        && last.track_id != 0
        && (ev.track_id == 0 || ev.track_id == last.track_id)
        && ev.duration > 0
        && last.position + 2 >= ev.duration;

    // 6. QConnect report edge (playback.rs:4648-4673): report on a track/play
    //    transition OR the ~2 s periodic cadence while playing. Runs regardless
    //    of `track_ended` (the desktop report block precedes the advance block).
    let next_report_tick = state.report_tick.wrapping_add(1);
    if ev.track_id != 0 {
        let transition = ev.track_id != state.last_reported_track_id
            || ev.is_playing != state.last_reported_playing;
        let periodic = ev.is_playing && next_report_tick % QCONNECT_REPORT_EVERY_N_TICKS == 0;
        if transition || periodic {
            actions.push(DriverAction::ReportEdge);
        }
    }

    // 7. Track-end handling (playback.rs:4705-4762): stop-after HALTS (pause,
    //    never advance, ahead of any repeat/shuffle); else advance to the next
    //    playable track; else the queue is finished. repeat one/all always yield
    //    a next track (QueueManager owns the replay/wrap), so they never finish.
    if track_ended {
        if queue.current != 0 && queue.stop_after == Some(queue.current) {
            actions.push(DriverAction::PauseStopAfter);
        } else if queue.repeat == "one" || queue.repeat == "all" {
            actions.push(DriverAction::AdvanceAndPlay);
        } else if next_playable(&queue.upcoming, MAX_OFFLINE_SKIPS).is_some() {
            actions.push(DriverAction::AdvanceAndPlay);
        } else {
            actions.push(DriverAction::QueueFinished);
        }
    }

    actions
}

/// The pure state-update rule applied after [`plan_tick`] each tick. Replicates
/// how the desktop loop mutates its carried variables, branch by branch.
pub fn advance_state(
    prev: &DriverState,
    ev: &PlaybackEvent,
    actions: &[DriverAction],
) -> DriverState {
    let seamless = actions
        .iter()
        .any(|a| matches!(a, DriverAction::SyncCursorTo(_)));
    let armed = actions
        .iter()
        .any(|a| matches!(a, DriverAction::ArmGapless(_)));
    let ended = actions.iter().any(|a| {
        matches!(
            a,
            DriverAction::AdvanceAndPlay
                | DriverAction::PauseStopAfter
                | DriverAction::QueueFinished
        )
    });
    let reported = actions.iter().any(|a| matches!(a, DriverAction::ReportEdge));

    // save_pos_tick advances every tick — playback.rs:4305 runs before the
    // seamless `continue`.
    let save_pos_tick = prev.save_pos_tick.wrapping_add(1);

    if seamless {
        // Seamless branch (playback.rs:4363-4369): last <- ev, gapless guard
        // cleared, report trackers untouched (the `continue` precedes the report
        // block, so report_tick does NOT advance this tick).
        return DriverState {
            last: LastTick::from_event(ev),
            save_pos_tick,
            gapless_requested_for: 0,
            report_tick: prev.report_tick,
            last_reported_track_id: prev.last_reported_track_id,
            last_reported_playing: prev.last_reported_playing,
        };
    }

    // Non-seamless: the report block runs (playback.rs:4648) so report_tick
    // advances; the report trackers move only when a ReportEdge fired.
    let report_tick = prev.report_tick.wrapping_add(1);
    let (last_reported_track_id, last_reported_playing) = if reported {
        (ev.track_id, ev.is_playing)
    } else {
        (prev.last_reported_track_id, prev.last_reported_playing)
    };

    // Edge trackers (playback.rs:4676-4700): last_track_id/seen_position update
    // only when track_id != 0; was_playing tracks is_playing unconditionally.
    let mut last = if ev.track_id != 0 {
        LastTick::from_event(ev)
    } else {
        LastTick {
            track_id: prev.last.track_id,
            position: prev.last.position,
            duration: prev.last.duration,
            is_playing: ev.is_playing,
        }
    };
    let mut gapless_requested_for = if armed {
        ev.track_id
    } else {
        prev.gapless_requested_for
    };

    // Track-end handler resets the edge trackers + the gapless guard
    // (playback.rs:4728-4742, both the stop-after and advance branches).
    if ended {
        last = LastTick::default();
        gapless_requested_for = 0;
    }

    DriverState {
        last,
        save_pos_tick,
        gapless_requested_for,
        report_tick,
        last_reported_track_id,
        last_reported_playing,
    }
}

/// Map the desktop `ui_prefs.streaming_quality` key to a request-layer
/// [`Quality`]. Byte-identical contract to `crates/qbz/src/ui_prefs.rs:823`
/// (`streaming_quality_for_key`), replicated here because the desktop crate is
/// out of the daemon's dependency graph. Unknown/unset keys fall back to the
/// top tier so hi-res never silently downgrades (01 §3.1).
pub fn quality_from_key(key: &str) -> Quality {
    match key {
        "mp3" => Quality::Mp3,
        "cd" => Quality::Lossless,
        "hires" => Quality::HiRes,
        _ => Quality::UltraHiRes, // "hires_plus" + unknown keys
    }
}

// ─────────────────────────── IO shell ───────────────────────────

/// Host-supplied side channels the shell drives on each relevant action. Kept
/// as trait objects so qbzd can wire daemon-shared latching / tick timestamping
/// / the QConnect report signal without this module depending on qbzd.
#[derive(Clone)]
pub struct DriverDeps {
    /// Resolve the streaming quality at play time (qbzd passes the daemon prefs).
    pub quality: Arc<dyn Fn() -> Quality + Send + Sync>,
    /// Report-edge signal (T10 wires the QConnect renderer report).
    pub on_edge: Arc<dyn Fn() + Send + Sync>,
    /// Latch a drained error under a category ("stream" | "transport" | "auth").
    pub on_latch: Arc<dyn Fn(&str, String) + Send + Sync>,
    /// Called at the end of every tick (qbzd timestamps `driver_last_tick`).
    pub on_tick: Arc<dyn Fn() + Send + Sync>,
}

/// The 450 ms IO shell. Each tick: read the player event, drain the stream-error
/// latch, project the queue, `plan_tick`, execute the actions, then
/// `advance_state`. Breaks when `shutdown` flips to `true`; the loop is thin by
/// design (01 §3.2 — too thin to hide bugs). Runs safely from boot regardless of
/// auth: with no session the queue is empty and every tick is a near-no-op.
pub async fn run_driver<A: FrontendAdapter + Send + Sync + 'static>(
    runtime: Arc<AppRuntime<A>>,
    deps: DriverDeps,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut state = DriverState::default();
    let mut ticker = tokio::time::interval(Duration::from_millis(TICK_MS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
                continue;
            }
        }

        let core = runtime.core();
        let player = core.player();
        let ev = player.get_playback_event();
        // Drain-once stream-error message (playback.rs:4111).
        let stream_error = player.state.take_stream_error_message();
        let queue = queue_snapshot(core).await;

        let actions = plan_tick(&state, &ev, &queue, stream_error.as_deref());

        for action in &actions {
            match action {
                DriverAction::SyncCursorTo(id) => {
                    core.sync_current_to_id(*id).await;
                }
                DriverAction::ArmGapless(id) => {
                    let quality = (deps.quality)();
                    if let Some(bytes) =
                        core.fetch_for_gapless_resolved(*id, quality, None, None).await
                    {
                        if let Err(e) = player.play_next(bytes, *id) {
                            log::warn!("[qbzd] driver: gapless play_next failed: {e}");
                        }
                    }
                }
                DriverAction::PauseStopAfter => {
                    // The ended track is the queue's current track (playback.rs:4708).
                    let finished = queue.current;
                    if finished != 0 && core.consume_stop_after_if(finished).await {
                        if let Err(e) = core.pause() {
                            log::warn!("[qbzd] driver: stop-after pause failed: {e}");
                        }
                    } else {
                        // Marker cleared between the snapshot and the consume — fall
                        // through to the normal advance (desktop parity: stop-after
                        // and advance share one track-end block).
                        advance_and_play_logged(&runtime, (deps.quality)()).await;
                    }
                }
                DriverAction::AdvanceAndPlay => {
                    advance_and_play_logged(&runtime, (deps.quality)()).await;
                }
                DriverAction::SavePosition(p) => {
                    runtime.with_session_store(|s| {
                        if let Err(e) = s.save_position(*p) {
                            log::debug!("[qbzd] driver: save_position failed: {e}");
                        }
                    });
                }
                DriverAction::LatchError(m) => {
                    (deps.on_latch)("stream", m.clone());
                }
                DriverAction::ReportEdge => {
                    (deps.on_edge)();
                }
                DriverAction::QueueFinished => {
                    if queue.autoplay_infinite {
                        log::info!(
                            "[qbzd] driver: autoplay 'infinite' unsupported on qbzd v1, \
                             treated as queue-finished"
                        );
                    }
                    log::info!("[qbzd] driver: queue finished");
                    if let Err(e) = core.stop() {
                        log::warn!("[qbzd] driver: stop on queue-finished failed: {e}");
                    }
                }
            }
        }

        state = advance_state(&state, &ev, &actions);
        (deps.on_tick)();
    }
    log::info!("[qbzd] driver: shutting down");
}

/// Run the advance ritual and log its outcome (queue-finished on `Ok(None)`, the
/// error on `Err`). The daemon stops on a genuine queue edge just like the
/// pure-decision `QueueFinished` branch does. Always forward — the driver's
/// auto-advance on track-end never walks backward (reverse is the `qbzd prev`
/// route's concern, wired directly through `advance_and_play(..., false)`).
async fn advance_and_play_logged<A: FrontendAdapter + Send + Sync + 'static>(
    runtime: &Arc<AppRuntime<A>>,
    quality: Quality,
) {
    match advance_and_play(runtime, quality, true).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            log::info!("[qbzd] driver: advance found nothing playable — queue finished");
            if let Err(e) = runtime.core().stop() {
                log::warn!("[qbzd] driver: stop after empty advance failed: {e}");
            }
        }
        Err(e) => log::warn!("[qbzd] driver: advance failed: {e}"),
    }
}

/// The FULL advance ritual, reused verbatim by the CLI next/prev routes (T7):
/// bounded skip-walk to the next (or previous) playable track → play it → warm
/// the successors for gapless → persist the session. Never a bare cursor move
/// (02 §2.2). The skip-walk mirrors `playback.rs::advance_to_playable` (capped
/// at `MAX_OFFLINE_SKIPS`); `next_track()`/`previous_track()` are the atomic
/// cursor movers — `forward` selects which one, exactly like the desktop's
/// `advance_to_playable(runtime, weak, forward)` (`crates/qbz/src/playback.rs:358`),
/// so `qbzd next` and `qbzd prev` share this one ritual instead of duplicating
/// the play → prefetch → persist tail.
pub async fn advance_and_play<A: FrontendAdapter + Send + Sync + 'static>(
    runtime: &AppRuntime<A>,
    quality: Quality,
    forward: bool,
) -> Result<Option<QueueTrack>, String> {
    let core = runtime.core();
    let mut skips = 0usize;
    let next = loop {
        let step = if forward {
            core.next_track().await
        } else {
            core.previous_track().await
        };
        let Some(track) = step else {
            break None; // queue edge
        };
        // Playable gate: local files always attempt (they play from disk);
        // streamable Qobuz tracks OK. Daemon P0 has no offline tier, so an
        // unstreamable remote track is a skip (mirrors advance_to_playable).
        if track.streamable || track.is_local {
            break Some(track);
        }
        skips += 1;
        log::info!("[qbzd] driver: skipping unavailable track {} ({skips}/{MAX_OFFLINE_SKIPS})", track.id);
        if skips >= MAX_OFFLINE_SKIPS {
            let _ = core.stop();
            break None;
        }
    };
    let Some(track) = next else {
        return Ok(None);
    };
    let track_id = track.id;
    core.play_track_resolved(track_id, quality, None, None, 0)
        .await?;
    // Warm the successors so the next transition can be gapless (best-effort).
    prefetch_successors(runtime, quality).await;
    // Persist the session (queue + current + position) so a restart resumes.
    save_session_now(runtime).await;
    Ok(Some(track))
}

/// Warm the player cache for the next upcoming track (best-effort; failures are
/// logged, never fatal). Mirrors `playback.rs::kick_prefetch` for the daemon:
/// only remote, non-local, not-already-cached tracks are fetched.
async fn prefetch_successors<A: FrontendAdapter + Send + Sync + 'static>(
    runtime: &AppRuntime<A>,
    quality: Quality,
) {
    let core = runtime.core();
    let upcoming = core.peek_upcoming(1).await;
    let Some(next) = upcoming.into_iter().next() else {
        return;
    };
    if next.is_local {
        return;
    }
    let player = core.player();
    if player.is_track_cached(next.id) {
        return;
    }
    let client_lock = core.client();
    let guard = client_lock.read().await;
    let Some(client) = guard.as_ref() else {
        return;
    };
    if let Err(e) = player.prefetch_into_cache(client, next.id, quality).await {
        log::debug!("[qbzd] driver: prefetch track {} failed: {e}", next.id);
    }
}

/// Project the live queue into the decision's [`QueueSnapshot`].
async fn queue_snapshot<A: FrontendAdapter + Send + Sync + 'static>(
    core: &QbzCore<A>,
) -> QueueSnapshot {
    let state = core.get_queue_state_full().await;
    let current = state.current_track.as_ref().map(|t| t.id).unwrap_or(0);
    let upcoming = state
        .upcoming
        .iter()
        .map(|t| (t.id, t.streamable || t.is_local))
        .collect();
    QueueSnapshot {
        current,
        upcoming,
        repeat: repeat_to_str(state.repeat).to_string(),
        stop_after: state.stop_after_track_id,
        // Autoplay "infinite" is a later-task wiring; P0 never sets it.
        autoplay_infinite: false,
    }
}

// ───────────────────── session persistence (daemon) ─────────────────────

/// Capture the live queue + playback state and persist it via the active
/// session store. No-op when no session is active (`with_session_store` returns
/// `None`). Mirrors `crates/qbz/src/session_persist.rs::capture_and_save`, minus
/// the desktop-only `persist_session` gate (the daemon's store IS its queue
/// persistence, so it always saves).
pub async fn save_session_now<A: FrontendAdapter + Send + Sync + 'static>(
    runtime: &AppRuntime<A>,
) {
    let core = runtime.core();
    let (tracks, current_index) = core.get_all_queue_tracks().await;
    let full = core.get_queue_state_full().await;
    let ev = core.player().get_playback_event();
    let snapshot = PersistedSessionSnapshot {
        playback: PersistedPlaybackSession {
            queue_tracks: tracks.iter().map(to_persisted).collect(),
            current_index,
            current_position_secs: ev.position,
            volume: ev.volume,
            shuffle_enabled: full.shuffle,
            repeat_mode: repeat_to_str(full.repeat).to_string(),
            was_playing: ev.is_playing,
            saved_at: 0, // set inside save_session
        },
        // Shell-view columns are desktop-only; keep defaults so the schema
        // round-trips unchanged.
        shell_view: PersistedShellViewState::default(),
    };
    let saved = runtime.with_session_store(|s| s.save_session(&snapshot));
    match saved {
        Some(Ok(())) => {}
        Some(Err(e)) => log::warn!("[qbzd] driver: session save failed: {e}"),
        None => log::debug!("[qbzd] driver: session save skipped (no active session)"),
    }
}

/// Restore the persisted queue at boot, PAUSED (queue + order + repeat + volume,
/// never auto-playing). Returns `true` when a non-empty queue was restored.
/// Mirrors `session_persist::restore`'s Phase A; the daemon has no
/// `resume_playback_position` gate, so the saved position is threaded into the
/// snapshot but only replayed when the CLI later plays the restored track.
pub async fn restore_session_paused<A: FrontendAdapter + Send + Sync + 'static>(
    runtime: &AppRuntime<A>,
) -> bool {
    let Some(loaded) = runtime.with_session_store(|s| s.load_session()) else {
        return false; // no active session
    };
    let snapshot = match loaded {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[qbzd] driver: session load failed: {e}");
            return false;
        }
    };
    let pb = snapshot.playback;
    if pb.queue_tracks.is_empty() {
        log::info!("[qbzd] driver: nothing to restore (saved queue is empty)");
        return false;
    }
    let count = pb.queue_tracks.len();
    let index = pb.current_index;
    let position = pb.current_position_secs;
    let tracks: Vec<QueueTrack> = pb.queue_tracks.into_iter().map(from_persisted).collect();
    let core = runtime.core();
    core.set_queue_with_order(tracks, index, pb.shuffle_enabled, None)
        .await;
    core.set_repeat_mode(repeat_from_str(&pb.repeat_mode)).await;
    let _ = core.set_volume(pb.volume);
    log::info!(
        "[qbzd] driver: restored {count} queue tracks (index {index:?}), paused; \
         saved position {position}s"
    );
    true
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
        version: None,
        artist: t.artist,
        album: t.album,
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
        context_kind: None,
        context_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `PlaybackEvent` builder: the four fields the driver reasons about
    /// plus Default-equivalents for the rest (gapless fields off, no stream meta).
    fn ev(track: u64, playing: bool, pos: u64, dur: u64) -> PlaybackEvent {
        PlaybackEvent {
            is_playing: playing,
            position: pos,
            duration: dur,
            track_id: track,
            volume: 1.0,
            sample_rate: None,
            bit_depth: None,
            shuffle: None,
            repeat: None,
            normalization_gain: None,
            gapless_ready: false,
            gapless_next_track_id: 0,
            bit_perfect_mode: None,
            buffer_progress: None,
        }
    }

    /// Queue-shape builder: current track id, `(id, streamable)` upcoming list,
    /// repeat key ("off"|"all"|"one"), optional stop-after marker.
    fn q(
        current: u64,
        upcoming: &[(u64, bool)],
        repeat: &str,
        stop_after: Option<u64>,
    ) -> QueueSnapshot {
        QueueSnapshot {
            current,
            upcoming: upcoming.to_vec(),
            repeat: repeat.to_string(),
            stop_after,
            autoplay_infinite: false,
        }
    }

    #[test]
    fn end_edge_advances() {
        let s = DriverState::after(&ev(1, true, 580, 581));
        let a = plan_tick(&s, &ev(1, false, 581, 581), &q(1, &[(2, true)], "off", None), None);
        assert!(a.contains(&DriverAction::AdvanceAndPlay));
        assert!(a.contains(&DriverAction::ReportEdge)); // play-state edge
    }

    #[test]
    fn mid_track_pause_does_not_advance() {
        let s = DriverState::after(&ev(1, true, 100, 581));
        let a = plan_tick(&s, &ev(1, false, 100, 581), &q(1, &[(2, true)], "off", None), None);
        assert!(!a.contains(&DriverAction::AdvanceAndPlay));
        assert!(a.contains(&DriverAction::ReportEdge));
    }

    #[test]
    fn stop_after_one_shot() {
        let s = DriverState::after(&ev(42, true, 580, 581));
        let a = plan_tick(
            &s,
            &ev(42, false, 581, 581),
            &q(42, &[(2, true)], "off", Some(42)),
            None,
        );
        assert!(a.contains(&DriverAction::PauseStopAfter));
        assert!(!a.contains(&DriverAction::AdvanceAndPlay));
    }

    #[test]
    fn gapless_arms_exactly_once() {
        let mut e = ev(1, true, 300, 581);
        e.gapless_ready = true;
        e.gapless_next_track_id = 0;
        let s = DriverState::after(&ev(1, true, 299, 581));
        let queue = q(1, &[(2, true)], "off", None);
        let a1 = plan_tick(&s, &e, &queue, None);
        assert!(a1.contains(&DriverAction::ArmGapless(2)));
        let s2 = advance_state(&s, &e, &a1);
        let a2 = plan_tick(&s2, &e, &queue, None);
        assert!(!a2.iter().any(|x| matches!(x, DriverAction::ArmGapless(_))));
    }

    #[test]
    fn repeat_one_advances_instead_of_finishing() {
        let s = DriverState::after(&ev(1, true, 580, 581));
        let a = plan_tick(&s, &ev(1, false, 581, 581), &q(1, &[], "one", None), None);
        assert!(a.contains(&DriverAction::AdvanceAndPlay));
        assert!(!a.contains(&DriverAction::QueueFinished));
    }

    #[test]
    fn queue_finished_when_nothing_playable() {
        let s = DriverState::after(&ev(1, true, 580, 581));
        let a = plan_tick(&s, &ev(1, false, 581, 581), &q(1, &[(2, false)], "off", None), None);
        assert!(a.contains(&DriverAction::QueueFinished));
    }

    #[test]
    fn skip_walk_bounds() {
        assert_eq!(next_playable(&[(2, false), (3, false), (4, true)], 50), Some((2, 4)));
        let all_bad: Vec<(u64, bool)> = (0..60).map(|i| (i, false)).collect();
        assert_eq!(next_playable(&all_bad, 50), None); // bounded — never walks forever
    }

    #[test]
    fn position_save_cadence_11_ticks() {
        let mut s = DriverState::after(&ev(1, true, 10, 581));
        for tick in 1..=11u32 {
            let e = ev(1, true, 10 + tick as u64, 581);
            let a = plan_tick(&s, &e, &q(1, &[], "off", None), None);
            if tick == 11 {
                assert!(a.contains(&DriverAction::SavePosition(21)));
            } else {
                assert!(!a.iter().any(|x| matches!(x, DriverAction::SavePosition(_))));
            }
            s = advance_state(&s, &e, &a);
        }
    }

    #[test]
    fn seamless_gapless_transition_syncs_cursor() {
        let s = DriverState::after(&ev(1, true, 580, 581));
        let a = plan_tick(&s, &ev(2, true, 0, 547), &q(1, &[(2, true)], "off", None), None);
        assert!(a.contains(&DriverAction::SyncCursorTo(2)));
    }

    #[test]
    fn stream_error_latches() {
        let s = DriverState::after(&ev(1, true, 10, 581));
        let a = plan_tick(
            &s,
            &ev(1, false, 10, 581),
            &q(1, &[], "off", None),
            Some("ALSA device disappeared"),
        );
        assert!(a.contains(&DriverAction::LatchError("ALSA device disappeared".into())));
    }

    #[test]
    fn duration_zero_never_advances() {
        let s = DriverState::after(&ev(1, true, 580, 581));
        let a = plan_tick(&s, &ev(1, false, 580, 0), &q(1, &[(2, true)], "off", None), None);
        assert!(!a.contains(&DriverAction::AdvanceAndPlay));
        assert!(a.contains(&DriverAction::ReportEdge)); // play-state edge
    }
}
