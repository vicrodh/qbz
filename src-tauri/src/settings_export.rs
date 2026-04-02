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
