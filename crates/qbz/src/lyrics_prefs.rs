//! Per-user lyrics display-prefs persistence (S5).
//!
//! Mirrors Tauri's `lyricsDisplayStore` (per-user localStorage key
//! `qbz-lyrics-display`, `lyricsDisplayStore.ts:35`): the six display prefs
//! the controls flyout edits, persisted per user so different Qobuz accounts
//! keep independent settings. Field values use Tauri's exact strings
//! (`font: 'line-seed-jp'`, `fontSize: 'medium'`, ...) so the shape stays
//! diff-able against the Tauri store; defaults are Tauri's
//! (`lyricsDisplayStore.ts:37-44`) and loads are sanitized the same way
//! (`:64-85` — any unknown value falls back to its default, `activeColor`
//! must be `#RRGGBB` or empty).
//!
//! Storage follows the `myqbz_view_prefs.rs` template — one tiny per-user
//! JSON, synchronous best-effort IO:
//!
//!   <data_dir>/qbz/users/<user_id>/lyrics_prefs.json
//!
//! Lifecycle:
//!  - `init_for_user` binds the store on session activation (all paths run
//!    through `init_shell_for_user`), then the caller seeds the UI via
//!    [`apply_to_ui`] on the event loop.
//!  - The flyout fires `LyricsState.prefs-changed()` after any mutation →
//!    [`persist_from_ui`] reads the in-out props back and saves.
//!  - `LyricsState.reset-prefs()` → [`reset`] re-seeds the defaults and
//!    persists them (Tauri `resetLyricsDisplay`).

use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use slint::ComponentHandle;

use crate::{AppWindow, LyricsState};

/// The active user id, set by `init_for_user`. `None` before login (the
/// store degrades to defaults).
static USER_ID: LazyLock<Mutex<Option<u64>>> = LazyLock::new(|| Mutex::new(None));

/// The persisted pref set — field names and value strings match Tauri's
/// `LyricsDisplayPrefs` (`lyricsDisplayStore.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricsPrefs {
    #[serde(default = "d_true")]
    pub auto_follow: bool,
    #[serde(default = "d_font")]
    pub font: String, // system | line-seed-jp | montserrat | noto-sans | source-sans-3
    #[serde(default = "d_size")]
    pub font_size: String, // small | medium | large | xl
    #[serde(default = "d_dimming")]
    pub dimming: String, // off | soft | strong
    #[serde(default)]
    pub active_color: String, // "#RRGGBB" or "" = theme accent
    #[serde(default)]
    pub uppercase: bool,
    /// Lite fill (perf): highlight the active line whole instead of the per-word
    /// karaoke sweep, so the lyrics surface stops driving continuous repaints.
    #[serde(default)]
    pub lite_fill: bool,
}

fn d_true() -> bool {
    true
}
fn d_font() -> String {
    "system".to_string()
}
fn d_size() -> String {
    "medium".to_string()
}
fn d_dimming() -> String {
    "strong".to_string()
}

impl Default for LyricsPrefs {
    /// Tauri defaults (`lyricsDisplayStore.ts:37-44`).
    fn default() -> Self {
        Self {
            auto_follow: true,
            font: d_font(),
            font_size: d_size(),
            dimming: d_dimming(),
            active_color: String::new(),
            uppercase: false,
            lite_fill: false,
        }
    }
}

const FONTS: [&str; 5] = ["system", "line-seed-jp", "montserrat", "noto-sans", "source-sans-3"];
const SIZES: [&str; 4] = ["small", "medium", "large", "xl"];
const DIMMINGS: [&str; 3] = ["off", "soft", "strong"];

/// `#RRGGBB` (or empty = theme) — the Tauri validator
/// (`lyricsDisplayStore.ts:46-52`).
fn valid_color(value: &str) -> bool {
    value.is_empty()
        || (value.len() == 7
            && value.starts_with('#')
            && value[1..].chars().all(|c| c.is_ascii_hexdigit()))
}

impl LyricsPrefs {
    /// Clamp every field to its known value set (Tauri's sanitize-on-load,
    /// `lyricsDisplayStore.ts:64-85`): unknown values fall back to defaults.
    fn sanitized(mut self) -> Self {
        if !FONTS.contains(&self.font.as_str()) {
            self.font = d_font();
        }
        if !SIZES.contains(&self.font_size.as_str()) {
            self.font_size = d_size();
        }
        if !DIMMINGS.contains(&self.dimming.as_str()) {
            self.dimming = d_dimming();
        }
        if !valid_color(&self.active_color) {
            self.active_color.clear();
        }
        self
    }
}

/// `<data_dir>/qbz/users/<user_id>/lyrics_prefs.json` for the active user.
fn store_path() -> Option<PathBuf> {
    let user_id = (*USER_ID.lock().ok()?)?;
    Some(
        dirs::data_dir()?
            .join("qbz")
            .join("users")
            .join(user_id.to_string())
            .join("lyrics_prefs.json"),
    )
}

/// Bind the store to `user_id` on session activation.
pub fn init_for_user(user_id: u64) {
    if let Ok(mut guard) = USER_ID.lock() {
        *guard = Some(user_id);
    }
}

/// Load the stored prefs, sanitized; missing/unreadable/unparseable file →
/// defaults.
pub fn load() -> LyricsPrefs {
    let Some(path) = store_path() else {
        return LyricsPrefs::default();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<LyricsPrefs>(&bytes)
            .map(LyricsPrefs::sanitized)
            .unwrap_or_default(),
        Err(_) => LyricsPrefs::default(),
    }
}

/// Persist the prefs (best-effort — failures are logged).
pub fn save(prefs: &LyricsPrefs) {
    let Some(path) = store_path() else {
        log::warn!("[qbz-slint] lyrics prefs: no active user, not saving");
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::error!("[qbz-slint] lyrics prefs: create dir failed: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(prefs) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("[qbz-slint] lyrics prefs: write failed: {e}");
            }
        }
        Err(e) => log::error!("[qbz-slint] lyrics prefs: serialize failed: {e}"),
    }
}

// ---- LyricsState <-> LyricsPrefs mapping (UI thread only) ------------------

fn font_index(font: &str) -> i32 {
    FONTS.iter().position(|f| *f == font).unwrap_or(0) as i32
}

fn size_index(size: &str) -> i32 {
    SIZES.iter().position(|s| *s == size).unwrap_or(1) as i32
}

fn dimming_index(dimming: &str) -> i32 {
    DIMMINGS.iter().position(|d| *d == dimming).unwrap_or(2) as i32
}

fn parse_color(hex: &str) -> Option<slint::Color> {
    if hex.len() != 7 || !hex.starts_with('#') {
        return None;
    }
    let r = u8::from_str_radix(&hex[1..3], 16).ok()?;
    let g = u8::from_str_radix(&hex[3..5], 16).ok()?;
    let b = u8::from_str_radix(&hex[5..7], 16).ok()?;
    Some(slint::Color::from_rgb_u8(r, g, b))
}

fn format_color(color: slint::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red(), color.green(), color.blue())
}

/// Seed `LyricsState` from a loaded pref set. UI thread only.
pub fn apply_to_ui(window: &AppWindow, prefs: &LyricsPrefs) {
    let state = window.global::<LyricsState>();
    state.set_auto_follow(prefs.auto_follow);
    state.set_font_index(font_index(&prefs.font));
    state.set_size_index(size_index(&prefs.font_size));
    state.set_dimming_mode(dimming_index(&prefs.dimming));
    state.set_uppercase(prefs.uppercase);
    match parse_color(&prefs.active_color) {
        Some(color) => {
            state.set_use_custom_color(true);
            state.set_custom_color(color);
        }
        None => state.set_use_custom_color(false),
    }
    state.set_lite_fill(prefs.lite_fill);
}

/// Read the in-out props back into a pref set and persist — the
/// `prefs-changed()` handler. UI thread only.
pub fn persist_from_ui(window: &AppWindow) {
    let state = window.global::<LyricsState>();
    let prefs = LyricsPrefs {
        auto_follow: state.get_auto_follow(),
        font: FONTS
            .get(state.get_font_index().max(0) as usize)
            .copied()
            .unwrap_or("system")
            .to_string(),
        font_size: SIZES
            .get(state.get_size_index().max(0) as usize)
            .copied()
            .unwrap_or("medium")
            .to_string(),
        dimming: DIMMINGS
            .get(state.get_dimming_mode().max(0) as usize)
            .copied()
            .unwrap_or("strong")
            .to_string(),
        active_color: if state.get_use_custom_color() {
            format_color(state.get_custom_color())
        } else {
            String::new()
        },
        uppercase: state.get_uppercase(),
        lite_fill: state.get_lite_fill(),
    };
    save(&prefs);
}

/// Restore + persist the Tauri defaults — the `reset-prefs()` handler
/// (Tauri `resetLyricsDisplay`). UI thread only.
pub fn reset(window: &AppWindow) {
    let prefs = LyricsPrefs::default();
    apply_to_ui(window, &prefs);
    save(&prefs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_tauri() {
        let p = LyricsPrefs::default();
        assert!(p.auto_follow);
        assert_eq!(p.font, "system");
        assert_eq!(p.font_size, "medium");
        assert_eq!(p.dimming, "strong");
        assert_eq!(p.active_color, "");
        assert!(!p.uppercase);
    }

    #[test]
    fn empty_json_deserializes_to_defaults() {
        let p: LyricsPrefs = serde_json::from_str("{}").expect("empty object deserializes");
        assert_eq!(p, LyricsPrefs::default());
    }

    #[test]
    fn sanitize_clamps_unknown_values() {
        let p = LyricsPrefs {
            auto_follow: false,
            font: "comic-sans".into(),
            font_size: "huge".into(),
            dimming: "max".into(),
            active_color: "purple".into(),
            uppercase: true,
            lite_fill: false,
        }
        .sanitized();
        assert!(!p.auto_follow); // bools pass through
        assert!(p.uppercase);
        assert_eq!(p.font, "system");
        assert_eq!(p.font_size, "medium");
        assert_eq!(p.dimming, "strong");
        assert_eq!(p.active_color, "");
    }

    #[test]
    fn sanitize_keeps_valid_values() {
        let p = LyricsPrefs {
            auto_follow: false,
            font: "line-seed-jp".into(),
            font_size: "xl".into(),
            dimming: "off".into(),
            active_color: "#8b5cf6".into(),
            uppercase: false,
            lite_fill: true,
        }
        .sanitized();
        assert_eq!(p.font, "line-seed-jp");
        assert_eq!(p.font_size, "xl");
        assert_eq!(p.dimming, "off");
        assert_eq!(p.active_color, "#8b5cf6");
    }

    #[test]
    fn color_validation() {
        assert!(valid_color(""));
        assert!(valid_color("#8b5cf6"));
        assert!(valid_color("#FFFFFF"));
        assert!(!valid_color("#fff"));
        assert!(!valid_color("8b5cf6"));
        assert!(!valid_color("#8b5cg6"));
    }

    #[test]
    fn index_round_trips() {
        for (i, f) in FONTS.iter().enumerate() {
            assert_eq!(font_index(f), i as i32);
        }
        for (i, s) in SIZES.iter().enumerate() {
            assert_eq!(size_index(s), i as i32);
        }
        for (i, d) in DIMMINGS.iter().enumerate() {
            assert_eq!(dimming_index(d), i as i32);
        }
        // Unknowns fall back to the defaults' indices.
        assert_eq!(font_index("nope"), 0);
        assert_eq!(size_index("nope"), 1);
        assert_eq!(dimming_index("nope"), 2);
    }

    #[test]
    fn color_round_trip() {
        let c = parse_color("#8b5cf6").expect("parses");
        assert_eq!(format_color(c), "#8b5cf6");
        assert!(parse_color("").is_none());
        assert!(parse_color("#xyzxyz").is_none());
    }
}
