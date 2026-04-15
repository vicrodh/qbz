//! V2 Commands - Using the new multi-crate architecture
//!
//! These commands use QbzCore via CoreBridge instead of the old AppState.
//! Runtime contract ensures proper lifecycle (see ADR_RUNTIME_SESSION_CONTRACT.md).
//!
//! Playback flows through CoreBridge -> QbzCore -> Player (qbz-player crate).

#[derive(Debug, Clone, serde::Deserialize)]
pub struct V2SuggestionArtistInput {
    pub name: String,
    pub qobuz_id: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct V2PlaylistSuggestionsInput {
    pub artists: Vec<V2SuggestionArtistInput>,
    pub exclude_track_ids: Vec<u64>,
    #[serde(default)]
    pub include_reasons: bool,
    pub config: Option<crate::artist_vectors::SuggestionConfig>,
}

mod helpers;
pub use helpers::*;

mod runtime;
pub use runtime::*;

mod playback;
pub use playback::*;

mod auth;
pub use auth::*;

mod settings;
pub use settings::*;

mod library;
pub use library::*;

mod link_resolver;
pub use link_resolver::*;

mod queue;
pub use queue::*;

mod search;
pub use search::*;

mod favorites;
pub use favorites::*;

mod audio;
pub use audio::*;

mod playlists;
pub use playlists::*;

mod catalog;
pub use catalog::*;

mod integrations;
pub use integrations::*;

mod session;
pub use session::*;

mod legacy_compat;
pub use legacy_compat::*;

mod image_cache;
pub use image_cache::*;

mod discovery;
pub use discovery::*;
pub(crate) use discovery::normalize_artist_name;

mod diagnostics;
pub use diagnostics::*;
