//! Linux system tray via `ksni` (StatusNotifierItem).
//!
//! Faithful port of `src-tauri/src/tray_linux_ksni.rs`. The icon decoding,
//! theme resolution, tooltip composition and the updater-thread pattern are
//! byte-for-byte equivalent. The ONLY behavioural difference: tray actions
//! drive the playback controller + winit window directly (no webview to emit
//! events to) — see the `super::dispatch_*` / `super::*_window` helpers.

use std::sync::{
    mpsc::{self, Sender},
    Arc, Mutex,
};

use image::GenericImageView;
use ksni::{blocking::TrayMethods, menu::StandardItem, Icon, MenuItem, Orientation, ToolTip, Tray};

use super::Runtime;
use crate::AppWindow;

// Multiple pixmap sizes per StatusNotifierItem spec — panels pick the best
// match for their bar height (22 = base, 32/44/64 = HiDPI).
//
// Legacy filename note (shared with the Tauri assets): `tray-light-*` holds
// the BLACK glyph (for LIGHT panels) and `tray-dark-*` holds the WHITE glyph.
// The constants use glyph-colour names so the mapping is explicit.
const TRAY_ICON_MONO_BLACK_22: &[u8] = include_bytes!("../../icons/tray-light-22.png");
const TRAY_ICON_MONO_BLACK_32: &[u8] = include_bytes!("../../icons/tray-light-32.png");
const TRAY_ICON_MONO_BLACK_44: &[u8] = include_bytes!("../../icons/tray-light-44.png");
const TRAY_ICON_MONO_BLACK_64: &[u8] = include_bytes!("../../icons/tray-light-64.png");
const TRAY_ICON_MONO_WHITE_22: &[u8] = include_bytes!("../../icons/tray-dark-22.png");
const TRAY_ICON_MONO_WHITE_32: &[u8] = include_bytes!("../../icons/tray-dark-32.png");
const TRAY_ICON_MONO_WHITE_44: &[u8] = include_bytes!("../../icons/tray-dark-44.png");
const TRAY_ICON_MONO_WHITE_64: &[u8] = include_bytes!("../../icons/tray-dark-64.png");
const TRAY_ICON_COLOR_22: &[u8] = include_bytes!("../../icons/tray-color-22.png");
const TRAY_ICON_COLOR_32: &[u8] = include_bytes!("../../icons/tray-color-32.png");
const TRAY_ICON_COLOR_44: &[u8] = include_bytes!("../../icons/tray-color-44.png");
const TRAY_ICON_COLOR_64: &[u8] = include_bytes!("../../icons/tray-color-64.png");

fn is_flatpak() -> bool {
    std::env::var("FLATPAK_ID").is_ok() || std::path::Path::new("/.flatpak-info").exists()
}

/// Detect whether the system prefers a dark color scheme. Tries (in order):
/// GNOME `color-scheme`, GTK `prefer-dark-theme`, KDE `ColorScheme`. Defaults
/// to `false` (light) when nothing matches.
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
                    if let Some(rest) =
                        line.trim().strip_prefix("gtk-application-prefer-dark-theme")
                    {
                        let v = rest.trim_start_matches(['=', ' ']);
                        return v.starts_with('1') || v.starts_with("true");
                    }
                }
            }
        }
        if let Ok(content) = std::fs::read_to_string(config.join("kdeglobals")) {
            for line in content.lines() {
                if let Some(rest) = line.trim().strip_prefix("ColorScheme") {
                    return rest
                        .trim_start_matches(['=', ' '])
                        .to_lowercase()
                        .contains("dark");
                }
            }
        }
    }
    false
}

/// Convert an embedded RGBA PNG to the ARGB32 big-endian layout ksni expects.
fn decode_pixmap(bytes: &[u8]) -> Result<Icon, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("decode tray icon: {e}"))?;
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

#[derive(Clone, Copy, Debug)]
enum IconVariant {
    /// Black glyph — for light panels.
    MonoBlack,
    /// White glyph — for dark panels (Plasma dark, GNOME top bar).
    MonoWhite,
    /// Full color vinyl.
    Color,
}

/// Resolve which icon variant to load. `theme_override`:
///   - "auto" (or unrecognised) — system color-scheme detection
///   - "mono-light" — white (light-coloured) glyph
///   - "mono-dark"  — black (dark-coloured) glyph
///   - "color"      — full vinyl logo
fn resolve_variant(theme_override: Option<&str>) -> IconVariant {
    match theme_override {
        Some("mono-light") => IconVariant::MonoWhite,
        Some("mono-dark") => IconVariant::MonoBlack,
        Some("color") => IconVariant::Color,
        _ => {
            if prefer_dark_tray() {
                IconVariant::MonoWhite
            } else {
                IconVariant::MonoBlack
            }
        }
    }
}

/// Decode pixmaps (22/32/44/64) for the resolved variant.
fn decode_tray_icons(theme_override: Option<&str>) -> Result<Vec<Icon>, String> {
    let sources: [&[u8]; 4] = match resolve_variant(theme_override) {
        IconVariant::MonoBlack => [
            TRAY_ICON_MONO_BLACK_22,
            TRAY_ICON_MONO_BLACK_32,
            TRAY_ICON_MONO_BLACK_44,
            TRAY_ICON_MONO_BLACK_64,
        ],
        IconVariant::MonoWhite => [
            TRAY_ICON_MONO_WHITE_22,
            TRAY_ICON_MONO_WHITE_32,
            TRAY_ICON_MONO_WHITE_44,
            TRAY_ICON_MONO_WHITE_64,
        ],
        IconVariant::Color => [
            TRAY_ICON_COLOR_22,
            TRAY_ICON_COLOR_32,
            TRAY_ICON_COLOR_44,
            TRAY_ICON_COLOR_64,
        ],
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
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    icons: Vec<Icon>,
    now_playing: Option<NowPlaying>,
    is_playing: bool,
}

impl QbzTray {
    fn play_pause(&self) {
        super::dispatch_play_pause(self.runtime.clone(), self.weak.clone(), self.handle.clone());
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
        // app id against the icon theme picks up the full colour app icon
        // instead of our themed monochrome glyph (issue #362). An empty name
        // forces panels to render IconPixmap directly.
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ToolTip {
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
            None => ("QBZ".to_string(), "Music Player\nNothing playing".to_string()),
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
        super::toggle_window(&self.weak);
    }

    /// Secondary click (middle) — play/pause.
    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        log::info!("[tray] secondary activate (middle click) -> play/pause");
        self.play_pause();
    }

    /// Mouse wheel — adjust volume in 5%-per-notch steps. Panels report ±120
    /// per notch; touch-pad scrolls produce smaller deltas, so we normalise by
    /// 120 and fall back to `signum()`.
    fn scroll(&mut self, delta: i32, orientation: Orientation) {
        if !matches!(orientation, Orientation::Vertical) || delta == 0 {
            return;
        }
        let mut ticks = delta / 120;
        if ticks == 0 {
            ticks = delta.signum();
        }
        log::debug!("[tray] scroll delta={} ticks={}", delta, ticks);
        // Positive ticks = wheel-up = volume up.
        super::dispatch_volume_delta(
            self.runtime.clone(),
            self.weak.clone(),
            self.handle.clone(),
            ticks,
        );
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Play/Pause".into(),
                activate: Box::new(|this: &mut Self| this.play_pause()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Next Track".into(),
                activate: Box::new(|this: &mut Self| {
                    super::dispatch_next(
                        this.runtime.clone(),
                        this.weak.clone(),
                        this.handle.clone(),
                    )
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Previous Track".into(),
                activate: Box::new(|this: &mut Self| {
                    super::dispatch_previous(
                        this.runtime.clone(),
                        this.weak.clone(),
                        this.handle.clone(),
                    )
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Show/Hide Window".into(),
                activate: Box::new(|this: &mut Self| super::toggle_window(&this.weak)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit QBZ".into(),
                activate: Box::new(|_this: &mut Self| super::quit()),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Updates dispatched from the rest of the app into the ksni worker.
///
/// We cannot call `ksni::blocking::Handle::update` directly from a tokio task —
/// `ksni` 0.3 (`feature = "blocking"`) wraps every update in
/// `Runtime::block_on`, which panics inside an existing tokio runtime (the
/// playback poll loop runs on one). The panic is swallowed, so updates appear
/// to succeed but never apply. We serialise updates over a `std::sync::mpsc`
/// channel applied from a dedicated `std::thread` outside any tokio context.
enum TrayUpdate {
    SetTrack {
        title: String,
        artist: String,
        album: String,
    },
    ClearTrack,
    SetPlaying(bool),
    /// Re-decode pixmaps for the requested theme override and push them live
    /// (`NewIcon` SNI signal — panels re-fetch without restart).
    SetIconTheme(String),
}

/// Cross-thread handle to the live ksni tray. Cloneable; mutators forward to
/// the worker thread, safe from any async context. When the tray failed to
/// start the inner sender stays `None` and every mutator is a no-op.
#[derive(Clone)]
pub struct LinuxTrayHandle {
    sender: Arc<Mutex<Option<Sender<TrayUpdate>>>>,
}

impl LinuxTrayHandle {
    fn empty() -> Self {
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
                            log::debug!("[tray] tooltip update -> {} / {} / {}", title, artist, album);
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

/// Initialize the Linux ksni tray service. Spawns a background thread that owns
/// the SNI service and returns a cloneable handle for live tooltip / theme
/// updates. `theme_override`: "auto"/"mono-light"/"mono-dark"/"color".
pub fn init(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    theme_override: &str,
) -> Result<LinuxTrayHandle, Box<dyn std::error::Error>> {
    log::info!(
        "Initializing ksni tray (Linux, SNI primary-activate enabled, theme={:?})",
        theme_override
    );

    let icons = decode_tray_icons(Some(theme_override))?;
    let tray = QbzTray {
        runtime,
        weak,
        handle,
        icons,
        now_playing: None,
        is_playing: false,
    };

    // Flatpak requires disabling the well-known DBus name because the sandbox
    // cannot own arbitrary bus names.
    let ksni_handle = if is_flatpak() {
        log::info!("[tray] Flatpak detected — spawning ksni without DBus well-known name");
        tray.disable_dbus_name(true).spawn()?
    } else {
        tray.spawn()?
    };

    let live = LinuxTrayHandle::empty();
    live.install(ksni_handle);

    log::info!("ksni tray initialized");
    Ok(live)
}
