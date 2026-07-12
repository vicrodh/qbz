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

use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

use qbz_app::shell::AppRuntime;

use crate::adapter::SlintAdapter;
use crate::lyrics_measure;
use crate::{AppWindow, ImmersiveState, LyricsSegment, LyricsState, ShellState};

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
    /// Last inputs the active-line segmentation was computed for, so the
    /// (potentially font-shaping) recompute only runs on a real change —
    /// active-line index, content-width budget, font family, size tier, or the
    /// uppercase transform — never every 30Hz tick. `None` forces a recompute.
    /// Separate cells per surface (the main window vs the miniplayer have
    /// different content widths, hence different segmentations).
    static SEG_KEY_MAIN: RefCell<Option<SegKey>> = const { RefCell::new(None) };
    static SEG_KEY_MINI: RefCell<Option<SegKey>> = const { RefCell::new(None) };
}

/// Which surface's segmentation cache to consult.
#[derive(Clone, Copy)]
pub(crate) enum SegSurface {
    Main,
    Mini,
}

/// Cache key for the active-line segmentation (see `SEG_KEY`).
#[derive(PartialEq, Clone)]
struct SegKey {
    index: i32,
    /// Content width in px, quantized to whole px so sub-pixel jitter from the
    /// layout doesn't thrash the recompute.
    width_px: i32,
    font_index: i32,
    size_index: i32,
    uppercase: bool,
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
/// The sync engine ticks while ANY lyrics surface is showing: the main
/// sidebar (`ShellState.lyrics-open`) OR an immersive lyrics panel (FOCUS
/// mode 4, or SPLIT panel 0). The immersive panels reuse this same engine, so
/// without widening the gate it stayed sidebar-only and immersive lyrics froze
/// in place (`active-index`/`line-progress` never advanced, and the open-time
/// `kick()` no-op'd because it bails here before computing).
fn lyrics_surface_open(window: &AppWindow) -> bool {
    let shell = window.global::<ShellState>();
    if shell.get_lyrics_open() {
        return true;
    }
    // Kiosk Now Playing Cover<->Lyrics toggle: it has no desktop sidebar, so it
    // raises its own flag while lyrics are on screen (see ShellState
    // `kiosk-lyrics-follow`). Same widen-the-gate fix the immersive panels got.
    if shell.get_kiosk_lyrics_follow() {
        return true;
    }
    let imm = window.global::<ImmersiveState>();
    imm.get_open()
        && ((imm.get_view_mode() == 0 && imm.get_mode() == 4)
            || (imm.get_view_mode() == 1 && imm.get_split_panel() == 0))
}

fn compute_and_push(window: &AppWindow, runtime: &Runtime, require_playing: bool) -> bool {
    if !lyrics_surface_open(window) {
        return false;
    }
    let lyrics = window.global::<LyricsState>();
    if !lyrics.get_synced() || lyrics.get_status() != crate::lyrics::STATUS_READY {
        return false;
    }

    let (position_ms, playing) = resolve_position_ms(runtime);
    if require_playing && !playing {
        // Pause freezes the ladder + fill in place (Tauri parity: the rAF
        // gate stops, getCurrentTime holds — review §8 "Pause"). Still refresh
        // the segmentation from the last-pushed active index so a resize or a
        // font/size pref change WHILE PAUSED re-flows the wrapped highlight
        // (cache-guarded — a no-op when nothing relevant changed).
        refresh_active_segments(
            &lyrics,
            lyrics.get_active_index(),
            main_render_prefs(&lyrics),
            SegSurface::Main,
        );
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

    // Per-visual-line karaoke segmentation: recompute (cache-guarded) when the
    // active line, content-width, font, size tier, or uppercase pref changes.
    refresh_active_segments(&lyrics, index, main_render_prefs(&lyrics), SegSurface::Main);

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

/// Render prefs for the MAIN sidebar surface — driven by the persisted prefs
/// on `LyricsState`, exactly as `LyricsSidebar.slint` binds them into
/// `LyricsLinesView`.
fn main_render_prefs(lyrics: &LyricsState) -> RenderPrefs {
    RenderPrefs {
        font_index: lyrics.get_font_index(),
        size_index: lyrics.get_size_index(),
        uppercase: lyrics.get_uppercase(),
    }
}

/// Active-line font size in px (`size-active`), replicating the size-tier ->
/// px mapping in `LyricsSidebar.slint` (0 S = 13 · 1 M = 15 · 2 L = 19 ·
/// 3 XL = 24). Used as the ppem for the segmentation measure pass so segment
/// widths match the drawn active line.
fn size_active_px(size_index: i32) -> f32 {
    match size_index {
        0 => 13.0,
        2 => 19.0,
        3 => 24.0,
        _ => 15.0,
    }
}

/// The font + size tier + uppercase the active line is RENDERED with — the
/// segmentation must measure against these so segment widths match the draw.
/// The main sidebar drives them from the persisted prefs on `LyricsState`; the
/// miniplayer renders with FIXED values (Inter / 15px / no uppercase — it
/// binds none of the prefs, see `MiniLyricsSurface.slint`), so it passes those.
#[derive(Clone, Copy)]
pub(crate) struct RenderPrefs {
    pub font_index: i32,
    pub size_index: i32,
    pub uppercase: bool,
}

/// Recompute (cache-guarded) the per-visual-line segmentation of the ACTIVE
/// line and push it onto `lyrics.active-segments`. Cheap no-op unless the
/// active index, content-width, or render prefs changed since the last push.
/// When there is no active line, clears the model. `surface` selects the
/// per-window cache cell (main window vs miniplayer).
pub(crate) fn refresh_active_segments(
    lyrics: &LyricsState,
    index: i32,
    prefs: RenderPrefs,
    surface: SegSurface,
) {
    let content_width = lyrics.get_content_width();
    let font_index = prefs.font_index;
    let size_index = prefs.size_index;
    let uppercase = prefs.uppercase;

    let key = SegKey {
        index,
        // Quantize to whole px so layout sub-pixel jitter doesn't re-segment.
        width_px: content_width.round() as i32,
        font_index,
        size_index,
        uppercase,
    };
    let set_key = |k: Option<SegKey>| match surface {
        SegSurface::Main => SEG_KEY_MAIN.with(|cell| *cell.borrow_mut() = k),
        SegSurface::Mini => SEG_KEY_MINI.with(|cell| *cell.borrow_mut() = k),
    };
    let unchanged = match surface {
        SegSurface::Main => SEG_KEY_MAIN.with(|cell| cell.borrow().as_ref() == Some(&key)),
        SegSurface::Mini => SEG_KEY_MINI.with(|cell| cell.borrow().as_ref() == Some(&key)),
    };
    if unchanged {
        return;
    }

    // No active line, no measurable width, or no doc -> clear the model.
    if index < 0 || content_width <= 0.0 {
        if lyrics.get_active_segments().row_count() > 0 {
            lyrics.set_active_segments(ModelRc::new(VecModel::<LyricsSegment>::default()));
        }
        set_key(Some(key));
        return;
    }

    // Pull the raw active-line text from the parsed doc (the UI model carries
    // the same text, but the doc read is already the engine's source of truth).
    let raw = crate::lyrics::with_current_doc(|doc| match doc {
        Some(doc) if doc.synced => doc
            .lines
            .get(index as usize)
            .map(|line| line.text.clone()),
        _ => None,
    });
    let Some(raw) = raw else {
        lyrics.set_active_segments(ModelRc::new(VecModel::<LyricsSegment>::default()));
        set_key(Some(key));
        return;
    };

    // Match the rendered transform: the active line is uppercased when the pref
    // is on, so segment widths must be measured on the uppercased string.
    let text = if uppercase { raw.to_uppercase() } else { raw };

    let size_px = size_active_px(size_index);
    let segments = lyrics_measure::wrap_segments(&text, font_index, size_px, content_width);

    // Convert measured widths into cumulative start-weight + per-segment
    // width-weight (Tauri's getSegmentProgress partition) so the row can
    // re-map the single global `line-progress` to a per-segment local fill.
    let total: f32 = segments.iter().map(|seg| seg.width_px).sum();
    let mut cumulative = 0.0_f32;
    let mut out: Vec<LyricsSegment> = Vec::with_capacity(segments.len());
    for seg in &segments {
        let width_weight = if total > 0.0 { seg.width_px / total } else { 0.0 };
        out.push(LyricsSegment {
            text: SharedString::from(seg.text.as_str()),
            start_weight: cumulative,
            width_weight,
        });
        cumulative += width_weight;
    }

    lyrics.set_active_segments(ModelRc::new(VecModel::from(out)));
    set_key(Some(key));
}
