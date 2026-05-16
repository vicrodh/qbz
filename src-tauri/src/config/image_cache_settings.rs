//! Tauri-facing re-exports for image cache settings.
//!
//! Persistence lives in `qbz-app`. Image cache runtime, file management,
//! stats, and clear behavior remain host-owned.

pub use qbz_app::settings::image_cache::{
    ImageCacheSettings, ImageCacheSettingsState, ImageCacheSettingsStore,
};
