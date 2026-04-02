use std::io::{self, stdout};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
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
use qbz_models::{CoreEvent, Quality, Track};
use qbz_player::Player;

use crate::adapter::TuiAdapter;
use crate::credentials;
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

/// Whether the TUI is in normal mode (vim-like navigation) or text input mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    /// Normal navigation mode — keys trigger actions.
    Normal,
    /// Text input mode — characters go to the active text field.
    TextInput,
}

/// Which tab is active in the search results view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchTab {
    Tracks,
    Albums,
    Artists,
}

/// State for the search view.
pub struct SearchState {
    /// Current search query text.
    pub query: String,
    /// Cursor position within the query string.
    pub cursor: usize,
    /// Active results tab.
    pub tab: SearchTab,
    /// Track results from the last search.
    pub tracks: Vec<Track>,
    /// Currently selected index in the results list.
    pub selected_index: usize,
    /// Total results reported by the API.
    pub total_results: u32,
    /// Whether a search is currently in progress.
    pub loading: bool,
    /// Error message from the last search attempt.
    pub error: Option<String>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            cursor: 0,
            tab: SearchTab::Tracks,
            tracks: Vec::new(),
            selected_index: 0,
            total_results: 0,
            loading: false,
            error: None,
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
    pub input_mode: InputMode,
    pub authenticated: bool,
    pub auth_email: Option<String>,
    pub search: SearchState,
    /// Transient status message shown at the bottom.
    pub status_message: Option<String>,
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
            input_mode: InputMode::Normal,
            authenticated: false,
            auth_email: None,
            search: SearchState::default(),
            status_message: None,
        }
    }
}

/// Type alias for the search result payload.
type SearchResult = Result<qbz_models::SearchResultsPage<Track>, qbz_core::error::CoreError>;

pub struct App {
    pub state: AppState,
    core_event_rx: mpsc::UnboundedReceiver<CoreEvent>,
    core: Arc<QbzCore<TuiAdapter>>,
    should_quit: bool,
    pub no_images: bool,
    rt_handle: tokio::runtime::Handle,
    /// Sender for search results (cloned into async tasks).
    search_result_tx: mpsc::UnboundedSender<SearchResult>,
    /// Receiver for search results (drained each tick).
    search_result_rx: mpsc::UnboundedReceiver<SearchResult>,
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
        let mut core_initialized = false;
        if let Err(err) = core.init().await {
            log::warn!("[TUI] Core init failed (offline mode): {}", err);
        } else {
            core_initialized = true;
        }

        let mut state = AppState::default();

        // Authenticate using saved credentials
        if core_initialized {
            match credentials::load_qobuz_credentials() {
                Ok(Some(creds)) => {
                    log::info!("[TUI] Found saved credentials for {}", creds.email);
                    match core.login(&creds.email, &creds.password).await {
                        Ok(session) => {
                            log::info!(
                                "[TUI] Authenticated as {} (plan: {})",
                                session.email,
                                session.subscription_label
                            );
                            state.authenticated = true;
                            state.auth_email = Some(session.email);
                            state.status_message =
                                Some(format!("Logged in ({})", session.subscription_label));
                        }
                        Err(e) => {
                            log::warn!("[TUI] Authentication failed: {}", e);
                            state.status_message = Some(format!("Auth failed: {}", e));
                        }
                    }
                }
                Ok(None) => {
                    log::info!("[TUI] No saved credentials found");
                    state.status_message = Some("No saved credentials".to_string());
                }
                Err(e) => {
                    log::warn!("[TUI] Failed to load credentials: {}", e);
                    state.status_message = Some(format!("Credential error: {}", e));
                }
            }
        }

        let core = Arc::new(core);
        let rt_handle = tokio::runtime::Handle::current();

        let (search_tx, search_rx) = mpsc::unbounded_channel::<SearchResult>();

        Ok(Self {
            state,
            core_event_rx: event_rx,
            core,
            should_quit: false,
            no_images,
            rt_handle,
            search_result_tx: search_tx,
            search_result_rx: search_rx,
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

            // Drain search results
            while let Ok(result) = self.search_result_rx.try_recv() {
                self.handle_search_result(result);
            }
        }

        // Cleanup terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        execute!(terminal.backend_mut(), crossterm::cursor::Show)?;

        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match self.state.input_mode {
            InputMode::TextInput => self.handle_key_text_input(key),
            InputMode::Normal => self.handle_key_normal(key),
        }
    }

    /// Handle keys in normal (navigation) mode.
    fn handle_key_normal(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            KeyCode::Tab => self.state.sidebar_expanded = !self.state.sidebar_expanded,
            KeyCode::Char('1') => self.state.active_view = ActiveView::Home,
            KeyCode::Char('2') => self.state.active_view = ActiveView::Favorites,
            KeyCode::Char('3') => self.state.active_view = ActiveView::Library,
            KeyCode::Char('4') => self.state.active_view = ActiveView::Playlists,
            KeyCode::Char('5') => {
                self.state.active_view = ActiveView::Search;
            }
            KeyCode::Char('6') => self.state.active_view = ActiveView::Settings,

            // Search view: press 'i' or '/' to enter text input mode
            KeyCode::Char('i') | KeyCode::Char('/') if self.state.active_view == ActiveView::Search => {
                self.state.input_mode = InputMode::TextInput;
            }

            // Search view: j/k for navigating results
            KeyCode::Char('j') | KeyCode::Down if self.state.active_view == ActiveView::Search => {
                let len = self.state.search.tracks.len();
                if len > 0 {
                    self.state.search.selected_index =
                        (self.state.search.selected_index + 1).min(len - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up if self.state.active_view == ActiveView::Search => {
                if self.state.search.selected_index > 0 {
                    self.state.search.selected_index -= 1;
                }
            }

            // Search view: Enter to play selected track
            KeyCode::Enter if self.state.active_view == ActiveView::Search => {
                self.play_selected_track();
            }

            // Playback controls
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

    /// Handle keys in text input mode (search query editing).
    fn handle_key_text_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.state.input_mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                // Execute search, then return to normal mode for result navigation
                self.execute_search();
                self.state.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                let search = &mut self.state.search;
                if search.cursor > 0 {
                    search.query.remove(search.cursor - 1);
                    search.cursor -= 1;
                }
            }
            KeyCode::Delete => {
                let search = &mut self.state.search;
                if search.cursor < search.query.len() {
                    search.query.remove(search.cursor);
                }
            }
            KeyCode::Left => {
                if self.state.search.cursor > 0 {
                    self.state.search.cursor -= 1;
                }
            }
            KeyCode::Right => {
                let len = self.state.search.query.len();
                if self.state.search.cursor < len {
                    self.state.search.cursor += 1;
                }
            }
            KeyCode::Home => {
                self.state.search.cursor = 0;
            }
            KeyCode::End => {
                self.state.search.cursor = self.state.search.query.len();
            }
            // Ctrl+U: clear the input
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.search.query.clear();
                self.state.search.cursor = 0;
            }
            KeyCode::Char(c) => {
                let search = &mut self.state.search;
                search.query.insert(search.cursor, c);
                search.cursor += 1;
            }
            _ => {}
        }
    }

    /// Launch an async search request against the Qobuz API.
    fn execute_search(&mut self) {
        let query = self.state.search.query.trim().to_string();
        if query.is_empty() {
            return;
        }

        if !self.state.authenticated {
            self.state.search.error = Some("Not authenticated".to_string());
            return;
        }

        self.state.search.loading = true;
        self.state.search.error = None;
        self.state.search.tracks.clear();
        self.state.search.selected_index = 0;

        let core = Arc::clone(&self.core);
        let event_tx = self.search_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.search_tracks(&query, 25, 0, None).await;
            let _ = event_tx.send(result);
        });
    }

    /// Play the currently selected track from search results.
    fn play_selected_track(&mut self) {
        let idx = self.state.search.selected_index;
        let track = match self.state.search.tracks.get(idx) {
            Some(tr) => tr.clone(),
            None => return,
        };

        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        self.state.status_message = Some(format!("Loading: {}...", track.title));

        let core = Arc::clone(&self.core);
        let track_id = track.id;

        self.rt_handle.spawn(async move {
            // Get the QobuzClient from the core to call play_track on the player
            let client_lock = core.client();
            let client_guard = client_lock.read().await;
            if let Some(client) = client_guard.as_ref() {
                let player = core.player();
                if let Err(e) = player.play_track(client, track_id, Quality::HiRes).await {
                    log::error!("[TUI] Failed to play track {}: {}", track_id, e);
                }
            } else {
                log::error!("[TUI] No Qobuz client available for playback");
            }
        });
    }

    fn handle_search_result(&mut self, result: SearchResult) {
        self.state.search.loading = false;
        match result {
            Ok(page) => {
                self.state.search.total_results = page.total;
                self.state.search.tracks = page.items;
                self.state.search.selected_index = 0;
                self.state.search.error = None;

                let count = self.state.search.tracks.len();
                self.state.status_message =
                    Some(format!("{} tracks found (of {})", count, page.total));
            }
            Err(e) => {
                self.state.search.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Search failed: {}", e));
            }
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
