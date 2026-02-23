//! Audio caching module
//!
//! Re-exports from qbz-cache crate for backwards compatibility.
//!
//! Provides two-level caching for audio data:
//! - L1: In-memory LRU cache (fast, limited to ~400MB)
//! - L2: Disk-based playback cache (slower, larger ~800MB)
//!
//! Flow:
//! 1. When a track is evicted from memory, it's saved to disk cache
//! 2. When loading, check memory -> disk -> network

// Re-export everything from qbz-cache
pub use qbz_cache::{AudioCache, CacheStats, CachedTrack, PlaybackCache, PlaybackCacheStats};
