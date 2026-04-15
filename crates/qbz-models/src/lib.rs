//! QBZ Models - Shared types, events, and traits
//!
//! This crate provides the foundation for all QBZ crates:
//! - Type definitions (Track, Album, Artist, etc.)
//! - Event definitions (CoreEvent enum)
//! - Trait definitions (FrontendAdapter)
//! - Playback types (QueueTrack, PlaybackState)
//! - Error types
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      qbz-models (Tier 0)                    │
//! │  Types, Events, Traits - No dependencies on other qbz-*    │
//! └─────────────────────────────────────────────────────────────┘
//!                              ↑
//!     ┌────────────────────────┼────────────────────────┐
//!     │                        │                        │
//! ┌───┴───┐              ┌─────┴─────┐            ┌─────┴─────┐
//! │qbz-audio│            │qbz-qobuz  │            │qbz-player │
//! │ Tier 1 │             │  Tier 1   │            │  Tier 2   │
//! └────────┘             └───────────┘            └───────────┘
//! ```
//!
//! # Usage
//!
//! ```rust
//! use qbz_models::{Track, Album, CoreEvent, FrontendAdapter};
//! ```

pub mod error;
pub mod events;
pub mod playback;
pub mod traits;
pub mod types;

// Re-export commonly used types at crate root
pub use error::{QbzError, QbzResult};
pub use events::CoreEvent;
pub use playback::{PlaybackState, PlaybackStatus, QueueState, QueueTrack, RepeatMode};
pub use traits::{FrontendAdapter, LoggingAdapter, NoOpAdapter};
pub use types::{
    Album,
    AlbumSummary,
    Artist,
    ArtistAlbums,
    ArtistBiography,
    // Discover types
    DiscoverAlbum,
    DiscoverAlbumDates,
    DiscoverAlbumImage,
    DiscoverArtist,
    DiscoverAudioInfo,
    DiscoverContainer,
    DiscoverContainers,
    DiscoverData,
    DiscoverPlaylist,
    DiscoverPlaylistImage,
    DiscoverPlaylistsResponse,
    DiscoverResponse,
    Favorites,
    Genre,
    GenreInfo,
    GenreListContainer,
    GenreListResponse,
    ImageSet,
    Label,
    LabelDetail,
    LabelExploreResponse,
    LabelPageContainer,
    LabelPageData,
    LabelPageGenericList,
    // Award types
    AlbumAward,
    AwardMagazine,
    AwardPageContainer,
    AwardPageData,
    AwardPageGenericList,
    // Artist page types
    PageArtistAward,
    PageArtistBiography,
    PageArtistImages,
    PageArtistName,
    PageArtistPhysicalSupport,
    PageArtistPlaylist,
    PageArtistPlaylistImages,
    PageArtistPlaylistOwner,
    PageArtistPlaylists,
    PageArtistPortrait,
    PageArtistRelease,
    PageArtistReleaseArtist,
    PageArtistReleaseContributor,
    PageArtistReleaseGroup,
    PageArtistResponse,
    PageArtistRights,
    PageArtistSimilar,
    PageArtistSimilarItem,
    PageArtistTrack,
    PageArtistTrackAlbum,
    Playlist,
    PlaylistDuplicateResult,
    PlaylistGenre,
    PlaylistOwner,
    PlaylistTag,
    PlaylistTagsResponse,
    PlaylistWithTrackIds,
    Quality,
    RawPlaylistTag,
    ReleasesGridResponse,
    SearchResults,
    SearchResultsPage,
    SessionStartResponse,
    StreamRestriction,
    StreamUrl,
    Track,
    TrackFileUrl,
    TracksContainer,
    UserSession,
};
