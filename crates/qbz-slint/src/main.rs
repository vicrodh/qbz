//! QBZ Slint POC - Main entry point

mod adapter;

use adapter::SlintAdapter;
use qbz_core::QbzCore;
use slint::ComponentHandle;
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
                    // UI update is handled by adapter via CoreEvent::LoginSuccess
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
}
