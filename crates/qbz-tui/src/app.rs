use crate::ui::layout::render_layout;
use ratatui::Frame;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActiveView {
    Home,
    Favorites,
    Library,
    Playlists,
    Search,
    Settings,
}

impl ActiveView {
    /// Human-readable label for display in placeholder views.
    pub fn label(self) -> &'static str {
        match self {
            ActiveView::Home => "Home",
            ActiveView::Favorites => "Favorites",
            ActiveView::Library => "Library",
            ActiveView::Playlists => "Playlists",
            ActiveView::Search => "Search",
            ActiveView::Settings => "Settings",
        }
    }
}

pub struct AppState {
    pub active_view: ActiveView,
    pub sidebar_expanded: bool,
    pub is_playing: bool,
    pub current_track_title: Option<String>,
    pub current_track_artist: Option<String>,
    pub current_track_quality: Option<String>,
    pub position_secs: u64,
    pub duration_secs: u64,
    pub volume: f32,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            active_view: ActiveView::Home,
            sidebar_expanded: false,
            is_playing: false,
            current_track_title: None,
            current_track_artist: None,
            current_track_quality: None,
            position_secs: 0,
            duration_secs: 0,
            volume: 1.0,
        }
    }
}

pub struct App {
    pub no_images: bool,
    pub state: AppState,
}

impl App {
    pub async fn new(no_images: bool) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            no_images,
            state: AppState::default(),
        })
    }

    /// Render the full UI for the current frame.
    pub fn draw(&self, frame: &mut Frame) {
        render_layout(frame, &self.state);
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("[QBZ-TUI] Scaffold - not yet implemented");
        Ok(())
    }
}
