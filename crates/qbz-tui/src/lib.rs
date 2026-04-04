pub mod app;
pub mod adapter;
pub mod credentials;
pub mod input;
pub mod playback;
pub mod qconnect;
pub mod theme;
pub mod ui;

/// Entry point for TUI mode.
pub fn run(no_images: bool) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize file-based logger (TUI owns the terminal, so stderr is unusable)
    let log_path = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("qbz")
        .join("tui.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let log_file = std::fs::File::create(&log_path)?;
    env_logger::Builder::new()
        .target(env_logger::Target::Pipe(Box::new(log_file)))
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_millis()
        .init();
    log::info!("[TUI] Log initialized at {}", log_path.display());

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let mut app = app::App::new(no_images).await?;
        app.run().await
    })
}
