//! Miniplayer — second Slint window: lifecycle + state mirroring.
//!
//! The miniplayer is a separate `MiniPlayerWindow` (borderless + transparent at
//! the winit level). Slint globals are per-component-tree, so this module
//! MIRRORS the main window's `NowPlayingState` / `LyricsState` into the mini's
//! instances and owns a self-contained navigable queue model. There is NO
//! second poll loop: the existing 450ms playback poll and the 30Hz lyrics tick
//! FAN OUT one extra call here (REQ-1). Transport callbacks DELEGATE to the
//! main window's already-wired remote-first handlers (zero duplication). The
//! per-row queue play routes remote-first and bails before touching local
//! (REQ-2).

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use i_slint_backend_winit::WinitWindowAccessor;
use slint::{ComponentHandle, Model};

use crate::adapter::SlintAdapter;
use crate::{
    AppWindow, ImmersiveState, LyricsState, MiniPlayerState, MiniPlayerWindow, NowPlayingState,
    ShellState,
};

type Runtime = Arc<qbz_app::shell::AppRuntime<SlintAdapter>>;

struct Ctx {
    runtime: Runtime,
    main: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
}

static CTX: OnceLock<Ctx> = OnceLock::new();
static MINI_WEAK: OnceLock<Mutex<Option<slint::Weak<MiniPlayerWindow>>>> = OnceLock::new();
static OPEN: AtomicBool = AtomicBool::new(false);

thread_local! {
    // Strong handle keeps the window alive; only ever touched on the UI thread.
    static MINI_STRONG: RefCell<Option<MiniPlayerWindow>> = RefCell::new(None);
    // Change-gates for the expensive mirror copies (artwork / queue / lyric lines).
    static LAST_TRACK: RefCell<String> = RefCell::new(String::new());
    static LAST_LYRICS_FP: RefCell<u64> = RefCell::new(0);
    // True when WE forced the main window's lyrics panel open to drive the sync
    // engine for the mini lyrics surface (restored on exit).
    static FORCED_LYRICS: RefCell<bool> = RefCell::new(false);
    // True ONLY while MiniPlayerWindow::new() runs — the global winit
    // window-attributes hook (set in main.rs) reads this to make ONLY the mini
    // borderless at CREATION (decorations can't be reliably removed post-creation
    // on Wayland/KDE, where server-side decorations are negotiated up front).
    static CREATING_MINI: RefCell<bool> = RefCell::new(false);
}

/// Read by the global winit window-attributes hook (main.rs) to scope
/// `decorations(false)` to the miniplayer window only. True only during the
/// mini window's construction (same UI thread as the hook).
pub fn is_creating_mini() -> bool {
    CREATING_MINI.with(|f| *f.borrow())
}

/// The lyrics sync engine (lyrics_sync.rs) only runs while the MAIN window's
/// lyrics panel is open. When the mini shows its lyrics surface, trigger the
/// fetch and open the (hidden) main panel so the karaoke advances; remember we
/// forced it so `exit` can restore it.
fn ensure_lyrics_engine(surface: i32) {
    if surface != 4 {
        return;
    }
    let Some(ctx) = CTX.get() else {
        return;
    };
    let Some(m) = ctx.main.upgrade() else {
        return;
    };
    if !m.global::<ShellState>().get_lyrics_open() {
        FORCED_LYRICS.with(|f| *f.borrow_mut() = true);
        m.global::<ShellState>().set_lyrics_open(true);
    }
    m.global::<LyricsState>().invoke_panel_opened();
}

/// Store the runtime/main-weak/handle. Idempotent (first call wins).
pub fn init(runtime: Runtime, main: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let _ = MINI_WEAK.set(Mutex::new(None));
    let _ = CTX.set(Ctx {
        runtime,
        main,
        handle,
    });
}

fn mini_weak() -> Option<slint::Weak<MiniPlayerWindow>> {
    MINI_WEAK.get()?.lock().ok()?.clone()
}

fn mini_upgrade() -> Option<MiniPlayerWindow> {
    mini_weak().and_then(|w| w.upgrade())
}

// ============================================================ lifecycle =====

/// Resolve the surface to open on: the per-user default-view, or the last
/// persisted surface when the default is "remember".
fn resolve_initial_surface(prefs: &crate::ui_prefs::UiPrefs) -> i32 {
    match prefs.mini_default_view.as_str() {
        "micro" => 0,
        "compact" => 1,
        "artwork" => 2,
        "queue" => 3,
        "lyrics" => 4,
        _ => prefs.mini_surface, // "remember" / unknown
    }
}

fn ensure_window() -> Option<MiniPlayerWindow> {
    if let Some(w) = MINI_STRONG.with(|s| s.borrow().as_ref().map(|w| w.clone_strong())) {
        return Some(w);
    }
    let ctx = CTX.get()?;
    // Borderless-at-creation: flag the global attributes hook for this one
    // construction (Wayland/KDE decide decorations at surface creation).
    CREATING_MINI.with(|f| *f.borrow_mut() = true);
    let created = MiniPlayerWindow::new().ok();
    CREATING_MINI.with(|f| *f.borrow_mut() = false);
    let mini = created?;
    if let Some(m) = ctx.main.upgrade() {
        mini.set_system_font(m.get_system_font());
    }

    let prefs = crate::ui_prefs::load();
    let mp = mini.global::<MiniPlayerState>();
    mp.set_surface(resolve_initial_surface(&prefs));
    mp.set_background_blur(prefs.mini_background_blur);

    wire_callbacks(&mini, ctx);
    install_winit(&mini);

    if let Some(slot) = MINI_WEAK.get() {
        if let Ok(mut g) = slot.lock() {
            *g = Some(mini.as_weak());
        }
    }
    MINI_STRONG.with(|s| *s.borrow_mut() = Some(mini.clone_strong()));
    Some(mini)
}

/// Enter miniplayer mode: show/focus the mini, then hide the main window.
pub fn enter() {
    let Some(mini) = ensure_window() else {
        return;
    };
    let Some(ctx) = CTX.get() else {
        return;
    };

    // The winit window/surface (where the attributes hook fires) may be created
    // lazily on set_size/show(), not on new() — keep CREATING_MINI true across
    // ALL of geometry-apply + show() so the borderless attribute applies
    // regardless of which call realizes the window.
    CREATING_MINI.with(|f| *f.borrow_mut() = true);
    apply_surface_geometry(&mini, mini.global::<MiniPlayerState>().get_surface());
    let _ = mini.show();
    CREATING_MINI.with(|f| *f.borrow_mut() = false);
    // Borderless + always-on-top are now driven by the Slint Window properties
    // (`no-frame` / `always-on-top`) on MiniPlayerWindow — Slint's adapter
    // overrides winit decorations/level from those at every realization, so we
    // must NOT fight it from winit here.
    ensure_lyrics_engine(mini.global::<MiniPlayerState>().get_surface());

    OPEN.store(true, Ordering::SeqCst);

    // Seed the mini immediately so it isn't blank for up to one poll tick.
    if let Some(m) = ctx.main.upgrade() {
        force_full_mirror(&m, &mini);
    }
    spawn_queue_refresh();

    // Hide the main window only after the mini is visible (avoid a blank flash).
    if let Some(m) = ctx.main.upgrade() {
        let _ = m.hide();
    }
}

/// Exit miniplayer mode (expand back to the full app).
pub fn exit() {
    OPEN.store(false, Ordering::SeqCst);
    MINI_STRONG.with(|s| {
        if let Some(mini) = s.borrow().as_ref() {
            let _ = mini.hide();
        }
    });
    if let Some(ctx) = CTX.get() {
        if let Some(m) = ctx.main.upgrade() {
            // Restore the main lyrics panel if WE forced it open for the mini.
            FORCED_LYRICS.with(|f| {
                if *f.borrow() {
                    m.global::<ShellState>().set_lyrics_open(false);
                    *f.borrow_mut() = false;
                }
            });
            let _ = m.show();
        }
    }
}

/// Close the whole app from the mini — hide mini, show main, route to the
/// main window's close-app handler (tray-or-quit choreography lives there).
fn close_app() {
    OPEN.store(false, Ordering::SeqCst);
    MINI_STRONG.with(|s| {
        if let Some(mini) = s.borrow().as_ref() {
            let _ = mini.hide();
        }
    });
    if let Some(ctx) = CTX.get() {
        if let Some(m) = ctx.main.upgrade() {
            let _ = m.show();
            m.invoke_close_app();
        }
    }
}

// ============================================================ callbacks =====

fn wire_callbacks(mini: &MiniPlayerWindow, ctx: &Ctx) {
    // --- Transport: delegate to the main window's wired remote-first handlers.
    let np = mini.global::<NowPlayingState>();
    macro_rules! delegate {
        ($on:ident, $invoke:ident) => {{
            let main = ctx.main.clone();
            np.$on(move || {
                if let Some(m) = main.upgrade() {
                    m.global::<NowPlayingState>().$invoke();
                }
            });
        }};
    }
    delegate!(on_toggle_play, invoke_toggle_play);
    delegate!(on_next, invoke_next);
    delegate!(on_previous, invoke_previous);
    delegate!(on_toggle_mute, invoke_toggle_mute);
    delegate!(on_toggle_shuffle, invoke_toggle_shuffle);
    delegate!(on_cycle_repeat, invoke_cycle_repeat);
    delegate!(on_qconnect_toggle, invoke_qconnect_toggle);
    {
        let main = ctx.main.clone();
        np.on_seek(move |f| {
            if let Some(m) = main.upgrade() {
                m.global::<NowPlayingState>().invoke_seek(f);
            }
        });
    }
    {
        let main = ctx.main.clone();
        np.on_set_volume(move |v| {
            if let Some(m) = main.upgrade() {
                m.global::<NowPlayingState>().invoke_set_volume(v);
            }
        });
    }

    // --- Mini-specific actions.
    let mp = mini.global::<MiniPlayerState>();
    mp.on_surface_change(|s| set_surface(s));
    mp.on_expand(|| exit());
    mp.on_close_app(|| close_app());
    mp.on_toggle_background_blur(|| toggle_background_blur());
    mp.on_start_drag(|| start_drag());
    mp.on_queue_play(|idx| queue_play(idx));
}

fn set_surface(surface: i32) {
    MINI_STRONG.with(|s| {
        if let Some(mini) = s.borrow().as_ref() {
            mini.global::<MiniPlayerState>().set_surface(surface);
            apply_surface_geometry(mini, surface);
        }
    });
    let mut prefs = crate::ui_prefs::load();
    prefs.mini_surface = surface;
    crate::ui_prefs::save(&prefs);
    if surface == 3 {
        spawn_queue_refresh();
    }
    ensure_lyrics_engine(surface);
}

fn toggle_background_blur() {
    let new = MINI_STRONG.with(|s| {
        s.borrow().as_ref().map(|mini| {
            let mp = mini.global::<MiniPlayerState>();
            let v = !mp.get_background_blur();
            mp.set_background_blur(v);
            v
        })
    });
    if let Some(v) = new {
        let mut prefs = crate::ui_prefs::load();
        prefs.mini_background_blur = v;
        crate::ui_prefs::save(&prefs);
    }
}

fn start_drag() {
    MINI_STRONG.with(|s| {
        if let Some(mini) = s.borrow().as_ref() {
            mini.window().with_winit_window(|win| {
                let _ = win.drag_window();
            });
        }
    });
}

fn queue_play(idx: i32) {
    let Some(ctx) = CTX.get() else {
        return;
    };
    let track_id = MINI_STRONG.with(|s| {
        s.borrow().as_ref().and_then(|mini| {
            let m = mini.global::<MiniPlayerState>().get_queue_tracks();
            if idx >= 0 && (idx as usize) < m.row_count() {
                m.row_data(idx as usize).map(|it| it.id.to_string())
            } else {
                None
            }
        })
    });
    let Some(tid_str) = track_id else {
        return;
    };
    let Ok(tid) = tid_str.parse::<u64>() else {
        return;
    };

    let rt = ctx.runtime.clone();
    let main = ctx.main.clone();
    ctx.handle.spawn(async move {
        // Remote-first; bail before touching local on handled OR error (REQ-2).
        if let Some(svc) = crate::qconnect_service::service() {
            match svc.play_remote_renderer_track_if_active(tid).await {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => {
                    log::warn!("[mini] queue play remote handoff: {e}");
                    return;
                }
            }
        }
        // Local. Index 0 = the current track -> resume if paused (no restart).
        if idx == 0 {
            let _ = main.upgrade_in_event_loop(|m| {
                let np = m.global::<NowPlayingState>();
                if !np.get_playing() {
                    np.invoke_toggle_play();
                }
            });
            return;
        }
        let upcoming_index = (idx - 1) as usize;
        if let Some(track) = rt.core().play_upcoming_at(upcoming_index).await {
            crate::playback::after_track_change(&rt, &main, track.id).await;
            crate::playback::refresh_sidebar(true);
        }
    });
}

// ============================================================ winit =========

fn apply_surface_geometry(mini: &MiniPlayerWindow, surface: i32) {
    let prefs = crate::ui_prefs::load();
    let (w, h, min_h): (f32, f32, f32) = match surface {
        0 => (380.0, 57.0, 57.0),
        1 => (380.0, 178.0, 170.0),
        _ => (prefs.mini_width.max(340.0), prefs.mini_height.max(420.0), 420.0),
    };
    mini.window()
        .set_size(slint::LogicalSize::new(w, h));
    mini.window().with_winit_window(|win| {
        use i_slint_backend_winit::winit::dpi::LogicalSize as WinitLogical;
        win.set_min_inner_size(Some(WinitLogical::new(340.0_f64, min_h as f64)));
    });
}

fn install_winit(mini: &MiniPlayerWindow) {
    let weak = mini.as_weak();
    mini.window()
        .on_winit_window_event(move |_win, event| {
            use i_slint_backend_winit::winit::event::WindowEvent;
            match event {
                WindowEvent::CursorMoved { .. } => {
                    if let Some(w) = weak.upgrade() {
                        w.global::<MiniPlayerState>().set_window_hovered(true);
                    }
                }
                WindowEvent::CursorLeft { .. } => {
                    if let Some(w) = weak.upgrade() {
                        w.global::<MiniPlayerState>().set_window_hovered(false);
                    }
                }
                WindowEvent::Resized(size) => {
                    if let Some(w) = weak.upgrade() {
                        let surface = w.global::<MiniPlayerState>().get_surface();
                        // Persist the EXPANDED size while an expanded surface is
                        // shown (so condensed->expanded restores it). Ignore
                        // sub-320 heights (mid-transition).
                        if surface >= 2 {
                            let scale = w.window().scale_factor().max(0.01);
                            let lw = size.width as f32 / scale;
                            let lh = size.height as f32 / scale;
                            if lh >= 320.0 {
                                let mut prefs = crate::ui_prefs::load();
                                prefs.mini_width = lw.max(340.0);
                                prefs.mini_height = lh.max(420.0);
                                crate::ui_prefs::save(&prefs);
                            }
                        }
                    }
                }
                _ => {}
            }
            i_slint_backend_winit::EventResult::Propagate
        });
}

// ============================================================ mirror ========

/// Fan-out hook called from the 450ms playback poll (REQ-1). Cheap when the
/// mini is closed.
pub fn mirror_tick(main: &AppWindow) {
    if !OPEN.load(Ordering::SeqCst) {
        return;
    }
    let Some(mini) = mini_upgrade() else {
        return;
    };
    mirror_now_playing_fast(main, &mini);

    let tid = main.global::<NowPlayingState>().get_track_id().to_string();
    let changed = LAST_TRACK.with(|l| {
        let c = *l.borrow() != tid;
        if c {
            *l.borrow_mut() = tid.clone();
        }
        c
    });
    if changed {
        mirror_artwork(main, &mini);
        spawn_queue_refresh();
    }
    mirror_lyrics_gated(main, &mini);
}

/// Fan-out hook called from the 30Hz lyrics tick — only the cheap karaoke
/// scalars, for smooth highlighting on the mini.
pub fn mirror_lyrics_scalars(main: &AppWindow) {
    if !OPEN.load(Ordering::SeqCst) {
        return;
    }
    let Some(mini) = mini_upgrade() else {
        return;
    };
    let s = main.global::<LyricsState>();
    let d = mini.global::<LyricsState>();
    d.set_active_index(s.get_active_index());
    d.set_line_progress(s.get_line_progress());
    d.set_fill_anim_ms(s.get_fill_anim_ms());
}

fn force_full_mirror(main: &AppWindow, mini: &MiniPlayerWindow) {
    mirror_now_playing_fast(main, mini);
    mirror_artwork(main, mini);
    LAST_TRACK.with(|l| *l.borrow_mut() = main.global::<NowPlayingState>().get_track_id().to_string());
    // Force the lyric-lines copy.
    LAST_LYRICS_FP.with(|l| *l.borrow_mut() = 0);
    mirror_lyrics_gated(main, mini);
}

fn mirror_now_playing_fast(main: &AppWindow, mini: &MiniPlayerWindow) {
    let s = main.global::<NowPlayingState>();
    let d = mini.global::<NowPlayingState>();
    d.set_has_track(s.get_has_track());
    d.set_title(s.get_title());
    d.set_artist(s.get_artist());
    d.set_album(s.get_album());
    d.set_album_id(s.get_album_id());
    d.set_artist_id(s.get_artist_id());
    d.set_track_id(s.get_track_id());
    d.set_is_ephemeral(s.get_is_ephemeral());
    d.set_explicit(s.get_explicit());
    d.set_playing(s.get_playing());
    d.set_loading(s.get_loading());
    d.set_position_secs(s.get_position_secs());
    d.set_duration_secs(s.get_duration_secs());
    d.set_progress(s.get_progress());
    d.set_cache(s.get_cache());
    d.set_seekable_max(s.get_seekable_max());
    d.set_elapsed(s.get_elapsed());
    d.set_remaining(s.get_remaining());
    d.set_volume(s.get_volume());
    d.set_muted(s.get_muted());
    d.set_shuffle(s.get_shuffle());
    d.set_repeat_mode(s.get_repeat_mode());
    d.set_quality_tier(s.get_quality_tier());
    d.set_quality_detail(s.get_quality_detail());
    d.set_is_remote(s.get_is_remote());
    d.set_volume_locked(s.get_volume_locked());
    d.set_qconnect_connected(s.get_qconnect_connected());
    d.set_cast_target(s.get_cast_target());
    d.set_cast_active(s.get_cast_active());
    d.set_cast_protocol(s.get_cast_protocol());
    d.set_context_kind(s.get_context_kind());
    d.set_context_id(s.get_context_id());

    let si = main.global::<ImmersiveState>();
    let di = mini.global::<ImmersiveState>();
    di.set_bg_image(si.get_bg_image());
}

fn mirror_artwork(main: &AppWindow, mini: &MiniPlayerWindow) {
    let s = main.global::<NowPlayingState>();
    let d = mini.global::<NowPlayingState>();
    d.set_artwork(s.get_artwork());
    d.set_artwork_large(s.get_artwork_large());
}

fn mirror_lyrics_gated(main: &AppWindow, mini: &MiniPlayerWindow) {
    let s = main.global::<LyricsState>();
    let d = mini.global::<LyricsState>();
    d.set_status(s.get_status());
    d.set_synced(s.get_synced());
    d.set_provider(s.get_provider());
    d.set_provider_label(s.get_provider_label());
    d.set_active_index(s.get_active_index());
    d.set_line_progress(s.get_line_progress());
    d.set_fill_anim_ms(s.get_fill_anim_ms());

    // Gate the lines-model copy on (status, line-count) so we don't thrash the
    // lyrics view (which would reset its scroll) every tick.
    let lines = s.get_lines();
    let fp: u64 = ((s.get_status() as u64) << 32) ^ (lines.row_count() as u64);
    let changed = LAST_LYRICS_FP.with(|l| {
        let c = *l.borrow() != fp;
        if c {
            *l.borrow_mut() = fp;
        }
        c
    });
    if changed {
        d.set_lines(lines);
    }
}

fn spawn_queue_refresh() {
    if !OPEN.load(Ordering::SeqCst) {
        return;
    }
    let Some(ctx) = CTX.get() else {
        return;
    };
    let Some(mw) = mini_weak() else {
        return;
    };
    let rt = ctx.runtime.clone();
    ctx.handle.spawn(async move {
        // `state` (qbz_models::QueueState) is plain Send data. Build the Slint
        // QueueItem model INSIDE the event loop — QueueItem holds a Slint Image
        // (!Send), so the Vec must never cross the thread boundary.
        let state = rt.core().get_queue_state_full().await;
        let _ = mw.upgrade_in_event_loop(move |mini| {
            let items = crate::queue::mini_queue_items(&state);
            let cur = state
                .current_track
                .as_ref()
                .map(|t| t.id.to_string())
                .unwrap_or_default();
            let model = std::rc::Rc::new(slint::VecModel::from(items));
            let mp = mini.global::<MiniPlayerState>();
            mp.set_queue_tracks(model.into());
            mp.set_current_track_id(cur.into());
        });
    });
}
