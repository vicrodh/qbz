//! macOS menu-bar tray (`NSStatusItem`), hand-rolled on objc2 0.5.
//!
//! Why not `tray-icon`/`muda`: Slint's `i-slint-backend-winit` bundles its own
//! `muda` and registers the `MudaMenuItem` objc class. A second `muda` (from
//! tray-icon) registering the same class either silently fails to dispatch the
//! menu item target-action (objc2 0.6) or panics at startup ("could not create
//! new class MudaMenuItem", objc2 0.5). So we build the `NSStatusItem` + its
//! `NSMenu` directly on objc2 0.5 / objc2-app-kit 0.2 — the SAME objc2 era
//! winit 0.30 uses — so the menu lives in winit's `NSApplication` runtime and
//! `[NSApp sendAction:to:from:]` routes our items' target-action.
//!
//! Everything here is main-thread only: the `NSStatusItem`, the menu, and the
//! `QbzTrayMenuTarget` instance are `!Send` (`thread_local!`). `create` is
//! invoked via `slint::invoke_from_event_loop` (main thread). The action
//! callback reads the clicked item's `tag` and routes to the shared dispatch
//! helpers in the parent `tray` module (those marshal back onto the Slint loop
//! / tokio runtime themselves; the captured `slint::Weak`, `Runtime`, and
//! `tokio::runtime::Handle` are all `Send + Sync`, kept in a process-global
//! `OnceLock`).
//!
//! Click behavior matches the Tauri tray (`show_menu_on_left_click(false)`):
//! the menu is NOT permanently attached to the status item. Instead the status
//! button gets its own target-action firing on both left and right mouse-up.
//! LEFT-click toggles the window; RIGHT-click (or control-click) pops the menu
//! up transiently (set menu → `performClick` → clear menu, the non-deprecated
//! replacement for `popUpStatusItemMenu:`).

use std::cell::RefCell;
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEventMask, NSEventModifierFlags, NSEventType,
    NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{MainThreadMarker, NSData, NSInteger, NSSize, NSString};

use super::Runtime;
use crate::AppWindow;

// 44px assets (= 22pt @2x menu bar). Filename trap (shared with Linux):
// `tray-dark-*` holds the WHITE glyph, `tray-light-*` holds the BLACK glyph.
const ICON_COLOR: &[u8] = include_bytes!("../../icons/tray-color-44.png");
const ICON_WHITE: &[u8] = include_bytes!("../../icons/tray-dark-44.png");
const ICON_BLACK: &[u8] = include_bytes!("../../icons/tray-light-44.png");

// Menu item tags → actions.
const TAG_PLAY_PAUSE: NSInteger = 1;
const TAG_NEXT: NSInteger = 2;
const TAG_PREVIOUS: NSInteger = 3;
const TAG_SHOW_HIDE: NSInteger = 4;
const TAG_QUIT: NSInteger = 5;

/// Process-global dispatch context. Set once by `create`. The captured types
/// (`Runtime` = `Arc<..>`, `slint::Weak`, `tokio::runtime::Handle`) are all
/// `Send + Sync`, so reading this from the AppKit action callback (main thread)
/// is sound.
static CTX: OnceLock<(Runtime, slint::Weak<AppWindow>, tokio::runtime::Handle)> = OnceLock::new();

thread_local! {
    // Kept alive for the tray's lifetime; dropping the status item removes it
    // from the menu bar. Both are `!Send`, main-thread only.
    static STATUS_ITEM: RefCell<Option<Retained<NSStatusItem>>> = const { RefCell::new(None) };
    static MENU_TARGET: RefCell<Option<Retained<QbzTrayMenuTarget>>> = const { RefCell::new(None) };
    // The menu is NOT permanently attached to the status item (that would make
    // a left-click pop it). It lives here and is only flashed onto the status
    // item for the duration of a right/control-click pop-up.
    static MENU: RefCell<Option<Retained<NSMenu>>> = const { RefCell::new(None) };
}

declare_class!(
    struct QbzTrayMenuTarget;

    unsafe impl ClassType for QbzTrayMenuTarget {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "QbzTrayMenuTarget";
    }

    impl DeclaredClass for QbzTrayMenuTarget {
        type Ivars = ();
    }

    unsafe impl QbzTrayMenuTarget {
        #[method(onMenuItem:)]
        fn on_menu_item(&self, sender: Option<&NSMenuItem>) {
            let tag = sender.map(|s| unsafe { s.tag() }).unwrap_or(0);
            dispatch_tag(tag);
        }

        // Fires on left AND right mouse-up of the status-bar button (see
        // `sendActionOn` in `create`). We inspect the current event to route:
        // right-click / control-click → pop the menu; plain left-click → toggle.
        #[method(onStatusButton:)]
        fn on_status_button(&self, _sender: Option<&AnyObject>) {
            handle_status_click();
        }
    }
);

impl QbzTrayMenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(());
        unsafe { msg_send_id![super(this), init] }
    }
}

/// Route a clicked menu item's tag to the shared dispatch helpers.
fn dispatch_tag(tag: NSInteger) {
    log::info!("[tray] menu item activated: tag={tag}");
    let Some((runtime, weak, handle)) = CTX.get() else {
        log::warn!("[tray] dispatch_tag: context not initialized");
        return;
    };
    match tag {
        TAG_PLAY_PAUSE => {
            super::dispatch_play_pause(runtime.clone(), weak.clone(), handle.clone())
        }
        TAG_NEXT => super::dispatch_next(runtime.clone(), weak.clone(), handle.clone()),
        TAG_PREVIOUS => super::dispatch_previous(runtime.clone(), weak.clone(), handle.clone()),
        TAG_SHOW_HIDE => super::toggle_window(weak),
        TAG_QUIT => super::quit(),
        other => log::debug!("[tray] unhandled menu tag {other}"),
    }
}

/// Status-bar button click router. Reads the current AppKit event: a
/// right-click or control-click pops the menu, a plain left-click toggles the
/// window. Main thread only (it's an AppKit action callback).
fn handle_status_click() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let (is_right, is_ctrl) = match app.currentEvent() {
        Some(ev) => {
            let ty = unsafe { ev.r#type() };
            let mods = unsafe { ev.modifierFlags() };
            (
                ty == NSEventType::RightMouseUp,
                mods.contains(NSEventModifierFlags::NSEventModifierFlagControl),
            )
        }
        None => (false, false),
    };

    if is_right || is_ctrl {
        pop_up_menu(mtm);
    } else if let Some((_, weak, _)) = CTX.get() {
        super::toggle_window(weak);
    }
}

/// Pop the tray menu transiently. Non-deprecated replacement for
/// `popUpStatusItemMenu:`: flash the menu onto the status item, simulate a
/// click (which opens it modally), then detach it so a left-click doesn't open
/// it. Main thread only.
fn pop_up_menu(mtm: MainThreadMarker) {
    STATUS_ITEM.with(|s| {
        let Some(status_item) = s.borrow().as_ref().cloned() else {
            return;
        };
        MENU.with(|m| {
            if let Some(menu) = m.borrow().as_ref() {
                unsafe { status_item.setMenu(Some(menu)) };
                if let Some(button) = unsafe { status_item.button(mtm) } {
                    unsafe { button.performClick(None) };
                }
                unsafe { status_item.setMenu(None) };
            }
        });
    });
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

/// Build an `NSImage` from PNG bytes, marking it a template image when asked.
fn make_image(bytes: &[u8], is_template: bool) -> Option<Retained<NSImage>> {
    let data = NSData::with_bytes(bytes);
    let image = NSImage::initWithData(NSImage::alloc(), &data)?;
    unsafe { image.setTemplate(is_template) };
    // The PNG assets are 44px (22pt @2x). Without an explicit point size the
    // menu bar renders them at native pixel size → a giant icon. Pin to the
    // standard menu-bar glyph box (18pt; the bar is 22pt tall).
    unsafe { image.setSize(NSSize::new(18.0, 18.0)) };
    Some(image)
}

/// Apply the resolved icon to the status item's button.
fn apply_icon(status_item: &NSStatusItem, theme: &str, mtm: MainThreadMarker) {
    let (bytes, is_template) = icon_for(theme);
    let Some(image) = make_image(bytes, is_template) else {
        log::error!("[tray] failed to decode menu-bar icon");
        return;
    };
    if let Some(button) = unsafe { status_item.button(mtm) } {
        unsafe { button.setImage(Some(&image)) };
    }
}

/// Build the menu-bar item + menu. MUST run on the main thread (call via
/// `slint::invoke_from_event_loop`).
pub fn create(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    theme_override: &str,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        log::error!("[tray] create called off the main thread");
        return;
    };

    // Store the dispatch context (only the first init wins).
    let _ = CTX.set((runtime, weak, handle));

    // The action target. Held alive in a thread_local so the menu's weak
    // target reference stays valid.
    let target = QbzTrayMenuTarget::new(mtm);
    let target_obj: &AnyObject = &target;
    let action = sel!(onMenuItem:);

    // Build the menu: 3 transport items, separator, show/hide, separator, quit.
    let menu = NSMenu::new(mtm);
    let empty_key = NSString::from_str("");
    let make_item = |title: &str, tag: NSInteger| -> Retained<NSMenuItem> {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str(title),
                Some(action),
                &empty_key,
            )
        };
        unsafe {
            item.setTarget(Some(target_obj));
            item.setTag(tag);
            item.setEnabled(true);
        }
        item
    };

    menu.addItem(&make_item("Play/Pause", TAG_PLAY_PAUSE));
    menu.addItem(&make_item("Next Track", TAG_NEXT));
    menu.addItem(&make_item("Previous Track", TAG_PREVIOUS));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&make_item("Show/Hide Window", TAG_SHOW_HIDE));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&make_item("Quit QBZ", TAG_QUIT));

    // Build the status item and wire the icon.
    let status_bar = unsafe { NSStatusBar::systemStatusBar() };
    let status_item = unsafe { status_bar.statusItemWithLength(NSVariableStatusItemLength) };
    apply_icon(&status_item, theme_override, mtm);

    // Do NOT attach the menu permanently (that makes any click open it). Give
    // the status button its own action that fires on left AND right mouse-up;
    // `handle_status_click` decides toggle-vs-menu. The menu is kept in the
    // MENU thread_local and only flashed on for a right-click pop-up.
    if let Some(button) = unsafe { status_item.button(mtm) } {
        unsafe {
            button.setTarget(Some(target_obj));
            button.setAction(Some(sel!(onStatusButton:)));
            button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
        }
    }

    STATUS_ITEM.with(|s| *s.borrow_mut() = Some(status_item));
    MENU_TARGET.with(|t| *t.borrow_mut() = Some(target));
    MENU.with(|m| *m.borrow_mut() = Some(menu));

    // A bare `cargo run` binary is NOT a bundled .app. Without an explicit
    // Regular activation policy + activation, macOS treats the app as a
    // background process and `[NSApp sendAction:]` may not route the menu item
    // target-action. Force Regular + active.
    ensure_regular_active_app(mtm);

    log::info!("[tray] menu-bar item initialized (theme={theme_override})");
}

/// Re-theme the live menu-bar icon (called on the main thread).
pub fn set_icon_theme(theme: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    STATUS_ITEM.with(|s| {
        if let Some(status_item) = s.borrow().as_ref() {
            apply_icon(status_item, theme, mtm);
        }
    });
}

/// Force the app to a Regular, active application so macOS dispatches the
/// `NSStatusItem` menu-item actions. Main thread only.
fn ensure_regular_active_app(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
}

/// Switch the macOS activation policy: `.accessory` hides the Dock icon
/// (menu-bar-only), `.regular` keeps it (Spotify default). Must run on the
/// main thread.
pub fn set_dock_icon_hidden(hidden: bool) {
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
