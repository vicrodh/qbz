//! Odesli/song.link API client

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::Client;

use super::errors::ShareError;
use super::models::{ContentType, OdesliResponse, SongLinkResponse};

const ODESLI_API_URL: &str = "https://api.song.link/v1-alpha.1/links";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CACHE_TTL: Duration = Duration::from_secs(3600); // 1 hour

/// Cached entry with TTL
struct CacheEntry {
    response: SongLinkResponse,
    created_at: Instant,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > CACHE_TTL
    }
}

/// Odesli/song.link client with caching
pub struct SongLinkClient {
    client: Client,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl Default for SongLinkClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SongLinkClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_default(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Get song.link URL for a track by ISRC
    /// Qobuz isn't supported by Odesli, so we use ISRC to find the track on Spotify
    pub async fn get_by_isrc(&self, isrc: &str) -> Result<SongLinkResponse, ShareError> {
        let cache_key = format!("isrc:{}", isrc);

        // Check cache first
        if let Some(cached) = self.get_from_cache(&cache_key) {
            log::debug!("Cache hit for ISRC: {}", isrc);
            return Ok(cached);
        }

        // Use Spotify search URL with ISRC - Odesli will resolve it
        // Format: https://open.spotify.com/search/isrc:{ISRC}
        let spotify_search_url = format!("https://open.spotify.com/search/isrc%3A{}", isrc);
        log::info!("Fetching song.link via Spotify ISRC search: {}", isrc);

        let response = self
            .client
            .get(ODESLI_API_URL)
            .query(&[("url", &spotify_search_url), ("userCountry", &"US".to_string())])
            .send()
            .await?;

        // If Spotify search doesn't work, try Deezer
        if !response.status().is_success() {
            log::debug!("Spotify search failed, trying Deezer...");
            let deezer_url = format!("https://www.deezer.com/search/{}", isrc);

            let response = self
                .client
                .get(ODESLI_API_URL)
                .query(&[("url", &deezer_url), ("userCountry", &"US".to_string())])
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(ShareError::OdesliError(format!(
                    "HTTP {}: {}",
                    status, text
                )));
            }

            let odesli: OdesliResponse = response.json().await?;
            let result = self.convert_response(odesli, isrc.to_string(), ContentType::Track)?;
            self.store_in_cache(cache_key, result.clone());
            return Ok(result);
        }

        let odesli: OdesliResponse = response.json().await?;
        let result = self.convert_response(odesli, isrc.to_string(), ContentType::Track)?;

        // Cache the result
        self.store_in_cache(cache_key, result.clone());

        Ok(result)
    }

    /// Get song.link URL for an album by UPC
    pub async fn get_by_upc(&self, upc: &str) -> Result<SongLinkResponse, ShareError> {
        let cache_key = format!("upc:{}", upc);

        // Check cache first
        if let Some(cached) = self.get_from_cache(&cache_key) {
            log::debug!("Cache hit for UPC: {}", upc);
            return Ok(cached);
        }

        log::info!("Fetching song.link for UPC: {}", upc);

        let response = self
            .client
            .get(ODESLI_API_URL)
            .query(&[("upc", upc), ("userCountry", "US")])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ShareError::OdesliError(format!(
                "HTTP {}: {}",
                status, text
            )));
        }

        let odesli: OdesliResponse = response.json().await?;
        let result = self.convert_response(odesli, upc.to_string(), ContentType::Album)?;

        // Cache the result
        self.store_in_cache(cache_key, result.clone());

        Ok(result)
    }

    /// Convert Odesli response to our simplified format
    fn convert_response(
        &self,
        response: OdesliResponse,
        identifier: String,
        content_type: ContentType,
    ) -> Result<SongLinkResponse, ShareError> {
        // Extract title and artist from the first entity
        let (title, artist, thumbnail_url) = response
            .entities_by_unique_id
            .values()
            .next()
            .map(|e| {
                (
                    e.title.clone(),
                    e.artist_name.clone(),
                    e.thumbnail_url.clone(),
                )
            })
            .unwrap_or((None, None, None));

        // Extract platform URLs
        let platforms: HashMap<String, String> = response
            .links_by_platform
            .into_iter()
            .map(|(platform, link)| (platform, link.url))
            .collect();

        if platforms.is_empty() {
            return Err(ShareError::NoMatches);
        }

        Ok(SongLinkResponse {
            page_url: response.page_url,
            title,
            artist,
            thumbnail_url,
            platforms,
            identifier,
            content_type: content_type.as_str().to_string(),
        })
    }

    /// Get from cache if not expired
    fn get_from_cache(&self, key: &str) -> Option<SongLinkResponse> {
        let cache = self.cache.lock().ok()?;
        let entry = cache.get(key)?;

        if entry.is_expired() {
            None
        } else {
            Some(entry.response.clone())
        }
    }

    /// Store in cache
    fn store_in_cache(&self, key: String, response: SongLinkResponse) {
        if let Ok(mut cache) = self.cache.lock() {
            // Clean up expired entries occasionally
            if cache.len() > 100 {
                cache.retain(|_, entry| !entry.is_expired());
            }

            cache.insert(
                key,
                CacheEntry {
                    response,
                    created_at: Instant::now(),
                },
            );
        }
    }

    /// Clear the cache
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
}
