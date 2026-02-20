//! Odesli/song.link API client

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::Client;

use super::errors::ShareError;
use super::models::{ContentType, OdesliResponse, SongLinkResponse};

const ODESLI_API_URL: &str = "https://api.song.link/v1-alpha.1/links";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
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
                .connect_timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Get song.link URL for a track by ISRC
    /// Qobuz isn't supported by Odesli, so we resolve ISRC → Deezer track ID → Odesli.
    /// Deezer's free API supports ISRC lookup without authentication.
    pub async fn get_by_isrc(&self, isrc: &str) -> Result<SongLinkResponse, ShareError> {
        let cache_key = format!("isrc:{}", isrc);

        // Check cache first
        if let Some(cached) = self.get_from_cache(&cache_key) {
            log::debug!("Cache hit for ISRC: {}", isrc);
            return Ok(cached);
        }

        // Step 1: Resolve ISRC to a Deezer track ID via Deezer's free API
        let deezer_api_url = format!("https://api.deezer.com/2.0/track/isrc:{}", isrc);
        log::info!("Resolving ISRC {} via Deezer API", isrc);

        let deezer_response = self
            .client
            .get(&deezer_api_url)
            .send()
            .await?;

        let deezer_id: Option<u64> = if deezer_response.status().is_success() {
            let body: serde_json::Value = deezer_response.json().await
                .map_err(|e| ShareError::OdesliError(format!("Failed to parse Deezer response: {}", e)))?;

            if body.get("error").is_some() {
                log::warn!("Deezer ISRC lookup returned error for {}", isrc);
                None
            } else {
                body.get("id").and_then(|v| v.as_u64())
            }
        } else {
            log::warn!("Deezer API returned {} for ISRC {}", deezer_response.status(), isrc);
            None
        };

        let deezer_id = deezer_id.ok_or_else(|| {
            ShareError::OdesliError(format!(
                "Could not find track with ISRC {} on Deezer", isrc
            ))
        })?;

        log::info!("Resolved ISRC {} to Deezer ID: {}", isrc, deezer_id);

        // Step 2: Query Odesli with platform+type+id (faster than URL resolution)
        let id_str = deezer_id.to_string();
        let response = self
            .client
            .get(ODESLI_API_URL)
            .query(&[
                ("platform", "deezer"),
                ("type", "song"),
                ("id", &id_str),
                ("userCountry", "US"),
            ])
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

        // Cache the result
        self.store_in_cache(cache_key, result.clone());

        Ok(result)
    }

    /// Get song.link URL for an album by UPC
    /// Resolves UPC → Deezer album ID → Odesli (Odesli no longer accepts ?upc= directly).
    pub async fn get_by_upc(&self, upc: &str) -> Result<SongLinkResponse, ShareError> {
        let cache_key = format!("upc:{}", upc);

        // Check cache first
        if let Some(cached) = self.get_from_cache(&cache_key) {
            log::debug!("Cache hit for UPC: {}", upc);
            return Ok(cached);
        }

        // Step 1: Resolve UPC to a Deezer album ID
        let deezer_api_url = format!("https://api.deezer.com/2.0/album/upc:{}", upc);
        log::info!("Resolving UPC {} via Deezer API", upc);

        let deezer_response = self
            .client
            .get(&deezer_api_url)
            .send()
            .await?;

        let deezer_id: Option<u64> = if deezer_response.status().is_success() {
            let body: serde_json::Value = deezer_response.json().await
                .map_err(|e| ShareError::OdesliError(format!("Failed to parse Deezer response: {}", e)))?;

            if body.get("error").is_some() {
                log::warn!("Deezer UPC lookup returned error for {}", upc);
                None
            } else {
                body.get("id").and_then(|v| v.as_u64())
            }
        } else {
            log::warn!("Deezer API returned {} for UPC {}", deezer_response.status(), upc);
            None
        };

        let deezer_id = deezer_id.ok_or_else(|| {
            ShareError::OdesliError(format!(
                "Could not find album with UPC {} on Deezer", upc
            ))
        })?;

        log::info!("Resolved UPC {} to Deezer album ID: {}", upc, deezer_id);

        // Step 2: Query Odesli with platform+type+id
        let id_str = deezer_id.to_string();
        let response = self
            .client
            .get(ODESLI_API_URL)
            .query(&[
                ("platform", "deezer"),
                ("type", "album"),
                ("id", &id_str),
                ("userCountry", "US"),
            ])
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

    /// Get song.link URL by URL (fallback when ISRC/UPC are missing)
    pub async fn get_by_url(&self, url: &str, content_type: ContentType) -> Result<SongLinkResponse, ShareError> {
        let cache_key = format!("url:{}", url);

        if let Some(cached) = self.get_from_cache(&cache_key) {
            log::debug!("Cache hit for URL: {}", url);
            return Ok(cached);
        }

        log::info!("Fetching song.link for URL: {}", url);

        let response = self
            .client
            .get(ODESLI_API_URL)
            .query(&[("url", url), ("userCountry", "US")])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            // Provide a friendlier message for common errors
            if status.as_u16() == 400 && text.contains("could_not_resolve_entity") {
                return Err(ShareError::OdesliError(
                    "Track not found on any supported platform. Try a track with an ISRC code.".to_string()
                ));
            }
            return Err(ShareError::OdesliError(format!(
                "HTTP {}: {}",
                status, text
            )));
        }

        let odesli: OdesliResponse = response.json().await?;
        let result = self.convert_response(odesli, url.to_string(), content_type)?;

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
