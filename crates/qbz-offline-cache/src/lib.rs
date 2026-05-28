//! Offline cache for QBZ — frontend-agnostic.
//!
//! Downloading Qobuz content for offline playback and managing the local
//! store: the on-disk CMAF bundle store, the per-track secret vault, the
//! SQLite index, maintenance/purge/migration, and the pure cached-playback
//! resolution.
//!
//! Extracted out of `src-tauri/src/offline_cache/` so both the Tauri and
//! the Slint frontends share ONE implementation (ADR-006). The download
//! pipeline emits progress through a `CacheEventSink` callback instead of
//! Tauri events, so this crate has no Tauri dependency.
//!
//! Migration status (Slice 0 — extraction): modules are moved here
//! incrementally; `src-tauri` keeps its own copy until the re-point step,
//! so the workspace stays green throughout.

pub mod cmaf_store;
pub mod db;
pub mod maintenance;
pub mod path_validator;
pub mod secret_vault;
pub mod types;

pub use db::{CmafBundleRow, OfflineCacheDb};
pub use path_validator::{is_offline_root_available, validate_path, PathStatus};
pub use types::{
    CacheProgress, CachedTrackInfo, OfflineCacheStats, OfflineCacheStatus, ReadyTrackForSync,
    TrackCacheInfo,
};
