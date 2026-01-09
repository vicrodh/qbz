//! QBZ-NIX: Native Qobuz client for Linux
//!
//! A high-fidelity music streaming client for Qobuz, designed for audiophiles
//! who need bit-perfect playback without browser sample rate limitations.

pub mod api;
pub mod commands;
pub mod config;
pub mod media_controls;
pub mod player;
pub mod queue;

use std::sync::Arc;
use tokio::sync::Mutex;

use api::QobuzClient;
use media_controls::{MediaControlsManager, TrackInfo};
use player::Player;

/// Application state shared across commands
pub struct AppState {
    pub client: Arc<Mutex<QobuzClient>>,
    pub player: Player,
    pub media_controls: MediaControlsManager,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(QobuzClient::default())),
            player: Player::new(),
            media_controls: MediaControlsManager::new(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Update MPRIS metadata when track changes
pub fn update_media_controls_metadata(
    media_controls: &MediaControlsManager,
    title: &str,
    artist: &str,
    album: &str,
    duration_secs: Option<u64>,
    cover_url: Option<String>,
) {
    let track_info = TrackInfo {
        title: title.to_string(),
        artist: artist.to_string(),
        album: album.to_string(),
        duration_secs,
        cover_url,
    };
    media_controls.set_metadata(&track_info);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("QBZ starting...");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            // Auth commands
            commands::init_client,
            commands::login,
            commands::logout,
            commands::is_logged_in,
            commands::get_user_info,
            // Search commands
            commands::search_albums,
            commands::search_tracks,
            commands::search_artists,
            commands::get_album,
            commands::get_track,
            // Playback commands
            commands::play_track,
            commands::pause_playback,
            commands::resume_playback,
            commands::stop_playback,
            commands::set_volume,
            commands::seek,
            commands::get_playback_state,
            commands::set_media_metadata,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
