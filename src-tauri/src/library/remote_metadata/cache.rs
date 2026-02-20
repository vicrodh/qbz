//! In-memory cache for remote metadata requests
//!
//! Provides short-lived caching to avoid redundant API calls during
//! a tag editing session.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::models::{RemoteAlbumMetadata, RemoteAlbumSearchResult, RemoteProvider};

/// Cache entry with expiration
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    data: T,
    inserted_at: Instant,
    ttl: Duration,
}

impl<T> CacheEntry<T> {
    fn new(data: T, ttl: Duration) -> Self {
        Self {
            data,
            inserted_at: Instant::now(),
            ttl,
        }
    }

    fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > self.ttl
    }
}

/// Cache key for search results
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct SearchCacheKey {
    provider: String,
    query: String,
    catalog_id: Option<String>,
    artist: Option<String>,
}

/// Cache key for full metadata
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct MetadataCacheKey {
    provider: String,
    provider_id: String,
}

/// Remote metadata cache
pub struct RemoteMetadataCache {
    /// Search results cache (short TTL: 10 minutes)
    search_cache: Arc<RwLock<HashMap<SearchCacheKey, CacheEntry<Vec<RemoteAlbumSearchResult>>>>>,
    /// Full metadata cache (longer TTL: 1 hour)
    metadata_cache: Arc<RwLock<HashMap<MetadataCacheKey, CacheEntry<RemoteAlbumMetadata>>>>,
    /// TTL for search results
    search_ttl: Duration,
    /// TTL for full metadata
    metadata_ttl: Duration,
}

impl Default for RemoteMetadataCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteMetadataCache {
    /// Create a new cache with default TTLs
    pub fn new() -> Self {
        Self {
            search_cache: Arc::new(RwLock::new(HashMap::new())),
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
            search_ttl: Duration::from_secs(10 * 60), // 10 minutes
            metadata_ttl: Duration::from_secs(60 * 60), // 1 hour
        }
    }

    /// Get cached search results
    pub async fn get_search(
        &self,
        provider: RemoteProvider,
        query: &str,
        catalog_id: Option<&str>,
        artist: Option<&str>,
    ) -> Option<Vec<RemoteAlbumSearchResult>> {
        let key = SearchCacheKey {
            provider: provider.to_string(),
            query: query.to_lowercase(),
            catalog_id: catalog_id.map(|s| s.to_lowercase()),
            artist: artist.map(|s| s.to_lowercase()),
        };

        let cache = self.search_cache.read().await;
        if let Some(entry) = cache.get(&key) {
            if !entry.is_expired() {
                log::debug!("Cache hit for search: {} {:?}", provider, query);
                return Some(entry.data.clone());
            }
        }
        None
    }

    /// Store search results in cache
    pub async fn set_search(
        &self,
        provider: RemoteProvider,
        query: &str,
        catalog_id: Option<&str>,
        artist: Option<&str>,
        results: Vec<RemoteAlbumSearchResult>,
    ) {
        let key = SearchCacheKey {
            provider: provider.to_string(),
            query: query.to_lowercase(),
            catalog_id: catalog_id.map(|s| s.to_lowercase()),
            artist: artist.map(|s| s.to_lowercase()),
        };

        let mut cache = self.search_cache.write().await;
        cache.insert(key, CacheEntry::new(results, self.search_ttl));
    }

    /// Get cached full metadata
    pub async fn get_metadata(
        &self,
        provider: RemoteProvider,
        provider_id: &str,
    ) -> Option<RemoteAlbumMetadata> {
        let key = MetadataCacheKey {
            provider: provider.to_string(),
            provider_id: provider_id.to_string(),
        };

        let cache = self.metadata_cache.read().await;
        if let Some(entry) = cache.get(&key) {
            if !entry.is_expired() {
                log::debug!("Cache hit for metadata: {} {}", provider, provider_id);
                return Some(entry.data.clone());
            }
        }
        None
    }

    /// Store full metadata in cache
    pub async fn set_metadata(
        &self,
        provider: RemoteProvider,
        provider_id: &str,
        metadata: RemoteAlbumMetadata,
    ) {
        let key = MetadataCacheKey {
            provider: provider.to_string(),
            provider_id: provider_id.to_string(),
        };

        let mut cache = self.metadata_cache.write().await;
        cache.insert(key, CacheEntry::new(metadata, self.metadata_ttl));
    }

    /// Clear expired entries from both caches
    pub async fn cleanup_expired(&self) -> usize {
        let mut removed = 0;

        {
            let mut cache = self.search_cache.write().await;
            let before = cache.len();
            cache.retain(|_, entry| !entry.is_expired());
            removed += before - cache.len();
        }

        {
            let mut cache = self.metadata_cache.write().await;
            let before = cache.len();
            cache.retain(|_, entry| !entry.is_expired());
            removed += before - cache.len();
        }

        if removed > 0 {
            log::debug!("Cleaned up {} expired cache entries", removed);
        }

        removed
    }

    /// Clear all cached data
    pub async fn clear_all(&self) {
        {
            let mut cache = self.search_cache.write().await;
            cache.clear();
        }
        {
            let mut cache = self.metadata_cache.write().await;
            cache.clear();
        }
        log::info!("Cleared remote metadata cache");
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let search_count = self.search_cache.read().await.len();
        let metadata_count = self.metadata_cache.read().await.len();

        CacheStats {
            search_entries: search_count,
            metadata_entries: metadata_count,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheStats {
    pub search_entries: usize,
    pub metadata_entries: usize,
}
