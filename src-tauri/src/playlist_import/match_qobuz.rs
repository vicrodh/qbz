//! Match imported tracks to Qobuz catalog

use crate::api::models::Track;
use crate::api::QobuzClient;
use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::models::{ImportTrack, TrackMatch};

const SEARCH_LIMIT: u32 = 20;
const TITLE_WEIGHT: f32 = 0.6;
const ARTIST_WEIGHT: f32 = 0.3;
const ALBUM_WEIGHT: f32 = 0.1;
const MIN_SCORE: f32 = 0.65;

pub async fn match_tracks(
    client: &QobuzClient,
    tracks: &[ImportTrack],
) -> Result<Vec<TrackMatch>, PlaylistImportError> {
    let mut results = Vec::new();

    for track in tracks {
        let query = format!("{} {}", track.artist, track.title);
        let search = client
            .search_tracks(&query, SEARCH_LIMIT, 0, None)
            .await
            .map_err(|e| PlaylistImportError::Qobuz(e.to_string()))?;

        let (best, score) = select_best_match(track, &search.items);

        let match_entry = match best {
            Some(candidate) if score >= MIN_SCORE => TrackMatch {
                source: track.clone(),
                qobuz_track_id: Some(candidate.id),
                qobuz_title: Some(candidate.title.clone()),
                qobuz_artist: candidate.performer.as_ref().map(|a| a.name.clone()),
                score,
            },
            _ => TrackMatch {
                source: track.clone(),
                qobuz_track_id: None,
                qobuz_title: None,
                qobuz_artist: None,
                score,
            },
        };

        results.push(match_entry);
    }

    Ok(results)
}

fn select_best_match<'a>(track: &ImportTrack, candidates: &'a [Track]) -> (Option<&'a Track>, f32) {
    let mut best: Option<&Track> = None;
    let mut best_score = 0.0f32;
    let mut best_quality = 0.0f32;

    for candidate in candidates {
        if !candidate.streamable {
            continue;
        }

        let score = score_candidate(track, candidate);
        let quality = quality_score(candidate);

        if score > best_score + 0.0001 {
            best = Some(candidate);
            best_score = score;
            best_quality = quality;
        } else if (score - best_score).abs() < 0.01 && quality > best_quality {
            best = Some(candidate);
            best_quality = quality;
        }
    }

    (best, best_score)
}

fn score_candidate(track: &ImportTrack, candidate: &Track) -> f32 {
    if let (Some(isrc), Some(candidate_isrc)) = (&track.isrc, &candidate.isrc) {
        if isrc.eq_ignore_ascii_case(candidate_isrc) {
            return 1.0;
        }
    }

    let title_score = similarity(&track.title, &candidate.title);
    let artist_score = similarity(&track.artist, candidate.performer.as_ref().map(|a| a.name.as_str()).unwrap_or(""));
    let album_score = track
        .album
        .as_ref()
        .map(|album| {
            candidate
                .album
                .as_ref()
                .map(|a| similarity(album, &a.title))
                .unwrap_or(0.0)
        })
        .unwrap_or(0.0);

    let mut score = title_score * TITLE_WEIGHT + artist_score * ARTIST_WEIGHT + album_score * ALBUM_WEIGHT;

    if let (Some(import_duration), Some(candidate_duration)) = (track.duration_ms, Some((candidate.duration as u64).saturating_mul(1000))) {
        let diff = if import_duration > candidate_duration {
            import_duration - candidate_duration
        } else {
            candidate_duration - import_duration
        };

        if diff <= 3000 {
            score += 0.05;
        } else if diff <= 5000 {
            score += 0.02;
        }
    }

    score
}

fn similarity(a: &str, b: &str) -> f32 {
    let na = normalize(a);
    let nb = normalize(b);

    if na.is_empty() || nb.is_empty() {
        return 0.0;
    }

    if na == nb {
        return 1.0;
    }

    if na.contains(&nb) || nb.contains(&na) {
        return 0.85;
    }

    token_overlap(&na, &nb)
}

fn normalize(input: &str) -> String {
    let stripped = remove_bracketed(input);
    let mut cleaned = String::new();

    for ch in stripped.chars() {
        if ch.is_ascii_alphanumeric() || ch.is_whitespace() {
            cleaned.push(ch.to_ascii_lowercase());
        } else {
            cleaned.push(' ');
        }
    }

    let stop_words = [
        "remaster",
        "remastered",
        "deluxe",
        "edition",
        "live",
        "feat",
        "featuring",
        "version",
        "mix",
        "mono",
        "stereo",
        "edit",
    ];

    cleaned
        .split_whitespace()
        .filter(|token| !stop_words.contains(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn remove_bracketed(input: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;

    for ch in input.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
            }
            ')' | ']' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ => {
                if depth == 0 {
                    out.push(ch);
                }
            }
        }
    }

    out
}

fn token_overlap(a: &str, b: &str) -> f32 {
    let a_tokens: Vec<&str> = a.split_whitespace().collect();
    let b_tokens: Vec<&str> = b.split_whitespace().collect();

    if a_tokens.is_empty() || b_tokens.is_empty() {
        return 0.0;
    }

    let mut matches = 0u32;
    for token in &a_tokens {
        if b_tokens.contains(token) {
            matches += 1;
        }
    }

    matches as f32 / a_tokens.len().max(b_tokens.len()) as f32
}

fn quality_score(track: &Track) -> f32 {
    let bit_depth = track.maximum_bit_depth.unwrap_or(0) as f32;
    let sample_rate = track.maximum_sampling_rate.unwrap_or(0.0) as f32;
    bit_depth * 100000.0 + sample_rate
}
