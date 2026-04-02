pub mod app;
pub mod adapter;
pub mod credentials;
pub mod input;
pub mod theme;
pub mod ui;

/// Entry point for TUI mode.
pub fn run(no_images: bool) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let mut app = app::App::new(no_images).await?;
        app.run().await
    })
}
