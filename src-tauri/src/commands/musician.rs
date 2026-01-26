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

    // Step 1: Search Qobuz for artist, handling "The" prefix variations
    // MusicBrainz often has "Beatles" but Qobuz has "The Beatles"
    let qobuz_artist = {
        let client = state.client.lock().await;
        let name_lower = name.to_lowercase();

        // Search with original name
        let mut best_match: Option<crate::api::Artist> = None;

        if let Ok(results) = client.search_artists(&name, 10, 0, None).await {
            // Look for exact match first
            for artist in &results.items {
                let artist_lower = artist.name.to_lowercase();
                if artist_lower == name_lower {
                    best_match = Some(artist.clone());
                    break;
                }
            }

            // If no exact match, check for "The X" variant
            if best_match.is_none() {
                let the_name = format!("the {}", name_lower);
                for artist in &results.items {
                    let artist_lower = artist.name.to_lowercase();
                    if artist_lower == the_name {
                        log::info!("Found 'The' variant: {} -> {}", name, artist.name);
                        best_match = Some(artist.clone());
                        break;
                    }
                }
            }

            // If we have a match but it has few albums, check if "The X" has more STUDIO albums
            if let Some(ref current) = best_match {
                let current_total = current.albums_count.unwrap_or(0);
                if current_total < 10 {
                    // Search specifically for "The X" to compare
                    let the_name = format!("The {}", name);
                    if let Ok(the_results) = client.search_artists(&the_name, 5, 0, None).await {
                        for artist in the_results.items {
                            let artist_lower = artist.name.to_lowercase();
                            let the_name_lower = the_name.to_lowercase();
                            if artist_lower == the_name_lower {
                                // Compare studio album counts for better accuracy
                                let current_studio = count_studio_albums(&client, current.id).await;
                                let the_studio = count_studio_albums(&client, artist.id).await;

                                log::info!(
                                    "Comparing studio albums: '{}' has {} studio albums, 'The {}' has {} studio albums",
                                    current.name, current_studio, name, the_studio
                                );

                                if the_studio > current_studio {
                                    log::info!(
                                        "Preferring 'The {}' ({} studio albums) over '{}' ({} studio albums)",
                                        name, the_studio, current.name, current_studio
                                    );
                                    best_match = Some(artist);
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        best_match
    };

    // Step 2: If found, check if they have a real catalog
    if let Some(artist) = qobuz_artist {
        let has_catalog = artist.albums_count.unwrap_or(0) > 0;

        if has_catalog {
            // Confidence = 3: Has a dedicated Qobuz artist page
            musician.qobuz_artist_id = Some(artist.id as i64);
            musician.confidence = MusicianConfidence::Confirmed;
            log::info!(
                "Musician {} resolved as Confirmed (Qobuz artist ID: {}, name: {})",
                name,
                artist.id,
                artist.name
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
                album_title: album.title.clone(),
                album_artwork: album
                    .image
                    .large
                    .clone()
                    .or_else(|| album.image.thumbnail.clone())
                    .unwrap_or_default(),
                // AlbumSummary doesn't have artist field, use track's performer
                artist_name: track
                    .performer
                    .as_ref()
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| performer_name.to_string()),
                // AlbumSummary doesn't have release_date_original
                year: None,
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

// ============ Album Categorization (mirrors qobuzAdapters.ts logic) ============

use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    // Unofficial releases patterns
    static ref BROADCAST_PATTERNS: Regex = Regex::new(r"(?i)\b(fm broadcasts?|radio broadcasts?|broadcasts?|bootleg|unofficial|pirate)\b").unwrap();
    static ref NON_MUSIC_PATTERNS: Regex = Regex::new(r"(?i)\b(interviews?|speaks|talking|in their own words|the interviews?|memories:?\s*the)\b").unwrap();
    static ref BOOTLEG_LABELS: Regex = Regex::new(r"(?i)\b(leftfield media|purple pyramid|cleopatra|laser media|radio lu|broadcast archives)\b").unwrap();

    // Compilation patterns
    static ref COMPILATION_PATTERNS: Regex = Regex::new(r"(?i)\b(greatest hits|best of|anthology|the very best|essentials?|definitive collection|gold|platinum|hits collection|complete collection|hit collection|b-sides|rarities)\b").unwrap();
    static ref VARIOUS_PATTERNS: Regex = Regex::new(r"(?i)\b(various artists|v\.?a\.?|original soundtrack|ost)\b").unwrap();

    // Live album patterns
    static ref LIVE_PATTERNS: Regex = Regex::new(r"(?i)\blive\b|\blive at\b|\blive in\b|\bin concert\b|\bunplugged\b|\bmtv unplugged\b|\bacoustic live\b|\balive\b|\bon stage\b").unwrap();
    static ref LIVE_MULTILANG: Regex = Regex::new(r"(?i)\b(en vivo|en directo|ao vivo|dal vivo|en direct|konzert|liveopname)\b|ライブ|라이브|콘서트").unwrap();
    static ref FM_BROADCAST: Regex = Regex::new(r"(?i)\b(fm broadcasts?|radio broadcasts?|broadcasts?)\b").unwrap();

    // Studio album variant patterns
    static ref STUDIO_VARIANT: Regex = Regex::new(r"(?i)\b(deluxe|remaster|anniversary|expanded|special edition|collector|box set)\b").unwrap();

    // EP/Single patterns
    static ref EP_PATTERNS: Regex = Regex::new(r"(?i)\b(- ep|\.ep|\(ep\))\b").unwrap();
    static ref EP_SUFFIX: Regex = Regex::new(r"(?i)\bep\s*$").unwrap();
    static ref SINGLE_PATTERNS: Regex = Regex::new(r"(?i)\b(- single|\(single\))\b").unwrap();
}

/// Album category for filtering
#[derive(Debug, Clone, Copy, PartialEq)]
enum AlbumCategory {
    Albums,    // Studio albums (main discography)
    Tributes,  // Albums by other artists
    Others,    // Compilations, bootlegs, unofficial
    Live,      // Live recordings
    Eps,       // EPs and singles
}

/// Check if album is by a different artist
fn is_different_artist(album: &crate::api::Album, main_artist_id: u64) -> bool {
    album.artist.id != main_artist_id
}

/// Check if this is an unofficial/bootleg release
fn is_unofficial_release(title: &str, label: &str, track_count: u32, duration: u32) -> bool {
    // FM Broadcasts, bootlegs
    if BROADCAST_PATTERNS.is_match(title) {
        return true;
    }

    // Interviews, spoken word
    if NON_MUSIC_PATTERNS.is_match(title) {
        return true;
    }

    // Known bootleg labels
    if BOOTLEG_LABELS.is_match(label) {
        return true;
    }

    // Podcast-like: single long track
    if track_count == 1 && duration >= 1200 {
        return true;
    }

    false
}

/// Check if this is a compilation/greatest hits
fn is_compilation_album(title: &str) -> bool {
    COMPILATION_PATTERNS.is_match(title) || VARIOUS_PATTERNS.is_match(title)
}

/// Check if this is a live album
fn is_live_album(title: &str) -> bool {
    // Don't classify FM broadcasts as live
    if FM_BROADCAST.is_match(title) {
        return false;
    }

    if LIVE_PATTERNS.is_match(title) || LIVE_MULTILANG.is_match(title) {
        return true;
    }

    false
}

/// Check if this is a studio album (main discography material)
fn is_studio_album(title: &str, track_count: u32, duration: u32) -> bool {
    // Single-track releases over 20 min are likely podcasts
    if track_count == 1 && duration >= 1200 {
        return false;
    }

    // Deluxe/remastered editions are studio albums
    if STUDIO_VARIANT.is_match(title) && (track_count >= 7 || duration >= 2100) {
        return true;
    }

    // Standard album: 7+ tracks or 25+ minutes
    if track_count >= 7 {
        return true;
    }
    if duration >= 1500 {
        return true;
    }

    false
}

/// Check if this is an EP or Single
fn is_ep_or_single(title: &str, track_count: u32, duration: u32) -> bool {
    // Explicitly marked as EP or Single
    if EP_PATTERNS.is_match(title) || EP_SUFFIX.is_match(title) || SINGLE_PATTERNS.is_match(title) {
        return true;
    }

    // Very short releases: 1-4 tracks
    if track_count > 0 && track_count <= 4 {
        return true;
    }

    // Under 15 minutes
    if duration > 0 && duration <= 900 {
        return true;
    }

    // 5-6 tracks AND under 20 minutes
    if track_count >= 5 && track_count <= 6 && duration > 0 && duration <= 1200 {
        return true;
    }

    false
}

/// Categorize an album (mirrors TypeScript categorizeAlbum)
fn categorize_album(album: &crate::api::Album, main_artist_id: u64) -> AlbumCategory {
    let title = album.title.to_lowercase();
    let label = album.label.as_ref().map(|l| l.name.as_str()).unwrap_or("");
    let track_count = album.tracks_count.unwrap_or(0);
    let duration = album.duration.unwrap_or(0);

    // 1. Albums by different artists -> Tributes
    if is_different_artist(album, main_artist_id) {
        return AlbumCategory::Tributes;
    }

    // 2. Unofficial releases -> Others
    if is_unofficial_release(&title, label, track_count, duration) {
        return AlbumCategory::Others;
    }

    // 3. Compilations -> Others
    if is_compilation_album(&title) {
        return AlbumCategory::Others;
    }

    // 4. Live albums -> Live
    if is_live_album(&title) {
        return AlbumCategory::Live;
    }

    // 5. Check if it's a studio album first
    if is_studio_album(&title, track_count, duration) {
        return AlbumCategory::Albums;
    }

    // 6. EPs and Singles
    if is_ep_or_single(&title, track_count, duration) {
        return AlbumCategory::Eps;
    }

    // 7. Default: treat as studio album
    AlbumCategory::Albums
}

/// Count studio albums for an artist (fetches albums and filters)
async fn count_studio_albums(
    client: &crate::api::QobuzClient,
    artist_id: u64,
) -> usize {
    // Fetch artist with albums (limit to 50 for performance)
    match client.get_artist_with_pagination(artist_id, true, Some(50), None).await {
        Ok(artist) => {
            if let Some(albums) = artist.albums {
                albums
                    .items
                    .iter()
                    .filter(|album| categorize_album(album, artist_id) == AlbumCategory::Albums)
                    .count()
            } else {
                0
            }
        }
        Err(e) => {
            log::warn!("Failed to fetch albums for artist {}: {}", artist_id, e);
            0
        }
    }
}
