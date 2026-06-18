//! S4 lyrics sync engine — drives `LyricsState.active-index` /
//! `line-progress` from the playback position (spec §4).
//!
//! # Tick / push design
//!
//! ONE `slint::Timer` on the UI thread, started once for the app lifetime,
//! with an ADAPTIVE cadence:
//!
//! - **active ~30Hz (33ms)** while the gate is open: `lyrics panel open &&
//!   doc synced+ready && effectively playing` (1:1 to Tauri's rAF gate,
//!   `+page.svelte:6145-6157`);
//! - **idle 250ms** otherwise — the tick then only re-checks the gate (a few
//!   property reads; something must observe the gate since Slint global
//!   properties have no Rust-side reactivity. Tauri leans on Svelte
//!   subscriptions for the same job).
//!
//! Every active tick recomputes the active index + karaoke fraction from the
//! ABSOLUTE position (binary search over `CURRENT_DOC` — `qbz_lyrics::sync`),
//! which makes seeks inherently safe: no smoothing ever crosses a
//! discontinuity because the engine zeroes `fill-anim-ms` whenever the index
//! changes or the fraction moves backward. Pause closes the gate → the last
//! pushed state freezes in place (parity). Pushes are equality-guarded so an
//! unchanged tick writes nothing.
//!
//! The 450ms playback poll loop is NOT touched (load-bearing for QConnect
//! report cadence + gapless edges); this timer is a parallel ms-precision
//! read used only by lyrics.
//!
//! # Position resolver (Q7 — lyrics FOLLOW the QConnect peer)
//!
//! - **Local playback**: `SharedState::current_position_ms()` — the
//!   read-only ms getter on `qbz-player` (Q1; pure derivation from the
//!   existing `playback_start_millis`/`position_at_start` anchors).
//! - **Peer renderer active (controller mode)**: the poll loop's controller
//!   branch publishes the RAW renderer snapshot anchor here every ~450ms
//!   ([`publish_remote_anchor`]); each engine tick extrapolates
//!   `position_ms + (now - updated_at_ms)` while playing — the exact
//!   controller-branch trick (`playback.rs` peer-position push). Tauri's
//!   lyrics freeze under a peer; this is the documented D7 improvement.
//!
//! [`kick`] runs one immediate pass that ignores the playing gate: called on
//! doc commit and panel open so the ladder lands on the correct line
//! instantly, even while paused (Tauri computes once on load).

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode};

use qbz_app::shell::AppRuntime;

use crate::adapter::SlintAdapter;
use crate::{AppWindow, LyricsState, ShellState};

type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Active cadence ~30Hz — within Tauri's effective granularity class; the
/// clip-width animation interpolates between ticks (spec §4.1).
const ACTIVE_TICK: Duration = Duration::from_millis(33);
/// Idle cadence: gate polling only.
const IDLE_TICK: Duration = Duration::from_millis(250);
/// Clip-width animation duration while progress is continuous — slightly
/// above the tick interval so a late tick doesn't stutter the fill.
const FILL_ANIM_MS: i32 = 45;

// ---- QConnect remote position anchor (Q7) ----------------------------------
// Raw renderer snapshot published by the poll loop's controller branch
// (~450ms); the engine extrapolates between publishes. Plain atomics — both
// sides touch them at low rates and a torn read across fields self-corrects
// on the next tick.
static REMOTE_ACTIVE: AtomicBool = AtomicBool::new(false);
static REMOTE_POSITION_MS: AtomicU64 = AtomicU64::new(0);
static REMOTE_UPDATED_AT_MS: AtomicU64 = AtomicU64::new(0);
static REMOTE_PLAYING: AtomicBool = AtomicBool::new(false);

/// Publish the RAW peer-renderer anchor (NOT the already-extrapolated
/// position — the engine extrapolates itself at tick time). Called from the
/// poll loop's controller branch on every tick while a peer owns playback.
pub fn publish_remote_anchor(position_ms: u64, updated_at_ms: u64, playing: bool) {
    REMOTE_POSITION_MS.store(position_ms, Ordering::Relaxed);
    REMOTE_UPDATED_AT_MS.store(updated_at_ms, Ordering::Relaxed);
    REMOTE_PLAYING.store(playing, Ordering::Relaxed);
    REMOTE_ACTIVE.store(true, Ordering::Release);
}

/// Drop back to the local position source. Called from the poll loop's
/// local fallthrough (every tick while no peer is active — cheap).
pub fn clear_remote_anchor() {
    REMOTE_ACTIVE.store(false, Ordering::Release);
}

thread_local! {
    static TIMER: Timer = Timer::default();
    static FAST: Cell<bool> = const { Cell::new(false) };
    static CTX: RefCell<Option<(Runtime, slint::Weak<AppWindow>)>> =
        const { RefCell::new(None) };
}

/// Start the engine timer. UI-thread only (slint::Timer requirement);
/// idempotent — called once from shell setup in `main`.
pub fn start(runtime: Runtime, weak: slint::Weak<AppWindow>) {
    CTX.with(|ctx| {
        *ctx.borrow_mut() = Some((runtime, weak));
    });
    TIMER.with(|timer| {
        if timer.running() {
            return;
        }
        timer.start(TimerMode::Repeated, IDLE_TICK, || tick(true));
    });
}

/// One immediate engine pass that bypasses the playing gate (panel-open /
/// doc-commit recompute — the view lands on the correct line instantly even
/// while paused). No-op before [`start`] or off the UI thread's context.
pub fn kick() {
    tick(false);
}

fn tick(require_playing: bool) {
    let Some((runtime, weak)) = CTX.with(|ctx| ctx.borrow().clone()) else {
        return;
    };
    let Some(window) = weak.upgrade() else {
        return;
    };
    let fast = compute_and_push(&window, &runtime, require_playing);
    set_cadence(fast);
}

fn set_cadence(fast: bool) {
    FAST.with(|flag| {
        if flag.get() == fast {
            return;
        }
        flag.set(fast);
        TIMER.with(|timer| {
            timer.set_interval(if fast { ACTIVE_TICK } else { IDLE_TICK });
        });
    });
}

/// Effective playback position in ms + the effective playing flag (the Q7
/// resolver — see module docs).
fn resolve_position_ms(runtime: &Runtime) -> (u64, bool) {
    if REMOTE_ACTIVE.load(Ordering::Acquire) {
        let playing = REMOTE_PLAYING.load(Ordering::Relaxed);
        let mut position_ms = REMOTE_POSITION_MS.load(Ordering::Relaxed);
        let updated_at_ms = REMOTE_UPDATED_AT_MS.load(Ordering::Relaxed);
        if playing && updated_at_ms > 0 {
            position_ms = position_ms.saturating_add(now_ms().saturating_sub(updated_at_ms));
        }
        (position_ms, playing)
    } else {
        let player = runtime.core().player();
        (
            player.state.current_position_ms(),
            player.state.is_playing(),
        )
    }
}

/// Compute the active index + karaoke fraction and push them onto
/// `LyricsState` — equality-guarded, animation zeroed across
/// discontinuities. Returns whether the gate is fully open (open + synced +
/// playing) so the caller can pick the cadence.
fn compute_and_push(window: &AppWindow, runtime: &Runtime, require_playing: bool) -> bool {
    if !window.global::<ShellState>().get_lyrics_open() {
        return false;
    }
    let lyrics = window.global::<LyricsState>();
    if !lyrics.get_synced() || lyrics.get_status() != crate::lyrics::STATUS_READY {
        return false;
    }

    let (position_ms, playing) = resolve_position_ms(runtime);
    if require_playing && !playing {
        // Pause freezes the ladder + fill in place (Tauri parity: the rAF
        // gate stops, getCurrentTime holds — review §8 "Pause").
        return false;
    }

    let now = position_ms.min(i64::MAX as u64) as i64;
    let (index, fraction) = crate::lyrics::with_current_doc(|doc| match doc {
        Some(doc) if doc.synced && !doc.lines.is_empty() => {
            let index = qbz_lyrics::sync::find_active_line_index(&doc.lines, now);
            let fraction = if index >= 0 {
                qbz_lyrics::sync::line_fill_fraction(&doc.lines, index as usize, now)
            } else {
                0.0
            };
            (index, fraction)
        }
        _ => (-1, 0.0),
    });

    // Push only on change. The properties themselves are the previous-tick
    // state (the commit path resets them with the doc), so the engine stays
    // stateless across track changes / re-opens.
    let ui_index = lyrics.get_active_index();
    let ui_progress = lyrics.get_line_progress();
    let index_changed = index != ui_index;
    let backward = fraction < ui_progress - 0.0005;
    // Discontinuity (line change / backward seek): first paint EXACTLY at
    // the reported progress — no smoothing across it (parity with
    // LyricsLines.svelte:114-138).
    let anim_ms = if index_changed || backward { 0 } else { FILL_ANIM_MS };
    if lyrics.get_fill_anim_ms() != anim_ms {
        lyrics.set_fill_anim_ms(anim_ms);
    }
    if index_changed {
        lyrics.set_active_index(index);
    }
    if index_changed || (fraction - ui_progress).abs() >= 0.001 {
        lyrics.set_line_progress(fraction);
    }

    // REQ-1 fan-out: mirror the karaoke scalars to the miniplayer (30Hz; no-op
    // when the mini is closed) so the mini lyrics highlight stays smooth.
    crate::miniplayer::mirror_lyrics_scalars(window);

    playing
}

/// Wall-clock now in epoch ms — same convention as the poll loop's
/// extrapolation (`playback.rs::now_ms`) and the renderer's
/// `updated_at_ms`.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
