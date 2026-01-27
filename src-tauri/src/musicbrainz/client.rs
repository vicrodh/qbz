//! MusicBrainz API client
//!
//! HTTP client with rate limiting and proper User-Agent handling
//! Uses Cloudflare Workers proxy for consistent rate limiting

use reqwest::Client;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use super::models::*;

/// Proxy URL for MusicBrainz requests
const MUSICBRAINZ_PROXY_URL: &str = "https://qbz-api-proxy.blitzkriegfc.workers.dev/musicbrainz";

/// Direct MusicBrainz API URL (fallback)
const MUSICBRAINZ_API_URL: &str = "https://musicbrainz.org/ws/2";

/// Rate limiter for MusicBrainz API (1 request per second)
pub struct RateLimiter {
    last_request: Mutex<Instant>,
    min_interval: Duration,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            // Start in the past so first request doesn't wait
            last_request: Mutex::new(Instant::now() - Duration::from_secs(2)),
            min_interval: Duration::from_millis(1100), // Slightly over 1 second for safety
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
    pub fn new() -> Self {
        Self::with_config(MusicBrainzConfig::default())
    }

    pub fn with_config(config: MusicBrainzConfig) -> Self {
        let version = env!("CARGO_PKG_VERSION");
        let user_agent = format!(
            "QBZ/{} (https://github.com/vicrodh/qbz; qbz@vicrodh.dev)",
            version
        );

        let client = Client::builder()
            .user_agent(&user_agent)
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            rate_limiter: Arc::new(RateLimiter::new()),
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
    pub async fn search_recording_by_isrc(&self, isrc: &str) -> Result<RecordingSearchResponse, String> {
        if !self.is_enabled().await {
            return Err("MusicBrainz integration is disabled".to_string());
        }

        self.rate_limiter.wait().await;

        let base_url = self.base_url().await;
        let url = format!(
            "{}/recording?query=isrc:{}&fmt=json&limit=5",
            base_url, isrc
        );

        log::debug!("MusicBrainz recording search by ISRC: {}", isrc);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("MusicBrainz request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("MusicBrainz API error {}: {}", status, text));
        }

        response
            .json::<RecordingSearchResponse>()
            .await
            .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))
    }

    /// Search recordings by title and artist
    pub async fn search_recording(
        &self,
        title: &str,
        artist: &str,
    ) -> Result<RecordingSearchResponse, String> {
        if !self.is_enabled().await {
            return Err("MusicBrainz integration is disabled".to_string());
        }

        self.rate_limiter.wait().await;

        let base_url = self.base_url().await;
        let query = format!(
            "recording:\"{}\" AND artist:\"{}\"",
            Self::escape_query(title),
            Self::escape_query(artist)
        );
        let url = format!(
            "{}/recording?query={}&fmt=json&limit=5",
            base_url,
            urlencoding::encode(&query)
        );

        log::debug!("MusicBrainz recording search: {} - {}", artist, title);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("MusicBrainz request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("MusicBrainz API error {}: {}", status, text));
        }

        response
            .json::<RecordingSearchResponse>()
            .await
            .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))
    }

    /// Search artists by name
    pub async fn search_artist(&self, name: &str) -> Result<ArtistSearchResponse, String> {
        if !self.is_enabled().await {
            return Err("MusicBrainz integration is disabled".to_string());
        }

        self.rate_limiter.wait().await;

        let base_url = self.base_url().await;
        let query = format!("artist:\"{}\"", Self::escape_query(name));
        let url = format!(
            "{}/artist?query={}&fmt=json&limit=5",
            base_url,
            urlencoding::encode(&query)
        );

        log::debug!("MusicBrainz artist search: {}", name);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("MusicBrainz request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("MusicBrainz API error {}: {}", status, text));
        }

        response
            .json::<ArtistSearchResponse>()
            .await
            .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))
    }

    /// Get artist details with relationships
    pub async fn get_artist_with_relations(&self, mbid: &str) -> Result<ArtistFullResponse, String> {
        if !self.is_enabled().await {
            return Err("MusicBrainz integration is disabled".to_string());
        }

        self.rate_limiter.wait().await;

        let base_url = self.base_url().await;
        let url = format!(
            "{}/artist/{}?inc=artist-rels&fmt=json",
            base_url, mbid
        );

        log::debug!("MusicBrainz artist lookup with relations: {}", mbid);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("MusicBrainz request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("MusicBrainz API error {}: {}", status, text));
        }

        response
            .json::<ArtistFullResponse>()
            .await
            .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))
    }

    /// Search releases by barcode (UPC/EAN)
    pub async fn search_release_by_barcode(&self, barcode: &str) -> Result<ReleaseSearchResponse, String> {
        if !self.is_enabled().await {
            return Err("MusicBrainz integration is disabled".to_string());
        }

        self.rate_limiter.wait().await;

        let base_url = self.base_url().await;
        let url = format!(
            "{}/release?query=barcode:{}&fmt=json&limit=5",
            base_url, barcode
        );

        log::debug!("MusicBrainz release search by barcode: {}", barcode);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("MusicBrainz request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("MusicBrainz API error {}: {}", status, text));
        }

        response
            .json::<ReleaseSearchResponse>()
            .await
            .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))
    }

    /// Search releases by title and artist
    pub async fn search_release(
        &self,
        title: &str,
        artist: &str,
    ) -> Result<ReleaseSearchResponse, String> {
        self.search_releases_extended(title, artist, None, 5).await
    }

    /// Search releases with extended options
    /// - `title`: Album title to search
    /// - `artist`: Artist name to search
    /// - `catalog_number`: Optional catalog number for more precise matching
    /// - `limit`: Maximum results to return (1-25)
    pub async fn search_releases_extended(
        &self,
        title: &str,
        artist: &str,
        catalog_number: Option<&str>,
        limit: usize,
    ) -> Result<ReleaseSearchResponse, String> {
        if !self.is_enabled().await {
            return Err("MusicBrainz integration is disabled".to_string());
        }

        self.rate_limiter.wait().await;

        let base_url = self.base_url().await;

        // Build query - if catalog number provided, prioritize it
        let query = if let Some(catno) = catalog_number.filter(|s| !s.trim().is_empty()) {
            format!(
                "catno:\"{}\" AND artist:\"{}\"",
                Self::escape_query(catno),
                Self::escape_query(artist)
            )
        } else {
            format!(
                "release:\"{}\" AND artist:\"{}\"",
                Self::escape_query(title),
                Self::escape_query(artist)
            )
        };

        let limit = limit.min(25).max(1);
        let url = format!(
            "{}/release?query={}&fmt=json&limit={}",
            base_url,
            urlencoding::encode(&query),
            limit
        );

        log::debug!(
            "MusicBrainz release search: {} - {} (catalog: {:?}, limit: {})",
            artist,
            title,
            catalog_number,
            limit
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("MusicBrainz request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("MusicBrainz API error {}: {}", status, text));
        }

        response
            .json::<ReleaseSearchResponse>()
            .await
            .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))
    }

    /// Get full release details including tracks
    /// Fetches media, recordings, artist credits, labels, and tags
    pub async fn get_release_with_tracks(&self, release_id: &str) -> Result<ReleaseFullResponse, String> {
        if !self.is_enabled().await {
            return Err("MusicBrainz integration is disabled".to_string());
        }

        self.rate_limiter.wait().await;

        let base_url = self.base_url().await;
        // inc=recordings gets track info, artist-credits for artist info,
        // labels for label/catalog, tags for genres
        let url = format!(
            "{}/release/{}?inc=recordings+artist-credits+labels+tags&fmt=json",
            base_url, release_id
        );

        log::debug!("MusicBrainz release lookup with tracks: {}", release_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("MusicBrainz request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("MusicBrainz API error {}: {}", status, text));
        }

        response
            .json::<ReleaseFullResponse>()
            .await
            .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))
    }

    /// Escape special characters in Lucene queries
    fn escape_query(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(':', "\\:")
            .replace('(', "\\(")
            .replace(')', "\\)")
            .replace('[', "\\[")
            .replace(']', "\\]")
            .replace('{', "\\{")
            .replace('}', "\\}")
            .replace('^', "\\^")
            .replace('~', "\\~")
            .replace('*', "\\*")
            .replace('?', "\\?")
            .replace('!', "\\!")
            .replace('+', "\\+")
            .replace('-', "\\-")
            .replace('&', "\\&")
            .replace('|', "\\|")
    }
}
