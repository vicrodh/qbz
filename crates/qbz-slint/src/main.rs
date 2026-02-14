//! QBZ Slint POC - Main entry point
//!
//! This is a proof-of-concept to evaluate Slint as an alternative
//! to the current Tauri + Svelte stack.

mod adapter;

use adapter::SlintAdapter;
use qbz_core::QbzCore;
use std::sync::Arc;

// Include Slint generated code ONCE here, then re-export
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

    // Setup UI callbacks
    setup_callbacks(&app, core.clone());

    log::info!("Running Slint event loop...");

    // Run the Slint event loop (blocks until window closes)
    app.run().expect("Failed to run Slint app");

    log::info!("QBZ Slint POC exiting.");
}

fn setup_callbacks(_app: &App, _core: Arc<QbzCore<SlintAdapter>>) {
    // Will be implemented in Phase 2 (Login)
    // Example:
    // app.on_login(move |email, password| {
    //     let core = core.clone();
    //     tokio::spawn(async move {
    //         core.login(&email, &password).await;
    //     });
    // });
}
