//! Artist vector module for similarity-based discovery
//!
//! This module provides sparse vector representations of artists based on their
//! relationships (from MusicBrainz) and similarity (from Qobuz). These vectors
//! enable playlist suggestions by finding artists geometrically close to a
//! playlist's combined vector.
//!
//! ## Architecture
//!
//! - `sparse_vector.rs`: Memory-efficient sparse vector with math operations
//! - `store.rs`: SQLite persistence for artist indices and vectors
//! - `weights.rs`: Configurable weights for different relationship types (Phase 2)
//! - `builder.rs`: Vector construction from MusicBrainz + Qobuz (Phase 2)
//! - `suggestions.rs`: Playlist suggestion engine (Phase 3)
//!
//! ## Usage
//!
//! ```ignore
//! // Build vectors for artists
//! let builder = ArtistVectorBuilder::new(store, mb_client, qobuz_client);
//! builder.ensure_vector("artist-mbid", 7).await?;
//!
//! // Compute playlist vector
//! let playlist_vec = compute_playlist_vector(&artist_mbids);
//!
//! // Find similar artists
//! let suggestions = store.find_nearest(&playlist_vec, 10, &exclude)?;
//! ```
//!
//! ## Vector Representation
//!
//! Each artist is represented as a sparse vector where:
//! - Dimensions correspond to other artists (via integer index)
//! - Values are weighted relationship strengths
//! - Sources include: MusicBrainz relationships, Qobuz similarity
//!
//! Example:
//! ```text
//! Pink Floyd vector:
//!   [Roger Waters: 1.0 (member_of_band),
//!    David Gilmour: 1.0 (member_of_band),
//!    King Crimson: 0.7 (qobuz_similar),
//!    ...]
//! ```

pub mod builder;
pub mod sparse_vector;
pub mod store;
pub mod suggestions;
pub mod weights;

pub use builder::{ArtistVectorBuilder, BuildResult};
pub use sparse_vector::SparseVector;
pub use store::{
    ArtistVectorStore, ArtistVectorStoreState, SimilarArtist, StoreStats, VectorEntry,
};
pub use suggestions::{
    extract_artist_mbids, SuggestedTrack, SuggestionConfig, SuggestionResult, SuggestionsEngine,
};
pub use weights::RelationshipWeights;
