//! Remote metadata fetching for the Tag Editor
//!
//! This module provides a unified interface for fetching album metadata
//! from MusicBrainz and Discogs. The Tag Editor uses these services to
//! help users fill in metadata for their local library albums.
//!
//! # Providers
//!
//! - **MusicBrainz** (default): Free, community-maintained database.
//!   No authentication required. Uses tags for genres.
//!
//! - **Discogs**: Commercial database with extensive catalog.
//!   Requires API credentials (handled via proxy). Has genres and styles.
//!
//! # Usage
//!
//! 1. Search for albums using `search_albums()`
//! 2. Display results to user
//! 3. Fetch full metadata using `get_album_metadata()`
//! 4. Apply to editor form

mod cache;
mod models;

pub use cache::{CacheStats, RemoteMetadataCache};
pub use models::{
    RemoteAlbumMetadata, RemoteAlbumSearchResult, RemoteMetadataError, RemoteProvider,
    RemoteSearchRequest, RemoteSearchResponse, RemoteTrackMetadata,
};

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::discogs::DiscogsClient;
use crate::musicbrainz::MusicBrainzSharedState;

/// Shared state for remote metadata operations
pub struct RemoteMetadataState {
    /// In-memory cache
    pub cache: RemoteMetadataCache,
    /// MusicBrainz client (shared with existing integration)
    pub musicbrainz: Option<Arc<MusicBrainzSharedState>>,
    /// Discogs client
    pub discogs: Arc<Mutex<DiscogsClient>>,
}

impl RemoteMetadataState {
    pub fn new(musicbrainz: Option<Arc<MusicBrainzSharedState>>) -> Self {
        Self {
            cache: RemoteMetadataCache::new(),
            musicbrainz,
            discogs: Arc::new(Mutex::new(DiscogsClient::new())),
        }
    }
}

// ============ MusicBrainz Adapter ============

/// Convert MusicBrainz release search result to unified DTO
pub fn musicbrainz_release_to_search_result(
    release: &crate::musicbrainz::models::ReleaseResult,
) -> RemoteAlbumSearchResult {
    // Extract artist from artist-credit
    let artist = release
        .artist_credit
        .as_ref()
        .map(|credits| {
            credits
                .iter()
                .map(|c| {
                    format!(
                        "{}{}",
                        c.name.as_deref().unwrap_or(&c.artist.name),
                        c.joinphrase.as_deref().unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    // Extract year from date (YYYY or YYYY-MM-DD)
    let year = release.date.as_ref().and_then(|d| {
        d.split('-').next().and_then(|y| y.parse::<u16>().ok())
    });

    // Extract label and catalog number
    let (label, catalog_number) = release
        .label_info
        .as_ref()
        .and_then(|info| info.first())
        .map(|li| {
            (
                li.label.as_ref().map(|l| l.name.clone()),
                li.catalog_number.clone(),
            )
        })
        .unwrap_or((None, None));

    RemoteAlbumSearchResult {
        provider: RemoteProvider::MusicBrainz,
        provider_id: release.id.clone(),
        title: release.title.clone(),
        artist,
        year,
        track_count: None, // Not available in search results
        country: release.country.clone(),
        label,
        catalog_number,
        confidence: release.score.map(|s| s.min(100) as u8),
        format: None,
    }
}

// ============ Discogs Adapter ============

/// Convert Discogs release search result to unified DTO
pub fn discogs_release_to_search_result(
    result: &crate::discogs::SearchResult,
) -> RemoteAlbumSearchResult {
    // Discogs title format is usually "Artist - Album"
    let (artist, title) = if let Some(pos) = result.title.find(" - ") {
        let (a, t) = result.title.split_at(pos);
        (a.to_string(), t.trim_start_matches(" - ").to_string())
    } else {
        ("Unknown Artist".to_string(), result.title.clone())
    };

    RemoteAlbumSearchResult {
        provider: RemoteProvider::Discogs,
        provider_id: result.id.to_string(),
        title,
        artist,
        year: None, // Not in search result, need to fetch details
        track_count: None,
        country: None,
        label: None,
        catalog_number: None,
        confidence: None,
        format: None,
    }
}

/// Parse Discogs track position to (disc_number, track_number)
/// Handles formats: "1", "A1", "1-1", "CD1-1", "1.1"
pub fn parse_discogs_position(position: &str) -> (u8, u8) {
    let position = position.trim();

    // Handle empty position
    if position.is_empty() {
        return (1, 1);
    }

    // Try "X-Y" format (e.g., "1-5", "CD1-3")
    if let Some(pos) = position.find('-') {
        let disc_part = &position[..pos];
        let track_part = &position[pos + 1..];

        // Extract number from disc part (handle "CD1", "1", etc.)
        let disc = disc_part
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u8>()
            .unwrap_or(1);

        let track = track_part.parse::<u8>().unwrap_or(1);
        return (disc, track);
    }

    // Try "X.Y" format
    if let Some(pos) = position.find('.') {
        let disc_part = &position[..pos];
        let track_part = &position[pos + 1..];

        let disc = disc_part.parse::<u8>().unwrap_or(1);
        let track = track_part.parse::<u8>().unwrap_or(1);
        return (disc, track);
    }

    // Handle vinyl sides (A, B, C, D -> disc 1, 1, 2, 2)
    if position.starts_with(|c: char| c.is_ascii_alphabetic()) {
        let side = position.chars().next().unwrap().to_ascii_uppercase();
        let track_str: String = position.chars().skip(1).collect();
        let track = track_str.parse::<u8>().unwrap_or(1);

        let disc = match side {
            'A' | 'B' => 1,
            'C' | 'D' => 2,
            'E' | 'F' => 3,
            _ => 1,
        };

        return (disc, track);
    }

    // Simple number
    let track = position.parse::<u8>().unwrap_or(1);
    (1, track)
}

/// Parse Discogs duration string to milliseconds
/// Handles format: "M:SS" or "MM:SS" or "H:MM:SS"
pub fn parse_discogs_duration(duration: &str) -> Option<u32> {
    let parts: Vec<&str> = duration.split(':').collect();

    match parts.len() {
        2 => {
            // M:SS or MM:SS
            let minutes: u32 = parts[0].parse().ok()?;
            let seconds: u32 = parts[1].parse().ok()?;
            Some((minutes * 60 + seconds) * 1000)
        }
        3 => {
            // H:MM:SS
            let hours: u32 = parts[0].parse().ok()?;
            let minutes: u32 = parts[1].parse().ok()?;
            let seconds: u32 = parts[2].parse().ok()?;
            Some((hours * 3600 + minutes * 60 + seconds) * 1000)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_discogs_position() {
        assert_eq!(parse_discogs_position("1"), (1, 1));
        assert_eq!(parse_discogs_position("5"), (1, 5));
        assert_eq!(parse_discogs_position("1-1"), (1, 1));
        assert_eq!(parse_discogs_position("1-5"), (1, 5));
        assert_eq!(parse_discogs_position("2-3"), (2, 3));
        assert_eq!(parse_discogs_position("CD1-5"), (1, 5));
        assert_eq!(parse_discogs_position("CD2-3"), (2, 3));
        assert_eq!(parse_discogs_position("A1"), (1, 1));
        assert_eq!(parse_discogs_position("B2"), (1, 2));
        assert_eq!(parse_discogs_position("C1"), (2, 1));
        assert_eq!(parse_discogs_position("D3"), (2, 3));
        assert_eq!(parse_discogs_position("1.5"), (1, 5));
        assert_eq!(parse_discogs_position("2.3"), (2, 3));
    }

    #[test]
    fn test_parse_discogs_duration() {
        assert_eq!(parse_discogs_duration("3:45"), Some(225000));
        assert_eq!(parse_discogs_duration("0:30"), Some(30000));
        assert_eq!(parse_discogs_duration("10:00"), Some(600000));
        assert_eq!(parse_discogs_duration("1:00:00"), Some(3600000));
        assert_eq!(parse_discogs_duration("invalid"), None);
    }
}
