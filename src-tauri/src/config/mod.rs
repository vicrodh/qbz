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
pub mod download_settings;
pub mod playback_preferences;
pub mod favorites_preferences;

pub use audio_settings::{
    AudioSettings,
    AudioSettingsState,
    get_audio_settings,
    set_audio_output_device,
    set_audio_exclusive_mode,
    set_audio_dac_passthrough,
    set_audio_sample_rate,
};

pub use download_settings::{
    DownloadSettings,
    DownloadSettingsState,
    get_download_settings,
    set_download_root,
    set_show_downloads_in_library,
    validate_download_root,
};

pub use playback_preferences::{
    AutoplayMode,
    PlaybackPreferences,
    PlaybackPreferencesState,
    get_playback_preferences,
    set_autoplay_mode,
};

pub use favorites_preferences::{
    FavoritesPreferences,
    FavoritesPreferencesState,
    get_favorites_preferences,
    save_favorites_preferences,
};
