//! MusicBrainz Tauri commands
//!
//! Exposes MusicBrainz entity resolution to the frontend

use tauri::State;

use crate::musicbrainz::{
    ArtistRelationships, CacheStats, MatchConfidence, MusicBrainzSharedState, ResolvedArtist,
    ResolvedRelease, ResolvedTrack,
};

/// Resolve a track to MusicBrainz by ISRC
#[tauri::command]
pub async fn musicbrainz_resolve_track(
    isrc: Option<String>,
    title: String,
    artist: String,
    state: State<'_, MusicBrainzSharedState>,
) -> Result<ResolvedTrack, String> {
    if !state.client.is_enabled().await {
        return Ok(ResolvedTrack::empty());
    }

    // Check cache first (short lock)
    if let Some(ref isrc_val) = isrc {
        let cached = {
            let cache_opt__ = state.cache.lock().await;
            let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
            cache.get_recording(isrc_val).ok().flatten()
        };
        if let Some(cached) = cached {
            log::debug!("MusicBrainz cache hit for ISRC: {}", isrc_val);
            return Ok(cached);
        }
    }

    // Try ISRC lookup
    if let Some(ref isrc_val) = isrc {
        match state.client.search_recording_by_isrc(isrc_val).await {
            Ok(response) => {
                if let Some(recording) = response.recordings.first() {
                    let resolved = recording_to_resolved(recording);
                    // Cache the result (short lock)
                    {
                        let cache_opt__ = state.cache.lock().await;
                        let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
                        let _ = cache.set_recording(isrc_val, &resolved);
                    }
                    return Ok(resolved);
                }
            }
            Err(e) => {
                log::warn!("MusicBrainz ISRC lookup failed: {}", e);
            }
        }
    }

    // Fallback to title+artist search
    match state.client.search_recording(&title, &artist).await {
        Ok(response) => {
            if let Some(recording) = response
                .recordings
                .iter()
                .find(|r| r.score.unwrap_or(0) >= 80)
            {
                let resolved = recording_to_resolved(recording);
                // Cache by ISRC if available
                if let Some(ref isrc_val) = isrc {
                    let cache_opt__ = state.cache.lock().await;
                    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
                    let _ = cache.set_recording(isrc_val, &resolved);
                }
                return Ok(resolved);
            }
        }
        Err(e) => {
            log::warn!("MusicBrainz recording search failed: {}", e);
        }
    }

    // Cache negative result
    if let Some(ref isrc_val) = isrc {
        let empty = ResolvedTrack::empty();
        let cache_opt__ = state.cache.lock().await;
        let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
        let _ = cache.set_recording(isrc_val, &empty);
    }

    Ok(ResolvedTrack::empty())
}

/// Resolve an artist to MusicBrainz by name
#[tauri::command]
pub async fn musicbrainz_resolve_artist(
    name: String,
    state: State<'_, MusicBrainzSharedState>,
) -> Result<ResolvedArtist, String> {
    if !state.client.is_enabled().await {
        return Ok(ResolvedArtist::empty());
    }

    // Check cache first
    let cached = {
        let cache_opt__ = state.cache.lock().await;
        let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
        cache.get_artist(&name).ok().flatten()
    };
    if let Some(cached) = cached {
        log::debug!("MusicBrainz cache hit for artist: {}", name);
        return Ok(cached);
    }

    match state.client.search_artist(&name).await {
        Ok(response) => {
            if let Some(artist) = response
                .artists
                .iter()
                .find(|a| a.score.unwrap_or(0) >= 90)
            {
                let resolved = artist_to_resolved(artist);
                // Cache the result
                {
                    let cache_opt__ = state.cache.lock().await;
                    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
                    let _ = cache.set_artist(&name, &resolved);
                }
                return Ok(resolved);
            }
        }
        Err(e) => {
            log::warn!("MusicBrainz artist search failed: {}", e);
        }
    }

    // Cache negative result
    let empty = ResolvedArtist::empty();
    {
        let cache_opt__ = state.cache.lock().await;
        let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
        let _ = cache.set_artist(&name, &empty);
    }

    Ok(empty)
}

/// Resolve a release to MusicBrainz by UPC/barcode
#[tauri::command]
pub async fn musicbrainz_resolve_release(
    upc: Option<String>,
    title: String,
    artist: String,
    state: State<'_, MusicBrainzSharedState>,
) -> Result<ResolvedRelease, String> {
    if !state.client.is_enabled().await {
        return Ok(ResolvedRelease::empty());
    }

    // Check cache first
    if let Some(ref upc_val) = upc {
        let cached = {
            let cache_opt__ = state.cache.lock().await;
            let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
            cache.get_release(upc_val).ok().flatten()
        };
        if let Some(cached) = cached {
            log::debug!("MusicBrainz cache hit for UPC: {}", upc_val);
            return Ok(cached);
        }
    }

    // Try barcode lookup
    if let Some(ref upc_val) = upc {
        match state.client.search_release_by_barcode(upc_val).await {
            Ok(response) => {
                if let Some(release) = response.releases.first() {
                    let resolved = release_to_resolved(release);
                    // Cache the result
                    {
                        let cache_opt__ = state.cache.lock().await;
                        let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
                        let _ = cache.set_release(upc_val, &resolved);
                    }
                    return Ok(resolved);
                }
            }
            Err(e) => {
                log::warn!("MusicBrainz barcode lookup failed: {}", e);
            }
        }
    }

    // Fallback to title+artist search
    match state.client.search_release(&title, &artist).await {
        Ok(response) => {
            if let Some(release) = response
                .releases
                .iter()
                .find(|r| r.score.unwrap_or(0) >= 80)
            {
                let resolved = release_to_resolved(release);
                // Cache by UPC if available
                if let Some(ref upc_val) = upc {
                    let cache_opt__ = state.cache.lock().await;
                    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
                    let _ = cache.set_release(upc_val, &resolved);
                }
                return Ok(resolved);
            }
        }
        Err(e) => {
            log::warn!("MusicBrainz release search failed: {}", e);
        }
    }

    // Cache negative result
    if let Some(ref upc_val) = upc {
        let empty = ResolvedRelease::empty();
        let cache_opt__ = state.cache.lock().await;
        let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
        let _ = cache.set_release(upc_val, &empty);
    }

    Ok(ResolvedRelease::empty())
}

/// Get artist relationships by MBID (for Stage 3)
#[tauri::command]
pub async fn musicbrainz_get_artist_relationships(
    mbid: String,
    state: State<'_, MusicBrainzSharedState>,
) -> Result<ArtistRelationships, String> {
    if !state.client.is_enabled().await {
        return Ok(ArtistRelationships::empty());
    }

    // Check cache first
    let cached = {
        let cache_opt__ = state.cache.lock().await;
        let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
        cache.get_artist_relations(&mbid).ok().flatten()
    };
    if let Some(cached) = cached {
        log::debug!("MusicBrainz cache hit for artist relations: {}", mbid);
        return Ok(cached);
    }

    match state.client.get_artist_with_relations(&mbid).await {
        Ok(response) => {
            let relations = extract_relationships(&response);
            // Cache the result
            {
                let cache_opt__ = state.cache.lock().await;
                let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
                let _ = cache.set_artist_relations(&mbid, &relations);
            }
            Ok(relations)
        }
        Err(e) => {
            log::warn!("MusicBrainz artist relations lookup failed: {}", e);
            // Cache negative result
            let empty = ArtistRelationships::empty();
            {
                let cache_opt__ = state.cache.lock().await;
                let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
                let _ = cache.set_artist_relations(&mbid, &empty);
            }
            Ok(empty)
        }
    }
}

/// Check if MusicBrainz integration is enabled
#[tauri::command]
pub async fn musicbrainz_is_enabled(
    state: State<'_, MusicBrainzSharedState>,
) -> Result<bool, String> {
    Ok(state.client.is_enabled().await)
}

/// Enable or disable MusicBrainz integration
#[tauri::command]
pub async fn musicbrainz_set_enabled(
    enabled: bool,
    state: State<'_, MusicBrainzSharedState>,
) -> Result<(), String> {
    state.client.set_enabled(enabled).await;
    log::info!(
        "MusicBrainz integration {}",
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

/// Get MusicBrainz cache statistics
#[tauri::command]
pub async fn musicbrainz_get_cache_stats(
    state: State<'_, MusicBrainzSharedState>,
) -> Result<CacheStats, String> {
    let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
    cache.get_stats()
}

/// Clear MusicBrainz cache
#[tauri::command]
pub async fn musicbrainz_clear_cache(
    state: State<'_, MusicBrainzSharedState>,
) -> Result<(), String> {
    let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
    cache.clear_all()
}

/// Cleanup expired cache entries
#[tauri::command]
pub async fn musicbrainz_cleanup_cache(
    state: State<'_, MusicBrainzSharedState>,
) -> Result<usize, String> {
    let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
    cache.cleanup_expired()
}

// ============ Helper functions ============

use crate::musicbrainz::models::{
    ArtistFullResponse, ArtistResult, RecordingResult, RelatedArtist, ReleaseResult, Period,
};

fn recording_to_resolved(recording: &RecordingResult) -> ResolvedTrack {
    let artist_credit = recording.artist_credit.as_ref().map(|credits| {
        credits
            .iter()
            .map(|c| {
                format!(
                    "{}{}",
                    c.name.as_deref().unwrap_or(&c.artist.name),
                    c.joinphrase.as_deref().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("")
    });

    let artist_mbids = recording
        .artist_credit
        .as_ref()
        .map(|credits| credits.iter().map(|c| c.artist.id.clone()).collect());

    let (release_mbid, release_title) = recording
        .releases
        .as_ref()
        .and_then(|releases| releases.first())
        .map(|r| (Some(r.id.clone()), r.title.clone()))
        .unwrap_or((None, None));

    ResolvedTrack {
        mbid: Some(recording.id.clone()),
        title: recording.title.clone(),
        artist_credit,
        artist_mbids,
        release_mbid,
        release_title,
        confidence: MatchConfidence::from_score(recording.score),
    }
}

fn artist_to_resolved(artist: &ArtistResult) -> ResolvedArtist {
    use crate::musicbrainz::ArtistType;

    ResolvedArtist {
        mbid: Some(artist.id.clone()),
        name: Some(artist.name.clone()),
        sort_name: artist.sort_name.clone(),
        artist_type: Some(ArtistType::from(artist.artist_type.as_deref())),
        country: artist.country.clone(),
        disambiguation: artist.disambiguation.clone(),
        confidence: MatchConfidence::from_score(artist.score),
    }
}

fn release_to_resolved(release: &ReleaseResult) -> ResolvedRelease {
    let (label, catalog_number) = release
        .label_info
        .as_ref()
        .and_then(|info| info.first())
        .map(|li| (li.label.as_ref().map(|l| l.name.clone()), li.catalog_number.clone()))
        .unwrap_or((None, None));

    ResolvedRelease {
        mbid: Some(release.id.clone()),
        title: Some(release.title.clone()),
        release_group_mbid: release.release_group.as_ref().map(|rg| rg.id.clone()),
        label,
        catalog_number,
        country: release.country.clone(),
        date: release.date.clone(),
        confidence: MatchConfidence::from_score(release.score),
    }
}

fn extract_relationships(artist: &ArtistFullResponse) -> ArtistRelationships {
    let mut members = Vec::new();
    let past_members = Vec::new();
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
                        // All members go to the same list - don't separate by ended status
                        members.push(related);
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
