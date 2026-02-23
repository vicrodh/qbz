//! Vector builder - constructs artist vectors from MusicBrainz and Qobuz data
//!
//! This module integrates with MusicBrainz for relationship data and Qobuz
//! for similarity data to build sparse vectors for each artist.

use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::api::QobuzClient;
use crate::musicbrainz::models::{ArtistFullResponse, ArtistRelationships, Period, RelatedArtist};
use crate::musicbrainz::{MusicBrainzCache, MusicBrainzClient};

use super::sparse_vector::SparseVector;
use super::store::ArtistVectorStore;
use super::weights::RelationshipWeights;

/// Builder for constructing artist vectors from multiple data sources
pub struct ArtistVectorBuilder {
    /// Vector store for persistence
    store: Arc<Mutex<Option<ArtistVectorStore>>>,
    /// MusicBrainz client for relationship data
    mb_client: Arc<MusicBrainzClient>,
    /// MusicBrainz cache
    mb_cache: Arc<Mutex<Option<MusicBrainzCache>>>,
    /// Qobuz client for similar artists
    qobuz_client: Arc<RwLock<QobuzClient>>,
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
        store: Arc<Mutex<Option<ArtistVectorStore>>>,
        mb_client: Arc<MusicBrainzClient>,
        mb_cache: Arc<Mutex<Option<MusicBrainzCache>>>,
        qobuz_client: Arc<RwLock<QobuzClient>>,
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
        log::info!("[VectorBuilder] build_vector START for {}", artist_mbid);

        let mut vector = SparseVector::new();
        let mut sources = Vec::new();
        let mut mb_relations_count = 0;
        let mut qobuz_similar_count = 0;

        // Store vectors for later persistence (to avoid deadlock)
        let mut mb_vec_to_store: Option<SparseVector> = None;
        let mut qobuz_vec_to_store: Option<SparseVector> = None;

        // 1. Get or create index for this artist
        log::info!("[VectorBuilder] Acquiring store lock...");
        {
            let mut guard__ = self.store.lock().await;
            let store = guard__
                .as_mut()
                .ok_or("No active session - please log in")?;
            log::info!("[VectorBuilder] Store lock acquired, creating index");
            store.get_or_create_idx(artist_mbid, artist_name)?;
        }
        log::info!("[VectorBuilder] Store lock released");

        // 2. Fetch MusicBrainz relationships
        log::info!("[VectorBuilder] Fetching MusicBrainz relationships...");
        match self.build_from_musicbrainz(artist_mbid).await {
            Ok((mb_vec, count)) => {
                vector = vector.add(&mb_vec);
                mb_relations_count = count;
                if count > 0 {
                    sources.push("musicbrainz".to_string());
                    mb_vec_to_store = Some(mb_vec);
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to fetch MusicBrainz relations for {}: {}",
                    artist_mbid,
                    e
                );
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
                        qobuz_vec_to_store = Some(qobuz_vec);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to fetch Qobuz similar for {}: {}", qobuz_id, e);
                }
            }
        }

        // 4. Persist the vectors (using saved vectors to avoid deadlock)
        log::info!("[VectorBuilder] Persisting vectors...");
        {
            let mut guard__ = self.store.lock().await;
            let store = guard__
                .as_mut()
                .ok_or("No active session - please log in")?;

            // Store MB relationships
            if let Some(mb_vec) = mb_vec_to_store {
                store.set_vector(artist_mbid, &mb_vec, "musicbrainz")?;
                log::info!("[VectorBuilder] Stored MB vector for {}", artist_mbid);
            }

            // Store Qobuz similarities
            if let Some(qobuz_vec) = qobuz_vec_to_store {
                store.set_vector(artist_mbid, &qobuz_vec, "qobuz")?;
                log::info!("[VectorBuilder] Stored Qobuz vector for {}", artist_mbid);
            }
        }
        log::info!("[VectorBuilder] build_vector COMPLETE for {}", artist_mbid);

        Ok(BuildResult {
            vector,
            mb_relations_count,
            qobuz_similar_count,
            sources,
        })
    }

    /// Build vector component from MusicBrainz relationships
    async fn build_from_musicbrainz(
        &self,
        artist_mbid: &str,
    ) -> Result<(SparseVector, usize), String> {
        log::info!(
            "[VectorBuilder] build_from_musicbrainz: checking cache for {}",
            artist_mbid
        );

        // Try cache first
        log::info!("[VectorBuilder] Acquiring mb_cache lock...");
        let cached = {
            let guard__ = self.mb_cache.lock().await;
            let cache = guard__
                .as_ref()
                .ok_or("No active session - please log in")?;
            log::info!("[VectorBuilder] mb_cache lock acquired, checking relations");
            cache.get_artist_relations(artist_mbid)?
        };
        log::info!(
            "[VectorBuilder] mb_cache lock released, cached={}",
            cached.is_some()
        );

        let relations = if let Some(rel) = cached {
            log::info!("[VectorBuilder] Cache HIT for {}", artist_mbid);
            rel
        } else {
            log::info!(
                "[VectorBuilder] Cache MISS - calling MusicBrainz API for {}",
                artist_mbid
            );
            // Fetch from API
            let response = self
                .mb_client
                .get_artist_with_relations(artist_mbid)
                .await?;
            log::info!(
                "[VectorBuilder] MusicBrainz API response received for {}",
                artist_mbid
            );

            // Extract relationships from raw response
            let extracted = extract_relationships(&response);

            // Cache it
            {
                let guard__ = self.mb_cache.lock().await;
                let cache = guard__
                    .as_ref()
                    .ok_or("No active session - please log in")?;
                cache.set_artist_relations(artist_mbid, &extracted)?;
            }

            extracted
        };

        let mut vector = SparseVector::new();
        let mut count = 0;

        // Get store for index lookups
        let mut guard__ = self.store.lock().await;
        let store = guard__
            .as_mut()
            .ok_or("No active session - please log in")?;

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
    async fn build_from_qobuz(
        &self,
        qobuz_artist_id: u64,
    ) -> Result<(SparseVector, usize), String> {
        let similar = {
            let client = self.qobuz_client.read().await;
            client
                .get_similar_artists(qobuz_artist_id, 20, 0)
                .await
                .map_err(|e| format!("Qobuz API error: {}", e))?
        };

        let mut vector = SparseVector::new();
        let mut count = 0;
        let mut guard__ = self.store.lock().await;
        let store = guard__
            .as_mut()
            .ok_or("No active session - please log in")?;

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
            let guard__ = self.store.lock().await;
            let store = guard__
                .as_ref()
                .ok_or("No active session - please log in")?;
            store.has_fresh_vector(artist_mbid, max_age_secs)
        };

        if has_fresh {
            log::debug!(
                "[VectorBuilder] Artist {} has fresh vector, skipping build",
                artist_mbid
            );
            return Ok(false);
        }

        // Build new vector
        log::info!(
            "[VectorBuilder] Building vector for artist: {}",
            artist_mbid
        );
        match self
            .build_vector(artist_mbid, artist_name, qobuz_artist_id)
            .await
        {
            Ok(result) => {
                log::info!(
                    "[VectorBuilder] Vector built: {} MB relations, {} Qobuz similar",
                    result.mb_relations_count,
                    result.qobuz_similar_count
                );
                Ok(true)
            }
            Err(e) => {
                log::error!(
                    "[VectorBuilder] Failed to build vector for {}: {}",
                    artist_mbid,
                    e
                );
                Err(e)
            }
        }
    }
}

/// Resolve a Qobuz artist ID to a MusicBrainz ID by searching
///
/// This is a best-effort match based on artist name
pub async fn resolve_qobuz_to_mbid(
    qobuz_client: &Arc<RwLock<QobuzClient>>,
    mb_client: &Arc<MusicBrainzClient>,
    _mb_cache: &Arc<Mutex<MusicBrainzCache>>,
    qobuz_artist_id: u64,
) -> Result<Option<String>, String> {
    // Get artist name from Qobuz
    let artist_name = {
        let client = qobuz_client.read().await;
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
                role: relation
                    .attributes
                    .as_ref()
                    .and_then(|a| a.first().cloned()),
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
