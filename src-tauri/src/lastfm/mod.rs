//! Last.fm integration module
//!
//! Handles Last.fm authentication and scrobbling

use md5::{Digest, Md5};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const LASTFM_API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

// Last.fm API credentials
// Set via environment variables at build time, or users can enter their own
// To set: LASTFM_API_KEY=xxx LASTFM_API_SECRET=yyy cargo build
const DEFAULT_API_KEY: Option<&str> = option_env!("LASTFM_API_KEY");
const DEFAULT_API_SECRET: Option<&str> = option_env!("LASTFM_API_SECRET");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastFmSession {
    pub name: String,
    pub key: String,
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
    api_secret: String,
    session_key: Option<String>,
}

impl Default for LastFmClient {
    fn default() -> Self {
        Self::new(
            DEFAULT_API_KEY.unwrap_or("").to_string(),
            DEFAULT_API_SECRET.unwrap_or("").to_string(),
        )
    }
}

impl LastFmClient {
    /// Check if embedded (build-time) credentials are available
    pub fn has_embedded_credentials() -> bool {
        DEFAULT_API_KEY.is_some() && DEFAULT_API_SECRET.is_some()
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

        let mut params = BTreeMap::new();
        params.insert("method", "auth.getSession");
        params.insert("api_key", &self.api_key);
        params.insert("token", token);

        let sig = self.generate_signature(&params);

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

        let data: LastFmResponse<AuthGetSessionResponse> = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        match data {
            LastFmResponse::Success(r) => {
                self.session_key = Some(r.session.key.clone());
                Ok(r.session)
            }
            LastFmResponse::Error { message, .. } => Err(message),
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
