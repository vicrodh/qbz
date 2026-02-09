//! API response models

use serde::{Deserialize, Serialize};

/// Audio quality format IDs
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u32)]
pub enum Quality {
    Mp3 = 5,
    Lossless = 6,    // 16-bit/44.1kHz (CD Quality)
    HiRes = 7,       // 24-bit/≤96kHz
    UltraHiRes = 27, // 24-bit/>96kHz
}

impl Quality {
    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            5 => Some(Quality::Mp3),
            6 => Some(Quality::Lossless),
            7 => Some(Quality::HiRes),
            27 => Some(Quality::UltraHiRes),
            _ => None,
        }
    }

    pub fn id(&self) -> u32 {
        *self as u32
    }

    pub fn label(&self) -> &'static str {
        match self {
            Quality::Mp3 => "MP3 320kbps",
            Quality::Lossless => "FLAC 16-bit/44.1kHz",
            Quality::HiRes => "FLAC 24-bit/≤96kHz",
            Quality::UltraHiRes => "FLAC 24-bit/>96kHz",
        }
    }

    /// Quality levels in descending order for fallback
    pub fn fallback_order() -> &'static [Quality] {
        &[
            Quality::UltraHiRes,
            Quality::HiRes,
            Quality::Lossless,
            Quality::Mp3,
        ]
    }
}

/// User credentials and session info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSession {
    pub user_auth_token: String,
    pub user_id: u64,
    pub email: String,
    pub display_name: String,
    pub subscription_label: String,
    #[serde(default)]
    pub subscription_valid_until: Option<String>,
}

/// Stream URL response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamUrl {
    pub url: String,
    pub format_id: u32,
    pub mime_type: String,
    pub sampling_rate: f64,
    pub bit_depth: Option<u32>,
    pub track_id: u64,
    pub restrictions: Vec<StreamRestriction>,
}

impl StreamUrl {
    /// Check if the stream has restrictions that prevent playback
    pub fn has_restrictions(&self) -> bool {
        self.restrictions.iter().any(|r| {
            r.code == "FormatRestrictedByFormatAvailability"
                || r.code == "SampleRestrictedByRightHolders"
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamRestriction {
    pub code: String,
}

/// Album model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: Artist,
    #[serde(default)]
    pub image: ImageSet,
    pub release_date_original: Option<String>,
    pub label: Option<Label>,
    pub genre: Option<Genre>,
    pub tracks_count: Option<u32>,
    pub duration: Option<u32>,
    #[serde(default)]
    pub hires: bool,
    #[serde(default)]
    pub hires_streamable: bool,
    pub maximum_sampling_rate: Option<f64>,
    pub maximum_bit_depth: Option<u32>,
    #[serde(default)]
    pub tracks: Option<TracksContainer>,
    /// Universal Product Code for the album
    pub upc: Option<String>,
    /// Editorial description/review of the album
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracksContainer {
    pub items: Vec<Track>,
    pub total: u32,
}

/// Track model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub title: String,
    pub isrc: Option<String>,
    #[serde(default)]
    pub duration: u32,
    #[serde(default)]
    pub track_number: u32,
    pub media_number: Option<u32>,
    pub performer: Option<Artist>,
    pub album: Option<AlbumSummary>,
    #[serde(default)]
    pub hires: bool,
    #[serde(default)]
    pub hires_streamable: bool,
    pub maximum_sampling_rate: Option<f64>,
    pub maximum_bit_depth: Option<u32>,
    #[serde(default)]
    pub streamable: bool,
    #[serde(default)]
    pub parental_warning: bool,
    /// Playlist-specific: ID within the playlist (for removal)
    pub playlist_track_id: Option<u64>,
    /// Performers/credits string from Qobuz (format: "Name, Role - Name, Role")
    pub performers: Option<String>,
    /// Composer information
    pub composer: Option<Artist>,
    /// Copyright information
    pub copyright: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumSummary {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub image: ImageSet,
    /// Label (if returned in track response)
    pub label: Option<Label>,
}

/// Artist model
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Artist {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    pub image: Option<ImageSet>,
    #[serde(default)]
    pub albums_count: Option<u32>,
    /// Biography (available when fetching full artist details)
    #[serde(default)]
    pub biography: Option<ArtistBiography>,
    /// Albums (available when fetching with extra=albums)
    #[serde(default)]
    pub albums: Option<ArtistAlbums>,
    /// Tracks where this artist appears (extra=tracks_appears_on)
    #[serde(default)]
    pub tracks_appears_on: Option<TracksContainer>,
    /// Curated playlists for this artist (extra=playlists)
    #[serde(default)]
    pub playlists: Option<Vec<Playlist>>,
}

/// Artist biography content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistBiography {
    pub summary: Option<String>,
    pub content: Option<String>,
    pub source: Option<String>,
}

/// Artist albums container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistAlbums {
    pub items: Vec<Album>,
    pub total: u32,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub limit: u32,
}

/// Playlist model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub owner: PlaylistOwner,
    pub images: Option<Vec<String>>,
    #[serde(default)]
    pub tracks_count: u32,
    #[serde(default)]
    pub duration: u32,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub tracks: Option<TracksContainer>,
    // Additional fields from search results
    pub genres: Option<Vec<PlaylistGenre>>,
    pub images150: Option<Vec<String>>,
    pub images300: Option<Vec<String>>,
    pub slug: Option<String>,
    pub users_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlaylistOwner {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistGenre {
    pub id: u64,
    pub name: String,
    pub slug: Option<String>,
}

/// Image set with multiple resolutions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageSet {
    pub small: Option<String>,
    pub thumbnail: Option<String>,
    pub large: Option<String>,
    pub extralarge: Option<String>,
    pub mega: Option<String>,
    pub back: Option<String>,
}

impl ImageSet {
    pub fn best(&self) -> Option<&String> {
        self.mega
            .as_ref()
            .or(self.extralarge.as_ref())
            .or(self.large.as_ref())
            .or(self.thumbnail.as_ref())
            .or(self.small.as_ref())
    }
}

/// Label model (basic reference)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub id: u64,
    pub name: String,
}

/// Label detail with albums
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelDetail {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    pub image: Option<ImageSet>,
    pub albums: Option<SearchResultsPage<Album>>,
    pub albums_count: Option<u32>,
}

/// Genre model (basic, used in album responses)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Genre {
    pub id: u64,
    pub name: String,
}

/// Genre info with full details (from genre/list endpoint)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenreInfo {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub path: Option<Vec<u64>>,
}

/// Genre list response from API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenreListResponse {
    pub genres: GenreListContainer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenreListContainer {
    pub items: Vec<GenreInfo>,
}

/// Search results container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub albums: Option<SearchResultsPage<Album>>,
    pub tracks: Option<SearchResultsPage<Track>>,
    pub artists: Option<SearchResultsPage<Artist>>,
    pub playlists: Option<SearchResultsPage<Playlist>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultsPage<T> {
    pub items: Vec<T>,
    pub total: u32,
    pub offset: u32,
    pub limit: u32,
}

/// Favorites container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorites {
    pub albums: Option<SearchResultsPage<Album>>,
    pub tracks: Option<SearchResultsPage<Track>>,
    pub artists: Option<SearchResultsPage<Artist>>,
}

// ============ Discover API Models ============

/// Discover index response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverResponse {
    pub containers: DiscoverContainers,
}

/// All discover containers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverContainers {
    pub playlists: Option<DiscoverContainer<DiscoverPlaylist>>,
    pub ideal_discography: Option<DiscoverContainer<DiscoverAlbum>>,
    pub playlists_tags: Option<DiscoverContainer<PlaylistTag>>,
    pub new_releases: Option<DiscoverContainer<DiscoverAlbum>>,
    pub qobuzissims: Option<DiscoverContainer<DiscoverAlbum>>,
    pub most_streamed: Option<DiscoverContainer<DiscoverAlbum>>,
    pub press_awards: Option<DiscoverContainer<DiscoverAlbum>>,
    pub album_of_the_week: Option<DiscoverContainer<DiscoverAlbum>>,
}

/// Generic discover container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverContainer<T> {
    pub id: String,
    pub data: DiscoverData<T>,
}

/// Generic discover data with items
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverData<T> {
    pub has_more: bool,
    pub items: Vec<T>,
}

/// Playlist from discover endpoint (different structure than regular Playlist)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverPlaylist {
    pub id: u64,
    pub name: String,
    pub owner: PlaylistOwner,
    pub image: DiscoverPlaylistImage,
    pub description: Option<String>,
    pub duration: u32,
    pub tracks_count: u32,
    pub genres: Option<Vec<PlaylistGenre>>,
    pub tags: Option<Vec<PlaylistTag>>,
}

/// Playlist image structure from discover (has rectangle and covers)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverPlaylistImage {
    pub rectangle: Option<String>,
    pub covers: Option<Vec<String>>,
}

/// Playlist tag (for filtering)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTag {
    pub id: u64,
    pub slug: String,
    pub name: String,
}

/// Response from discover/playlists endpoint
/// Note: This endpoint returns items directly at root level (not wrapped in "playlists")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverPlaylistsResponse {
    pub has_more: bool,
    pub items: Vec<DiscoverPlaylist>,
}

/// Album from discover endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverAlbum {
    pub id: String,
    pub title: String,
    pub version: Option<String>,
    pub track_count: Option<u32>,
    pub duration: Option<u32>,
    pub parental_warning: Option<bool>,
    pub image: DiscoverAlbumImage,
    pub artists: Vec<DiscoverArtist>,
    pub label: Option<Label>,
    pub genre: Option<Genre>,
    pub dates: Option<DiscoverAlbumDates>,
    pub audio_info: Option<DiscoverAudioInfo>,
}

/// Album image from discover endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverAlbumImage {
    pub small: Option<String>,
    pub thumbnail: Option<String>,
    pub large: Option<String>,
}

/// Artist in discover album
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverArtist {
    pub id: u64,
    pub name: String,
    pub roles: Option<Vec<String>>,
}

/// Album dates from discover
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverAlbumDates {
    pub download: Option<String>,
    pub original: Option<String>,
    pub stream: Option<String>,
}

/// Audio info from discover album
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverAudioInfo {
    pub maximum_sampling_rate: Option<f64>,
    pub maximum_bit_depth: Option<u32>,
    pub maximum_channel_count: Option<u32>,
}
