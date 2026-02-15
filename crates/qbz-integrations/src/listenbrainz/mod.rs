//! ListenBrainz integration
//!
//! Provides scrobbling and now-playing notifications to ListenBrainz.
//! Uses personal user tokens (not OAuth) for authentication.
//!
//! ## Scrobbling Rules
//!
//! - Now Playing: Submitted when track starts
//! - Scrobble: Submitted after 50% of track OR 4 minutes played
//! - Skip: No submission if < 30 seconds played
//!
//! ## Usage
//!
//! ```no_run
//! use qbz_integrations::{ListenBrainzClient, ListenBrainzConfig, ListenType};
//!
//! async fn example() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ListenBrainzConfig {
//!         enabled: true,
//!         token: Some("your-user-token".to_string()),
//!         user_name: Some("username".to_string()),
//!     };
//!
//!     let client = ListenBrainzClient::with_config(config);
//!
//!     // Submit now playing
//!     client.submit_listen(
//!         ListenType::PlayingNow,
//!         "Artist",
//!         "Track",
//!         Some("Album"),
//!         None, // No timestamp for now playing
//!         None, // No MBIDs
//!     ).await?;
//!
//!     Ok(())
//! }
//! ```

mod client;
mod models;

#[cfg(feature = "cache")]
pub mod cache;

pub use client::{ListenBrainzClient, ListenBrainzConfig};
pub use models::{ListenType, TrackMetadata, AdditionalInfo, Listen, SubmitListensPayload};
