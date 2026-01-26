//! Musician resolution commands
//!
//! Handles musician discovery and confidence assessment for the
//! Musician Page and Informational Modal features.

use std::collections::HashSet;

use tauri::State;

// Note: We use Qobuz search APIs which return these types internally
use crate::musicbrainz::{
    AlbumAppearance, MusicBrainzSharedState, MusicianAppearances, MusicianConfidence,
    ResolvedMusician,
};
use crate::AppState;

/// Resolve a musician from credits to determine navigation destination
///
/// Returns a ResolvedMusician with:
/// - Confidence level (determines UI: Artist Page, Musician Page, or Modal)
/// - Qobuz artist ID if found (for navigation to Artist Page)
/// - MusicBrainz data if available (bands, MBID)
/// - Appears-on count from Qobuz
#[tauri::command]
pub async fn resolve_musician(
    name: String,
    role: String,
    state: State<'_, AppState>,
    mb_state: State<'_, MusicBrainzSharedState>,
) -> Result<ResolvedMusician, String> {
    log::info!("Resolving musician: {} (role: {})", name, role);

    let mut musician = ResolvedMusician::empty(name.clone(), role.clone());

    // Step 1: Search Qobuz for artist by exact name
    let qobuz_artist = {
        let client = state.client.lock().await;
        match client.search_artists(&name, 5, 0, None).await {
            Ok(results) => results
                .items
                .into_iter()
                .find(|a| a.name.to_lowercase() == name.to_lowercase()),
            Err(e) => {
                log::warn!("Qobuz artist search failed: {}", e);
                None
            }
        }
    };

    // Step 2: If found, check if they have a real catalog
    if let Some(artist) = qobuz_artist {
        let has_catalog = artist.albums_count.unwrap_or(0) > 0;

        if has_catalog {
            // Confidence = 3: Has a dedicated Qobuz artist page
            musician.qobuz_artist_id = Some(artist.id as i64);
            musician.confidence = MusicianConfidence::Confirmed;
            log::info!(
                "Musician {} resolved as Confirmed (Qobuz artist ID: {})",
                name,
                artist.id
            );
        }
    }

    // Step 3: If no artist page, search for appearances in track credits
    if musician.confidence == MusicianConfidence::None {
        let appearances_count = count_track_appearances(&state, &name).await;
        musician.appears_on_count = appearances_count;

        if appearances_count >= 3 {
            // Confidence = 2: Appears in multiple album credits
            musician.confidence = MusicianConfidence::Contextual;
            log::info!(
                "Musician {} resolved as Contextual ({} appearances)",
                name,
                appearances_count
            );
        } else if appearances_count > 0 {
            // Confidence = 1: Sparse results
            musician.confidence = MusicianConfidence::Weak;
            log::info!(
                "Musician {} resolved as Weak ({} appearances)",
                name,
                appearances_count
            );
        }
    }

    // Step 4: Try MusicBrainz enrichment (if enabled)
    if mb_state.client.is_enabled().await {
        match enrich_from_musicbrainz(&mb_state, &name).await {
            Ok((mbid, bands)) => {
                musician.mbid = mbid;
                musician.bands = bands;
            }
            Err(e) => {
                log::warn!("MusicBrainz enrichment failed for {}: {}", name, e);
                // Continue without MB data - don't block
            }
        }
    }

    log::info!(
        "Musician {} final confidence: {:?}",
        name,
        musician.confidence
    );
    Ok(musician)
}

/// Get albums a musician appears on (Qobuz only)
///
/// Returns deduplicated album appearances sorted by year.
/// This is the data source for the "Appears On" section of the Musician Page.
#[tauri::command]
pub async fn get_musician_appearances(
    name: String,
    _role: Option<String>,
    limit: u32,
    offset: u32,
    state: State<'_, AppState>,
) -> Result<MusicianAppearances, String> {
    log::info!(
        "Getting musician appearances: {} (limit: {}, offset: {})",
        name,
        limit,
        offset
    );

    // Search for tracks by performer name
    let tracks = {
        let client = state.client.lock().await;
        client
            .search_tracks(&name, 100, 0, None)
            .await
            .map_err(|e| e.to_string())?
    };

    // Deduplicate by album, keeping track of roles
    let mut seen_albums = HashSet::new();
    let mut albums: Vec<AlbumAppearance> = Vec::new();

    for track in tracks.items {
        // Check if performer matches
        let performer_name = track
            .performer
            .as_ref()
            .map(|p| p.name.as_str())
            .unwrap_or("");

        // Also check the performers string for session musicians
        let performers_str = track.performers.as_deref().unwrap_or("");
        let name_lower = name.to_lowercase();

        let matches = performer_name.to_lowercase() == name_lower
            || performers_str.to_lowercase().contains(&name_lower);

        if !matches {
            continue;
        }

        if let Some(album) = &track.album {
            if seen_albums.contains(&album.id) {
                continue;
            }
            seen_albums.insert(album.id.clone());

            // Extract role from performers string if possible
            let role = extract_role_from_performers(performers_str, &name);

            albums.push(AlbumAppearance {
                album_id: album.id.clone(),
                album_title: album.title.clone().unwrap_or_default(),
                album_artwork: album
                    .image
                    .as_ref()
                    .and_then(|i| i.large.clone())
                    .or_else(|| album.image.as_ref().and_then(|i| i.thumbnail.clone()))
                    .unwrap_or_default(),
                artist_name: track
                    .album
                    .as_ref()
                    .and_then(|a| a.artist.as_ref())
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| performer_name.to_string()),
                year: album
                    .release_date_original
                    .as_ref()
                    .and_then(|d| d.split('-').next())
                    .map(|s| s.to_string()),
                role_on_album: role,
            });
        }
    }

    // Sort by year descending
    albums.sort_by(|a, b| {
        let year_a = a.year.as_deref().unwrap_or("0");
        let year_b = b.year.as_deref().unwrap_or("0");
        year_b.cmp(year_a)
    });

    let total = albums.len();

    // Apply pagination
    let start = offset as usize;
    let end = (start + limit as usize).min(albums.len());
    let paginated = if start < albums.len() {
        albums[start..end].to_vec()
    } else {
        Vec::new()
    };

    Ok(MusicianAppearances {
        albums: paginated,
        total,
    })
}

// ============ Helper Functions ============

/// Count how many unique albums a musician appears on
async fn count_track_appearances(state: &State<'_, AppState>, name: &str) -> usize {
    let client = state.client.lock().await;

    match client.search_tracks(name, 50, 0, None).await {
        Ok(results) => {
            let mut seen_albums = HashSet::new();
            let name_lower = name.to_lowercase();

            for track in results.items {
                // Check performer name
                let performer_name = track
                    .performer
                    .as_ref()
                    .map(|p| p.name.to_lowercase())
                    .unwrap_or_default();

                // Check performers string
                let performers_str = track
                    .performers
                    .as_ref()
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();

                let matches =
                    performer_name == name_lower || performers_str.contains(&name_lower);

                if matches {
                    if let Some(album) = &track.album {
                        seen_albums.insert(album.id.clone());
                    }
                }
            }

            seen_albums.len()
        }
        Err(e) => {
            log::warn!("Failed to count appearances for {}: {}", name, e);
            0
        }
    }
}

/// Enrich musician data from MusicBrainz
async fn enrich_from_musicbrainz(
    mb_state: &State<'_, MusicBrainzSharedState>,
    name: &str,
) -> Result<(Option<String>, Vec<String>), String> {
    // Resolve artist to get MBID
    let resolved = {
        let cache = mb_state.cache.lock().await;
        cache.get_artist(name).ok().flatten()
    };

    let mbid = if let Some(cached) = resolved {
        cached.mbid
    } else {
        // Search MusicBrainz for the artist
        match mb_state.client.search_artist(name).await {
            Ok(response) => {
                // Find best match (score >= 90)
                response
                    .artists
                    .into_iter()
                    .find(|a| a.score.unwrap_or(0) >= 90)
                    .map(|a| a.id)
            }
            Err(e) => {
                log::warn!("MusicBrainz artist search failed: {}", e);
                None
            }
        }
    };

    let mut bands = Vec::new();

    // If we have an MBID, get relationships to find bands
    if let Some(ref mbid) = mbid {
        match mb_state.client.get_artist_with_relations(mbid).await {
            Ok(response) => {
                if let Some(relations) = response.relations {
                    for relation in relations {
                        // "member of band" relation with forward direction means this person is in a band
                        if relation.relation_type == "member of band"
                            && relation.direction.as_deref() != Some("backward")
                        {
                            if let Some(artist) = relation.artist {
                                bands.push(artist.name);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("MusicBrainz relations fetch failed: {}", e);
            }
        }
    }

    Ok((mbid, bands))
}

/// Extract role from performers string
/// e.g., "John Smith, Piano - Jane Doe, Vocals" -> "Piano" for "John Smith"
fn extract_role_from_performers(performers: &str, name: &str) -> String {
    let name_lower = name.to_lowercase();

    // Split by common delimiters
    for segment in performers.split(&['-', '–', '|', ';'][..]) {
        let segment = segment.trim();

        // Check if this segment contains the name
        if segment.to_lowercase().contains(&name_lower) {
            // Try to extract role after comma
            if let Some(comma_pos) = segment.find(',') {
                let after_comma = segment[comma_pos + 1..].trim();
                // Clean up - take first word or phrase before next punctuation
                let role = after_comma
                    .split(&[',', '(', ')'][..])
                    .next()
                    .unwrap_or(after_comma)
                    .trim();
                if !role.is_empty() {
                    return role.to_string();
                }
            }
        }
    }

    // Fallback
    "Performer".to_string()
}
