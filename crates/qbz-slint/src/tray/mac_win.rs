//! macOS / Windows system tray via the `tray-icon` crate.
//!
//! macOS has no "system tray" — `tray-icon` gives us an `NSStatusItem` in the
//! menu bar. Unlike the Linux ksni path, `tray_icon::TrayIcon` is `!Send`
//! (`Rc<RefCell<..>>`) and on macOS MUST be built on the main thread with the
//! `NSApplication` already running, so the whole thing lives in a
//! `thread_local!` and is created via `slint::invoke_from_event_loop` (main
//! thread). Events arrive on global crossbeam channels which we drain from a
//! repeating `slint::Timer` (also main-thread).
//!
//! Faithful to the Tauri macOS tray: same 5-item menu, left-click toggles the
//! window, static "QBZ - Music Player" tooltip (no live track reflection on
//! macOS), icon theme switchable. close-to-tray itself is handled in the
//! cross-platform `on_close_requested` path (Slint hide/show). This module
//! adds the macOS Dock-icon activation-policy toggle on top.

use std::cell::RefCell;

use image::GenericImageView;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use super::Runtime;
use crate::AppWindow;

// 44px assets (= 22pt @2x menu bar). Filename trap (shared with Linux):
// `tray-dark-*` holds the WHITE glyph, `tray-light-*` holds the BLACK glyph.
const ICON_COLOR: &[u8] = include_bytes!("../../icons/tray-color-44.png");
const ICON_WHITE: &[u8] = include_bytes!("../../icons/tray-dark-44.png");
const ICON_BLACK: &[u8] = include_bytes!("../../icons/tray-light-44.png");

thread_local! {
    // Kept alive for the icon's lifetime; dropping the TrayIcon removes it.
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
    // The event-pump timer; dropping it stops polling.
    static PUMP_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

/// Resolve the icon bytes + whether to render it as a macOS template image
/// (template = adapts to the light/dark menu bar automatically).
/// - "color"      → full vinyl, not a template
/// - "mono-light" → white glyph (`tray-dark`), not a template
/// - "mono-dark"  → black glyph (`tray-light`), not a template
/// - "auto"/other → black glyph as a template, so macOS adapts it
fn icon_for(theme: &str) -> (&'static [u8], bool) {
    match theme {
        "color" => (ICON_COLOR, false),
        "mono-light" => (ICON_WHITE, false),
        "mono-dark" => (ICON_BLACK, false),
        _ => (ICON_BLACK, true),
    }
}

fn decode(bytes: &[u8]) -> Option<Icon> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = img.dimensions();
    let rgba = img.into_rgba8().into_vec();
    Icon::from_rgba(rgba, w, h).ok()
}

/// Build the menu-bar item + menu and start the event pump. MUST run on the
/// main thread (call via `slint::invoke_from_event_loop`).
pub fn create(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    theme_override: &str,
) {
    let (bytes, is_template) = icon_for(theme_override);
    let Some(icon) = decode(bytes) else {
        log::error!("[tray] failed to decode menu-bar icon");
        return;
    };

    let menu = Menu::new();
    let play = MenuItem::with_id("play_pause", "Play/Pause", true, None);
    let next = MenuItem::with_id("next", "Next Track", true, None);
    let prev = MenuItem::with_id("previous", "Previous Track", true, None);
    let sep1 = PredefinedMenuItem::separator();
    let show = MenuItem::with_id("show_hide", "Show/Hide Window", true, None);
    let sep2 = PredefinedMenuItem::separator();
    let quit = MenuItem::with_id("quit", "Quit QBZ", true, None);
    if let Err(e) =
        menu.append_items(&[&play, &next, &prev, &sep1, &show, &sep2, &quit])
    {
        log::error!("[tray] failed to build menu: {e}");
        return;
    }

    let tray = match TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("QBZ - Music Player")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
    {
        Ok(t) => t,
        Err(e) => {
            log::error!("[tray] menu-bar item build failed: {e}");
            return;
        }
    };
    #[cfg(target_os = "macos")]
    tray.set_icon_as_template(is_template);
    #[cfg(not(target_os = "macos"))]
    let _ = is_template;

    TRAY.with(|t| *t.borrow_mut() = Some(tray));

    // Drain tray + menu events on the main thread.
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(120),
        move || pump(&runtime, &weak, &handle),
    );
    PUMP_TIMER.with(|t| *t.borrow_mut() = Some(timer));

    log::info!("[tray] menu-bar item initialized (theme={theme_override})");
}

fn pump(runtime: &Runtime, weak: &slint::Weak<AppWindow>, handle: &tokio::runtime::Handle) {
    while let Ok(ev) = MenuEvent::receiver().try_recv() {
        match ev.id.0.as_str() {
            "play_pause" => {
                super::dispatch_play_pause(runtime.clone(), weak.clone(), handle.clone())
            }
            "next" => super::dispatch_next(runtime.clone(), weak.clone(), handle.clone()),
            "previous" => super::dispatch_previous(runtime.clone(), weak.clone(), handle.clone()),
            "show_hide" => super::toggle_window(weak),
            "quit" => super::quit(),
            other => log::debug!("[tray] unhandled menu id '{other}'"),
        }
    }
    while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = ev
        {
            super::toggle_window(weak);
        }
    }
}

/// Re-theme the live menu-bar icon (called on the main thread).
pub fn set_icon_theme(theme: &str) {
    let (bytes, is_template) = icon_for(theme);
    let Some(icon) = decode(bytes) else { return };
    TRAY.with(|t| {
        if let Some(tray) = t.borrow().as_ref() {
            let _ = tray.set_icon(Some(icon));
            #[cfg(target_os = "macos")]
            tray.set_icon_as_template(is_template);
            #[cfg(not(target_os = "macos"))]
            let _ = is_template;
        }
    });
}

/// Switch the macOS activation policy: `.accessory` hides the Dock icon
/// (menu-bar-only), `.regular` keeps it (Spotify default). Must run on the
/// main thread. No-op on Windows.
#[cfg(target_os = "macos")]
pub fn set_dock_icon_hidden(hidden: bool) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let policy = if hidden {
        NSApplicationActivationPolicy::Accessory
    } else {
        NSApplicationActivationPolicy::Regular
    };
    app.setActivationPolicy(policy);
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_icon_hidden(_hidden: bool) {}
