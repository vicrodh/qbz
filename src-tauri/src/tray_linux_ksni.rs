//! Linux system tray via `ksni` (StatusNotifierItem).
//!
//! Tauri's default tray on Linux is backed by libayatana-appindicator, which
//! by design does not dispatch primary-click events to clients — left-click
//! always shows the menu. See issue #310.
//!
//! `ksni` is a pure-Rust SNI implementation that exposes the full protocol
//! including `Activate` (left-click), `SecondaryActivate` (middle-click) and
//! `ContextMenu` (right-click → menu). Same SNI compatibility as the old
//! tray (both need an SNI-aware panel: KDE native, GNOME w/ extension,
//! XFCE/Cinnamon/MATE/Budgie, wlroots tray widgets).
//!
//! This module runs a ksni `TrayService` in a dedicated background thread
//! (via the blocking API). All callbacks receive `&mut QbzTray` and close
//! over the Tauri `AppHandle` to emit events and manipulate the main window.

use image::GenericImageView;
use ksni::{
    blocking::TrayMethods,
    menu::StandardItem,
    Icon, MenuItem, ToolTip, Tray,
};
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ICON_DARK_PNG: &[u8] = include_bytes!("../icons/tray-dark.png");
const TRAY_ICON_LIGHT_PNG: &[u8] = include_bytes!("../icons/tray-light.png");

fn is_flatpak() -> bool {
    std::env::var("FLATPAK_ID").is_ok() || std::path::Path::new("/.flatpak-info").exists()
}

/// Detect whether the system prefers a dark color scheme. Used to pick the
/// matching tray icon variant (white glyph for dark trays, black for light).
/// Tries (in order): GNOME `color-scheme`, GTK `prefer-dark-theme`,
/// KDE `ColorScheme`. Defaults to `false` (light) when nothing matches.
fn prefer_dark_tray() -> bool {
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        if String::from_utf8_lossy(&out.stdout).contains("prefer-dark") {
            return true;
        }
    }
    if let Some(config) = dirs::config_dir() {
        for variant in ["gtk-4.0", "gtk-3.0"] {
            let path = config.join(variant).join("settings.ini");
            if let Ok(content) = std::fs::read_to_string(&path) {
                for line in content.lines() {
                    if let Some(rest) = line.trim().strip_prefix("gtk-application-prefer-dark-theme") {
                        let v = rest.trim_start_matches(['=', ' ']);
                        return v.starts_with('1') || v.starts_with("true");
                    }
                }
            }
        }
        if let Ok(content) = std::fs::read_to_string(config.join("kdeglobals")) {
            for line in content.lines() {
                if let Some(rest) = line.trim().strip_prefix("ColorScheme") {
                    return rest.trim_start_matches(['=', ' ']).to_lowercase().contains("dark");
                }
            }
        }
    }
    false
}

/// Convert an embedded RGBA PNG to the ARGB32 big-endian layout ksni expects.
fn decode_tray_icon() -> Result<Icon, String> {
    let bytes = if prefer_dark_tray() { TRAY_ICON_DARK_PNG } else { TRAY_ICON_LIGHT_PNG };
    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("decode tray icon: {e}"))?;
    let (width, height) = img.dimensions();
    let mut data = img.into_rgba8().into_vec();
    // ksni spec: ARGB32 with A, R, G, B order per pixel. `image` gives us
    // RGBA; rotate_right(1) moves the alpha byte from the last slot to the
    // first.
    for pixel in data.chunks_exact_mut(4) {
        pixel.rotate_right(1);
    }
    Ok(Icon {
        width: width as i32,
        height: height as i32,
        data,
    })
}

struct QbzTray {
    app: AppHandle,
    icon: Icon,
}

impl QbzTray {
    fn toggle_window(&self) {
        if let Some(window) = self.app.get_webview_window("main") {
            let is_visible = window.is_visible().unwrap_or(false);
            let is_minimized = window.is_minimized().unwrap_or(false);
            if is_visible && !is_minimized {
                log::info!("[tray] hiding window");
                let _ = window.hide();
            } else {
                log::info!("[tray] showing window");
                let _ = window.show();
                if is_minimized {
                    let _ = window.unminimize();
                }
                let _ = window.set_focus();
            }
        }
    }

    fn emit_to_main(&self, event: &str) {
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.emit(event, ());
        }
    }
}

impl Tray for QbzTray {
    fn id(&self) -> String {
        "com.blitzfc.qbz".into()
    }

    fn title(&self) -> String {
        "QBZ".into()
    }

    fn icon_name(&self) -> String {
        // Fall back to the themed icon name when the panel prefers it over
        // pixmap data (some GNOME extensions, certain remote X11 setups).
        "com.blitzfc.qbz".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![self.icon.clone()]
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "QBZ".into(),
            description: "Music Player".into(),
            icon_name: String::new(),
            icon_pixmap: vec![],
        }
    }

    /// Primary click (left) — the headline feature of switching to ksni.
    fn activate(&mut self, _x: i32, _y: i32) {
        log::info!("[tray] primary activate (left click)");
        self.toggle_window();
    }

    /// Secondary click (middle) — same as left for symmetry; users who were
    /// middle-clicking on the old build to open the menu still get a useful
    /// behavior.
    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        log::info!("[tray] secondary activate (middle click)");
        self.toggle_window();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Play/Pause".into(),
                activate: Box::new(|this: &mut Self| this.emit_to_main("tray:play_pause")),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Next Track".into(),
                activate: Box::new(|this: &mut Self| this.emit_to_main("tray:next")),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Previous Track".into(),
                activate: Box::new(|this: &mut Self| this.emit_to_main("tray:previous")),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Show/Hide Window".into(),
                activate: Box::new(|this: &mut Self| this.toggle_window()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit QBZ".into(),
                activate: Box::new(|this: &mut Self| {
                    log::info!("[tray] quit requested");
                    this.app.exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Initialize the Linux ksni tray service. Spawns a background thread that
/// owns the SNI service; the returned Handle is intentionally leaked because
/// the tray is a singleton that lives for the app's lifetime (dropping the
/// handle would tear down the tray).
pub fn init(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    log::info!("Initializing ksni tray (Linux, SNI primary-activate enabled)");

    let icon = decode_tray_icon()?;
    let tray = QbzTray {
        app: app.clone(),
        icon,
    };

    // Flatpak requires disabling the well-known DBus name because the sandbox
    // cannot own arbitrary bus names (Chromium and others hit the same issue).
    let handle = if is_flatpak() {
        log::info!("[tray] Flatpak detected — spawning ksni without DBus well-known name");
        tray.disable_dbus_name(true).spawn()?
    } else {
        tray.spawn()?
    };

    // Keep the service alive for the app lifetime. We don't need to update the
    // tray dynamically yet; if we do in the future (e.g., change icon when
    // playing), we'd store this handle in AppState instead.
    std::mem::forget(handle);

    log::info!("ksni tray initialized");
    Ok(())
}
