//! QBZ Slint POC - Main entry point

mod adapter;

use adapter::SlintAdapter;
use qbz_core::{Album, QbzCore, Track};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::sync::Arc;

slint::include_modules!();

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

    // Initialize Qobuz API client in background
    let core_init = core.clone();
    tokio::spawn(async move {
        if let Err(e) = core_init.init().await {
            log::error!("Failed to initialize Qobuz client: {}", e);
        }
    });

    // Setup UI callbacks
    setup_callbacks(&app, core.clone());

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

fn setup_callbacks(app: &App, core: Arc<QbzCore<SlintAdapter>>) {
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

    // Play/Pause callback (placeholder for now)
    app.on_play_pause(move || {
        log::info!("Play/Pause clicked (not implemented yet)");
    });

    // Next track callback
    app.on_next(move || {
        log::info!("Next clicked (not implemented yet)");
    });

    // Previous track callback
    app.on_previous(move || {
        log::info!("Previous clicked (not implemented yet)");
    });

    // Seek callback
    app.on_seek(move |position| {
        log::info!("Seek to {} (not implemented yet)", position);
    });

    // Volume callback
    app.on_set_volume(move |volume| {
        log::info!("Set volume to {} (not implemented yet)", volume);
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

    // Play album callback
    app.on_play_album(move |album_id| {
        log::info!("Play album {} (not implemented yet)", album_id);
        // TODO: Fetch album tracks and start playback
    });

    // Play track callback
    app.on_play_track(move |track_id| {
        log::info!("Play track {} (not implemented yet)", track_id);
        // TODO: Add track to queue and start playback
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
