//! QBZ-NIX: Native Qobuz client for Linux
//!
//! A high-fidelity music streaming client for Qobuz, designed for audiophiles
//! who need bit-perfect playback without browser sample rate limitations.

pub mod api;
pub mod api_cache;
pub mod cache;
pub mod cast;
pub mod commands;
pub mod config;
pub mod credentials;
pub mod discogs;
pub mod download_cache;
pub mod lastfm;
pub mod library;
pub mod lyrics;
pub mod media_controls;
pub mod player;
pub mod playlist_import;
pub mod queue;
pub mod reco_store;
pub mod session_store;
pub mod share;

use std::sync::Arc;
use tauri::{Emitter, Manager};
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
        Self::with_device(None)
    }

    pub fn with_device(device_name: Option<String>) -> Self {
        Self {
            client: Arc::new(Mutex::new(QobuzClient::default())),
            player: Player::new(device_name),
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

    // Initialize casting state (Chromecast, DLNA, AirPlay)
    let cast_state = cast::CastState::new()
        .expect("Failed to initialize Chromecast state");
    let dlna_state = cast::dlna::commands::DlnaState::new()
        .expect("Failed to initialize DLNA state");
    let airplay_state = cast::airplay::commands::AirPlayState::new()
        .expect("Failed to initialize AirPlay state");

    // Initialize download cache state
    let download_cache_state = download_cache::DownloadCacheState::new()
        .expect("Failed to initialize download cache");
    // Initialize lyrics cache state
    let lyrics_state = lyrics::LyricsState::new()
        .expect("Failed to initialize lyrics cache");
    // Initialize recommendation store state
    let reco_state = reco_store::RecoState::new()
        .expect("Failed to initialize recommendation store");
    // Initialize API cache state
    let api_cache_state = api_cache::ApiCacheState::new()
        .expect("Failed to initialize API cache");
    // Initialize session store state
    let session_store_state = session_store::SessionStoreState::new()
        .expect("Failed to initialize session store");
    // Initialize audio settings state
    let audio_settings_state = config::audio_settings::AudioSettingsState::new()
        .expect("Failed to initialize audio settings");

    // Read saved audio device setting for player initialization
    let saved_device = audio_settings_state
        .store
        .lock()
        .ok()
        .and_then(|store| store.get_settings().ok())
        .and_then(|settings| settings.output_device);

    if let Some(ref device) = saved_device {
        log::info!("Initializing player with saved device: {}", device);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::with_device(saved_device))
        .setup(|app| {
            // Start background task to emit playback events
            let app_handle = app.handle().clone();
            let player_state = app.state::<AppState>().player.state.clone();

            std::thread::spawn(move || {
                let mut last_position: u64 = 0;
                let mut last_is_playing: bool = false;
                let mut last_track_id: u64 = 0;

                loop {
                    std::thread::sleep(std::time::Duration::from_millis(250));

                    let is_playing = player_state.is_playing();
                    let position = player_state.current_position();
                    let duration = player_state.duration();
                    let track_id = player_state.current_track_id();
                    let volume = player_state.volume();

                    // Only emit if state changed or position advanced
                    let should_emit = track_id != 0 && (
                        is_playing != last_is_playing
                        || track_id != last_track_id
                        || (is_playing && position != last_position)
                    );

                    if should_emit {
                        let event = player::PlaybackEvent {
                            is_playing,
                            position,
                            duration,
                            track_id,
                            volume,
                        };
                        let _ = app_handle.emit("playback:state", &event);

                        last_position = position;
                        last_is_playing = is_playing;
                        last_track_id = track_id;
                    }
                }
            });

            Ok(())
        })
        .manage(library_state)
        .manage(cast_state)
        .manage(dlna_state)
        .manage(airplay_state)
        .manage(download_cache_state)
        .manage(lyrics_state)
        .manage(reco_state)
        .manage(api_cache_state)
        .manage(session_store_state)
        .manage(audio_settings_state)
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
            commands::get_featured_albums,
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
            commands::get_audio_output_status,
            commands::get_pipewire_sinks,
            commands::reinit_audio_device,
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
            // Playlist import commands
            commands::playlist_import_preview,
            commands::playlist_import_execute,
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
            // Chromecast casting commands
            cast::commands::cast_start_discovery,
            cast::commands::cast_stop_discovery,
            cast::commands::cast_get_devices,
            cast::commands::cast_connect,
            cast::commands::cast_disconnect,
            cast::commands::cast_get_status,
            cast::commands::cast_play_track,
            cast::commands::cast_play_local_track,
            cast::commands::cast_play,
            cast::commands::cast_pause,
            cast::commands::cast_stop,
            cast::commands::cast_seek,
            cast::commands::cast_set_volume,
            // DLNA casting commands
            cast::dlna::commands::dlna_start_discovery,
            cast::dlna::commands::dlna_stop_discovery,
            cast::dlna::commands::dlna_get_devices,
            cast::dlna::commands::dlna_connect,
            cast::dlna::commands::dlna_disconnect,
            cast::dlna::commands::dlna_get_status,
            cast::dlna::commands::dlna_load_media,
            cast::dlna::commands::dlna_play,
            cast::dlna::commands::dlna_pause,
            cast::dlna::commands::dlna_stop,
            cast::dlna::commands::dlna_set_volume,
            // AirPlay casting commands
            cast::airplay::commands::airplay_start_discovery,
            cast::airplay::commands::airplay_stop_discovery,
            cast::airplay::commands::airplay_get_devices,
            cast::airplay::commands::airplay_connect,
            cast::airplay::commands::airplay_disconnect,
            cast::airplay::commands::airplay_get_status,
            cast::airplay::commands::airplay_load_media,
            cast::airplay::commands::airplay_play,
            cast::airplay::commands::airplay_pause,
            cast::airplay::commands::airplay_stop,
            cast::airplay::commands::airplay_set_volume,
            // Download cache commands
            download_cache::commands::download_track,
            download_cache::commands::is_track_downloaded,
            download_cache::commands::get_downloaded_track_path,
            download_cache::commands::get_downloaded_track,
            download_cache::commands::get_downloaded_tracks,
            download_cache::commands::get_download_cache_stats,
            download_cache::commands::remove_downloaded_track,
            download_cache::commands::clear_download_cache,
            download_cache::commands::set_download_cache_limit,
            download_cache::commands::open_download_cache_folder,
            // Lyrics commands
            lyrics::commands::lyrics_get,
            lyrics::commands::lyrics_clear_cache,
            // Recommendation store commands
            reco_store::commands::reco_log_event,
            reco_store::commands::reco_get_home,
            reco_store::commands::reco_train_scores,
            reco_store::commands::reco_get_home_ml,
            // Session persistence commands
            session_store::save_session_state,
            session_store::load_session_state,
            session_store::save_session_volume,
            session_store::save_session_position,
            session_store::save_session_playback_mode,
            session_store::clear_session,
            // Audio settings commands
            config::audio_settings::get_audio_settings,
            config::audio_settings::set_audio_output_device,
            config::audio_settings::set_audio_exclusive_mode,
            config::audio_settings::set_audio_dac_passthrough,
            config::audio_settings::set_audio_sample_rate,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
