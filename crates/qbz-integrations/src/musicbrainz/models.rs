//! MusicBrainz API response models
//!
//! Types for deserializing MusicBrainz JSON responses

use serde::{Deserialize, Serialize};

/// Match confidence levels for MusicBrainz lookups
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchConfidence {
    Exact,  // ISRC/UPC exact match
    High,   // Score >= 95
    Medium, // Score >= 80
    Low,    // Score >= 60
    None,   // No match found
}

impl MatchConfidence {
    pub fn from_score(score: Option<i32>) -> Self {
        match score {
            Some(s) if s >= 100 => Self::Exact,
            Some(s) if s >= 95 => Self::High,
            Some(s) if s >= 80 => Self::Medium,
            Some(s) if s >= 60 => Self::Low,
            _ => Self::None,
        }
    }
}

/// Artist type classification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtistType {
    Person,
    Group,
    Orchestra,
    Choir,
    Character,
    Other,
}

impl Default for ArtistType {
    fn default() -> Self {
        Self::Other
    }
}

impl From<Option<&str>> for ArtistType {
    fn from(s: Option<&str>) -> Self {
        match s.map(|s| s.to_lowercase()).as_deref() {
            Some("person") => Self::Person,
            Some("group") => Self::Group,
            Some("orchestra") => Self::Orchestra,
            Some("choir") => Self::Choir,
            Some("character") => Self::Character,
            _ => Self::Other,
        }
    }
}

// ============ API Response Types ============

/// Recording search response
#[derive(Debug, Deserialize)]
pub struct RecordingSearchResponse {
    pub created: Option<String>,
    pub count: i32,
    pub offset: i32,
    pub recordings: Vec<RecordingResult>,
}

/// Single recording in search results
#[derive(Debug, Deserialize)]
pub struct RecordingResult {
    pub id: String,
    pub score: Option<i32>,
    pub title: Option<String>,
    pub length: Option<i64>,
    #[serde(rename = "artist-credit")]
    pub artist_credit: Option<Vec<ArtistCredit>>,
    pub isrcs: Option<Vec<String>>,
    pub releases: Option<Vec<ReleaseRef>>,
}

/// Artist credit entry
#[derive(Debug, Deserialize)]
pub struct ArtistCredit {
    pub name: Option<String>,
    pub joinphrase: Option<String>,
    pub artist: ArtistRef,
}

/// Reference to an artist
#[derive(Debug, Deserialize)]
pub struct ArtistRef {
    pub id: String,
    pub name: String,
    #[serde(rename = "sort-name")]
    pub sort_name: Option<String>,
    pub disambiguation: Option<String>,
}

/// Reference to a release (album)
#[derive(Debug, Deserialize)]
pub struct ReleaseRef {
    pub id: String,
    pub title: Option<String>,
    pub status: Option<String>,
    pub date: Option<String>,
    pub country: Option<String>,
    #[serde(rename = "release-group")]
    pub release_group: Option<ReleaseGroupRef>,
}

/// Reference to a release group
#[derive(Debug, Deserialize)]
pub struct ReleaseGroupRef {
    pub id: String,
    #[serde(rename = "primary-type")]
    pub primary_type: Option<String>,
}

/// Artist search response
#[derive(Debug, Deserialize)]
pub struct ArtistSearchResponse {
    pub created: Option<String>,
    pub count: i32,
    pub offset: i32,
    pub artists: Vec<ArtistResult>,
}

/// Single artist in search results
#[derive(Debug, Deserialize)]
pub struct ArtistResult {
    pub id: String,
    pub score: Option<i32>,
    pub name: String,
    #[serde(rename = "sort-name")]
    pub sort_name: Option<String>,
    #[serde(rename = "type")]
    pub artist_type: Option<String>,
    pub country: Option<String>,
    pub disambiguation: Option<String>,
    #[serde(rename = "life-span")]
    pub life_span: Option<LifeSpan>,
}

/// Life span for an artist
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LifeSpan {
    pub begin: Option<String>,
    pub end: Option<String>,
    pub ended: Option<bool>,
}

/// Relation between entities
#[derive(Debug, Deserialize)]
pub struct Relation {
    #[serde(rename = "type")]
    pub relation_type: String,
    #[serde(rename = "type-id")]
    pub type_id: Option<String>,
    pub direction: Option<String>,
    pub begin: Option<String>,
    pub end: Option<String>,
    pub ended: Option<bool>,
    pub attributes: Option<Vec<String>>,
    pub artist: Option<ArtistRef>,
}

// ============ Resolved Types (for caching/output) ============

/// Resolved artist with all metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedArtist {
    pub mbid: String,
    pub name: String,
    pub sort_name: Option<String>,
    pub artist_type: ArtistType,
    pub country: Option<String>,
    pub disambiguation: Option<String>,
    pub confidence: MatchConfidence,
}

/// Resolved track with MusicBrainz data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedTrack {
    pub recording_mbid: String,
    pub title: String,
    pub artist_mbids: Vec<String>,
    pub release_mbid: Option<String>,
    pub isrcs: Vec<String>,
    pub confidence: MatchConfidence,
}

/// Resolved release (album) with MusicBrainz data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRelease {
    pub mbid: String,
    pub title: String,
    pub release_group_mbid: Option<String>,
    pub date: Option<String>,
    pub country: Option<String>,
    pub confidence: MatchConfidence,
}
