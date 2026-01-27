//! Playlist suggestions engine
//!
//! Uses artist vectors to suggest new tracks for a playlist.
//! Algorithm:
//! 1. Extract unique artists from playlist tracks
//! 2. Compute combined playlist vector (sum + normalize)
//! 3. Find nearest artists not already in playlist
//! 4. Search Qobuz for top tracks by those artists
//! 5. Return suggested tracks with optional reasons

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::api::{QobuzClient, Track};

use super::builder::ArtistVectorBuilder;
use super::sparse_vector::SparseVector;
use super::store::ArtistVectorStore;

/// Configuration for suggestion generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionConfig {
    /// Maximum number of artists to consider for suggestions
    pub max_artists: usize,
    /// Number of tracks to fetch per artist
    pub tracks_per_artist: usize,
    /// Maximum total tracks in the suggestion pool
    pub max_pool_size: usize,
    /// Maximum age (days) for vector freshness
    pub vector_max_age_days: i64,
    /// Minimum similarity score to include artist
    pub min_similarity: f32,
}

impl Default for SuggestionConfig {
    fn default() -> Self {
        Self {
            max_artists: 20,
            tracks_per_artist: 5,
            max_pool_size: 100,
            vector_max_age_days: 7,
            min_similarity: 0.1,
        }
    }
}

/// A suggested track with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedTrack {
    /// Qobuz track ID
    pub track_id: u64,
    /// Track title
    pub title: String,
    /// Artist name
    pub artist_name: String,
    /// Artist MBID (if known)
    pub artist_mbid: Option<String>,
    /// Album title
    pub album_title: String,
    /// Album ID for cover art
    pub album_id: String,
    /// Duration in seconds
    pub duration: u32,
    /// Similarity score (higher = more similar to playlist)
    pub similarity_score: f32,
    /// Reason for suggestion (for dev mode)
    pub reason: Option<String>,
}

/// Result of suggestion generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionResult {
    /// Pool of suggested tracks
    pub tracks: Vec<SuggestedTrack>,
    /// Artists that contributed to suggestions
    pub source_artists: Vec<String>,
    /// Number of playlist artists analyzed
    pub playlist_artists_count: usize,
    /// Number of similar artists found
    pub similar_artists_count: usize,
}

/// Playlist suggestions engine
pub struct SuggestionsEngine {
    /// Vector store for similarity lookups
    store: Arc<Mutex<ArtistVectorStore>>,
    /// Vector builder for lazy construction
    builder: Arc<ArtistVectorBuilder>,
    /// Qobuz client for track search
    qobuz_client: Arc<Mutex<QobuzClient>>,
    /// Configuration
    config: SuggestionConfig,
}

impl SuggestionsEngine {
    /// Create a new suggestions engine
    pub fn new(
        store: Arc<Mutex<ArtistVectorStore>>,
        builder: Arc<ArtistVectorBuilder>,
        qobuz_client: Arc<Mutex<QobuzClient>>,
        config: SuggestionConfig,
    ) -> Self {
        Self {
            store,
            builder,
            qobuz_client,
            config,
        }
    }

    /// Generate suggestions for a playlist
    ///
    /// # Arguments
    /// * `playlist_artist_mbids` - MBIDs of artists in the playlist
    /// * `exclude_track_ids` - Track IDs to exclude (already in playlist)
    /// * `include_reasons` - Whether to include reason strings (dev mode)
    pub async fn generate_suggestions(
        &self,
        playlist_artist_mbids: &[String],
        exclude_track_ids: &HashSet<u64>,
        include_reasons: bool,
    ) -> Result<SuggestionResult, String> {
        if playlist_artist_mbids.is_empty() {
            return Ok(SuggestionResult {
                tracks: Vec::new(),
                source_artists: Vec::new(),
                playlist_artists_count: 0,
                similar_artists_count: 0,
            });
        }

        // 1. Ensure vectors exist for playlist artists
        for mbid in playlist_artist_mbids {
            let _ = self
                .builder
                .ensure_vector(mbid, None, None, self.config.vector_max_age_days)
                .await;
        }

        // 2. Compute combined playlist vector
        let playlist_vector = self.compute_playlist_vector(playlist_artist_mbids).await?;

        if playlist_vector.is_empty() {
            return Ok(SuggestionResult {
                tracks: Vec::new(),
                source_artists: Vec::new(),
                playlist_artists_count: playlist_artist_mbids.len(),
                similar_artists_count: 0,
            });
        }

        // 3. Find nearest artists (excluding playlist artists)
        let exclude_vec: Vec<String> = playlist_artist_mbids.to_vec();
        let similar_artists = {
            let store = self.store.lock().await;
            store.find_nearest(&playlist_vector, self.config.max_artists, &exclude_vec)?
        };

        let similar_artists_count = similar_artists.len();
        let mut source_artists = Vec::new();
        let mut all_tracks = Vec::new();

        // 4. Search for tracks by similar artists
        for artist in &similar_artists {
            if artist.similarity < self.config.min_similarity {
                continue;
            }

            source_artists.push(artist.name.clone().unwrap_or_else(|| artist.mbid.clone()));

            // Search Qobuz for tracks by this artist
            let tracks = self
                .search_artist_tracks(&artist.mbid, artist.name.as_deref(), artist.similarity)
                .await;

            for mut track in tracks {
                // Skip if already in playlist
                if exclude_track_ids.contains(&track.track_id) {
                    continue;
                }

                // Add reason if requested
                if include_reasons {
                    track.reason = Some(self.generate_reason(
                        &artist.mbid,
                        artist.name.as_deref(),
                        artist.similarity,
                        playlist_artist_mbids,
                    ));
                }

                all_tracks.push(track);
            }

            // Stop if we have enough tracks
            if all_tracks.len() >= self.config.max_pool_size {
                break;
            }
        }

        // 5. Sort by similarity score and limit pool size
        all_tracks.sort_by(|a, b| {
            b.similarity_score
                .partial_cmp(&a.similarity_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all_tracks.truncate(self.config.max_pool_size);

        Ok(SuggestionResult {
            tracks: all_tracks,
            source_artists,
            playlist_artists_count: playlist_artist_mbids.len(),
            similar_artists_count,
        })
    }

    /// Compute combined vector for playlist artists
    async fn compute_playlist_vector(
        &self,
        artist_mbids: &[String],
    ) -> Result<SparseVector, String> {
        let mut combined = SparseVector::new();
        let store = self.store.lock().await;

        for mbid in artist_mbids {
            if let Some(vector) = store.get_vector(mbid) {
                combined = combined.add(&vector);
            }
        }

        // Normalize to unit length
        Ok(combined.normalize())
    }

    /// Search Qobuz for tracks by an artist
    async fn search_artist_tracks(
        &self,
        artist_mbid: &str,
        artist_name: Option<&str>,
        similarity: f32,
    ) -> Vec<SuggestedTrack> {
        let search_query = match artist_name {
            Some(name) => name.to_string(),
            None => {
                // Try to get name from store
                let store = self.store.lock().await;
                store
                    .get_artist_name(artist_mbid)
                    .unwrap_or_else(|| artist_mbid.to_string())
            }
        };

        let client = self.qobuz_client.lock().await;

        // Search for tracks by artist name
        match client
            .search_tracks(&search_query, self.config.tracks_per_artist as u32, 0, None)
            .await
        {
            Ok(results) => {
                let mut tracks = Vec::new();

                for item in results.items {
                    tracks.push(self.track_to_suggested(&item, artist_mbid, similarity));
                }

                tracks
            }
            Err(e) => {
                log::warn!("Failed to search tracks for {}: {}", search_query, e);
                Vec::new()
            }
        }
    }

    /// Convert a Track to a SuggestedTrack
    fn track_to_suggested(&self, track: &Track, artist_mbid: &str, similarity: f32) -> SuggestedTrack {
        // Extract album info
        let (album_title, album_id) = match &track.album {
            Some(album) => (album.title.clone(), album.id.clone()),
            None => (String::new(), String::new()),
        };

        // Extract artist name from track performer
        let track_artist = track
            .performer
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default();

        SuggestedTrack {
            track_id: track.id,
            title: track.title.clone(),
            artist_name: track_artist,
            artist_mbid: Some(artist_mbid.to_string()),
            album_title,
            album_id,
            duration: track.duration,
            similarity_score: similarity,
            reason: None,
        }
    }

    /// Generate a human-readable reason for suggestion
    fn generate_reason(
        &self,
        _artist_mbid: &str,
        artist_name: Option<&str>,
        similarity: f32,
        _playlist_artists: &[String],
    ) -> String {
        let name = artist_name.unwrap_or("Unknown");
        let score_pct = (similarity * 100.0).round() as u32;

        format!("Similar to your playlist ({score_pct}% match) - {name}")
    }
}

/// Extract unique artist MBIDs from playlist tracks
///
/// This is a helper function that should be called from the Tauri command
/// to get artist MBIDs from track data.
pub fn extract_artist_mbids(
    tracks: &[(u64, Option<String>)], // (track_id, artist_mbid)
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut mbids = Vec::new();

    for (_, mbid) in tracks {
        if let Some(id) = mbid {
            if !id.is_empty() && seen.insert(id.clone()) {
                mbids.push(id.clone());
            }
        }
    }

    mbids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_artist_mbids() {
        let tracks = vec![
            (1, Some("mbid-1".to_string())),
            (2, Some("mbid-2".to_string())),
            (3, Some("mbid-1".to_string())), // Duplicate
            (4, None),                        // No MBID
            (5, Some("".to_string())),        // Empty MBID
            (6, Some("mbid-3".to_string())),
        ];

        let mbids = extract_artist_mbids(&tracks);

        assert_eq!(mbids.len(), 3);
        assert!(mbids.contains(&"mbid-1".to_string()));
        assert!(mbids.contains(&"mbid-2".to_string()));
        assert!(mbids.contains(&"mbid-3".to_string()));
    }

    #[test]
    fn test_suggestion_config_default() {
        let config = SuggestionConfig::default();

        assert_eq!(config.max_artists, 20);
        assert_eq!(config.tracks_per_artist, 5);
        assert_eq!(config.max_pool_size, 100);
        assert_eq!(config.vector_max_age_days, 7);
        assert!(config.min_similarity > 0.0);
    }
}
