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

use std::collections::BTreeMap;
use std::path::PathBuf;

use qbz_models::Quality;
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

/// Default UI language key. `"auto"` follows the OS locale (resolved at startup
/// via `qbz_i18n::resolve_auto()`); otherwise one of `en` | `es` | `de` | `fr` |
/// `pt`. Persists the raw user choice ("auto" stays "auto").
pub const DEFAULT_LANGUAGE: &str = "auto";

/// Map a language select index to its persisted key. The on-screen order in
/// `AppearanceState.languages` is Auto / English / Español / Français / Deutsch
/// / Português / Русский / 日本語 / Nederlands (0-8); any unknown index falls
/// back to the default (`"auto"`).
pub fn language_for_index(index: i32) -> &'static str {
    match index {
        1 => "en",
        2 => "es",
        3 => "fr",
        4 => "de",
        5 => "pt",
        6 => "ru",
        7 => "ja",
        8 => "nl",
        _ => DEFAULT_LANGUAGE,
    }
}

/// Inverse of [`language_for_index`]: the select index for a persisted key,
/// falling back to the default's index (0 = "auto").
pub fn language_index(key: &str) -> i32 {
    match key {
        "en" => 1,
        "es" => 2,
        "fr" => 3,
        "de" => 4,
        "pt" => 5,
        "ru" => 6,
        "ja" => 7,
        "nl" => 8,
        _ => 0,
    }
}

/// Default auto-theme source key (`"system"`: DE color scheme with a wallpaper
/// fallback). The other keys are `"wallpaper"` and `"image"`.
pub const DEFAULT_AUTO_THEME_SOURCE: &str = "system";

/// Map an auto-theme-source select index to its persisted key. On-screen order
/// is System Colors / Wallpaper Sync / Custom Image (0-2); unknown indices fall
/// back to the default (`"system"`).
pub fn auto_theme_source_for_index(index: i32) -> &'static str {
    match index {
        1 => "wallpaper",
        2 => "image",
        _ => "system",
    }
}

/// Inverse of [`auto_theme_source_for_index`]: the select index for a persisted
/// key, falling back to the default's index (0 = "system").
pub fn auto_theme_source_index(key: &str) -> i32 {
    match key {
        "wallpaper" => 1,
        "image" => 2,
        _ => 0,
    }
}

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

/// Default app-wide dynamic background: "off". Other keys: "ambient" (GPU
/// shader scene, wgpu tier only) | "blurred" (blurred-artwork atmosphere).
pub const DEFAULT_APP_BACKGROUND: &str = "off";

/// Map an immersive-default-view select index to its persisted key. The
/// on-screen order is Remember last / Album Reactive / Static / Coverflow /
/// Spectrum / Lyrics / Queue (0-6); any unknown index falls back to the
/// default (`"remember"`).
fn default_system_notifications() -> bool {
    true
}

fn default_musicbrainz_enabled() -> bool {
    true
}

fn default_nav_in_sidebar() -> bool {
    true
}

fn default_volume() -> f32 {
    1.0
}

fn default_startup_page() -> String {
    "home".to_string()
}

fn default_last_view() -> String {
    "home".to_string()
}

/// Startup-page select index (0 = Home, 1 = Where you left off) -> key.
pub fn startup_page_for_index(index: i32) -> &'static str {
    if index == 1 {
        "remember"
    } else {
        "home"
    }
}

/// Inverse: select index for a persisted startup-page key.
pub fn startup_page_index(key: &str) -> i32 {
    if key == "remember" {
        1
    } else {
        0
    }
}

/// Renderer select index (0 = Auto, 1 = GPU, 2 = GPU compatibility/GL,
/// 3 = Software) -> persisted key. Unknown indices fall back to "auto".
pub fn renderer_for_index(index: i32) -> &'static str {
    match index {
        1 => "wgpu",
        2 => "gl",
        3 => "software",
        _ => "auto",
    }
}

/// Inverse: select index for a persisted renderer key.
pub fn renderer_index(key: &str) -> i32 {
    match key {
        "wgpu" => 1,
        "gl" => 2,
        "software" => 3,
        _ => 0,
    }
}

/// Interface-size select index (0 = Extra small, 1 = Small, 2 = Default,
/// 3 = Large, 4 = Extra large) -> persisted key. Unknown indices fall back
/// to "default".
pub fn ui_scale_for_index(index: i32) -> &'static str {
    match index {
        0 => "xs",
        1 => "small",
        3 => "large",
        4 => "xl",
        _ => "default",
    }
}

/// Inverse: select index for a persisted interface-size key.
pub fn ui_scale_index(key: &str) -> i32 {
    match key {
        "xs" => 0,
        "small" => 1,
        "large" => 3,
        "xl" => 4,
        _ => 2,
    }
}

/// Numeric window-scale multiplier for a persisted interface-size key.
pub fn ui_scale_factor(key: &str) -> f32 {
    match key {
        "xs" => 0.8,
        "small" => 0.9,
        "large" => 1.2,
        "xl" => 1.5,
        _ => 1.0,
    }
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

/// Map an app-background select index to its persisted key. On-screen order is
/// Off / Ambient / Blurred (0-2); any unknown index falls back to the default
/// (`"off"`).
pub fn app_background_for_index(index: i32) -> &'static str {
    match index {
        0 => "off",
        1 => "ambient",
        2 => "blurred",
        _ => DEFAULT_APP_BACKGROUND,
    }
}

/// Inverse of [`app_background_for_index`]: the select index for a persisted
/// key, falling back to the default's index (0 = "off").
pub fn app_background_index(key: &str) -> i32 {
    match key {
        "off" => 0,
        "ambient" => 1,
        "blurred" => 2,
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
    /// UI language key: `"auto"` (follow the OS locale) or one of `en` | `es` |
    /// `de` | `fr` | `pt`. Persists the raw user choice; "auto" is resolved to a
    /// concrete language at startup via `qbz_i18n::resolve_auto()`.
    #[serde(default = "default_language")]
    pub language: String,
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
    /// Appearance toggles (persisted; the live Slint globals are seeded from
    /// these at startup, so the user's choice survives a restart).
    #[serde(default = "default_window_title_show")]
    pub window_title_show: bool,
    #[serde(default = "default_show_volume_steppers")]
    pub show_volume_steppers: bool,
    #[serde(default = "default_sidebar_playlist_collage")]
    pub sidebar_playlist_collage: bool,
    #[serde(default = "default_local_library_track_artwork")]
    pub local_library_track_artwork: bool,
    #[serde(default = "default_in_app_toasts")]
    pub in_app_toasts: bool,
    #[serde(default = "default_theme_filter")]
    pub theme_filter: i32,
    /// Whether desktop "now playing" system notifications fire on track change.
    /// Default ON (the notify backend ships default-on; this is the user gate).
    #[serde(default = "default_system_notifications")]
    pub system_notifications: bool,
    /// Whether MusicBrainz metadata enrichment is enabled (opt-out, default ON).
    /// Drives the core client's enabled flag at startup and gates the artist
    /// Network/Scene sidebar + playlist Suggested-Songs. Serde-default means an
    /// older prefs file without the field deserializes to ON (zero migration).
    #[serde(default = "default_musicbrainz_enabled")]
    pub musicbrainz_enabled: bool,
    /// Discord Rich Presence "now listening" opt-in. Default OFF — external
    /// integrations are opt-in. (Tauri scoped this per Qobuz user; here it is a
    /// per-machine app preference — your Discord client is per-machine.)
    #[serde(default)]
    pub discord_rpc_enabled: bool,
    /// Show the opt-in "Purchases" navigation entry (sidebar / header). Default
    /// OFF — the whole DRM-free Purchases feature is hidden until the user
    /// enables it (mirrors Tauri `showPurchases`, default `false`). Re-seeded
    /// into `AppearanceState.show-purchases` at startup; persisted on toggle.
    #[serde(default)]
    pub show_purchases: bool,
    /// Place the Purchases nav entry in the custom title bar instead of the
    /// sidebar. Default OFF (mirrors Tauri `purchasesInTitlebar`, default
    /// `false`). Only meaningful when the custom title bar is shown; the toggle
    /// is disabled in Settings when the title bar is hidden / system-native.
    #[serde(default)]
    pub nav_tb_purchases: bool,
    /// Use the system window decorations for the main window. Per-OS default
    /// (owner decision 2026-07-03): TRUE on Linux (native KDE/GNOME chrome is
    /// fine — Tauri only defaulted to custom because its webview CSD was ugly),
    /// FALSE on macOS (the native pattern there is the overlay: traffic lights
    /// over the app's own header). Startup-time choice — decorations negotiate
    /// at surface creation on Wayland, so changes need a restart.
    #[serde(default = "default_use_system_title_bar")]
    pub use_system_title_bar: bool,
    /// Custom-chrome variant for tiling-WM users: frameless window WITHOUT the
    /// drawn window controls / header drag. Only meaningful when
    /// `use_system_title_bar` is false. Default OFF.
    #[serde(default)]
    pub hide_title_bar: bool,
    /// Show the min/max/close cluster in the header (Linux custom chrome).
    #[serde(default = "default_show_window_controls")]
    pub show_window_controls: bool,
    /// Window-controls side: `"left"` | `"right"`. Default right.
    #[serde(default = "default_wc_position")]
    pub wc_position: String,
    /// Purchases filter: hide unavailable items. Default OFF (show all). Mirrors
    /// Tauri's per-user persisted purchase filter; here it is per-machine like
    /// the other Slint Purchases prefs. Re-seeded into
    /// `PurchasesState.filter-hide-unavailable` at startup; persisted on toggle.
    #[serde(default)]
    pub purchases_hide_unavailable: bool,
    /// Purchases filter: hide already-downloaded items. Default OFF (show all).
    /// Same per-machine persistence as [`Self::purchases_hide_unavailable`].
    #[serde(default)]
    pub purchases_hide_downloaded: bool,
    /// Purchases quality filter: `"all"` | `"hires"` | `"cd"` | `"lossy"`.
    /// Default `"all"` (no quality filtering). Mirrors
    /// `PurchasesState.filter-quality`.
    #[serde(default = "default_purchases_quality_filter")]
    pub purchases_quality_filter: String,
    /// Whether the user has dismissed the Purchases region notice. Default OFF
    /// (notice shown until dismissed, matching Tauri's
    /// `getUserItem('qbz-purchases-region-notice-seen') !== 'true'`). Seeds
    /// `PurchasesState.show-region-notice` as `!region_notice_seen` at startup.
    #[serde(default)]
    pub purchases_region_notice_seen: bool,
    /// Three-state sidebar: 0 open / 1 mini / 2 closed. Restored at startup.
    #[serde(default)]
    pub sidebar_state: i32,
    /// Section navigation lives in the sidebar (vs the header). Default true.
    #[serde(default = "default_nav_in_sidebar")]
    pub nav_in_sidebar: bool,
    /// Header nav (nav_in_sidebar = false) always uses the icon-only compact
    /// mode instead of the full text tabs. Default false (opt-in).
    #[serde(default)]
    pub nav_header_compact: bool,
    /// Player volume, 0.0..=1.0. Restored at startup. Default full.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Startup page: "home" (always Home) or "remember" (restore last_view).
    /// Maps to Settings > Appearance > Startup page.
    #[serde(default = "default_startup_page")]
    pub startup_page: String,
    /// Last visited SAFE top-level view (no required id), for "remember". One of
    /// "home" | "discover" | "favorites" | "local-library" | "mixtapes" |
    /// "collections". Detail views (album/artist/playlist/…) are never stored.
    #[serde(default = "default_last_view")]
    pub last_view: String,
    /// Full last nav destination as JSON-encoded `nav::NavEntry`, for
    /// "remember". Unlike `last_view` (top-level only) this restores the EXACT
    /// view — album/artist/playlist/mix/label/etc. (re-fetched by id, falling
    /// back to Home on failure). `None` until a view is visited. Search and
    /// Settings are intentionally not persisted here (transient/config).
    #[serde(default)]
    pub last_nav: Option<String>,
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
    /// App-wide dynamic background key: `"off"` (none) | `"ambient"` (GPU shader
    /// scene, wgpu tier only) | `"blurred"` (blurred-artwork atmosphere). See
    /// [`DEFAULT_APP_BACKGROUND`].
    #[serde(default = "default_app_background")]
    pub app_background: String,
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
    /// Auto-theme source (only meaningful when `theme == "auto"`): `"system"`
    /// (DE color scheme → wallpaper fallback) | `"wallpaper"` | `"image"`.
    /// Persisted so the dynamic theme regenerates from the same source across
    /// restarts. See [`DEFAULT_AUTO_THEME_SOURCE`].
    #[serde(default = "default_auto_theme_source")]
    pub auto_theme_source: String,
    /// Absolute path to the user's custom image for the `"image"` auto-theme
    /// source. Empty until the user picks one. Ignored for other sources.
    #[serde(default)]
    pub auto_theme_image_path: String,

    // ---- Keyboard shortcuts (hotkeys) ----------------------------------
    /// User keybinding overrides: action id → shortcut string. Only actions
    /// the user re-bound away from their default are stored (mirrors Tauri's
    /// `qbz_keybindings` localStorage overrides). A missing entry means the
    /// action uses its compiled default (see `crate::keybindings::ACTIONS`).
    #[serde(default)]
    pub keybindings: BTreeMap<String, String>,

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

    // ---- Main window geometry ------------------------------------------
    /// Last main-window LOGICAL size. 0 = never saved → use the `.slint`
    /// preferred size. Restored at startup, clamped to the monitor's size so a
    /// smaller display never opens an oversized window.
    #[serde(default)]
    pub window_width: f32,
    #[serde(default)]
    pub window_height: f32,
    /// Last main-window PHYSICAL outer position. `i32::MIN` = never saved → let
    /// the window manager place it. Nice-to-have; clamped on-screen at restore.
    #[serde(default = "default_window_pos")]
    pub window_x: i32,
    #[serde(default = "default_window_pos")]
    pub window_y: i32,
    /// Last main-window maximized state (restored at startup and on every
    /// tray / miniplayer re-show — #618). While true, `window_width/height`
    /// keep the FLOATING size (the Resized handler skips persistence).
    #[serde(default)]
    pub window_maximized: bool,
    /// Renderer tier override: `"auto"` (heuristic picks the best tier) |
    /// `"wgpu"` | `"gl"` | `"software"`. Linux-only surface; read BEFORE the
    /// window exists (`select_slint_backend`), so changes apply on restart.
    /// A non-"auto" value is protected by the startup auto-revert sentinel
    /// (see `renderer_sentinel` in main.rs): if the app dies before the first
    /// window comes up, the next start reverts this to "auto".
    #[serde(default = "default_renderer")]
    pub renderer: String,
    /// Preferred GPU adapter: "auto" | "integrated" | "discrete". Drives the
    /// wgpu PowerPreference at startup (WGPU_POWER_PREF env still wins). On a
    /// hybrid laptop "discrete" moves the render off the integrated GPU
    /// (thermals); requires restart. See main.rs `gpu_power_from_prefs`.
    #[serde(default = "default_gpu_power")]
    pub gpu_power: String,
    /// App version that AUTO-degraded `renderer` (the ladder persisted "gl"
    /// or "software" after failed starts). Empty = `renderer` is the user's
    /// own choice. A NEW build re-probes "auto" once (vendored renderer
    /// fixes / driver updates are likely) — the ladder re-degrades within
    /// one start if the stack is still broken. Cleared when the user picks
    /// a renderer manually in Settings.
    #[serde(default)]
    pub renderer_auto_degraded: String,
    /// App version whose ALT-adapter wgpu rung SURVIVED a session (stamped
    /// at sentinel-disarm time). While it matches the running build, fresh
    /// auto-detects arm the alternate adapter directly — without this the
    /// rung-2 success dies with the process and a #542-family machine would
    /// crash every other launch forever. Version-keyed like
    /// `renderer_auto_degraded`; cleared on a manual renderer pick.
    #[serde(default)]
    pub renderer_wgpu_alt: String,
    /// Interface-size preset: `"default"` | `"small"` | `"large"` | `"xl"`.
    /// Read at the very top of main() (before ANY thread exists) to set
    /// SLINT_SCALE_FACTOR, so changes apply on restart.
    #[serde(default = "default_ui_scale")]
    pub ui_scale: String,
    /// Last observed compositor device-pixel-ratio. SLINT_SCALE_FACTOR
    /// OVERRIDES the compositor DPR (it does not multiply), so a scaled
    /// launch must bake the real DPR into the env value itself:
    /// `env = last_dpr × preset`. Refreshed a few seconds after the window
    /// maps (winit reports the real compositor value there).
    #[serde(default = "default_last_dpr")]
    pub last_dpr: f32,
    /// Startup profile: `"desktop"` (default) | `"kiosk"`. The kiosk profile
    /// boots the main window fullscreen and forces reduce-motion — a small-
    /// panel touch appliance (2.0.2 frente #3). `QBZ_PROFILE=kiosk` env
    /// OVERRIDES this at startup (the kiosk image sets it in the autostart);
    /// the image also pins `QBZ_RENDERER=gl` and the XS `ui_scale` preset via
    /// env — those stay separate knobs, not forced by the profile.
    #[serde(default = "default_profile")]
    pub profile: String,
}

/// Sentinel for "no saved window position" (let the WM place the window).
fn default_window_pos() -> i32 {
    i32::MIN
}

/// Per-OS chrome default: Linux keeps the system decorations; macOS defaults
/// to the overlay (custom) mode — see the field doc.
fn default_use_system_title_bar() -> bool {
    !cfg!(target_os = "macos")
}

fn default_show_window_controls() -> bool {
    true
}

fn default_wc_position() -> String {
    "right".to_string()
}

fn default_gpu_power() -> String {
    "auto".to_string()
}

fn default_renderer() -> String {
    "auto".to_string()
}

fn default_ui_scale() -> String {
    "default".to_string()
}

fn default_profile() -> String {
    "desktop".to_string()
}

fn default_last_dpr() -> f32 {
    1.0
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

/// Default Purchases quality filter (`"all"` = no filtering).
fn default_purchases_quality_filter() -> String {
    "all".to_string()
}

fn default_npb_mode() -> String {
    DEFAULT_NPB_MODE.to_string()
}

fn default_language() -> String {
    DEFAULT_LANGUAGE.to_string()
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

fn default_window_title_show() -> bool {
    false
}
fn default_show_volume_steppers() -> bool {
    false
}
fn default_sidebar_playlist_collage() -> bool {
    true
}
fn default_local_library_track_artwork() -> bool {
    false
}
fn default_in_app_toasts() -> bool {
    true
}
fn default_theme_filter() -> i32 {
    0
}

fn default_immersive_search_action() -> String {
    DEFAULT_IMMERSIVE_SEARCH_ACTION.to_string()
}

fn default_immersive_default_view() -> String {
    DEFAULT_IMMERSIVE_DEFAULT_VIEW.to_string()
}

fn default_app_background() -> String {
    DEFAULT_APP_BACKGROUND.to_string()
}

/// Default theme slug. Owner decision 2026-06-20: OLED Dark is the default for
/// fresh installs and any profile without a persisted theme. Sourced from the
/// `qbz-theme` registry so the default stays single-sourced.
fn default_theme() -> String {
    qbz_theme::default_slug().to_string()
}

fn default_auto_theme_source() -> String {
    DEFAULT_AUTO_THEME_SOURCE.to_string()
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            streaming_quality: default_streaming_quality(),
            npb_mode: default_npb_mode(),
            language: default_language(),
            large_visualizer: default_large_visualizer(),
            large_spectrum_mode: default_large_spectrum_mode(),
            album_header_gradient: default_album_header_gradient(),
            intelligent_search: default_intelligent_search(),
            window_title_show: default_window_title_show(),
            show_volume_steppers: default_show_volume_steppers(),
            sidebar_playlist_collage: default_sidebar_playlist_collage(),
            local_library_track_artwork: default_local_library_track_artwork(),
            in_app_toasts: default_in_app_toasts(),
            theme_filter: default_theme_filter(),
            system_notifications: default_system_notifications(),
            musicbrainz_enabled: default_musicbrainz_enabled(),
            discord_rpc_enabled: false,
            show_purchases: false,
            nav_tb_purchases: false,
            use_system_title_bar: default_use_system_title_bar(),
            hide_title_bar: false,
            show_window_controls: default_show_window_controls(),
            wc_position: default_wc_position(),
            purchases_hide_unavailable: false,
            purchases_hide_downloaded: false,
            purchases_quality_filter: default_purchases_quality_filter(),
            purchases_region_notice_seen: false,
            sidebar_state: 0,
            nav_in_sidebar: default_nav_in_sidebar(),
            nav_header_compact: false,
            volume: default_volume(),
            startup_page: default_startup_page(),
            last_view: default_last_view(),
            last_nav: None,
            immersive_search_action: default_immersive_search_action(),
            immersive_default_view: default_immersive_default_view(),
            app_background: default_app_background(),
            immersive_last_view_mode: 0,
            immersive_last_mode: 0,
            immersive_last_split_panel: 0,
            theme: default_theme(),
            auto_theme_source: default_auto_theme_source(),
            auto_theme_image_path: String::new(),
            keybindings: BTreeMap::new(),
            mini_surface: default_mini_surface(),
            mini_width: default_mini_width(),
            mini_height: default_mini_height(),
            mini_background_blur: false,
            mini_default_view: default_mini_default_view(),
            window_width: 0.0,
            window_height: 0.0,
            window_x: default_window_pos(),
            window_y: default_window_pos(),
            window_maximized: false,
            renderer: default_renderer(),
            gpu_power: default_gpu_power(),
            renderer_auto_degraded: String::new(),
            renderer_wgpu_alt: String::new(),
            ui_scale: default_ui_scale(),
            last_dpr: default_last_dpr(),
            profile: default_profile(),
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

/// Map a persisted streaming-quality key to the Qobuz format id the
/// request layer expects (`Quality`). Unknown/unset keys fall back to the
/// default tier (`Hi-Res+` = `Quality::UltraHiRes`), mirroring
/// `streaming_quality_index`.
pub fn streaming_quality_for_key(key: &str) -> Quality {
    match key {
        "mp3" => Quality::Mp3,
        "cd" => Quality::Lossless,
        "hires" => Quality::HiRes,
        _ => Quality::UltraHiRes, // "hires_plus" + unknown keys
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
    fn quality_key_maps_to_qobuz_format_id() {
        assert_eq!(streaming_quality_for_key("mp3"), Quality::Mp3);
        assert_eq!(streaming_quality_for_key("cd"), Quality::Lossless);
        assert_eq!(streaming_quality_for_key("hires"), Quality::HiRes);
        assert_eq!(streaming_quality_for_key("hires_plus"), Quality::UltraHiRes);
        // Unknown/unset keys fall back to the default tier.
        assert_eq!(streaming_quality_for_key("bogus"), Quality::UltraHiRes);
        assert_eq!(streaming_quality_for_key(""), Quality::UltraHiRes);
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
