//! Last.fm data models

use serde::{Deserialize, Deserializer, Serialize};

/// Deserialize integer (0/1) as boolean - Last.fm API returns subscriber as number
fn deserialize_int_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::Bool(b) => Ok(b),
        serde_json::Value::Number(n) => Ok(n.as_i64().unwrap_or(0) != 0),
        _ => Ok(false),
    }
}

/// Last.fm session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastFmSession {
    /// Username on Last.fm
    pub name: String,
    /// Session key for API calls
    pub key: String,
    /// Whether user is a subscriber
    #[serde(deserialize_with = "deserialize_int_bool")]
    pub subscriber: bool,
}

/// Response from auth.getSession
#[derive(Debug, Deserialize)]
pub(crate) struct AuthGetSessionResponse {
    pub session: LastFmSession,
}

/// Response from auth.getToken
#[derive(Debug, Deserialize)]
pub(crate) struct AuthGetTokenResponse {
    pub token: String,
    #[serde(rename = "authUrl")]
    pub auth_url: Option<String>,
}

/// Last.fm API response wrapper (success or error)
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum LastFmResponse<T> {
    Success(T),
    Error { error: u32, message: String },
}
