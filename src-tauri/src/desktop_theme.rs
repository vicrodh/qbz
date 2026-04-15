//! Desktop theme detection for adaptive window controls.
//!
//! Reads relevant KDE Plasma config files (`kwinrc`, `kdeglobals`, `klassyrc`)
//! so QBZ's custom titlebar can mirror the system's decoration colors when
//! the user has native decorations disabled. Plasma-only today; other desktops
//! return `None`.

use serde::Serialize;

#[derive(Debug, Default, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DesktopThemeInfo {
    /// e.g. "plasma-klassy", "plasma-breeze", "other"
    pub desktop: String,
    pub is_klassy: bool,
    /// Hex `#rrggbb` when available.
    pub titlebar_active_bg: Option<String>,
    pub titlebar_active_fg: Option<String>,
    pub titlebar_inactive_bg: Option<String>,
    pub titlebar_inactive_fg: Option<String>,
    pub accent: Option<String>,
    pub decoration_hover: Option<String>,
    pub klassy_button_icon_style: Option<String>,
    pub klassy_button_shape: Option<String>,
    pub klassy_match_app_color: Option<bool>,
    /// Best-effort default corner radius for the active desktop, in px.
    /// Used by the "match system window chrome" feature to approximate
    /// the decoration radius without transparent-window trickery.
    pub window_corner_radius_px: u16,
}

/// Parse a `R,G,B` KDE color triplet into a `#rrggbb` hex string. Accepts
/// whitespace between components (KDE writes inconsistent spacing).
fn kde_color_to_hex(raw: &str) -> Option<String> {
    let parts: Vec<u8> = raw
        .split(',')
        .map(|s| s.trim().parse::<u8>().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.len() < 3 {
        return None;
    }
    Some(format!("#{:02x}{:02x}{:02x}", parts[0], parts[1], parts[2]))
}

/// Read `key=value` from an ini-ish file, scoped to a `[section]` header.
/// Stops at the next `[section]`. Returns the first match found.
fn ini_get(path: &std::path::Path, section: &str, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut in_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_section = &line[1..line.len() - 1] == section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Return a best-effort snapshot of the user's desktop decoration theme.
/// Non-Plasma desktops yield `desktop="other"` with everything else `None`.
/// Best-effort default window corner radius for a detected desktop.
/// Plasma/Klassy leans to ~10px, Breeze to ~4px, GNOME/Adwaita to ~12px.
/// Conservative for unknown desktops.
fn default_corner_radius(desktop: &str, is_klassy: bool) -> u16 {
    if is_klassy {
        return 10;
    }
    match desktop {
        "plasma-breeze" => 6,
        "gnome" => 12,
        _ => 8,
    }
}

#[tauri::command]
pub fn detect_desktop_theme() -> DesktopThemeInfo {
    let mut info = DesktopThemeInfo {
        desktop: "other".to_string(),
        window_corner_radius_px: 8,
        ..Default::default()
    };

    let config_dir = match dirs::config_dir() {
        Some(d) => d,
        None => return info,
    };

    let kwinrc = config_dir.join("kwinrc");
    let kdeglobals = config_dir.join("kdeglobals");
    let klassyrc = config_dir.join("klassy/klassyrc");

    if !kwinrc.exists() && !kdeglobals.exists() {
        return info;
    }

    let kwin_theme = ini_get(&kwinrc, "org.kde.kdecoration2", "theme")
        .or_else(|| ini_get(&kwinrc, "org.kde.kdecoration3", "theme"));

    match kwin_theme.as_deref() {
        Some("Klassy") => {
            info.desktop = "plasma-klassy".to_string();
            info.is_klassy = true;
        }
        Some(_) => info.desktop = "plasma-breeze".to_string(),
        None => info.desktop = "plasma-breeze".to_string(),
    }

    if info.is_klassy {
        info.klassy_button_icon_style = ini_get(&klassyrc, "Windeco", "ButtonIconStyle");
        info.klassy_button_shape = ini_get(&klassyrc, "Windeco", "ButtonShape");
        info.klassy_match_app_color = ini_get(&klassyrc, "Windeco", "MatchTitleBarToApplicationColor")
            .as_deref()
            .map(|v| v.eq_ignore_ascii_case("true") || v.starts_with('1'));
    }

    info.window_corner_radius_px = default_corner_radius(&info.desktop, info.is_klassy);

    info.titlebar_active_bg = ini_get(&kdeglobals, "WM", "activeBackground").and_then(|v| kde_color_to_hex(&v));
    info.titlebar_active_fg = ini_get(&kdeglobals, "WM", "activeForeground").and_then(|v| kde_color_to_hex(&v));
    info.titlebar_inactive_bg = ini_get(&kdeglobals, "WM", "inactiveBackground").and_then(|v| kde_color_to_hex(&v));
    info.titlebar_inactive_fg = ini_get(&kdeglobals, "WM", "inactiveForeground").and_then(|v| kde_color_to_hex(&v));
    info.accent = ini_get(&kdeglobals, "Colors:Window", "DecorationFocus").and_then(|v| kde_color_to_hex(&v));
    info.decoration_hover = ini_get(&kdeglobals, "Colors:Window", "DecorationHover").and_then(|v| kde_color_to_hex(&v));

    info
}
