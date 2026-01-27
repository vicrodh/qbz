//! Smart Playlist Generation
//!
//! Generates playlists based on MusicBrainz artist relationships.
//! Uses Stage 1 infrastructure to resolve artists and Stage 3 relationships.

use serde::{Deserialize, Serialize};

/// Types of smart playlists that can be generated
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistRule {
    /// All solo work by members of a band
    BandMembers {
        /// The band's MusicBrainz ID
        artist_mbid: String,
        /// Include past members
        include_past_members: bool,
    },
    /// Groups that a solo artist is/was a member of
    ArtistGroups {
        /// The artist's MusicBrainz ID
        artist_mbid: String,
    },
    /// Artists who have collaborated with the seed artist
    CollaboratorNetwork {
        /// Seed artist's MusicBrainz ID
        artist_mbid: String,
        /// How many degrees of separation (1 = direct collaborators only)
        depth: u8,
    },
}

/// A generated smart playlist
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPlaylist {
    /// Suggested name for the playlist
    pub suggested_name: String,
    /// Description of how it was generated
    pub description: String,
    /// The rule used to generate this playlist
    pub rule: PlaylistRule,
    /// Artist names included in this playlist
    pub included_artists: Vec<String>,
    /// Qobuz track IDs found
    pub track_ids: Vec<u64>,
    /// Total tracks found (before any limits)
    pub total_tracks_found: usize,
}

/// Result of searching for an artist's tracks on Qobuz
#[derive(Debug, Clone)]
pub struct ArtistTracksResult {
    pub artist_name: String,
    pub qobuz_artist_id: Option<u64>,
    pub track_ids: Vec<u64>,
}

/// Configuration for playlist generation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistGenerationConfig {
    /// Maximum tracks per related artist
    pub max_tracks_per_artist: usize,
    /// Maximum total tracks in playlist
    pub max_total_tracks: usize,
    /// Only include tracks available in user's region
    pub filter_unavailable: bool,
}

impl Default for PlaylistGenerationConfig {
    fn default() -> Self {
        Self {
            max_tracks_per_artist: 10,
            max_total_tracks: 100,
            filter_unavailable: true,
        }
    }
}
