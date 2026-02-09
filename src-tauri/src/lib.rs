//! QBZ-NIX: Native high-fidelity streaming client for Linux
//!
//! This application uses the Qobuz API but is not certified by Qobuz.

pub mod api;
pub mod api_cache;
pub mod api_server;
pub mod artist_blacklist;
pub mod artist_vectors;
pub mod audio;
pub mod cache;
pub mod cast;
pub mod commands;
pub mod config;
pub mod credentials;
pub mod discogs;
pub mod offline_cache;
pub mod flatpak;
pub mod lastfm;
pub mod library;
pub mod listenbrainz;
pub mod logging;
pub mod lyrics;
pub mod media_controls;
pub mod migration;
pub mod musicbrainz;
pub mod network;
pub mod offline;
pub mod playback_context;
pub mod plex;
pub mod player;
pub mod playlist_import;
pub mod queue;
pub mod reco_store;
pub mod radio_engine;
pub mod session_store;
pub mod share;
pub mod tray;
pub mod updates;
pub mod user_data;
pub mod visualizer;

use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

use api::QobuzClient;
use cache::{AudioCache, PlaybackCache};
use lastfm::LastFmClient;
use media_controls::{MediaControlsManager, TrackInfo};
use playback_context::ContextManager;
use player::Player;
use queue::QueueManager;
use share::SongLinkClient;
use visualizer::Visualizer;

/// Application state shared across commands
pub struct AppState {
    pub client: Arc<Mutex<QobuzClient>>,
    pub player: Player,
    pub queue: QueueManager,
    pub context: ContextManager,
    pub media_controls: MediaControlsManager,
    pub audio_cache: Arc<AudioCache>,
    pub lastfm: Arc<Mutex<LastFmClient>>,
    pub songlink: SongLinkClient,
    pub visualizer: Visualizer,
}

impl AppState {
    pub fn new() -> Self {
        Self::with_device_and_settings(None, config::audio_settings::AudioSettings::default())
    }

    pub fn with_device(device_name: Option<String>) -> Self {
        Self::with_device_and_settings(device_name, config::audio_settings::AudioSettings::default())
    }

    pub fn with_device_and_settings(
        device_name: Option<String>,
        audio_settings: config::audio_settings::AudioSettings,
    ) -> Self {
        // Create playback cache (L2 - disk, 500MB)
        let playback_cache = match PlaybackCache::new(500 * 1024 * 1024) {
            Ok(cache) => Some(Arc::new(cache)),
            Err(e) => {
                log::warn!("Failed to create playback cache: {}. Disk spillover disabled.", e);
                None
            }
        };

        // Create audio cache (L1 - memory, 300MB) with optional disk spillover
        let audio_cache = if let Some(pc) = playback_cache {
            Arc::new(AudioCache::with_playback_cache(300 * 1024 * 1024, pc))
        } else {
            Arc::new(AudioCache::default())
        };

        // Create visualizer first to get the tap for the player
        let visualizer = Visualizer::new();
        let viz_tap = visualizer.get_tap();

        Self {
            client: Arc::new(Mutex::new(QobuzClient::default())),
            player: Player::new(device_name, audio_settings, Some(viz_tap), audio::AudioDiagnostic::new()),
            queue: QueueManager::new(),
            context: ContextManager::new(),
            media_controls: MediaControlsManager::new(),
            audio_cache,
            lastfm: Arc::new(Mutex::new(LastFmClient::default())),
            songlink: SongLinkClient::new(),
            visualizer,
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

    // Initialize logging with TeeWriter (captures to ring buffer + stderr)
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .target(env_logger::Target::Pipe(Box::new(logging::TeeWriter)))
        .init();

    log::info!("QBZ starting...");

    // Migrate data from old App ID if needed
    match flatpak::migrate_app_id_data() {
        Ok(true) => log::info!("App ID migration completed successfully"),
        Ok(false) => log::debug!("No App ID migration needed"),
        Err(e) => log::error!("App ID migration failed: {}", e),
    }

    // ── Phase 1: Device-level init (before login) ─────────────────────
    // Read audio settings from flat path once for player initialization.
    // The managed state starts empty and is populated after login.
    let (saved_device, audio_settings) = config::audio_settings::AudioSettingsStore::new()
        .ok()
        .and_then(|store| {
            store.get_settings().ok().map(|settings| {
                (settings.output_device.clone(), settings)
            })
        })
        .unwrap_or_else(|| {
            log::info!("No saved audio settings found, using defaults");
            (None, config::audio_settings::AudioSettings::default())
        });

    if let Some(ref device) = saved_device {
        log::info!("Initializing player with saved device: {}", device);
    }
    log::info!(
        "Audio settings: exclusive_mode={}, dac_passthrough={}, sample_rate={:?}",
        audio_settings.exclusive_mode,
        audio_settings.dac_passthrough,
        audio_settings.preferred_sample_rate
    );

    // Read tray settings from flat path once for tray initialization.
    let tray_settings = config::tray_settings::TraySettingsStore::new()
        .and_then(|store| store.get_settings())
        .unwrap_or_default();
    log::info!(
        "Tray settings: enable={}, minimize_to_tray={}, close_to_tray={}",
        tray_settings.enable_tray,
        tray_settings.minimize_to_tray,
        tray_settings.close_to_tray
    );

    // Initialize casting state (Chromecast, DLNA) — device-level, not per-user
    let cast_state = cast::CastState::new()
        .expect("Failed to initialize Chromecast state");
    let dlna_state = cast::dlna::commands::DlnaState::new(cast_state.media_server.clone())
        .expect("Failed to initialize DLNA state");

    // Initialize API server state for remote control (device-level)
    let api_server_state = api_server::ApiServerState::new();

    // Initialize remote metadata state (device-level, uses its own MusicBrainz instance)
    let remote_metadata_state = commands::RemoteMetadataSharedState {
        inner: Arc::new(Mutex::new(library::remote_metadata::RemoteMetadataState::new(
            Some(Arc::new(musicbrainz::MusicBrainzSharedState::new()
                .expect("Failed to initialize MusicBrainz for remote metadata")))
        ))),
    };

    // ── Phase 2: Per-user states (empty until activate_user_session) ──
    let library_state = library::init_library_state_empty();
    let offline_cache_state = offline_cache::OfflineCacheState::new_empty();
    let lyrics_state = lyrics::LyricsState::new_empty();
    let reco_state = reco_store::RecoState::new_empty();
    let api_cache_state = api_cache::ApiCacheState::new_empty();
    let session_store_state = session_store::SessionStoreState::new_empty();
    let audio_settings_state = config::audio_settings::AudioSettingsState::new_empty();
    let download_settings_state = config::download_settings::create_empty_download_settings_state();
    let offline_state = offline::OfflineState::new_empty();
    let playback_prefs_state = config::playback_preferences::PlaybackPreferencesState::new_empty();
    let favorites_prefs_state = config::favorites_preferences::FavoritesPreferencesState::new_empty();
    let favorites_cache_state = config::favorites_cache::FavoritesCacheState::new_empty();
    let tray_settings_state = config::tray_settings::TraySettingsState::new_empty();
    let remote_control_settings_state = config::remote_control_settings::RemoteControlSettingsState::new_empty();
    let allowed_origins_state = config::remote_control_settings::AllowedOriginsState::new_empty();
    let legal_settings_state = config::legal_settings::create_empty_legal_settings_state();
    let updates_state = updates::UpdatesState::new_empty()
        .expect("Failed to initialize empty updates state");
    let subscription_state = config::create_empty_subscription_state();
    let musicbrainz_state = musicbrainz::MusicBrainzSharedState::new_empty();
    let artist_vectors_state = artist_vectors::ArtistVectorStoreState::new_empty();
    let blacklist_state = artist_blacklist::BlacklistState::new_empty();
    let listenbrainz_state = listenbrainz::ListenBrainzSharedState::new_empty();
    let developer_settings_state = config::developer_settings::DeveloperSettingsState::new()
        .unwrap_or_else(|e| {
            log::warn!("Failed to initialize developer settings: {}. Using empty state.", e);
            config::developer_settings::DeveloperSettingsState::new_empty()
        });
    let graphics_settings_state = config::graphics_settings::GraphicsSettingsState::new()
        .unwrap_or_else(|e| {
            log::warn!("Failed to initialize graphics settings: {}. Using empty state.", e);
            config::graphics_settings::GraphicsSettingsState::new_empty()
        });

    // Clone settings for use in closures
    let enable_tray = tray_settings.enable_tray;
    let close_to_tray = tray_settings.close_to_tray;

    // Initialize per-user data paths (no user active yet until login)
    let user_data_paths = user_data::UserDataPaths::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::with_device_and_settings(saved_device, audio_settings))
        .manage(user_data_paths)
        .setup(move |app| {
            // Initialize system tray icon (only if enabled)
            if enable_tray {
                if let Err(e) = tray::init_tray(app.handle()) {
                    log::error!("Failed to initialize tray icon: {}", e);
                }
            } else {
                log::info!("System tray icon disabled by user setting");
            }

            // Initialize media controls (MPRIS) now that we have an AppHandle
            app.state::<AppState>()
                .media_controls
                .init(app.handle().clone());

            // NOTE: Visualizer FFT thread and Remote Control API server are started
            // in activate_user_session (post-login), not here. They need per-user
            // state to be initialized first.

            // NOTE: Subscription purge check moved to activate_user_session
            // (runs after login when per-user state is available)

            // Start background task to emit playback events
            let app_handle = app.handle().clone();
            let player_state = app.state::<AppState>().player.state.clone();

            std::thread::spawn(move || {
                let mut last_position: u64 = 0;
                let mut last_is_playing: bool = false;
                let mut last_track_id: u64 = 0;

                loop {
                    // Check playing/track state first to determine sleep duration
                    let is_playing = player_state.is_playing();
                    let track_id = player_state.current_track_id();

                    // Adaptive polling:
                    // - fast (250ms) when playing
                    // - slow (1000ms) when paused/stopped with a track loaded
                    // - very slow (5000ms) when no track is loaded (idle)
                    let sleep_duration = if is_playing {
                        std::time::Duration::from_millis(250)
                    } else if track_id == 0 {
                        std::time::Duration::from_millis(5000)
                    } else {
                        std::time::Duration::from_millis(1000)
                    };
                    std::thread::sleep(sleep_duration);

                    // Re-check after sleep (state might have changed)
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

                    let should_update_mpris = should_emit || (track_id == 0 && last_track_id != 0);

                    if should_emit {
                        let sample_rate = player_state.get_sample_rate();
                        let bit_depth = player_state.get_bit_depth();
                        // Get queue state for shuffle/repeat
                        let queue_state = &app_handle.state::<AppState>().queue;
                        let shuffle = queue_state.is_shuffle();
                        let repeat = match queue_state.get_repeat() {
                            queue::RepeatMode::Off => "off",
                            queue::RepeatMode::All => "all",
                            queue::RepeatMode::One => "one",
                        };
                        let normalization_gain = player_state.get_normalization_gain();
                        let event = player::PlaybackEvent {
                            is_playing,
                            position,
                            duration,
                            track_id,
                            volume,
                            sample_rate: if sample_rate > 0 { Some(sample_rate) } else { None },
                            bit_depth: if bit_depth > 0 { Some(bit_depth) } else { None },
                            shuffle: Some(shuffle),
                            repeat: Some(repeat.to_string()),
                            normalization_gain,
                            gapless_ready: player_state.is_gapless_ready(),
                            gapless_next_track_id: player_state.get_gapless_next_track_id(),
                        };
                        let _ = app_handle.emit("playback:state", &event);
                        api_server::broadcast_playback_event(&app_handle, &event);
                        last_position = position;
                        last_is_playing = is_playing;
                        last_track_id = track_id;
                    }

                    if should_update_mpris {
                        let media_controls = &app_handle.state::<AppState>().media_controls;
                        if track_id == 0 {
                            media_controls.set_stopped();
                        } else {
                            media_controls.set_playback_with_progress(is_playing, position);
                        }
                    }
                }
            });

            Ok(())
        })
        .on_window_event(move |window, event| {
            // Handle close to tray
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if close_to_tray {
                    log::info!("Close to tray: hiding window instead of closing");
                    let _ = window.hide();
                    api.prevent_close();
                } else {
                    // Cleanup cast devices on actual close
                    log::info!("App closing: cleaning up cast devices");
                    
                    // Disconnect Chromecast if connected (sends message through channel)
                    if let Some(cast_state) = window.app_handle().try_state::<cast::CastState>() {
                        log::info!("Disconnecting Chromecast on app exit");
                        let _ = cast_state.chromecast.disconnect();
                    }
                    
                    // Note: DLNA connection will be dropped when the app exits,
                    // which will naturally close the connection. The tokio Mutex
                    // prevents us from synchronously stopping playback here.
                    log::info!("DLNA connection will be cleaned up on drop");
                }
            }
        })
        .manage(library_state)
        .manage(cast_state)
        .manage(dlna_state)
        // .manage(airplay_state)  // AirPlay DISABLED
        .manage(offline_cache_state)
        .manage(lyrics_state)
        .manage(reco_state)
        .manage(api_cache_state)
        .manage(session_store_state)
        .manage(audio_settings_state)
        .manage(download_settings_state)
        .manage(subscription_state)
        .manage(offline_state)
        .manage(playback_prefs_state)
        .manage(favorites_prefs_state)
        .manage(favorites_cache_state)
        .manage(tray_settings_state)
        .manage(remote_control_settings_state)
        .manage(allowed_origins_state)
        .manage(api_server_state)
        .manage(legal_settings_state)
        .manage(updates_state)
        .manage(musicbrainz_state)
        .manage(listenbrainz_state)
        .manage(remote_metadata_state)
        .manage(artist_vectors_state)
        .manage(blacklist_state)
        .manage(developer_settings_state)
        .manage(graphics_settings_state)
        .invoke_handler(tauri::generate_handler![
            // Auth commands
            commands::init_client,
            commands::login,
            commands::logout,
            commands::is_logged_in,
            commands::get_user_info,
            commands::set_api_locale,
            // Credential persistence commands
            commands::has_saved_credentials,
            commands::save_credentials,
            commands::clear_saved_credentials,
            commands::auto_login,
            // User session lifecycle
            commands::get_last_user_id,
            commands::activate_user_session,
            commands::deactivate_user_session,
            commands::factory_reset,
            // Search commands
            commands::search_albums,
            commands::search_tracks,
            commands::search_artists,
            commands::search_all,
            commands::get_album,
            commands::get_featured_albums,
            commands::get_genres,
            commands::get_discover_index,
            commands::get_discover_playlists,
            commands::get_track,
            commands::get_artist,
            commands::get_artist_basic,
            commands::get_artist_detail,
            commands::get_artist_tracks,
            commands::get_artist_albums,
            commands::get_similar_artists,
            commands::get_label,
            // Playback commands
            commands::play_track,
            commands::prefetch_track,
            commands::pause_playback,
            commands::resume_playback,
            commands::stop_playback,
            commands::play_next_gapless,
            commands::set_volume,
            commands::seek,
            commands::get_playback_state,
            commands::set_media_metadata,
            commands::get_audio_devices,
            commands::get_audio_output_status,
            commands::get_pipewire_sinks,
            commands::debug_get_cpal_devices,
            commands::set_pipewire_default_sink,
            commands::reinit_audio_device,
            commands::get_hardware_audio_status,
            commands::start_bitdepth_capture,
            commands::stop_bitdepth_capture,
            // Queue commands
            commands::add_to_queue,
            commands::add_to_queue_next,
            commands::add_tracks_to_queue,
            commands::set_queue,
            commands::clear_queue,
            commands::remove_from_queue,
            commands::move_queue_track,
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
            // Radio engine commands
            commands::create_artist_radio,
            commands::create_track_radio,
            commands::refill_radio_queue,
            commands::get_queue_remaining,
            commands::create_infinite_radio,
            // Playback context commands
            commands::get_playback_context,
            commands::set_playback_context,
            commands::clear_playback_context,
            commands::has_playback_context,
            // Playlist commands
            commands::get_user_playlists,
            commands::get_playlist,
            commands::search_playlists,
            commands::create_playlist,
            commands::delete_playlist,
            commands::add_tracks_to_playlist,
            commands::remove_tracks_from_playlist,
            commands::update_playlist,
            commands::get_tracks_by_ids,
            commands::get_current_user_id,
            commands::subscribe_playlist,
            commands::get_track_info,
            commands::get_album_credits,
            // Playlist import commands
            commands::playlist_import_preview,
            commands::playlist_import_execute,
            // Playlist suggestions commands (v2 vector-based)
            commands::get_playlist_suggestions_v2,
            commands::get_vector_store_stats,
            commands::cleanup_vector_store,
            commands::clear_vector_store,
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
            commands::clear_artist_cache,
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
            library::commands::library_cleanup_missing_files,
            library::commands::library_get_cache_stats,
            library::commands::library_clear_artwork_cache,
            library::commands::library_clear_thumbnails_cache,
            library::commands::library_get_folders,
            library::commands::library_get_folders_with_metadata,
            library::commands::library_get_folder,
            library::commands::library_update_folder_settings,
            library::commands::library_set_folder_enabled,
            library::commands::library_update_folder_path,
            library::commands::library_check_folder_accessible,
            library::commands::library_scan,
            library::commands::library_scan_folder,
            library::commands::library_get_scan_progress,
            library::commands::library_stop_scan,
            library::commands::library_get_albums,
            library::commands::library_get_album_tracks,
            library::commands::library_get_artists,
            library::commands::library_search,
            library::commands::library_get_stats,
            library::commands::library_clear,
            library::commands::library_get_track,
            library::commands::get_track_by_path,
            library::commands::library_play_track,
            library::commands::library_get_tracks_by_ids,
            // Playlist local settings commands
            library::commands::playlist_get_settings,
            library::commands::playlist_save_settings,
            library::commands::playlist_set_sort,
            library::commands::playlist_set_artwork,
            library::commands::playlist_add_local_track,
            library::commands::playlist_remove_local_track,
            library::commands::playlist_get_local_tracks,
            library::commands::playlist_get_local_tracks_with_position,
            library::commands::playlist_get_all_local_track_counts,
            library::commands::playlist_clear_local_tracks,
            // Playlist management commands
            library::commands::playlist_get_all_settings,
            library::commands::playlist_set_hidden,
            library::commands::playlist_set_favorite,
            library::commands::playlist_get_favorites,
            library::commands::playlist_set_position,
            library::commands::playlist_reorder,
            library::commands::playlist_get_stats,
            library::commands::playlist_get_all_stats,
            library::commands::playlist_increment_play_count,
            // Playlist custom order commands
            library::commands::playlist_get_custom_order,
            library::commands::playlist_init_custom_order,
            library::commands::playlist_set_custom_order,
            library::commands::playlist_move_track,
            library::commands::playlist_has_custom_order,
            library::commands::playlist_clear_custom_order,
            // Playlist folders commands
            library::commands::create_playlist_folder,
            library::commands::get_playlist_folders,
            library::commands::update_playlist_folder,
            library::commands::delete_playlist_folder,
            library::commands::reorder_playlist_folders,
            library::commands::move_playlist_to_folder,
            // Discogs artwork commands
            library::commands::discogs_has_credentials,
            library::commands::discogs_search_artist,
            library::commands::discogs_search_artwork,
            library::commands::discogs_download_artwork,
            // Thumbnail commands
            library::commands::library_get_thumbnail,
            library::commands::library_clear_thumbnails,
            library::commands::library_get_thumbnails_cache_size,
            // Artwork commands
            library::commands::library_fetch_missing_artwork,
            library::commands::library_fetch_album_artwork,
            library::commands::library_set_album_artwork,
            // Album settings commands
            library::commands::library_get_album_settings,
            library::commands::library_set_album_hidden,
            library::commands::library_update_album_metadata,
            library::commands::library_write_album_metadata_to_files,
            library::commands::library_refresh_album_metadata_from_files,
            library::commands::library_get_hidden_albums,
            library::commands::library_backfill_downloads,
            // Artist images commands
            library::commands::library_get_artist_image,
            library::commands::library_get_artist_images,
            library::commands::library_cache_artist_image,
            library::commands::library_set_custom_artist_image,
            library::commands::library_get_canonical_names,
            // Playlist local content analysis commands (offline mode)
            library::commands::playlist_analyze_local_content,
            library::commands::playlist_get_local_content_status,
            library::commands::playlist_track_is_local,
            library::commands::playlist_get_local_track_id,
            library::commands::playlist_get_tracks_with_local_copies,
            library::commands::playlist_get_offline_available,
            // Chromecast casting commands
            cast::commands::cast_start_discovery,
            cast::commands::cast_stop_discovery,
            cast::commands::cast_get_devices,
            cast::commands::cast_connect,
            cast::commands::cast_disconnect,
            cast::commands::cast_get_status,
            cast::commands::cast_get_position,
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
            cast::dlna::commands::dlna_get_position,
            cast::dlna::commands::dlna_play_track,
            cast::dlna::commands::dlna_load_media,
            cast::dlna::commands::dlna_play,
            cast::dlna::commands::dlna_pause,
            cast::dlna::commands::dlna_stop,
            cast::dlna::commands::dlna_seek,
            cast::dlna::commands::dlna_set_volume,
            // Plex LAN-only POC commands
            plex::plex_ping,
            plex::plex_get_music_sections,
            plex::plex_get_section_tracks,
            plex::plex_get_track_metadata,
            plex::plex_play_track,
            plex::plex_auth_pin_start,
            plex::plex_auth_pin_check,
            plex::plex_open_auth_url,
            plex::plex_cache_get_sections,
            plex::plex_cache_save_sections,
            plex::plex_cache_get_tracks,
            plex::plex_cache_save_tracks,
            plex::plex_cache_update_track_quality,
            plex::plex_cache_get_albums,
            plex::plex_cache_get_album_tracks,
            plex::plex_cache_search_tracks,
            plex::plex_cache_clear,
            // AirPlay casting commands - DISABLED until RAOP implementation is complete
            // See docs/AIRPLAY_IMPLEMENTATION_STATUS.md for details
            // cast::airplay::commands::airplay_start_discovery,
            // cast::airplay::commands::airplay_stop_discovery,
            // cast::airplay::commands::airplay_get_devices,
            // cast::airplay::commands::airplay_connect,
            // cast::airplay::commands::airplay_disconnect,
            // cast::airplay::commands::airplay_get_status,
            // cast::airplay::commands::airplay_load_media,
            // cast::airplay::commands::airplay_play,
            // cast::airplay::commands::airplay_pause,
            // cast::airplay::commands::airplay_stop,
            // cast::airplay::commands::airplay_set_volume,
            // Offline cache commands
            offline_cache::commands::cache_track_for_offline,
            offline_cache::commands::is_track_cached,
            offline_cache::commands::get_cached_track_path,
            offline_cache::commands::get_cached_track,
            offline_cache::commands::get_cached_tracks,
            offline_cache::commands::get_offline_cache_stats,
            offline_cache::commands::remove_cached_track,
            offline_cache::commands::clear_offline_cache,
            offline_cache::commands::set_offline_cache_limit,
            offline_cache::commands::open_offline_cache_folder,
            offline_cache::commands::open_album_folder,
            offline_cache::commands::open_track_folder,
            offline_cache::commands::check_album_fully_cached,
            offline_cache::commands::check_offline_root_mounted,
            offline_cache::commands::validate_offline_path,
            offline_cache::commands::move_offline_cache_to_path,
            offline_cache::commands::detect_legacy_cached_files,
            offline_cache::commands::start_legacy_migration,
            offline_cache::commands::sync_offline_cache_to_library,
            // Lyrics commands
            lyrics::commands::lyrics_get,
            lyrics::commands::lyrics_get_cache_stats,
            lyrics::commands::lyrics_clear_cache,
            // Recommendation store commands
            reco_store::commands::reco_log_event,
            reco_store::commands::reco_get_home,
            reco_store::commands::reco_train_scores,
            reco_store::commands::reco_get_home_ml,
            reco_store::commands::get_playlist_suggestions,
            reco_store::commands::reco_backfill_genres,
            reco_store::commands::reco_needs_genre_backfill,
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
            config::audio_settings::set_audio_backend_type,
            config::audio_settings::set_audio_alsa_plugin,
            config::audio_settings::set_audio_alsa_hardware_volume,
            config::audio_settings::set_audio_stream_first_track,
            config::audio_settings::set_audio_stream_buffer_seconds,
            config::audio_settings::set_audio_streaming_only,
            config::audio_settings::set_audio_limit_quality_to_device,
            config::audio_settings::set_audio_device_max_sample_rate,
            config::audio_settings::set_audio_normalization_enabled,
            config::audio_settings::set_audio_normalization_target,
            config::audio_settings::set_audio_gapless_enabled,
            config::audio_settings::reset_audio_settings,
            // Audio backend commands
            commands::get_available_backends,
            commands::get_devices_for_backend,
            commands::get_alsa_plugins,
            commands::check_alsa_utils_installed,
            commands::get_linux_distro,
            commands::query_dac_capabilities,
            // Download settings commands
            config::download_settings::get_download_settings,
            config::download_settings::set_download_root,
            config::download_settings::set_show_downloads_in_library,
            config::download_settings::validate_download_root,
            // Offline mode commands
            offline::commands::get_offline_status,
            offline::commands::get_offline_settings,
            offline::commands::set_manual_offline,
            offline::commands::set_show_partial_playlists,
            offline::commands::set_allow_cast_while_offline,
            // Playback preferences commands
            config::playback_preferences::get_playback_preferences,
            config::playback_preferences::set_autoplay_mode,
            config::playback_preferences::set_show_context_icon,
            config::favorites_preferences::get_favorites_preferences,
            config::favorites_preferences::save_favorites_preferences,
            // Favorites cache commands (local persistence)
            config::favorites_cache::get_cached_favorite_tracks,
            config::favorites_cache::get_cached_favorite_albums,
            config::favorites_cache::get_cached_favorite_artists,
            config::favorites_cache::cache_favorite_track,
            config::favorites_cache::uncache_favorite_track,
            config::favorites_cache::cache_favorite_album,
            config::favorites_cache::uncache_favorite_album,
            config::favorites_cache::cache_favorite_artist,
            config::favorites_cache::uncache_favorite_artist,
            config::favorites_cache::sync_cached_favorite_tracks,
            config::favorites_cache::sync_cached_favorite_albums,
            config::favorites_cache::sync_cached_favorite_artists,
            config::favorites_cache::clear_favorites_cache,
            // Updates commands
            updates::get_update_preferences,
            updates::set_update_check_on_launch,
            updates::set_show_whats_new_on_launch,
            updates::acknowledge_release,
            updates::ignore_release,
            updates::is_release_acknowledged,
            updates::is_release_ignored,
            updates::has_whats_new_been_shown,
            updates::mark_whats_new_shown,
            updates::has_flatpak_welcome_been_shown,
            updates::mark_flatpak_welcome_shown,
            updates::get_current_version,
            updates::check_for_updates,
            updates::fetch_release_for_version,
            offline::commands::set_allow_immediate_scrobbling,
            offline::commands::set_allow_accumulated_scrobbling,
            offline::commands::set_show_network_folders_in_manual_offline,
            offline::commands::check_network,
            // Offline playlist sync queue commands
            offline::commands::create_pending_playlist,
            offline::commands::get_pending_playlists,
            offline::commands::add_tracks_to_pending_playlist,
            offline::commands::get_pending_playlist_count,
            offline::commands::update_pending_playlist_qobuz_id,
            offline::commands::mark_pending_playlist_synced,
            offline::commands::delete_pending_playlist,
            // Scrobble queue commands (for offline Last.fm scrobbling)
            offline::commands::queue_scrobble,
            offline::commands::get_queued_scrobbles,
            offline::commands::mark_scrobbles_sent,
            offline::commands::get_queued_scrobble_count,
            offline::commands::cleanup_sent_scrobbles,
            // Network folder detection commands
            network::commands::check_network_path,
            network::commands::get_network_mounts_cmd,
            network::commands::check_mount_accessible,
            network::commands::check_network_paths_batch,
            // Flatpak detection commands
            flatpak::is_running_in_flatpak,
            flatpak::get_flatpak_help_text,
            // Tray settings commands
            config::tray_settings::get_tray_settings,
            config::tray_settings::set_enable_tray,
            config::tray_settings::set_minimize_to_tray,
            config::tray_settings::set_close_to_tray,
            // Remote control commands
            api_server::remote_control_get_status,
            api_server::remote_control_set_enabled,
            api_server::remote_control_set_port,
            api_server::remote_control_set_secure,
            api_server::remote_control_get_pairing_qr,
            api_server::remote_control_regenerate_token,
            api_server::remote_control_get_allowed_origins,
            api_server::remote_control_add_allowed_origin,
            api_server::remote_control_remove_allowed_origin,
            api_server::remote_control_restore_default_origins,
            // Legal settings commands
            config::legal_settings::get_legal_settings,
            config::legal_settings::get_qobuz_tos_accepted,
            config::legal_settings::set_qobuz_tos_accepted,
            // MusicBrainz integration commands
            commands::musicbrainz_resolve_track,
            commands::musicbrainz_resolve_artist,
            commands::musicbrainz_resolve_release,
            commands::musicbrainz_get_artist_relationships,
            commands::musicbrainz_is_enabled,
            commands::musicbrainz_set_enabled,
            commands::musicbrainz_get_cache_stats,
            commands::musicbrainz_clear_cache,
            commands::musicbrainz_cleanup_cache,
            // Musician resolution commands
            commands::resolve_musician,
            commands::get_musician_appearances,
            // ListenBrainz integration commands
            commands::listenbrainz_get_status,
            commands::listenbrainz_is_enabled,
            commands::listenbrainz_set_enabled,
            commands::listenbrainz_connect,
            commands::listenbrainz_disconnect,
            commands::listenbrainz_now_playing,
            commands::listenbrainz_scrobble,
            commands::listenbrainz_queue_listen,
            commands::listenbrainz_get_queue,
            commands::listenbrainz_get_queue_count,
            commands::listenbrainz_mark_sent,
            commands::listenbrainz_flush_queue,
            commands::listenbrainz_clear_queue,
            commands::listenbrainz_cleanup_queue,
            // Smart playlist generation commands
            commands::smart_playlist_preview,
            commands::smart_playlist_resolve_artist,
            commands::smart_playlist_get_available_types,
            // Remote metadata commands (Tag Editor service integration)
            commands::remote_metadata_search,
            commands::remote_metadata_get_album,
            commands::remote_metadata_cache_stats,
            commands::remote_metadata_clear_cache,
            // Visualizer commands
            commands::set_visualizer_enabled,
            commands::is_visualizer_enabled,
            // Artist blacklist commands
            commands::get_artist_blacklist,
            commands::add_to_artist_blacklist,
            commands::remove_from_artist_blacklist,
            commands::is_artist_blacklisted,
            commands::set_blacklist_enabled,
            commands::is_blacklist_enabled,
            commands::get_blacklist_settings,
            commands::get_blacklist_count,
            commands::clear_artist_blacklist,
            // Developer settings commands
            config::developer_settings::get_developer_settings,
            config::developer_settings::set_developer_force_dmabuf,
            // Graphics settings commands
            config::graphics_settings::get_graphics_settings,
            config::graphics_settings::set_hardware_acceleration,
            // Log capture commands
            logging::get_backend_logs,
            logging::upload_logs_to_paste,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
