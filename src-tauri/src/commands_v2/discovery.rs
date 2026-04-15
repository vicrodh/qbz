use std::collections::HashSet;

use tauri::State;

use crate::artist_blacklist::BlacklistState;
use crate::core_bridge::CoreBridgeState;
use crate::integrations_v2::MusicBrainzV2State;
use crate::reco_store::RecoState;

/// Normalize an artist name for dedup: trim, lowercase, collapse whitespace
pub(crate) fn normalize_artist_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Discover new artists via MusicBrainz tag-based search.
///
/// Pipeline: "Listeners also enjoy"
///
/// Uses MusicBrainz tag search to find artists that share the seed artist's
/// primary genre tag. This gives genre-accurate results (e.g., searching
/// "thrash metal" for Metallica returns Megadeth, Slayer, Anthrax — not
/// mainstream crossover like Led Zeppelin).
///
/// Pipeline:
/// 1. Fetch seed artist's tags from MusicBrainz (sorted by vote count)
/// 2. Search MB for artists tagged with the primary genre tag
/// 3. Filter: seed artist, known similar artists, local listening history
/// 4. Resolve on Qobuz (verify exact name match to avoid homonyms)
/// 5. Return top 8, minimum 5 (frontend shows 6, keeps 2 reserves)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryArtist {
    pub mbid: String,
    pub name: String,
    pub normalized_name: String,
    pub affinity_score: f64,
    pub similarity_percent: f64,
    pub qobuz_id: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResponse {
    pub artists: Vec<DiscoveryArtist>,
    pub primary_tag: String,
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_discovery_artists(
    seedMbid: String,
    seedArtistName: String,
    similarArtistNames: Vec<String>,
    musicbrainz: State<'_, MusicBrainzV2State>,
    reco_state: State<'_, RecoState>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<DiscoveryResponse, String> {
    log::info!(
        "[Discovery] Starting pipeline for {} (MBID: {})",
        seedArtistName,
        seedMbid
    );

    // Step 1: Check MB is enabled
    {
        let client = musicbrainz.client.lock().await;
        if !client.is_enabled().await {
            log::warn!("[Discovery] MusicBrainz is disabled, returning empty");
            return Ok(DiscoveryResponse {
                artists: Vec::new(),
                primary_tag: String::new(),
            });
        }
    }

    // Step 2: Get seed artist's primary genre tag
    let seed_tags = {
        let client = musicbrainz.client.lock().await;
        client.get_artist_tags(&seedMbid).await.unwrap_or_default()
    };

    if seed_tags.is_empty() {
        log::warn!("[Discovery] No tags found for seed artist, returning empty");
        return Ok(DiscoveryResponse {
            artists: Vec::new(),
            primary_tag: String::new(),
        });
    }

    let primary_tag = &seed_tags[0];
    log::info!(
        "[Discovery] Seed primary tag: '{}' (from {} tags total)",
        primary_tag,
        seed_tags.len()
    );

    // Step 3: Search MB for artists with the same primary tag
    // Request more than we need to account for filtering
    let mb_results = {
        let client = musicbrainz.client.lock().await;
        client
            .search_artists_by_tag(primary_tag, 50)
            .await
            .map_err(|e| format!("Tag search failed: {}", e))?
    };

    log::info!(
        "[Discovery] MB tag search returned {} artists for '{}'",
        mb_results.artists.len(),
        primary_tag
    );

    if mb_results.artists.is_empty() {
        return Ok(DiscoveryResponse {
            artists: Vec::new(),
            primary_tag: primary_tag.to_string(),
        });
    }

    // Step 4: Build exclusion sets
    let seed_name_normalized = normalize_artist_name(&seedArtistName);

    let similar_names_set: HashSet<String> = similarArtistNames
        .iter()
        .map(|name| normalize_artist_name(name))
        .collect();

    // Exclude any artist listened more than 2 times (user already knows them)
    let listen_threshold: u32 = 2;
    let (local_known_qobuz_ids, local_known_names): (HashSet<u64>, HashSet<String>) = {
        let guard = reco_state.db.lock().await;
        if let Some(db) = guard.as_ref() {
            let top_artists = db.get_top_artist_ids(500).unwrap_or_default();
            let qobuz_ids: HashSet<u64> = top_artists
                .iter()
                .filter(|a| a.play_count > listen_threshold)
                .map(|a| a.artist_id)
                .collect();

            let known_artists = db.get_known_artist_names(1000).unwrap_or_default();
            let known_ids: HashSet<u64> = qobuz_ids.clone();
            let names: HashSet<String> = known_artists
                .iter()
                .filter(|(id, _)| known_ids.contains(id))
                .map(|(_, name)| normalize_artist_name(name))
                .collect();

            log::debug!(
                "[Discovery] Exclusion: {} known artists (>{} plays)",
                qobuz_ids.len(),
                listen_threshold
            );

            (qobuz_ids, names)
        } else {
            (HashSet::new(), HashSet::new())
        }
    };

    // Step 4b: Load dismissed artists for this tag
    let dismissed_names: HashSet<String> = {
        let guard = reco_state.db.lock().await;
        if let Some(db) = guard.as_ref() {
            db.get_dismissed_artists_for_tag(&primary_tag.to_lowercase())
                .unwrap_or_default()
                .into_iter()
                .collect()
        } else {
            HashSet::new()
        }
    };

    if !dismissed_names.is_empty() {
        log::debug!(
            "[Discovery] {} dismissed artists for tag '{}'",
            dismissed_names.len(),
            primary_tag
        );
    }

    // Step 5: Filter MB results
    let mut candidates: Vec<(String, String)> = Vec::new(); // (mbid, name)

    for artist in &mb_results.artists {
        let normalized = normalize_artist_name(&artist.name);

        // Skip seed artist
        if normalized == seed_name_normalized || artist.id.to_lowercase() == seedMbid.to_lowercase()
        {
            continue;
        }
        // Skip artists already shown in the similar section
        if similar_names_set.contains(&normalized) {
            continue;
        }
        // Skip locally known artists
        if local_known_names.contains(&normalized) {
            continue;
        }
        // Skip dismissed artists for this tag
        if dismissed_names.contains(&normalized) {
            continue;
        }
        candidates.push((artist.id.clone(), artist.name.clone()));
    }

    // Step 6: Shuffle deterministically using seed MBID
    // This ensures: same artist page = same results, different artist = different results
    {
        use rand::seq::SliceRandom;
        use rand::SeedableRng;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        seedMbid.hash(&mut hasher);
        let hash = hasher.finish();
        let mut rng = rand::rngs::StdRng::seed_from_u64(hash);
        candidates.shuffle(&mut rng);
    }

    log::info!(
        "[Discovery] {} candidates after filtering + shuffle (from {} MB results)",
        candidates.len(),
        mb_results.artists.len()
    );

    // Step 7: Resolve on Qobuz
    let bridge_guard = bridge.try_get().await;
    let mut results: Vec<DiscoveryArtist> = Vec::new();
    let min_results = 5;
    let max_results = 8;

    if let Some(ref core_bridge) = bridge_guard {
        for (mbid, name) in &candidates {
            if results.len() >= max_results {
                break;
            }

            let qobuz_artist = match core_bridge.search_artists(name, 1, 0, None).await {
                Ok(search_results) => {
                    if let Some(artist) = search_results.items.first() {
                        let qobuz_norm = normalize_artist_name(&artist.name);
                        let cand_norm = normalize_artist_name(name);
                        if qobuz_norm == cand_norm
                            && !local_known_qobuz_ids.contains(&artist.id)
                            && !blacklist_state.is_blacklisted(artist.id)
                        {
                            Some((artist.id, artist.name.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                Err(_) => None,
            };

            if let Some((qobuz_id, qobuz_name)) = qobuz_artist {
                results.push(DiscoveryArtist {
                    mbid: mbid.to_string(),
                    name: qobuz_name.clone(),
                    normalized_name: normalize_artist_name(&qobuz_name),
                    affinity_score: 0.0,
                    similarity_percent: 0.0,
                    qobuz_id: Some(qobuz_id),
                });
            }
        }
    } else {
        log::warn!("[Discovery] CoreBridge not available");
        return Ok(DiscoveryResponse {
            artists: Vec::new(),
            primary_tag: primary_tag.to_string(),
        });
    }

    // Step 7: If not enough results with primary tag, try secondary tag
    if results.len() < min_results && seed_tags.len() > 1 {
        let secondary_tag = &seed_tags[1];
        log::info!(
            "[Discovery] Only {} results, trying secondary tag: '{}'",
            results.len(),
            secondary_tag
        );

        // Load dismissals for secondary tag too
        let secondary_dismissed: HashSet<String> = {
            let guard = reco_state.db.lock().await;
            if let Some(db) = guard.as_ref() {
                db.get_dismissed_artists_for_tag(&secondary_tag.to_lowercase())
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            } else {
                HashSet::new()
            }
        };

        let secondary_search = {
            let client = musicbrainz.client.lock().await;
            client.search_artists_by_tag(secondary_tag, 30).await
        };
        if let Ok(secondary_results) = secondary_search {
            let existing_mbids: HashSet<String> = results.iter().map(|r| r.mbid.clone()).collect();

            // Filter and shuffle secondary candidates too
            let mut secondary_candidates: Vec<(String, String)> = Vec::new();
            for artist in &secondary_results.artists {
                let normalized = normalize_artist_name(&artist.name);
                if normalized == seed_name_normalized
                    || artist.id.to_lowercase() == seedMbid.to_lowercase()
                {
                    continue;
                }
                if similar_names_set.contains(&normalized)
                    || local_known_names.contains(&normalized)
                    || dismissed_names.contains(&normalized)
                    || secondary_dismissed.contains(&normalized)
                {
                    continue;
                }
                if existing_mbids.contains(&artist.id) {
                    continue;
                }
                secondary_candidates.push((artist.id.clone(), artist.name.clone()));
            }

            {
                use rand::seq::SliceRandom;
                use rand::SeedableRng;
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};

                let mut hasher = DefaultHasher::new();
                seedMbid.hash(&mut hasher);
                secondary_tag.hash(&mut hasher);
                let hash = hasher.finish();
                let mut rng = rand::rngs::StdRng::seed_from_u64(hash);
                secondary_candidates.shuffle(&mut rng);
            }

            if let Some(ref core_bridge) = bridge_guard {
                for (mbid, name) in &secondary_candidates {
                    if results.len() >= max_results {
                        break;
                    }

                    let qobuz_artist = match core_bridge.search_artists(name, 1, 0, None).await {
                        Ok(sr) => {
                            if let Some(qa) = sr.items.first() {
                                let qobuz_norm = normalize_artist_name(&qa.name);
                                let cand_norm = normalize_artist_name(name);
                                if qobuz_norm == cand_norm
                                    && !local_known_qobuz_ids.contains(&qa.id)
                                    && !blacklist_state.is_blacklisted(qa.id)
                                {
                                    Some((qa.id, qa.name.clone()))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    };

                    if let Some((qobuz_id, qobuz_name)) = qobuz_artist {
                        results.push(DiscoveryArtist {
                            mbid: mbid.clone(),
                            name: qobuz_name.clone(),
                            normalized_name: normalize_artist_name(&qobuz_name),
                            affinity_score: 0.0,
                            similarity_percent: 0.0,
                            qobuz_id: Some(qobuz_id),
                        });
                    }
                }
            }
        }
    }

    log::info!("[Discovery] Returning {} discovery artists", results.len());
    Ok(DiscoveryResponse {
        artists: results,
        primary_tag: primary_tag.to_string(),
    })
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_dismiss_discovery_artist(
    tag: String,
    artistName: String,
    reco_state: State<'_, RecoState>,
) -> Result<(), String> {
    let normalized = normalize_artist_name(&artistName);
    let tag_lower = tag.to_lowercase();

    log::info!(
        "[Discovery] Dismissing '{}' for tag '{}'",
        normalized,
        tag_lower
    );

    let guard = reco_state.db.lock().await;
    if let Some(db) = guard.as_ref() {
        db.dismiss_discovery_artist(&tag_lower, &normalized)?;
    }
    Ok(())
}
