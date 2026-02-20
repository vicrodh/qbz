//! Data models for Odesli/song.link integration

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Response from Odesli API
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OdesliResponse {
    /// The unique ID for the input entity that was supplied in the request
    pub entity_unique_id: Option<String>,

    /// The userCountry query param that was supplied in the request
    pub user_country: Option<String>,

    /// The main song.link page URL
    pub page_url: String,

    /// A map of platform names to their link info
    #[serde(default)]
    pub links_by_platform: HashMap<String, PlatformLink>,

    /// A map of entity unique IDs to entity info
    #[serde(default)]
    pub entities_by_unique_id: HashMap<String, Entity>,
}

/// Link info for a specific platform
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformLink {
    /// The country code for this link
    pub country: Option<String>,

    /// The URL to this entity on the platform
    pub url: String,

    /// The native app URI
    pub native_app_uri_mobile: Option<String>,
    pub native_app_uri_desktop: Option<String>,

    /// The unique ID for this entity
    pub entity_unique_id: Option<String>,
}

/// Entity info from Odesli
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    /// The unique ID (can be a string or number in the API response)
    #[serde(deserialize_with = "deserialize_string_or_number")]
    pub id: String,

    /// Type: "song", "album"
    #[serde(rename = "type")]
    pub entity_type: Option<String>,

    /// Title
    pub title: Option<String>,

    /// Artist name
    pub artist_name: Option<String>,

    /// Thumbnail URL
    pub thumbnail_url: Option<String>,

    /// Thumbnail dimensions
    pub thumbnail_width: Option<u32>,
    pub thumbnail_height: Option<u32>,

    /// API provider
    pub api_provider: Option<String>,

    /// Platforms this entity is available on
    pub platforms: Option<Vec<String>>,
}

/// Simplified response for frontend consumption
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongLinkResponse {
    /// The main song.link URL to share
    pub page_url: String,

    /// Title of the content (if available)
    pub title: Option<String>,

    /// Artist name (if available)
    pub artist: Option<String>,

    /// Thumbnail URL (if available)
    pub thumbnail_url: Option<String>,

    /// Map of platform names to their direct URLs
    pub platforms: HashMap<String, String>,

    /// The identifier used (ISRC or UPC)
    pub identifier: String,

    /// Type of content: "track" or "album"
    pub content_type: String,
}

/// Content type for sharing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Track,
    Album,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentType::Track => "track",
            ContentType::Album => "album",
        }
    }
}

/// Deserialize a JSON value that may be a string or a number into a String.
/// Bandcamp's Odesli entities return numeric IDs while others return strings.
fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrNumber;

    impl<'de> de::Visitor<'de> for StringOrNumber {
        type Value = String;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or number")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_string())
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }

    deserializer.deserialize_any(StringOrNumber)
}
