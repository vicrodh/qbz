//! Tauri commands module
//!
//! Exposes backend functionality to the frontend via IPC

pub mod artist_blacklist;
pub mod audio_backends;
pub mod audio_diagnostics;
pub mod auth;
pub mod cache;
pub mod credits;
pub mod favorites;
pub mod lastfm;
pub mod listenbrainz;
pub mod musician;
pub mod musicbrainz;
pub mod notification;
pub mod playback;
pub mod playback_context;
pub mod playlist;
pub mod playlist_import;
pub mod playlist_suggestions;
pub mod queue;
pub mod radio;
pub mod search;
pub mod share;
pub mod smart_playlists;
pub mod remote_metadata;
pub mod user_session;
pub mod visualizer;

pub use artist_blacklist::*;
pub use audio_backends::*;
pub use audio_diagnostics::*;
pub use auth::*;
pub use cache::*;
pub use credits::*;
pub use favorites::*;
pub use lastfm::*;
pub use listenbrainz::*;
pub use musician::*;
pub use musicbrainz::*;
pub use notification::*;
pub use playback::*;
pub use playback_context::*;
pub use playlist::*;
pub use playlist_import::*;
pub use playlist_suggestions::*;
pub use queue::*;
pub use radio::*;
pub use search::*;
pub use share::*;
pub use smart_playlists::*;
pub use remote_metadata::*;
pub use user_session::*;
pub use visualizer::*;
