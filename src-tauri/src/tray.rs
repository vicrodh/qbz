//! System tray icon implementation for QBZ
//!
//! Provides system tray integration with playback controls and window management.

use image::GenericImageView;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

// Embed tray icon at compile time (transparent background)
const TRAY_ICON_PNG: &[u8] = include_bytes!("../icons/tray.png");

/// Decode PNG and convert to RGBA for Tauri Image
fn load_tray_icon() -> Image<'static> {
    let img = image::load_from_memory(TRAY_ICON_PNG)
        .expect("Failed to decode tray icon PNG");
    let (width, height) = img.dimensions();
    let rgba = img.into_rgba8().into_raw();
    Image::new_owned(rgba, width, height)
}

/// Initialize the system tray icon with menu
pub fn init_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    log::info!("Initializing system tray icon");

    // Create menu items
    let play_pause = MenuItem::with_id(app, "play_pause", "Play/Pause", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Next Track", true, None::<&str>)?;
    let previous = MenuItem::with_id(app, "previous", "Previous Track", true, None::<&str>)?;
    let separator1 = PredefinedMenuItem::separator(app)?;
    let show_hide = MenuItem::with_id(app, "show_hide", "Show/Hide Window", true, None::<&str>)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit QBZ", true, None::<&str>)?;

    // Build tray menu
    let tray_menu = Menu::with_items(
        app,
        &[
            &play_pause,
            &next,
            &previous,
            &separator1,
            &show_hide,
            &separator2,
            &quit,
        ],
    )?;

    // Load custom tray icon (with transparent background)
    let tray_icon = load_tray_icon();

    // Build and display tray icon
    let _tray = TrayIconBuilder::new()
        .icon(tray_icon)
        .menu(&tray_menu)
        .tooltip("QBZ - Music Player")
        .show_menu_on_left_click(false) // Left click toggles window, right click shows menu
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            log::info!("Tray menu event: {}", id);

            match id {
                "play_pause" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.emit("tray:play_pause", ());
                    }
                }
                "next" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.emit("tray:next", ());
                    }
                }
                "previous" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.emit("tray:previous", ());
                    }
                }
                "show_hide" => {
                    if let Some(window) = app.get_webview_window("main") {
                        if window.is_visible().unwrap_or(true) {
                            let _ = window.hide();
                        } else {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                }
                "quit" => {
                    log::info!("Quit from tray menu");
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            match event {
                // Left click toggles window visibility
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => {
                    log::info!("Tray icon left-click");
                    let app = tray.app_handle();
                    if let Some(window) = app.get_webview_window("main") {
                        if window.is_visible().unwrap_or(true) {
                            let _ = window.hide();
                        } else {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                }
                // Double click always shows and focuses
                TrayIconEvent::DoubleClick { .. } => {
                    log::info!("Tray icon double-click");
                    let app = tray.app_handle();
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
                _ => {}
            }
        })
        .build(app)?;

    log::info!("System tray icon initialized");
    Ok(())
}
