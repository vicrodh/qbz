//! Tauri commands module
//!
//! Exposes backend functionality to the frontend via IPC

pub mod auth;
pub mod cache;
pub mod favorites;
pub mod lastfm;
pub mod notification;
pub mod playback;
pub mod playlist;
pub mod playlist_import;
pub mod queue;
pub mod search;
pub mod share;

pub use auth::*;
pub use cache::*;
pub use favorites::*;
pub use lastfm::*;
pub use notification::*;
pub use playback::*;
pub use playlist::*;
pub use playlist_import::*;
pub use queue::*;
pub use search::*;
pub use share::*;
