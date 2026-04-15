use std::sync::Arc;
use tokio::sync::broadcast;

use qbz_audio::{AudioDiagnostic, AudioSettings};
use qbz_audio::settings::AudioSettingsStore;
use qbz_cache::AudioCache;
use qbz_core::QbzCore;
use qbz_player::Player;

use tokio::sync::RwLock;

use crate::adapter::{DaemonAdapter, DaemonEvent};
use crate::config::DaemonConfig;
use crate::session::UserSession;

/// Central state container for the headless daemon.
/// Replaces Tauri's app.state::<T>() with direct field access.
pub struct DaemonCore {
    pub config: DaemonConfig,
    pub core: Arc<QbzCore<DaemonAdapter>>,
    pub audio_cache: Arc<AudioCache>,
    pub event_bus: broadcast::Sender<DaemonEvent>,
    /// Per-user state, populated after login + session activation
    pub user: RwLock<Option<UserSession>>,
    /// Flag to prevent orchestrator auto-advance during explicit play commands
    pub skip_auto_advance: std::sync::atomic::AtomicBool,
    /// QConnect next track id (from SetState.next_track) for auto-advance
    /// when iOS controls without sending a queue. 0 = no next track known.
    pub qconnect_next_track_id: Arc<std::sync::atomic::AtomicU64>,
}

/// Run the daemon main loop.
pub async fn run(mut config: DaemonConfig) -> Result<(), String> {
    log::info!("[qbzd] Access: LAN-only (no authentication required on local network)");

    // Create event bus (bounded, slow SSE clients get dropped)
    let (event_tx, _) = broadcast::channel::<DaemonEvent>(256);

    // Create adapter for QbzCore events
    let adapter = DaemonAdapter::new(event_tx.clone());

    // Load audio settings (from shared database if exists)
    let audio_settings = AudioSettingsStore::new()
        .ok()
        .and_then(|store| store.get_settings().ok())
        .unwrap_or_else(|| {
            log::info!("[qbzd] No saved audio settings, using defaults");
            AudioSettings::default()
        });

    let device_name = audio_settings.output_device.clone();
    log::info!(
        "[qbzd] Audio: backend={:?}, device={:?}, exclusive={}, gapless={}",
        audio_settings.backend_type,
        device_name,
        audio_settings.exclusive_mode,
        config.audio.gapless,
    );

    // Create player (audio thread starts immediately)
    let diagnostic = AudioDiagnostic::new();
    let player = Player::new(device_name, audio_settings, None, diagnostic);

    // Create QbzCore — this extracts Qobuz bundle tokens (requires network)
    let core = QbzCore::new(adapter, player);
    log::info!("[qbzd] Initializing QbzCore (extracting Qobuz bundle tokens)...");
    core.init().await.map_err(|e| format!("QbzCore init failed: {}", e))?;
    log::info!("[qbzd] QbzCore initialized");

    // Create audio cache with configured sizes
    let cache_bytes = config.memory_cache_bytes();
    let audio_cache = Arc::new(AudioCache::new(cache_bytes));
    log::info!("[qbzd] Audio cache: {} MB", config.cache.memory_mb);

    let core = Arc::new(core);

    let daemon = Arc::new(DaemonCore {
        config: config.clone(),
        core: core.clone(),
        audio_cache,
        event_bus: event_tx.clone(),
        user: RwLock::new(None),
        skip_auto_advance: std::sync::atomic::AtomicBool::new(false),
        qconnect_next_track_id: Arc::new(std::sync::atomic::AtomicU64::new(0)),
    });

    // Try auto-login from saved OAuth token
    match try_auto_login(&core).await {
        Some(user_id) => {
            // Demoted from info to debug to keep user_id out of default-level
            // logs (CodeQL rust/cleartext-logging). Set RUST_LOG=debug to see.
            log::debug!("[qbzd] Auto-login successful (user_id: {})", user_id);
            // Activate per-user session (initialize stores, sync settings)
            match crate::session::activate_session(user_id, &core, &event_tx).await {
                Ok(session) => {
                    *daemon.user.write().await = Some(session);
                    log::info!("[qbzd] User session activated");
                }
                Err(e) => {
                    log::error!("[qbzd] Session activation failed: {}", e);
                }
            }

            // Start QConnect if enabled
            if config.qconnect.enabled {
                let device_name = if config.qconnect.device_name.is_empty() {
                    hostname::get()
                        .ok()
                        .and_then(|h| h.into_string().ok())
                        .unwrap_or_else(|| "qbzd".to_string())
                } else {
                    config.qconnect.device_name.clone()
                };
                let _qconnect = crate::qconnect::start_qconnect(
                    &core,
                    event_tx.clone(),
                    &device_name,
                    daemon.qconnect_next_track_id.clone(),
                )
                .await;
            }
        }
        None => {
            log::warn!("[qbzd] No saved credentials. Run `qbzd login` to authenticate.");
        }
    }

    // Start MPRIS media controls (Linux D-Bus, headless)
    let mpris_handle = if config.mpris.enabled {
        crate::mpris::start_mpris(daemon.core.clone())
    } else {
        log::info!("[qbzd] MPRIS disabled via config");
        None
    };

    // Start playback state polling loop (broadcasts to event bus + MPRIS)
    spawn_playback_loop(daemon.core.clone(), daemon.event_bus.clone(), mpris_handle.clone());

    // Playback orchestrator: auto-advance, gapless pre-queue, repeat modes
    spawn_playback_orchestrator(daemon.clone());

    // Spawn MPRIS metadata updater (listens to event bus for TrackStarted)
    if let Some(ref mc) = mpris_handle {
        spawn_mpris_metadata_updater(daemon.event_bus.subscribe(), mc.clone());
    }

    // Register mDNS service for LAN discovery
    let _mdns_handle = if config.mdns.enabled {
        match register_mdns(&config) {
            Ok(handle) => {
                log::info!("[qbzd] mDNS registered: _qbz._tcp on port {}", config.server.port);
                Some(handle)
            }
            Err(e) => {
                log::warn!("[qbzd] mDNS registration failed: {}", e);
                None
            }
        }
    } else {
        None
    };

    log::info!(
        "[qbzd] Daemon ready on {}:{}",
        config.server.bind,
        config.server.port
    );

    // Start HTTP server
    let bind_addr = format!("{}:{}", config.server.bind, config.server.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| format!("Failed to bind {}: {}", bind_addr, e))?;

    log::info!("[qbzd] HTTP server listening on {}", bind_addr);

    // Axum router
    let app = build_router(daemon.clone());

    // Graceful shutdown on SIGTERM/SIGINT
    let shutdown = async {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to register SIGTERM handler");

        #[cfg(unix)]
        tokio::select! {
            _ = ctrl_c => log::info!("[qbzd] Received SIGINT, shutting down..."),
            _ = sigterm.recv() => log::info!("[qbzd] Received SIGTERM, shutting down..."),
        }

        #[cfg(not(unix))]
        ctrl_c.await.ok();
    };

    let app = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| format!("HTTP server error: {}", e))?;

    // Graceful shutdown: stop player
    log::info!("[qbzd] Stopping player...");
    let _ = daemon.core.stop();

    log::info!("[qbzd] Shutdown complete");
    Ok(())
}

/// Try to restore session from saved OAuth token in keyring.
/// Returns the user_id on success.
async fn try_auto_login(core: &QbzCore<DaemonAdapter>) -> Option<u64> {
    let token = load_oauth_token()?;

    match core.login_with_token(&token).await {
        Ok(session) => {
            // Demoted from info to debug to keep user_id out of default-level
            // logs (CodeQL rust/cleartext-logging). Set RUST_LOG=debug to see.
            log::debug!(
                "[qbzd] Session restored for user {} ({})",
                session.display_name,
                session.user_id
            );
            Some(session.user_id)
        }
        Err(e) => {
            log::warn!("[qbzd] Saved token expired or invalid: {}", e);
            None
        }
    }
}

/// Load OAuth token — tries keyring first, then file fallback.
fn load_oauth_token() -> Option<String> {
    const SERVICE: &str = "qbz-player";
    const KEY: &str = "qobuz-oauth-token";

    // Try keyring first
    if let Ok(entry) = keyring::Entry::new(SERVICE, KEY) {
        match entry.get_password() {
            Ok(token) if !token.is_empty() => {
                log::info!("[qbzd] OAuth token loaded from keyring");
                return Some(token);
            }
            Ok(_) => {}
            Err(keyring::Error::NoEntry) => {
                log::debug!("[qbzd] No OAuth token in keyring");
            }
            Err(e) => {
                log::warn!("[qbzd] Keyring access failed: {}", e);
            }
        }
    }

    // Fallback: try file
    if let Some(token) = crate::login::load_token_from_file() {
        log::info!("[qbzd] OAuth token loaded from file fallback");
        return Some(token);
    }

    None
}

/// Playback orchestrator: handles auto-advance, gapless, and repeat.
/// This replaces the frontend's setOnTrackEnded + setGaplessGetNextTrackId
/// callbacks since the daemon has no frontend.
fn spawn_playback_orchestrator(daemon: Arc<DaemonCore>) {
    tokio::spawn(async move {
        let mut last_track_id: u64 = 0;
        let mut was_playing = false;
        let mut gapless_queued_for: u64 = 0; // Track ID we've already queued gapless for

        loop {
            let player = daemon.core.player();
            let state = &player.state;
            let track_id = state.current_track_id();
            let is_playing = state.is_playing();
            let gapless_ready = state.is_gapless_ready();

            // === GAPLESS PRE-QUEUE ===
            // When player signals gapless_ready (5s before track end),
            // download next track and queue it for seamless transition.
            if gapless_ready && track_id != 0 && gapless_queued_for != track_id {
                gapless_queued_for = track_id;

                let queue_state = daemon.core.get_queue_state().await;
                let repeat_mode = queue_state.repeat;

                // Determine next track ID (same logic as desktop's setGaplessGetNextTrackId)
                let next_id = if repeat_mode == qbz_models::RepeatMode::One {
                    Some(track_id) // Repeat same track
                } else {
                    // Peek at upcoming queue
                    let upcoming = daemon.core.peek_upcoming(2).await;
                    upcoming.first()
                        .filter(|t| t.id != track_id)
                        .or_else(|| upcoming.get(1).filter(|t| t.id != track_id))
                        .map(|t| t.id)
                };

                if let Some(next_id) = next_id {
                    log::info!("[qbzd/gapless] Pre-queuing track {} for gapless", next_id);
                    if let Ok(audio_data) = download_track(&daemon, next_id).await {
                        match player.play_next(audio_data, next_id) {
                            Ok(()) => {
                                log::info!("[qbzd/gapless] Track {} queued for seamless transition", next_id);
                            }
                            Err(e) => {
                                log::warn!("[qbzd/gapless] play_next failed: {}", e);
                            }
                        }
                    }
                }
            }

            // === AUTO-ADVANCE ===
            // When track ends (was playing, now track_id is 0), advance queue.
            // This handles the case when gapless wasn't available or failed.
            let track_ended = was_playing && !is_playing && track_id == 0 && last_track_id != 0;

            if track_ended && !daemon.skip_auto_advance.load(std::sync::atomic::Ordering::Acquire) {
                let queue_state = daemon.core.get_queue_state().await;
                let repeat_mode = queue_state.repeat;

                // Repeat-one: replay the same track
                if repeat_mode == qbz_models::RepeatMode::One {
                    log::info!("[qbzd/advance] Repeat-one: replaying track {}", last_track_id);
                    if let Ok(audio_data) = download_track(&daemon, last_track_id).await {
                        let _ = player.play_data(audio_data, last_track_id);
                    }
                } else {
                    // Normal advance: try local queue first, then QConnect next_track fallback
                    let previous_id = last_track_id;
                    let local_next = daemon.core.next_track().await
                        .filter(|t| t.id != previous_id);

                    if let Some(next_track) = local_next {
                        log::info!("[qbzd/advance] Next (local queue): {} - {}", next_track.artist, next_track.title);
                        if let Ok(audio_data) = download_track(&daemon, next_track.id).await {
                            let _ = player.play_data(audio_data, next_track.id);
                        }
                    } else {
                        // Fallback: use next_track from last QConnect SetState
                        let qconnect_next = daemon.qconnect_next_track_id.load(std::sync::atomic::Ordering::Acquire);
                        if qconnect_next != 0 && qconnect_next != previous_id {
                            log::info!("[qbzd/advance] Next (QConnect fallback): {}", qconnect_next);
                            // Clear before downloading to prevent re-use on next iteration
                            daemon.qconnect_next_track_id.store(0, std::sync::atomic::Ordering::Release);
                            if let Ok(audio_data) = download_track(&daemon, qconnect_next).await {
                                let _ = player.play_data(audio_data, qconnect_next);
                            }
                        } else {
                            log::info!("[qbzd/advance] Queue empty, playback stopped");
                        }
                    }
                }
                gapless_queued_for = 0;
            }

            // Reset gapless tracking on track change
            if track_id != last_track_id {
                gapless_queued_for = 0;
            }

            last_track_id = track_id;
            was_playing = is_playing;

            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    });
}

/// Download a track's audio from Qobuz. Returns the raw audio bytes.
pub async fn download_track(daemon: &DaemonCore, track_id: u64) -> Result<Vec<u8>, String> {
    let quality = qbz_models::Quality::HiRes; // TODO: use configured quality
    let stream_url = daemon.core.get_stream_url(track_id, quality).await
        .map_err(|e| format!("Stream URL failed for {}: {}", track_id, e))?;

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let data = http.get(&stream_url.url).send().await
        .map_err(|e| format!("Download failed for {}: {}", track_id, e))?
        .bytes().await
        .map_err(|e| format!("Read failed for {}: {}", track_id, e))?
        .to_vec();

    log::info!("[qbzd/download] Track {} downloaded ({} bytes, {:.0}kHz/{}bit)",
        track_id, data.len(), stream_url.sampling_rate, stream_url.bit_depth.unwrap_or(0));

    Ok(data)
}

/// Register the daemon as a `_qbz._tcp` mDNS service for LAN discovery.
fn register_mdns(config: &DaemonConfig) -> Result<mdns_sd::ServiceDaemon, String> {
    let mdns = mdns_sd::ServiceDaemon::new()
        .map_err(|e| format!("mDNS daemon error: {}", e))?;

    let service_name = if config.mdns.name.is_empty() {
        hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "qbzd".to_string())
    } else {
        config.mdns.name.clone()
    };

    let service_info = mdns_sd::ServiceInfo::new(
        "_qbz._tcp.local.",
        &service_name,
        &format!("{}.local.", hostname::get().ok().and_then(|h| h.into_string().ok()).unwrap_or_else(|| "localhost".to_string())),
        "",
        config.server.port,
        None,
    )
    .map_err(|e| format!("mDNS service info error: {}", e))?;

    mdns.register(service_info)
        .map_err(|e| format!("mDNS register error: {}", e))?;

    Ok(mdns)
}

/// Listen for CoreEvent::TrackStarted on the event bus and update MPRIS metadata.
fn spawn_mpris_metadata_updater(
    mut rx: broadcast::Receiver<DaemonEvent>,
    mc: Arc<std::sync::Mutex<souvlaki::MediaControls>>,
) {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(DaemonEvent::Core(qbz_models::CoreEvent::TrackStarted { track, .. })) => {
                    crate::mpris::update_mpris_metadata(
                        &mc,
                        &track.title,
                        &track.artist,
                        &track.album,
                        track.duration_secs,
                    );
                    log::debug!("[qbzd/mpris] Metadata: {} - {}", track.artist, track.title);
                }
                Ok(DaemonEvent::Core(qbz_models::CoreEvent::LoggedOut)) => {
                    // Clear metadata on logout
                    if let Ok(mut controls) = mc.lock() {
                        let _ = controls.set_playback(souvlaki::MediaPlayback::Stopped);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    log::debug!("[qbzd/mpris] Lagged {} events", n);
                }
                Err(broadcast::error::RecvError::Closed) => break,
                _ => {}
            }
        }
    });
}

/// Spawn the playback state polling loop.
/// Reads player state and broadcasts PlaybackSnapshot events.
/// Also updates MPRIS metadata when track changes.
/// Adaptive polling: 250ms playing, 1s paused, 5s idle.
fn spawn_playback_loop(
    core: Arc<QbzCore<DaemonAdapter>>,
    event_tx: broadcast::Sender<DaemonEvent>,
    mpris: Option<Arc<std::sync::Mutex<souvlaki::MediaControls>>>,
) {
    tokio::spawn(async move {
        let mut last_position: u64 = 0;
        let mut last_is_playing = false;
        let mut last_track_id: u64 = 0;

        loop {
            let player = core.player();
            let state = &player.state;

            let is_playing = state.is_playing();
            let track_id = state.current_track_id();
            let position = state.current_position();
            let duration = state.duration();
            let volume = state.volume();
            let sample_rate = state.get_sample_rate();
            let bit_depth = state.get_bit_depth();

            let track_cleared = track_id == 0 && last_track_id != 0;
            let should_emit = (track_id != 0
                && (is_playing != last_is_playing
                    || track_id != last_track_id
                    || (is_playing && position != last_position)))
                || track_cleared;

            if should_emit {
                let snapshot = crate::adapter::PlaybackSnapshot {
                    state: if is_playing {
                        "Playing".to_string()
                    } else if track_id != 0 {
                        "Paused".to_string()
                    } else {
                        "Stopped".to_string()
                    },
                    track_id,
                    position_secs: position,
                    duration_secs: duration,
                    volume,
                    sample_rate,
                    bit_depth,
                };
                let _ = event_tx.send(DaemonEvent::Playback(snapshot));

                // Update MPRIS
                if let Some(ref mc) = mpris {
                    if track_id != last_track_id && track_id != 0 {
                        // Track changed — update metadata
                        // We don't have track title here (just ID), so use a
                        // placeholder. Full metadata comes from CoreEvent::TrackStarted.
                        crate::mpris::update_mpris_playback(mc, is_playing, position);
                    } else {
                        crate::mpris::update_mpris_playback(mc, is_playing, position);
                    }
                    if track_id == 0 && last_track_id != 0 {
                        crate::mpris::update_mpris_playback(mc, false, 0);
                    }
                }

                last_position = position;
                last_is_playing = is_playing;
                last_track_id = track_id;
            }

            // Adaptive polling
            let sleep_ms = if is_playing {
                250
            } else if track_id == 0 {
                5000
            } else {
                1000
            };
            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
        }
    });
}

/// Macro to reduce boilerplate for route handlers that take Arc<DaemonCore>
macro_rules! with_daemon {
    ($daemon:expr, $handler:path) => {{
        let d = $daemon.clone();
        move || $handler(d.clone())
    }};
    ($daemon:expr, $handler:path, json) => {{
        let d = $daemon.clone();
        move |body| $handler(d.clone(), body)
    }};
    ($daemon:expr, $handler:path, query) => {{
        let d = $daemon.clone();
        move |q| $handler(d.clone(), q)
    }};
    ($daemon:expr, $handler:path, path) => {{
        let d = $daemon.clone();
        move |p| $handler(d.clone(), p)
    }};
    ($daemon:expr, $handler:path, path_query) => {{
        let d = $daemon.clone();
        move |p, q| $handler(d.clone(), p, q)
    }};
    ($daemon:expr, $handler:path, path_json) => {{
        let d = $daemon.clone();
        move |p, j| $handler(d.clone(), p, j)
    }};
}

/// Build the Axum HTTP router.
fn build_router(daemon: Arc<DaemonCore>) -> axum::Router {
    use axum::routing::{get, post, patch, put, delete};
    use axum::middleware as axum_mw;
    use crate::api::{audio, catalog, catalog_ext, discover, favorites, integrations, library, middleware, playback, playlists, queue, search, system};

    axum::Router::new()
        // System
        .route("/api/ping", get(ping_handler))
        .route("/api/info", get(with_daemon!(daemon, info_handler)))
        .route("/api/status", get(with_daemon!(daemon, status_handler)))
        .route("/api/events", get(with_daemon!(daemon, crate::api::events::sse_handler)))
        // Playback
        .route("/api/playback", get(with_daemon!(daemon, playback::get_playback)))
        .route("/api/playback/play", post(with_daemon!(daemon, playback::play)))
        .route("/api/playback/play-track", post(with_daemon!(daemon, playback::play_track, json)))
        .route("/api/playback/play-album", post(with_daemon!(daemon, playback::play_album, json)))
        .route("/api/playback/pause", post(with_daemon!(daemon, playback::pause)))
        .route("/api/playback/stop", post(with_daemon!(daemon, playback::stop)))
        .route("/api/playback/next", post(with_daemon!(daemon, playback::next)))
        .route("/api/playback/previous", post(with_daemon!(daemon, playback::previous)))
        .route("/api/playback/seek", post(with_daemon!(daemon, playback::seek, json)))
        .route("/api/playback/volume", post(with_daemon!(daemon, playback::volume, json)))
        // Queue
        .route("/api/queue", get(with_daemon!(daemon, queue::get_queue)))
        .route("/api/queue/set", post(with_daemon!(daemon, queue::set_queue, json)))
        .route("/api/queue/add", post(with_daemon!(daemon, queue::add, json)))
        .route("/api/queue/add-next", post(with_daemon!(daemon, queue::add_next, json)))
        .route("/api/queue/play-index", post(with_daemon!(daemon, queue::play_index, json)))
        .route("/api/queue/remove", post(with_daemon!(daemon, queue::remove, json)))
        .route("/api/queue/move", post(with_daemon!(daemon, queue::move_track, json)))
        .route("/api/queue/clear", post(with_daemon!(daemon, queue::clear)))
        .route("/api/queue/shuffle", post(with_daemon!(daemon, queue::shuffle, json)))
        .route("/api/queue/repeat", post(with_daemon!(daemon, queue::repeat, json)))
        // Search
        .route("/api/search", get(with_daemon!(daemon, search::search, query)))
        // Catalog
        .route("/api/albums/{id}", get(with_daemon!(daemon, catalog::get_album, path)))
        .route("/api/artists/{id}", get(with_daemon!(daemon, catalog::get_artist, path)))
        .route("/api/tracks/{id}", get(with_daemon!(daemon, catalog::get_track, path)))
        .route("/api/tracks/batch", get(with_daemon!(daemon, catalog::get_tracks_batch, query)))
        // Audio settings
        .route("/api/audio/settings", get(with_daemon!(daemon, audio::get_settings)))
        .route("/api/audio/settings", patch(with_daemon!(daemon, audio::update_settings, json)))
        .route("/api/audio/backends", get(with_daemon!(daemon, audio::get_backends)))
        .route("/api/audio/devices", get(with_daemon!(daemon, audio::get_devices, query)))
        .route("/api/audio/hardware-status", get(with_daemon!(daemon, audio::get_hardware_status)))
        // Discover / Home
        .route("/api/discover", get(with_daemon!(daemon, discover::get_discover_index, query)))
        .route("/api/discover/playlists", get(with_daemon!(daemon, discover::get_discover_playlists, query)))
        .route("/api/discover/featured", get(with_daemon!(daemon, discover::get_featured, query)))
        .route("/api/genres", get(with_daemon!(daemon, discover::get_genres)))
        // Favorites
        .route("/api/favorites", get(with_daemon!(daemon, favorites::get_favorites, query)))
        .route("/api/favorites", post(with_daemon!(daemon, favorites::add_favorite, json)))
        .route("/api/favorites", delete(with_daemon!(daemon, favorites::remove_favorite, json)))
        // Playlists
        .route("/api/playlists", get(with_daemon!(daemon, playlists::get_playlists)))
        .route("/api/playlists", post(with_daemon!(daemon, playlists::create_playlist, json)))
        .route("/api/playlists/search", get(with_daemon!(daemon, playlists::search_playlists, query)))
        .route("/api/playlists/{id}", get(with_daemon!(daemon, playlists::get_playlist, path)))
        .route("/api/playlists/{id}", put(with_daemon!(daemon, playlists::update_playlist, path_json)))
        .route("/api/playlists/{id}", delete(with_daemon!(daemon, playlists::delete_playlist, path)))
        .route("/api/playlists/{id}/tracks", post(with_daemon!(daemon, playlists::add_tracks, path_json)))
        .route("/api/playlists/{id}/tracks", delete(with_daemon!(daemon, playlists::remove_tracks, path_json)))
        // Extended catalog
        .route("/api/artists/{id}/page", get(with_daemon!(daemon, catalog_ext::get_artist_page, path)))
        .route("/api/artists/{id}/similar", get(with_daemon!(daemon, catalog_ext::get_similar_artists, path_query)))
        .route("/api/labels/{id}", get(with_daemon!(daemon, catalog_ext::get_label, path_query)))
        .route("/api/labels/{id}/page", get(with_daemon!(daemon, catalog_ext::get_label_page, path)))
        .route("/api/labels/explore", get(with_daemon!(daemon, catalog_ext::get_label_explore, query)))
        .route("/api/playlist-tags", get(with_daemon!(daemon, catalog_ext::get_playlist_tags)))
        // Library (local files)
        .route("/api/library/albums", get(with_daemon!(daemon, library::get_albums)))
        .route("/api/library/artists", get(with_daemon!(daemon, library::get_artists)))
        .route("/api/library/albums/{key}/tracks", get(with_daemon!(daemon, library::get_album_tracks, path)))
        .route("/api/library/search", get(with_daemon!(daemon, library::search_library, query)))
        .route("/api/library/stats", get(with_daemon!(daemon, library::get_stats)))
        .route("/api/library/folders", get(with_daemon!(daemon, library::get_folders)))
        .route("/api/library/folders", post(with_daemon!(daemon, library::add_folder, json)))
        .route("/api/library/folders", delete(with_daemon!(daemon, library::remove_folder, json)))
        .route("/api/library/scan", post(with_daemon!(daemon, library::start_scan)))
        // Integrations
        .route("/api/integrations/listenbrainz", get(with_daemon!(daemon, integrations::get_listenbrainz_status)))
        .route("/api/integrations/listenbrainz/connect", post(with_daemon!(daemon, integrations::connect_listenbrainz, json)))
        .route("/api/integrations/listenbrainz", delete(with_daemon!(daemon, integrations::disconnect_listenbrainz)))
        .route("/api/integrations/lastfm", get(with_daemon!(daemon, integrations::get_lastfm_status)))
        // System / Resources
        .route("/api/system/resources", get(with_daemon!(daemon, system::get_resources)))
        .route("/api/cache", delete(with_daemon!(daemon, system::clear_cache)))
        // CORS: allow qbz-control PWA and any LAN origin
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any)
        )
        // LAN-only: reject requests from non-private IPs
        .layer(axum_mw::from_fn(middleware::lan_only))
}

async fn ping_handler() -> &'static str {
    "pong"
}

async fn info_handler(daemon: Arc<DaemonCore>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "name": "qbzd",
        "version": env!("CARGO_PKG_VERSION"),
        "cache": {
            "memory_mb": daemon.config.cache.memory_mb,
            "disk_mb": daemon.config.cache.disk_mb,
            "prefetch_count": daemon.config.cache.prefetch_count,
        },
    }))
}

async fn status_handler(daemon: Arc<DaemonCore>) -> axum::Json<serde_json::Value> {
    let logged_in = daemon.core.has_session().await;
    axum::Json(serde_json::json!({
        "state": if logged_in { "ready" } else { "no_session" },
        "logged_in": logged_in,
        "audio": {
            "cache_mb": daemon.config.cache.memory_mb,
        },
    }))
}
