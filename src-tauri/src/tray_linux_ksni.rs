//! Linux system tray via `ksni` (StatusNotifierItem).
//!
//! Tauri's default tray on Linux is backed by libayatana-appindicator, which
//! by design does not dispatch primary-click events to clients — left-click
//! always shows the menu. See issue #310.
//!
//! `ksni` is a pure-Rust SNI implementation that exposes the full protocol
//! including `Activate` (left-click), `SecondaryActivate` (middle-click),
//! `Scroll` (wheel) and `ContextMenu` (right-click → menu). Same SNI
//! compatibility as the old tray (both need an SNI-aware panel: KDE native,
//! GNOME w/ extension, XFCE/Cinnamon/MATE/Budgie, wlroots tray widgets).
//!
//! This module runs a ksni `TrayService` in a dedicated background thread
//! (via the blocking API). All callbacks receive `&mut QbzTray` and close
//! over the Tauri `AppHandle` to emit events and manipulate the main window.
//!
//! The returned `LinuxTrayHandle` is stored in Tauri state so the rest of
//! the backend (metadata setter, playback poll loop) can push live updates
//! into the SNI tooltip — replicating the rich tooltip the Plasma media
//! plasmoid shows on hover.

use std::sync::{
    mpsc::{self, Sender},
    Arc, Mutex,
};

use image::GenericImageView;
use ksni::{
    blocking::TrayMethods,
    menu::StandardItem,
    Icon, MenuItem, Orientation, ToolTip, Tray,
};
use tauri::{AppHandle, Emitter, Manager};

// Multiple pixmap sizes per StatusNotifierItem spec — panels pick the best
// match for their bar height (22 = base, 44/64 = HiDPI). All are monochromatic
// silhouettes of the qbz glyph: black on transparent for light panels, white
// for dark panels. Generated from icons/icon-symbolic.svg via Inkscape.
const TRAY_ICON_LIGHT_22: &[u8] = include_bytes!("../icons/tray-light-22.png");
const TRAY_ICON_LIGHT_32: &[u8] = include_bytes!("../icons/tray-light-32.png");
const TRAY_ICON_LIGHT_44: &[u8] = include_bytes!("../icons/tray-light-44.png");
const TRAY_ICON_LIGHT_64: &[u8] = include_bytes!("../icons/tray-light-64.png");
const TRAY_ICON_DARK_22: &[u8] = include_bytes!("../icons/tray-dark-22.png");
const TRAY_ICON_DARK_32: &[u8] = include_bytes!("../icons/tray-dark-32.png");
const TRAY_ICON_DARK_44: &[u8] = include_bytes!("../icons/tray-dark-44.png");
const TRAY_ICON_DARK_64: &[u8] = include_bytes!("../icons/tray-dark-64.png");

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
fn decode_pixmap(bytes: &[u8]) -> Result<Icon, String> {
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

/// Resolve which icon variant to load. `theme_override` accepts "auto",
/// "light", "dark"; anything else falls through to system detection. The
/// override exists for desktops like GNOME where the top bar is dark even
/// when the system theme reports light, leaving auto-detected dark
/// glyphs invisible.
fn resolve_dark_tray(theme_override: Option<&str>) -> bool {
    match theme_override {
        Some("light") => false,
        Some("dark") => true,
        _ => prefer_dark_tray(),
    }
}

/// Decode all monochromatic pixmap sizes for the active theme. Panels pick the
/// best size from the list; supplying 22/32/44/64 covers standard SNI bar
/// heights and HiDPI variants.
fn decode_tray_icons(theme_override: Option<&str>) -> Result<Vec<Icon>, String> {
    let sources: [&[u8]; 4] = if resolve_dark_tray(theme_override) {
        [TRAY_ICON_DARK_22, TRAY_ICON_DARK_32, TRAY_ICON_DARK_44, TRAY_ICON_DARK_64]
    } else {
        [TRAY_ICON_LIGHT_22, TRAY_ICON_LIGHT_32, TRAY_ICON_LIGHT_44, TRAY_ICON_LIGHT_64]
    };
    sources.iter().map(|b| decode_pixmap(b)).collect()
}

/// Now-playing info shown in the tooltip. Cleared when no track is loaded.
#[derive(Clone, Debug, Default)]
struct NowPlaying {
    title: String,
    artist: String,
    album: String,
}

struct QbzTray {
    app: AppHandle,
    icons: Vec<Icon>,
    now_playing: Option<NowPlaying>,
    is_playing: bool,
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

    fn emit_payload<S: serde::Serialize + Clone>(&self, event: &str, payload: S) {
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.emit(event, payload);
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
        // Intentionally empty: SNI panels (KDE Plasma especially) prefer
        // IconName over IconPixmap when both are present, and resolving the
        // app id `com.blitzfc.qbz` against the icon theme picks up the full
        // colour app icon instead of our themed monochrome glyph (issue #362).
        // An empty name forces panels to render IconPixmap directly.
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        // Plasma's media plasmoid renders a multi-line tooltip with the track
        // title bolded on top and secondary lines below. SNI's ToolTip has a
        // (title, description) split that maps onto the same visual: panels
        // typically render `title` bold and `description` as wrapping body
        // text that respects '\n'.
        let (title, description) = match &self.now_playing {
            Some(np) => {
                let header = if np.title.is_empty() {
                    "QBZ".to_string()
                } else {
                    np.title.clone()
                };
                let mut lines: Vec<String> = Vec::with_capacity(3);
                if !np.artist.is_empty() {
                    lines.push(format!("by {}", np.artist));
                }
                if !np.album.is_empty() {
                    lines.push(np.album.clone());
                }
                lines.push(if self.is_playing {
                    "Middle-click to pause".to_string()
                } else {
                    "Middle-click to play".to_string()
                });
                lines.push("Scroll to adjust volume".to_string());
                (header, lines.join("\n"))
            }
            None => (
                "QBZ".to_string(),
                "Music Player\nNothing playing".to_string(),
            ),
        };
        ToolTip {
            title,
            description,
            icon_name: String::new(),
            icon_pixmap: vec![],
        }
    }

    /// Primary click (left) — toggle main window visibility.
    fn activate(&mut self, _x: i32, _y: i32) {
        log::info!("[tray] primary activate (left click)");
        self.toggle_window();
    }

    /// Secondary click (middle) — play/pause, mirroring the Plasma media
    /// plasmoid behaviour. When nothing is loaded the frontend simply ignores
    /// the toggle, so emitting unconditionally is safe.
    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        log::info!("[tray] secondary activate (middle click) -> play/pause");
        self.emit_to_main("tray:play_pause");
    }

    /// Mouse wheel — adjust volume in 5%-per-notch steps. Most panels (KDE
    /// Plasma, GNOME Shell appindicator) report ±120 per wheel notch
    /// following the X11/wayland convention; touch-pad scrolls produce
    /// smaller fractional deltas. We normalise by dividing by 120 and fall
    /// back to `signum()` so very small deltas still register one tick.
    fn scroll(&mut self, delta: i32, orientation: Orientation) {
        if !matches!(orientation, Orientation::Vertical) || delta == 0 {
            return;
        }
        let mut ticks = delta / 120;
        if ticks == 0 {
            ticks = delta.signum();
        }
        log::debug!("[tray] scroll delta={} ticks={}", delta, ticks);
        // Positive ticks = wheel-up = volume up, matching Plasma plasmoid.
        self.emit_payload("tray:volume_delta", ticks);
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

/// Updates dispatched from the rest of the backend into the ksni worker.
///
/// We cannot call `ksni::blocking::Handle::update` directly from a Tauri
/// command or from the playback poll loop — `ksni` 0.3 with `feature =
/// "blocking"` wraps every update in `Runtime::block_on`, which panics when
/// invoked from within an existing tokio runtime (which is exactly what
/// Tauri commands and `tauri::async_runtime::spawn` tasks run on). The
/// panic is silently swallowed by the runtime, so updates appear to
/// succeed but the tooltip never changes.
///
/// To avoid the nested-runtime issue we serialise updates over a
/// `std::sync::mpsc` channel and apply them from a dedicated `std::thread`
/// that lives entirely outside any tokio context.
enum TrayUpdate {
    SetTrack {
        title: String,
        artist: String,
        album: String,
    },
    ClearTrack,
    SetPlaying(bool),
    /// Re-decode pixmaps for the requested theme override and push them
    /// to the live ksni service. Triggers a `NewIcon` SNI signal so
    /// panels re-fetch the icon without restart.
    SetIconTheme(String),
}

/// Cross-thread handle to the live ksni tray. Cloneable; mutators just
/// forward to the worker thread, so they are safe to call from any
/// async context. When the tray is disabled or failed to start, the
/// inner sender stays `None` and every mutator becomes a no-op.
#[derive(Clone)]
pub struct LinuxTrayHandle {
    sender: Arc<Mutex<Option<Sender<TrayUpdate>>>>,
}

impl LinuxTrayHandle {
    pub fn empty() -> Self {
        Self {
            sender: Arc::new(Mutex::new(None)),
        }
    }

    fn install(&self, handle: ksni::blocking::Handle<QbzTray>) {
        let (tx, rx) = mpsc::channel::<TrayUpdate>();
        std::thread::Builder::new()
            .name("qbz-tray-updater".into())
            .spawn(move || {
                while let Ok(msg) = rx.recv() {
                    match msg {
                        TrayUpdate::SetTrack {
                            title,
                            artist,
                            album,
                        } => {
                            log::debug!(
                                "[tray] tooltip update -> {} / {} / {}",
                                title,
                                artist,
                                album
                            );
                            handle.update(move |tray| {
                                tray.now_playing = Some(NowPlaying {
                                    title,
                                    artist,
                                    album,
                                });
                            });
                        }
                        TrayUpdate::ClearTrack => {
                            log::debug!("[tray] tooltip cleared");
                            handle.update(|tray| {
                                tray.now_playing = None;
                                tray.is_playing = false;
                            });
                        }
                        TrayUpdate::SetPlaying(is_playing) => {
                            handle.update(move |tray| {
                                tray.is_playing = is_playing;
                            });
                        }
                        TrayUpdate::SetIconTheme(theme) => {
                            log::info!("[tray] icon theme override -> {}", theme);
                            match decode_tray_icons(Some(&theme)) {
                                Ok(new_icons) => {
                                    handle.update(move |tray| {
                                        tray.icons = new_icons;
                                    });
                                }
                                Err(e) => {
                                    log::error!(
                                        "[tray] failed to decode icons for theme '{}': {}",
                                        theme,
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
                log::debug!("[tray] updater thread exiting");
            })
            .expect("spawn tray updater thread");
        if let Ok(mut guard) = self.sender.lock() {
            *guard = Some(tx);
        }
    }

    fn send(&self, msg: TrayUpdate) {
        let guard = match self.sender.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(msg);
        }
    }

    pub fn set_track(&self, title: String, artist: String, album: String) {
        self.send(TrayUpdate::SetTrack {
            title,
            artist,
            album,
        });
    }

    pub fn clear_track(&self) {
        self.send(TrayUpdate::ClearTrack);
    }

    pub fn set_playing(&self, is_playing: bool) {
        self.send(TrayUpdate::SetPlaying(is_playing));
    }

    pub fn set_icon_theme(&self, theme: String) {
        self.send(TrayUpdate::SetIconTheme(theme));
    }
}

/// Initialize the Linux ksni tray service. Spawns a background thread that
/// owns the SNI service and returns a cloneable handle so live tooltip
/// updates can be pushed in from the rest of the backend.
///
/// `theme_override` is the persisted user preference: "auto" (system
/// detection), "light", or "dark". GNOME users often pick "light" because
/// its top bar is permanently dark even when the system theme isn't.
pub fn init(
    app: &AppHandle,
    theme_override: Option<&str>,
) -> Result<LinuxTrayHandle, Box<dyn std::error::Error>> {
    log::info!(
        "Initializing ksni tray (Linux, SNI primary-activate enabled, theme={:?})",
        theme_override
    );

    let icons = decode_tray_icons(theme_override)?;
    let tray = QbzTray {
        app: app.clone(),
        icons,
        now_playing: None,
        is_playing: false,
    };

    // Flatpak requires disabling the well-known DBus name because the sandbox
    // cannot own arbitrary bus names (Chromium and others hit the same issue).
    let handle = if is_flatpak() {
        log::info!("[tray] Flatpak detected — spawning ksni without DBus well-known name");
        tray.disable_dbus_name(true).spawn()?
    } else {
        tray.spawn()?
    };

    let live = LinuxTrayHandle::empty();
    live.install(handle);

    log::info!("ksni tray initialized");
    Ok(live)
}
