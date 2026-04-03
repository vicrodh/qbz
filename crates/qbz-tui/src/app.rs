use std::io::{self, stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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

use qbz_audio::{settings::AudioSettingsStore, AudioDiagnostic, AudioSettings, VisualizerTap};
use qbz_cache::PlaybackCache;
use qbz_core::QbzCore;
use qbz_models::{Album, CoreEvent, DiscoverAlbum, DiscoverPlaylist, DiscoverResponse, Playlist, QueueState, RepeatMode, SearchResultsPage, Track, UserSession};
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
    Artist,
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
            ActiveView::Artist => "Artist",
        }
    }
}

/// Which tab is active in the library view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LibraryTab {
    Albums,
    Artists,
    Tracks,
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

/// What the right panel displays when visible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RightPanelMode {
    Queue,
    Visualizer,
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

/// State for the login modal.
pub struct LoginState {
    pub email: String,
    pub email_cursor: usize,
    pub password: String,
    pub password_cursor: usize,
    /// Which field is focused (0=email, 1=password).
    pub active_field: u8,
    /// Whether a login attempt is in progress.
    pub logging_in: bool,
    /// Error message from the last login attempt.
    pub error: Option<String>,
}

impl Default for LoginState {
    fn default() -> Self {
        Self {
            email: String::new(),
            email_cursor: 0,
            password: String::new(),
            password_cursor: 0,
            active_field: 0,
            logging_in: false,
            error: None,
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
    pub editor_picks_discover: Vec<DiscoverAlbum>,
    pub essential_discography: Vec<DiscoverAlbum>,
    pub qobuz_playlists: Vec<DiscoverPlaylist>,
    // Editor's Picks tab
    pub editor_picks: Vec<Album>,
    pub editor_picks_loaded: bool,
    // For You tab
    pub for_you_albums: Vec<Album>,
    pub for_you_loaded: bool,
    pub for_you_artists: Vec<qbz_models::Artist>,
    pub for_you_artists_loaded: bool,
    pub for_you_tracks: Vec<Track>,
    pub for_you_tracks_loaded: bool,
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
            editor_picks_discover: Vec::new(),
            essential_discography: Vec::new(),
            qobuz_playlists: Vec::new(),
            editor_picks: Vec::new(),
            editor_picks_loaded: false,
            for_you_albums: Vec::new(),
            for_you_loaded: false,
            for_you_artists: Vec::new(),
            for_you_artists_loaded: false,
            for_you_tracks: Vec::new(),
            for_you_tracks_loaded: false,
            scrollbar_state: ScrollbarState::default(),
        }
    }
}

/// State for the settings view.
pub struct SettingsState {
    /// Loaded audio settings (snapshot).
    pub audio_settings: AudioSettings,
    /// Streaming quality preference (MP3/CD/Hi-Res/Hi-Res+).
    /// Stored separately from AudioSettings (same as desktop stores in localStorage).
    pub streaming_quality: String,
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
            streaming_quality: "Hi-Res".to_string(),
            loaded: false,
            selected_index: 0,
            scrollbar_state: ScrollbarState::default(),
        }
    }
}

/// State for the library view (user's full collection).
pub struct LibraryState {
    pub tab: LibraryTab,
    pub albums: Vec<Album>,
    pub artists: Vec<qbz_models::Artist>,
    pub tracks: Vec<Track>,
    pub selected_index: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub albums_loaded: bool,
    pub artists_loaded: bool,
    pub tracks_loaded: bool,
    pub scrollbar_state: ScrollbarState,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            tab: LibraryTab::Albums,
            albums: Vec::new(),
            artists: Vec::new(),
            tracks: Vec::new(),
            selected_index: 0,
            loading: false,
            error: None,
            albums_loaded: false,
            artists_loaded: false,
            tracks_loaded: false,
            scrollbar_state: ScrollbarState::default(),
        }
    }
}

/// State for the artist detail view.
pub struct ArtistState {
    pub artist: Option<qbz_models::PageArtistResponse>,
    pub selected_index: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub scrollbar_state: ScrollbarState,
    pub return_view: ActiveView,
}

impl Default for ArtistState {
    fn default() -> Self {
        Self {
            artist: None,
            selected_index: 0,
            loading: false,
            error: None,
            scrollbar_state: ScrollbarState::default(),
            return_view: ActiveView::Search,
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
    /// Whether the login modal is visible.
    pub show_login_modal: bool,
    /// Login modal state.
    pub login: LoginState,
    /// Album detail view state.
    pub album: AlbumState,
    /// Artist detail view state.
    pub artist_detail: ArtistState,
    /// Library view state.
    pub library: LibraryState,
    /// Settings view state.
    pub settings: SettingsState,
    /// Playlists view state.
    pub playlists: PlaylistsState,
    /// Discovery view state.
    pub discovery: DiscoveryState,
    /// Current track artwork URL (from QueueTrack or Track).
    pub current_artwork_url: Option<String>,
    /// Decoded cover art image for the player bar (ratatui-image protocol).
    pub cover_art: Option<ratatui_image::protocol::StatefulProtocol>,
    /// Whether images are disabled (--no-images CLI flag).
    pub no_images: bool,
    /// Whether a track is currently being buffered/downloaded.
    pub is_buffering: bool,
    /// Human-readable buffering status (e.g., "Downloading...", "Playing from cache...")
    pub buffering_status: Option<String>,
    /// Dynamic accent color extracted from current cover art.
    pub dynamic_accent: Option<ratatui::style::Color>,
    /// What the right panel displays (queue or visualizer).
    pub right_panel_mode: RightPanelMode,
    /// Current frequency bar heights (0.0 to 1.0) for the visualizer.
    pub visualizer_bars: Vec<f32>,
    /// Whether the device picker modal is visible.
    pub show_device_picker: bool,
    /// Available audio devices for the current backend.
    pub available_devices: Vec<qbz_audio::AudioDevice>,
    /// Selected index in the device picker.
    pub device_picker_index: usize,
    /// Whether devices are being enumerated.
    pub devices_loading: bool,
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
            show_login_modal: false,
            login: LoginState::default(),
            album: AlbumState::default(),
            artist_detail: ArtistState::default(),
            library: LibraryState::default(),
            settings: SettingsState::default(),
            playlists: PlaylistsState::default(),
            discovery: DiscoveryState::default(),
            current_artwork_url: None,
            cover_art: None,
            no_images: false,
            is_buffering: false,
            buffering_status: None,
            dynamic_accent: None,
            right_panel_mode: RightPanelMode::Queue,
            visualizer_bars: Vec::new(),
            show_device_picker: false,
            available_devices: Vec::new(),
            device_picker_index: 0,
            devices_loading: false,
        }
    }
}

/// Type alias for the login result payload.
type LoginResult = Result<UserSession, qbz_core::error::CoreError>;

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

/// Type alias for the for-you artists result payload.
type ForYouArtistsResult = Result<Vec<qbz_models::Artist>, qbz_core::error::CoreError>;

/// Type alias for the for-you tracks (continue listening) result payload.
type ForYouTracksResult = Result<Vec<Track>, qbz_core::error::CoreError>;

/// Type alias for the library albums result payload.
type LibraryAlbumsResult = Result<Vec<Album>, qbz_core::error::CoreError>;

/// Type alias for the library artists result payload.
type LibraryArtistsResult = Result<Vec<qbz_models::Artist>, qbz_core::error::CoreError>;

/// Type alias for the library tracks result payload.
type LibraryTracksResult = Result<Vec<Track>, qbz_core::error::CoreError>;

/// Type alias for the artist page result payload.
type ArtistPageResult = Result<qbz_models::PageArtistResponse, qbz_core::error::CoreError>;

pub struct App {
    pub state: AppState,
    core_event_rx: mpsc::UnboundedReceiver<CoreEvent>,
    core: Arc<QbzCore<TuiAdapter>>,
    should_quit: bool,
    pub no_images: bool,
    rt_handle: tokio::runtime::Handle,
    /// Visualizer tap for reading audio samples from the player.
    visualizer_tap: Option<VisualizerTap>,
    /// Playback generation counter — incremented each time a new track is requested.
    /// Spawned download tasks compare their captured generation to skip stale play_data calls.
    playback_generation: Arc<AtomicU64>,
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
    /// Sender for for-you artists results.
    for_you_artists_result_tx: mpsc::UnboundedSender<ForYouArtistsResult>,
    /// Receiver for for-you artists results.
    for_you_artists_result_rx: mpsc::UnboundedReceiver<ForYouArtistsResult>,
    /// Sender for for-you tracks (continue listening) results.
    for_you_tracks_result_tx: mpsc::UnboundedSender<ForYouTracksResult>,
    /// Receiver for for-you tracks (continue listening) results.
    for_you_tracks_result_rx: mpsc::UnboundedReceiver<ForYouTracksResult>,
    /// Sender for library albums results.
    library_albums_result_tx: mpsc::UnboundedSender<LibraryAlbumsResult>,
    /// Receiver for library albums results.
    library_albums_result_rx: mpsc::UnboundedReceiver<LibraryAlbumsResult>,
    /// Sender for library artists results.
    library_artists_result_tx: mpsc::UnboundedSender<LibraryArtistsResult>,
    /// Receiver for library artists results.
    library_artists_result_rx: mpsc::UnboundedReceiver<LibraryArtistsResult>,
    /// Sender for library tracks results.
    library_tracks_result_tx: mpsc::UnboundedSender<LibraryTracksResult>,
    /// Receiver for library tracks results.
    library_tracks_result_rx: mpsc::UnboundedReceiver<LibraryTracksResult>,
    /// Sender for artist page results.
    artist_page_result_tx: mpsc::UnboundedSender<ArtistPageResult>,
    /// Receiver for artist page results.
    artist_page_result_rx: mpsc::UnboundedReceiver<ArtistPageResult>,
    /// Sender for login results (cloned into async tasks).
    login_result_tx: mpsc::UnboundedSender<LoginResult>,
    /// Receiver for login results (drained each tick).
    login_result_rx: mpsc::UnboundedReceiver<LoginResult>,
    /// Layout areas from the last render, used for mouse hit-testing.
    layout_areas: LayoutAreas,
    /// Sender for playback status updates (cloned into async tasks).
    playback_status_tx: mpsc::UnboundedSender<PlaybackStatus>,
    /// Receiver for playback status updates (drained each tick).
    playback_status_rx: mpsc::UnboundedReceiver<PlaybackStatus>,
    /// Whether playback was active on the previous tick (for auto-advance detection).
    was_playing: bool,
    /// Track ID from the previous tick (for gapless transition detection).
    last_track_id: u64,
    /// L2 disk cache for playback audio data.
    playback_cache: Option<Arc<PlaybackCache>>,
    /// Sender for cover art images (cloned into async download tasks).
    cover_art_tx: mpsc::UnboundedSender<Option<image::DynamicImage>>,
    /// Receiver for cover art images (drained each tick).
    cover_art_rx: mpsc::UnboundedReceiver<Option<image::DynamicImage>>,
    /// Image protocol picker (detects terminal capabilities once).
    picker: ratatui_image::picker::Picker,
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
        let visualizer_tap = VisualizerTap::new();
        let player = Player::new(device_name, audio_settings, Some(visualizer_tap.clone()), diagnostic);
        let core = QbzCore::new(adapter, player);

        // Initialize core (extracts Qobuz bundle tokens)
        let mut core_initialized = false;
        if let Err(err) = core.init().await {
            log::warn!("[TUI] Core init failed (offline mode): {}", err);
        } else {
            core_initialized = true;
        }

        let mut state = AppState::default();
        state.no_images = no_images;

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
        let (for_you_artists_tx, for_you_artists_rx) = mpsc::unbounded_channel::<ForYouArtistsResult>();
        let (for_you_tracks_tx, for_you_tracks_rx) = mpsc::unbounded_channel::<ForYouTracksResult>();
        let (lib_albums_tx, lib_albums_rx) = mpsc::unbounded_channel::<LibraryAlbumsResult>();
        let (lib_artists_tx, lib_artists_rx) = mpsc::unbounded_channel::<LibraryArtistsResult>();
        let (lib_tracks_tx, lib_tracks_rx) = mpsc::unbounded_channel::<LibraryTracksResult>();
        let (artist_page_tx, artist_page_rx) = mpsc::unbounded_channel::<ArtistPageResult>();
        let (login_tx, login_rx) = mpsc::unbounded_channel::<LoginResult>();
        let (cover_art_tx, cover_art_rx) = mpsc::unbounded_channel::<Option<image::DynamicImage>>();

        // Create image picker — use Halfblocks as safe default that works everywhere.
        // Picker::from_query_stdio() requires non-raw-mode terminal, and we haven't
        // entered raw mode yet, but Halfblocks is universally supported.
        let picker = ratatui_image::picker::Picker::from_query_stdio()
            .unwrap_or_else(|_| {
                log::info!("[TUI] Terminal image query failed, falling back to halfblocks");
                ratatui_image::picker::Picker::from_fontsize((8, 16))
            });

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
            for_you_artists_result_tx: for_you_artists_tx,
            for_you_artists_result_rx: for_you_artists_rx,
            for_you_tracks_result_tx: for_you_tracks_tx,
            for_you_tracks_result_rx: for_you_tracks_rx,
            library_albums_result_tx: lib_albums_tx,
            library_albums_result_rx: lib_albums_rx,
            library_artists_result_tx: lib_artists_tx,
            library_artists_result_rx: lib_artists_rx,
            library_tracks_result_tx: lib_tracks_tx,
            library_tracks_result_rx: lib_tracks_rx,
            artist_page_result_tx: artist_page_tx,
            artist_page_result_rx: artist_page_rx,
            login_result_tx: login_tx,
            login_result_rx: login_rx,
            layout_areas: LayoutAreas::default(),
            playback_status_tx: playback_tx,
            playback_status_rx: playback_rx,
            was_playing: false,
            last_track_id: 0,
            playback_cache,
            cover_art_tx,
            cover_art_rx,
            picker,
            visualizer_tap: Some(visualizer_tap),
            playback_generation: Arc::new(AtomicU64::new(0)),
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

        // Auto-load Discovery data on startup (default view)
        self.load_discovery_if_needed();

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

            // Update visualizer bars from audio tap (every tick)
            self.update_visualizer();

            // Drain playback status updates
            while let Ok(status) = self.playback_status_rx.try_recv() {
                match status {
                    PlaybackStatus::Buffering(msg) => {
                        self.state.is_buffering = true;
                        self.state.buffering_status = Some(msg.clone());
                        self.state.status_message = Some(msg);
                    }
                    PlaybackStatus::Playing => {
                        self.state.is_buffering = false;
                        self.state.buffering_status = None;
                        self.state.status_message = None;
                    }
                    PlaybackStatus::Error(msg) => {
                        self.state.is_buffering = false;
                        self.state.buffering_status = None;
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

            // Drain for-you artists results
            while let Ok(result) = self.for_you_artists_result_rx.try_recv() {
                self.handle_for_you_artists_result(result);
            }

            // Drain for-you tracks results
            while let Ok(result) = self.for_you_tracks_result_rx.try_recv() {
                self.handle_for_you_tracks_result(result);
            }

            // Drain library albums results
            while let Ok(result) = self.library_albums_result_rx.try_recv() {
                self.handle_library_albums_result(result);
            }

            // Drain library artists results
            while let Ok(result) = self.library_artists_result_rx.try_recv() {
                self.handle_library_artists_result(result);
            }

            // Drain library tracks results
            while let Ok(result) = self.library_tracks_result_rx.try_recv() {
                self.handle_library_tracks_result(result);
            }

            // Drain artist page results
            while let Ok(result) = self.artist_page_result_rx.try_recv() {
                self.handle_artist_page_result(result);
            }

            // Drain login results
            while let Ok(result) = self.login_result_rx.try_recv() {
                self.handle_login_result(result);
            }

            // Drain cover art results
            while let Ok(img_opt) = self.cover_art_rx.try_recv() {
                match img_opt {
                    Some(img) => {
                        self.state.dynamic_accent = Some(extract_dominant_color(&img));
                        let protocol = self.picker.new_resize_protocol(img);
                        self.state.cover_art = Some(protocol);
                    }
                    None => {
                        self.state.dynamic_accent = None;
                        self.state.cover_art = None;
                    }
                }
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
        if self.state.show_device_picker {
            self.handle_key_device_picker(key);
            return;
        }
        if self.state.show_login_modal {
            self.handle_key_login(key);
            return;
        }
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
            } else if view == ActiveView::Library {
                self.load_library_for_active_tab();
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
            ActiveView::Library => {
                let len = self.library_active_list_len();
                if len > 0 {
                    let current = self.state.library.selected_index as i32;
                    let next = (current + delta).clamp(0, (len as i32) - 1) as usize;
                    self.state.library.selected_index = next;
                }
            }
            ActiveView::Artist => {
                let len = self.artist_top_tracks_len();
                if len > 0 {
                    let current = self.state.artist_detail.selected_index as i32;
                    let next = (current + delta).clamp(0, (len as i32) - 1) as usize;
                    self.state.artist_detail.selected_index = next;
                }
            }
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
            // 'q' cycles right panel: Hidden -> Queue -> Visualizer -> Hidden
            KeyCode::Char('q') => {
                if !self.state.show_queue_panel {
                    // Hidden -> Queue
                    self.state.show_queue_panel = true;
                    self.state.right_panel_mode = RightPanelMode::Queue;
                    // Disable visualizer tap when showing queue
                    if let Some(ref tap) = self.visualizer_tap {
                        tap.set_enabled(false);
                    }
                } else if self.state.right_panel_mode == RightPanelMode::Queue {
                    // Queue -> Visualizer
                    self.state.right_panel_mode = RightPanelMode::Visualizer;
                    // Enable visualizer tap for audio capture
                    if let Some(ref tap) = self.visualizer_tap {
                        tap.set_enabled(true);
                    }
                } else {
                    // Visualizer -> Hidden
                    self.state.show_queue_panel = false;
                    self.state.right_panel_mode = RightPanelMode::Queue;
                    // Disable visualizer tap when hiding
                    if let Some(ref tap) = self.visualizer_tap {
                        tap.set_enabled(false);
                    }
                }
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
            KeyCode::Char('3') => {
                self.state.active_view = ActiveView::Library;
                self.load_library_for_active_tab();
            }
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

            // 'l' opens the login modal when not authenticated
            KeyCode::Char('l') if !self.state.authenticated => {
                self.state.show_login_modal = true;
                self.state.input_mode = InputMode::TextInput;
            }

            // 'i' in search view enters text input mode (legacy, also opens modal)
            KeyCode::Char('i') if self.state.active_view == ActiveView::Search => {
                self.state.show_search_modal = true;
                self.state.input_mode = InputMode::TextInput;
            }

            // Search modal open (normal mode): j/k for navigating results
            KeyCode::Char('j') | KeyCode::Down if self.state.show_search_modal => {
                let len = self.search_active_list_len();
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

            // Search modal open: Tab/Shift+Tab to switch tabs
            KeyCode::Tab if self.state.show_search_modal => {
                self.cycle_search_tab(true);
            }
            KeyCode::BackTab if self.state.show_search_modal => {
                self.cycle_search_tab(false);
            }

            // Search modal open: Enter to play/open selected item
            KeyCode::Enter if self.state.show_search_modal => {
                match self.state.search.tab {
                    SearchTab::Tracks => self.play_selected_track(),
                    SearchTab::Albums => self.open_selected_search_album(),
                    SearchTab::Artists => {} // TODO: artist detail
                }
            }

            // Search modal open: Esc closes the modal
            KeyCode::Esc if self.state.show_search_modal => {
                self.state.show_search_modal = false;
            }

            // Search modal open: 'a' adds track to queue
            KeyCode::Char('a') if self.state.show_search_modal => {
                self.add_selected_to_queue();
            }

            // Search view (non-modal): Tab/Shift+Tab to switch tabs
            KeyCode::Tab if self.state.active_view == ActiveView::Search => {
                self.cycle_search_tab(true);
            }
            KeyCode::BackTab if self.state.active_view == ActiveView::Search => {
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
                    SearchTab::Artists => self.open_selected_search_artist(),
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
                    FavoritesTab::Artists => self.open_selected_favorite_artist(),
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

            // Library view: Tab/Shift+Tab to switch tabs
            KeyCode::Tab if self.state.active_view == ActiveView::Library => {
                self.cycle_library_tab(true);
            }
            KeyCode::BackTab if self.state.active_view == ActiveView::Library => {
                self.cycle_library_tab(false);
            }

            // Library view: j/k for navigating items
            KeyCode::Char('j') | KeyCode::Down if self.state.active_view == ActiveView::Library => {
                let len = self.library_active_list_len();
                if len > 0 {
                    self.state.library.selected_index =
                        (self.state.library.selected_index + 1).min(len - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up if self.state.active_view == ActiveView::Library => {
                if self.state.library.selected_index > 0 {
                    self.state.library.selected_index -= 1;
                }
            }

            // Library view: Enter to play/open selected item
            KeyCode::Enter if self.state.active_view == ActiveView::Library => {
                match self.state.library.tab {
                    LibraryTab::Tracks => self.play_selected_library_track(),
                    LibraryTab::Albums => self.open_selected_library_album(),
                    LibraryTab::Artists => self.open_selected_library_artist(),
                }
            }

            // Library view: 'g' to go to album detail from tracks tab
            KeyCode::Char('g') if self.state.active_view == ActiveView::Library => {
                self.navigate_to_album_from_library();
            }

            // Library view: 'a' to add track to queue
            KeyCode::Char('a') if self.state.active_view == ActiveView::Library => {
                self.add_selected_library_track_to_queue();
            }

            // Artist detail view: j/k navigation
            KeyCode::Char('j') | KeyCode::Down if self.state.active_view == ActiveView::Artist => {
                let len = self.artist_top_tracks_len();
                if len > 0 {
                    self.state.artist_detail.selected_index =
                        (self.state.artist_detail.selected_index + 1).min(len - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up if self.state.active_view == ActiveView::Artist => {
                if self.state.artist_detail.selected_index > 0 {
                    self.state.artist_detail.selected_index -= 1;
                }
            }

            // Artist detail view: Enter to play top track
            KeyCode::Enter if self.state.active_view == ActiveView::Artist => {
                self.play_selected_artist_track();
            }

            // Artist detail view: 'g' to go to album from top track
            KeyCode::Char('g') if self.state.active_view == ActiveView::Artist => {
                self.navigate_to_album_from_artist();
            }

            // Artist detail view: Backspace/Esc returns to previous view
            KeyCode::Backspace | KeyCode::Esc if self.state.active_view == ActiveView::Artist => {
                self.state.active_view = self.state.artist_detail.return_view;
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

            // Shuffle toggle (global)
            KeyCode::Char('s') => {
                let core = Arc::clone(&self.core);
                self.rt_handle.spawn(async move {
                    core.toggle_shuffle().await;
                });
                self.state.queue_shuffle = !self.state.queue_shuffle;
                self.state.status_message = Some(format!(
                    "Shuffle: {}",
                    if self.state.queue_shuffle { "ON" } else { "OFF" }
                ));
            }

            // Repeat mode cycle (global, except Settings where 'r' = reload)
            KeyCode::Char('r') if self.state.active_view != ActiveView::Settings => {
                let next_mode = match self.state.queue_repeat {
                    RepeatMode::Off => RepeatMode::One,
                    RepeatMode::One => RepeatMode::All,
                    RepeatMode::All => RepeatMode::Off,
                };
                let core = Arc::clone(&self.core);
                let mode = next_mode;
                self.rt_handle.spawn(async move {
                    core.set_repeat_mode(mode).await;
                });
                self.state.queue_repeat = next_mode;
                self.state.status_message = Some(format!("Repeat: {:?}", next_mode));
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

            // Seek: left/right arrows (5 seconds per press)
            KeyCode::Left if self.state.is_playing && !self.state.show_search_modal => {
                let new_pos = self.state.position_secs.saturating_sub(5);
                let _ = self.core.player().seek(new_pos);
                self.state.position_secs = new_pos;
            }
            KeyCode::Right if self.state.is_playing && !self.state.show_search_modal => {
                let new_pos = (self.state.position_secs + 5).min(self.state.duration_secs);
                let _ = self.core.player().seek(new_pos);
                self.state.position_secs = new_pos;
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
            // Tab switches search tab even while typing
            KeyCode::Tab if self.state.show_search_modal => {
                self.cycle_search_tab(true);
            }
            KeyCode::BackTab if self.state.show_search_modal => {
                self.cycle_search_tab(false);
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

    /// Handle keys when the login modal is visible.
    fn handle_key_login(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.state.show_login_modal = false;
                self.state.input_mode = InputMode::Normal;
                self.state.login.error = None;
            }
            KeyCode::Tab => {
                // Toggle between email (0) and password (1)
                self.state.login.active_field =
                    if self.state.login.active_field == 0 { 1 } else { 0 };
            }
            KeyCode::Enter => {
                self.attempt_login();
            }
            KeyCode::Backspace => {
                if self.state.login.active_field == 0 {
                    let login = &mut self.state.login;
                    if login.email_cursor > 0 {
                        let prev = login.email[..login.email_cursor]
                            .char_indices()
                            .next_back()
                            .map(|(idx, _)| idx)
                            .unwrap_or(0);
                        login.email.remove(prev);
                        login.email_cursor = prev;
                    }
                } else {
                    let login = &mut self.state.login;
                    if login.password_cursor > 0 {
                        let prev = login.password[..login.password_cursor]
                            .char_indices()
                            .next_back()
                            .map(|(idx, _)| idx)
                            .unwrap_or(0);
                        login.password.remove(prev);
                        login.password_cursor = prev;
                    }
                }
            }
            KeyCode::Char(c) => {
                if self.state.login.active_field == 0 {
                    self.state.login.email.insert(self.state.login.email_cursor, c);
                    self.state.login.email_cursor += c.len_utf8();
                } else {
                    self.state
                        .login
                        .password
                        .insert(self.state.login.password_cursor, c);
                    self.state.login.password_cursor += c.len_utf8();
                }
            }
            _ => {}
        }
    }

    /// Attempt to log in with the credentials entered in the login modal.
    fn attempt_login(&mut self) {
        let email = self.state.login.email.trim().to_string();
        let password = self.state.login.password.clone();

        if email.is_empty() || password.is_empty() {
            self.state.login.error = Some("Email and password required".to_string());
            return;
        }

        self.state.login.logging_in = true;
        self.state.login.error = None;

        let core = Arc::clone(&self.core);
        let event_tx = self.login_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.login(&email, &password).await;
            let _ = event_tx.send(result);
        });
    }

    /// Process the result of a login attempt.
    fn handle_login_result(&mut self, result: LoginResult) {
        self.state.login.logging_in = false;
        match result {
            Ok(session) => {
                self.state.authenticated = true;
                self.state.auth_email = Some(session.email.clone());
                self.state.show_login_modal = false;
                self.state.input_mode = InputMode::Normal;
                self.state.status_message =
                    Some(format!("Logged in ({})", session.subscription_label));
                // Clear login form
                self.state.login = LoginState::default();
            }
            Err(e) => {
                self.state.login.error = Some(format!("Login failed: {}", e));
            }
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

    /// Play the currently selected track from search results, queuing all results.
    fn play_selected_track(&mut self) {
        let idx = self.state.search.selected_index;
        if idx >= self.state.search.tracks.len() {
            return;
        }

        // Build queue from ALL search results starting at the selected index
        let all_tracks: Vec<qbz_models::QueueTrack> = self.state.search.tracks
            .iter()
            .map(|tr| Self::track_to_queue_track(tr))
            .collect();

        let track = self.state.search.tracks[idx].clone();
        self.play_track_with_queue(track, all_tracks, idx);
    }

    /// Play a track with a full queue (e.g., all search results).
    fn play_track_with_queue(&mut self, track: Track, queue: Vec<qbz_models::QueueTrack>, queue_index: usize) {
        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        let _ = self.core.player().stop();
        let generation = self.playback_generation.fetch_add(1, Ordering::SeqCst) + 1;

        self.state.current_track_title = Some(track.title.clone());
        self.state.current_track_artist = Some(track.performer.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "Unknown".to_string()));
        self.state.current_track_quality = if track.hires_streamable {
            Some("Hi-Res".to_string())
        } else {
            Some(format!("{}bit/{}kHz", track.maximum_bit_depth.unwrap_or(16), track.maximum_sampling_rate.unwrap_or(44.1)))
        };
        self.state.status_message = Some(format!("Loading: {}...", track.title));
        self.state.is_buffering = true;

        self.update_artwork_from_track(&track);

        let core = Arc::clone(&self.core);
        let track_id = track.id;
        let status_tx = self.playback_status_tx.clone();
        let cache = self.playback_cache.clone();
        let gen = Arc::clone(&self.playback_generation);

        self.rt_handle.spawn(async move {
            core.set_queue(queue, Some(queue_index)).await;

            if gen.load(Ordering::SeqCst) != generation {
                log::info!("[TUI] Playback generation changed, skipping stale track {}", track_id);
                return;
            }

            if let Err(e) = playback::play_qobuz_track(&core, track_id, &cache, &status_tx).await {
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
        if idx >= self.state.favorites.tracks.len() {
            return;
        }

        let all_tracks: Vec<qbz_models::QueueTrack> = self.state.favorites.tracks
            .iter()
            .map(|tr| Self::track_to_queue_track(tr))
            .collect();

        let track = self.state.favorites.tracks[idx].clone();
        self.play_track_with_queue(track, all_tracks, idx);
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

        // Trigger cover art download
        self.update_artwork_from_track(&track);

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

    /// Load for-you favorite albums and artists if not already loaded.
    fn load_for_you_if_needed(&mut self) {
        if !self.state.authenticated {
            self.state.discovery.error = Some("Not authenticated".to_string());
            return;
        }

        // Load albums
        if !self.state.discovery.for_you_loaded && !self.state.discovery.loading {
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

        // Load artists
        if !self.state.discovery.for_you_artists_loaded {
            let core = Arc::clone(&self.core);
            let event_tx = self.for_you_artists_result_tx.clone();

            self.rt_handle.spawn(async move {
                let result = core.get_favorites("artists", 10, 0).await;
                let parsed = result.and_then(|json| {
                    let artists_page = json
                        .get("artists")
                        .and_then(|artists| {
                            serde_json::from_value::<qbz_models::SearchResultsPage<qbz_models::Artist>>(artists.clone()).ok()
                        });
                    match artists_page {
                        Some(page) => Ok(page.items),
                        None => {
                            log::warn!("[TUI] Could not parse for-you artists response");
                            Ok(Vec::new())
                        }
                    }
                });
                let _ = event_tx.send(parsed);
            });
        }

        // Load tracks (Continue Listening)
        if !self.state.discovery.for_you_tracks_loaded {
            let core = Arc::clone(&self.core);
            let event_tx = self.for_you_tracks_result_tx.clone();

            self.rt_handle.spawn(async move {
                let result = core.get_favorites("tracks", 10, 0).await;
                let parsed = result.and_then(|json| {
                    let tracks_page = json
                        .get("tracks")
                        .and_then(|tracks| {
                            serde_json::from_value::<qbz_models::SearchResultsPage<Track>>(tracks.clone()).ok()
                        });
                    match tracks_page {
                        Some(page) => Ok(page.items),
                        None => {
                            log::warn!("[TUI] Could not parse for-you tracks response");
                            Ok(Vec::new())
                        }
                    }
                });
                let _ = event_tx.send(parsed);
            });
        }
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
                self.state.discovery.editor_picks_discover = response
                    .containers
                    .album_of_the_week
                    .map(|c| c.data.items)
                    .unwrap_or_default();
                self.state.discovery.essential_discography = response
                    .containers
                    .ideal_discography
                    .map(|c| c.data.items)
                    .unwrap_or_default();
                self.state.discovery.qobuz_playlists = response
                    .containers
                    .playlists
                    .map(|c| c.data.items)
                    .unwrap_or_default();
                self.state.discovery.selected_index = 0;
                self.state.discovery.error = None;

                let total = self.state.discovery.new_releases.len()
                    + self.state.discovery.essential_discography.len()
                    + self.state.discovery.editor_picks_discover.len()
                    + self.state.discovery.press_awards.len()
                    + self.state.discovery.most_streamed.len()
                    + self.state.discovery.qobuzissimes.len()
                    + self.state.discovery.qobuz_playlists.len();
                self.state.status_message = Some(format!("Discovery: {} items", total));
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

    /// Handle the result of a for-you artists load.
    fn handle_for_you_artists_result(&mut self, result: ForYouArtistsResult) {
        self.state.discovery.for_you_artists_loaded = true;
        match result {
            Ok(artists) => {
                self.state.discovery.for_you_artists = artists;
            }
            Err(e) => {
                log::warn!("[TUI] Failed to load for-you artists: {}", e);
            }
        }
    }

    /// Handle the result of a for-you tracks (continue listening) load.
    fn handle_for_you_tracks_result(&mut self, result: ForYouTracksResult) {
        self.state.discovery.for_you_tracks_loaded = true;
        match result {
            Ok(tracks) => {
                self.state.discovery.for_you_tracks = tracks;
            }
            Err(e) => {
                log::warn!("[TUI] Failed to load for-you tracks: {}", e);
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
                    + self.state.discovery.essential_discography.len()
                    + self.state.discovery.editor_picks_discover.len()
                    + self.state.discovery.press_awards.len()
                    + self.state.discovery.most_streamed.len()
                    + self.state.discovery.qobuzissimes.len()
                    + self.state.discovery.qobuz_playlists.len()
            }
            DiscoveryTab::EditorPicks => self.state.discovery.editor_picks.len(),
            DiscoveryTab::ForYou => {
                4 // Your Mixes (static items)
                    + self.state.discovery.for_you_tracks.len().min(10)
                    + self.state.discovery.for_you_albums.len().min(8)
                    + self.state.discovery.for_you_artists.len().min(8)
            }
        }
    }

    /// Open the selected album from the discovery view.
    fn open_selected_discovery_album(&mut self) {
        let idx = self.state.discovery.selected_index;
        let album_id: Option<String> = match self.state.discovery.tab {
            DiscoveryTab::Home => {
                // Build flat list of all albums (same order as render_home sections).
                // Playlists are not albums, so they are skipped here.
                let disc = &self.state.discovery;
                let all_albums: Vec<&DiscoverAlbum> = disc
                    .new_releases
                    .iter()
                    .chain(disc.essential_discography.iter())
                    .chain(disc.editor_picks_discover.iter())
                    .chain(disc.press_awards.iter())
                    .chain(disc.most_streamed.iter())
                    .chain(disc.qobuzissimes.iter())
                    .collect();
                let playlist_count = disc.qobuz_playlists.len();
                // If idx falls within the album range, return the album id.
                // If it falls in the playlist range (after all albums), return None
                // because playlists can't be opened as albums.
                if idx < all_albums.len() {
                    all_albums.get(idx).map(|a| a.id.clone())
                } else {
                    // idx is in the playlist range — no album to open
                    let _ = playlist_count; // suppress unused warning
                    None
                }
            }
            DiscoveryTab::EditorPicks => {
                self.state.discovery.editor_picks.get(idx).map(|a| a.id.clone())
            }
            DiscoveryTab::ForYou => {
                // For You layout: 4 mixes, then tracks, then albums, then artists.
                // Only albums are openable as album detail.
                let mixes_count = 4;
                let tracks_count = self.state.discovery.for_you_tracks.len().min(10);
                let albums_start = mixes_count + tracks_count;
                let albums_count = self.state.discovery.for_you_albums.len().min(8);
                if idx >= albums_start && idx < albums_start + albums_count {
                    self.state.discovery.for_you_albums.get(idx - albums_start).map(|a| a.id.clone())
                } else {
                    None
                }
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

                    // Load streaming quality from file (TUI-specific, desktop uses localStorage)
                    let quality_path = dirs::data_dir()
                        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                        .join("qbz")
                        .join("tui_streaming_quality");
                    if let Ok(saved) = std::fs::read_to_string(&quality_path) {
                        let trimmed = saved.trim();
                        if ["MP3", "CD", "Hi-Res", "Hi-Res+"].contains(&trimmed) {
                            self.state.settings.streaming_quality = trimmed.to_string();
                        }
                    }

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
            Some(item) if item.kind == SettingKind::Toggle || item.kind == SettingKind::Cycle => item.clone(),
            _ => return,
        };

        // Handle Cycle items separately
        if item.kind == SettingKind::Cycle {
            self.cycle_selected_setting(&item);
            return;
        }

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
            "PW Force Bit-Perfect" => {
                settings.pw_force_bitperfect = !settings.pw_force_bitperfect;
                store.set_pw_force_bitperfect(settings.pw_force_bitperfect)
            }
            "Hardware Volume" => {
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
            "Stream Uncached" => {
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
                    "PW Force Bit-Perfect" => settings.pw_force_bitperfect,
                    "Hardware Volume" => settings.alsa_hardware_volume,
                    "Limit Quality to Device" => settings.limit_quality_to_device,
                    "Streaming Only" => settings.streaming_only,
                    "Stream Uncached" => settings.stream_first_track,
                    "Gapless Playback" => settings.gapless_enabled,
                    "Volume Normalization" => settings.normalization_enabled,
                    _ => false,
                };

                // Push updated settings to the running Player so changes take
                // effect without restarting the app.
                let player = self.core.player();
                if let Err(e) = player.reload_settings(self.state.settings.audio_settings.clone()) {
                    log::warn!("Failed to push settings to player: {}", e);
                }

                // Audio-stream settings only take effect on the next track play
                // because the audio thread reads them when creating a new stream.
                let next_track_hint = matches!(
                    item.label.as_str(),
                    "Exclusive Mode"
                        | "DAC Passthrough"
                        | "PW Force Bit-Perfect"
                        | "Hardware Volume"
                );
                let suffix = if next_track_hint {
                    " (applies on next track)"
                } else {
                    ""
                };

                self.state.status_message = Some(format!(
                    "{}: {}{}",
                    item.label,
                    if new_val { "ON" } else { "OFF" },
                    suffix,
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
            "Initial Buffer" => {
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
            "Preferred Sample Rate" => {
                let rates: &[Option<u32>] = &[
                    None, Some(44100), Some(48000), Some(88200),
                    Some(96000), Some(176400), Some(192000), Some(384000),
                ];
                let current_idx = rates.iter().position(|r| *r == settings.preferred_sample_rate).unwrap_or(0);
                let next_idx = if delta > 0 {
                    (current_idx + 1) % rates.len()
                } else {
                    (current_idx + rates.len() - 1) % rates.len()
                };
                let new_val = rates[next_idx];
                settings.preferred_sample_rate = new_val;
                let r = store.set_sample_rate(new_val);
                let label = new_val.map(|r| format!("{} Hz", r)).unwrap_or_else(|| "Auto".into());
                self.state.status_message = Some(format!("Preferred Sample Rate: {}", label));
                r
            }
            "Device Max Sample Rate" => {
                // Same options as desktop: No limit, 44.1kHz, 48kHz, 96kHz, 192kHz, 384kHz
                let rates: &[Option<u32>] = &[
                    None, Some(44100), Some(48000), Some(96000),
                    Some(192000), Some(384000),
                ];
                let current_idx = rates.iter().position(|r| *r == settings.device_max_sample_rate).unwrap_or(0);
                let next_idx = if delta > 0 {
                    (current_idx + 1) % rates.len()
                } else {
                    (current_idx + rates.len() - 1) % rates.len()
                };
                let new_val = rates[next_idx];
                settings.device_max_sample_rate = new_val;
                let r = store.set_device_max_sample_rate(new_val);
                let label = match new_val {
                    Some(44100) => "44.1 kHz (CD)",
                    Some(48000) => "48 kHz (DVD)",
                    Some(96000) => "96 kHz (Hi-Res)",
                    Some(192000) => "192 kHz (Hi-Res+)",
                    Some(384000) => "384 kHz (DSD)",
                    _ => "No limit",
                };
                self.state.status_message = Some(format!("Device Max Sample Rate: {}", label));
                r
            }
            _ => return,
        };

        match result {
            Ok(()) => {
                // Push updated settings to the running Player.
                let player = self.core.player();
                if let Err(e) = player.reload_settings(self.state.settings.audio_settings.clone()) {
                    log::warn!("Failed to push settings to player: {}", e);
                }
            }
            Err(e) => {
                self.state.status_message = Some(format!("Failed to save: {}", e));
            }
        }
    }

    /// Cycle a setting through its available options (e.g. Backend type).
    fn cycle_selected_setting(&mut self, item: &crate::ui::settings::SettingItem) {
        use qbz_audio::AudioBackendType;

        let settings = &mut self.state.settings.audio_settings;
        let store = match AudioSettingsStore::new() {
            Ok(s) => s,
            Err(e) => {
                self.state.status_message = Some(format!("Settings store error: {}", e));
                return;
            }
        };

        match item.label.as_str() {
            "Output Device" => {
                self.open_device_picker();
                return;
            }
            "Streaming Quality" => {
                // Cycle: MP3 → CD → Hi-Res → Hi-Res+ → MP3
                let next = match self.state.settings.streaming_quality.as_str() {
                    "MP3" => "CD",
                    "CD" => "Hi-Res",
                    "Hi-Res" => "Hi-Res+",
                    "Hi-Res+" => "MP3",
                    _ => "Hi-Res",
                };
                self.state.settings.streaming_quality = next.to_string();
                // Persist to file (simple key-value)
                let quality_path = dirs::data_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                    .join("qbz")
                    .join("tui_streaming_quality");
                let _ = std::fs::write(&quality_path, next);
                self.state.status_message = Some(format!("Streaming Quality: {}", next));
                return;
            }
            "Audio Backend" => {
                // Cycle: Auto → PipeWire → Alsa → Pulse → Auto
                let next = match settings.backend_type {
                    None => Some(AudioBackendType::PipeWire),
                    Some(AudioBackendType::PipeWire) => Some(AudioBackendType::Alsa),
                    Some(AudioBackendType::Alsa) => Some(AudioBackendType::Pulse),
                    Some(AudioBackendType::Pulse) => None,
                    Some(_) => None,
                };

                settings.backend_type = next;
                let label = match &settings.backend_type {
                    Some(b) => format!("{:?}", b),
                    None => "Auto".to_string(),
                };

                match store.set_backend_type(settings.backend_type) {
                    Ok(()) => {
                        // Reload into player
                        let player = self.core.player();
                        if let Err(e) = player.reload_settings(settings.clone()) {
                            log::warn!("Failed to push settings to player: {}", e);
                        }
                        self.state.status_message =
                            Some(format!("Backend: {} (applies on next track)", label));
                    }
                    Err(e) => {
                        self.state.status_message = Some(format!("Failed to save: {}", e));
                    }
                }
            }
            "ALSA Plugin" => {
                use qbz_audio::AlsaPlugin;
                // Cycle: Default → Hw → PlugHw → Pcm → Default
                let next = match settings.alsa_plugin {
                    None => Some(AlsaPlugin::Hw),
                    Some(AlsaPlugin::Hw) => Some(AlsaPlugin::PlugHw),
                    Some(AlsaPlugin::PlugHw) => Some(AlsaPlugin::Pcm),
                    Some(AlsaPlugin::Pcm) => None,
                };

                settings.alsa_plugin = next;
                let label = match &settings.alsa_plugin {
                    Some(p) => format!("{:?}", p),
                    None => "Default".to_string(),
                };

                match store.set_alsa_plugin(settings.alsa_plugin) {
                    Ok(()) => {
                        let player = self.core.player();
                        if let Err(e) = player.reload_settings(settings.clone()) {
                            log::warn!("Failed to push settings to player: {}", e);
                        }
                        self.state.status_message =
                            Some(format!("ALSA Plugin: {} (applies on next track)", label));
                    }
                    Err(e) => {
                        self.state.status_message = Some(format!("Failed to save: {}", e));
                    }
                }
            }
            _ => {}
        }
    }

    // ==================== Device Picker ====================

    /// Open the device picker modal and enumerate devices for the current backend.
    fn open_device_picker(&mut self) {
        let backend_type = self.state.settings.audio_settings.backend_type
            .unwrap_or_else(qbz_audio::AudioBackendType::default);

        self.state.devices_loading = true;
        self.state.show_device_picker = true;
        self.state.device_picker_index = 0;

        match qbz_audio::BackendManager::create_backend(backend_type) {
            Ok(backend) => {
                match backend.enumerate_devices() {
                    Ok(devices) => {
                        // Find the currently selected device to highlight it
                        let current_device = &self.state.settings.audio_settings.output_device;
                        if let Some(pos) = devices.iter().position(|dev| {
                            current_device.as_ref().map(|c| c == &dev.id).unwrap_or(dev.is_default)
                        }) {
                            self.state.device_picker_index = pos;
                        }
                        self.state.available_devices = devices;
                        self.state.devices_loading = false;
                    }
                    Err(e) => {
                        self.state.status_message = Some(format!("Failed to list devices: {}", e));
                        self.state.show_device_picker = false;
                        self.state.devices_loading = false;
                    }
                }
            }
            Err(e) => {
                self.state.status_message = Some(format!("Backend error: {}", e));
                self.state.show_device_picker = false;
                self.state.devices_loading = false;
            }
        }
    }

    /// Handle key events when the device picker modal is open.
    fn handle_key_device_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.state.show_device_picker = false;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.state.available_devices.len();
                if len > 0 {
                    self.state.device_picker_index =
                        (self.state.device_picker_index + 1).min(len - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.state.device_picker_index > 0 {
                    self.state.device_picker_index -= 1;
                }
            }
            KeyCode::Enter => {
                self.select_audio_device();
            }
            _ => {}
        }
    }

    /// Select the highlighted device in the picker, persist it, and close the modal.
    fn select_audio_device(&mut self) {
        let idx = self.state.device_picker_index;
        let device = match self.state.available_devices.get(idx) {
            Some(dev) => dev.clone(),
            None => return,
        };

        let store = match AudioSettingsStore::new() {
            Ok(s) => s,
            Err(e) => {
                self.state.status_message = Some(format!("Store error: {}", e));
                return;
            }
        };

        // Save device selection
        if let Err(e) = store.set_output_device(Some(&device.id)) {
            self.state.status_message = Some(format!("Failed to save device: {}", e));
            return;
        }

        // Save device max sample rate if known
        if let Some(rate) = device.max_sample_rate {
            let _ = store.set_device_max_sample_rate(Some(rate));
            self.state.settings.audio_settings.device_max_sample_rate = Some(rate);
        }

        // Save per-device sample rate limit
        if let Some(rate) = device.max_sample_rate {
            let _ = store.set_device_sample_rate_limit(&device.id, Some(rate));
            self.state.settings.audio_settings.device_sample_rate_limits
                .insert(device.id.clone(), rate);
        }

        // Update local settings state
        self.state.settings.audio_settings.output_device = Some(device.id.clone());

        // Push to player
        let player = self.core.player();
        if let Err(e) = player.reload_settings(self.state.settings.audio_settings.clone()) {
            log::warn!("Failed to push device settings to player: {}", e);
        }

        // Close picker
        self.state.show_device_picker = false;
        self.state.status_message = Some(format!("Output: {} (applies on next track)", device.name));
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
        if self.state.album.tracks.is_empty() || idx >= self.state.album.tracks.len() {
            return;
        }

        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        // Clone the selected track to avoid borrow conflicts with &mut self
        let selected_track = self.state.album.tracks[idx].clone();

        // Build queue from selected track onward
        let queue_tracks: Vec<qbz_models::QueueTrack> = self.state.album.tracks[idx..]
            .iter()
            .map(|track| Self::track_to_queue_track(track))
            .collect();

        let first_track_id = selected_track.id;
        let first_title = selected_track.title.clone();

        // Update now-playing info immediately
        self.state.current_track_title = Some(first_title.clone());
        self.state.current_track_artist = Some(
            selected_track
                .performer
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
        );
        self.state.current_track_quality = if selected_track.hires_streamable {
            Some("Hi-Res".to_string())
        } else {
            Some(format!(
                "{}bit/{}kHz",
                selected_track.maximum_bit_depth.unwrap_or(16),
                selected_track.maximum_sampling_rate.unwrap_or(44.1)
            ))
        };
        self.state.status_message = Some(format!("Loading: {}...", first_title));

        // Trigger cover art download
        self.update_artwork_from_track(&selected_track);

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

    // ==================== Library ====================

    /// Load library data for the currently active tab.
    fn load_library_for_active_tab(&mut self) {
        match self.state.library.tab {
            LibraryTab::Albums => self.load_library_albums_if_needed(),
            LibraryTab::Artists => self.load_library_artists_if_needed(),
            LibraryTab::Tracks => self.load_library_tracks_if_needed(),
        }
    }

    /// Load library albums if not already loaded.
    fn load_library_albums_if_needed(&mut self) {
        if self.state.library.albums_loaded || self.state.library.loading {
            return;
        }

        if !self.state.authenticated {
            self.state.library.error = Some("Not authenticated".to_string());
            return;
        }

        self.state.library.loading = true;
        self.state.library.error = None;
        self.state.status_message = Some("Loading library albums...".to_string());

        let core = Arc::clone(&self.core);
        let event_tx = self.library_albums_result_tx.clone();

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
                        log::warn!("[TUI] Could not parse library albums response");
                        Ok(Vec::new())
                    }
                }
            });
            let _ = event_tx.send(parsed);
        });
    }

    /// Handle the result of a library albums load.
    fn handle_library_albums_result(&mut self, result: LibraryAlbumsResult) {
        self.state.library.loading = false;
        self.state.library.albums_loaded = true;
        match result {
            Ok(albums) => {
                let count = albums.len();
                self.state.library.albums = albums;
                self.state.library.selected_index = 0;
                self.state.library.error = None;
                self.state.status_message = Some(format!("{} library albums", count));
            }
            Err(e) => {
                self.state.library.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load library albums: {}", e));
            }
        }
    }

    /// Load library artists if not already loaded.
    fn load_library_artists_if_needed(&mut self) {
        if self.state.library.artists_loaded || self.state.library.loading {
            return;
        }

        if !self.state.authenticated {
            self.state.library.error = Some("Not authenticated".to_string());
            return;
        }

        self.state.library.loading = true;
        self.state.library.error = None;
        self.state.status_message = Some("Loading library artists...".to_string());

        let core = Arc::clone(&self.core);
        let event_tx = self.library_artists_result_tx.clone();

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
                        log::warn!("[TUI] Could not parse library artists response");
                        Ok(Vec::new())
                    }
                }
            });
            let _ = event_tx.send(parsed);
        });
    }

    /// Handle the result of a library artists load.
    fn handle_library_artists_result(&mut self, result: LibraryArtistsResult) {
        self.state.library.loading = false;
        self.state.library.artists_loaded = true;
        match result {
            Ok(artists) => {
                let count = artists.len();
                self.state.library.artists = artists;
                self.state.library.selected_index = 0;
                self.state.library.error = None;
                self.state.status_message = Some(format!("{} library artists", count));
            }
            Err(e) => {
                self.state.library.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load library artists: {}", e));
            }
        }
    }

    /// Load library tracks if not already loaded.
    fn load_library_tracks_if_needed(&mut self) {
        if self.state.library.tracks_loaded || self.state.library.loading {
            return;
        }

        if !self.state.authenticated {
            self.state.library.error = Some("Not authenticated".to_string());
            return;
        }

        self.state.library.loading = true;
        self.state.library.error = None;
        self.state.status_message = Some("Loading library tracks...".to_string());

        let core = Arc::clone(&self.core);
        let event_tx = self.library_tracks_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.get_favorites("tracks", 500, 0).await;
            let parsed = result.and_then(|json| {
                let tracks_page = json
                    .get("tracks")
                    .and_then(|tracks| {
                        serde_json::from_value::<qbz_models::SearchResultsPage<Track>>(tracks.clone()).ok()
                    });
                match tracks_page {
                    Some(page) => Ok(page.items),
                    None => {
                        log::warn!("[TUI] Could not parse library tracks response");
                        Ok(Vec::new())
                    }
                }
            });
            let _ = event_tx.send(parsed);
        });
    }

    /// Handle the result of a library tracks load.
    fn handle_library_tracks_result(&mut self, result: LibraryTracksResult) {
        self.state.library.loading = false;
        self.state.library.tracks_loaded = true;
        match result {
            Ok(tracks) => {
                let count = tracks.len();
                self.state.library.tracks = tracks;
                self.state.library.selected_index = 0;
                self.state.library.error = None;
                self.state.status_message = Some(format!("{} library tracks", count));
            }
            Err(e) => {
                self.state.library.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load library tracks: {}", e));
            }
        }
    }

    /// Cycle through library tabs.
    fn cycle_library_tab(&mut self, forward: bool) {
        let tabs = [LibraryTab::Albums, LibraryTab::Artists, LibraryTab::Tracks];
        let current = tabs.iter().position(|tab| *tab == self.state.library.tab).unwrap_or(0);
        let next = if forward {
            (current + 1) % tabs.len()
        } else {
            (current + tabs.len() - 1) % tabs.len()
        };
        self.state.library.tab = tabs[next];
        self.state.library.selected_index = 0;

        // Load data for the new tab if needed
        self.load_library_for_active_tab();
    }

    /// Get the length of the active library list.
    fn library_active_list_len(&self) -> usize {
        match self.state.library.tab {
            LibraryTab::Albums => self.state.library.albums.len(),
            LibraryTab::Artists => self.state.library.artists.len(),
            LibraryTab::Tracks => self.state.library.tracks.len(),
        }
    }

    /// Play the selected track from the library tracks tab.
    fn play_selected_library_track(&mut self) {
        let idx = self.state.library.selected_index;
        let track = match self.state.library.tracks.get(idx) {
            Some(tr) => tr.clone(),
            None => return,
        };

        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

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
            if let Err(e) = playback::play_qobuz_track(&core, track_id, &cache, &status_tx).await {
                log::error!("[TUI] Failed to play library track {}: {}", track_id, e);
                let _ = status_tx.send(PlaybackStatus::Error(e));
            }
        });
    }

    /// Open the selected album from library albums tab.
    fn open_selected_library_album(&mut self) {
        let idx = self.state.library.selected_index;
        let album_id = match self.state.library.albums.get(idx) {
            Some(album) => album.id.clone(),
            None => return,
        };
        self.load_album(&album_id, ActiveView::Library);
    }

    /// Open the selected artist from library artists tab.
    fn open_selected_library_artist(&mut self) {
        let idx = self.state.library.selected_index;
        let artist_id = match self.state.library.artists.get(idx) {
            Some(artist) => artist.id,
            None => return,
        };
        self.load_artist(artist_id, ActiveView::Library);
    }

    /// Navigate to album detail from a library track.
    fn navigate_to_album_from_library(&mut self) {
        if self.state.library.tab != LibraryTab::Tracks {
            return;
        }
        let idx = self.state.library.selected_index;
        let album_id = match self.state.library.tracks.get(idx) {
            Some(track) => track.album.as_ref().map(|a| a.id.clone()),
            None => None,
        };
        if let Some(id) = album_id {
            self.load_album(&id, ActiveView::Library);
        }
    }

    /// Add the selected library track to the queue.
    fn add_selected_library_track_to_queue(&mut self) {
        if self.state.library.tab != LibraryTab::Tracks {
            return;
        }
        let idx = self.state.library.selected_index;
        let track = match self.state.library.tracks.get(idx) {
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

    // ==================== Artist Detail ====================

    /// Load an artist page by ID and switch to the artist detail view.
    fn load_artist(&mut self, artist_id: u64, return_view: ActiveView) {
        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        self.state.artist_detail = ArtistState {
            loading: true,
            return_view,
            ..ArtistState::default()
        };
        self.state.active_view = ActiveView::Artist;
        self.state.status_message = Some("Loading artist...".to_string());

        // Close search modal if open
        if self.state.show_search_modal {
            self.state.show_search_modal = false;
            self.state.input_mode = InputMode::Normal;
        }

        let core = Arc::clone(&self.core);
        let event_tx = self.artist_page_result_tx.clone();

        self.rt_handle.spawn(async move {
            let result = core.get_artist_page(artist_id, None).await;
            let _ = event_tx.send(result);
        });
    }

    /// Handle the result of an artist page load.
    fn handle_artist_page_result(&mut self, result: ArtistPageResult) {
        self.state.artist_detail.loading = false;
        match result {
            Ok(artist) => {
                let track_count = artist.top_tracks.as_ref().map(|tracks| tracks.len()).unwrap_or(0);
                self.state.status_message = Some(format!("{} - {} top tracks", artist.name.display, track_count));
                self.state.artist_detail.artist = Some(artist);
                self.state.artist_detail.selected_index = 0;
                self.state.artist_detail.error = None;
            }
            Err(e) => {
                self.state.artist_detail.error = Some(format!("{}", e));
                self.state.status_message = Some(format!("Failed to load artist: {}", e));
            }
        }
    }

    /// Get the number of top tracks for the current artist.
    fn artist_top_tracks_len(&self) -> usize {
        self.state.artist_detail.artist
            .as_ref()
            .and_then(|a| a.top_tracks.as_ref())
            .map(|tracks| tracks.len())
            .unwrap_or(0)
    }

    /// Play the selected top track from the artist detail view.
    fn play_selected_artist_track(&mut self) {
        let artist = match &self.state.artist_detail.artist {
            Some(a) => a,
            None => return,
        };

        let top_tracks = match &artist.top_tracks {
            Some(tracks) => tracks,
            None => return,
        };

        let idx = self.state.artist_detail.selected_index;
        let track = match top_tracks.get(idx) {
            Some(tr) => tr,
            None => return,
        };

        if !self.state.authenticated {
            self.state.status_message = Some("Not authenticated".to_string());
            return;
        }

        self.state.current_track_title = Some(track.title.clone());
        self.state.current_track_artist = Some(
            track.artist.as_ref().map(|a| a.name.display.clone()).unwrap_or_else(|| "Unknown".to_string()),
        );

        let is_hires = track.audio_info.as_ref()
            .and_then(|ai| ai.maximum_bit_depth)
            .map(|bd| bd > 16)
            .unwrap_or(false);
        self.state.current_track_quality = if is_hires {
            Some("Hi-Res".to_string())
        } else {
            Some("CD".to_string())
        };

        self.state.status_message = Some(format!("Loading: {}...", track.title));

        let core = Arc::clone(&self.core);
        let track_id = track.id;
        let status_tx = self.playback_status_tx.clone();
        let cache = self.playback_cache.clone();

        self.rt_handle.spawn(async move {
            if let Err(e) = playback::play_qobuz_track(&core, track_id, &cache, &status_tx).await {
                log::error!("[TUI] Failed to play artist track {}: {}", track_id, e);
                let _ = status_tx.send(PlaybackStatus::Error(e));
            }
        });
    }

    /// Navigate to album detail from an artist top track.
    fn navigate_to_album_from_artist(&mut self) {
        let artist = match &self.state.artist_detail.artist {
            Some(a) => a,
            None => return,
        };

        let top_tracks = match &artist.top_tracks {
            Some(tracks) => tracks,
            None => return,
        };

        let idx = self.state.artist_detail.selected_index;
        let album_id = match top_tracks.get(idx) {
            Some(track) => track.album.as_ref().map(|a| a.id.clone()),
            None => None,
        };
        if let Some(id) = album_id {
            self.load_album(&id, ActiveView::Artist);
        }
    }

    /// Open the selected artist from search results.
    fn open_selected_search_artist(&mut self) {
        let idx = self.state.search.selected_index;
        let artist_id = match self.state.search.artists.get(idx) {
            Some(artist) => artist.id,
            None => return,
        };
        self.load_artist(artist_id, ActiveView::Search);
    }

    /// Open the selected artist from favorites.
    fn open_selected_favorite_artist(&mut self) {
        let idx = self.state.favorites.selected_index;
        let artist_id = match self.state.favorites.artists.get(idx) {
            Some(artist) => artist.id,
            None => return,
        };
        self.load_artist(artist_id, ActiveView::Favorites);
    }

    /// Extract artwork URL from a Track's album image set.
    fn artwork_url_from_track(track: &Track) -> Option<String> {
        track.album.as_ref().and_then(|a| {
            a.image.large.clone()
                .or_else(|| a.image.extralarge.clone())
                .or_else(|| a.image.thumbnail.clone())
                .or_else(|| a.image.small.clone())
        })
    }

    /// Set the current artwork URL and trigger a download if it changed.
    fn update_artwork_from_track(&mut self, track: &Track) {
        if self.no_images || self.state.no_images {
            return;
        }
        let new_url = Self::artwork_url_from_track(track);
        if new_url != self.state.current_artwork_url {
            self.state.current_artwork_url = new_url.clone();
            if let Some(ref url) = new_url {
                self.load_cover_art(url);
            } else {
                self.state.cover_art = None;
            }
        }
    }

    /// Set the current artwork URL from a QueueTrack and trigger download if changed.
    fn update_artwork_from_queue_track(&mut self, queue_track: &qbz_models::QueueTrack) {
        if self.no_images || self.state.no_images {
            return;
        }
        let new_url = queue_track.artwork_url.clone();
        if new_url != self.state.current_artwork_url {
            self.state.current_artwork_url = new_url.clone();
            if let Some(ref url) = new_url {
                self.load_cover_art(url);
            } else {
                self.state.cover_art = None;
            }
        }
    }

    /// Download and decode artwork from URL asynchronously.
    fn load_cover_art(&mut self, url: &str) {
        let url = url.to_string();
        let art_tx = self.cover_art_tx.clone();

        self.rt_handle.spawn(async move {
            match reqwest::get(&url).await {
                Ok(response) => {
                    if let Ok(bytes) = response.bytes().await {
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            let _ = art_tx.send(Some(img));
                        } else {
                            log::warn!("[TUI] Failed to decode artwork image");
                        }
                    }
                }
                Err(err) => {
                    log::warn!("[TUI] Failed to download artwork: {}", err);
                }
            }
        });
    }

    /// Poll the player's shared atomic state to update the now-playing bar.
    /// The V2 player doesn't emit CoreEvents for position/playback changes,
    /// so we poll every tick (~100ms). Also detects track end for auto-advance
    /// and gapless transitions.
    async fn poll_player_state(&mut self) {
        let ps = self.core.get_playback_state();
        let is_playing = ps.is_playing;
        let player = self.core.player();

        // Detect gapless transition: track_id changed while still playing.
        // The audio thread already transitioned seamlessly; we need to advance
        // the queue and update the UI to reflect the new track.
        if is_playing
            && ps.track_id != 0
            && self.last_track_id != 0
            && ps.track_id != self.last_track_id
        {
            log::info!(
                "[TUI] Gapless transition detected: track {} -> {}",
                self.last_track_id,
                ps.track_id,
            );

            // Advance the queue (the player already switched internally)
            if let Some(advanced_track) = self.core.next_track().await {
                log::info!(
                    "[TUI] Queue advanced for gapless: {} - {}",
                    advanced_track.artist,
                    advanced_track.title,
                );

                // Update now-playing info
                self.state.current_track_title = Some(advanced_track.title.clone());
                self.state.current_track_artist = Some(advanced_track.artist.clone());
                self.state.current_track_quality =
                    match (advanced_track.bit_depth, advanced_track.sample_rate) {
                        (Some(bd), Some(sr)) => {
                            Some(format!("{}-bit / {:.1}kHz", bd, sr / 1000.0))
                        }
                        _ if advanced_track.hires => Some("Hi-Res".to_string()),
                        _ => None,
                    };

                // Trigger cover art download for the new track
                self.update_artwork_from_queue_track(&advanced_track);

                // Pre-buffer the NEXT track after this gapless transition
                self.prebuffer_next_track();
            }
        }

        // Check for auto-advance (non-gapless: track ended, player stopped)
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

            // Trigger cover art download for auto-advanced track
            self.update_artwork_from_queue_track(&next_track);

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

        // Gapless pre-buffering: when the audio thread signals it wants the
        // next track queued (~5s before current track ends), download and
        // feed it via play_next() for seamless transition.
        if player.state.is_gapless_ready() && player.state.get_gapless_next_track_id() == 0 {
            self.prebuffer_next_track();
        }

        // Update state
        self.was_playing = is_playing;
        self.last_track_id = ps.track_id;
        self.state.is_playing = is_playing;
        self.state.position_secs = ps.position;
        self.state.duration_secs = ps.duration;
        self.state.volume = ps.volume;
    }

    /// Pre-buffer the next track in the queue for gapless playback.
    ///
    /// Downloads (or loads from cache) the next track and feeds it to the
    /// player via `play_next()` so the audio thread can append it to the
    /// current sink without any gap.
    fn prebuffer_next_track(&mut self) {
        let core = Arc::clone(&self.core);
        let cache = self.playback_cache.clone();

        self.rt_handle.spawn(async move {
            // Peek at the next track without advancing the queue
            let upcoming = core.peek_upcoming(1).await;
            let next_track = match upcoming.first() {
                Some(track) => track.clone(),
                None => return,
            };

            log::info!(
                "[TUI] Pre-buffering next track for gapless: {} - {} (id={})",
                next_track.artist,
                next_track.title,
                next_track.id,
            );

            // Load quality settings
            let (quality, streaming_only) = playback::load_quality_settings();

            // Check L2 disk cache first
            if let Some(ref pc) = cache {
                if let Some(cached_data) = pc.get(next_track.id) {
                    log::info!(
                        "[TUI] Gapless pre-buffer: cache HIT for track {} ({} bytes)",
                        next_track.id,
                        cached_data.len(),
                    );
                    let player = core.player();
                    match player.play_next(cached_data, next_track.id) {
                        Ok(()) => log::info!("[TUI] Next track queued for gapless (from cache)"),
                        Err(e) => log::warn!("[TUI] Failed to queue gapless track: {}", e),
                    }
                    return;
                }
            }

            // Download from network
            match core.get_stream_url(next_track.id, quality).await {
                Ok(stream_url) => match reqwest::get(&stream_url.url).await {
                    Ok(response) => {
                        if let Ok(bytes) = response.bytes().await {
                            let audio_vec = bytes.to_vec();

                            // Cache the download (unless streaming_only)
                            if !streaming_only {
                                if let Some(ref pc) = cache {
                                    pc.insert(next_track.id, &audio_vec);
                                    log::debug!(
                                        "[TUI] Cached pre-buffered track {} to L2 disk",
                                        next_track.id,
                                    );
                                }
                            }

                            // Queue for gapless playback
                            let player = core.player();
                            match player.play_next(audio_vec, next_track.id) {
                                Ok(()) => {
                                    log::info!(
                                        "[TUI] Next track queued for gapless (from network)"
                                    )
                                }
                                Err(e) => {
                                    log::warn!("[TUI] Failed to queue gapless track: {}", e)
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("[TUI] Failed to download next track for gapless: {}", e)
                    }
                },
                Err(e) => {
                    log::warn!(
                        "[TUI] Failed to get stream URL for gapless pre-buffer: {}",
                        e
                    )
                }
            }
        });
    }

    /// Read audio samples from the visualizer tap and compute bar heights.
    fn update_visualizer(&mut self) {
        if self.state.right_panel_mode != RightPanelMode::Visualizer
            || !self.state.show_queue_panel
        {
            return;
        }

        let num_bars: usize = 16;

        if !self.state.is_playing {
            // Decay bars to zero when not playing
            if !self.state.visualizer_bars.is_empty() {
                let mut all_zero = true;
                for bar in self.state.visualizer_bars.iter_mut() {
                    *bar *= 0.8;
                    if *bar > 0.01 {
                        all_zero = false;
                    } else {
                        *bar = 0.0;
                    }
                }
                if all_zero {
                    self.state.visualizer_bars.clear();
                }
            }
            return;
        }

        if let Some(ref tap) = self.visualizer_tap {
            // Read a snapshot of the latest samples from the ring buffer
            let sample_count = 2048;
            let mut samples = vec![0.0f32; sample_count];
            tap.ring_buffer.snapshot(&mut samples);

            // Check if we have any actual audio data
            let has_signal = samples.iter().any(|s| s.abs() > 0.0001);
            if !has_signal {
                // No signal — decay existing bars
                if !self.state.visualizer_bars.is_empty() {
                    for bar in self.state.visualizer_bars.iter_mut() {
                        *bar *= 0.85;
                        if *bar < 0.01 {
                            *bar = 0.0;
                        }
                    }
                }
                return;
            }

            let chunk_size = samples.len() / num_bars;
            let mut bars = Vec::with_capacity(num_bars);

            for chunk_idx in 0..num_bars {
                let start = chunk_idx * chunk_size;
                let end = (start + chunk_size).min(samples.len());
                let chunk = &samples[start..end];

                // RMS of the chunk
                let rms: f32 = (chunk.iter().map(|s| s * s).sum::<f32>()
                    / chunk.len().max(1) as f32)
                    .sqrt();

                // Scale to 0.0..1.0 (typical audio RMS peaks around 0.3-0.5)
                let height = (rms * 3.0).clamp(0.0, 1.0);
                bars.push(height);
            }

            // Smooth with previous bars (instant rise, slow decay)
            if self.state.visualizer_bars.len() == num_bars {
                for (idx, bar) in bars.iter_mut().enumerate() {
                    let prev = self.state.visualizer_bars[idx];
                    *bar = if *bar > prev {
                        *bar // instant rise
                    } else {
                        prev * 0.85 + *bar * 0.15 // slow decay
                    };
                }
            }

            self.state.visualizer_bars = bars;
        }
    }

    fn handle_core_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::TrackStarted { track, .. } => {
                self.state.is_playing = true;
                self.state.current_track_title = Some(track.title.clone());
                self.state.current_track_artist = Some(track.artist.clone());
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

                // Trigger cover art download from TrackStarted event
                self.update_artwork_from_queue_track(&track);
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

/// Extract a dominant color from album cover art for use as a dynamic accent.
///
/// Downscales the image to 16x16 for fast analysis, skips transparent and
/// very dark/light pixels (backgrounds and highlights), then averages the
/// remaining pixels and boosts saturation slightly for visibility on dark
/// terminal backgrounds. Falls back to Cyan if no qualifying pixels are found.
fn extract_dominant_color(img: &image::DynamicImage) -> ratatui::style::Color {
    let thumb = img.resize_exact(16, 16, image::imageops::FilterType::Nearest);
    let rgba = thumb.to_rgba8();

    let mut total_r: u64 = 0;
    let mut total_g: u64 = 0;
    let mut total_b: u64 = 0;
    let mut count: u64 = 0;

    for pixel in rgba.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 128 {
            continue; // skip transparent
        }
        // Skip very dark and very light pixels (backgrounds/highlights)
        let brightness = (r as u32 + g as u32 + b as u32) / 3;
        if brightness < 30 || brightness > 225 {
            continue;
        }
        total_r += r as u64;
        total_g += g as u64;
        total_b += b as u64;
        count += 1;
    }

    if count == 0 {
        return ratatui::style::Color::Cyan; // fallback to default accent
    }

    let r = (total_r / count) as u8;
    let g = (total_g / count) as u8;
    let b = (total_b / count) as u8;

    // Boost saturation slightly for better visibility on dark background
    let max_ch = r.max(g).max(b);
    let min_ch = r.min(g).min(b);
    if max_ch > min_ch {
        let boost = 1.3f32;
        let mid = (max_ch as f32 + min_ch as f32) / 2.0;
        let r2 = ((r as f32 - mid) * boost + mid).clamp(0.0, 255.0) as u8;
        let g2 = ((g as f32 - mid) * boost + mid).clamp(0.0, 255.0) as u8;
        let b2 = ((b as f32 - mid) * boost + mid).clamp(0.0, 255.0) as u8;
        ratatui::style::Color::Rgb(r2, g2, b2)
    } else {
        ratatui::style::Color::Rgb(r, g, b)
    }
}
