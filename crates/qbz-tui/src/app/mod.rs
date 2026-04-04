pub struct App;

impl App {
    pub async fn new(_no_images: bool) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self)
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
