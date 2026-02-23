//! MusicBrainz API client
//!
//! HTTP client with rate limiting and proper User-Agent handling.
//! Uses Cloudflare Workers proxy for consistent rate limiting.

use reqwest::Client;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::error::{IntegrationError, IntegrationResult};
use super::models::*;

/// Proxy URL for MusicBrainz requests
const MUSICBRAINZ_PROXY_URL: &str = "https://qbz-api-proxy.blitzkriegfc.workers.dev/musicbrainz";

/// Direct MusicBrainz API URL (fallback)
const MUSICBRAINZ_API_URL: &str = "https://musicbrainz.org/ws/2";

/// Rate limiter for MusicBrainz API
pub struct RateLimiter {
    last_request: Mutex<Instant>,
    min_interval: Duration,
}

impl RateLimiter {
    /// Create rate limiter for direct MusicBrainz API (1 req/sec)
    pub fn new() -> Self {
        Self::with_interval(Duration::from_millis(1100))
    }

    /// Create rate limiter for proxy (faster, proxy handles actual rate limiting)
    pub fn for_proxy() -> Self {
        Self::with_interval(Duration::from_millis(200))
    }

    /// Create rate limiter with custom interval
    pub fn with_interval(min_interval: Duration) -> Self {
        Self {
            // Start in the past so first request doesn't wait
            last_request: Mutex::new(Instant::now() - Duration::from_secs(2)),
            min_interval,
        }
    }

    pub async fn wait(&self) {
        let mut last = self.last_request.lock().await;
        let elapsed = last.elapsed();
        if elapsed < self.min_interval {
            tokio::time::sleep(self.min_interval - elapsed).await;
        }
        *last = Instant::now();
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// MusicBrainz API client configuration
#[derive(Debug, Clone)]
pub struct MusicBrainzConfig {
    /// Whether MusicBrainz integration is enabled
    pub enabled: bool,
    /// Use proxy instead of direct API
    pub use_proxy: bool,
}

impl Default for MusicBrainzConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            use_proxy: true,
        }
    }
}

/// MusicBrainz API client
pub struct MusicBrainzClient {
    client: Client,
    rate_limiter: Arc<RateLimiter>,
    config: Arc<Mutex<MusicBrainzConfig>>,
}

impl Default for MusicBrainzClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicBrainzClient {
    /// Create a new MusicBrainz client with default config
    pub fn new() -> Self {
        Self::with_config(MusicBrainzConfig::default())
    }

    /// Create client with specific configuration
    pub fn with_config(config: MusicBrainzConfig) -> Self {
        let version = "1.0.0";
        let user_agent = format!(
            "QBZ/{} (https://github.com/vicrodh/qbz; qbz@vicrodh.dev)",
            version
        );

        let client = Client::builder()
            .user_agent(&user_agent)
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        // Use faster rate limiter when using proxy
        let rate_limiter = if config.use_proxy {
            RateLimiter::for_proxy()
        } else {
            RateLimiter::new()
        };

        Self {
            client,
            rate_limiter: Arc::new(rate_limiter),
            config: Arc::new(Mutex::new(config)),
        }
    }

    /// Check if MusicBrainz integration is enabled
    pub async fn is_enabled(&self) -> bool {
        self.config.lock().await.enabled
    }

    /// Enable or disable MusicBrainz integration
    pub async fn set_enabled(&self, enabled: bool) {
        self.config.lock().await.enabled = enabled;
    }

    /// Get the base URL based on configuration
    async fn base_url(&self) -> &'static str {
        if self.config.lock().await.use_proxy {
            MUSICBRAINZ_PROXY_URL
        } else {
            MUSICBRAINZ_API_URL
        }
    }

    /// Search recordings by ISRC
    pub async fn search_recording_by_isrc(&self, isrc: &str) -> IntegrationResult<RecordingSearchResponse> {
        if !self.is_enabled().await {
            return Err(IntegrationError::ServiceUnavailable(
                "MusicBrainz integration is disabled".into(),
            ));
        }

        self.rate_limiter.wait().await;

        let base = self.base_url().await;
        let url = format!("{}/recording?query=isrc:{}&fmt=json", base, isrc);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            if status.as_u16() == 429 {
                return Err(IntegrationError::RateLimited(60));
            }
            let text = response.text().await.unwrap_or_default();
            return Err(IntegrationError::internal(format!(
                "MusicBrainz search failed: {} - {}",
                status, text
            )));
        }

        response.json().await.map_err(Into::into)
    }

    /// Search artists by name
    pub async fn search_artist(&self, name: &str, limit: u32) -> IntegrationResult<ArtistSearchResponse> {
        if !self.is_enabled().await {
            return Err(IntegrationError::ServiceUnavailable(
                "MusicBrainz integration is disabled".into(),
            ));
        }

        self.rate_limiter.wait().await;

        let base = self.base_url().await;
        let encoded_name = urlencoding::encode(name);
        let url = format!(
            "{}/artist?query=artist:{}&limit={}&fmt=json",
            base, encoded_name, limit
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            if status.as_u16() == 429 {
                return Err(IntegrationError::RateLimited(60));
            }
            let text = response.text().await.unwrap_or_default();
            return Err(IntegrationError::internal(format!(
                "MusicBrainz artist search failed: {} - {}",
                status, text
            )));
        }

        response.json().await.map_err(Into::into)
    }

    /// Resolve a track to get MusicBrainz IDs
    ///
    /// Searches by ISRC if available, falling back to text search.
    pub async fn resolve_track(
        &self,
        artist: &str,
        title: &str,
        isrc: Option<&str>,
    ) -> IntegrationResult<Option<ResolvedTrack>> {
        // Try ISRC first (most accurate)
        if let Some(isrc) = isrc {
            let response = self.search_recording_by_isrc(isrc).await?;
            if let Some(recording) = response.recordings.first() {
                let confidence = if recording.isrcs.as_ref().map_or(false, |isrcs| isrcs.contains(&isrc.to_string())) {
                    MatchConfidence::Exact
                } else {
                    MatchConfidence::from_score(recording.score)
                };

                return Ok(Some(ResolvedTrack {
                    recording_mbid: recording.id.clone(),
                    title: recording.title.clone().unwrap_or_default(),
                    artist_mbids: recording
                        .artist_credit
                        .as_ref()
                        .map(|ac| ac.iter().map(|a| a.artist.id.clone()).collect())
                        .unwrap_or_default(),
                    release_mbid: recording
                        .releases
                        .as_ref()
                        .and_then(|r| r.first())
                        .map(|r| r.id.clone()),
                    isrcs: recording.isrcs.clone().unwrap_or_default(),
                    confidence,
                }));
            }
        }

        // TODO: Implement text-based search fallback
        // For now, return None if ISRC search fails
        let _ = (artist, title); // Silence unused warnings
        Ok(None)
    }

    /// Resolve an artist to get MusicBrainz ID
    pub async fn resolve_artist(&self, name: &str) -> IntegrationResult<Option<ResolvedArtist>> {
        let response = self.search_artist(name, 5).await?;

        if let Some(artist) = response.artists.first() {
            let confidence = MatchConfidence::from_score(artist.score);

            return Ok(Some(ResolvedArtist {
                mbid: artist.id.clone(),
                name: artist.name.clone(),
                sort_name: artist.sort_name.clone(),
                artist_type: ArtistType::from(artist.artist_type.as_deref()),
                country: artist.country.clone(),
                disambiguation: artist.disambiguation.clone(),
                confidence,
            }));
        }

        Ok(None)
    }
}
