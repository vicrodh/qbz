//! ListenBrainz API client
//!
//! Direct client for ListenBrainz submissions (no proxy needed - uses user token)

use reqwest::Client;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::{IntegrationError, IntegrationResult};
use super::models::*;

/// ListenBrainz API base URL
const LISTENBRAINZ_API_URL: &str = "https://api.listenbrainz.org/1";

/// ListenBrainz client configuration
#[derive(Debug, Clone)]
pub struct ListenBrainzConfig {
    /// Whether ListenBrainz integration is enabled
    pub enabled: bool,
    /// User token from listenbrainz.org
    pub token: Option<String>,
    /// Username (set after token validation)
    pub user_name: Option<String>,
}

impl Default for ListenBrainzConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            token: None,
            user_name: None,
        }
    }
}

/// ListenBrainz API client
pub struct ListenBrainzClient {
    client: Client,
    config: Arc<Mutex<ListenBrainzConfig>>,
    version: String,
}

impl Default for ListenBrainzClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ListenBrainzClient {
    /// Create a new ListenBrainz client
    pub fn new() -> Self {
        Self::with_config(ListenBrainzConfig::default())
    }

    /// Create client with specific configuration
    pub fn with_config(config: ListenBrainzConfig) -> Self {
        let version = "1.0.0".to_string();
        let user_agent = format!(
            "QBZ/{} (https://github.com/vicrodh/qbz; qbz@vicrodh.dev)",
            version
        );

        let client = Client::builder()
            .user_agent(&user_agent)
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            config: Arc::new(Mutex::new(config)),
            version,
        }
    }

    /// Set the application version for submission metadata
    pub fn set_version(&mut self, version: impl Into<String>) {
        self.version = version.into();
    }

    /// Check if ListenBrainz integration is enabled
    pub async fn is_enabled(&self) -> bool {
        self.config.lock().await.enabled
    }

    /// Enable or disable ListenBrainz integration
    pub async fn set_enabled(&self, enabled: bool) {
        self.config.lock().await.enabled = enabled;
    }

    /// Check if authenticated (has valid token)
    pub async fn is_authenticated(&self) -> bool {
        let config = self.config.lock().await;
        config.token.is_some() && config.user_name.is_some()
    }

    /// Get current status
    pub async fn get_status(&self) -> ListenBrainzStatus {
        let config = self.config.lock().await;
        ListenBrainzStatus {
            connected: config.token.is_some() && config.user_name.is_some(),
            user_name: config.user_name.clone(),
            enabled: config.enabled,
        }
    }

    /// Set user token and validate it
    pub async fn set_token(&self, token: &str) -> IntegrationResult<UserInfo> {
        // Validate token first
        let validation = self.validate_token(token).await?;

        if !validation.valid {
            return Err(IntegrationError::AuthFailed(validation.message));
        }

        let user_name = validation
            .user_name
            .ok_or_else(|| IntegrationError::AuthFailed("Token valid but no username returned".into()))?;

        // Store validated token and username
        {
            let mut config = self.config.lock().await;
            config.token = Some(token.to_string());
            config.user_name = Some(user_name.clone());
        }

        log::info!("ListenBrainz connected");

        Ok(UserInfo { user_name })
    }

    /// Restore token from saved session (without re-validating)
    pub async fn restore_token(&self, token: String, user_name: String) {
        let mut config = self.config.lock().await;
        config.token = Some(token);
        config.user_name = Some(user_name);
    }

    /// Get current token (for persistence)
    pub async fn get_token(&self) -> Option<String> {
        self.config.lock().await.token.clone()
    }

    /// Get current username
    pub async fn get_user_name(&self) -> Option<String> {
        self.config.lock().await.user_name.clone()
    }

    /// Disconnect (clear token)
    pub async fn disconnect(&self) {
        let mut config = self.config.lock().await;
        config.token = None;
        config.user_name = None;
        log::info!("ListenBrainz disconnected");
    }

    /// Validate a token with ListenBrainz API
    async fn validate_token(&self, token: &str) -> IntegrationResult<TokenValidationResponse> {
        let url = format!("{}/validate-token", LISTENBRAINZ_API_URL);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Token {}", token))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(IntegrationError::AuthFailed(format!(
                "Token validation failed: {} - {}",
                status, text
            )));
        }

        response.json::<TokenValidationResponse>().await.map_err(Into::into)
    }

    /// Submit "now playing" notification
    pub async fn submit_playing_now(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
        additional_info: Option<AdditionalInfo>,
    ) -> IntegrationResult<()> {
        let token = {
            let config = self.config.lock().await;
            if !config.enabled {
                return Ok(()); // Silently skip if disabled
            }
            config.token.clone()
        };

        let token = token.ok_or(IntegrationError::NotAuthenticated)?;

        let info = self.prepare_additional_info(additional_info);

        let payload = SubmitListensPayload {
            listen_type: ListenType::PlayingNow,
            payload: vec![Listen {
                listened_at: None, // Not used for playing_now
                track_metadata: TrackMetadata {
                    artist_name: artist.to_string(),
                    track_name: track.to_string(),
                    release_name: album.map(|s| s.to_string()),
                    additional_info: Some(info),
                },
            }],
        };

        self.submit_listens(&token, &payload).await
    }

    /// Submit a scrobble (track finished playing)
    pub async fn submit_listen(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
        timestamp: i64,
        additional_info: Option<AdditionalInfo>,
    ) -> IntegrationResult<()> {
        let token = {
            let config = self.config.lock().await;
            if !config.enabled {
                return Ok(()); // Silently skip if disabled
            }
            config.token.clone()
        };

        let token = token.ok_or(IntegrationError::NotAuthenticated)?;

        let info = self.prepare_additional_info(additional_info);

        let payload = SubmitListensPayload {
            listen_type: ListenType::Single,
            payload: vec![Listen {
                listened_at: Some(timestamp),
                track_metadata: TrackMetadata {
                    artist_name: artist.to_string(),
                    track_name: track.to_string(),
                    release_name: album.map(|s| s.to_string()),
                    additional_info: Some(info),
                },
            }],
        };

        self.submit_listens(&token, &payload).await
    }

    /// Prepare additional info with QBZ identifiers
    fn prepare_additional_info(&self, info: Option<AdditionalInfo>) -> AdditionalInfo {
        let mut info = info.unwrap_or_default();
        info.media_player = "QBZ".to_string();
        info.media_player_version = self.version.clone();
        info.submission_client = "QBZ".to_string();
        info.submission_client_version = self.version.clone();
        info
    }

    /// Internal: Submit listens to API
    async fn submit_listens(&self, token: &str, payload: &SubmitListensPayload) -> IntegrationResult<()> {
        let url = format!("{}/submit-listens", LISTENBRAINZ_API_URL);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Token {}", token))
            .header("Content-Type", "application/json")
            .json(payload)
            .send()
            .await?;

        if response.status().is_success() {
            let listen_type = match payload.listen_type {
                ListenType::PlayingNow => "now playing",
                ListenType::Single => "scrobble",
            };
            if let Some(listen) = payload.payload.first() {
                log::debug!(
                    "ListenBrainz {}: {} - {}",
                    listen_type,
                    listen.track_metadata.artist_name,
                    listen.track_metadata.track_name
                );
            }
            Ok(())
        } else {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            Err(IntegrationError::internal(format!(
                "ListenBrainz submission failed: {} - {}",
                status, text
            )))
        }
    }
}
