//! Tauri commands module
//!
//! Exposes backend functionality to the frontend via IPC

pub mod auth;
pub mod playback;
pub mod queue;
pub mod search;

pub use auth::*;
pub use playback::*;
pub use queue::*;
pub use search::*;
