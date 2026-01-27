//! Vector builder - constructs artist vectors from MusicBrainz and Qobuz data
//!
//! This module integrates with MusicBrainz for relationship data and Qobuz
//! for similarity data to build sparse vectors for each artist.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::QobuzClient;
use crate::musicbrainz::models::{
    ArtistFullResponse, ArtistRelationships, Period, RelatedArtist,
};
use crate::musicbrainz::{MusicBrainzCache, MusicBrainzClient};

use super::sparse_vector::SparseVector;
use super::store::ArtistVectorStore;
use super::weights::RelationshipWeights;

/// Builder for constructing artist vectors from multiple data sources
pub struct ArtistVectorBuilder {
    /// Vector store for persistence
    store: Arc<Mutex<ArtistVectorStore>>,
    /// MusicBrainz client for relationship data
    mb_client: Arc<MusicBrainzClient>,
    /// MusicBrainz cache
    mb_cache: Arc<Mutex<MusicBrainzCache>>,
    /// Qobuz client for similar artists
    qobuz_client: Arc<Mutex<QobuzClient>>,
    /// Configurable weights
    weights: RelationshipWeights,
}

/// Result of building a vector
#[derive(Debug, Clone)]
pub struct BuildResult {
    /// The built vector
    pub vector: SparseVector,
    /// Number of MusicBrainz relationships found
    pub mb_relations_count: usize,
    /// Number of Qobuz similar artists found
    pub qobuz_similar_count: usize,
    /// Sources that contributed to the vector
    pub sources: Vec<String>,
}

impl ArtistVectorBuilder {
    /// Create a new builder with the given dependencies
    pub fn new(
        store: Arc<Mutex<ArtistVectorStore>>,
        mb_client: Arc<MusicBrainzClient>,
        mb_cache: Arc<Mutex<MusicBrainzCache>>,
        qobuz_client: Arc<Mutex<QobuzClient>>,
        weights: RelationshipWeights,
    ) -> Self {
        Self {
            store,
            mb_client,
            mb_cache,
            qobuz_client,
            weights,
        }
    }

    /// Build a vector for an artist, fetching data from all sources
    ///
    /// This method:
    /// 1. Fetches MusicBrainz relationships (members, groups, collaborators)
    /// 2. Fetches Qobuz similar artists (if Qobuz ID available)
    /// 3. Combines all data into a weighted sparse vector
    /// 4. Persists the vector to the store
    pub async fn build_vector(
        &self,
        artist_mbid: &str,
        artist_name: Option<&str>,
        qobuz_artist_id: Option<u64>,
    ) -> Result<BuildResult, String> {
        let mut vector = SparseVector::new();
        let mut sources = Vec::new();
        let mut mb_relations_count = 0;
        let mut qobuz_similar_count = 0;

        // 1. Get or create index for this artist
        {
            let mut store = self.store.lock().await;
            store.get_or_create_idx(artist_mbid, artist_name)?;
        }

        // 2. Fetch MusicBrainz relationships
        match self.build_from_musicbrainz(artist_mbid).await {
            Ok((mb_vec, count)) => {
                vector = vector.add(&mb_vec);
                mb_relations_count = count;
                if count > 0 {
                    sources.push("musicbrainz".to_string());
                }
            }
            Err(e) => {
                log::warn!("Failed to fetch MusicBrainz relations for {}: {}", artist_mbid, e);
            }
        }

        // 3. Fetch Qobuz similar artists (if we have the ID)
        if let Some(qobuz_id) = qobuz_artist_id {
            match self.build_from_qobuz(qobuz_id).await {
                Ok((qobuz_vec, count)) => {
                    vector = vector.add(&qobuz_vec);
                    qobuz_similar_count = count;
                    if count > 0 {
                        sources.push("qobuz".to_string());
                    }
                }
                Err(e) => {
                    log::warn!("Failed to fetch Qobuz similar for {}: {}", qobuz_id, e);
                }
            }
        }

        // 4. Persist the vector
        {
            let mut store = self.store.lock().await;

            // Store MB relationships
            if mb_relations_count > 0 {
                // We need to rebuild just the MB part for proper source tagging
                if let Ok((mb_vec, _)) = self.build_from_musicbrainz(artist_mbid).await {
                    store.set_vector(artist_mbid, &mb_vec, "musicbrainz")?;
                }
            }

            // Store Qobuz similarities
            if qobuz_similar_count > 0 {
                if let Some(qobuz_id) = qobuz_artist_id {
                    if let Ok((qobuz_vec, _)) = self.build_from_qobuz(qobuz_id).await {
                        store.set_vector(artist_mbid, &qobuz_vec, "qobuz")?;
                    }
                }
            }
        }

        Ok(BuildResult {
            vector,
            mb_relations_count,
            qobuz_similar_count,
            sources,
        })
    }

    /// Build vector component from MusicBrainz relationships
    async fn build_from_musicbrainz(&self, artist_mbid: &str) -> Result<(SparseVector, usize), String> {
        // Try cache first
        let cached = {
            let cache = self.mb_cache.lock().await;
            cache.get_artist_relations(artist_mbid)?
        };

        let relations = if let Some(rel) = cached {
            rel
        } else {
            // Fetch from API
            let response = self.mb_client.get_artist_with_relations(artist_mbid).await?;

            // Extract relationships from raw response
            let extracted = extract_relationships(&response);

            // Cache it
            {
                let cache = self.mb_cache.lock().await;
                cache.set_artist_relations(artist_mbid, &extracted)?;
            }

            extracted
        };

        let mut vector = SparseVector::new();
        let mut count = 0;

        // Get store for index lookups
        let mut store = self.store.lock().await;

        // Process members (band → person)
        for member in &relations.members {
            let idx = store.get_or_create_idx(&member.mbid, Some(&member.name))?;
            let weight = self.weights.member_of_band;
            vector.set(idx, weight);
            count += 1;
        }

        // Process past members (slightly lower weight)
        for member in &relations.past_members {
            let idx = store.get_or_create_idx(&member.mbid, Some(&member.name))?;
            let weight = self.weights.member_of_band * 0.8; // Past members slightly less relevant
            vector.set(idx, weight);
            count += 1;
        }

        // Process groups (person → band)
        for group in &relations.groups {
            let idx = store.get_or_create_idx(&group.mbid, Some(&group.name))?;
            let weight = self.weights.has_member;
            vector.set(idx, weight);
            count += 1;
        }

        // Process collaborators
        for collab in &relations.collaborators {
            let idx = store.get_or_create_idx(&collab.mbid, Some(&collab.name))?;
            let weight = self.weights.collaboration;
            vector.set(idx, weight);
            count += 1;
        }

        Ok((vector, count))
    }

    /// Build vector component from Qobuz similar artists
    async fn build_from_qobuz(&self, qobuz_artist_id: u64) -> Result<(SparseVector, usize), String> {
        let similar = {
            let client = self.qobuz_client.lock().await;
            client
                .get_similar_artists(qobuz_artist_id, 20, 0)
                .await
                .map_err(|e| format!("Qobuz API error: {}", e))?
        };

        let mut vector = SparseVector::new();
        let mut count = 0;
        let mut store = self.store.lock().await;

        for artist in similar.items {
            // We need to resolve Qobuz artist to MBID
            // For now, use a synthetic MBID based on Qobuz ID (we'll improve this later)
            let synthetic_mbid = format!("qobuz:{}", artist.id);

            let idx = store.get_or_create_idx(&synthetic_mbid, Some(&artist.name))?;
            let weight = self.weights.qobuz_similar;
            vector.set(idx, weight);
            count += 1;
        }

        Ok((vector, count))
    }

    /// Ensure a vector exists and is fresh, building if necessary
    ///
    /// Returns true if vector was built/updated, false if existing vector was used
    pub async fn ensure_vector(
        &self,
        artist_mbid: &str,
        artist_name: Option<&str>,
        qobuz_artist_id: Option<u64>,
        max_age_days: i64,
    ) -> Result<bool, String> {
        let max_age_secs = max_age_days * 24 * 60 * 60;

        // Check if we have a fresh vector
        let has_fresh = {
            let store = self.store.lock().await;
            store.has_fresh_vector(artist_mbid, max_age_secs)
        };

        if has_fresh {
            return Ok(false);
        }

        // Build new vector
        self.build_vector(artist_mbid, artist_name, qobuz_artist_id).await?;

        Ok(true)
    }
}

/// Resolve a Qobuz artist ID to a MusicBrainz ID by searching
///
/// This is a best-effort match based on artist name
pub async fn resolve_qobuz_to_mbid(
    qobuz_client: &Arc<Mutex<QobuzClient>>,
    mb_client: &Arc<MusicBrainzClient>,
    _mb_cache: &Arc<Mutex<MusicBrainzCache>>,
    qobuz_artist_id: u64,
) -> Result<Option<String>, String> {
    // Get artist name from Qobuz
    let artist_name = {
        let client = qobuz_client.lock().await;
        let artist = client
            .get_artist(qobuz_artist_id, false)
            .await
            .map_err(|e| format!("Failed to get Qobuz artist: {}", e))?;
        artist.name
    };

    // Search MusicBrainz for this artist
    let mb_result = mb_client.search_artist(&artist_name).await?;

    // Return first high-confidence match (score >= 80)
    if let Some(artist) = mb_result.artists.first() {
        if let Some(score) = artist.score {
            if score >= 80 {
                return Ok(Some(artist.id.clone()));
            }
        }
    }

    Ok(None)
}

/// Extract ArtistRelationships from raw MusicBrainz response
fn extract_relationships(artist: &ArtistFullResponse) -> ArtistRelationships {
    let mut members = Vec::new();
    let mut past_members = Vec::new();
    let mut groups = Vec::new();
    let mut collaborators = Vec::new();

    if let Some(relations) = &artist.relations {
        for relation in relations {
            let Some(related_artist) = &relation.artist else {
                continue;
            };

            let related = RelatedArtist {
                mbid: related_artist.id.clone(),
                name: related_artist.name.clone(),
                role: relation.attributes.as_ref().and_then(|a| a.first().cloned()),
                period: Some(Period {
                    begin: relation.begin.clone(),
                    end: relation.end.clone(),
                }),
                ended: relation.ended.unwrap_or(false),
            };

            match relation.relation_type.as_str() {
                "member of band" => {
                    if relation.direction.as_deref() == Some("backward") {
                        // We're viewing a BAND, the related artist is a MEMBER
                        if related.ended {
                            past_members.push(related);
                        } else {
                            members.push(related);
                        }
                    } else {
                        // We're viewing a PERSON, the related artist is a BAND/GROUP
                        groups.push(related);
                    }
                }
                "collaboration" => {
                    collaborators.push(related);
                }
                _ => {}
            }
        }
    }

    ArtistRelationships {
        members,
        past_members,
        groups,
        collaborators,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests would require mocking the clients
    // For now, we test the weight application logic

    #[test]
    fn test_weights_applied() {
        let weights = RelationshipWeights::default();

        assert!(weights.member_of_band > weights.collaboration);
        assert!(weights.collaboration > weights.shared_tag);
    }
}
