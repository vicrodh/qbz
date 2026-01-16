//! Last.fm integration module
//!
//! Handles Last.fm authentication and scrobbling

use md5::{Digest, Md5};
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

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

const LASTFM_API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

// Last.fm API credentials
// Checked in order:
// 1. Compile-time environment variables (for release builds)
// 2. Runtime environment variables (for development with .env)
// Note: Supports both LAST_FM_* and LASTFM_* naming conventions
const DEFAULT_API_KEY: Option<&str> = option_env!("LAST_FM_API_KEY");
const DEFAULT_API_SECRET: Option<&str> = option_env!("LAST_FM_API_SHARED_SECRET");

/// Get API key from compile-time or runtime environment
fn get_api_key() -> Option<String> {
    DEFAULT_API_KEY
        .map(String::from)
        .or_else(|| std::env::var("LAST_FM_API_KEY").ok())
        .or_else(|| std::env::var("LASTFM_API_KEY").ok())
}

/// Get API secret from compile-time or runtime environment
fn get_api_secret() -> Option<String> {
    DEFAULT_API_SECRET
        .map(String::from)
        .or_else(|| std::env::var("LAST_FM_API_SHARED_SECRET").ok())
        .or_else(|| std::env::var("LASTFM_API_SECRET").ok())
}

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
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LastFmResponse<T> {
    Success(T),
    Error { error: u32, message: String },
}

/// Last.fm API client
pub struct LastFmClient {
    client: Client,
    api_key: String,
    session_key: Option<String>,
}

impl Default for LastFmClient {
    fn default() -> Self {
        Self::new(
            get_api_key().unwrap_or_default(),
            get_api_secret().unwrap_or_default(),
        )
    }
}

impl LastFmClient {
    /// Check if embedded (build-time or runtime) credentials are available
    pub fn has_embedded_credentials() -> bool {
        get_api_key().is_some() && get_api_secret().is_some()
    }
}

impl LastFmClient {
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            api_secret,
            session_key: None,
        }
    }

    pub fn set_session_key(&mut self, key: String) {
        self.session_key = Some(key);
    }

    pub fn is_authenticated(&self) -> bool {
        self.session_key.is_some() && !self.api_key.is_empty()
    }

    pub fn has_credentials(&self) -> bool {
        !self.api_key.is_empty() && !self.api_secret.is_empty()
    }

    pub fn set_credentials(&mut self, api_key: String, api_secret: String) {
        self.api_key = api_key;
        self.api_secret = api_secret;
    }

    /// Generate API signature for authenticated requests
    fn generate_signature(&self, params: &BTreeMap<&str, &str>) -> String {
        let mut sig_string = String::new();
        for (key, value) in params {
            sig_string.push_str(key);
            sig_string.push_str(value);
        }
        sig_string.push_str(&self.api_secret);

        let mut hasher = Md5::new();
        hasher.update(sig_string.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Get authorization URL for user to visit
    pub fn get_auth_url(&self, token: &str) -> String {
        format!(
            "https://www.last.fm/api/auth/?api_key={}&token={}",
            self.api_key, token
        )
    }

    /// Get a request token for authentication
    pub async fn get_token(&self) -> Result<String, String> {
        if !self.has_credentials() {
            return Err("Last.fm API credentials not configured".to_string());
        }

        let mut params = BTreeMap::new();
        params.insert("method", "auth.getToken");
        params.insert("api_key", &self.api_key);

        let sig = self.generate_signature(&params);

        let response = self
            .client
            .get(LASTFM_API_URL)
            .query(&[
                ("method", "auth.getToken"),
                ("api_key", &self.api_key),
                ("api_sig", &sig),
                ("format", "json"),
            ])
            .send()
            .await
            .map_err(|e| format!("Failed to get token: {}", e))?;

        let data: LastFmResponse<AuthGetTokenResponse> = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        match data {
            LastFmResponse::Success(r) => Ok(r.token),
            LastFmResponse::Error { message, .. } => Err(message),
        }
    }

    /// Get session key after user has authorized
    pub async fn get_session(&mut self, token: &str) -> Result<LastFmSession, String> {
        if !self.has_credentials() {
            return Err("Last.fm API credentials not configured".to_string());
        }

        log::info!("Getting Last.fm session with token: {}...", &token[..token.len().min(8)]);

        let mut params = BTreeMap::new();
        params.insert("method", "auth.getSession");
        params.insert("api_key", &self.api_key);
        params.insert("token", token);

        let sig = self.generate_signature(&params);
        log::debug!("Generated signature for auth.getSession");

        let response = self
            .client
            .get(LASTFM_API_URL)
            .query(&[
                ("method", "auth.getSession"),
                ("api_key", &self.api_key),
                ("token", token),
                ("api_sig", &sig),
                ("format", "json"),
            ])
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

        let timestamp_str = timestamp.to_string();
        let api_key = &self.api_key;

        let mut params = BTreeMap::new();
        params.insert("method", "track.scrobble");
        params.insert("api_key", api_key.as_str());
        params.insert("sk", session_key.as_str());
        params.insert("artist", artist);
        params.insert("track", track);
        params.insert("timestamp", &timestamp_str);
        if let Some(album_name) = album {
            params.insert("album", album_name);
        }

        let sig = self.generate_signature(&params);

        let mut form_params = vec![
            ("method", "track.scrobble"),
            ("api_key", api_key.as_str()),
            ("sk", session_key.as_str()),
            ("artist", artist),
            ("track", track),
            ("timestamp", &timestamp_str),
            ("api_sig", &sig),
            ("format", "json"),
        ];
        if let Some(album_name) = album {
            form_params.push(("album", album_name));
        }

        let response = self
            .client
            .post(LASTFM_API_URL)
            .form(&form_params)
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

        let api_key = &self.api_key;

        let mut params = BTreeMap::new();
        params.insert("method", "track.updateNowPlaying");
        params.insert("api_key", api_key.as_str());
        params.insert("sk", session_key.as_str());
        params.insert("artist", artist);
        params.insert("track", track);
        if let Some(album_name) = album {
            params.insert("album", album_name);
        }

        let sig = self.generate_signature(&params);

        let mut form_params = vec![
            ("method", "track.updateNowPlaying"),
            ("api_key", api_key.as_str()),
            ("sk", session_key.as_str()),
            ("artist", artist),
            ("track", track),
            ("api_sig", &sig),
            ("format", "json"),
        ];
        if let Some(album_name) = album {
            form_params.push(("album", album_name));
        }

        let response = self
            .client
            .post(LASTFM_API_URL)
            .form(&form_params)
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
