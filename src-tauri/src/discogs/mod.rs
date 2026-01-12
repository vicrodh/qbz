//! Discogs API client for fetching album artwork
//!
//! Uses the Discogs database API to search for releases and download cover images.

use reqwest::Client;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::time::Duration;

/// Discogs API client
pub struct DiscogsClient {
    client: Client,
    consumer_key: Option<String>,
    consumer_secret: Option<String>,
}

/// Search result from Discogs API
#[derive(Debug, Deserialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    id: u64,
    cover_image: Option<String>,
    thumb: Option<String>,
    title: String,
    #[serde(rename = "type")]
    result_type: String,
}

// Compile-time embedded credentials (from build environment)
const EMBEDDED_CONSUMER_KEY: Option<&str> = option_env!("DISCOGS_API_CLIENT_KEY");
const EMBEDDED_CONSUMER_SECRET: Option<&str> = option_env!("DISCOGS_API_CLIENT_SECRET");

impl DiscogsClient {
    /// Create a new Discogs client with optional credentials
    /// Tries embedded credentials first, falls back to runtime env vars
    pub fn new() -> Self {
        let consumer_key = EMBEDDED_CONSUMER_KEY
            .map(String::from)
            .or_else(|| std::env::var("DISCOGS_API_CLIENT_KEY").ok());
        let consumer_secret = EMBEDDED_CONSUMER_SECRET
            .map(String::from)
            .or_else(|| std::env::var("DISCOGS_API_CLIENT_SECRET").ok());

        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("QBZ/0.1.0 +https://github.com/vicrodh/qbz")
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            consumer_key,
            consumer_secret,
        }
    }

    /// Check if credentials are configured
    pub fn has_credentials(&self) -> bool {
        self.consumer_key.is_some() && self.consumer_secret.is_some()
    }

    /// Search for album artwork and download if found
    /// Returns the path to the downloaded image or None
    pub async fn fetch_artwork(
        &self,
        artist: &str,
        album: &str,
        cache_dir: &Path,
    ) -> Option<String> {
        if !self.has_credentials() {
            log::debug!("Discogs credentials not configured, skipping artwork fetch");
            return None;
        }

        // Search for the release
        let cover_url = self.search_release(artist, album).await?;

        // Generate cache filename
        let filename = format!(
            "discogs_{:x}.jpg",
            Self::simple_hash(&format!("{}_{}", artist, album))
        );
        let cache_path = cache_dir.join(&filename);

        // Return cached if exists
        if cache_path.exists() {
            return Some(cache_path.to_string_lossy().to_string());
        }

        // Download the image
        self.download_image(&cover_url, &cache_path).await?;

        Some(cache_path.to_string_lossy().to_string())
    }

    /// Search for a release and return the cover image URL
    async fn search_release(&self, artist: &str, album: &str) -> Option<String> {
        let key = self.consumer_key.as_ref()?;
        let secret = self.consumer_secret.as_ref()?;

        // Build search query
        let query = format!("{} {}", artist, album);
        let url = format!(
            "https://api.discogs.com/database/search?q={}&type=release&key={}&secret={}",
            urlencoding::encode(&query),
            key,
            secret
        );

        log::debug!("Searching Discogs for: {} - {}", artist, album);

        let response = self.client.get(&url).send().await.ok()?;

        if !response.status().is_success() {
            log::warn!("Discogs search failed with status: {}", response.status());
            return None;
        }

        let search: SearchResponse = response.json().await.ok()?;

        // Find first result with a cover image
        for result in search.results {
            if result.result_type == "release" || result.result_type == "master" {
                if let Some(cover) = result.cover_image {
                    if !cover.is_empty() && !cover.contains("spacer.gif") {
                        log::debug!("Found Discogs cover for {} - {}", artist, album);
                        return Some(cover);
                    }
                }
                // Fallback to thumbnail
                if let Some(thumb) = result.thumb {
                    if !thumb.is_empty() && !thumb.contains("spacer.gif") {
                        return Some(thumb);
                    }
                }
            }
        }

        log::debug!("No Discogs cover found for {} - {}", artist, album);
        None
    }

    /// Download an image to the cache directory
    async fn download_image(&self, url: &str, path: &Path) -> Option<()> {
        log::debug!("Downloading Discogs artwork: {}", url);

        let key = self.consumer_key.as_ref()?;
        let secret = self.consumer_secret.as_ref()?;

        // Discogs requires auth even for image downloads
        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Discogs key={}, secret={}", key, secret))
            .send()
            .await
            .ok()?;

        if !response.status().is_success() {
            log::warn!("Failed to download Discogs image: {}", response.status());
            return None;
        }

        let bytes = response.bytes().await.ok()?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok()?;
        }

        fs::write(path, &bytes).ok()?;

        log::info!("Saved Discogs artwork to: {}", path.display());
        Some(())
    }

    /// Simple hash function for generating filenames
    fn simple_hash(s: &str) -> u64 {
        let mut hash: u64 = 5381;
        for byte in s.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        }
        hash
    }
}

impl Default for DiscogsClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash() {
        let hash1 = DiscogsClient::simple_hash("Artist_Album");
        let hash2 = DiscogsClient::simple_hash("Artist_Album");
        assert_eq!(hash1, hash2);

        let hash3 = DiscogsClient::simple_hash("Different_Album");
        assert_ne!(hash1, hash3);
    }
}
