//! Last.fm API client

use reqwest::Client;
use serde_json::json;

use crate::error::{IntegrationError, IntegrationResult};
use super::models::{AuthGetSessionResponse, AuthGetTokenResponse, LastFmResponse, LastFmSession, LastFmSimilarArtist};

/// Cloudflare Workers proxy URL - handles API credentials and signature generation
const LASTFM_PROXY_URL: &str = "https://qbz-api-proxy.blitzkriegfc.workers.dev/lastfm";

/// Last.fm API client
///
/// Uses Cloudflare Workers proxy to handle API credentials and signature generation.
/// This means the client doesn't need to know the API key or secret.
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
    /// Create a new Last.fm client
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

    /// Create a client with an existing session key
    pub fn with_session_key(session_key: String) -> Self {
        let mut client = Self::new();
        client.session_key = Some(session_key);
        client
    }

    /// Set the session key (for restoring a saved session)
    pub fn set_session_key(&mut self, key: String) {
        self.session_key = Some(key);
    }

    /// Get the current session key
    pub fn session_key(&self) -> Option<&str> {
        self.session_key.as_deref()
    }

    /// Check if authenticated
    pub fn is_authenticated(&self) -> bool {
        self.session_key.is_some()
    }

    /// Clear the session (logout)
    pub fn clear_session(&mut self) {
        self.session_key = None;
    }

    /// Get a request token and authorization URL for authentication
    ///
    /// Returns: (token, auth_url)
    ///
    /// The user should be directed to auth_url to authorize the application.
    /// Once authorized, call `get_session` with the token to complete authentication.
    pub async fn get_token(&self) -> IntegrationResult<(String, String)> {
        let url = format!("{}/auth.getToken", LASTFM_PROXY_URL);

        let response = self
            .client
            .post(&url)
            .json(&json!({}))
            .send()
            .await?;

        let data: LastFmResponse<AuthGetTokenResponse> = response.json().await?;

        match data {
            LastFmResponse::Success(r) => {
                let auth_url = r.auth_url.unwrap_or_else(|| {
                    format!("https://www.last.fm/api/auth/?token={}", r.token)
                });
                Ok((r.token, auth_url))
            }
            LastFmResponse::Error { error, message } => {
                Err(IntegrationError::api(error, message))
            }
        }
    }

    /// Get session key after user has authorized
    ///
    /// Call this after the user has visited the auth_url from `get_token`.
    pub async fn get_session(&mut self, token: &str) -> IntegrationResult<LastFmSession> {
        log::info!(
            "Getting Last.fm session with token: {}...",
            &token[..token.len().min(8)]
        );

        let url = format!("{}/auth.getSession", LASTFM_PROXY_URL);

        let response = self
            .client
            .post(&url)
            .json(&json!({ "token": token }))
            .send()
            .await?;

        let response_text = response.text().await?;
        log::debug!("Last.fm auth.getSession response: {}", response_text);

        let data: LastFmResponse<AuthGetSessionResponse> = serde_json::from_str(&response_text)?;

        match data {
            LastFmResponse::Success(r) => {
                log::info!("Last.fm session obtained for user: {}", r.session.name);
                self.session_key = Some(r.session.key.clone());
                Ok(r.session)
            }
            LastFmResponse::Error { error, message } => {
                log::error!("Last.fm auth error {}: {}", error, message);
                Err(IntegrationError::api(error, message))
            }
        }
    }

    /// Scrobble a track (mark as played)
    ///
    /// Requires authentication.
    pub async fn scrobble(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
        timestamp: u64,
    ) -> IntegrationResult<()> {
        let session_key = self
            .session_key
            .as_ref()
            .ok_or(IntegrationError::NotAuthenticated)?;

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

        let response = self.client.post(&url).json(&body).send().await?;

        if response.status().is_success() {
            log::info!("Scrobbled: {} - {}", artist, track);
            Ok(())
        } else {
            let text = response.text().await.unwrap_or_default();
            Err(IntegrationError::internal(format!("Scrobble failed: {}", text)))
        }
    }

    /// Update "now playing" status
    ///
    /// Requires authentication.
    pub async fn update_now_playing(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
    ) -> IntegrationResult<()> {
        let session_key = self
            .session_key
            .as_ref()
            .ok_or(IntegrationError::NotAuthenticated)?;

        let url = format!("{}/track.updateNowPlaying", LASTFM_PROXY_URL);

        let mut body = json!({
            "sk": session_key,
            "artist": artist,
            "track": track,
        });

        if let Some(album_name) = album {
            body["album"] = json!(album_name);
        }

        let response = self.client.post(&url).json(&body).send().await?;

        if response.status().is_success() {
            log::debug!("Updated now playing: {} - {}", artist, track);
            Ok(())
        } else {
            let text = response.text().await.unwrap_or_default();
            Err(IntegrationError::internal(format!(
                "Update now playing failed: {}",
                text
            )))
        }
    }

    /// Get similar artists for a given artist name
    ///
    /// Uses Last.fm's artist.getSimilar which returns genre-accurate similarity.
    /// Requires authentication (user must have Last.fm connected).
    pub async fn get_similar_artists(
        &self,
        artist: &str,
        limit: u32,
    ) -> IntegrationResult<Vec<LastFmSimilarArtist>> {
        // artist.getSimilar is a public read endpoint - no session key needed.
        // The proxy handles the API key.
        let url = format!("{}/artist.getSimilar", LASTFM_PROXY_URL);

        let response = self
            .client
            .post(&url)
            .json(&json!({
                "artist": artist,
                "limit": limit,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(IntegrationError::internal(format!(
                "Last.fm artist.getSimilar failed: {}",
                text
            )));
        }

        let text = response.text().await?;

        let data: serde_json::Value = serde_json::from_str(&text)?;

        // Handle Last.fm error responses
        if let Some(error) = data.get("error") {
            let message = data
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(IntegrationError::api(
                error.as_u64().unwrap_or(0) as u32,
                message.to_string(),
            ));
        }

        let artists = data
            .get("similarartists")
            .and_then(|sa| sa.get("artist"))
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let name = item.get("name")?.as_str()?.to_string();
                        // Last.fm returns match as string "0" to "1"
                        let match_score: f64 = item
                            .get("match")
                            .and_then(|m| {
                                m.as_str()
                                    .and_then(|s| s.parse().ok())
                                    .or_else(|| m.as_f64())
                            })
                            .unwrap_or(0.0);
                        let mbid = item
                            .get("mbid")
                            .and_then(|m| m.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string());

                        Some(LastFmSimilarArtist {
                            name,
                            match_score,
                            mbid,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(artists)
    }
}
