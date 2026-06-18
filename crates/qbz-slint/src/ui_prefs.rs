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

/// Persisted UI preferences. New fields must default sanely so an older
/// file (missing the field) still deserializes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiPrefs {
    /// Streaming-quality key — one of `STREAMING_QUALITIES[*].key`.
    #[serde(default = "default_streaming_quality")]
    pub streaming_quality: String,
    /// Now-playing bar layout: `"new"` | `"classic"` | `"small"`. Maps to
    /// `ShellState.npb-mode` (0 / 1 / 2).
    #[serde(default = "default_npb_mode")]
    pub npb_mode: String,
    /// Whether album/artist detail headers use artwork-derived backdrops.
    #[serde(default = "default_album_header_gradient")]
    pub album_header_gradient: bool,

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

fn default_album_header_gradient() -> bool {
    DEFAULT_ALBUM_HEADER_GRADIENT
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            streaming_quality: default_streaming_quality(),
            npb_mode: default_npb_mode(),
            album_header_gradient: default_album_header_gradient(),
            mini_surface: default_mini_surface(),
            mini_width: default_mini_width(),
            mini_height: default_mini_height(),
            mini_background_blur: false,
            mini_default_view: default_mini_default_view(),
        }
    }
}

/// Map a persisted npb-mode key to the `ShellState.npb-mode` int
/// (New = 0, Classic = 1, Small = 2). Unknown keys fall back to New.
pub fn npb_mode_index(key: &str) -> i32 {
    match key {
        "classic" => 1,
        "small" => 2,
        _ => 0,
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
    }
}
