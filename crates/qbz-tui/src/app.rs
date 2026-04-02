pub struct App {
    pub no_images: bool,
}

impl App {
    pub async fn new(no_images: bool) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { no_images })
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("[QBZ-TUI] Scaffold - not yet implemented");
        Ok(())
    }
}
