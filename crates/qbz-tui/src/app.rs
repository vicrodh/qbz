use std::io::{self, stdout};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::CrosstermBackend;
use ratatui::Frame;
use ratatui::Terminal;
use tokio::sync::mpsc;

use qbz_audio::{AudioDiagnostic, AudioSettings};
use qbz_core::QbzCore;
use qbz_models::CoreEvent;
use qbz_player::Player;

use crate::adapter::TuiAdapter;
use crate::ui::layout::render_layout;

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
    pub state: AppState,
    core_event_rx: mpsc::UnboundedReceiver<CoreEvent>,
    core: Arc<QbzCore<TuiAdapter>>,
    should_quit: bool,
    pub no_images: bool,
    rt_handle: tokio::runtime::Handle,
}

impl App {
    pub async fn new(no_images: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let (event_tx, event_rx) = mpsc::unbounded_channel::<CoreEvent>();
        let adapter = TuiAdapter::new(event_tx);

        // Use default audio settings for TUI (proper settings loading added later)
        let audio_settings = AudioSettings::default();
        let diagnostic = AudioDiagnostic::new();

        let player = Player::new(None, audio_settings, None, diagnostic);
        let core = QbzCore::new(adapter, player);

        // Initialize core (extracts Qobuz bundle tokens)
        if let Err(err) = core.init().await {
            log::warn!("Core init failed (offline mode): {}", err);
        }

        let core = Arc::new(core);
        let rt_handle = tokio::runtime::Handle::current();

        Ok(Self {
            state: AppState::default(),
            core_event_rx: event_rx,
            core,
            should_quit: false,
            no_images,
            rt_handle,
        })
    }

    /// Render the full UI for the current frame.
    pub fn draw(&self, frame: &mut Frame) {
        render_layout(frame, &self.state);
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Install panic hook to restore terminal on crash
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            let _ = execute!(io::stdout(), crossterm::cursor::Show);
            original_hook(panic_info);
        }));

        // Set up terminal
        enable_raw_mode()?;
        let mut stdout_handle = stdout();
        execute!(stdout_handle, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout_handle);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        // Main event loop
        while !self.should_quit {
            // Draw UI
            terminal.draw(|frame| self.draw(frame))?;

            // Poll crossterm events with 100ms timeout
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    // Only handle key press events (ignore release/repeat on some terminals)
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key);
                    }
                }
            }

            // Drain all pending core events
            while let Ok(core_event) = self.core_event_rx.try_recv() {
                self.handle_core_event(core_event);
            }
        }

        // Cleanup terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        execute!(terminal.backend_mut(), crossterm::cursor::Show)?;

        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            KeyCode::Tab => self.state.sidebar_expanded = !self.state.sidebar_expanded,
            KeyCode::Char('1') => self.state.active_view = ActiveView::Home,
            KeyCode::Char('2') => self.state.active_view = ActiveView::Favorites,
            KeyCode::Char('3') => self.state.active_view = ActiveView::Library,
            KeyCode::Char('4') => self.state.active_view = ActiveView::Playlists,
            KeyCode::Char('5') => self.state.active_view = ActiveView::Search,
            KeyCode::Char('6') => self.state.active_view = ActiveView::Settings,
            KeyCode::Char(' ') => {
                if self.state.is_playing {
                    let _ = self.core.pause();
                } else {
                    let _ = self.core.resume();
                }
            }
            KeyCode::Char('n') => {
                let core = Arc::clone(&self.core);
                self.rt_handle.spawn(async move {
                    let _ = core.next_track().await;
                });
            }
            KeyCode::Char('p') => {
                let core = Arc::clone(&self.core);
                self.rt_handle.spawn(async move {
                    let _ = core.previous_track().await;
                });
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                let new_vol = (self.state.volume + 0.05).min(1.0);
                if self.core.set_volume(new_vol).is_ok() {
                    self.state.volume = new_vol;
                }
            }
            KeyCode::Char('-') => {
                let new_vol = (self.state.volume - 0.05).max(0.0);
                if self.core.set_volume(new_vol).is_ok() {
                    self.state.volume = new_vol;
                }
            }
            _ => {}
        }
    }

    fn handle_core_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::TrackStarted { track, .. } => {
                self.state.is_playing = true;
                self.state.current_track_title = Some(track.title);
                self.state.current_track_artist = Some(track.artist);
                self.state.duration_secs = track.duration_secs;
                self.state.position_secs = 0;

                // Build quality string from track metadata
                let quality = match (track.bit_depth, track.sample_rate) {
                    (Some(bd), Some(sr)) => Some(format!("{}-bit / {:.1}kHz", bd, sr / 1000.0)),
                    (Some(bd), None) => Some(format!("{}-bit", bd)),
                    (None, Some(sr)) => Some(format!("{:.1}kHz", sr / 1000.0)),
                    (None, None) if track.hires => Some("Hi-Res".to_string()),
                    _ => None,
                };
                self.state.current_track_quality = quality;
            }
            CoreEvent::PlaybackStateChanged { state } => {
                self.state.is_playing = state == qbz_models::PlaybackState::Playing;
            }
            CoreEvent::PositionUpdated {
                position_secs,
                duration_secs,
            } => {
                self.state.position_secs = position_secs;
                self.state.duration_secs = duration_secs;
            }
            CoreEvent::VolumeChanged { volume } => {
                self.state.volume = volume;
            }
            CoreEvent::TrackEnded { .. } => {
                // Queue auto-advance is handled by core; we just reflect state
                self.state.is_playing = false;
                self.state.position_secs = 0;
            }
            CoreEvent::PlaybackStatusUpdated { status } => {
                self.state.is_playing = status.state == qbz_models::PlaybackState::Playing;
                self.state.position_secs = status.position_secs;
                self.state.duration_secs = status.duration_secs;
                self.state.volume = status.volume;
            }
            CoreEvent::PlaybackError { message, .. } => {
                log::error!("Playback error: {}", message);
            }
            CoreEvent::Error { message, .. } => {
                log::error!("Core error: {}", message);
            }
            // Queue, auth, library, loading, audio device, search, navigation events
            // are not yet reflected in the TUI state (handled in later tasks)
            _ => {}
        }
    }
}
