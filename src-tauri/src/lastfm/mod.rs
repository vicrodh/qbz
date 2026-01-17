//! Last.fm integration module
//!
//! Handles Last.fm authentication and scrobbling via Cloudflare Workers proxy

use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;

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

// Cloudflare Workers proxy URL - handles API credentials and signature generation
const LASTFM_PROXY_URL: &str = "https://qbz-api-proxy.blitzkriegfc.workers.dev/lastfm";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastFmSession {
    pub name: String,
    pub key: String,
    #[serde(deserialize_with = "deserialize_int_bool")]
    pub subscriber: bool,
}

#[derive(Debug, Deserialize)]
struct AuthGetSessionResponse {
    session: LastFmSession,
}

#[derive(Debug, Deserialize)]
struct AuthGetTokenResponse {
    token: String,
    #[serde(rename = "authUrl")]
    auth_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LastFmResponse<T> {
    Success(T),
    Error { error: u32, message: String },
}

/// Last.fm API client
/// Uses Cloudflare Workers proxy to handle API credentials and signature generation
pub struct LastFmClient {
    client: Client,
    session_key: Option<String>,
}

impl Default for LastFmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LastFmClient {
    /// Check if embedded credentials are available (always true - proxy handles them)
    pub fn has_embedded_credentials() -> bool {
        true
    }

    pub fn new() -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("QBZ/1.0.0"),
        );

        Self {
            client: Client::builder()
                .default_headers(headers)
                .build()
                .unwrap_or_else(|_| Client::new()),
            session_key: None,
        }
    }

    pub fn set_session_key(&mut self, key: String) {
        self.session_key = Some(key);
    }

    pub fn is_authenticated(&self) -> bool {
        self.session_key.is_some()
    }

    /// Compatibility: proxy handles credentials, this is a no-op
    pub fn has_credentials(&self) -> bool {
        true
    }

    /// Compatibility: proxy handles credentials, this is a no-op
    pub fn set_credentials(&mut self, _api_key: String, _api_secret: String) {
        // No-op: proxy handles credentials
    }

    /// Get a request token and authorization URL for authentication
    /// Returns: (token, auth_url)
    pub async fn get_token(&self) -> Result<(String, String), String> {
        let url = format!("{}/auth.getToken", LASTFM_PROXY_URL);

        let response = self
            .client
            .post(&url)
            .json(&json!({}))
            .send()
            .await
            .map_err(|e| format!("Failed to get token: {}", e))?;

        let data: LastFmResponse<AuthGetTokenResponse> = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        match data {
            LastFmResponse::Success(r) => {
                let auth_url = r.auth_url.unwrap_or_else(|| {
                    format!("https://www.last.fm/api/auth/?token={}", r.token)
                });
                Ok((r.token, auth_url))
            }
            LastFmResponse::Error { message, .. } => Err(message),
        }
    }

    /// Get session key after user has authorized
    pub async fn get_session(&mut self, token: &str) -> Result<LastFmSession, String> {
        log::info!("Getting Last.fm session with token: {}...", &token[..token.len().min(8)]);

        let url = format!("{}/auth.getSession", LASTFM_PROXY_URL);

        let response = self
            .client
            .post(&url)
            .json(&json!({ "token": token }))
            .send()
            .await
            .map_err(|e| format!("Failed to get session: {}", e))?;

        let response_text = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        log::debug!("Last.fm auth.getSession response: {}", response_text);

        let data: LastFmResponse<AuthGetSessionResponse> = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse response: {} - Raw: {}", e, response_text))?;

        match data {
            LastFmResponse::Success(r) => {
                log::info!("Last.fm session obtained for user: {}", r.session.name);
                self.session_key = Some(r.session.key.clone());
                Ok(r.session)
            }
            LastFmResponse::Error { error, message } => {
                log::error!("Last.fm auth error {}: {}", error, message);
                Err(message)
            }
        }
    }

    /// Scrobble a track (mark as played)
    pub async fn scrobble(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
        timestamp: u64,
    ) -> Result<(), String> {
        let session_key = self
            .session_key
            .as_ref()
            .ok_or("Not authenticated with Last.fm")?;

        let url = format!("{}/track.scrobble", LASTFM_PROXY_URL);

        let mut body = json!({
            "sk": session_key,
            "artist": artist,
            "track": track,
            "timestamp": timestamp.to_string(),
        });

        if let Some(album_name) = album {
            body["album"] = json!(album_name);
        }

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to scrobble: {}", e))?;

        if response.status().is_success() {
            log::info!("Scrobbled: {} - {}", artist, track);
            Ok(())
        } else {
            let text = response.text().await.unwrap_or_default();
            Err(format!("Scrobble failed: {}", text))
        }
    }

    /// Update "now playing" status
    pub async fn update_now_playing(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
    ) -> Result<(), String> {
        let session_key = self
            .session_key
            .as_ref()
            .ok_or("Not authenticated with Last.fm")?;

        let url = format!("{}/track.updateNowPlaying", LASTFM_PROXY_URL);

        let mut body = json!({
            "sk": session_key,
            "artist": artist,
            "track": track,
        });

        if let Some(album_name) = album {
            body["album"] = json!(album_name);
        }

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to update now playing: {}", e))?;

        if response.status().is_success() {
            log::debug!("Updated now playing: {} - {}", artist, track);
            Ok(())
        } else {
            let text = response.text().await.unwrap_or_default();
            Err(format!("Update now playing failed: {}", text))
        }
    }
}
