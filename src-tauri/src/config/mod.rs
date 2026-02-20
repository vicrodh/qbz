//! Configuration and state persistence
//!
//! Handles:
//! - User credentials (encrypted)
//! - Audio preferences
//! - Download preferences
//! - Playback preferences
//! - UI preferences
//! - Local playlists
//! - Cached favorites

pub mod audio_settings;
pub mod developer_settings;
pub mod download_settings;
pub mod favorites_cache;
pub mod favorites_preferences;
pub mod graphics_settings;
pub mod legal_settings;
pub mod playback_preferences;
pub mod remote_control_settings;
pub mod subscription_state;
pub mod tray_settings;
pub mod window_settings;

pub use audio_settings::{
    get_audio_settings, reset_audio_settings, set_audio_dac_passthrough, set_audio_exclusive_mode,
    set_audio_output_device, set_audio_sample_rate, AudioSettings, AudioSettingsState,
};

pub use download_settings::{
    get_download_settings, set_download_root, set_show_downloads_in_library,
    validate_download_root, DownloadSettings, DownloadSettingsState,
};

pub use playback_preferences::{
    get_playback_preferences, set_autoplay_mode, AutoplayMode, PlaybackPreferences,
    PlaybackPreferencesState,
};

pub use favorites_preferences::{
    get_favorites_preferences, save_favorites_preferences, FavoritesPreferences,
    FavoritesPreferencesState,
};

pub use subscription_state::{
    create_empty_subscription_state, create_subscription_state, SubscriptionState,
    SubscriptionStateState, SubscriptionStateStore,
};

pub use tray_settings::{
    get_tray_settings, set_close_to_tray, set_enable_tray, set_minimize_to_tray, TraySettings,
    TraySettingsState,
};

pub use favorites_cache::{
    cache_favorite_album, cache_favorite_artist, cache_favorite_track, clear_favorites_cache,
    get_cached_favorite_albums, get_cached_favorite_artists, get_cached_favorite_tracks,
    sync_cached_favorite_albums, sync_cached_favorite_artists, sync_cached_favorite_tracks,
    uncache_favorite_album, uncache_favorite_artist, uncache_favorite_track, FavoritesCacheState,
};

pub use legal_settings::{
    get_legal_settings, get_qobuz_tos_accepted, set_qobuz_tos_accepted, LegalSettings,
    LegalSettingsState, LegalSettingsStore,
};

pub use remote_control_settings::{
    AllowedOrigin, AllowedOriginsState, RemoteControlSettings, RemoteControlSettingsState,
};

pub use developer_settings::{
    get_developer_settings, set_developer_force_dmabuf, DeveloperSettings, DeveloperSettingsState,
};

pub use graphics_settings::{
    get_graphics_settings, set_force_x11, set_gdk_dpi_scale, set_gdk_scale,
    set_hardware_acceleration, GraphicsSettings, GraphicsSettingsState,
};

pub use window_settings::{
    get_window_settings, set_use_system_titlebar, WindowSettings, WindowSettingsState,
};
