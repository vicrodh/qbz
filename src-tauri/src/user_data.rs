//! Tauri adapter for per-user data path management.
//!
//! The portable path provider lives in `qbz-app`; this module keeps the existing
//! Tauri import surface unchanged.

pub use qbz_app::user_data::UserDataPaths;
