//! System tray for the Slint app.
//!
//! Faithful port of the Tauri tray (`src-tauri/src/tray.rs` +
//! `src-tauri/src/tray_linux_ksni.rs`). Platform split, same as Tauri:
//!   - **Linux** → `ksni` / StatusNotifierItem (`linux` submodule). Tauri's
//!     libayatana path never dispatches primary-activate (left-click); ksni
//!     exposes Activate / SecondaryActivate / Scroll (issue #310).
//!   - **macOS / Windows** → `tray-icon` (added with the
//!     CustomApplicationHandler slice). Until then, a no-op on those targets.
//!
//! Tray actions differ from Tauri in ONE way: there is no webview to emit
//! events to, so play/pause/next/previous/volume call the playback controller
//! directly (mirroring the now-playing bar's QConnect-aware dispatch) and
//! show/hide toggles the winit window in place.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use slint::ComponentHandle;

use crate::adapter::SlintAdapter;
use crate::AppWindow;
use qbz_app::shell::AppRuntime;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
mod macos;

/// Shared runtime handle type used across the Slint app.
pub(crate) type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Whether the main window is currently shown. The tray toggle and (later)
/// close-to-tray both flip this so left-click / "Show/Hide" stay consistent
/// even on backends where querying winit visibility is unreliable (Wayland).
static WINDOW_SHOWN: AtomicBool = AtomicBool::new(true);

/// Cross-thread handle to the live tray. Cloneable; mutators forward to the
/// platform backend (ksni on Linux) and are no-ops when the tray is disabled
/// or on a platform without a live-update path.
#[derive(Clone, Default)]
pub struct TrayHandle {
    #[cfg(target_os = "linux")]
    linux: Option<linux::LinuxTrayHandle>,
}

impl TrayHandle {
    pub fn set_track(&self, title: String, artist: String, album: String) {
        #[cfg(target_os = "linux")]
        if let Some(h) = &self.linux {
            h.set_track(title, artist, album);
            return;
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (title, artist, album);
    }

    pub fn clear_track(&self) {
        #[cfg(target_os = "linux")]
        if let Some(h) = &self.linux {
            h.clear_track();
        }
    }

    pub fn set_playing(&self, is_playing: bool) {
        #[cfg(target_os = "linux")]
        if let Some(h) = &self.linux {
            h.set_playing(is_playing);
            return;
        }
        #[cfg(not(target_os = "linux"))]
        let _ = is_playing;
    }

    pub fn set_icon_theme(&self, theme: String) {
        #[cfg(target_os = "linux")]
        if let Some(h) = &self.linux {
            h.set_icon_theme(theme);
            return;
        }
        #[cfg(target_os = "macos")]
        {
            // The NSStatusItem is !Send and lives on the main thread — re-theme
            // it there.
            let _ = slint::invoke_from_event_loop(move || macos::set_icon_theme(&theme));
            return;
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let _ = theme;
    }
}

/// Process-global tray handle, set once by `init`. `None` until the tray is
/// created (or forever, if disabled / unsupported platform).
static TRAY: std::sync::OnceLock<TrayHandle> = std::sync::OnceLock::new();

/// The live tray handle, if the tray was initialized. Callers (playback poll
/// loop, settings) use this to push tooltip / theme updates.
pub fn handle() -> Option<&'static TrayHandle> {
    TRAY.get()
}

/// Initialize the system tray, gated by the user's `enable_tray` setting.
/// `theme_override` is the persisted `tray_icon_theme` ("auto"/"mono-light"/
/// "mono-dark"/"color"). No-op when disabled or already initialized.
pub fn init(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    theme_override: String,
    enabled: bool,
) {
    if !enabled {
        log::info!("[tray] disabled by user setting");
        return;
    }
    if TRAY.get().is_some() {
        return;
    }

    #[cfg(target_os = "linux")]
    {
        // ksni's blocking `spawn()` calls `Runtime::block_on` internally, which
        // panics inside an existing tokio runtime (`init` is called from the
        // tokio-based shell-entry task). Run the ksni setup on a dedicated
        // std::thread, outside any tokio context (the Tauri build is safe
        // because it inits from the non-tokio Tauri setup hook). The ksni
        // service + updater thread persist independently; this thread exits.
        std::thread::Builder::new()
            .name("qbz-tray-init".into())
            .spawn(move || match linux::init(runtime, weak, handle, &theme_override) {
                Ok(linux_handle) => {
                    let _ = TRAY.set(TrayHandle {
                        linux: Some(linux_handle),
                    });
                }
                Err(e) => log::error!("[tray] Linux tray init failed: {e}"),
            })
            .expect("spawn tray init thread");
    }

    #[cfg(target_os = "macos")]
    {
        // The NSStatusItem is !Send and must be built on the main thread with
        // NSApplication already running — create it on the Slint event loop.
        let _ = slint::invoke_from_event_loop(move || {
            macos::create(runtime, weak, handle, &theme_override);
            let _ = TRAY.set(TrayHandle::default());
        });
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (runtime, weak, handle, theme_override);
        log::info!("[tray] no tray backend on this platform");
    }
}

// ---------------------------------------------------------------------------
// Window show/hide — used by left-click, the "Show/Hide" menu item, and
// close-to-tray. Uses Slint's own `hide()`/`show()` (NOT winit `set_visible`,
// which is a no-op on Wayland): since Slint 1.7 `hide()` destroys the winit
// surface on Wayland and `show()` recreates it (PR slint-ui/slint#5529), the
// only path that actually works on KWin Wayland. The app survives a hidden
// window because main() runs the loop via `run_event_loop_until_quit()`
// (quit_on_last_window_closed = false).
// ---------------------------------------------------------------------------

/// Toggle the main window: hide if shown, else show + focus.
pub(crate) fn toggle_window(weak: &slint::Weak<AppWindow>) {
    if WINDOW_SHOWN.load(Ordering::Relaxed) {
        hide_window(weak);
    } else {
        show_window(weak);
    }
}

/// Show the main window (recreates the Wayland surface) and focus it.
pub(crate) fn show_window(weak: &slint::Weak<AppWindow>) {
    WINDOW_SHOWN.store(true, Ordering::Relaxed);
    let _ = weak.upgrade_in_event_loop(|w| {
        if let Err(e) = w.show() {
            log::error!("[tray] window show failed: {e}");
        }
        // Best-effort raise/focus the re-created window (the compositor has
        // the final say on Wayland).
        use i_slint_backend_winit::WinitWindowAccessor;
        w.window().with_winit_window(|win| {
            win.focus_window();
        });
        // Restore the Dock icon when coming back from the menu bar.
        #[cfg(target_os = "macos")]
        macos::set_dock_icon_hidden(false);
    });
}

/// Hide the main window to the tray (surface destroyed; the process keeps
/// running, the ksni service stays alive on its own thread).
pub(crate) fn hide_window(weak: &slint::Weak<AppWindow>) {
    WINDOW_SHOWN.store(false, Ordering::Relaxed);
    let _ = weak.upgrade_in_event_loop(|w| {
        if let Err(e) = w.hide() {
            log::error!("[tray] window hide failed: {e}");
        }
        // Spotify-style opt-in: drop the Dock icon while closed to the menu bar.
        #[cfg(target_os = "macos")]
        if crate::tray_settings::get().mac_hide_dock {
            macos::set_dock_icon_hidden(true);
        }
    });
}

/// Sync the shown-state flag without touching the window — used when Slint
/// itself performs the hide (e.g. an `on_close_requested` → `HideWindow`
/// response) so the next tray toggle knows to show.
pub(crate) fn set_window_shown(shown: bool) {
    WINDOW_SHOWN.store(shown, Ordering::Relaxed);
}

/// Apply the macOS Dock-icon activation policy (`.accessory` hides the Dock
/// icon, `.regular` keeps it). No-op off macOS. Must be called on the main
/// thread (it is, from the close handlers / window hide-show).
pub(crate) fn set_mac_dock_hidden(hidden: bool) {
    #[cfg(target_os = "macos")]
    macos::set_dock_icon_hidden(hidden);
    #[cfg(not(target_os = "macos"))]
    let _ = hidden;
}

/// Quit the whole app from a tray action (any thread).
pub(crate) fn quit() {
    log::info!("[tray] quit requested");
    let _ = slint::invoke_from_event_loop(|| {
        let _ = slint::quit_event_loop();
    });
}

// ---------------------------------------------------------------------------
// Player-action dispatch — mirrors the now-playing bar's QConnect-aware
// handlers (main.rs on_toggle_play / on_next / on_previous) so the tray drives
// the exact same path: try the remote renderer first, else local playback.
// ---------------------------------------------------------------------------

pub(crate) fn dispatch_play_pause(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    let spawn_handle = handle.clone();
    handle.spawn(async move {
        if let Some(svc) = crate::qconnect_service::service() {
            match svc.toggle_remote_renderer_playback_if_active().await {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => {
                    log::warn!("[tray] play_pause handoff: {e}");
                    return;
                }
            }
        }
        crate::playback::toggle_play_pause(runtime, weak, spawn_handle);
    });
}

pub(crate) fn dispatch_next(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    let spawn_handle = handle.clone();
    handle.spawn(async move {
        if let Some(svc) = crate::qconnect_service::service() {
            match svc.skip_next_if_remote().await {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => {
                    log::warn!("[tray] next handoff: {e}");
                    return;
                }
            }
        }
        crate::playback::next(runtime, weak, spawn_handle);
    });
}

pub(crate) fn dispatch_previous(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    let spawn_handle = handle.clone();
    handle.spawn(async move {
        if let Some(svc) = crate::qconnect_service::service() {
            match svc.skip_previous_if_remote().await {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => {
                    log::warn!("[tray] previous handoff: {e}");
                    return;
                }
            }
        }
        crate::playback::previous(runtime, weak, spawn_handle);
    });
}

/// Step the local volume by `ticks` notches of 5% (positive = up). Mirrors the
/// Tauri `tray:volume_delta` handler. Local-only for now (remote-renderer
/// volume forwarding is a later refinement). Linux-only: scroll-to-volume is a
/// StatusNotifierItem feature the macOS/Windows tray doesn't expose.
#[cfg(target_os = "linux")]
pub(crate) fn dispatch_volume_delta(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    ticks: i32,
) {
    let spawn_handle = handle.clone();
    handle.spawn(async move {
        let current = runtime.core().player().get_playback_event().volume;
        let next = (current + ticks as f32 * 0.05).clamp(0.0, 1.0);
        crate::playback::set_volume(runtime, weak, spawn_handle, next);
    });
}
