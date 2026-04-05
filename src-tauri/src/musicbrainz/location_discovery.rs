//! Location-based artist discovery using MusicBrainz
//!
//! Implements the scene discovery pipeline:
//! 1. Extract artist metadata (location, genres) from MusicBrainz
//! 2. Browse candidates by area with genre affinity scoring
//! 3. Validate candidates against Qobuz catalog

use std::collections::HashSet;

use super::genre_normalization::{extract_affinity_seeds, normalize_genre};
use qbz_integrations::musicbrainz::*;

/// Extract artist metadata from the full MB response
pub fn extract_metadata(response: &ArtistFullResponse) -> ArtistMetadata {
    let artist_type = ArtistType::from(response.artist_type.as_deref());

    // Resolve location: prefer begin_area (city-level), fallback to area (country)
    let location = resolve_location(
        response.begin_area.as_ref(),
        response.area.as_ref(),
        response.country.as_deref(),
    );

    // Extract affinity seeds from tags
    let tags = response.tags.as_deref().unwrap_or(&[]);
    let affinity_seeds = extract_affinity_seeds(tags);

    ArtistMetadata {
        mbid: response.id.clone(),
        name: response.name.clone(),
        artist_type,
        life_span: response.life_span.clone(),
        location,
        affinity_seeds,
    }
}

/// Resolve the most precise location from MB area data
fn resolve_location(
    begin_area: Option<&Area>,
    area: Option<&Area>,
    country: Option<&str>,
) -> Option<ArtistLocation> {
    let cc = country.map(|c| c.to_lowercase());

    // Try begin_area first (formation/birth location — typically city-level)
    if let Some(ba) = begin_area {
        let is_city = ba
            .area_type
            .as_deref()
            .map(|t| t.eq_ignore_ascii_case("city") || t.eq_ignore_ascii_case("municipality"))
            .unwrap_or(false);

        let is_subdivision = ba
            .area_type
            .as_deref()
            .map(|t| t.eq_ignore_ascii_case("subdivision"))
            .unwrap_or(false);

        // MB's "country" field is where the artist is active (not where born).
        // When we have a city-level begin_area, display only the city name
        // to avoid incorrect country attribution (e.g., Zimmer: born Frankfurt,
        // but country=US because he works in the US).
        let precision = if is_city {
            LocationPrecision::City
        } else if is_subdivision {
            LocationPrecision::State
        } else {
            LocationPrecision::City // best guess
        };

        return Some(ArtistLocation {
            city: Some(ba.name.clone()),
            area_id: Some(ba.id.clone()),
            country: country.map(|c| country_code_to_name(c)),
            country_code: cc,
            display_name: ba.name.clone(),
            precision,
        });
    }

    // Fallback to area (usually country-level)
    if let Some(a) = area {
        let is_country = a
            .area_type
            .as_deref()
            .map(|t| t.eq_ignore_ascii_case("country"))
            .unwrap_or(false);

        if is_country {
            return Some(ArtistLocation {
                city: None,
                area_id: Some(a.id.clone()),
                country: Some(a.name.clone()),
                country_code: cc,
                display_name: a.name.clone(),
                precision: LocationPrecision::Country,
            });
        }

        // Non-country area (could be city without begin_area)
        let country_name = country.map(|c| country_code_to_name(c));
        let display = if let Some(ref cn) = country_name {
            format!("{}, {}", a.name, cn)
        } else {
            a.name.clone()
        };

        return Some(ArtistLocation {
            city: Some(a.name.clone()),
            area_id: Some(a.id.clone()),
            country: country_name,
            country_code: cc,
            display_name: display,
            precision: LocationPrecision::City,
        });
    }

    // Country code only (no area data)
    if let Some(raw_cc) = country {
        let name = country_code_to_name(raw_cc);
        return Some(ArtistLocation {
            city: None,
            area_id: None,
            country: Some(name.clone()),
            country_code: cc,
            display_name: name,
            precision: LocationPrecision::Country,
        });
    }

    None
}

/// Convert ISO 3166-1 alpha-2 country code to human-readable name
fn country_code_to_name(code: &str) -> String {
    match code.to_uppercase().as_str() {
        "US" => "United States",
        "GB" => "United Kingdom",
        "CA" => "Canada",
        "AU" => "Australia",
        "DE" => "Germany",
        "FR" => "France",
        "JP" => "Japan",
        "SE" => "Sweden",
        "NO" => "Norway",
        "FI" => "Finland",
        "IE" => "Ireland",
        "NZ" => "New Zealand",
        "BR" => "Brazil",
        "MX" => "Mexico",
        "AR" => "Argentina",
        "CL" => "Chile",
        "CO" => "Colombia",
        "ES" => "Spain",
        "IT" => "Italy",
        "NL" => "Netherlands",
        "BE" => "Belgium",
        "AT" => "Austria",
        "CH" => "Switzerland",
        "DK" => "Denmark",
        "IS" => "Iceland",
        "PT" => "Portugal",
        "PL" => "Poland",
        "CZ" => "Czech Republic",
        "RU" => "Russia",
        "KR" => "South Korea",
        "CN" => "China",
        "TW" => "Taiwan",
        "IN" => "India",
        "ZA" => "South Africa",
        "NG" => "Nigeria",
        "JM" => "Jamaica",
        "CU" => "Cuba",
        "PR" => "Puerto Rico",
        _ => code,
    }
    .to_string()
}

/// Affinity scoring weights
const SCORE_EXACT_CITY: i32 = 40;
const SCORE_SAME_COUNTRY: i32 = 15;
const SCORE_GENRE_CORE: i32 = 20;
const _SCORE_GENRE_SECONDARY: i32 = 10;
const SCORE_TAG_USEFUL: i32 = 8;
const SCORE_NOISY_ONLY: i32 = -12;

/// Compute affinity score for a candidate artist against the source seeds
pub fn compute_affinity_score(
    candidate_tags: &[String],
    source_seeds: &AffinitySeeds,
    same_city: bool,
    same_country: bool,
) -> i32 {
    let mut score: i32 = 0;

    if same_city {
        score += SCORE_EXACT_CITY;
    }
    if same_country {
        score += SCORE_SAME_COUNTRY;
    }

    // Normalize candidate tags for comparison
    let candidate_normalized: HashSet<String> = candidate_tags
        .iter()
        .map(|tag| normalize_genre(tag))
        .collect();

    // Core genre overlap
    let core_overlap = source_seeds
        .genres
        .iter()
        .filter(|g| candidate_normalized.contains(g.as_str()))
        .count();
    score += (core_overlap as i32) * SCORE_GENRE_CORE;

    // Secondary tag overlap
    let tag_overlap = source_seeds
        .tags
        .iter()
        .filter(|tag| candidate_normalized.contains(tag.as_str()))
        .count();
    score += (tag_overlap as i32) * SCORE_TAG_USEFUL;

    // Penalty: if candidate has tags but zero overlap with any seed
    if !candidate_normalized.is_empty() && core_overlap == 0 && tag_overlap == 0 {
        score += SCORE_NOISY_ONLY;
    }

    score
}

/// Build the scene cache key from location + seeds
pub fn build_scene_cache_key(area_id: &str, seeds: &AffinitySeeds) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    for seed in &seeds.normalized_seeds {
        seed.hash(&mut hasher);
    }
    let seed_hash = hasher.finish();

    format!("{}:{:x}", area_id, seed_hash)
}

/// Format a human-readable date from MB life_span
pub fn format_life_span_date(life_span: &LifeSpan, _is_person: bool) -> Option<String> {
    let begin = life_span.begin.as_deref()?;

    let begin_formatted = format_mb_date(begin);
    let ended = life_span.ended.unwrap_or(false);

    if ended {
        if let Some(end) = life_span.end.as_deref() {
            let end_formatted = format_mb_date(end);
            Some(format!("{}–{}", begin_formatted, end_formatted))
        } else {
            Some(begin_formatted)
        }
    } else {
        Some(begin_formatted)
    }
}

/// Format a MusicBrainz date string into a short human-readable form
/// Input formats: "1990", "1990-05", "1990-05-14"
fn format_mb_date(date: &str) -> String {
    let parts: Vec<&str> = date.split('-').collect();
    match parts.len() {
        1 => parts[0].to_string(),
        2 => {
            let month = match parts[1] {
                "01" => "Jan",
                "02" => "Feb",
                "03" => "Mar",
                "04" => "Apr",
                "05" => "May",
                "06" => "Jun",
                "07" => "Jul",
                "08" => "Aug",
                "09" => "Sep",
                "10" => "Oct",
                "11" => "Nov",
                "12" => "Dec",
                _ => parts[1],
            };
            format!("{} {}", month, parts[0])
        }
        3 => {
            let month = match parts[1] {
                "01" => "Jan",
                "02" => "Feb",
                "03" => "Mar",
                "04" => "Apr",
                "05" => "May",
                "06" => "Jun",
                "07" => "Jul",
                "08" => "Aug",
                "09" => "Sep",
                "10" => "Oct",
                "11" => "Nov",
                "12" => "Dec",
                _ => parts[1],
            };
            format!("{} {}", month, parts[0])
        }
        _ => date.to_string(),
    }
}
