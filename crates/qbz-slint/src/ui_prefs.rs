//! Tiny JSON-backed UI preference store.
//!
//! Some settings the Tauri app exposes are not part of any domain store
//! (`AudioSettings`, `PlaybackPreferences`). Streaming Quality is one: it
//! is a pure UI/request preference. Rather than thread it into a domain
//! store, this module persists those preferences to a small JSON file
//! next to the other QBZ data (`<data_dir>/qbz/ui_prefs.json`).
//!
//! The store is intentionally minimal — read-modify-write the whole file
//! on every set. The file is tiny and writes are rare (a settings change).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Streaming-quality tiers, mirroring the Tauri app's dropdown. The
/// `format_id` is the Qobuz format identifier the request layer expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamingQuality {
    /// Stable key persisted to JSON.
    pub key: &'static str,
    /// Human-facing label for the dropdown.
    pub label: &'static str,
}

/// The four streaming-quality options, in on-screen order.
pub const STREAMING_QUALITIES: &[StreamingQuality] = &[
    StreamingQuality { key: "mp3", label: "MP3" },
    StreamingQuality { key: "cd", label: "CD Quality" },
    StreamingQuality { key: "hires", label: "Hi-Res" },
    StreamingQuality { key: "hires_plus", label: "Hi-Res+" },
];

/// Default streaming-quality key (`Hi-Res+`).
pub const DEFAULT_STREAMING_QUALITY: &str = "hires_plus";

/// Default now-playing bar layout key (`New`).
pub const DEFAULT_NPB_MODE: &str = "new";

/// Default album/artist header backdrop setting.
pub const DEFAULT_ALBUM_HEADER_GRADIENT: bool = true;

/// Default intelligent-search setting (smart cache, ranking, preview dropdown).
pub const DEFAULT_INTELLIGENT_SEARCH: bool = true;

/// Default immersive-search action. The in-immersive search dropdown acts on
/// playback (immersive has no navigation): `"disabled"` turns the field inert,
/// `"replace"` swaps the queue and plays, `"next"` inserts after the current
/// track, `"queue"` appends to the end. Default = `"replace"`.
pub const DEFAULT_IMMERSIVE_SEARCH_ACTION: &str = "replace";

/// Map an immersive-search-action select index to its persisted key. The
/// on-screen order is Disabled / Replace / Play next / Add to queue (0-3);
/// any unknown index falls back to the default (`"replace"`).
pub fn immersive_search_action_for_index(index: i32) -> &'static str {
    match index {
        0 => "disabled",
        1 => "replace",
        2 => "next",
        3 => "queue",
        _ => DEFAULT_IMMERSIVE_SEARCH_ACTION,
    }
}

/// Inverse of [`immersive_search_action_for_index`]: the select index for a
/// persisted key, falling back to the default's index (1 = "replace").
pub fn immersive_search_action_index(key: &str) -> i32 {
    match key {
        "disabled" => 0,
        "replace" => 1,
        "next" => 2,
        "queue" => 3,
        _ => 1,
    }
}

/// Default immersive default-view. `"remember"` restores whatever immersive view
/// was open last time; the other keys PIN a fixed FOCUS-mode foreground.
pub const DEFAULT_IMMERSIVE_DEFAULT_VIEW: &str = "remember";

/// Map an immersive-default-view select index to its persisted key. The
/// on-screen order is Remember last / Album Reactive / Static / Coverflow /
/// Spectrum / Lyrics / Queue (0-6); any unknown index falls back to the
/// default (`"remember"`).
fn default_system_notifications() -> bool {
    true
}

/// Miniplayer default-view select index -> persisted key (mirrors the
/// miniplayer's own reader in `miniplayer.rs`). 0 = "remember".
pub fn mini_default_view_for_index(index: i32) -> &'static str {
    match index {
        1 => "micro",
        2 => "compact",
        3 => "artwork",
        4 => "queue",
        5 => "lyrics",
        _ => "remember",
    }
}

/// Inverse of [`mini_default_view_for_index`]: select index for a persisted key.
pub fn mini_default_view_index(key: &str) -> i32 {
    match key {
        "micro" => 1,
        "compact" => 2,
        "artwork" => 3,
        "queue" => 4,
        "lyrics" => 5,
        _ => 0,
    }
}

pub fn immersive_default_view_for_index(index: i32) -> &'static str {
    match index {
        0 => "remember",
        1 => "reactive",
        2 => "static",
        3 => "coverflow",
        4 => "spectrum",
        5 => "lyrics",
        6 => "queue",
        _ => DEFAULT_IMMERSIVE_DEFAULT_VIEW,
    }
}

/// Inverse of [`immersive_default_view_for_index`]: the select index for a
/// persisted key, falling back to the default's index (0 = "remember").
pub fn immersive_default_view_index(key: &str) -> i32 {
    match key {
        "remember" => 0,
        "reactive" => 1,
        "static" => 2,
        "coverflow" => 3,
        "spectrum" => 4,
        "lyrics" => 5,
        "queue" => 6,
        _ => 0,
    }
}

/// Persisted UI preferences. New fields must default sanely so an older
/// file (missing the field) still deserializes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiPrefs {
    /// Streaming-quality key — one of `STREAMING_QUALITIES[*].key`.
    #[serde(default = "default_streaming_quality")]
    pub streaming_quality: String,
    /// Now-playing bar layout: `"new"` | `"classic"` | `"small"` | `"large"`.
    /// Maps to `ShellState.npb-mode` (0 / 1 / 2 / 3).
    #[serde(default = "default_npb_mode")]
    pub npb_mode: String,
    /// Large-NPB dock visualizer on/off (the cover's top-right eye toggle).
    /// Persisted so a user who dislikes the spectrum keeps it off across runs.
    #[serde(default = "default_large_visualizer")]
    pub large_visualizer: bool,
    /// Large-NPB dock spectrum visualization: `"bars"` | `"waveform"` | `"energy"`.
    /// Maps to `ShellState.large-spectrum-mode` (0 / 1 / 2).
    #[serde(default = "default_large_spectrum_mode")]
    pub large_spectrum_mode: String,
    /// Whether album/artist detail headers use artwork-derived backdrops.
    #[serde(default = "default_album_header_gradient")]
    pub album_header_gradient: bool,
    /// Whether intelligent search (cache, ranking, preview dropdown) is enabled.
    #[serde(default = "default_intelligent_search")]
    pub intelligent_search: bool,
    /// Whether desktop "now playing" system notifications fire on track change.
    /// Default ON (the notify backend ships default-on; this is the user gate).
    #[serde(default = "default_system_notifications")]
    pub system_notifications: bool,
    /// Immersive in-view search action: `"disabled"` | `"replace"` | `"next"` |
    /// `"queue"`. Doubles as the enable switch (`"disabled"` keeps the field
    /// inert). See [`DEFAULT_IMMERSIVE_SEARCH_ACTION`].
    #[serde(default = "default_immersive_search_action")]
    pub immersive_search_action: String,
    /// Immersive default-view key: `"remember"` restores the last view, else one
    /// of `"reactive"` | `"static"` | `"coverflow"` | `"spectrum"` | `"lyrics"` |
    /// `"queue"` pins a fixed FOCUS view. See [`DEFAULT_IMMERSIVE_DEFAULT_VIEW`].
    #[serde(default = "default_immersive_default_view")]
    pub immersive_default_view: String,
    /// Last immersive view-mode (0 FOCUS / 1 SPLIT), persisted only while the
    /// default is `"remember"`; restored on the next overlay open.
    #[serde(default)]
    pub immersive_last_view_mode: i32,
    /// Last immersive FOCUS panel (read when last view-mode == 0). Same
    /// remember-last persistence as [`Self::immersive_last_view_mode`].
    #[serde(default)]
    pub immersive_last_mode: i32,
    /// Last immersive SPLIT panel (read when last view-mode == 1). Same
    /// remember-last persistence as [`Self::immersive_last_view_mode`].
    #[serde(default)]
    pub immersive_last_split_panel: i32,
    /// Active theme — a stable `qbz_theme::ThemeId` slug ("oled", "dark",
    /// "tokyo-night", "system", ...). Stored as a slug (not an index) so it is
    /// order-independent and stable across releases. Owner default: OLED Dark.
    #[serde(default = "default_theme")]
    pub theme: String,

    // ---- Miniplayer ----------------------------------------------------
    /// Last miniplayer surface: 0 micro · 1 compact · 2 artwork · 3 queue · 4 lyrics.
    #[serde(default = "default_mini_surface")]
    pub mini_surface: i32,
    /// Remembered EXPANDED window size (artwork/queue/lyrics share it).
    #[serde(default = "default_mini_width")]
    pub mini_width: f32,
    #[serde(default = "default_mini_height")]
    pub mini_height: f32,
    /// Whether the miniplayer uses the static artwork-derived background.
    #[serde(default)]
    pub mini_background_blur: bool,
    /// Default-view key: "remember" | micro | compact | artwork | queue | lyrics.
    #[serde(default = "default_mini_default_view")]
    pub mini_default_view: String,
}

fn default_mini_surface() -> i32 {
    2
}
fn default_mini_width() -> f32 {
    380.0
}
fn default_mini_height() -> f32 {
    540.0
}
fn default_mini_default_view() -> String {
    "remember".to_string()
}

fn default_streaming_quality() -> String {
    DEFAULT_STREAMING_QUALITY.to_string()
}

fn default_npb_mode() -> String {
    DEFAULT_NPB_MODE.to_string()
}

fn default_large_visualizer() -> bool {
    true
}

fn default_large_spectrum_mode() -> String {
    "bars".to_string()
}

fn default_album_header_gradient() -> bool {
    DEFAULT_ALBUM_HEADER_GRADIENT
}

fn default_intelligent_search() -> bool {
    DEFAULT_INTELLIGENT_SEARCH
}

fn default_immersive_search_action() -> String {
    DEFAULT_IMMERSIVE_SEARCH_ACTION.to_string()
}

fn default_immersive_default_view() -> String {
    DEFAULT_IMMERSIVE_DEFAULT_VIEW.to_string()
}

/// Default theme slug. Owner decision 2026-06-20: OLED Dark is the default for
/// fresh installs and any profile without a persisted theme. Sourced from the
/// `qbz-theme` registry so the default stays single-sourced.
fn default_theme() -> String {
    qbz_theme::default_slug().to_string()
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            streaming_quality: default_streaming_quality(),
            npb_mode: default_npb_mode(),
            large_visualizer: default_large_visualizer(),
            large_spectrum_mode: default_large_spectrum_mode(),
            album_header_gradient: default_album_header_gradient(),
            intelligent_search: default_intelligent_search(),
            system_notifications: default_system_notifications(),
            immersive_search_action: default_immersive_search_action(),
            immersive_default_view: default_immersive_default_view(),
            immersive_last_view_mode: 0,
            immersive_last_mode: 0,
            immersive_last_split_panel: 0,
            theme: default_theme(),
            mini_surface: default_mini_surface(),
            mini_width: default_mini_width(),
            mini_height: default_mini_height(),
            mini_background_blur: false,
            mini_default_view: default_mini_default_view(),
        }
    }
}

/// Map a persisted npb-mode key to the `ShellState.npb-mode` int
/// (New = 0, Classic = 1, Small = 2, Large = 3). Unknown keys fall back to New.
pub fn npb_mode_index(key: &str) -> i32 {
    match key {
        "classic" => 1,
        "small" => 2,
        "large" => 3,
        _ => 0,
    }
}

/// Map a persisted Large-dock spectrum key to `ShellState.large-spectrum-mode`
/// (Bars = 0, Waveform = 1, Energy = 2). Unknown keys fall back to Bars.
pub fn large_spectrum_mode_index(key: &str) -> i32 {
    match key {
        "waveform" => 1,
        "energy" => 2,
        _ => 0,
    }
}

/// Inverse of [`large_spectrum_mode_index`] — the persisted key for an int mode.
pub fn large_spectrum_mode_key(index: i32) -> &'static str {
    match index {
        1 => "waveform",
        2 => "energy",
        _ => "bars",
    }
}

/// Resolve `<data_dir>/qbz/ui_prefs.json`.
fn prefs_path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("qbz").join("ui_prefs.json"))
}

/// Load the UI preferences. A missing or unreadable file degrades to
/// `UiPrefs::default()` rather than erroring.
pub fn load() -> UiPrefs {
    let Some(path) = prefs_path() else {
        return UiPrefs::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!("[qbz-slint] ui_prefs.json parse failed, using defaults: {e}");
            UiPrefs::default()
        }),
        Err(_) => UiPrefs::default(),
    }
}

/// Persist the UI preferences. Best-effort — failures are logged.
pub fn save(prefs: &UiPrefs) {
    let Some(path) = prefs_path() else {
        log::warn!("[qbz-slint] ui_prefs.json: data dir unavailable, not saving");
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::error!("[qbz-slint] ui_prefs.json: create dir failed: {e}");
            return;
        }
    }
    match serde_json::to_string_pretty(prefs) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                log::error!("[qbz-slint] ui_prefs.json: write failed: {e}");
            }
        }
        Err(e) => log::error!("[qbz-slint] ui_prefs.json: serialize failed: {e}"),
    }
}

/// Index of `key` in `STREAMING_QUALITIES`, falling back to the default
/// (`Hi-Res+`) when the stored key is unknown.
pub fn streaming_quality_index(key: &str) -> usize {
    STREAMING_QUALITIES
        .iter()
        .position(|q| q.key == key)
        .unwrap_or_else(|| {
            STREAMING_QUALITIES
                .iter()
                .position(|q| q.key == DEFAULT_STREAMING_QUALITY)
                .unwrap_or(0)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_hires_plus() {
        assert_eq!(UiPrefs::default().streaming_quality, "hires_plus");
        assert_eq!(STREAMING_QUALITIES.len(), 4);
        assert_eq!(STREAMING_QUALITIES[3].key, "hires_plus");
    }

    #[test]
    fn unknown_key_resolves_to_default_index() {
        // Default is hires_plus, which is index 3.
        assert_eq!(streaming_quality_index("bogus"), 3);
        assert_eq!(streaming_quality_index("mp3"), 0);
        assert_eq!(streaming_quality_index("cd"), 1);
    }

    #[test]
    fn legacy_json_without_field_deserializes() {
        let prefs: UiPrefs = serde_json::from_str("{}").expect("empty object deserializes");
        assert_eq!(prefs.streaming_quality, "hires_plus");
        assert!(prefs.album_header_gradient);
        // A profile that predates the theme field falls back to OLED.
        assert_eq!(prefs.theme, "oled");
    }

    #[test]
    fn default_theme_is_oled() {
        assert_eq!(UiPrefs::default().theme, "oled");
        assert_eq!(default_theme(), "oled");
    }
}
