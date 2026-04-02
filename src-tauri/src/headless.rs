// src-tauri/src/headless.rs

/// Run QBZ in headless mode (no UI, optional web server).
pub fn run(enable_web: bool) {
    dotenvy::dotenv().ok();

    eprintln!("[QBZ] Starting in headless mode...");

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        if enable_web {
            eprintln!("[QBZ] Web server would start here (not yet implemented)");
        }

        eprintln!("[QBZ] Headless mode running. Press Ctrl+C to stop.");

        // Wait for shutdown signal
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");

        eprintln!("[QBZ] Shutting down...");
    });
}
