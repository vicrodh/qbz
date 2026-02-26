//! ListenBrainz API models
//!
//! Types for ListenBrainz submission payloads and responses

use serde::{Deserialize, Serialize};

/// Listen type for submission
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenType {
    /// Currently playing track
    PlayingNow,
    /// Single scrobble
    Single,
}

/// A single listen submission
#[derive(Debug, Clone, Serialize)]
pub struct Listen {
    /// Unix timestamp (omit for playing_now)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listened_at: Option<i64>,
    /// Track metadata
    pub track_metadata: TrackMetadata,
}

/// Track metadata for a listen
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMetadata {
    /// Artist name (required)
    pub artist_name: String,
    /// Track name (required)
    pub track_name: String,
    /// Release/album name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_name: Option<String>,
    /// Additional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_info: Option<AdditionalInfo>,
}

/// Additional track info for richer scrobbles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdditionalInfo {
    /// MusicBrainz recording ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_mbid: Option<String>,
    /// MusicBrainz release ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_mbid: Option<String>,
    /// MusicBrainz artist IDs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist_mbids: Option<Vec<String>>,
    /// ISRC code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    /// Track duration in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Track number on release
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracknumber: Option<u32>,
    /// Media player name
    pub media_player: String,
    /// Media player version
    pub media_player_version: String,
    /// Submission client name
    pub submission_client: String,
    /// Submission client version
    pub submission_client_version: String,
}

impl AdditionalInfo {
    /// Create new AdditionalInfo with QBZ identifiers
    pub fn new() -> Self {
        Self {
            recording_mbid: None,
            release_mbid: None,
            artist_mbids: None,
            isrc: None,
            duration_ms: None,
            tracknumber: None,
            media_player: "QBZ".to_string(),
            media_player_version: "1.0.0".to_string(),
            submission_client: "QBZ".to_string(),
            submission_client_version: "1.0.0".to_string(),
        }
    }

    /// Set version info (call this from your app with actual version)
    pub fn with_version(mut self, version: &str) -> Self {
        self.media_player_version = version.to_string();
        self.submission_client_version = version.to_string();
        self
    }
}

impl Default for AdditionalInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// Payload for submitting listens
#[derive(Debug, Clone, Serialize)]
pub struct SubmitListensPayload {
    pub listen_type: ListenType,
    pub payload: Vec<Listen>,
}

/// User info response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserInfo {
    pub user_name: String,
}

/// Token validation response
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct TokenValidationResponse {
    pub code: i32,
    pub message: String,
    pub valid: bool,
    #[serde(default)]
    pub user_name: Option<String>,
}

/// ListenBrainz connection status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenBrainzStatus {
    pub connected: bool,
    pub user_name: Option<String>,
    pub enabled: bool,
}

/// Queued listen for offline submission
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedListen {
    pub id: i64,
    pub listened_at: i64,
    pub artist_name: String,
    pub track_name: String,
    pub release_name: Option<String>,
    pub recording_mbid: Option<String>,
    pub release_mbid: Option<String>,
    pub artist_mbids: Option<Vec<String>>,
    pub isrc: Option<String>,
    pub duration_ms: Option<u64>,
    pub created_at: i64,
    pub attempts: i32,
    pub sent: bool,
}
