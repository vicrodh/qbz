//! TUI application state types
//!
//! Defines all state structs for the entire TUI. Each view gets its own
//! small struct. This file defines DATA only -- no logic.

use qbz_audio::{AudioDevice, AudioSettings};
use qbz_models::types::{
    Album, Artist, DiscoverResponse, PageArtistResponse, Playlist, Track,
};

// ============ Navigation ============

/// Which main view is currently active
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveView {
    Discover,
    Favorites,
    Library,
    Purchases,
    Search,
    AlbumDetail,
    ArtistDetail,
    PlaylistDetail,
    Settings,
}

impl Default for ActiveView {
    fn default() -> Self {
        Self::Discover
    }
}

/// Which major section has keyboard focus
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusSection {
    Sidebar,
    Main,
    Queue,
}

impl Default for FocusSection {
    fn default() -> Self {
        Self::Main
    }
}

/// Modal dialogs that overlay the main UI
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalType {
    Login,
    DevicePicker,
    Search,
}

/// Severity level for transient status messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Warning,
    Error,
}

// ============ Playback Display ============

/// Snapshot of playback state polled from Player SharedState every tick.
/// Decoupled from the player internals so the UI never holds locks.
#[derive(Debug, Clone)]
pub struct PlaybackDisplayState {
    pub track_title: Option<String>,
    pub track_artist: Option<String>,
    pub track_album: Option<String>,
    pub position_secs: u64,
    pub duration_secs: u64,
    pub volume: f32,
    pub quality_label: Option<String>,
    pub is_playing: bool,
    pub is_buffering: bool,
    pub buffer_status: Option<String>,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub track_id: u64,
    pub artwork_url: Option<String>,
}

impl Default for PlaybackDisplayState {
    fn default() -> Self {
        Self {
            track_title: None,
            track_artist: None,
            track_album: None,
            position_secs: 0,
            duration_secs: 0,
            volume: 0.75,
            quality_label: None,
            is_playing: false,
            is_buffering: false,
            buffer_status: None,
            sample_rate: 0,
            bit_depth: 0,
            track_id: 0,
            artwork_url: None,
        }
    }
}

// ============ View-Specific States ============

/// Tab options for the Discover view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiscoverTab {
    #[default]
    Featured,
    NewReleases,
    Playlists,
    PressAwards,
    MostStreamed,
}

/// State for the Discover view
#[derive(Debug, Clone, Default)]
pub struct DiscoverState {
    pub tab: DiscoverTab,
    pub data: Option<DiscoverResponse>,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub loaded: bool,
    pub loading: bool,
}

/// Tab options for the Favorites view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FavoritesTab {
    #[default]
    Tracks,
    Albums,
    Artists,
    Playlists,
}

/// State for the Favorites view
#[derive(Debug, Clone, Default)]
pub struct FavoritesState {
    pub tab: FavoritesTab,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub selected_index_tracks: usize,
    pub selected_index_albums: usize,
    pub selected_index_artists: usize,
    pub selected_index_playlists: usize,
    pub loaded_tracks: bool,
    pub loaded_albums: bool,
    pub loaded_artists: bool,
    pub loaded_playlists: bool,
}

/// Tab options for the Library view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryTab {
    #[default]
    Albums,
    Artists,
    Tracks,
}

/// State for the Library view
#[derive(Debug, Clone, Default)]
pub struct LibraryState {
    pub tab: LibraryTab,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub tracks: Vec<Track>,
    pub selected_index: usize,
    pub loaded: bool,
}

/// Tab options for the Search view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchTab {
    #[default]
    All,
    Albums,
    Tracks,
    Artists,
    Playlists,
}

/// State for the Search view
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub tab: SearchTab,
    pub query: String,
    pub cursor_pos: usize,
    pub albums: Vec<Album>,
    pub tracks: Vec<Track>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub selected_index: usize,
    pub loading: bool,
}

/// State for the Album Detail view
#[derive(Debug, Clone, Default)]
pub struct AlbumDetailState {
    pub album: Option<Album>,
    pub tracks: Vec<Track>,
    pub selected_index: usize,
    pub return_view: Option<ActiveView>,
}

/// State for the Artist Detail view
#[derive(Debug, Clone, Default)]
pub struct ArtistDetailState {
    pub artist: Option<PageArtistResponse>,
    pub tab: ArtistDetailTab,
    pub selected_index: usize,
    pub return_view: Option<ActiveView>,
}

/// Tab options for the Artist Detail view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArtistDetailTab {
    #[default]
    Overview,
    Albums,
    Tracks,
    Similar,
}

/// State for the Playlist Detail view
#[derive(Debug, Clone, Default)]
pub struct PlaylistDetailState {
    pub playlist: Option<Playlist>,
    pub selected_index: usize,
    pub return_view: Option<ActiveView>,
}

/// State for the Settings view
#[derive(Debug, Clone)]
pub struct SettingsState {
    pub audio_settings: AudioSettings,
    pub streaming_quality: String,
    pub selected_index: usize,
    pub loaded: bool,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            audio_settings: AudioSettings::default(),
            streaming_quality: String::new(),
            selected_index: 0,
            loaded: false,
        }
    }
}

/// Tab options for the Queue display panel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueueTab {
    #[default]
    Queue,
    History,
}

/// State for the Queue display panel
#[derive(Debug, Clone, Default)]
pub struct QueueDisplayState {
    pub tab: QueueTab,
    pub selected_index: usize,
}

/// State for the Sidebar
#[derive(Debug, Clone, Default)]
pub struct SidebarState {
    pub playlists: Vec<Playlist>,
    pub selected_index: usize,
    pub expanded: bool,
}

/// State for QConnect integration
#[derive(Debug, Clone, Default)]
pub struct QConnectState {
    pub enabled: bool,
    pub status: String,
    pub last_error: Option<String>,
}

/// Which field is active in the Login modal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoginField {
    #[default]
    Email,
    Password,
}

/// State for the Login modal
#[derive(Debug, Clone, Default)]
pub struct LoginState {
    pub email: String,
    pub password: String,
    pub email_cursor: usize,
    pub password_cursor: usize,
    pub active_field: LoginField,
    pub logging_in: bool,
    pub error: Option<String>,
}

/// State for the Device Picker modal
#[derive(Debug, Clone, Default)]
pub struct DevicePickerState {
    pub devices: Vec<AudioDevice>,
    pub selected_index: usize,
    pub loading: bool,
}

// ============ Top-Level State ============

/// Complete application state for the TUI.
///
/// Contains all view states as sub-structs. The `App` struct owns
/// a single `AppState` instance and mutates it in response to events.
pub struct AppState {
    // Navigation
    pub active_view: ActiveView,
    pub focus: FocusSection,
    pub view_stack: Vec<ActiveView>,

    // Playback
    pub playback: PlaybackDisplayState,

    // Cover art (ratatui-image protocol state)
    pub cover_art: Option<ratatui_image::protocol::StatefulProtocol>,
    pub dynamic_accent: Option<ratatui::style::Color>,

    // View states
    pub discover: DiscoverState,
    pub favorites: FavoritesState,
    pub library: LibraryState,
    pub search: SearchState,
    pub album_detail: AlbumDetailState,
    pub artist_detail: ArtistDetailState,
    pub playlist_detail: PlaylistDetailState,
    pub settings: SettingsState,
    pub queue: QueueDisplayState,
    pub sidebar: SidebarState,
    pub qconnect: QConnectState,

    // Modal states
    pub login: LoginState,
    pub device_picker: DevicePickerState,

    // Session
    pub authenticated: bool,
    pub user_email: Option<String>,
    pub subscription: Option<String>,

    // UI
    pub active_modal: Option<ModalType>,
    pub status_message: Option<(String, StatusLevel)>,
    pub no_images: bool,
}

impl AppState {
    pub fn new(no_images: bool) -> Self {
        Self {
            active_view: ActiveView::default(),
            focus: FocusSection::default(),
            view_stack: Vec::new(),

            playback: PlaybackDisplayState::default(),

            cover_art: None,
            dynamic_accent: None,

            discover: DiscoverState::default(),
            favorites: FavoritesState::default(),
            library: LibraryState::default(),
            search: SearchState::default(),
            album_detail: AlbumDetailState::default(),
            artist_detail: ArtistDetailState::default(),
            playlist_detail: PlaylistDetailState::default(),
            settings: SettingsState::default(),
            queue: QueueDisplayState::default(),
            sidebar: SidebarState::default(),
            qconnect: QConnectState::default(),

            login: LoginState::default(),
            device_picker: DevicePickerState::default(),

            authenticated: false,
            user_email: None,
            subscription: None,

            active_modal: None,
            status_message: None,
            no_images,
        }
    }
}
