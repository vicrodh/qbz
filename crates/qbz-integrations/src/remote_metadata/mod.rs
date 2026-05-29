//! Remote-metadata converters (MusicBrainz + Discogs -> unified DTOs).
//!
//! Frontend-agnostic copy of the pure converters that live in
//! `src-tauri/src/library/remote_metadata/`, so the Slint frontend can do
//! remote album lookup via `qbz_integrations` without depending on the Tauri
//! binary. The Tauri side keeps its own copy + its cache/state orchestration;
//! only these pure adapters are shared here.

mod models;
pub use models::{
    RemoteAlbumMetadata, RemoteAlbumSearchResult, RemoteMetadataError, RemoteProvider,
    RemoteSearchRequest, RemoteSearchResponse, RemoteTrackMetadata,
};

pub fn musicbrainz_release_to_search_result(
    release: &crate::musicbrainz::ReleaseResult,
) -> RemoteAlbumSearchResult {
    // Extract artist from artist-credit
    let artist = release
        .artist_credit
        .as_ref()
        .map(|credits| {
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
        })
        .unwrap_or_default();

    // Extract year from date (YYYY or YYYY-MM-DD)
    let year = release
        .date
        .as_ref()
        .and_then(|d| d.split('-').next().and_then(|y| y.parse::<u16>().ok()));

    // Extract label and catalog number
    let (label, catalog_number) = release
        .label_info
        .as_ref()
        .and_then(|info| info.first())
        .map(|li| {
            (
                li.label.as_ref().map(|l| l.name.clone()),
                li.catalog_number.clone(),
            )
        })
        .unwrap_or((None, None));

    // Get track count - either from direct field or sum from media
    let track_count = release.track_count.or_else(|| {
        release
            .media
            .as_ref()
            .map(|media| media.iter().filter_map(|m| m.track_count).sum())
    });

    // Get format from first medium
    let format = release
        .media
        .as_ref()
        .and_then(|m| m.first())
        .and_then(|m| m.format.clone());

    RemoteAlbumSearchResult {
        provider: RemoteProvider::MusicBrainz,
        provider_id: release.id.clone(),
        title: release.title.clone(),
        artist,
        year,
        track_count,
        country: release.country.clone(),
        label,
        catalog_number,
        confidence: release.score.map(|s| s.min(100) as u8),
        format,
    }
}

// ============ Discogs Adapter ============

/// Parse Discogs track position to (disc_number, track_number)
/// Handles formats: "1", "A1", "1-1", "CD1-1", "1.1"
pub fn parse_discogs_position(position: &str) -> (u8, u8) {
    let position = position.trim();

    // Handle empty position
    if position.is_empty() {
        return (1, 1);
    }

    // Try "X-Y" format (e.g., "1-5", "CD1-3")
    if let Some(pos) = position.find('-') {
        let disc_part = &position[..pos];
        let track_part = &position[pos + 1..];

        // Extract number from disc part (handle "CD1", "1", etc.)
        let disc = disc_part
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u8>()
            .unwrap_or(1);

        let track = track_part.parse::<u8>().unwrap_or(1);
        return (disc, track);
    }

    // Try "X.Y" format
    if let Some(pos) = position.find('.') {
        let disc_part = &position[..pos];
        let track_part = &position[pos + 1..];

        let disc = disc_part.parse::<u8>().unwrap_or(1);
        let track = track_part.parse::<u8>().unwrap_or(1);
        return (disc, track);
    }

    // Handle vinyl sides (A, B, C, D -> disc 1, 1, 2, 2)
    if position.starts_with(|c: char| c.is_ascii_alphabetic()) {
        let side = position.chars().next().unwrap().to_ascii_uppercase();
        let track_str: String = position.chars().skip(1).collect();
        let track = track_str.parse::<u8>().unwrap_or(1);

        let disc = match side {
            'A' | 'B' => 1,
            'C' | 'D' => 2,
            'E' | 'F' => 3,
            _ => 1,
        };

        return (disc, track);
    }

    // Simple number
    let track = position.parse::<u8>().unwrap_or(1);
    (1, track)
}

/// Parse Discogs duration string to milliseconds
/// Handles format: "M:SS" or "MM:SS" or "H:MM:SS"
pub fn parse_discogs_duration(duration: &str) -> Option<u32> {
    let parts: Vec<&str> = duration.split(':').collect();

    match parts.len() {
        2 => {
            // M:SS or MM:SS
            let minutes: u32 = parts[0].parse().ok()?;
            let seconds: u32 = parts[1].parse().ok()?;
            Some((minutes * 60 + seconds) * 1000)
        }
        3 => {
            // H:MM:SS
            let hours: u32 = parts[0].parse().ok()?;
            let minutes: u32 = parts[1].parse().ok()?;
            let seconds: u32 = parts[2].parse().ok()?;
            Some((hours * 3600 + minutes * 60 + seconds) * 1000)
        }
        _ => None,
    }
}

/// Convert Discogs extended search result to unified DTO
pub fn discogs_extended_to_search_result(
    result: &crate::discogs::DiscogsSearchResultExtended,
) -> RemoteAlbumSearchResult {
    // Discogs title format is usually "Artist - Album"
    let (artist, title) = if let Some(pos) = result.title.find(" - ") {
        let (a, t) = result.title.split_at(pos);
        (a.to_string(), t.trim_start_matches(" - ").to_string())
    } else {
        ("Unknown Artist".to_string(), result.title.clone())
    };

    // Parse year from string
    let year = result.year.as_ref().and_then(|y| y.parse::<u16>().ok());

    // Get first label
    let label = result.label.as_ref().and_then(|l| l.first().cloned());

    // Get format as string
    let format = result.format.as_ref().map(|f| f.join(", "));

    RemoteAlbumSearchResult {
        provider: RemoteProvider::Discogs,
        provider_id: result.id.to_string(),
        title,
        artist,
        year,
        track_count: None,
        country: result.country.clone(),
        label,
        catalog_number: result.catno.clone(),
        confidence: None,
        format,
    }
}

/// Convert MusicBrainz full release to unified metadata DTO
pub fn musicbrainz_full_to_metadata(
    release: &crate::musicbrainz::ReleaseFullResponse,
) -> RemoteAlbumMetadata {
    // Extract artist from artist-credit
    let artist = release
        .artist_credit
        .as_ref()
        .map(|credits| {
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
        })
        .unwrap_or_default();

    // Extract year from date
    let year = release
        .date
        .as_ref()
        .and_then(|d| d.split('-').next().and_then(|y| y.parse::<u16>().ok()));

    // Extract genres from tags (sorted by count, take top 5)
    let genres: Vec<String> = release
        .tags
        .as_ref()
        .map(|tags| {
            let mut sorted: Vec<_> = tags.iter().collect();
            sorted.sort_by(|a, b| b.count.cmp(&a.count));
            sorted.iter().take(5).map(|t| t.name.clone()).collect()
        })
        .unwrap_or_default();

    // Extract label and catalog number
    let (label, catalog_number) = release
        .label_info
        .as_ref()
        .and_then(|info| info.first())
        .map(|li| {
            (
                li.label.as_ref().map(|l| l.name.clone()),
                li.catalog_number.clone(),
            )
        })
        .unwrap_or((None, None));

    // Count discs
    let disc_count = release.media.as_ref().map(|m| m.len() as u8).unwrap_or(1);

    // Convert tracks
    let tracks: Vec<RemoteTrackMetadata> = release
        .media
        .as_ref()
        .map(|media| {
            let mut all_tracks = Vec::new();
            for medium in media {
                if let Some(tracks) = &medium.tracks {
                    for track in tracks {
                        all_tracks.push(RemoteTrackMetadata {
                            disc_number: medium.position.unwrap_or(1),
                            track_number: track.position.unwrap_or(1),
                            title: track
                                .title
                                .clone()
                                .or_else(|| track.recording.as_ref().and_then(|r| r.title.clone()))
                                .unwrap_or_default(),
                            duration_ms: track.length.map(|l| l as u32).or_else(|| {
                                track
                                    .recording
                                    .as_ref()
                                    .and_then(|r| r.length.map(|l| l as u32))
                            }),
                        });
                    }
                }
            }
            all_tracks
        })
        .unwrap_or_default();

    RemoteAlbumMetadata {
        provider: RemoteProvider::MusicBrainz,
        provider_id: release.id.clone(),
        title: release.title.clone(),
        artist,
        year,
        genres,
        label,
        catalog_number,
        country: release.country.clone(),
        barcode: release.barcode.clone(),
        tracks,
        disc_count,
        source_url: Some(format!("https://musicbrainz.org/release/{}", release.id)),
    }
}

/// Convert Discogs full release to unified metadata DTO
pub fn discogs_full_to_metadata(
    release: &crate::discogs::DiscogsReleaseMetadata,
) -> RemoteAlbumMetadata {
    // Combine artists with join phrases
    let artist = release
        .artists
        .as_ref()
        .map(|artists| {
            artists
                .iter()
                .map(|a| format!("{}{}", a.name.clone(), a.join.as_deref().unwrap_or("")))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    // Combine genres and styles
    let genres: Vec<String> = {
        let mut combined = Vec::new();
        if let Some(g) = &release.genres {
            combined.extend(g.clone());
        }
        if let Some(s) = &release.styles {
            combined.extend(s.clone());
        }
        combined
    };

    // Get first label and catalog number
    let (label, catalog_number) = release
        .labels
        .as_ref()
        .and_then(|labels| labels.first())
        .map(|l| (Some(l.name.clone()), l.catno.clone()))
        .unwrap_or((None, None));

    // Convert tracklist
    let tracks: Vec<RemoteTrackMetadata> = release
        .tracklist
        .as_ref()
        .map(|tracklist| {
            tracklist
                .iter()
                .filter(|t| {
                    // Filter out headings (disc separators)
                    t.track_type.as_deref() != Some("heading")
                })
                .map(|t| {
                    let (disc_number, track_number) = parse_discogs_position(&t.position);
                    RemoteTrackMetadata {
                        disc_number,
                        track_number,
                        title: t.title.clone(),
                        duration_ms: t.duration.as_ref().and_then(|d| parse_discogs_duration(d)),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Count unique discs
    let disc_count = tracks.iter().map(|t| t.disc_number).max().unwrap_or(1);

    RemoteAlbumMetadata {
        provider: RemoteProvider::Discogs,
        provider_id: release.id.to_string(),
        title: release.title.clone(),
        artist,
        year: release.year.map(|y| y as u16),
        genres,
        label,
        catalog_number,
        country: release.country.clone(),
        barcode: None, // Discogs doesn't include barcode in release details
        tracks,
        disc_count,
        source_url: release.uri.clone(),
    }
}
