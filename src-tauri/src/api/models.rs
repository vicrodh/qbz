//! API response models

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

/// Lenient deserializer: if the field is present but has a wrong type, return None
/// instead of failing the entire response. Use for non-vital optional fields.
fn lenient_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    Ok(serde_json::from_value(value).ok())
}

fn lenient_page<'de, D, T>(deserializer: D) -> Result<SearchResultsPage<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(SearchResultsPage {
            items: Vec::new(),
            total: 0,
            offset: 0,
            limit: 0,
        });
    }
    Ok(serde_json::from_value(value).unwrap_or(SearchResultsPage {
        items: Vec::new(),
        total: 0,
        offset: 0,
        limit: 0,
    }))
}

fn lenient_page_flexible<'de, D, T>(deserializer: D) -> Result<SearchResultsPage<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(SearchResultsPage {
            items: Vec::new(),
            total: 0,
            offset: 0,
            limit: 0,
        });
    }

    let items: Vec<T> = value
        .get("items")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| serde_json::from_value::<T>(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    let limit = value
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(items.len() as u32);

    let offset = value
        .get("offset")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(0);

    let total = value
        .get("total")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(items.len() as u32);

    Ok(SearchResultsPage {
        items,
        total,
        offset,
        limit,
    })
}

fn deserialize_string_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(v) => v,
        serde_json::Value::Number(v) => v.to_string(),
        _ => String::new(),
    })
}

fn deserialize_u64_id<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Number(v) => v.as_u64().unwrap_or(0),
        serde_json::Value::String(v) => v.parse::<u64>().unwrap_or(0),
        _ => 0,
    })
}

fn serde_true() -> bool {
    true
}

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

/// Lightweight playlist response with track IDs only (no full Track objects).
/// Returned by `playlist/get?extra=track_ids`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistWithTrackIds {
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
    pub track_ids: Vec<u64>,
    pub genres: Option<Vec<PlaylistGenre>>,
    pub images150: Option<Vec<String>>,
    pub images300: Option<Vec<String>>,
    pub slug: Option<String>,
    pub users_count: Option<u32>,
}

/// Result of checking for duplicate tracks in a playlist
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistDuplicateResult {
    pub total_tracks: usize,
    pub duplicate_count: usize,
    pub duplicate_track_ids: std::collections::HashSet<u64>,
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

// ============ Label Page Types (/label/page) ============

/// Top-level response from /label/page
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelPageData {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub image: Option<serde_json::Value>,
    #[serde(default)]
    pub releases: Option<Vec<LabelPageContainer>>,
    #[serde(default)]
    pub playlists: Option<LabelPageGenericList>,
    #[serde(default)]
    pub top_tracks: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub top_artists: Option<LabelPageGenericList>,
}

/// A container within label page (e.g. releases category)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelPageContainer {
    pub id: Option<String>,
    pub data: Option<LabelPageGenericList>,
}

/// Generic list with has_more and items
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelPageGenericList {
    pub has_more: Option<bool>,
    pub items: Option<Vec<serde_json::Value>>,
}

/// Response from /label/explore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelExploreResponse {
    pub has_more: Option<bool>,
    pub items: Option<Vec<serde_json::Value>>,
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

impl<T> Default for SearchResultsPage<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            total: 0,
            offset: 0,
            limit: 0,
        }
    }
}

// ============ Purchases API Models ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseResponse {
    #[serde(default, deserialize_with = "lenient_page")]
    pub albums: SearchResultsPage<PurchaseAlbum>,
    #[serde(default, deserialize_with = "lenient_page")]
    pub tracks: SearchResultsPage<PurchaseTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseIdsResponse {
    #[serde(default, deserialize_with = "lenient_page")]
    pub albums: SearchResultsPage<serde_json::Value>,
    #[serde(default, deserialize_with = "lenient_page")]
    pub tracks: SearchResultsPage<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseAlbum {
    #[serde(default, deserialize_with = "deserialize_string_id")]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: Artist,
    #[serde(default)]
    pub image: ImageSet,
    #[serde(default, deserialize_with = "lenient_option")]
    pub release_date_original: Option<String>,
    #[serde(default, deserialize_with = "lenient_option")]
    pub label: Option<Label>,
    #[serde(default, deserialize_with = "lenient_option")]
    pub genre: Option<Genre>,
    #[serde(default, deserialize_with = "lenient_option")]
    pub tracks_count: Option<u32>,
    #[serde(default, deserialize_with = "lenient_option")]
    pub duration: Option<u32>,
    #[serde(default)]
    pub hires: bool,
    #[serde(default, deserialize_with = "lenient_option")]
    pub maximum_sampling_rate: Option<f64>,
    #[serde(default, deserialize_with = "lenient_option")]
    pub maximum_bit_depth: Option<u32>,
    #[serde(default = "serde_true")]
    pub downloadable: bool,
    #[serde(default)]
    pub downloaded: bool,
    #[serde(default, deserialize_with = "lenient_option")]
    pub purchased_at: Option<i64>,
    #[serde(default, deserialize_with = "lenient_option")]
    pub tracks: Option<SearchResultsPage<PurchaseTrack>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseTrack {
    #[serde(default, deserialize_with = "deserialize_u64_id")]
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub track_number: u32,
    #[serde(default, deserialize_with = "lenient_option")]
    pub media_number: Option<u32>,
    #[serde(default)]
    pub duration: u32,
    #[serde(default)]
    pub performer: Artist,
    #[serde(default, deserialize_with = "lenient_option")]
    pub album: Option<AlbumSummary>,
    #[serde(default)]
    pub hires: bool,
    #[serde(default, deserialize_with = "lenient_option")]
    pub maximum_sampling_rate: Option<f64>,
    #[serde(default, deserialize_with = "lenient_option")]
    pub maximum_bit_depth: Option<u32>,
    #[serde(default = "serde_true")]
    pub streamable: bool,
    #[serde(default)]
    pub downloaded: bool,
    #[serde(default, deserialize_with = "lenient_option")]
    pub purchased_at: Option<i64>,
}

// ============ Dynamic Suggest API Models ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSuggestRequest {
    pub limit: u32,
    #[serde(default)]
    pub listened_tracks_ids: Vec<u64>,
    #[serde(default)]
    pub track_to_analysed: Vec<DynamicTrackToAnalyse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicTrackToAnalyse {
    pub track_id: u64,
    pub artist_id: u64,
    pub genre_id: u64,
    pub label_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSuggestResponse {
    #[serde(default)]
    pub algorithm: String,
    #[serde(default, deserialize_with = "lenient_page_flexible")]
    pub tracks: SearchResultsPage<DynamicSuggestTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSuggestTrack {
    #[serde(default, deserialize_with = "deserialize_u64_id")]
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default, deserialize_with = "lenient_option")]
    pub duration: Option<u32>,
    #[serde(default, deserialize_with = "lenient_option")]
    pub performer: Option<Artist>,
    #[serde(default, deserialize_with = "lenient_option")]
    pub album: Option<AlbumSummary>,
    #[serde(default)]
    pub hires: bool,
    #[serde(default, deserialize_with = "lenient_option")]
    pub maximum_sampling_rate: Option<f64>,
    #[serde(default, deserialize_with = "lenient_option")]
    pub maximum_bit_depth: Option<u32>,
    #[serde(default = "serde_true")]
    pub streamable: bool,
}

/// Response from `/radio/album` endpoint
#[derive(Debug, Clone, Deserialize)]
pub struct RadioResponse {
    #[serde(rename = "type")]
    pub radio_type: Option<String>,
    pub algorithm: Option<String>,
    pub title: Option<String>,
    pub duration: Option<u64>,
    pub track_count: Option<u32>,
    #[serde(default, deserialize_with = "lenient_page_flexible")]
    pub tracks: SearchResultsPage<Track>,
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

/// Raw playlist tag from /playlist/getTags (has localized name_json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPlaylistTag {
    pub slug: String,
    pub name_json: String,
    pub position: Option<String>,
    pub is_discover: Option<String>,
    pub featured_tag_id: Option<String>,
}

/// Response from /playlist/getTags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTagsResponse {
    pub tags: Vec<RawPlaylistTag>,
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

// ============ Artist Page Types (/artist/page) ============

/// Top-level response from /artist/page
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistResponse {
    pub id: u64,
    pub name: PageArtistName,
    pub artist_category: Option<String>,
    pub biography: Option<PageArtistBiography>,
    pub images: Option<PageArtistImages>,
    pub similar_artists: Option<PageArtistSimilar>,
    pub top_tracks: Option<Vec<PageArtistTrack>>,
    pub last_release: Option<serde_json::Value>,
    pub releases: Option<Vec<PageArtistReleaseGroup>>,
    pub tracks_appears_on: Option<Vec<PageArtistTrack>>,
    pub playlists: Option<PageArtistPlaylists>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistName {
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistBiography {
    pub content: Option<String>,
    pub source: Option<serde_json::Value>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistImages {
    pub portrait: Option<PageArtistPortrait>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistPortrait {
    pub hash: String,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistSimilar {
    pub has_more: bool,
    pub items: Vec<PageArtistSimilarItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistSimilarItem {
    pub id: u64,
    pub name: PageArtistName,
    pub images: Option<PageArtistImages>,
}

/// A group of releases by type (e.g., "album", "ep", "live")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistReleaseGroup {
    #[serde(rename = "type")]
    pub release_type: String,
    pub has_more: bool,
    pub items: Vec<PageArtistRelease>,
}

/// A release item (album/ep/live/etc) from /artist/page or /artist/getReleasesGrid
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistRelease {
    pub id: String,
    pub title: String,
    pub version: Option<String>,
    pub tracks_count: Option<u32>,
    pub artist: Option<PageArtistReleaseArtist>,
    pub artists: Option<Vec<PageArtistReleaseContributor>>,
    pub image: Option<ImageSet>,
    pub label: Option<Label>,
    pub genre: Option<Genre>,
    pub release_type: Option<String>,
    pub release_tags: Option<Vec<String>>,
    pub duration: Option<u32>,
    pub dates: Option<DiscoverAlbumDates>,
    pub parental_warning: Option<bool>,
    pub audio_info: Option<DiscoverAudioInfo>,
    pub rights: Option<PageArtistRights>,
    pub awards: Option<Vec<PageArtistAward>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistReleaseArtist {
    pub id: u64,
    pub name: PageArtistName,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistReleaseContributor {
    pub id: u64,
    pub name: String,
    pub roles: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistRights {
    pub streamable: Option<bool>,
    pub hires_streamable: Option<bool>,
    pub hires_purchasable: Option<bool>,
    pub purchasable: Option<bool>,
    pub downloadable: Option<bool>,
    pub previewable: Option<bool>,
    pub sampleable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistAward {
    pub id: u64,
    pub name: String,
    pub awarded_at: Option<String>,
}

/// Track from /artist/page (top_tracks or tracks_appears_on)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistTrack {
    pub id: u64,
    pub title: String,
    pub version: Option<String>,
    pub duration: Option<u32>,
    pub isrc: Option<String>,
    pub parental_warning: Option<bool>,
    pub artist: Option<PageArtistReleaseArtist>,
    pub composer: Option<serde_json::Value>,
    pub audio_info: Option<DiscoverAudioInfo>,
    pub rights: Option<PageArtistRights>,
    pub physical_support: Option<PageArtistPhysicalSupport>,
    pub album: Option<PageArtistTrackAlbum>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistPhysicalSupport {
    pub media_number: Option<u32>,
    pub track_number: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistTrackAlbum {
    pub id: String,
    pub title: String,
    pub version: Option<String>,
    pub image: Option<ImageSet>,
    pub label: Option<Label>,
    pub genre: Option<Genre>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistPlaylists {
    pub has_more: bool,
    pub items: Vec<PageArtistPlaylist>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistPlaylist {
    pub id: u64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub owner: Option<PageArtistPlaylistOwner>,
    pub tracks_count: Option<u32>,
    pub duration: Option<u32>,
    pub images: Option<PageArtistPlaylistImages>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistPlaylistOwner {
    pub id: u64,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageArtistPlaylistImages {
    pub rectangle: Option<Vec<String>>,
}

/// Response from /artist/getReleasesGrid
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleasesGridResponse {
    pub has_more: bool,
    pub items: Vec<PageArtistRelease>,
}
