//! Integrations V2 Commands
//!
//! These commands use the qbz-integrations crate which is Tauri-independent.
//! They can work without Tauri for TUI/headless clients.

use tauri::{Emitter, State};

use crate::artist_blacklist::BlacklistState;
use crate::core_bridge::CoreBridgeState;
use crate::integrations_v2::{LastFmV2State, ListenBrainzV2State, MusicBrainzV2State};
use crate::runtime::{CommandRequirement, RuntimeError, RuntimeManagerState};

// --- ListenBrainz V2 ---

/// Get ListenBrainz status (V2)
#[tauri::command]
pub async fn v2_listenbrainz_get_status(
    state: State<'_, ListenBrainzV2State>,
) -> Result<qbz_integrations::listenbrainz::ListenBrainzStatus, RuntimeError> {
    log::info!("[V2] listenbrainz_get_status");
    let client = state.client.lock().await;
    Ok(client.get_status().await)
}

/// Check if ListenBrainz is enabled (V2)
#[tauri::command]
pub async fn v2_listenbrainz_is_enabled(
    state: State<'_, ListenBrainzV2State>,
) -> Result<bool, RuntimeError> {
    let client = state.client.lock().await;
    Ok(client.is_enabled().await)
}

/// Enable or disable ListenBrainz (V2)
#[tauri::command]
pub async fn v2_listenbrainz_set_enabled(
    enabled: bool,
    state: State<'_, ListenBrainzV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] listenbrainz_set_enabled: {}", enabled);
    let client = state.client.lock().await;
    client.set_enabled(enabled).await;
    drop(client);

    // Persist to V2 cache
    let cache_guard = state.cache.lock().await;
    if let Some(cache) = cache_guard.as_ref() {
        if let Err(e) = cache.set_enabled(enabled) {
            log::warn!("[V2] Failed to persist LB enabled state: {}", e);
        }
    }
    Ok(())
}

/// Connect to ListenBrainz with token (V2)
#[tauri::command]
pub async fn v2_listenbrainz_connect(
    token: String,
    state: State<'_, ListenBrainzV2State>,
) -> Result<qbz_integrations::listenbrainz::UserInfo, RuntimeError> {
    log::info!("[V2] listenbrainz_connect");
    let client = state.client.lock().await;
    let user_info = client
        .set_token(&token)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))?;

    // Save credentials to V2 cache (persists across restarts)
    drop(client);
    state
        .save_credentials(token, user_info.user_name.clone())
        .await;

    Ok(user_info)
}

/// Disconnect from ListenBrainz (V2)
#[tauri::command]
pub async fn v2_listenbrainz_disconnect(
    state: State<'_, ListenBrainzV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] listenbrainz_disconnect");
    let client = state.client.lock().await;
    client.disconnect().await;
    drop(client);
    // Clears in-memory + V2 cache credentials
    state.clear_credentials().await;
    Ok(())
}

/// Submit now playing to ListenBrainz (V2)
#[tauri::command]
pub async fn v2_listenbrainz_now_playing(
    artist: String,
    track: String,
    album: Option<String>,
    recording_mbid: Option<String>,
    release_mbid: Option<String>,
    artist_mbids: Option<Vec<String>>,
    isrc: Option<String>,
    duration_ms: Option<u64>,
    state: State<'_, ListenBrainzV2State>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] listenbrainz_now_playing: {} - {}", artist, track);

    // Build additional info if any MusicBrainz data provided
    let additional_info = if recording_mbid.is_some()
        || release_mbid.is_some()
        || artist_mbids.is_some()
        || isrc.is_some()
        || duration_ms.is_some()
    {
        Some(qbz_integrations::listenbrainz::AdditionalInfo {
            recording_mbid,
            release_mbid,
            artist_mbids,
            isrc,
            duration_ms,
            tracknumber: None,
            media_player: "QBZ".to_string(),
            media_player_version: env!("CARGO_PKG_VERSION").to_string(),
            submission_client: "QBZ".to_string(),
            submission_client_version: env!("CARGO_PKG_VERSION").to_string(),
        })
    } else {
        None
    };

    let client = state.client.lock().await;
    client
        .submit_playing_now(&artist, &track, album.as_deref(), additional_info)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

/// Submit scrobble to ListenBrainz (V2)
#[tauri::command]
pub async fn v2_listenbrainz_scrobble(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: i64,
    recording_mbid: Option<String>,
    release_mbid: Option<String>,
    artist_mbids: Option<Vec<String>>,
    isrc: Option<String>,
    duration_ms: Option<u64>,
    state: State<'_, ListenBrainzV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] listenbrainz_scrobble: {} - {}", artist, track);

    // Build additional info if any MusicBrainz data provided
    let additional_info = if recording_mbid.is_some()
        || release_mbid.is_some()
        || artist_mbids.is_some()
        || isrc.is_some()
        || duration_ms.is_some()
    {
        Some(qbz_integrations::listenbrainz::AdditionalInfo {
            recording_mbid,
            release_mbid,
            artist_mbids,
            isrc,
            duration_ms,
            tracknumber: None,
            media_player: "QBZ".to_string(),
            media_player_version: env!("CARGO_PKG_VERSION").to_string(),
            submission_client: "QBZ".to_string(),
            submission_client_version: env!("CARGO_PKG_VERSION").to_string(),
        })
    } else {
        None
    };

    let client = state.client.lock().await;
    client
        .submit_listen(
            &artist,
            &track,
            album.as_deref(),
            timestamp,
            additional_info,
        )
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

// --- MusicBrainz V2 ---

/// Check if MusicBrainz is enabled (V2)
#[tauri::command]
pub async fn v2_musicbrainz_is_enabled(
    state: State<'_, MusicBrainzV2State>,
) -> Result<bool, RuntimeError> {
    let client = state.client.lock().await;
    Ok(client.is_enabled().await)
}

/// Enable or disable MusicBrainz (V2)
#[tauri::command]
pub async fn v2_musicbrainz_set_enabled(
    enabled: bool,
    state: State<'_, MusicBrainzV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] musicbrainz_set_enabled: {}", enabled);
    let client = state.client.lock().await;
    client.set_enabled(enabled).await;
    drop(client);

    // Persist to V2 cache
    let cache_guard = state.cache.lock().await;
    if let Some(cache) = cache_guard.as_ref() {
        if let Err(e) = cache.set_enabled(enabled) {
            log::warn!("[V2] Failed to persist MB enabled state: {}", e);
        }
    }
    Ok(())
}

/// Resolve track to MusicBrainz IDs (V2)
#[tauri::command]
pub async fn v2_musicbrainz_resolve_track(
    artist: String,
    title: String,
    isrc: Option<String>,
    state: State<'_, MusicBrainzV2State>,
) -> Result<Option<qbz_integrations::musicbrainz::ResolvedTrack>, RuntimeError> {
    log::debug!("[V2] musicbrainz_resolve_track: {} - {}", artist, title);
    let client = state.client.lock().await;
    client
        .resolve_track(&artist, &title, isrc.as_deref())
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

/// Resolve artist to MusicBrainz ID (V2)
#[tauri::command]
pub async fn v2_musicbrainz_resolve_artist(
    name: String,
    state: State<'_, MusicBrainzV2State>,
) -> Result<Option<qbz_integrations::musicbrainz::ResolvedArtist>, RuntimeError> {
    log::debug!("[V2] musicbrainz_resolve_artist: {}", name);
    let client = state.client.lock().await;
    client
        .resolve_artist(&name)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn v2_resolve_musician(
    name: String,
    role: String,
    mb_state: State<'_, MusicBrainzV2State>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<qbz_integrations::musicbrainz::ResolvedMusician, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let resolved_artist = {
        let client = mb_state.client.lock().await;
        client
            .resolve_artist(&name)
            .await
            .map_err(|e| RuntimeError::Internal(e.to_string()))?
    };

    let normalized_target = name.trim().to_lowercase();
    let bridge = bridge.get().await;
    let artist_results = bridge
        .search_artists(&name, 10, 0, None)
        .await
        .map_err(RuntimeError::Internal)?;
    let exact = artist_results
        .items
        .iter()
        .find(|artist| artist.name.trim().to_lowercase() == normalized_target);

    if let Some(artist) = exact {
        let qobuz_artist_id = i64::try_from(artist.id).ok();
        return Ok(qbz_integrations::musicbrainz::ResolvedMusician {
            name,
            role,
            mbid: None,
            qobuz_artist_id,
            confidence: qbz_integrations::musicbrainz::MusicianConfidence::Confirmed,
            bands: Vec::new(),
            appears_on_count: 0,
        });
    }

    let album_results = bridge
        .search_albums(&name, 20, 0, None)
        .await
        .map_err(RuntimeError::Internal)?;
    let appears_on_count = album_results.total as usize;

    let confidence = if appears_on_count > 0 {
        qbz_integrations::musicbrainz::MusicianConfidence::Contextual
    } else if resolved_artist.is_some() {
        qbz_integrations::musicbrainz::MusicianConfidence::Weak
    } else {
        qbz_integrations::musicbrainz::MusicianConfidence::None
    };

    Ok(qbz_integrations::musicbrainz::ResolvedMusician {
        name,
        role,
        mbid: resolved_artist.as_ref().map(|a| a.mbid.clone()),
        qobuz_artist_id: None,
        confidence,
        bands: Vec::new(),
        appears_on_count,
    })
}

#[tauri::command]
pub async fn v2_get_musician_appearances(
    name: String,
    role: String,
    limit: Option<u32>,
    offset: Option<u32>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<qbz_integrations::musicbrainz::MusicianAppearances, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let bridge = bridge.get().await;
    let results = bridge
        .search_albums(&name, limit.unwrap_or(20), offset.unwrap_or(0), None)
        .await
        .map_err(RuntimeError::Internal)?;

    let albums = results
        .items
        .into_iter()
        .map(|album| qbz_integrations::musicbrainz::AlbumAppearance {
            album_id: album.id,
            album_title: album.title,
            album_artwork: album.image.large.or(album.image.small).unwrap_or_default(),
            artist_name: album.artist.name,
            year: album.release_date_original,
            role_on_album: role.clone(),
        })
        .collect::<Vec<_>>();

    Ok(qbz_integrations::musicbrainz::MusicianAppearances {
        albums,
        total: results.total as usize,
    })
}

#[tauri::command]
pub async fn v2_remote_metadata_search(
    provider: String,
    query: String,
    artist: Option<String>,
    limit: Option<usize>,
    musicbrainz_state: State<'_, MusicBrainzV2State>,
) -> Result<Vec<crate::library::remote_metadata::RemoteAlbumSearchResult>, RuntimeError> {
    use crate::library::remote_metadata::{
        discogs_extended_to_search_result, musicbrainz_release_to_search_result, RemoteProvider,
    };
    let provider = provider
        .parse::<RemoteProvider>()
        .map_err(RuntimeError::Internal)?;
    let max = limit.unwrap_or(10).clamp(1, 25);

    match provider {
        RemoteProvider::MusicBrainz => {
            let client = musicbrainz_state.client.lock().await;
            let response = client
                .search_releases_extended(&query, artist.as_deref().unwrap_or(""), None, max)
                .await
                .map_err(|e| RuntimeError::Internal(e.to_string()))?;
            Ok(response
                .releases
                .iter()
                .map(musicbrainz_release_to_search_result)
                .collect())
        }
        RemoteProvider::Discogs => {
            let client = crate::discogs::DiscogsClient::new();
            let results = client
                .search_releases(artist.as_deref().unwrap_or(""), &query, None, max)
                .await
                .map_err(RuntimeError::Internal)?;
            Ok(results
                .iter()
                .map(discogs_extended_to_search_result)
                .collect())
        }
    }
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_remote_metadata_get_album(
    provider: String,
    providerId: String,
    musicbrainz_state: State<'_, MusicBrainzV2State>,
) -> Result<crate::library::remote_metadata::RemoteAlbumMetadata, RuntimeError> {
    use crate::library::remote_metadata::{
        discogs_full_to_metadata, musicbrainz_full_to_metadata, RemoteProvider,
    };

    let provider = provider
        .parse::<RemoteProvider>()
        .map_err(RuntimeError::Internal)?;

    match provider {
        RemoteProvider::MusicBrainz => {
            let client = musicbrainz_state.client.lock().await;
            let full = client
                .get_release_with_tracks(&providerId)
                .await
                .map_err(|e| RuntimeError::Internal(e.to_string()))?;
            Ok(musicbrainz_full_to_metadata(&full))
        }
        RemoteProvider::Discogs => {
            let id = providerId.parse::<u64>().map_err(|e| {
                RuntimeError::Internal(format!("Invalid Discogs release id: {}", e))
            })?;
            let client = crate::discogs::DiscogsClient::new();
            let full = client
                .get_release_metadata(id)
                .await
                .map_err(RuntimeError::Internal)?;
            Ok(discogs_full_to_metadata(&full))
        }
    }
}

#[tauri::command]
pub async fn v2_musicbrainz_get_artist_relationships(
    mbid: String,
    state: State<'_, MusicBrainzV2State>,
) -> Result<qbz_integrations::musicbrainz::ArtistRelationships, String> {
    use qbz_integrations::musicbrainz::{ArtistRelationships, Period, RelatedArtist};

    // Check V2 cache first
    {
        let cache_opt = state.cache.lock().await;
        if let Some(cache) = cache_opt.as_ref() {
            if let Some(cached) = cache.get_artist_relations(&mbid)? {
                return Ok(cached);
            }
        }
    }

    let client = state.client.lock().await;
    let artist = client
        .get_artist_with_relations(&mbid)
        .await
        .map_err(|e| e.to_string())?;
    drop(client);

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
                        if related.ended {
                            past_members.push(related);
                        } else {
                            members.push(related);
                        }
                    } else {
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

    let result = ArtistRelationships {
        members,
        past_members,
        groups,
        collaborators,
    };

    // Cache to V2 cache
    {
        let cache_opt = state.cache.lock().await;
        if let Some(cache) = cache_opt.as_ref() {
            let _ = cache.set_artist_relations(&mbid, &result);
        }
    }

    Ok(result)
}

/// Get artist metadata (location, genres, life span) from MusicBrainz (V2)
///
/// Returns location data (city/country), formatted date, and affinity seeds.
/// Used by the ArtistNetwork sidebar and scene discovery feature.
#[tauri::command]
pub async fn v2_musicbrainz_get_artist_metadata(
    mbid: String,
    state: State<'_, MusicBrainzV2State>,
) -> Result<qbz_integrations::musicbrainz::ArtistMetadata, String> {
    // Check V2 cache first
    {
        let cache_opt = state.cache.lock().await;
        if let Some(cache) = cache_opt.as_ref() {
            if let Some(cached) = cache.get_artist_metadata(&mbid)? {
                return Ok(cached);
            }
        }
    }

    // Fetch from MB API via V2 client
    let client = state.client.lock().await;
    let artist = client
        .get_artist_with_relations(&mbid)
        .await
        .map_err(|e| e.to_string())?;
    drop(client);

    // Extract metadata using the location discovery module (now uses V2 types)
    let mut metadata = crate::musicbrainz::location_discovery::extract_metadata(&artist);

    // Resolve real country from begin_area hierarchy (MB's "country" field is
    // where the artist is active, not where they were born/formed)
    if let Some(ref mut loc) = metadata.location {
        if loc.city.is_some() {
            if let Some(ref area_id) = loc.area_id {
                let client = state.client.lock().await;
                if let Ok(Some((country_name, country_code))) =
                    client.resolve_area_country(area_id).await
                {
                    loc.display_name = format!("{}, {}", loc.display_name, country_name);
                    loc.country = Some(country_name);
                    loc.country_code = country_code;
                }
            }
        }
    }

    // Cache to V2 cache
    {
        let cache_opt = state.cache.lock().await;
        if let Some(cache) = cache_opt.as_ref() {
            let _ = cache.set_artist_metadata(&mbid, &metadata);
        }
    }

    Ok(metadata)
}

/// Discover artists from the same location using MusicBrainz + Qobuz validation.
///
/// Pipeline:
/// 1. Check scene cache (30-day TTL)
/// 2. For each source genre, search MB: tag:"genre" AND beginarea:"area"
/// 3. Merge + deduplicate across genres, score with affinity
/// 4. Validate top candidates against Qobuz catalog
/// 5. Cache and return sorted results
#[tauri::command]
pub async fn v2_discover_artists_by_location(
    source_mbid: String,
    area_id: Option<String>,
    area_name: String,
    country: Option<String>,
    genres: Vec<String>,
    tags: Vec<String>,
    limit: usize,
    offset: usize,
    state: State<'_, MusicBrainzV2State>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
    app: tauri::AppHandle,
) -> Result<qbz_integrations::musicbrainz::LocationDiscoveryResponse, RuntimeError> {
    use crate::musicbrainz::genre_normalization::{
        extract_affinity_seeds, genre_summary, is_broad_genre,
    };
    use crate::musicbrainz::location_discovery::{build_scene_cache_key, compute_affinity_score};
    use qbz_integrations::musicbrainz::{
        AffinitySeeds, LocationCandidate, LocationDiscoveryResponse, Tag,
    };
    use std::collections::HashMap;

    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    // Step 0: Smart area resolution — resolve city → subdivision for broader results
    // e.g., Leyton → England, Seattle → Washington
    let (search_name, display_name) = if let Some(ref aid) = area_id {
        // Try to resolve city to parent subdivision
        let client = state.client.lock().await;
        match client.resolve_parent_subdivision(aid).await {
            Ok(Some((subdivision_name, _subdivision_id))) => {
                let display = if let Some(ref c) = country {
                    format!("{}, {}", c, subdivision_name)
                } else {
                    subdivision_name.clone()
                };
                log::info!(
                    "[V2] Area resolved: '{}' → subdivision '{}'",
                    area_name,
                    subdivision_name
                );
                (subdivision_name, display)
            }
            Ok(None) => {
                // No subdivision found, use area_name as-is
                let display = if let Some(ref c) = country {
                    format!("{}, {}", c, area_name)
                } else {
                    area_name.clone()
                };
                (area_name.clone(), display)
            }
            Err(e) => {
                log::warn!(
                    "[V2] Area resolution failed: {}, using '{}' directly",
                    e,
                    area_name
                );
                let display = if let Some(ref c) = country {
                    format!("{}, {}", c, area_name)
                } else {
                    area_name.clone()
                };
                (area_name.clone(), display)
            }
        }
    } else {
        let display = if let Some(ref c) = country {
            format!("{}, {}", c, area_name)
        } else {
            area_name.clone()
        };
        (area_name.clone(), display)
    };

    log::info!(
        "[V2] discover_artists_by_location: search='{}' display='{}' genres={:?} offset={}",
        search_name,
        display_name,
        genres,
        offset
    );

    // Build affinity seeds from the source artist's genres/tags
    let source_seeds = AffinitySeeds {
        genres: genres.clone(),
        tags: tags.clone(),
        normalized_seeds: genres.iter().chain(tags.iter()).cloned().collect(),
    };

    let cache_key_area = area_id.as_deref().unwrap_or(&search_name);
    let cache_key = build_scene_cache_key(cache_key_area, &source_seeds);

    // Step 1: Check scene cache
    if offset == 0 {
        let cache_opt = state.cache.lock().await;
        if let Some(cache) = cache_opt.as_ref() {
            if let Ok(Some(cached)) = cache.get_scene_cache(&cache_key) {
                log::info!(
                    "[V2] Scene cache hit for {} ({} artists)",
                    area_name,
                    cached.artists.len()
                );
                return Ok(cached);
            }
        }
    }

    // Step 2: Search MB for each genre + area combination
    // Filter out overly broad tags (latin, rock, pop, etc.) that return the entire
    // country's catalog. These are kept for affinity scoring but not used as search queries.
    let search_genres: Vec<&str> = if genres.is_empty() {
        tags.iter()
            .filter(|s| !is_broad_genre(s.as_str()))
            .take(3)
            .map(|s| s.as_str())
            .collect()
    } else {
        genres
            .iter()
            .chain(tags.iter().take(2))
            .filter(|s| !is_broad_genre(s.as_str()))
            .map(|s| s.as_str())
            .collect()
    };

    // If ALL genres were broad (edge case: artist tagged only "rock" + "pop"),
    // fall back to using them anyway — some results are better than none
    let search_genres: Vec<&str> = if search_genres.is_empty() {
        log::warn!("[V2] All genres were broad, falling back to original list");
        if genres.is_empty() {
            tags.iter().take(3).map(|s| s.as_str()).collect()
        } else {
            genres.iter().take(3).map(|s| s.as_str()).collect()
        }
    } else {
        search_genres
    };

    log::info!(
        "[V2] Search genres after broad filter: {:?} (from genres={:?}, tags={:?})",
        search_genres,
        genres,
        tags
    );

    if search_genres.is_empty() {
        return Ok(LocationDiscoveryResponse {
            artists: Vec::new(),
            scene_label: format!("{} scene", display_name),
            genre_summary: String::new(),
            total_candidates: 0,
            has_more: false,
            next_offset: 0,
        });
    }

    // Deduplicate candidates across genre queries: mbid -> (name, score_sum, genre_hits, tags)
    let mut candidate_map: HashMap<String, (String, i32, usize, Vec<String>)> = HashMap::new();
    let per_genre_limit = 200; // Get up to 200 per genre query for broader coverage

    let total_genres = search_genres.len();
    for (genre_idx, genre) in search_genres.iter().enumerate() {
        // Progress: MB search phase = 0-40%
        let progress = ((genre_idx as f64 / total_genres as f64) * 40.0) as u8;
        let _ = app.emit(
            "scene-discovery-progress",
            serde_json::json!({
                "phase": "searching",
                "progress": progress,
                "detail": genre
            }),
        );
        let client = state.client.lock().await;
        let search_result = client
            .search_artists_by_tag_and_area(
                genre,
                &search_name,
                country.as_deref(),
                per_genre_limit,
                0,
            )
            .await;
        drop(client);

        match search_result {
            Ok(response) => {
                log::info!(
                    "[V2] tag:'{}' + country:'{}' returned {} artists",
                    genre,
                    country.as_deref().unwrap_or(&search_name),
                    response.artists.len()
                );

                for artist in &response.artists {
                    // Skip the source artist
                    if artist.id == source_mbid {
                        continue;
                    }

                    let candidate_tags: Vec<String> = artist
                        .tags
                        .as_ref()
                        .map(|tag_list| {
                            tag_list
                                .iter()
                                .filter(|tag| tag.count.unwrap_or(0) > 0)
                                .map(|tag| tag.name.clone())
                                .collect()
                        })
                        .unwrap_or_default();

                    let same_city = artist
                        .begin_area
                        .as_ref()
                        .map(|ba| {
                            ba.name.eq_ignore_ascii_case(&search_name)
                                || area_id.as_deref().map(|aid| ba.id == aid).unwrap_or(false)
                        })
                        .unwrap_or(false);

                    let same_country = artist
                        .area
                        .as_ref()
                        .map(|a| a.name.eq_ignore_ascii_case(&search_name))
                        .unwrap_or(false);

                    let score = compute_affinity_score(
                        &candidate_tags,
                        &source_seeds,
                        same_city,
                        same_country,
                    );

                    let entry = candidate_map
                        .entry(artist.id.clone())
                        .or_insert_with(|| (artist.name.clone(), 0, 0, Vec::new()));
                    entry.1 += score;
                    entry.2 += 1; // appeared in N genre queries = more relevant
                                  // Merge tags
                    for tag in &candidate_tags {
                        if !entry.3.contains(tag) {
                            entry.3.push(tag.clone());
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "[V2] Genre+area search failed for tag:'{}' area:'{}': {}",
                    genre,
                    area_name,
                    e
                );
            }
        }
    }

    log::info!(
        "[V2] Merged {} unique candidates from {} genre queries",
        candidate_map.len(),
        search_genres.len()
    );

    // Step 3: Score and sort
    // Final score = affinity_score + (genre_hit_count * 15) bonus for appearing in multiple queries
    let mut scored: Vec<(String, String, Vec<String>, i32)> = candidate_map
        .into_iter()
        .map(|(mbid, (name, score, genre_hits, tag_list))| {
            let candidate_seeds = extract_affinity_seeds(
                &tag_list
                    .iter()
                    .map(|name| Tag {
                        name: name.clone(),
                        count: Some(1),
                    })
                    .collect::<Vec<_>>(),
            );
            let multi_genre_bonus = ((genre_hits as i32) - 1) * 15;
            (
                mbid,
                name,
                candidate_seeds.genres,
                score + multi_genre_bonus,
            )
        })
        .collect();

    // Sort by score descending, then by mbid for stable ordering across paginated calls
    scored.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));

    // Apply offset and limit
    let total_candidates = scored.len();
    let candidates_to_validate: Vec<_> = scored.into_iter().skip(offset).take(limit).collect();

    log::info!(
        "[V2] Validating {} candidates against Qobuz (total pool: {})",
        candidates_to_validate.len(),
        total_candidates
    );

    // Step 4: Validate against Qobuz
    let bridge_guard = bridge.try_get().await;
    let mut validated: Vec<LocationCandidate> = Vec::new();
    let total_to_validate = candidates_to_validate.len();

    let _ = app.emit(
        "scene-discovery-progress",
        serde_json::json!({
            "phase": "validating",
            "progress": 40,
            "detail": format!("{} candidates", total_to_validate)
        }),
    );

    if let Some(ref core_bridge) = bridge_guard {
        for (validate_idx, (mbid, mb_name, candidate_genres, score)) in
            candidates_to_validate.iter().enumerate()
        {
            // Progress: Qobuz validation phase = 40-95%
            if validate_idx % 5 == 0 {
                let progress = 40 + ((validate_idx as f64 / total_to_validate as f64) * 55.0) as u8;
                let _ = app.emit(
                    "scene-discovery-progress",
                    serde_json::json!({
                        "phase": "validating",
                        "progress": progress,
                        "detail": format!("{}/{}", validate_idx, total_to_validate)
                    }),
                );
            }
            let name_normalized =
                qbz_integrations::musicbrainz::cache::MusicBrainzCache::normalize_name(mb_name);

            // Search Qobuz for this artist — request multiple results to handle
            // name collisions (e.g., multiple "The Warning" artists)
            match core_bridge.search_artists(mb_name, 5, 0, None).await {
                Ok(search_results) => {
                    let mb_norm = super::normalize_artist_name(mb_name);

                    // Find best match: exact name match with most albums (popularity proxy)
                    let best_match = search_results
                        .items
                        .iter()
                        .filter(|a| {
                            super::normalize_artist_name(&a.name) == mb_norm
                                && !blacklist_state.is_blacklisted(a.id)
                        })
                        .max_by_key(|a| a.albums_count.unwrap_or(0));

                    if let Some(qobuz_artist) = best_match {
                        let image_url = qobuz_artist
                            .image
                            .as_ref()
                            .and_then(|img| img.small.as_ref().or(img.thumbnail.as_ref()).cloned());

                        let candidate = LocationCandidate {
                            mbid: mbid.clone(),
                            mb_name: mb_name.clone(),
                            qobuz_id: Some(qobuz_artist.id as i64),
                            qobuz_name: Some(qobuz_artist.name.clone()),
                            qobuz_image: image_url,
                            score: *score,
                            genres: candidate_genres.clone(),
                            qobuz_albums_count: qobuz_artist.albums_count,
                        };

                        if let Ok(json) = serde_json::to_string(&candidate) {
                            let cache_opt = state.cache.lock().await;
                            if let Some(cache) = cache_opt.as_ref() {
                                let _ = cache.set_qobuz_validation(&name_normalized, &json);
                            }
                        }

                        validated.push(candidate);
                    } else {
                        // Negative cache — TEMPORARILY DISABLED
                    }
                }
                Err(e) => {
                    log::warn!("[V2] Qobuz validation failed for '{}': {}", mb_name, e);
                }
            }
        }
    }

    let _ = app.emit(
        "scene-discovery-progress",
        serde_json::json!({
            "phase": "done",
            "progress": 100,
            "detail": format!("{} artists", validated.len())
        }),
    );

    log::info!(
        "[V2] Scene discovery complete: {} validated artists from {}",
        validated.len(),
        area_name
    );

    let scene_label = country.clone().unwrap_or_else(|| display_name.clone());
    let genre_sum = genre_summary(&source_seeds);
    let next_offset = offset + candidates_to_validate.len();
    let has_more = next_offset < total_candidates;

    let response = LocationDiscoveryResponse {
        artists: validated,
        scene_label,
        genre_summary: genre_sum,
        total_candidates,
        has_more,
        next_offset,
    };

    // Cache the full response (first page only)
    if offset == 0 && !response.artists.is_empty() {
        let cache_opt = state.cache.lock().await;
        if let Some(cache) = cache_opt.as_ref() {
            let _ = cache.set_scene_cache(&cache_key, &response);
        }
    }

    Ok(response)
}

// --- Last.fm V2 ---

/// Get Last.fm auth token and URL (V2)
#[tauri::command]
pub async fn v2_lastfm_get_auth_url(
    state: State<'_, LastFmV2State>,
) -> Result<String, RuntimeError> {
    log::info!("[V2] lastfm_get_auth_url");
    let client = state.client.lock().await;
    let (token, auth_url) = client
        .get_token()
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))?;

    // Store pending token for later session retrieval
    drop(client);
    state.set_pending_token(token).await;

    Ok(auth_url)
}

/// Complete Last.fm authentication (V2)
#[tauri::command]
pub async fn v2_lastfm_complete_auth(
    state: State<'_, LastFmV2State>,
) -> Result<qbz_integrations::LastFmSession, RuntimeError> {
    log::info!("[V2] lastfm_complete_auth");

    let token = state
        .take_pending_token()
        .await
        .ok_or_else(|| RuntimeError::Internal("No pending auth token".to_string()))?;

    let mut client = state.client.lock().await;
    let session = client
        .get_session(&token)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))?;

    Ok(session)
}

/// Check if Last.fm is authenticated (V2)
#[tauri::command]
pub async fn v2_lastfm_is_authenticated(
    state: State<'_, LastFmV2State>,
) -> Result<bool, RuntimeError> {
    let client = state.client.lock().await;
    Ok(client.is_authenticated())
}

/// Disconnect from Last.fm (V2)
#[tauri::command]
pub async fn v2_lastfm_disconnect(state: State<'_, LastFmV2State>) -> Result<(), RuntimeError> {
    log::info!("[V2] lastfm_disconnect");
    let mut client = state.client.lock().await;
    client.clear_session();
    Ok(())
}

/// Submit now playing to Last.fm (V2)
#[tauri::command]
pub async fn v2_lastfm_now_playing(
    artist: String,
    track: String,
    album: Option<String>,
    state: State<'_, LastFmV2State>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] lastfm_now_playing: {} - {}", artist, track);
    let client = state.client.lock().await;
    client
        .update_now_playing(&artist, &track, album.as_deref())
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

/// Scrobble to Last.fm (V2)
#[tauri::command]
pub async fn v2_lastfm_scrobble(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: u64,
    state: State<'_, LastFmV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] lastfm_scrobble: {} - {}", artist, track);
    let client = state.client.lock().await;
    client
        .scrobble(&artist, &track, album.as_deref(), timestamp)
        .await
        .map_err(|e| RuntimeError::Internal(e.to_string()))
}

/// Set Last.fm session key (V2)
///
/// Used to restore a previously saved session key.
#[tauri::command]
pub async fn v2_lastfm_set_session(
    session_key: String,
    state: State<'_, LastFmV2State>,
) -> Result<(), RuntimeError> {
    log::info!("[V2] lastfm_set_session");
    let mut client = state.client.lock().await;
    client.set_session_key(session_key);
    Ok(())
}

/// Queue a listen for offline submission (V2)
#[tauri::command]
pub async fn v2_listenbrainz_queue_listen(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: i64,
    recording_mbid: Option<String>,
    release_mbid: Option<String>,
    artist_mbids: Option<Vec<String>>,
    isrc: Option<String>,
    duration_ms: Option<u64>,
    state: State<'_, ListenBrainzV2State>,
) -> Result<i64, RuntimeError> {
    log::info!("[V2] listenbrainz_queue_listen: {} - {}", artist, track);

    let cache_guard = state.cache.lock().await;
    let cache = cache_guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;

    cache
        .queue_listen(
            timestamp,
            &artist,
            &track,
            album.as_deref(),
            recording_mbid.as_deref(),
            release_mbid.as_deref(),
            artist_mbids.as_deref(),
            isrc.as_deref(),
            duration_ms,
        )
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_listenbrainz_flush_queue(
    state: State<'_, ListenBrainzV2State>,
) -> Result<u32, RuntimeError> {
    let queued = {
        let cache_guard = state.cache.lock().await;
        let cache = cache_guard.as_ref().ok_or_else(|| {
            RuntimeError::Internal("No active session - please log in".to_string())
        })?;
        cache
            .get_pending_listens(500)
            .map_err(RuntimeError::Internal)?
    };

    if queued.is_empty() {
        return Ok(0);
    }

    let client = state.client.lock().await;
    let mut sent_ids = Vec::new();

    for listen in &queued {
        let additional_info = qbz_integrations::listenbrainz::AdditionalInfo {
            recording_mbid: listen.recording_mbid.clone(),
            release_mbid: listen.release_mbid.clone(),
            artist_mbids: listen.artist_mbids.clone(),
            isrc: listen.isrc.clone(),
            duration_ms: listen.duration_ms,
            tracknumber: None,
            media_player: "QBZ".to_string(),
            media_player_version: env!("CARGO_PKG_VERSION").to_string(),
            submission_client: "QBZ".to_string(),
            submission_client_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        if client
            .submit_listen(
                &listen.artist_name,
                &listen.track_name,
                listen.release_name.as_deref(),
                listen.listened_at,
                Some(additional_info),
            )
            .await
            .is_ok()
        {
            sent_ids.push(listen.id);
        }
    }
    drop(client);

    if sent_ids.is_empty() {
        return Ok(0);
    }

    let cache_guard = state.cache.lock().await;
    let cache = cache_guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session - please log in".to_string()))?;
    cache
        .mark_listens_sent(&sent_ids)
        .map_err(RuntimeError::Internal)?;

    Ok(sent_ids.len() as u32)
}
