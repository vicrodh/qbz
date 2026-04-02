//! Unified settings export/import for QBZ
//!
//! Collects all user-configurable settings into a single serializable struct for
//! backup, restore, and headless setup. Credentials, tokens, session state, and
//! hardware-specific identifiers are intentionally excluded.

use serde::{Deserialize, Serialize};

use crate::audio::{AlsaPlugin, AudioBackendType};
use crate::config::{
    audio_settings::AudioSettingsStore,
    developer_settings::DeveloperSettingsStore,
    download_settings::DownloadSettingsStore,
    favorites_preferences::FavoritesPreferencesStore,
    graphics_settings::GraphicsSettingsStore,
    image_cache_settings::ImageCacheSettingsStore,
    playback_preferences::{AutoplayMode, PlaybackPreferencesStore},
    remote_control_settings::RemoteControlSettingsStore,
    tray_settings::TraySettingsStore,
    window_settings::WindowSettingsStore,
};

// ============================================================================
// Per-category export structs
// All fields are Option<T> so partial exports / forward-compatibility work.
// ============================================================================

/// Audio playback settings (excludes output_device and device_max_sample_rate
/// because those are hardware-specific identifiers that won't transfer).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AudioExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclusive_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dac_passthrough: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_type: Option<AudioBackendType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alsa_plugin: Option<AlsaPlugin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alsa_hardware_volume: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_first_track: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_buffer_seconds: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_quality_to_device: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalization_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalization_target_lufs: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gapless_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pw_force_bitperfect: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_audio_on_startup: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_fallback_behavior: Option<String>,
}

/// Download preferences.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DownloadExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_in_library: Option<bool>,
}

/// Playback behaviour preferences.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlaybackExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoplay_mode: Option<AutoplayMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_context_icon: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persist_session: Option<bool>,
}

/// Favourites view preferences (icon, background, tab order).
/// `custom_icon_path` is excluded because it points to a local file that
/// won't exist on the target machine.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FavoritesExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_icon_preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_order: Option<Vec<String>>,
}

/// System tray preferences.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrayExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_tray: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimize_to_tray: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_to_tray: Option<bool>,
}

/// GPU / rendering preferences.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphicsExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware_acceleration: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_x11: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gdk_scale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gdk_dpi_scale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gsk_renderer: Option<String>,
}

/// Developer / debug toggles.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeveloperExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_dmabuf: Option<bool>,
}

/// Remote control / API server preferences.
/// `token` is excluded — it is a security credential.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteControlExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,
}

/// Window geometry preferences.
/// `window_width`, `window_height`, and `is_maximized` are excluded because
/// they are transient session state, not portable preferences.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_system_titlebar: Option<bool>,
}

/// Image cache preferences.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageCacheExport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size_mb: Option<u32>,
}

// ============================================================================
// Top-level exported settings bundle
// ============================================================================

/// A complete, version-tagged snapshot of all exportable QBZ settings.
///
/// Each category is wrapped in `Option` so that:
/// - A missing category during import means "leave as-is" (additive restore).
/// - A store that fails to read does not abort the entire export.
///
/// **Excluded from export:**
/// - Credentials / API tokens (passwords, auth tokens, remote-control pairing token)
/// - Session state (playback position, queue)
/// - Hardware-specific identifiers (output_device name, device_max_sample_rate,
///   device_sample_rate_limits, custom_icon_path)
/// - Legal acceptance records (machine-specific)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExportedSettings {
    /// Schema version for future forward-compatibility checks.
    pub version: u32,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioExport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub download: Option<DownloadExport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback: Option<PlaybackExport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub favorites: Option<FavoritesExport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tray: Option<TrayExport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub graphics: Option<GraphicsExport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub developer: Option<DeveloperExport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_control: Option<RemoteControlExport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowExport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_cache: Option<ImageCacheExport>,
}

/// Write all exportable settings to `path` as pretty-printed JSON.
///
/// Returns `Err` only on I/O or serialization failure.  Individual store
/// read failures are logged as warnings and result in `None` for that
/// category — they do NOT abort the export.
pub fn export_to_file(path: &str) -> Result<(), String> {
    let settings = ExportedSettings::collect();
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    std::fs::write(path, json)
        .map_err(|e| format!("Failed to write settings file '{}': {}", path, e))?;
    Ok(())
}

/// Read settings from a JSON file at `path` and apply each non-None
/// section to its corresponding settings store.
///
/// All per-field errors are collected and reported at the end rather than
/// aborting on the first failure, so a partial import is still applied.
pub fn import_from_file(path: &str) -> Result<(), String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read settings file '{}': {}", path, e))?;
    let settings: ExportedSettings = serde_json::from_str(&contents)
        .map_err(|e| format!("Failed to parse settings JSON: {}", e))?;

    let mut errors: Vec<String> = Vec::new();

    // --- Audio ---
    if let Some(audio) = settings.audio {
        apply_audio(&audio, &mut errors);
    }

    // --- Download ---
    if let Some(download) = settings.download {
        apply_download(&download, &mut errors);
    }

    // --- Playback ---
    if let Some(playback) = settings.playback {
        apply_playback(&playback, &mut errors);
    }

    // --- Favorites ---
    if let Some(favorites) = settings.favorites {
        apply_favorites(&favorites, &mut errors);
    }

    // --- Tray ---
    if let Some(tray) = settings.tray {
        apply_tray(&tray, &mut errors);
    }

    // --- Graphics ---
    if let Some(graphics) = settings.graphics {
        apply_graphics(&graphics, &mut errors);
    }

    // --- Developer ---
    if let Some(developer) = settings.developer {
        apply_developer(&developer, &mut errors);
    }

    // --- Remote control ---
    if let Some(remote_control) = settings.remote_control {
        apply_remote_control(&remote_control, &mut errors);
    }

    // --- Window ---
    if let Some(window) = settings.window {
        apply_window(&window, &mut errors);
    }

    // --- Image cache ---
    if let Some(image_cache) = settings.image_cache {
        apply_image_cache(&image_cache, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} error(s) during import:\n{}",
            errors.len(),
            errors.join("\n")
        ))
    }
}

// ============================================================================
// Per-category apply helpers (pub so Tauri commands can reuse them)
// ============================================================================

pub fn apply_audio(audio: &AudioExport, errors: &mut Vec<String>) {
    let store = match AudioSettingsStore::new() {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("audio_settings open: {}", e));
            return;
        }
    };

    if let Some(v) = audio.exclusive_mode {
        if let Err(e) = store.set_exclusive_mode(v) {
            errors.push(format!("audio.exclusive_mode: {}", e));
        }
    }
    if let Some(v) = audio.dac_passthrough {
        if let Err(e) = store.set_dac_passthrough(v) {
            errors.push(format!("audio.dac_passthrough: {}", e));
        }
    }
    if let Some(v) = audio.preferred_sample_rate {
        if let Err(e) = store.set_sample_rate(Some(v)) {
            errors.push(format!("audio.preferred_sample_rate: {}", e));
        }
    }
    if let Some(ref v) = audio.backend_type {
        if let Err(e) = store.set_backend_type(Some(v.clone())) {
            errors.push(format!("audio.backend_type: {}", e));
        }
    }
    if let Some(ref v) = audio.alsa_plugin {
        if let Err(e) = store.set_alsa_plugin(Some(v.clone())) {
            errors.push(format!("audio.alsa_plugin: {}", e));
        }
    }
    if let Some(v) = audio.alsa_hardware_volume {
        if let Err(e) = store.set_alsa_hardware_volume(v) {
            errors.push(format!("audio.alsa_hardware_volume: {}", e));
        }
    }
    if let Some(v) = audio.stream_first_track {
        if let Err(e) = store.set_stream_first_track(v) {
            errors.push(format!("audio.stream_first_track: {}", e));
        }
    }
    if let Some(v) = audio.stream_buffer_seconds {
        if let Err(e) = store.set_stream_buffer_seconds(v) {
            errors.push(format!("audio.stream_buffer_seconds: {}", e));
        }
    }
    if let Some(v) = audio.streaming_only {
        if let Err(e) = store.set_streaming_only(v) {
            errors.push(format!("audio.streaming_only: {}", e));
        }
    }
    if let Some(v) = audio.limit_quality_to_device {
        if let Err(e) = store.set_limit_quality_to_device(v) {
            errors.push(format!("audio.limit_quality_to_device: {}", e));
        }
    }
    if let Some(v) = audio.normalization_enabled {
        if let Err(e) = store.set_normalization_enabled(v) {
            errors.push(format!("audio.normalization_enabled: {}", e));
        }
    }
    if let Some(v) = audio.normalization_target_lufs {
        if let Err(e) = store.set_normalization_target_lufs(v) {
            errors.push(format!("audio.normalization_target_lufs: {}", e));
        }
    }
    if let Some(v) = audio.gapless_enabled {
        if let Err(e) = store.set_gapless_enabled(v) {
            errors.push(format!("audio.gapless_enabled: {}", e));
        }
    }
    if let Some(v) = audio.pw_force_bitperfect {
        if let Err(e) = store.set_pw_force_bitperfect(v) {
            errors.push(format!("audio.pw_force_bitperfect: {}", e));
        }
    }
    if let Some(v) = audio.sync_audio_on_startup {
        if let Err(e) = store.set_sync_audio_on_startup(v) {
            errors.push(format!("audio.sync_audio_on_startup: {}", e));
        }
    }
    if let Some(ref v) = audio.quality_fallback_behavior {
        if let Err(e) = store.set_quality_fallback_behavior(v) {
            errors.push(format!("audio.quality_fallback_behavior: {}", e));
        }
    }
}

pub fn apply_download(download: &DownloadExport, errors: &mut Vec<String>) {
    let store = match DownloadSettingsStore::new() {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("download_settings open: {}", e));
            return;
        }
    };

    if let Some(ref v) = download.download_root {
        if let Err(e) = store.set_download_root(v) {
            errors.push(format!("download.download_root: {}", e));
        }
    }
    if let Some(v) = download.show_in_library {
        if let Err(e) = store.set_show_in_library(v) {
            errors.push(format!("download.show_in_library: {}", e));
        }
    }
}

pub fn apply_playback(playback: &PlaybackExport, errors: &mut Vec<String>) {
    let store = match PlaybackPreferencesStore::new() {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("playback_preferences open: {}", e));
            return;
        }
    };

    if let Some(v) = playback.autoplay_mode {
        if let Err(e) = store.set_autoplay_mode(v) {
            errors.push(format!("playback.autoplay_mode: {}", e));
        }
    }
    if let Some(v) = playback.show_context_icon {
        if let Err(e) = store.set_show_context_icon(v) {
            errors.push(format!("playback.show_context_icon: {}", e));
        }
    }
    if let Some(v) = playback.persist_session {
        if let Err(e) = store.set_persist_session(v) {
            errors.push(format!("playback.persist_session: {}", e));
        }
    }
}

pub fn apply_favorites(favorites: &FavoritesExport, errors: &mut Vec<String>) {
    let store = match FavoritesPreferencesStore::new() {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("favorites_preferences open: {}", e));
            return;
        }
    };

    // Read current prefs so we can do a partial update via save_preferences
    let mut prefs = match store.get_preferences() {
        Ok(p) => p,
        Err(e) => {
            errors.push(format!("favorites_preferences read: {}", e));
            return;
        }
    };

    // custom_icon_preset: exported as Option<String>, stored as Option<String>
    if favorites.custom_icon_preset.is_some() {
        prefs.custom_icon_preset = favorites.custom_icon_preset.clone();
    }
    // icon_background
    if favorites.icon_background.is_some() {
        prefs.icon_background = favorites.icon_background.clone();
    }
    // tab_order
    if let Some(ref order) = favorites.tab_order {
        prefs.tab_order = order.clone();
    }
    // custom_icon_path is intentionally NOT applied (hardware-specific path)

    if let Err(e) = store.save_preferences(prefs) {
        errors.push(format!("favorites_preferences save: {}", e));
    }
}

pub fn apply_tray(tray: &TrayExport, errors: &mut Vec<String>) {
    let store = match TraySettingsStore::new() {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("tray_settings open: {}", e));
            return;
        }
    };

    if let Some(v) = tray.enable_tray {
        if let Err(e) = store.set_enable_tray(v) {
            errors.push(format!("tray.enable_tray: {}", e));
        }
    }
    if let Some(v) = tray.minimize_to_tray {
        if let Err(e) = store.set_minimize_to_tray(v) {
            errors.push(format!("tray.minimize_to_tray: {}", e));
        }
    }
    if let Some(v) = tray.close_to_tray {
        if let Err(e) = store.set_close_to_tray(v) {
            errors.push(format!("tray.close_to_tray: {}", e));
        }
    }
}

pub fn apply_graphics(graphics: &GraphicsExport, errors: &mut Vec<String>) {
    let store = match GraphicsSettingsStore::new() {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("graphics_settings open: {}", e));
            return;
        }
    };

    if let Some(v) = graphics.hardware_acceleration {
        if let Err(e) = store.set_hardware_acceleration(v) {
            errors.push(format!("graphics.hardware_acceleration: {}", e));
        }
    }
    if let Some(v) = graphics.force_x11 {
        if let Err(e) = store.set_force_x11(v) {
            errors.push(format!("graphics.force_x11: {}", e));
        }
    }
    // gdk_scale / gdk_dpi_scale / gsk_renderer are Option<String> — import as-is
    if graphics.gdk_scale.is_some() || matches!(graphics.gdk_scale, None) {
        // Only write if the field was present in the export (non-default)
        // The field uses skip_serializing_if = "Option::is_none", so if it
        // deserialized to Some(_), the user explicitly set it.
        if let Some(ref v) = graphics.gdk_scale {
            if let Err(e) = store.set_gdk_scale(Some(v.clone())) {
                errors.push(format!("graphics.gdk_scale: {}", e));
            }
        }
    }
    if let Some(ref v) = graphics.gdk_dpi_scale {
        if let Err(e) = store.set_gdk_dpi_scale(Some(v.clone())) {
            errors.push(format!("graphics.gdk_dpi_scale: {}", e));
        }
    }
    if let Some(ref v) = graphics.gsk_renderer {
        if let Err(e) = store.set_gsk_renderer(Some(v.clone())) {
            errors.push(format!("graphics.gsk_renderer: {}", e));
        }
    }
}

pub fn apply_developer(developer: &DeveloperExport, errors: &mut Vec<String>) {
    let store = match DeveloperSettingsStore::new() {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("developer_settings open: {}", e));
            return;
        }
    };

    if let Some(v) = developer.force_dmabuf {
        if let Err(e) = store.set_force_dmabuf(v) {
            errors.push(format!("developer.force_dmabuf: {}", e));
        }
    }
}

pub fn apply_remote_control(remote_control: &RemoteControlExport, errors: &mut Vec<String>) {
    let store = match RemoteControlSettingsStore::new() {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("remote_control_settings open: {}", e));
            return;
        }
    };

    if let Some(v) = remote_control.enabled {
        if let Err(e) = store.set_enabled(v) {
            errors.push(format!("remote_control.enabled: {}", e));
        }
    }
    if let Some(v) = remote_control.port {
        if let Err(e) = store.set_port(v) {
            errors.push(format!("remote_control.port: {}", e));
        }
    }
    if let Some(v) = remote_control.secure {
        if let Err(e) = store.set_secure(v) {
            errors.push(format!("remote_control.secure: {}", e));
        }
    }
    // token is intentionally NOT applied — it is a security credential
}

pub fn apply_window(window: &WindowExport, errors: &mut Vec<String>) {
    let store = match WindowSettingsStore::new() {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("window_settings open: {}", e));
            return;
        }
    };

    if let Some(v) = window.use_system_titlebar {
        if let Err(e) = store.set_use_system_titlebar(v) {
            errors.push(format!("window.use_system_titlebar: {}", e));
        }
    }
}

pub fn apply_image_cache(image_cache: &ImageCacheExport, errors: &mut Vec<String>) {
    let store = match ImageCacheSettingsStore::new() {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("image_cache_settings open: {}", e));
            return;
        }
    };

    if let Some(v) = image_cache.enabled {
        if let Err(e) = store.set_enabled(v) {
            errors.push(format!("image_cache.enabled: {}", e));
        }
    }
    if let Some(v) = image_cache.max_size_mb {
        if let Err(e) = store.set_max_size_mb(v) {
            errors.push(format!("image_cache.max_size_mb: {}", e));
        }
    }
}

impl ExportedSettings {
    /// Current schema version written to every export.
    pub const CURRENT_VERSION: u32 = 1;

    /// Read all settings from the default data directory and populate the struct.
    ///
    /// Individual store failures are logged and result in `None` for that category
    /// rather than aborting the whole export.
    pub fn collect() -> Self {
        let mut out = Self {
            version: Self::CURRENT_VERSION,
            ..Default::default()
        };

        // Audio settings
        match AudioSettingsStore::new() {
            Ok(store) => match store.get_settings() {
                Ok(s) => {
                    out.audio = Some(AudioExport {
                        exclusive_mode: Some(s.exclusive_mode),
                        dac_passthrough: Some(s.dac_passthrough),
                        preferred_sample_rate: s.preferred_sample_rate,
                        backend_type: s.backend_type,
                        alsa_plugin: s.alsa_plugin,
                        alsa_hardware_volume: Some(s.alsa_hardware_volume),
                        stream_first_track: Some(s.stream_first_track),
                        stream_buffer_seconds: Some(s.stream_buffer_seconds),
                        streaming_only: Some(s.streaming_only),
                        limit_quality_to_device: Some(s.limit_quality_to_device),
                        normalization_enabled: Some(s.normalization_enabled),
                        normalization_target_lufs: Some(s.normalization_target_lufs),
                        gapless_enabled: Some(s.gapless_enabled),
                        pw_force_bitperfect: Some(s.pw_force_bitperfect),
                        sync_audio_on_startup: Some(s.sync_audio_on_startup),
                        quality_fallback_behavior: Some(s.quality_fallback_behavior),
                    });
                }
                Err(e) => log::warn!("[settings_export] audio_settings read failed: {}", e),
            },
            Err(e) => log::warn!("[settings_export] audio_settings open failed: {}", e),
        }

        // Download settings
        match DownloadSettingsStore::new() {
            Ok(store) => match store.get_settings() {
                Ok(s) => {
                    out.download = Some(DownloadExport {
                        download_root: Some(s.download_root),
                        show_in_library: Some(s.show_in_library),
                    });
                }
                Err(e) => log::warn!("[settings_export] download_settings read failed: {}", e),
            },
            Err(e) => log::warn!("[settings_export] download_settings open failed: {}", e),
        }

        // Playback preferences
        match PlaybackPreferencesStore::new() {
            Ok(store) => match store.get_preferences() {
                Ok(s) => {
                    out.playback = Some(PlaybackExport {
                        autoplay_mode: Some(s.autoplay_mode),
                        show_context_icon: Some(s.show_context_icon),
                        persist_session: Some(s.persist_session),
                    });
                }
                Err(e) => log::warn!(
                    "[settings_export] playback_preferences read failed: {}",
                    e
                ),
            },
            Err(e) => log::warn!("[settings_export] playback_preferences open failed: {}", e),
        }

        // Favourites preferences
        match FavoritesPreferencesStore::new() {
            Ok(store) => match store.get_preferences() {
                Ok(s) => {
                    out.favorites = Some(FavoritesExport {
                        // custom_icon_path excluded: local filesystem path, not portable
                        custom_icon_preset: s.custom_icon_preset,
                        icon_background: s.icon_background,
                        tab_order: Some(s.tab_order),
                    });
                }
                Err(e) => log::warn!(
                    "[settings_export] favorites_preferences read failed: {}",
                    e
                ),
            },
            Err(e) => log::warn!(
                "[settings_export] favorites_preferences open failed: {}",
                e
            ),
        }

        // Tray settings
        match TraySettingsStore::new() {
            Ok(store) => match store.get_settings() {
                Ok(s) => {
                    out.tray = Some(TrayExport {
                        enable_tray: Some(s.enable_tray),
                        minimize_to_tray: Some(s.minimize_to_tray),
                        close_to_tray: Some(s.close_to_tray),
                    });
                }
                Err(e) => log::warn!("[settings_export] tray_settings read failed: {}", e),
            },
            Err(e) => log::warn!("[settings_export] tray_settings open failed: {}", e),
        }

        // Graphics settings
        match GraphicsSettingsStore::new() {
            Ok(store) => match store.get_settings() {
                Ok(s) => {
                    out.graphics = Some(GraphicsExport {
                        hardware_acceleration: Some(s.hardware_acceleration),
                        force_x11: Some(s.force_x11),
                        gdk_scale: s.gdk_scale,
                        gdk_dpi_scale: s.gdk_dpi_scale,
                        gsk_renderer: s.gsk_renderer,
                    });
                }
                Err(e) => log::warn!("[settings_export] graphics_settings read failed: {}", e),
            },
            Err(e) => log::warn!("[settings_export] graphics_settings open failed: {}", e),
        }

        // Developer settings
        match DeveloperSettingsStore::new() {
            Ok(store) => match store.get_settings() {
                Ok(s) => {
                    out.developer = Some(DeveloperExport {
                        force_dmabuf: Some(s.force_dmabuf),
                    });
                }
                Err(e) => log::warn!("[settings_export] developer_settings read failed: {}", e),
            },
            Err(e) => log::warn!("[settings_export] developer_settings open failed: {}", e),
        }

        // Remote control settings (token excluded)
        match RemoteControlSettingsStore::new() {
            Ok(store) => match store.get_settings() {
                Ok(s) => {
                    out.remote_control = Some(RemoteControlExport {
                        enabled: Some(s.enabled),
                        port: Some(s.port),
                        secure: Some(s.secure),
                        // token excluded: security credential
                    });
                }
                Err(e) => log::warn!(
                    "[settings_export] remote_control_settings read failed: {}",
                    e
                ),
            },
            Err(e) => log::warn!(
                "[settings_export] remote_control_settings open failed: {}",
                e
            ),
        }

        // Window settings (window geometry excluded — session state)
        match WindowSettingsStore::new() {
            Ok(store) => match store.get_settings() {
                Ok(s) => {
                    out.window = Some(WindowExport {
                        use_system_titlebar: Some(s.use_system_titlebar),
                        // window_width, window_height, is_maximized excluded: transient session state
                    });
                }
                Err(e) => log::warn!("[settings_export] window_settings read failed: {}", e),
            },
            Err(e) => log::warn!("[settings_export] window_settings open failed: {}", e),
        }

        // Image cache settings
        match ImageCacheSettingsStore::new() {
            Ok(store) => match store.get_settings() {
                Ok(s) => {
                    out.image_cache = Some(ImageCacheExport {
                        enabled: Some(s.enabled),
                        max_size_mb: Some(s.max_size_mb),
                    });
                }
                Err(e) => log::warn!(
                    "[settings_export] image_cache_settings read failed: {}",
                    e
                ),
            },
            Err(e) => log::warn!("[settings_export] image_cache_settings open failed: {}", e),
        }

        out
    }
}
