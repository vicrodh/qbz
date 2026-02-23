//! Match imported tracks to Qobuz catalog

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use futures_util::stream::{self, StreamExt};
use tauri::{AppHandle, Emitter};

use crate::api::models::Track;
use crate::api::QobuzClient;
use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::models::{ImportProgress, ImportTrack, TrackMatch};

const SEARCH_LIMIT: u32 = 20;
const TITLE_WEIGHT: f32 = 0.6;
const ARTIST_WEIGHT: f32 = 0.3;
const ALBUM_WEIGHT: f32 = 0.1;
const MIN_SCORE: f32 = 0.65;
const CONCURRENCY: usize = 8;

pub async fn match_tracks(
    client: &QobuzClient,
    tracks: &[ImportTrack],
    app: &AppHandle,
) -> Result<Vec<TrackMatch>, PlaylistImportError> {
    let total = tracks.len() as u32;
    let matched_counter = Arc::new(AtomicU32::new(0));
    let completed_counter = Arc::new(AtomicU32::new(0));

    // Pre-allocate results vector with None slots
    let results: Arc<tokio::sync::Mutex<Vec<Option<TrackMatch>>>> =
        Arc::new(tokio::sync::Mutex::new(vec![None; tracks.len()]));

    let owned_tracks: Vec<(usize, ImportTrack)> = tracks
        .iter()
        .enumerate()
        .map(|(i, tr)| (i, tr.clone()))
        .collect();

    stream::iter(owned_tracks)
        .map(|(idx, track)| {
            let client = client.clone();
            let app = app.clone();
            let matched_counter = Arc::clone(&matched_counter);
            let completed_counter = Arc::clone(&completed_counter);
            let results = Arc::clone(&results);

            async move {
                let query = format!("{} {}", track.artist, track.title);
                let search_result = client.search_tracks(&query, SEARCH_LIMIT, 0, None).await;

                let match_entry = match search_result {
                    Ok(search) => {
                        let (best, score) = select_best_match(&track, &search.items);
                        match best {
                            Some(candidate) if score >= MIN_SCORE => {
                                matched_counter.fetch_add(1, Ordering::Relaxed);
                                TrackMatch {
                                    source: track.clone(),
                                    qobuz_track_id: Some(candidate.id),
                                    qobuz_title: Some(candidate.title.clone()),
                                    qobuz_artist: candidate
                                        .performer
                                        .as_ref()
                                        .map(|a| a.name.clone()),
                                    score,
                                }
                            }
                            _ => TrackMatch {
                                source: track.clone(),
                                qobuz_track_id: None,
                                qobuz_title: None,
                                qobuz_artist: None,
                                score,
                            },
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Search failed for '{}' - '{}': {}",
                            track.artist,
                            track.title,
                            e
                        );
                        TrackMatch {
                            source: track.clone(),
                            qobuz_track_id: None,
                            qobuz_title: None,
                            qobuz_artist: None,
                            score: 0.0,
                        }
                    }
                };

                // Store result at correct index
                {
                    let mut res = results.lock().await;
                    res[idx] = Some(match_entry);
                }

                let current = completed_counter.fetch_add(1, Ordering::Relaxed) + 1;
                let matched = matched_counter.load(Ordering::Relaxed);

                let current_track = Some(format!("{} - {}", track.artist, track.title));

                let _ = app.emit(
                    "import:progress",
                    ImportProgress {
                        phase: "matching".to_string(),
                        current,
                        total,
                        matched_so_far: matched,
                        current_track,
                    },
                );
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect::<Vec<()>>()
        .await;

    // Extract results in order
    let locked = results.lock().await;
    let ordered: Vec<TrackMatch> = locked
        .iter()
        .map(|slot| slot.clone().expect("All slots should be filled"))
        .collect();

    Ok(ordered)
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
    let artist_score = similarity(
        &track.artist,
        candidate
            .performer
            .as_ref()
            .map(|a| a.name.as_str())
            .unwrap_or(""),
    );
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

    let mut score =
        title_score * TITLE_WEIGHT + artist_score * ARTIST_WEIGHT + album_score * ALBUM_WEIGHT;

    if let (Some(import_duration), Some(candidate_duration)) = (
        track.duration_ms,
        Some((candidate.duration as u64).saturating_mul(1000)),
    ) {
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
