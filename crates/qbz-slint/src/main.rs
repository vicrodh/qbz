//! QBZ Slint POC - Main entry point

mod adapter;

use adapter::SlintAdapter;
use qbz_core::{Album, AudioPlayer, QbzCore, Track};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::sync::{mpsc, Arc};

slint::include_modules!();

/// Commands sent to the audio thread
enum AudioCommand {
    Play(Vec<u8>),
    Pause,
    Resume,
    Toggle,
    Stop,
    SetVolume(f32),
}

/// Start the audio thread and return a sender for commands
fn start_audio_thread() -> Option<mpsc::Sender<AudioCommand>> {
    let (tx, rx) = mpsc::channel::<AudioCommand>();

    std::thread::spawn(move || {
        let player = match AudioPlayer::new() {
            Ok(p) => {
                log::info!("Audio thread: player initialized");
                p
            }
            Err(e) => {
                log::error!("Audio thread: failed to create player: {}", e);
                return;
            }
        };

        loop {
            match rx.recv() {
                Ok(cmd) => match cmd {
                    AudioCommand::Play(data) => {
                        if let Err(e) = player.play_bytes_sync(data) {
                            log::error!("Audio thread: play failed: {}", e);
                        }
                    }
                    AudioCommand::Pause => player.pause_sync(),
                    AudioCommand::Resume => player.resume_sync(),
                    AudioCommand::Toggle => player.toggle_sync(),
                    AudioCommand::Stop => player.stop_sync(),
                    AudioCommand::SetVolume(v) => player.set_volume_sync(v),
                },
                Err(_) => {
                    log::info!("Audio thread: channel closed, exiting");
                    break;
                }
            }
        }
    });

    Some(tx)
}

#[tokio::main]
async fn main() {
    // Initialize logging
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).init();

    log::info!("QBZ Slint POC starting...");

    // Create Slint app
    let app = App::new().expect("Failed to create Slint app");

    // Create adapter that bridges Slint <-> Core
    let adapter = SlintAdapter::new(&app);

    // Create core with our adapter
    let core = Arc::new(QbzCore::new(adapter));

    // Start audio thread
    let audio_tx = start_audio_thread();

    // Initialize Qobuz API client in background
    let core_init = core.clone();
    tokio::spawn(async move {
        if let Err(e) = core_init.init().await {
            log::error!("Failed to initialize Qobuz client: {}", e);
        }
    });

    // Setup UI callbacks
    setup_callbacks(&app, core.clone(), audio_tx);

    log::info!("Running Slint event loop...");

    // Run the Slint event loop (blocks until window closes)
    app.run().expect("Failed to run Slint app");

    log::info!("QBZ Slint POC exiting.");
}

/// Convert core Album to Slint AlbumData
fn album_to_slint(album: &Album) -> AlbumData {
    AlbumData {
        id: SharedString::from(album.id.clone()),
        title: SharedString::from(album.title.clone()),
        artist: SharedString::from(album.artist.clone()),
        cover_url: SharedString::from(album.cover_url.clone().unwrap_or_default()),
        hires: album.hires_available,
    }
}

/// Convert core Track to Slint TrackData
fn track_to_slint(track: &Track) -> TrackData {
    TrackData {
        id: SharedString::from(track.id.to_string()),
        title: SharedString::from(track.title.clone()),
        artist: SharedString::from(track.artist.clone()),
        album: SharedString::from(track.album.clone()),
        duration: SharedString::from(format_duration(track.duration)),
        hires: track.hires_available,
    }
}

/// Format duration in milliseconds to MM:SS
fn format_duration(ms: u64) -> String {
    let secs = ms / 1000;
    let mins = secs / 60;
    let secs = secs % 60;
    format!("{}:{:02}", mins, secs)
}

/// Convert Track to QueueItem with index
fn track_to_queue_item(track: &Track, index: i32) -> QueueItem {
    QueueItem {
        index,
        id: SharedString::from(track.id.to_string()),
        title: SharedString::from(track.title.clone()),
        artist: SharedString::from(track.artist.clone()),
        album: SharedString::from(track.album.clone()),
        duration: SharedString::from(format_duration(track.duration)),
        hires: track.hires_available,
    }
}

/// Load home data (featured albums) and update UI
async fn load_home_data(core: Arc<QbzCore<SlintAdapter>>, app_weak: slint::Weak<App>) {
    log::info!("Loading home data...");

    // Set loading state
    let _ = slint::invoke_from_event_loop({
        let app_weak = app_weak.clone();
        move || {
            if let Some(app) = app_weak.upgrade() {
                app.set_is_loading(true);
            }
        }
    });

    // Fetch new releases and editor picks in parallel
    let (new_releases, editor_picks) = tokio::join!(
        core.get_featured_albums(10),
        core.get_editor_picks(10)
    );

    // Update UI with results
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_is_loading(false);

            // New releases
            match new_releases {
                Ok(albums) => {
                    log::info!("Loaded {} new releases", albums.len());
                    let items: Vec<AlbumData> = albums.iter().map(album_to_slint).collect();
                    let model = ModelRc::new(VecModel::from(items));
                    app.set_new_releases(model);
                }
                Err(e) => {
                    log::error!("Failed to load new releases: {}", e);
                }
            }

            // Editor picks
            match editor_picks {
                Ok(albums) => {
                    log::info!("Loaded {} editor picks", albums.len());
                    let items: Vec<AlbumData> = albums.iter().map(album_to_slint).collect();
                    let model = ModelRc::new(VecModel::from(items));
                    app.set_editor_picks(model);
                }
                Err(e) => {
                    log::error!("Failed to load editor picks: {}", e);
                }
            }
        }
    });
}

fn setup_callbacks(app: &App, core: Arc<QbzCore<SlintAdapter>>, audio_tx: Option<mpsc::Sender<AudioCommand>>) {
    // Login callback
    let core_login = core.clone();
    let app_weak = app.as_weak();
    app.on_login(move |email, password| {
        let core = core_login.clone();
        let app_weak = app_weak.clone();
        let email = email.to_string();
        let password = password.to_string();

        // Set loading state
        if let Some(app) = app_weak.upgrade() {
            app.set_is_loading(true);
            app.set_login_error("".into());
        }

        tokio::spawn(async move {
            match core.login(&email, &password).await {
                Ok(user) => {
                    log::info!("Login successful: {}", user.display_name);

                    // Update user name in UI
                    let user_name = user.display_name.clone();
                    let _ = slint::invoke_from_event_loop({
                        let app_weak = app_weak.clone();
                        move || {
                            if let Some(app) = app_weak.upgrade() {
                                app.set_user_name(user_name.into());
                            }
                        }
                    });

                    // Load home data after successful login
                    load_home_data(core, app_weak).await;
                }
                Err(e) => {
                    log::error!("Login failed: {}", e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = app_weak.upgrade() {
                            app.set_is_loading(false);
                            app.set_login_error(e.to_string().into());
                        }
                    });
                }
            }
        });
    });

    // Play/Pause callback - toggles playing state
    let audio_pp = audio_tx.clone();
    let app_weak = app.as_weak();
    app.on_play_pause(move || {
        if let Some(tx) = &audio_pp {
            let _ = tx.send(AudioCommand::Toggle);
        }
        // Toggle UI state (we don't get feedback from audio thread for simplicity)
        if let Some(app) = app_weak.upgrade() {
            let is_playing = app.get_is_playing();
            app.set_is_playing(!is_playing);
        }
    });

    // Next track callback
    let app_weak = app.as_weak();
    app.on_next(move || {
        log::info!("Next track");
        if let Some(app) = app_weak.upgrade() {
            let idx = app.get_queue_index();
            app.set_queue_index(idx + 1);
            // In real implementation, would call core.next()
        }
    });

    // Previous track callback
    let app_weak = app.as_weak();
    app.on_previous(move || {
        log::info!("Previous track");
        if let Some(app) = app_weak.upgrade() {
            let idx = app.get_queue_index();
            if idx > 0 {
                app.set_queue_index(idx - 1);
            }
            // In real implementation, would call core.previous()
        }
    });

    // Seek callback - updates position
    let app_weak = app.as_weak();
    app.on_seek(move |position| {
        log::info!("Seek to {:.2}", position);
        if let Some(app) = app_weak.upgrade() {
            app.set_position(position);
            // Update position text (mock: assume 3 min track)
            let secs = (position * 180.0) as u64;
            let mins = secs / 60;
            let secs = secs % 60;
            app.set_position_text(format!("{}:{:02}", mins, secs).into());
            // In real implementation, would call core.seek(position)
        }
    });

    // Volume callback - updates volume
    let audio_vol = audio_tx.clone();
    let app_weak = app.as_weak();
    app.on_set_volume(move |volume| {
        if let Some(tx) = &audio_vol {
            let _ = tx.send(AudioCommand::SetVolume(volume));
        }
        if let Some(app) = app_weak.upgrade() {
            app.set_volume(volume);
        }
    });

    // Load home data callback
    let core_home = core.clone();
    let app_weak = app.as_weak();
    app.on_load_home_data(move || {
        let core = core_home.clone();
        let app_weak = app_weak.clone();
        tokio::spawn(async move {
            load_home_data(core, app_weak).await;
        });
    });

    // Play album callback - fetches real tracks and plays
    let core_album = core.clone();
    let audio_album = audio_tx.clone();
    let app_weak = app.as_weak();
    app.on_play_album(move |album_id| {
        let album_id = album_id.to_string();
        let core = core_album.clone();
        let audio_tx = audio_album.clone();
        let app_weak = app_weak.clone();

        tokio::spawn(async move {
            log::info!("Fetching album tracks: {}", album_id);

            match core.get_album_tracks(&album_id).await {
                Ok((album, tracks)) => {
                    log::info!("Got {} tracks from album '{}'", tracks.len(), album.title);

                    if tracks.is_empty() {
                        log::warn!("Album has no tracks");
                        return;
                    }

                    // Store tracks for queue reference
                    let tracks_clone = tracks.clone();
                    let first_track = tracks[0].clone();
                    let album_title = album.title.clone();
                    let track_duration = first_track.duration;

                    // Update UI with queue
                    let _ = slint::invoke_from_event_loop({
                        let app_weak = app_weak.clone();
                        let first_track = first_track.clone();
                        move || {
                            if let Some(app) = app_weak.upgrade() {
                                // Populate queue
                                let queue_items: Vec<QueueItem> = tracks_clone
                                    .iter()
                                    .enumerate()
                                    .map(|(i, t)| track_to_queue_item(t, i as i32))
                                    .collect();

                                let model = ModelRc::new(VecModel::from(queue_items));
                                app.set_queue_tracks(model);
                                app.set_queue_index(0);

                                // Set current track info
                                app.set_current_title(first_track.title.clone().into());
                                app.set_current_artist(first_track.artist.clone().into());
                                app.set_current_album(album_title.into());
                                app.set_is_playing(true);
                                app.set_duration_text(format_duration(track_duration).into());
                                app.set_position(0.0);
                                app.set_position_text("0:00".into());
                            }
                        }
                    });

                    // Download and play the first track
                    let first_track_id = first_track.id;
                    match core.get_track_audio(first_track_id).await {
                        Ok(audio_data) => {
                            // Send to audio thread
                            if let Some(tx) = audio_tx {
                                if let Err(e) = tx.send(AudioCommand::Play(audio_data)) {
                                    log::error!("Failed to send play command: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to download track: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to fetch album: {}", e);
                }
            }
        });
    });

    // Play track callback
    app.on_play_track(move |track_id| {
        log::info!("Play track {} (not implemented yet)", track_id);
        // TODO: Add track to queue and start playback
    });

    // Queue: play specific index
    app.on_queue_play_index(move |index| {
        log::info!("Queue play index {} (not implemented yet)", index);
        // TODO: Jump to index in queue
    });

    // Queue: remove track at index
    app.on_queue_remove(move |index| {
        log::info!("Queue remove index {} (not implemented yet - would go through core)", index);
        // In real implementation, this would call core.queue_remove(index)
        // and core would emit QueueChanged event to update UI
    });

    // Queue: clear all
    app.on_queue_clear(move || {
        log::info!("Queue clear (not implemented yet - would go through core)");
        // In real implementation, this would call core.queue_clear()
        // and core would emit QueueChanged event to update UI
    });

    // Search callback
    let core_search = core.clone();
    let app_weak = app.as_weak();
    app.on_search(move |query| {
        let query = query.to_string();
        if query.is_empty() {
            return;
        }

        let core = core_search.clone();
        let app_weak = app_weak.clone();

        // Set loading state
        if let Some(app) = app_weak.upgrade() {
            app.set_is_loading(true);
        }

        tokio::spawn(async move {
            log::info!("Searching for: {}", query);

            // Search albums and tracks in parallel
            let (albums_result, tracks_result) = tokio::join!(
                core.search_albums(&query, 20),
                core.search_tracks(&query, 30)
            );

            // Update UI
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = app_weak.upgrade() {
                    app.set_is_loading(false);

                    // Albums
                    match albums_result {
                        Ok(albums) => {
                            log::info!("Found {} albums", albums.len());
                            let items: Vec<AlbumData> = albums.iter().map(album_to_slint).collect();
                            let model = ModelRc::new(VecModel::from(items));
                            app.set_search_albums(model);
                        }
                        Err(e) => {
                            log::error!("Album search failed: {}", e);
                            app.set_search_albums(ModelRc::new(VecModel::default()));
                        }
                    }

                    // Tracks
                    match tracks_result {
                        Ok(tracks) => {
                            log::info!("Found {} tracks", tracks.len());
                            let items: Vec<TrackData> = tracks.iter().map(track_to_slint).collect();
                            let model = ModelRc::new(VecModel::from(items));
                            app.set_search_tracks(model);
                        }
                        Err(e) => {
                            log::error!("Track search failed: {}", e);
                            app.set_search_tracks(ModelRc::new(VecModel::default()));
                        }
                    }
                }
            });
        });
    });
}
