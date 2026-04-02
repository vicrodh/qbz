use std::io::{self, stdout};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::CrosstermBackend;
use ratatui::widgets::ScrollbarState;
use ratatui::Frame;
use ratatui::Terminal;
use tokio::sync::mpsc;

use qbz_audio::{settings::AudioSettingsStore, AudioDiagnostic, AudioSettings};
use qbz_cache::PlaybackCache;
use qbz_core::QbzCore;
use qbz_models::{Album, CoreEvent, DiscoverAlbum, DiscoverResponse, Playlist, QueueState, RepeatMode, SearchResultsPage, Track};
use qbz_player::Player;

use crate::adapter::TuiAdapter;
use crate::credentials;
use crate::playback::{self, PlaybackStatus};
use crate::ui::layout::{render_layout, LayoutAreas};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActiveView {
    Discovery,
    Favorites,
    Library,
    Playlists,
    Search,
    Settings,
    Album,
}

impl ActiveView {
    /// Human-readable label for display in placeholder views.
    pub fn label(self) -> &'static str {
        match self {
            ActiveView::Discovery => "Discovery",
            ActiveView::Favorites => "Favorites",
            ActiveView::Library => "Library",
            ActiveView::Playlists => "Playlists",
            ActiveView::Search => "Search",
            ActiveView::Settings => "Settings",
            ActiveView::Album => "Album",
        }
    }
}

/// Which tab is active in the discovery view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiscoveryTab {
    Home,
    EditorPicks,
    ForYou,
}

/// Simplified track info for display in the queue panel.
#[derive(Debug, Clone)]
pub struct QueueTrackInfo {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub duration_secs: u64,
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

/// Which tab is active in the favorites view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FavoritesTab {
    Tracks,
    Albums,
    Artists,
    Playlists,
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
    /// Album results from the last search.
    pub albums: Vec<Album>,
    /// Artist results from the last search.
    pub artists: Vec<qbz_models::Artist>,
    /// Currently selected index in the results list.
    pub selected_index: usize,
    /// Total results reported by the API.
    pub total_results: u32,
    /// Whether a search is currently in progress.
    pub loading: bool,
    /// Error message from the last search attempt.
    pub error: Option<String>,
    /// Scrollbar state for the results list.
    pub scrollbar_state: ScrollbarState,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            cursor: 0,
            tab: SearchTab::Tracks,
            tracks: Vec::new(),
            albums: Vec::new(),
            artists: Vec::new(),
            selected_index: 0,
            total_results: 0,
            loading: false,
            error: None,
            scrollbar_state: ScrollbarState::default(),
        }
    }
}

/// State for the favorites view.
pub struct FavoritesState {
    /// Active tab in the favorites view.
    pub tab: FavoritesTab,
    /// Favorite tracks from the last load.
    pub tracks: Vec<Track>,
    /// Favorite albums from the last load.
    pub albums: Vec<Album>,
    /// Favorite artists from the last load.
    pub artists: Vec<qbz_models::Artist>,
    /// Currently selected index in the active tab's list.
    pub selected_index: usize,
    /// Whether a load is currently in progress.
    pub loading: bool,
    /// Error message from the last load attempt.
    pub error: Option<String>,
    /// Whether tracks have been fetched at least once.
    pub loaded: bool,
    /// Whether albums have been fetched at least once.
    pub albums_loaded: bool,
    /// Whether artists have been fetched at least once.
    pub artists_loaded: bool,
    /// Scrollbar state for the active list.
    pub scrollbar_state: ScrollbarState,
}

impl Default for FavoritesState {
    fn default() -> Self {
        Self {
            tab: FavoritesTab::Tracks,
            tracks: Vec::new(),
            albums: Vec::new(),
            artists: Vec::new(),
            selected_index: 0,
            loading: false,
            error: None,
            loaded: false,
            albums_loaded: false,
            artists_loaded: false,
            scrollbar_state: ScrollbarState::default(),
        }
    }
}

/// State for the album detail view.
pub struct AlbumState {
    /// The loaded album metadata.
    pub album: Option<Album>,
    /// Album tracks (from album.tracks.items).
    pub tracks: Vec<Track>,
    /// Currently selected track index.
    pub selected_index: usize,
    /// Whether the album is being loaded.
    pub loading: bool,
    /// Error from the last load attempt.
    pub error: Option<String>,
    /// Scrollbar state for the track list.
    pub scrollbar_state: ScrollbarState,
    /// The view to return to when pressing Backspace/Esc.
    pub return_view: ActiveView,
}

impl Default for AlbumState {
    fn default() -> Self {
        Self {
            album: None,
            tracks: Vec::new(),
            selected_index: 0,
            loading: false,
            error: None,
            scrollbar_state: ScrollbarState::default(),
            return_view: ActiveView::Search,
        }
    }
}

/// State for the playlists view.
pub struct PlaylistsState {
    /// User's playlists.
    pub playlists: Vec<Playlist>,
    /// Currently selected playlist index.
    pub selected_index: usize,
    /// Whether playlists are being loaded.
    pub loading: bool,
    /// Error from the last load attempt.
    pub error: Option<String>,
    /// Whether playlists have been loaded.
    pub loaded: bool,
    /// Scrollbar state.
    pub scrollbar_state: ScrollbarState,
    /// Currently viewing a playlist's tracks (Some = detail view).
    pub detail_playlist: Option<Playlist>,
    /// Selected track index in playlist detail.
    pub detail_selected_index: usize,
    /// Scrollbar state for playlist detail.
    pub detail_scrollbar_state: ScrollbarState,
}

impl Default for PlaylistsState {
    fn default() -> Self {
        Self {
            playlists: Vec::new(),
            selected_index: 0,
            loading: false,
            error: None,
            loaded: false,
            scrollbar_state: ScrollbarState::default(),
            detail_playlist: None,
            detail_selected_index: 0,
            detail_scrollbar_state: ScrollbarState::default(),
        }
    }
}

/// State for the discovery view.
pub struct DiscoveryState {
    /// Active tab.
    pub tab: DiscoveryTab,
    /// Currently selected index in the active tab's list.
    pub selected_index: usize,
    /// Whether a load is in progress.
    pub loading: bool,
    /// Error from the last load attempt.
    pub error: Option<String>,
    /// Whether Home tab data has been loaded.
    pub loaded: bool,
    // Home tab data (from discover index)
    pub new_releases: Vec<DiscoverAlbum>,
    pub most_streamed: Vec<DiscoverAlbum>,
    pub press_awards: Vec<DiscoverAlbum>,
    pub qobuzissimes: Vec<DiscoverAlbum>,
    // Editor's Picks tab
    pub editor_picks: Vec<Album>,
    pub editor_picks_loaded: bool,
    // For You tab (favorite albums)
    pub for_you_albums: Vec<Album>,
    pub for_you_loaded: bool,
    /// Scrollbar state.
    pub scrollbar_state: ScrollbarState,
}

impl Default for DiscoveryState {
    fn default() -> Self {
        Self {
            tab: DiscoveryTab::Home,
            selected_index: 0,
            loading: false,
            error: None,
            loaded: false,
            new_releases: Vec::new(),
            most_streamed: Vec::new(),
            press_awards: Vec::new(),
            qobuzissimes: Vec::new(),
            editor_picks: Vec::new(),
            editor_picks_loaded: false,
            for_you_albums: Vec::new(),
            for_you_loaded: false,
            scrollbar_state: ScrollbarState::default(),
        }
    }
}

/// State for the settings view.
pub struct SettingsState {
    /// Loaded audio settings (snapshot).
    pub audio_settings: AudioSettings,
    /// Whether settings have been loaded from the database.
    pub loaded: bool,
    /// Currently selected setting index.
    pub selected_index: usize,
    /// Scrollbar state.
    pub scrollbar_state: ScrollbarState,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            audio_settings: AudioSettings::default(),
            loaded: false,
            selected_index: 0,
            scrollbar_state: ScrollbarState::default(),
        }
    }
}

pub struct AppState {
    pub active_view: ActiveView,
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
    pub favorites: FavoritesState,
    /// Transient status message shown at the bottom.
    pub status_message: Option<String>,
    /// Simplified queue info for the queue panel display.
    pub queue_tracks: Vec<QueueTrackInfo>,
    /// Index of the currently playing track within the full queue.
    pub queue_current_index: Option<usize>,
    /// Whether shuffle mode is active.
    pub queue_shuffle: bool,
    /// Current repeat mode.
    pub queue_repeat: RepeatMode,
    /// Whether the queue panel is visible (toggled with 'q').
    pub show_queue_panel: bool,
    /// Scrollbar state for the queue panel.
    pub queue_scrollbar_state: ScrollbarState,
    /// Whether the search modal popup is visible (toggled with '/').
    pub show_search_modal: bool,
    /// Album detail view state.
    pub album: AlbumState,
    /// Settings view state.
    pub settings: SettingsState,
    /// Playlists view state.
    pub playlists: PlaylistsState,
    /// Discovery view state.
    pub discovery: DiscoveryState,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            active_view: ActiveView::Discovery,
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
            favorites: FavoritesState::default(),
            status_message: None,
            queue_tracks: Vec::new(),
            queue_current_index: None,
            queue_shuffle: false,
            queue_repeat: RepeatMode::Off,
            show_queue_panel: false,
            queue_scrollbar_state: ScrollbarState::default(),
            show_search_modal: false,
            album: AlbumState::default(),
            settings: SettingsState::default(),
            playlists: PlaylistsState::default(),
            discovery: DiscoveryState::default(),
        }
    }
}

/// Type alias for the album result payload.
type AlbumResult = Result<Album, qbz_core::error::CoreError>;

/// Type alias for the search result payload.
type SearchResult = Result<qbz_models::SearchResultsPage<Track>, qbz_core::error::CoreError>;

/// Type alias for the search albums result payload.
type SearchAlbumsResult = Result<qbz_models::SearchResultsPage<Album>, qbz_core::error::CoreError>;

/// Type alias for the search artists result payload.
type SearchArtistsResult = Result<qbz_models::SearchResultsPage<qbz_models::Artist>, qbz_core::error::CoreError>;

/// Type alias for the favorites result payload.
type FavoritesResult = Result<Vec<Track>, qbz_core::error::CoreError>;

/// Type alias for the favorites albums result payload.
type FavoritesAlbumsResult = Result<Vec<Album>, qbz_core::error::CoreError>;

/// Type alias for the favorites artists result payload.
type FavoritesArtistsResult = Result<Vec<qbz_models::Artist>, qbz_core::error::CoreError>;

/// Type alias for the playlists result payload.
type PlaylistsResult = Result<Vec<Playlist>, qbz_core::error::CoreError>;

/// Type alias for the playlist detail result payload.
type PlaylistDetailResult = Result<Playlist, qbz_core::error::CoreError>;

/// Type alias for the discover index result payload.
type DiscoverResult = Result<DiscoverResponse, qbz_core::error::CoreError>;

/// Type alias for the editor picks result payload.
type EditorPicksResult = Result<SearchResultsPage<Album>, qbz_core::error::CoreError>;

/// Type alias for the for-you (favorite albums) result payload.
type ForYouResult = Result<Vec<Album>, qbz_core::error::CoreError>;

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
    /// Sender for search albums results.
    search_albums_result_tx: mpsc::UnboundedSender<SearchAlbumsResult>,
    /// Receiver for search albums results.
    search_albums_result_rx: mpsc::UnboundedReceiver<SearchAlbumsResult>,
    /// Sender for search artists results.
    search_artists_result_tx: mpsc::UnboundedSender<SearchArtistsResult>,
    /// Receiver for search artists results.
    search_artists_result_rx: mpsc::UnboundedReceiver<SearchArtistsResult>,
    /// Sender for favorites results (cloned into async tasks).
    favorites_result_tx: mpsc::UnboundedSender<FavoritesResult>,
    /// Receiver for favorites results (drained each tick).
    favorites_result_rx: mpsc::UnboundedReceiver<FavoritesResult>,
    /// Sender for album detail results (cloned into async tasks).
    album_result_tx: mpsc::UnboundedSender<AlbumResult>,
    /// Receiver for album detail results (drained each tick).
    album_result_rx: mpsc::UnboundedReceiver<AlbumResult>,
    /// Sender for favorites albums results.
    fav_albums_result_tx: mpsc::UnboundedSender<FavoritesAlbumsResult>,
    /// Receiver for favorites albums results.
    fav_albums_result_rx: mpsc::UnboundedReceiver<FavoritesAlbumsResult>,
    /// Sender for favorites artists results.
    fav_artists_result_tx: mpsc::UnboundedSender<FavoritesArtistsResult>,
    /// Receiver for favorites artists results.
    fav_artists_result_rx: mpsc::UnboundedReceiver<FavoritesArtistsResult>,
    /// Sender for playlists results.
    playlists_result_tx: mpsc::UnboundedSender<PlaylistsResult>,
    /// Receiver for playlists results.
    playlists_result_rx: mpsc::UnboundedReceiver<PlaylistsResult>,
    /// Sender for playlist detail results.
    playlist_detail_result_tx: mpsc::UnboundedSender<PlaylistDetailResult>,
    /// Receiver for playlist detail results.
    playlist_detail_result_rx: mpsc::UnboundedReceiver<PlaylistDetailResult>,
    /// Sender for discover index results.
    discover_result_tx: mpsc::UnboundedSender<DiscoverResult>,
    /// Receiver for discover index results.
    discover_result_rx: mpsc::UnboundedReceiver<DiscoverResult>,
    /// Sender for editor picks results.
    editor_picks_result_tx: mpsc::UnboundedSender<EditorPicksResult>,
    /// Receiver for editor picks results.
    editor_picks_result_rx: mpsc::UnboundedReceiver<EditorPicksResult>,
    /// Sender for for-you results.
    for_you_result_tx: mpsc::UnboundedSender<ForYouResult>,
    /// Receiver for for-you results.
    for_you_result_rx: mpsc::UnboundedReceiver<ForYouResult>,
    /// Layout areas from the last render, used for mouse hit-testing.
    layout_areas: LayoutAreas,
    /// Sender for playback status updates (cloned into async tasks).
    playback_status_tx: mpsc::UnboundedSender<PlaybackStatus>,
    /// Receiver for playback status updates (drained each tick).
    playback_status_rx: mpsc::UnboundedReceiver<PlaybackStatus>,
    /// Whether playback was active on the previous tick (for auto-advance detection).
    was_playing: bool,
    /// L2 disk cache for playback audio data.
    playback_cache: Option<Arc<PlaybackCache>>,
}

impl App {
    pub async fn new(no_images: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let (event_tx, event_rx) = mpsc::unbounded_channel::<CoreEvent>();
        let adapter = TuiAdapter::new(event_tx);

        // Load saved audio settings from database (same as desktop CoreBridge)
        let (device_name, audio_settings) = AudioSettingsStore::new()
            .ok()
            .and_then(|store| {
                store
                    .get_settings()
                    .ok()
                    .map(|settings| (settings.output_device.clone(), settings))
            })
            .unwrap_or_else(|| {
                log::info!("[TUI] No saved audio settings, using defaults");
                (None, AudioSettings::default())
            });

        log::info!(
            "[TUI] Player init: device={:?}, backend={:?}, exclusive={}, dac_passthrough={}",
            device_name,
            audio_settings.backend_type,
            audio_settings.exclusive_mode,
            audio_settings.dac_passthrough
        );

        let diagnostic = AudioDiagnostic::new();
        let player = Player::new(device_name, audio_settings, None, diagnostic);
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
            let mut logged_in = false;

            // Try email/password first
            if let Ok(Some(creds)) = credentials::load_qobuz_credentials() {
                log::info!("[TUI] Found saved credentials for {}", creds.email);
                match core.login(&creds.email, &creds.password).await {
                    Ok(session) => {
                        log::info!("[TUI] Authenticated as {} (plan: {})", session.email, session.subscription_label);
                        state.authenticated = true;
                        state.auth_email = Some(session.email);
                        state.status_message = Some(format!("Logged in ({})", session.subscription_label));
                        logged_in = true;
                    }
                    Err(e) => log::warn!("[TUI] Password auth failed: {}", e),
                }
            }

            // Fallback: try saved OAuth token
            if !logged_in {
                match credentials::load_oauth_token() {
                    Ok(Some(token)) => {
                        log::info!("[TUI] Found saved OAuth token, restoring session...");
                        match core.login_with_token(&token).await {
                            Ok(session) => {
                                log::info!("[TUI] OAuth session restored for {}", session.email);
                                state.authenticated = true;
                                state.auth_email = Some(session.email);
                                state.status_message = Some(format!("Logged in ({})", session.subscription_label));
                            }
                            Err(e) => {
                                log::warn!("[TUI] OAuth token expired or invalid: {}", e);
                                state.status_message = Some(format!("Auth failed: {}", e));
                            }
                        }
                    }
                    Ok(None) => {
                        log::info!("[TUI] No saved credentials or OAuth token found");
                        state.status_message = Some("Not logged in".to_string());
                    }
                    Err(e) => {
                        log::warn!("[TUI] Failed to load OAuth token: {}", e);
                        state.status_message = Some("Not logged in".to_string());
                    }
                }
            }
        }

        let core = Arc::new(core);
        let rt_handle = tokio::runtime::Handle::current();

        let (search_tx, search_rx) = mpsc::unbounded_channel::<SearchResult>();
        let (search_albums_tx, search_albums_rx) = mpsc::unbounded_channel::<SearchAlbumsResult>();
        let (search_artists_tx, search_artists_rx) = mpsc::unbounded_channel::<SearchArtistsResult>();
        let (favorites_tx, favorites_rx) = mpsc::unbounded_channel::<FavoritesResult>();
        let (album_tx, album_rx) = mpsc::unbounded_channel::<AlbumResult>();
        let (fav_albums_tx, fav_albums_rx) = mpsc::unbounded_channel::<FavoritesAlbumsResult>();
        let (fav_artists_tx, fav_artists_rx) = mpsc::unbounded_channel::<FavoritesArtistsResult>();
        let (playlists_tx, playlists_rx) = mpsc::unbounded_channel::<PlaylistsResult>();
        let (playlist_detail_tx, playlist_detail_rx) = mpsc::unbounded_channel::<PlaylistDetailResult>();
        let (playback_tx, playback_rx) = mpsc::unbounded_channel::<PlaybackStatus>();
        let (discover_tx, discover_rx) = mpsc::unbounded_channel::<DiscoverResult>();
        let (editor_picks_tx, editor_picks_rx) = mpsc::unbounded_channel::<EditorPicksResult>();
        let (for_you_tx, for_you_rx) = mpsc::unbounded_channel::<ForYouResult>();

        // Initialize L2 disk playback cache (800MB limit)
        let playback_cache = match PlaybackCache::new(800 * 1024 * 1024) {
            Ok(cache) => {
                let stats = cache.stats();
                log::info!(
                    "[TUI] Playback cache initialized: {} tracks, {} MB",
                    stats.cached_tracks,
                    stats.current_size_bytes / 1_048_576
                );
                Some(Arc::new(cache))
            }
            Err(e) => {
                log::warn!("[TUI] Playback cache unavailable: {}", e);
                None
            }
        };

        Ok(Self {
            state,
            core_event_rx: event_rx,
            core,
            should_quit: false,
            no_images,
            rt_handle,
            search_result_tx: search_tx,
            search_result_rx: search_rx,
            search_albums_result_tx: search_albums_tx,
            search_albums_result_rx: search_albums_rx,
            search_artists_result_tx: search_artists_tx,
            search_artists_result_rx: search_artists_rx,
            favorites_result_tx: favorites_tx,
            favorites_result_rx: favorites_rx,
            album_result_tx: album_tx,
            album_result_rx: album_rx,
            fav_albums_result_tx: fav_albums_tx,
            fav_albums_result_rx: fav_albums_rx,
            fav_artists_result_tx: fav_artists_tx,
            fav_artists_result_rx: fav_artists_rx,
            playlists_result_tx: playlists_tx,
            playlists_result_rx: playlists_rx,
            playlist_detail_result_tx: playlist_detail_tx,
            playlist_detail_result_rx: playlist_detail_rx,
            discover_result_tx: discover_tx,
            discover_result_rx: discover_rx,
            editor_picks_result_tx: editor_picks_tx,
            editor_picks_result_rx: editor_picks_rx,
            for_you_result_tx: for_you_tx,
            for_you_result_rx: for_you_rx,
            layout_areas: LayoutAreas::default(),
            playback_status_tx: playback_tx,
            playback_status_rx: playback_rx,
            was_playing: false,
            playback_cache,
        })
    }

    /// Render the full UI for the current frame and return the computed layout areas.
    pub fn draw(&mut self, frame: &mut Frame) -> LayoutAreas {
        render_layout(frame, &mut self.state)
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Install panic hook to restore terminal on crash
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture
            );
            let _ = execute!(io::stdout(), crossterm::cursor::Show);
            original_hook(panic_info);
        }));

        // Set up terminal
        enable_raw_mode()?;
        let mut stdout_handle = stdout();
        execute!(
            stdout_handle,
            EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
        let backend = CrosstermBackend::new(stdout_handle);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        // Main event loop
        while !self.should_quit {
            // Draw UI and capture layout areas for mouse hit-testing
            let areas = std::cell::Cell::new(LayoutAreas::default());
            terminal.draw(|frame| {
                areas.set(self.draw(frame));
            })?;
            self.layout_areas = areas.get();

            // Poll crossterm events with 100ms timeout
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) => {
                        // Only handle key press events (ignore release/repeat on some terminals)
                        if key.kind == KeyEventKind::Press {
                            self.handle_key(key);
                        }
                    }
                    Event::Mouse(mouse) => {
                        self.handle_mouse(mouse);
                    }
                    _ => {}
                }
            }

            // Drain all pending core events
            while let Ok(core_event) = self.core_event_rx.try_recv() {
                self.handle_core_event(core_event);
            }

            // Poll player state for now-playing bar (player doesn't emit events)
            self.poll_player_state().await;

            // Drain playback status updates
            while let Ok(status) = self.playback_status_rx.try_recv() {
                match status {
                    PlaybackStatus::Buffering(msg) => {
                        self.state.status_message = Some(msg);
                    }
                    PlaybackStatus::Playing => {
                        self.state.status_message = None;
                    }
                    PlaybackStatus::Error(msg) => {
                        self.state.status_message = Some(format!("Error: {}", msg));
                    }
                }
            }

            // Drain search results
            while let Ok(result) = self.search_result_rx.try_recv() {
                self.handle_search_result(result);
            }

            // Drain search albums results
            while let Ok(result) = self.search_albums_result_rx.try_recv() {
                self.handle_search_albums_result(result);
            }

            // Drain search artists results
            while let Ok(result) = self.search_artists_result_rx.try_recv() {
                self.handle_search_artists_result(result);
            }

            // Drain favorites results
            while let Ok(result) = self.favorites_result_rx.try_recv() {
                self.handle_favorites_result(result);
            }

            // Drain album detail results
            while let Ok(result) = self.album_result_rx.try_recv() {
                self.handle_album_result(result);
            }

            // Drain favorites albums results
            while let Ok(result) = self.fav_albums_result_rx.try_recv() {
                self.handle_fav_albums_result(result);
            }

            // Drain favorites artists results
            while let Ok(result) = self.fav_artists_result_rx.try_recv() {
                self.handle_fav_artists_result(result);
            }

            // Drain playlists results
            while let Ok(result) = self.playlists_result_rx.try_recv() {
                self.handle_playlists_result(result);
            }

            // Drain playlist detail results
            while let Ok(result) = self.playlist_detail_result_rx.try_recv() {
                self.handle_playlist_detail_result(result);
            }

            // Drain discovery results
            while let Ok(result) = self.discover_result_rx.try_recv() {
                self.handle_discover_result(result);
            }

            // Drain editor picks results
            while let Ok(result) = self.editor_picks_result_rx.try_recv() {
                self.handle_editor_picks_result(result);
            }

            // Drain for-you results
            while let Ok(result) = self.for_you_result_rx.try_recv() {
                self.handle_for_you_result(result);
            }
        }

        // Cleanup terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
        execute!(terminal.backend_mut(), crossterm::cursor::Show)?;

        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match self.state.input_mode {
            InputMode::TextInput => self.handle_key_text_input(key),
            InputMode::Normal => self.handle_key_normal(key),
        }
    }

    /// Handle mouse events using the stored layout areas from the last render.
    fn handle_mouse(&mut self, mouse: MouseEvent) {
        let col = mouse.column;
        let row = mouse.row;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if Self::is_in_rect(col, row, self.layout_areas.sidebar) {
                    self.handle_sidebar_click(col, row);
                } else if self.state.active_view == ActiveView::Search
                    && Self::is_in_rect(col, row, self.layout_areas.search_results)
                {
                    self.handle_search_click(row);
                }
            }
            MouseEventKind::ScrollUp => {
                self.handle_scroll(-1);
            }
            MouseEventKind::ScrollDown => {
                self.handle_scroll(1);
            }
            _ => {}
        }
    }

    /// Check whether a screen position falls inside a given rectangle.
    fn is_in_rect(col: u16, row: u16, rect: ratatui::layout::Rect) -> bool {
        col >= rect.x
            && col < rect.x + rect.width
            && row >= rect.y
            && row < rect.y + rect.height
    }

    /// Map a click on the sidebar to a navigation item.
    fn handle_sidebar_click(&mut self, _col: u16, row: u16) {
        use crate::ui::sidebar::NAV_ITEMS;

        let area = self.layout_areas.sidebar;

        // The sidebar renders inside a Block with a right border, so the inner
        // area starts 1 row into the block (though the block has no top border,
        // the inner area matches the block area minus the right border column).
        // For hit-testing we use the row offset from the top of the sidebar area.
        let relative_row = row.saturating_sub(area.y);

        // Sidebar layout:
        //   row 0: header ("QBZ")
        //   row 1: separator
        //   row 2..2+N: nav items
        let nav_view = if relative_row >= 2 {
            let item_idx = (relative_row - 2) as usize;
            NAV_ITEMS.get(item_idx).map(|(view, _)| *view)
        } else {
            None
        };

        if let Some(view) = nav_view {
            self.state.active_view = view;
            if view == ActiveView::Discovery {
                self.load_discovery_if_needed();
            } else if view == ActiveView::Favorites {
                self.load_favorites_if_needed();
            } else if view == ActiveView::Settings {
                self.load_settings_if_needed();
            } else if view == ActiveView::Playlists {
                self.load_playlists_if_needed();
            }
        }
    }

    /// Map a click on the search results list to a selection change.
    fn handle_search_click(&mut self, row: u16) {
        let results_area = self.layout_areas.search_results;
        let relative_row = row.saturating_sub(results_area.y) as usize;
        let len = self.state.search.tracks.len();
        if len > 0 && relative_row < len {
            self.state.search.selected_index = relative_row;
        }
    }

    /// Handle scroll wheel: move the selection in the current list.
    fn handle_scroll(&mut self, delta: i32) {
        match self.state.active_view {
            ActiveView::Search => {
                let len = self.search_active_list_len();
                if len == 0 {
                    return;
                }
                let current = self.state.search.selected_index as i32;
                let next = (current + delta).clamp(0, (len as i32) - 1) as usize;
                self.state.search.selected_index = next;
            }
            ActiveView::Favorites => {
                let len = self.state.favorites.tracks.len();
                if len == 0 {
                    return;
                }
                let current = self.state.favorites.selected_index as i32;
                let next = (current + delta).clamp(0, (len as i32) - 1) as usize;
                self.state.favorites.selected_index = next;
            }
            ActiveView::Album => {
                let len = self.state.album.tracks.len();
                if len == 0 {
                    return;
                }
                let current = self.state.album.selected_index as i32;
                let next = (current + delta).clamp(0, (len as i32) - 1) as usize;
                self.state.album.selected_index = next;
            }
            ActiveView::Settings => {
                let total = self.settings_item_count();
                if total == 0 {
                    return;
                }
                let current = self.state.settings.selected_index as i32;
                let next = (current + delta).clamp(0, (total as i32) - 1) as usize;
                self.state.settings.selected_index = next;
            }
            ActiveView::Discovery => {
                let len = self.discovery_item_count();
                if len == 0 {
                    return;
                }
                let current = self.state.discovery.selected_index as i32;
                let next = (current + delta).clamp(0, (len as i32) - 1) as usize;
                self.state.discovery.selected_index = next;
            }
            ActiveView::Playlists => {
                if self.state.playlists.detail_playlist.is_some() {
                    let len = self.state.playlists.detail_playlist.as_ref()
                        .and_then(|p| p.tracks.as_ref())
                        .map(|tc| tc.items.len())
                        .unwrap_or(0);
                    if len > 0 {
                        let current = self.state.playlists.detail_selected_index as i32;
                        let next = (current + delta).clamp(0, (len as i32) - 1) as usize;
                        self.state.playlists.detail_selected_index = next;
                    }
                } else {
                    let len = self.state.playlists.playlists.len();
                    if len > 0 {
                        let current = self.state.playlists.selected_index as i32;
                        let next = (current + delta).clamp(0, (len as i32) - 1) as usize;
                        self.state.playlists.selected_index = next;
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle keys in normal (navigation) mode.
    fn handle_key_normal(&mut self, key: KeyEvent) {
        match key.code {
            // Ctrl+q or Ctrl+c quits the application
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            // 'q' toggles the queue panel
            KeyCode::Char('q') => {
                self.state.show_queue_panel = !self.state.show_queue_panel;
            }
            // Tab/BackTab for discovery tab cycling
            KeyCode::Tab if self.state.active_view == ActiveView::Discovery => {
                self.cycle_discovery_tab(true);
            }
            KeyCode::BackTab if self.state.active_view == ActiveView::Discovery => {
                self.cycle_discovery_tab(false);
            }

            // Discovery view: j/k for navigating items
            KeyCode::Char('j') | KeyCode::Down if self.state.active_view == ActiveView::Discovery => {
                let len = self.discovery_item_count();
                if len > 0 {
                    self.state.discovery.selected_index =
                        (self.state.discovery.selected_index + 1).min(len - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up if self.state.active_view == ActiveView::Discovery => {
                if self.state.discovery.selected_index > 0 {
                    self.state.discovery.selected_index -= 1;
                }
            }

            // Discovery view: Enter to navigate to album detail
            KeyCode::Enter if self.state.active_view == ActiveView::Discovery => {
                self.open_selected_discovery_album();
            }

            KeyCode::Char('1') => {
                self.state.active_view = ActiveView::Discovery;
                self.load_discovery_if_needed();
            }
            KeyCode::Char('2') => {
                self.state.active_view = ActiveView::Favorites;
                self.load_favorites_if_needed();
            }
            KeyCode::Char('3') => self.state.active_view = ActiveView::Library,
            KeyCode::Char('4') => {
                self.state.active_view = ActiveView::Playlists;
                self.load_playlists_if_needed();
            }
            KeyCode::Char('5') => {
                self.state.active_view = ActiveView::Search;
            }
            KeyCode::Char('6') => {
                self.state.active_view = ActiveView::Settings;
                self.load_settings_if_needed();
            }

            // '/' from any view opens the search modal popup
            KeyCode::Char('/') => {
                self.state.show_search_modal = true;
                self.state.input_mode = InputMode::TextInput;
            }

            // 'i' in search view enters text input mode (legacy, also opens modal)
            KeyCode::Char('i') if self.state.active_view == ActiveView::Search => {
                self.state.show_search_modal = true;
                self.state.input_mode = InputMode::TextInput;
            }

            // Search modal open (normal mode): j/k for navigating results
            KeyCode::Char('j') | KeyCode::Down if self.state.show_search_modal => {
                let len = self.state.search.tracks.len();
                if len > 0 {
                    self.state.search.selected_index =
                        (self.state.search.selected_index + 1).min(len - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up if self.state.show_search_modal => {
                if self.state.search.selected_index > 0 {
                    self.state.search.selected_index -= 1;
                }
            }

            // Search modal open: Enter to play selected track
            KeyCode::Enter if self.state.show_search_modal => {
                self.play_selected_track();
            }

            // Search modal open: Esc closes the modal
            KeyCode::Esc if self.state.show_search_modal => {
                self.state.show_search_modal = false;
            }

            // Search modal open: 'a' adds track to queue
            KeyCode::Char('a') if self.state.show_search_modal => {
                self.add_selected_to_queue();
            }

            // Search view: Tab/Shift+Tab to switch tabs
            KeyCode::Tab if self.state.active_view == ActiveView::Search && !self.state.show_search_modal => {
                self.cycle_search_tab(true);
            }
            KeyCode::BackTab if self.state.active_view == ActiveView::Search && !self.state.show_search_modal => {
                self.cycle_search_tab(false);
            }

            // Search view (non-modal): j/k for navigating results
            KeyCode::Char('j') | KeyCode::Down if self.state.active_view == ActiveView::Search => {
                let len = self.search_active_list_len();
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

            // Search view: Enter to play/open selected item
            KeyCode::Enter if self.state.active_view == ActiveView::Search => {
                match self.state.search.tab {
                    SearchTab::Tracks => self.play_selected_track(),
                    SearchTab::Albums => self.open_selected_search_album(),
                    SearchTab::Artists => {} // TODO: artist detail view
                }
            }

            // Favorites view: j/k for navigating tracks
            KeyCode::Char('j') | KeyCode::Down if self.state.active_view == ActiveView::Favorites => {
                let len = self.state.favorites.tracks.len();
                if len > 0 {
                    self.state.favorites.selected_index =
                        (self.state.favorites.selected_index + 1).min(len - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up if self.state.active_view == ActiveView::Favorites => {
                if self.state.favorites.selected_index > 0 {
                    self.state.favorites.selected_index -= 1;
                }
            }

            // Favorites view: Tab/Shift+Tab to switch tabs
            KeyCode::Tab if self.state.active_view == ActiveView::Favorites => {
                self.cycle_favorites_tab(true);
            }
            KeyCode::BackTab if self.state.active_view == ActiveView::Favorites => {
                self.cycle_favorites_tab(false);
            }

            // Favorites view: Enter to play selected item
            KeyCode::Enter if self.state.active_view == ActiveView::Favorites => {
                match self.state.favorites.tab {
                    FavoritesTab::Tracks => self.play_selected_favorite(),
                    FavoritesTab::Albums => self.open_selected_favorite_album(),
                    FavoritesTab::Artists => {} // TODO: artist detail view
                    FavoritesTab::Playlists => {} // use playlists view instead
                }
            }

            // Favorites view: 'a' to add selected track to queue
            KeyCode::Char('a') if self.state.active_view == ActiveView::Favorites => {
                self.add_selected_favorite_to_queue();
            }

            // Playlists view: j/k navigation
            KeyCode::Char('j') | KeyCode::Down if self.state.active_view == ActiveView::Playlists => {
                if self.state.playlists.detail_playlist.is_some() {
                    let len = self.state.playlists.detail_playlist.as_ref()
                        .and_then(|p| p.tracks.as_ref())
                        .map(|tc| tc.items.len())
                        .unwrap_or(0);
                    if len > 0 {
                        self.state.playlists.detail_selected_index =
                            (self.state.playlists.detail_selected_index + 1).min(len - 1);
                    }
                } else {
                    let len = self.state.playlists.playlists.len();
                    if len > 0 {
                        self.state.playlists.selected_index =
                            (self.state.playlists.selected_index + 1).min(len - 1);
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up if self.state.active_view == ActiveView::Playlists => {
                if self.state.playlists.detail_playlist.is_some() {
                    if self.state.playlists.detail_selected_index > 0 {
                        self.state.playlists.detail_selected_index -= 1;
                    }
                } else {
                    if self.state.playlists.selected_index > 0 {
                        self.state.playlists.selected_index -= 1;
                    }
                }
            }

            // Playlists view: Enter to open playlist detail or play track
            KeyCode::Enter if self.state.active_view == ActiveView::Playlists => {
                if self.state.playlists.detail_playlist.is_some() {
                    self.play_playlist_track();
                } else {
                    self.open_selected_playlist();
                }
            }

            // Playlists view: Backspace/Esc to return from playlist detail to list
            KeyCode::Backspace | KeyCode::Esc if self.state.active_view == ActiveView::Playlists && self.state.playlists.detail_playlist.is_some() => {
                self.state.playlists.detail_playlist = None;
                self.state.playlists.detail_selected_index = 0;
            }

            // Playlists view: 'a' to add track to queue (in detail view)
            KeyCode::Char('a') if self.state.active_view == ActiveView::Playlists && self.state.playlists.detail_playlist.is_some() => {
                self.add_playlist_track_to_queue();
            }

            // Search/Favorites: 'g' to go to album detail
            KeyCode::Char('g') if self.state.active_view == ActiveView::Search => {
                self.navigate_to_album_from_search();
            }
            KeyCode::Char('g') if self.state.active_view == ActiveView::Favorites => {
                self.navigate_to_album_from_favorites();
            }
            // Search modal: 'g' to go to album detail
            KeyCode::Char('g') if self.state.show_search_modal => {
                self.navigate_to_album_from_search();
            }

            // Album view: j/k navigation
            KeyCode::Char('j') | KeyCode::Down if self.state.active_view == ActiveView::Album => {
                let len = self.state.album.tracks.len();
                if len > 0 {
                    self.state.album.selected_index =
                        (self.state.album.selected_index + 1).min(len - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up if self.state.active_view == ActiveView::Album => {
                if self.state.album.selected_index > 0 {
                    self.state.album.selected_index -= 1;
                }
            }

            // Album view: Enter to play selected track (queues whole album starting from selection)
            KeyCode::Enter if self.state.active_view == ActiveView::Album => {
                self.play_album_from_selected();
            }

            // Album view: 'a' to add selected track to queue
            KeyCode::Char('a') if self.state.active_view == ActiveView::Album => {
                self.add_album_track_to_queue();
            }

            // Album view: Backspace/Esc returns to previous view
            KeyCode::Backspace | KeyCode::Esc if self.state.active_view == ActiveView::Album => {
                self.state.active_view = self.state.album.return_view;
            }

            // Settings view: j/k navigation
            KeyCode::Char('j') | KeyCode::Down if self.state.active_view == ActiveView::Settings => {
                let total = self.settings_item_count();
                if total > 0 {
                    self.state.settings.selected_index =
                        (self.state.settings.selected_index + 1).min(total - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up if self.state.active_view == ActiveView::Settings => {
                if self.state.settings.selected_index > 0 {
                    self.state.settings.selected_index -= 1;
                }
            }

            // Settings view: Enter/Space to toggle, +/- to adjust
            KeyCode::Enter | KeyCode::Char(' ') if self.state.active_view == ActiveView::Settings => {
                self.toggle_selected_setting();
            }

            // Settings view: 'r' to reload settings from database
            KeyCode::Char('r') if self.state.active_view == ActiveView::Settings => {
                self.state.settings.loaded = false;
                self.load_settings_if_needed();
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
                let status_tx = self.playback_status_tx.clone();
                let cache = self.playback_cache.clone();
                self.rt_handle.spawn(async move {
                    if let Some(track) = core.next_track().await {
                        log::info!("[TUI] Next track: {} - {}", track.artist, track.title);
                        if let Err(e) = playback::play_qobuz_track(&core, track.id, &cache, &status_tx).await {
                            log::error!("[TUI] Next track playback failed: {}", e);
                            let _ = status_tx.send(PlaybackStatus::Error(e));
                        }
                    }
                });
            }
            KeyCode::Char('p') => {
                let core = Arc::clone(&self.core);
                let status_tx = self.playback_status_tx.clone();
                let cache = self.playback_cache.clone();
                self.rt_handle.spawn(async move {
                    if let Some(track) = core.previous_track().await {
                        log::info!("[TUI] Previous track: {} - {}", track.artist, track.title);
                        if let Err(e) = playback::play_qobuz_track(&core, track.id, &cache, &status_tx).await {
                            log::error!("[TUI] Previous track playback failed: {}", e);
                            let _ = status_tx.send(PlaybackStatus::Error(e));
                        }
                    }
                });
            }

            // Search view: 'a' to add selected track to queue
            KeyCode::Char('a') if self.state.active_view == ActiveView::Search => {
                self.add_selected_to_queue();
            }

            // Settings view: +/- to adjust numeric values
            KeyCode::Char('+') | KeyCode::Char('=') if self.state.active_view == ActiveView::Settings => {
                self.adjust_selected_setting(1);
            }
            KeyCode::Char('-') if self.state.active_view == ActiveView::Settings => {
                self.adjust_selected_setting(-1);
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
                // If in modal, close it entirely
                if self.state.show_search_modal {
                    self.state.show_search_modal = false;
                }
            }
            KeyCode::Enter => {
                // Execute search, then return to normal mode for result navigation
                // (modal stays open for browsing results)
                self.execute_search();
                self.state.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                let search = &mut self.state.search;
                if search.cursor > 0 {
                    // Find the previous char boundary
                    let prev = search.query[..search.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    search.query.remove(prev);
                    search.cursor = prev;
                }
            }
            KeyCode::Delete => {
                let search = &mut self.state.search;
                if search.cursor < search.query.len() {
                    search.query.remove(search.cursor);
                }
            }
            KeyCode::Left => {
                let search = &mut self.state.search;
                if search.cursor > 0 {
                    // Move to previous char boundary
                    search.cursor = search.query[..search.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            KeyCode::Right => {
                let search = &mut self.state.search;
                if search.cursor < search.query.len() {
                    // Move to next char boundary
                    let rest = &search.query[search.cursor..];
                    let ch = rest.chars().next().unwrap();
                    search.cursor += ch.len_utf8();
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
                search.cursor += c.len_utf8();
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
        self.state.search.selected_index = 0;

        let core = Arc::clone(&self.core);

        match self.state.search.tab {
            SearchTab::Tracks => {
                self.state.search.tracks.clear();
                let event_tx = self.search_result_tx.clone();
                self.rt_handle.spawn(async move {
                    let result = core.search_tracks(&query, 25, 0, None).await;
                    let _ = event_tx.send(result);
                });
            }
            SearchTab::Albums => {
                self.state.search.albums.clear();
                let event_tx = self.search_albums_result_tx.clone();
                self.rt_handle.spawn(async move {
                    let result = core.search_albums(&query, 25, 0, None).await;
                    let _ = event_tx.send(result);
                });
            }
            SearchTab::Artists => {
                self.state.search.artists.clear();
                let event_tx = self.search_artists_result_tx.clone();
                self.rt_handle.spawn(async move {
                    let result = core.search_artists(&query, 25, 0, None).await;
                    let _ = event_tx.send(result);
                });
            }
        }
    }

    /// Build a QueueTrack from a search result Track.
    fn track_to_queue_track(track: &Track) -> qbz_models::QueueTrack {
        qbz_models::QueueTrack {
            id: track.id,
            title: track.title.clone(),
            artist: track.performer.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "Unknown".to_string()),
            album: track.album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
            duration_secs: track.duration as u64,
            artwork_url: track.album.as_ref().and_then(|a| a.image.large.clone()),
            hires: track.hires_streamable,
            bit_depth: track.maximum_bit_depth,
            sample_rate: track.maximum_sampling_rate,
            is_local: false,
            album_id: track.album.as_ref().map(|a| a.id.clone()),
            artist_id: track.performer.as_ref().map(|p| p.id),
            streamable: true,
            source: Some("qobuz".to_string()),
            parental_warning: false,
        }
    }

    /// Add the selected search result to the queue.
    fn add_selected_to_queue(&mut self) {
        let idx = self.state.search.selected_index;
        let track = match self.state.search.tracks.get(idx) {
            Some(tr) => tr.clone(),
            None => return,
        };

        let queue_track = Self::track_to_queue_track(&track);
        let core = Arc::clone(&self.core);

        self.state.status_message = Some(format!("Added to queue: {}", track.title));

        self.rt_handle.spawn(async move {
            core.add_track(queue_track).await;
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

        // Update now-playing info immediately (player state only has track_id)
        self.state.current_track_title = Some(track.title.clone());
        self.state.current_track_artist = Some(track.performer.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "Unknown".to_string()));
        self.state.current_track_quality = if track.hires_streamable {
            Some("Hi-Res".to_string())
        } else {
            Some(format!("{}bit/{}kHz", track.maximum_bit_depth.unwrap_or(16), track.maximum_sampling_rate.unwrap_or(44.1)))
        };
        self.state.status_message = Some(format!("Loading: {}...", track.title));

        let core = Arc::clone(&self.core);
        let track_id = track.id;
        let status_tx = self.playback_status_tx.clone();
        let cache = self.playback_cache.clone();

        self.rt_handle.spawn(async move {
            if let Err(e) = playback::play_qobuz_track(
                &core,
                track_id,
                &cache,
                &status_tx,
            )
            .await
            {
                log::error!("[TUI] Failed to play track {}: {}", track_id, e);
                let _ = status_tx.send(PlaybackStatus::Error(e));
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

    /// Handle the result of a search albums request.
    fn handle_search_albums_result(&mut self, result: SearchAlbumsResult) {
        self.state.search.loading = false;
        match result {
            Ok(page) => {
                self.state.search.total_results = page.total;
                self.state.search.albums = page.items;
                self.state.search.selected_index = 0;
                self.state.search.error = None;

                let count = self.state.search.albums.len();
                self.state.status_message =
                    Some(format!("{} albums found (of {})", count, page.total));
            }
            Err(e) => {
                self.state.search.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Search failed: {}", e));
            }
        }
    }

    /// Handle the result of a search artists request.
    fn handle_search_artists_result(&mut self, result: SearchArtistsResult) {
        self.state.search.loading = false;
        match result {
            Ok(page) => {
                self.state.search.total_results = page.total;
                self.state.search.artists = page.items;
                self.state.search.selected_index = 0;
                self.state.search.error = None;

                let count = self.state.search.artists.len();
                self.state.status_message =
                    Some(format!("{} artists found (of {})", count, page.total));
            }
            Err(e) => {
                self.state.search.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Search failed: {}", e));
            }
        }
    }

    /// Get the length of the active search results list.
    fn search_active_list_len(&self) -> usize {
        match self.state.search.tab {
            SearchTab::Tracks => self.state.search.tracks.len(),
            SearchTab::Albums => self.state.search.albums.len(),
            SearchTab::Artists => self.state.search.artists.len(),
        }
    }

    /// Cycle through search tabs.
    fn cycle_search_tab(&mut self, forward: bool) {
        let tabs = [SearchTab::Tracks, SearchTab::Albums, SearchTab::Artists];
        let current = tabs.iter().position(|tab| *tab == self.state.search.tab).unwrap_or(0);
        let next = if forward {
            (current + 1) % tabs.len()
        } else {
            (current + tabs.len() - 1) % tabs.len()
        };
        self.state.search.tab = tabs[next];
        self.state.search.selected_index = 0;

        // Re-execute the search for the new tab if there's a query
        if !self.state.search.query.trim().is_empty() {
            self.execute_search();
        }
    }

    /// Open the selected album from search results.
    fn open_selected_search_album(&mut self) {
        let idx = self.state.search.selected_index;
        let album_id = match self.state.search.albums.get(idx) {
            Some(album) => album.id.clone(),
            None => return,
        };
        self.load_album(&album_id, ActiveView::Search);
    }

    /// Load favorites if they haven't been loaded yet (lazy load on first visit).
    fn load_favorites_if_needed(&mut self) {
        if self.state.favorites.loaded || self.state.favorites.loading {
            return;
        }

        if !self.state.authenticated {
            self.state.favorites.error = Some("Not authenticated".to_string());
            return;
        }

        self.state.favorites.loading = true;
        self.state.favorites.error = None;
        self.state.status_message = Some("Loading favorites...".to_string());

        let core = Arc::clone(&self.core);
        let event_tx = self.favorites_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.get_favorites("tracks", 500, 0).await;
            let parsed = result.and_then(|json| {
                // The API returns { "tracks": { "items": [...], "total": N, ... } }
                // when fav_type is "tracks". The key matches the fav_type argument.
                let tracks_page = json
                    .get("tracks")
                    .and_then(|tracks| {
                        serde_json::from_value::<qbz_models::SearchResultsPage<Track>>(tracks.clone()).ok()
                    });
                match tracks_page {
                    Some(page) => Ok(page.items),
                    None => {
                        log::warn!("[TUI] Could not parse favorites response");
                        Ok(Vec::new())
                    }
                }
            });
            let _ = event_tx.send(parsed);
        });
    }

    /// Handle the result of a favorites load.
    fn handle_favorites_result(&mut self, result: FavoritesResult) {
        self.state.favorites.loading = false;
        self.state.favorites.loaded = true;
        match result {
            Ok(tracks) => {
                let count = tracks.len();
                self.state.favorites.tracks = tracks;
                self.state.favorites.selected_index = 0;
                self.state.favorites.error = None;
                self.state.status_message = Some(format!("{} favorite tracks", count));
            }
            Err(e) => {
                self.state.favorites.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load favorites: {}", e));
            }
        }
    }

    /// Play the currently selected track from favorites.
    fn play_selected_favorite(&mut self) {
        let idx = self.state.favorites.selected_index;
        let track = match self.state.favorites.tracks.get(idx) {
            Some(tr) => tr.clone(),
            None => return,
        };

        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        // Update now-playing info immediately
        self.state.current_track_title = Some(track.title.clone());
        self.state.current_track_artist = Some(
            track
                .performer
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
        );
        self.state.current_track_quality = if track.hires_streamable {
            Some("Hi-Res".to_string())
        } else {
            Some(format!(
                "{}bit/{}kHz",
                track.maximum_bit_depth.unwrap_or(16),
                track.maximum_sampling_rate.unwrap_or(44.1)
            ))
        };
        self.state.status_message = Some(format!("Loading: {}...", track.title));

        let core = Arc::clone(&self.core);
        let track_id = track.id;
        let status_tx = self.playback_status_tx.clone();
        let cache = self.playback_cache.clone();

        self.rt_handle.spawn(async move {
            if let Err(e) =
                playback::play_qobuz_track(&core, track_id, &cache, &status_tx).await
            {
                log::error!("[TUI] Failed to play favorite track {}: {}", track_id, e);
                let _ = status_tx.send(PlaybackStatus::Error(e));
            }
        });
    }

    /// Add the selected favorite track to the queue.
    fn add_selected_favorite_to_queue(&mut self) {
        let idx = self.state.favorites.selected_index;
        let track = match self.state.favorites.tracks.get(idx) {
            Some(tr) => tr.clone(),
            None => return,
        };

        let queue_track = Self::track_to_queue_track(&track);
        let core = Arc::clone(&self.core);

        self.state.status_message = Some(format!("Added to queue: {}", track.title));

        self.rt_handle.spawn(async move {
            core.add_track(queue_track).await;
        });
    }

    // ==================== Favorites Tabs ====================

    /// Cycle through favorites tabs (forward or backward).
    fn cycle_favorites_tab(&mut self, forward: bool) {
        let tabs = [
            FavoritesTab::Tracks,
            FavoritesTab::Albums,
            FavoritesTab::Artists,
        ];
        let current = tabs.iter().position(|tab| *tab == self.state.favorites.tab).unwrap_or(0);
        let next = if forward {
            (current + 1) % tabs.len()
        } else {
            (current + tabs.len() - 1) % tabs.len()
        };
        self.state.favorites.tab = tabs[next];
        self.state.favorites.selected_index = 0;

        // Load data for the new tab if needed
        match self.state.favorites.tab {
            FavoritesTab::Tracks => self.load_favorites_if_needed(),
            FavoritesTab::Albums => self.load_favorite_albums_if_needed(),
            FavoritesTab::Artists => self.load_favorite_artists_if_needed(),
            FavoritesTab::Playlists => {} // use playlists view
        }
    }

    /// Load favorite albums if not already loaded.
    fn load_favorite_albums_if_needed(&mut self) {
        if self.state.favorites.albums_loaded || self.state.favorites.loading {
            return;
        }

        if !self.state.authenticated {
            self.state.favorites.error = Some("Not authenticated".to_string());
            return;
        }

        self.state.favorites.loading = true;
        self.state.favorites.error = None;
        self.state.status_message = Some("Loading favorite albums...".to_string());

        let core = Arc::clone(&self.core);
        let event_tx = self.fav_albums_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.get_favorites("albums", 500, 0).await;
            let parsed = result.and_then(|json| {
                let albums_page = json
                    .get("albums")
                    .and_then(|albums| {
                        serde_json::from_value::<qbz_models::SearchResultsPage<Album>>(albums.clone()).ok()
                    });
                match albums_page {
                    Some(page) => Ok(page.items),
                    None => {
                        log::warn!("[TUI] Could not parse favorites albums response");
                        Ok(Vec::new())
                    }
                }
            });
            let _ = event_tx.send(parsed);
        });
    }

    /// Handle the result of a favorites albums load.
    fn handle_fav_albums_result(&mut self, result: FavoritesAlbumsResult) {
        self.state.favorites.loading = false;
        self.state.favorites.albums_loaded = true;
        match result {
            Ok(albums) => {
                let count = albums.len();
                self.state.favorites.albums = albums;
                self.state.favorites.error = None;
                self.state.status_message = Some(format!("{} favorite albums", count));
            }
            Err(e) => {
                self.state.favorites.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load favorite albums: {}", e));
            }
        }
    }

    /// Load favorite artists if not already loaded.
    fn load_favorite_artists_if_needed(&mut self) {
        if self.state.favorites.artists_loaded || self.state.favorites.loading {
            return;
        }

        if !self.state.authenticated {
            self.state.favorites.error = Some("Not authenticated".to_string());
            return;
        }

        self.state.favorites.loading = true;
        self.state.favorites.error = None;
        self.state.status_message = Some("Loading favorite artists...".to_string());

        let core = Arc::clone(&self.core);
        let event_tx = self.fav_artists_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.get_favorites("artists", 500, 0).await;
            let parsed = result.and_then(|json| {
                let artists_page = json
                    .get("artists")
                    .and_then(|artists| {
                        serde_json::from_value::<qbz_models::SearchResultsPage<qbz_models::Artist>>(artists.clone()).ok()
                    });
                match artists_page {
                    Some(page) => Ok(page.items),
                    None => {
                        log::warn!("[TUI] Could not parse favorites artists response");
                        Ok(Vec::new())
                    }
                }
            });
            let _ = event_tx.send(parsed);
        });
    }

    /// Handle the result of a favorites artists load.
    fn handle_fav_artists_result(&mut self, result: FavoritesArtistsResult) {
        self.state.favorites.loading = false;
        self.state.favorites.artists_loaded = true;
        match result {
            Ok(artists) => {
                let count = artists.len();
                self.state.favorites.artists = artists;
                self.state.favorites.error = None;
                self.state.status_message = Some(format!("{} favorite artists", count));
            }
            Err(e) => {
                self.state.favorites.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load favorite artists: {}", e));
            }
        }
    }

    /// Open the selected album from favorites albums tab.
    fn open_selected_favorite_album(&mut self) {
        let idx = self.state.favorites.selected_index;
        let album_id = match self.state.favorites.albums.get(idx) {
            Some(album) => album.id.clone(),
            None => return,
        };
        self.load_album(&album_id, ActiveView::Favorites);
    }

    // ==================== Playlists ====================

    /// Load user playlists if not already loaded.
    fn load_playlists_if_needed(&mut self) {
        if self.state.playlists.loaded || self.state.playlists.loading {
            return;
        }

        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        self.state.playlists.loading = true;
        self.state.status_message = Some("Loading playlists...".to_string());

        let core = Arc::clone(&self.core);
        let event_tx = self.playlists_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.get_user_playlists().await;
            let _ = event_tx.send(result);
        });
    }

    /// Handle the result of a playlists load.
    fn handle_playlists_result(&mut self, result: PlaylistsResult) {
        self.state.playlists.loading = false;
        self.state.playlists.loaded = true;
        match result {
            Ok(playlists) => {
                let count = playlists.len();
                self.state.playlists.playlists = playlists;
                self.state.playlists.error = None;
                self.state.status_message = Some(format!("{} playlists", count));
            }
            Err(e) => {
                self.state.playlists.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load playlists: {}", e));
            }
        }
    }

    /// Open the selected playlist to show its tracks.
    fn open_selected_playlist(&mut self) {
        let idx = self.state.playlists.selected_index;
        let playlist = match self.state.playlists.playlists.get(idx) {
            Some(p) => p.clone(),
            None => return,
        };

        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        let playlist_id = playlist.id;
        self.state.playlists.loading = true;
        self.state.status_message = Some(format!("Loading {}...", playlist.name));

        let core = Arc::clone(&self.core);
        let event_tx = self.playlist_detail_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.get_playlist(playlist_id).await;
            let _ = event_tx.send(result);
        });
    }

    /// Handle the result of a playlist detail load.
    fn handle_playlist_detail_result(&mut self, result: PlaylistDetailResult) {
        self.state.playlists.loading = false;
        match result {
            Ok(playlist) => {
                let track_count = playlist.tracks.as_ref().map(|tc| tc.items.len()).unwrap_or(0);
                self.state.status_message = Some(format!("{} - {} tracks", playlist.name, track_count));
                self.state.playlists.detail_playlist = Some(playlist);
                self.state.playlists.detail_selected_index = 0;
            }
            Err(e) => {
                self.state.status_message = Some(format!("Failed to load playlist: {}", e));
            }
        }
    }

    /// Play the selected track from a playlist detail view.
    fn play_playlist_track(&mut self) {
        let playlist = match &self.state.playlists.detail_playlist {
            Some(p) => p,
            None => return,
        };

        let tracks = match &playlist.tracks {
            Some(tc) => &tc.items,
            None => return,
        };

        let idx = self.state.playlists.detail_selected_index;
        let track = match tracks.get(idx) {
            Some(tr) => tr.clone(),
            None => return,
        };

        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        // Queue remaining tracks from the playlist
        let queue_tracks: Vec<qbz_models::QueueTrack> = tracks[idx..]
            .iter()
            .map(|tr| Self::track_to_queue_track(tr))
            .collect();

        // Update now-playing info
        self.state.current_track_title = Some(track.title.clone());
        self.state.current_track_artist = Some(
            track.performer.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "Unknown".to_string()),
        );
        self.state.current_track_quality = if track.hires_streamable {
            Some("Hi-Res".to_string())
        } else {
            Some(format!(
                "{}bit/{}kHz",
                track.maximum_bit_depth.unwrap_or(16),
                track.maximum_sampling_rate.unwrap_or(44.1)
            ))
        };
        self.state.status_message = Some(format!("Loading: {}...", track.title));

        let core = Arc::clone(&self.core);
        let track_id = track.id;
        let status_tx = self.playback_status_tx.clone();
        let cache = self.playback_cache.clone();

        self.rt_handle.spawn(async move {
            core.set_queue(queue_tracks, Some(0)).await;
            if let Err(e) = playback::play_qobuz_track(&core, track_id, &cache, &status_tx).await {
                log::error!("[TUI] Failed to play playlist track {}: {}", track_id, e);
                let _ = status_tx.send(PlaybackStatus::Error(e));
            }
        });
    }

    /// Add the selected playlist track to the queue.
    fn add_playlist_track_to_queue(&mut self) {
        let playlist = match &self.state.playlists.detail_playlist {
            Some(p) => p,
            None => return,
        };

        let tracks = match &playlist.tracks {
            Some(tc) => &tc.items,
            None => return,
        };

        let idx = self.state.playlists.detail_selected_index;
        let track = match tracks.get(idx) {
            Some(tr) => tr.clone(),
            None => return,
        };

        let queue_track = Self::track_to_queue_track(&track);
        let core = Arc::clone(&self.core);

        self.state.status_message = Some(format!("Added to queue: {}", track.title));

        self.rt_handle.spawn(async move {
            core.add_track(queue_track).await;
        });
    }

    // ==================== Discovery ====================

    /// Load the discover index if not already loaded (Home tab).
    fn load_discovery_if_needed(&mut self) {
        if self.state.discovery.loaded || self.state.discovery.loading {
            return;
        }

        if !self.state.authenticated {
            self.state.discovery.error = Some("Not authenticated".to_string());
            return;
        }

        self.state.discovery.loading = true;
        self.state.discovery.error = None;
        self.state.status_message = Some("Loading discovery...".to_string());

        let core = Arc::clone(&self.core);
        let event_tx = self.discover_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.get_discover_index(None).await;
            let _ = event_tx.send(result);
        });
    }

    /// Load editor picks if not already loaded.
    fn load_editor_picks_if_needed(&mut self) {
        if self.state.discovery.editor_picks_loaded || self.state.discovery.loading {
            return;
        }

        if !self.state.authenticated {
            self.state.discovery.error = Some("Not authenticated".to_string());
            return;
        }

        self.state.discovery.loading = true;
        self.state.discovery.error = None;
        self.state.status_message = Some("Loading editor's picks...".to_string());

        let core = Arc::clone(&self.core);
        let event_tx = self.editor_picks_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.get_featured_albums("editor-picks", 50, 0, None).await;
            let _ = event_tx.send(result);
        });
    }

    /// Load for-you favorite albums if not already loaded.
    fn load_for_you_if_needed(&mut self) {
        if self.state.discovery.for_you_loaded || self.state.discovery.loading {
            return;
        }

        if !self.state.authenticated {
            self.state.discovery.error = Some("Not authenticated".to_string());
            return;
        }

        self.state.discovery.loading = true;
        self.state.discovery.error = None;
        self.state.status_message = Some("Loading your favorites...".to_string());

        let core = Arc::clone(&self.core);
        let event_tx = self.for_you_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.get_favorites("albums", 50, 0).await;
            let parsed = result.and_then(|json| {
                let albums_page = json
                    .get("albums")
                    .and_then(|albums| {
                        serde_json::from_value::<SearchResultsPage<Album>>(albums.clone()).ok()
                    });
                match albums_page {
                    Some(page) => Ok(page.items),
                    None => {
                        log::warn!("[TUI] Could not parse for-you albums response");
                        Ok(Vec::new())
                    }
                }
            });
            let _ = event_tx.send(parsed);
        });
    }

    /// Handle the result of a discover index load.
    fn handle_discover_result(&mut self, result: DiscoverResult) {
        self.state.discovery.loading = false;
        self.state.discovery.loaded = true;
        match result {
            Ok(response) => {
                self.state.discovery.new_releases = response
                    .containers
                    .new_releases
                    .map(|c| c.data.items)
                    .unwrap_or_default();
                self.state.discovery.most_streamed = response
                    .containers
                    .most_streamed
                    .map(|c| c.data.items)
                    .unwrap_or_default();
                self.state.discovery.press_awards = response
                    .containers
                    .press_awards
                    .map(|c| c.data.items)
                    .unwrap_or_default();
                self.state.discovery.qobuzissimes = response
                    .containers
                    .qobuzissims
                    .map(|c| c.data.items)
                    .unwrap_or_default();
                self.state.discovery.selected_index = 0;
                self.state.discovery.error = None;

                let total = self.state.discovery.new_releases.len()
                    + self.state.discovery.most_streamed.len()
                    + self.state.discovery.press_awards.len()
                    + self.state.discovery.qobuzissimes.len();
                self.state.status_message = Some(format!("Discovery: {} albums", total));
            }
            Err(e) => {
                self.state.discovery.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load discovery: {}", e));
            }
        }
    }

    /// Handle the result of an editor picks load.
    fn handle_editor_picks_result(&mut self, result: EditorPicksResult) {
        self.state.discovery.loading = false;
        self.state.discovery.editor_picks_loaded = true;
        match result {
            Ok(page) => {
                let count = page.items.len();
                self.state.discovery.editor_picks = page.items;
                self.state.discovery.selected_index = 0;
                self.state.discovery.error = None;
                self.state.status_message = Some(format!("Editor's Picks: {} albums", count));
            }
            Err(e) => {
                self.state.discovery.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load editor's picks: {}", e));
            }
        }
    }

    /// Handle the result of a for-you load.
    fn handle_for_you_result(&mut self, result: ForYouResult) {
        self.state.discovery.loading = false;
        self.state.discovery.for_you_loaded = true;
        match result {
            Ok(albums) => {
                let count = albums.len();
                self.state.discovery.for_you_albums = albums;
                self.state.discovery.selected_index = 0;
                self.state.discovery.error = None;
                self.state.status_message = Some(format!("For You: {} albums", count));
            }
            Err(e) => {
                self.state.discovery.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load favorites: {}", e));
            }
        }
    }

    /// Cycle through discovery tabs.
    fn cycle_discovery_tab(&mut self, forward: bool) {
        let tabs = [
            DiscoveryTab::Home,
            DiscoveryTab::EditorPicks,
            DiscoveryTab::ForYou,
        ];
        let current = tabs.iter().position(|tab| *tab == self.state.discovery.tab).unwrap_or(0);
        let next = if forward {
            (current + 1) % tabs.len()
        } else {
            (current + tabs.len() - 1) % tabs.len()
        };
        self.state.discovery.tab = tabs[next];
        self.state.discovery.selected_index = 0;

        // Load data for the new tab if needed
        match self.state.discovery.tab {
            DiscoveryTab::Home => self.load_discovery_if_needed(),
            DiscoveryTab::EditorPicks => self.load_editor_picks_if_needed(),
            DiscoveryTab::ForYou => self.load_for_you_if_needed(),
        }
    }

    /// Get the total number of items in the current discovery tab.
    fn discovery_item_count(&self) -> usize {
        match self.state.discovery.tab {
            DiscoveryTab::Home => {
                self.state.discovery.new_releases.len()
                    + self.state.discovery.most_streamed.len()
                    + self.state.discovery.press_awards.len()
                    + self.state.discovery.qobuzissimes.len()
            }
            DiscoveryTab::EditorPicks => self.state.discovery.editor_picks.len(),
            DiscoveryTab::ForYou => self.state.discovery.for_you_albums.len(),
        }
    }

    /// Open the selected album from the discovery view.
    fn open_selected_discovery_album(&mut self) {
        let idx = self.state.discovery.selected_index;
        let album_id: Option<String> = match self.state.discovery.tab {
            DiscoveryTab::Home => {
                // Build flat list to find the album at idx
                let all_albums: Vec<&DiscoverAlbum> = self
                    .state
                    .discovery
                    .new_releases
                    .iter()
                    .chain(self.state.discovery.most_streamed.iter())
                    .chain(self.state.discovery.press_awards.iter())
                    .chain(self.state.discovery.qobuzissimes.iter())
                    .collect();
                all_albums.get(idx).map(|a| a.id.clone())
            }
            DiscoveryTab::EditorPicks => {
                self.state.discovery.editor_picks.get(idx).map(|a| a.id.clone())
            }
            DiscoveryTab::ForYou => {
                self.state.discovery.for_you_albums.get(idx).map(|a| a.id.clone())
            }
        };

        if let Some(id) = album_id {
            self.load_album(&id, ActiveView::Discovery);
        }
    }

    // ==================== Settings ====================

    /// Load settings from AudioSettingsStore if not already loaded.
    fn load_settings_if_needed(&mut self) {
        if self.state.settings.loaded {
            return;
        }

        match AudioSettingsStore::new() {
            Ok(store) => match store.get_settings() {
                Ok(settings) => {
                    self.state.settings.audio_settings = settings;
                    self.state.settings.loaded = true;
                    self.state.status_message = Some("Settings loaded".to_string());
                }
                Err(e) => {
                    self.state.status_message =
                        Some(format!("Failed to load settings: {}", e));
                }
            },
            Err(e) => {
                self.state.status_message =
                    Some(format!("Failed to open settings store: {}", e));
            }
        }
    }

    /// Get the total number of editable setting items.
    fn settings_item_count(&self) -> usize {
        crate::ui::settings::build_settings_list(&self.state).len()
    }

    /// Toggle the selected boolean setting.
    fn toggle_selected_setting(&mut self) {
        use crate::ui::settings::{build_settings_list, SettingKind};

        let items = build_settings_list(&self.state);
        let idx = self.state.settings.selected_index;
        let item = match items.get(idx) {
            Some(item) if item.kind == SettingKind::Toggle => item.clone(),
            _ => return,
        };

        let settings = &mut self.state.settings.audio_settings;
        let store = match AudioSettingsStore::new() {
            Ok(s) => s,
            Err(e) => {
                self.state.status_message = Some(format!("Settings store error: {}", e));
                return;
            }
        };

        let result = match item.label.as_str() {
            "Exclusive Mode" => {
                settings.exclusive_mode = !settings.exclusive_mode;
                store.set_exclusive_mode(settings.exclusive_mode)
            }
            "DAC Passthrough" => {
                settings.dac_passthrough = !settings.dac_passthrough;
                store.set_dac_passthrough(settings.dac_passthrough)
            }
            "PipeWire Force Bit-Perfect" => {
                settings.pw_force_bitperfect = !settings.pw_force_bitperfect;
                store.set_pw_force_bitperfect(settings.pw_force_bitperfect)
            }
            "ALSA Hardware Volume" => {
                settings.alsa_hardware_volume = !settings.alsa_hardware_volume;
                store.set_alsa_hardware_volume(settings.alsa_hardware_volume)
            }
            "Limit Quality to Device" => {
                settings.limit_quality_to_device = !settings.limit_quality_to_device;
                store.set_limit_quality_to_device(settings.limit_quality_to_device)
            }
            "Streaming Only" => {
                settings.streaming_only = !settings.streaming_only;
                store.set_streaming_only(settings.streaming_only)
            }
            "Stream First Track" => {
                settings.stream_first_track = !settings.stream_first_track;
                store.set_stream_first_track(settings.stream_first_track)
            }
            "Gapless Playback" => {
                settings.gapless_enabled = !settings.gapless_enabled;
                store.set_gapless_enabled(settings.gapless_enabled)
            }
            "Volume Normalization" => {
                settings.normalization_enabled = !settings.normalization_enabled;
                store.set_normalization_enabled(settings.normalization_enabled)
            }
            _ => return,
        };

        match result {
            Ok(()) => {
                let new_val = match item.label.as_str() {
                    "Exclusive Mode" => settings.exclusive_mode,
                    "DAC Passthrough" => settings.dac_passthrough,
                    "PipeWire Force Bit-Perfect" => settings.pw_force_bitperfect,
                    "ALSA Hardware Volume" => settings.alsa_hardware_volume,
                    "Limit Quality to Device" => settings.limit_quality_to_device,
                    "Streaming Only" => settings.streaming_only,
                    "Stream First Track" => settings.stream_first_track,
                    "Gapless Playback" => settings.gapless_enabled,
                    "Volume Normalization" => settings.normalization_enabled,
                    _ => false,
                };
                self.state.status_message = Some(format!(
                    "{}: {}",
                    item.label,
                    if new_val { "ON" } else { "OFF" }
                ));
            }
            Err(e) => {
                self.state.status_message = Some(format!("Failed to save: {}", e));
            }
        }
    }

    /// Adjust the selected numeric setting by a delta.
    fn adjust_selected_setting(&mut self, delta: i32) {
        use crate::ui::settings::{build_settings_list, SettingKind};

        let items = build_settings_list(&self.state);
        let idx = self.state.settings.selected_index;
        let item = match items.get(idx) {
            Some(item) if item.kind == SettingKind::Numeric => item.clone(),
            _ => return,
        };

        let settings = &mut self.state.settings.audio_settings;
        let store = match AudioSettingsStore::new() {
            Ok(s) => s,
            Err(e) => {
                self.state.status_message = Some(format!("Settings store error: {}", e));
                return;
            }
        };

        let result = match item.label.as_str() {
            "Stream Buffer" => {
                let new_val =
                    (settings.stream_buffer_seconds as i32 + delta).clamp(1, 10) as u8;
                settings.stream_buffer_seconds = new_val;
                let r = store.set_stream_buffer_seconds(new_val);
                self.state.status_message =
                    Some(format!("Stream Buffer: {} seconds", new_val));
                r
            }
            "Normalization Target" => {
                let new_val = (settings.normalization_target_lufs + delta as f32).clamp(-30.0, 0.0);
                settings.normalization_target_lufs = new_val;
                let r = store.set_normalization_target_lufs(new_val);
                self.state.status_message =
                    Some(format!("Normalization Target: {:.1} LUFS", new_val));
                r
            }
            _ => return,
        };

        if let Err(e) = result {
            self.state.status_message = Some(format!("Failed to save: {}", e));
        }
    }

    // ==================== Album ====================

    /// Navigate to album detail from a search result track.
    fn navigate_to_album_from_search(&mut self) {
        let idx = self.state.search.selected_index;
        let album_id = match self.state.search.tracks.get(idx) {
            Some(track) => track.album.as_ref().map(|a| a.id.clone()),
            None => None,
        };
        if let Some(id) = album_id {
            self.load_album(&id, ActiveView::Search);
        }
    }

    /// Navigate to album detail from a favorites track.
    fn navigate_to_album_from_favorites(&mut self) {
        let idx = self.state.favorites.selected_index;
        let album_id = match self.state.favorites.tracks.get(idx) {
            Some(track) => track.album.as_ref().map(|a| a.id.clone()),
            None => None,
        };
        if let Some(id) = album_id {
            self.load_album(&id, ActiveView::Favorites);
        }
    }

    /// Load an album by ID and switch to the album detail view.
    fn load_album(&mut self, album_id: &str, return_view: ActiveView) {
        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        self.state.album = AlbumState {
            loading: true,
            return_view,
            ..AlbumState::default()
        };
        self.state.active_view = ActiveView::Album;
        self.state.status_message = Some("Loading album...".to_string());

        // Close search modal if open
        if self.state.show_search_modal {
            self.state.show_search_modal = false;
            self.state.input_mode = InputMode::Normal;
        }

        let core = Arc::clone(&self.core);
        let event_tx = self.album_result_tx.clone();
        let id = album_id.to_string();

        self.rt_handle.spawn(async move {
            let result = core.get_album(&id).await;
            let _ = event_tx.send(result);
        });
    }

    /// Handle the result of an album detail load.
    fn handle_album_result(&mut self, result: AlbumResult) {
        self.state.album.loading = false;
        match result {
            Ok(album) => {
                let tracks = album
                    .tracks
                    .as_ref()
                    .map(|tc| tc.items.clone())
                    .unwrap_or_default();
                let count = tracks.len();
                self.state.status_message =
                    Some(format!("{} - {} tracks", album.title, count));
                self.state.album.tracks = tracks;
                self.state.album.album = Some(album);
                self.state.album.selected_index = 0;
                self.state.album.error = None;
            }
            Err(e) => {
                self.state.album.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load album: {}", e));
            }
        }
    }

    /// Play the album starting from the selected track.
    /// Queues all tracks from the selected index onward, then starts playback.
    fn play_album_from_selected(&mut self) {
        let idx = self.state.album.selected_index;
        let tracks = &self.state.album.tracks;
        if tracks.is_empty() || idx >= tracks.len() {
            return;
        }

        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        // Build queue from selected track onward
        let queue_tracks: Vec<qbz_models::QueueTrack> = tracks[idx..]
            .iter()
            .map(|track| Self::track_to_queue_track(track))
            .collect();

        let first_track_id = tracks[idx].id;
        let first_title = tracks[idx].title.clone();

        // Update now-playing info immediately
        self.state.current_track_title = Some(first_title.clone());
        self.state.current_track_artist = Some(
            tracks[idx]
                .performer
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
        );
        self.state.current_track_quality = if tracks[idx].hires_streamable {
            Some("Hi-Res".to_string())
        } else {
            Some(format!(
                "{}bit/{}kHz",
                tracks[idx].maximum_bit_depth.unwrap_or(16),
                tracks[idx].maximum_sampling_rate.unwrap_or(44.1)
            ))
        };
        self.state.status_message = Some(format!("Loading: {}...", first_title));

        let core = Arc::clone(&self.core);
        let status_tx = self.playback_status_tx.clone();
        let cache = self.playback_cache.clone();

        self.rt_handle.spawn(async move {
            // Set the queue with all remaining album tracks, starting at index 0
            core.set_queue(queue_tracks, Some(0)).await;

            // Play the first track
            if let Err(e) =
                playback::play_qobuz_track(&core, first_track_id, &cache, &status_tx).await
            {
                log::error!("[TUI] Failed to play album track {}: {}", first_track_id, e);
                let _ = status_tx.send(PlaybackStatus::Error(e));
            }
        });
    }

    /// Add the selected album track to the queue.
    fn add_album_track_to_queue(&mut self) {
        let idx = self.state.album.selected_index;
        let track = match self.state.album.tracks.get(idx) {
            Some(tr) => tr.clone(),
            None => return,
        };

        let queue_track = Self::track_to_queue_track(&track);
        let core = Arc::clone(&self.core);

        self.state.status_message = Some(format!("Added to queue: {}", track.title));

        self.rt_handle.spawn(async move {
            core.add_track(queue_track).await;
        });
    }

    /// Poll the player's shared atomic state to update the now-playing bar.
    /// The V2 player doesn't emit CoreEvents for position/playback changes,
    /// so we poll every tick (~100ms). Also detects track end for auto-advance.
    async fn poll_player_state(&mut self) {
        let ps = self.core.get_playback_state();
        let is_playing = ps.is_playing;

        // Check for auto-advance before updating state
        if let Some(next_track) = playback::check_auto_advance(
            &self.core,
            self.was_playing,
            is_playing,
            ps.position,
            ps.duration,
        )
        .await
        {
            log::info!(
                "[TUI] Auto-advancing to: {} - {}",
                next_track.artist,
                next_track.title,
            );

            // Update now-playing info from queue track
            self.state.current_track_title = Some(next_track.title.clone());
            self.state.current_track_artist = Some(next_track.artist.clone());
            self.state.current_track_quality = match (next_track.bit_depth, next_track.sample_rate)
            {
                (Some(bd), Some(sr)) => Some(format!("{}-bit / {:.1}kHz", bd, sr / 1000.0)),
                _ if next_track.hires => Some("Hi-Res".to_string()),
                _ => None,
            };

            // Play the next track through the orchestrator
            let core = Arc::clone(&self.core);
            let track_id = next_track.id;
            let status_tx = self.playback_status_tx.clone();
            let cache = self.playback_cache.clone();

            self.rt_handle.spawn(async move {
                if let Err(e) = playback::play_qobuz_track(
                    &core,
                    track_id,
                    &cache,
                    &status_tx,
                )
                .await
                {
                    log::error!("[TUI] Auto-advance failed for track {}: {}", track_id, e);
                    let _ = status_tx.send(PlaybackStatus::Error(e));
                }
            });
        }

        // Update state
        self.was_playing = is_playing;
        self.state.is_playing = is_playing;
        self.state.position_secs = ps.position;
        self.state.duration_secs = ps.duration;
        self.state.volume = ps.volume;
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
            CoreEvent::QueueUpdated { state } => {
                self.update_queue_state(state);
            }
            CoreEvent::RepeatModeChanged { mode } => {
                self.state.queue_repeat = mode;
            }
            // Auth, library, loading, audio device, search, navigation events
            // are not yet reflected in the TUI state (handled in later tasks)
            _ => {}
        }
    }

    /// Map a `QueueState` snapshot from the core into the TUI's display state.
    fn update_queue_state(&mut self, queue: QueueState) {
        self.state.queue_shuffle = queue.shuffle;
        self.state.queue_repeat = queue.repeat;
        self.state.queue_current_index = queue.current_index;

        // Build flat list: history (reversed) + current + upcoming
        // For display purposes we only care about current + upcoming so the
        // panel shows "now playing" + "up next".
        let mut tracks: Vec<QueueTrackInfo> = Vec::new();

        if let Some(ref cur) = queue.current_track {
            tracks.push(QueueTrackInfo {
                id: cur.id,
                title: cur.title.clone(),
                artist: cur.artist.clone(),
                duration_secs: cur.duration_secs,
            });
        }

        for track in &queue.upcoming {
            tracks.push(QueueTrackInfo {
                id: track.id,
                title: track.title.clone(),
                artist: track.artist.clone(),
                duration_secs: track.duration_secs,
            });
        }

        self.state.queue_tracks = tracks;
    }
}
