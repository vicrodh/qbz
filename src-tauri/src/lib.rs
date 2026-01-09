//! QBZ-NIX: Native Qobuz client for Linux
//!
//! A high-fidelity music streaming client for Qobuz, designed for audiophiles
//! who need bit-perfect playback without browser sample rate limitations.

pub mod api;
pub mod cache;
// NOTE: cast module disabled due to thread safety issues with rust_cast (Rc not Send)
// and rupnp API changes. Scaffolding preserved in src/cast/ for future work.
// pub mod cast;
pub mod commands;
pub mod config;
pub mod credentials;
pub mod discogs;
pub mod lastfm;
pub mod library;
pub mod media_controls;
pub mod player;
pub mod queue;
pub mod share;

use std::sync::Arc;
use tokio::sync::Mutex;

use api::QobuzClient;
use cache::AudioCache;
use lastfm::LastFmClient;
use media_controls::{MediaControlsManager, TrackInfo};
use player::Player;
use queue::QueueManager;
use share::SongLinkClient;

/// Application state shared across commands
pub struct AppState {
    pub client: Arc<Mutex<QobuzClient>>,
    pub player: Player,
    pub queue: QueueManager,
    pub media_controls: MediaControlsManager,
    pub audio_cache: Arc<AudioCache>,
    pub lastfm: Arc<Mutex<LastFmClient>>,
    pub songlink: SongLinkClient,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(QobuzClient::default())),
            player: Player::new(),
            queue: QueueManager::new(),
            media_controls: MediaControlsManager::new(),
            audio_cache: Arc::new(AudioCache::default()),
            lastfm: Arc::new(Mutex::new(LastFmClient::default())),
            songlink: SongLinkClient::new(),
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
    // Load .env file if present (for development)
    // Silently ignore if not found (production builds use compile-time env vars)
    dotenvy::dotenv().ok();

    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("QBZ starting...");

    // Initialize library state
    let library_state = library::init_library_state()
        .expect("Failed to initialize library database");

    // NOTE: Casting module is scaffolded but not wired yet.
    // The cast module has thread safety issues with rust_cast (uses Rc, not Send)
    // and API compatibility issues with rupnp that need to be resolved first.
    // See src/cast/ for the scaffolded implementation.

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::new())
        .manage(library_state)
        .invoke_handler(tauri::generate_handler![
            // Auth commands
            commands::init_client,
            commands::login,
            commands::logout,
            commands::is_logged_in,
            commands::get_user_info,
            // Credential persistence commands
            commands::has_saved_credentials,
            commands::save_credentials,
            commands::clear_saved_credentials,
            commands::auto_login,
            // Search commands
            commands::search_albums,
            commands::search_tracks,
            commands::search_artists,
            commands::get_album,
            commands::get_track,
            commands::get_artist,
            // Playback commands
            commands::play_track,
            commands::pause_playback,
            commands::resume_playback,
            commands::stop_playback,
            commands::set_volume,
            commands::seek,
            commands::get_playback_state,
            commands::set_media_metadata,
            commands::get_audio_devices,
            // Queue commands
            commands::add_to_queue,
            commands::add_to_queue_next,
            commands::add_tracks_to_queue,
            commands::set_queue,
            commands::clear_queue,
            commands::remove_from_queue,
            commands::get_current_queue_track,
            commands::peek_next_track,
            commands::next_track,
            commands::previous_track,
            commands::play_queue_index,
            commands::set_shuffle,
            commands::get_shuffle,
            commands::set_repeat,
            commands::get_repeat,
            commands::get_queue_state,
            // Playlist commands
            commands::get_user_playlists,
            commands::get_playlist,
            commands::search_playlists,
            commands::create_playlist,
            commands::delete_playlist,
            commands::add_tracks_to_playlist,
            commands::remove_tracks_from_playlist,
            commands::update_playlist,
            // Favorites commands
            commands::get_favorites,
            commands::add_favorite,
            commands::remove_favorite,
            // Notification commands
            commands::show_track_notification,
            commands::show_notification,
            // Cache commands
            commands::get_cache_stats,
            commands::clear_cache,
            // Last.fm commands
            commands::lastfm_has_embedded_credentials,
            commands::lastfm_has_credentials,
            commands::lastfm_set_credentials,
            commands::lastfm_is_authenticated,
            commands::lastfm_get_auth_url,
            commands::lastfm_open_auth_url,
            commands::lastfm_authenticate,
            commands::lastfm_set_session,
            commands::lastfm_disconnect,
            commands::lastfm_scrobble,
            commands::lastfm_now_playing,
            // Share commands
            commands::share_track_songlink,
            commands::share_album_songlink,
            commands::get_qobuz_track_url,
            commands::get_qobuz_album_url,
            commands::get_qobuz_artist_url,
            // Local library commands
            library::commands::library_add_folder,
            library::commands::library_remove_folder,
            library::commands::library_get_folders,
            library::commands::library_scan,
            library::commands::library_get_scan_progress,
            library::commands::library_get_albums,
            library::commands::library_get_album_tracks,
            library::commands::library_get_artists,
            library::commands::library_search,
            library::commands::library_get_stats,
            library::commands::library_clear,
            library::commands::library_get_track,
            library::commands::library_play_track,
            // Playlist local settings commands
            library::commands::playlist_get_settings,
            library::commands::playlist_save_settings,
            library::commands::playlist_set_sort,
            library::commands::playlist_set_artwork,
            library::commands::playlist_add_local_track,
            library::commands::playlist_remove_local_track,
            library::commands::playlist_get_local_tracks,
            library::commands::playlist_clear_local_tracks,
            // Discogs artwork commands
            library::commands::discogs_has_credentials,
            library::commands::library_fetch_missing_artwork,
            library::commands::library_fetch_album_artwork,
            // Casting commands - NOT WIRED YET (see NOTE above)
            // cast::commands::cast_* - Chromecast
            // cast::airplay::commands::airplay_* - AirPlay
            // cast::dlna::commands::dlna_* - DLNA/UPnP
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
