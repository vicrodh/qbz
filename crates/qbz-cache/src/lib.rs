//! QBZ Cache - Audio caching system
//!
//! Provides two-level caching for audio data:
//! - **L1 (Memory)**: In-memory LRU cache (~300MB, fast access)
//! - **L2 (Disk)**: Disk-based playback cache (~500MB, persistent)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    Audio Request                         │
//! └─────────────────────────────────────────────────────────┘
//!                           │
//!                           ▼
//! ┌─────────────────────────────────────────────────────────┐
//! │              L1 Memory Cache (AudioCache)                │
//! │  - Fast HashMap + LRU tracking                           │
//! │  - ~300MB limit (3-4 Hi-Res tracks)                      │
//! │  - Evicted tracks spill to L2                            │
//! └─────────────────────────────────────────────────────────┘
//!                           │ miss/evict
//!                           ▼
//! ┌─────────────────────────────────────────────────────────┐
//! │              L2 Disk Cache (PlaybackCache)               │
//! │  - File-based storage                                    │
//! │  - ~500MB limit                                          │
//! │  - LRU eviction by access time                           │
//! └─────────────────────────────────────────────────────────┘
//!                           │ miss
//!                           ▼
//!                      [ Network ]
//! ```
//!
//! ## Usage
//!
//! ```no_run
//! use qbz_cache::{AudioCache, PlaybackCache};
//! use std::sync::Arc;
//!
//! // Create L2 disk cache
//! let playback_cache = Arc::new(PlaybackCache::new(500 * 1024 * 1024).unwrap());
//!
//! // Create L1 memory cache with L2 spillover
//! let audio_cache = AudioCache::with_playback_cache(
//!     300 * 1024 * 1024,
//!     playback_cache.clone()
//! );
//!
//! // Insert track
//! audio_cache.insert(12345, audio_data);
//!
//! // Retrieve track (checks L1, then L2 if configured)
//! if let Some(cached) = audio_cache.get(12345) {
//!     // Use cached.data
//! }
//! ```

mod audio_cache;
mod playback_cache;

pub use audio_cache::{AudioCache, CachedTrack, CacheStats};
pub use playback_cache::{PlaybackCache, PlaybackCacheStats};
