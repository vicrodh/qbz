//! Immersive Suggestions panel controller (split-only, split-panel == 2).
//!
//! Ports Tauri's `SuggestionsPanel.svelte` 1:1, with all assembly logic in
//! Rust (ADR-006): live artist queries only (NEVER `reco_store` — that powers
//! the home page; see data-panels.md §6). Two data products:
//!
//!   * RECOMMENDED TRACKS — `artist.tracks_appears_on`, falling back to
//!     `get_artist_tracks(limit 30)` when sparse (<5), deduped by exact
//!     lowercase title, the current track filtered out, shuffled, take 10.
//!   * CARDS — the first 2 curated `artist.playlists` (each a book-collage of
//!     up to 3 distinct album covers, fetched via `get_playlist`) + ONE seed
//!     "Song Radio" card (diamond-collage of up to 4 rec-track covers).
//!
//! The shuffle is a deterministic splitmix64 seeded off the artist+track ids
//! (matches qbz-radio's RNG family; avoids pulling `rand` just for this).

use std::collections::HashSet;
use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AppWindow, SuggestionCard, SuggestionsState, TrackItem};

/// Recommended-track target count (Tauri `slice(0, 10)`).
const REC_LIMIT: usize = 10;
/// Sparse threshold below which the artist-tracks fallback runs (Tauri `< 5`).
const SPARSE_THRESHOLD: usize = 5;
/// Artist-tracks fallback page size (Tauri `limit: 30`).
const FALLBACK_LIMIT: u32 = 30;
/// Max curated playlist cards (Tauri `slice(0, 2)`).
const MAX_PLAYLIST_CARDS: usize = 2;
/// Book-collage cover count per playlist card (Tauri 3).
const BOOK_COVERS: usize = 3;
/// Diamond-collage cover count for the radio card (Tauri max 4).
const RADIO_COVERS: usize = 4;

/// A resolved playlist card (book collage of up to 3 distinct album covers).
pub(crate) struct PlaylistCard {
    id: String,
    name: String,
    track_count: u32,
    /// Up to 3 distinct album-cover URLs for the book collage.
    cover_urls: Vec<String>,
}

/// The fully-assembled suggestions for one (artist, track) pair.
pub struct SuggestionsPayload {
    pub artist_id: String,
    pub seed_track_id: String,
    pub seed_track_name: String,
    pub seed_artist_id: String,
    pub playlist_cards: Vec<PlaylistCard>,
    pub rec_tracks: Vec<qbz_models::Track>,
    /// Up to 4 distinct rec-track album covers for the radio diamond collage.
    pub radio_cover_urls: Vec<String>,
    pub error: bool,
}

/// An empty payload (no cards, no tracks, no error) — the "no track selected"
/// reset state applied when the immersive panel opens with no current track.
pub fn empty_payload() -> SuggestionsPayload {
    SuggestionsPayload {
        artist_id: String::new(),
        seed_track_id: String::new(),
        seed_track_name: String::new(),
        seed_artist_id: String::new(),
        playlist_cards: Vec::new(),
        rec_tracks: Vec::new(),
        radio_cover_urls: Vec::new(),
        error: false,
    }
}

/// Deterministic splitmix64 step (qbz-radio's RNG family). Used for an
/// in-place Fisher-Yates shuffle so the rec list is varied but reproducible
/// per (artist, track) — no `rand` dependency pulled just for this.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Fisher-Yates shuffle seeded off the (artist, track) ids.
fn shuffle_tracks(tracks: &mut [qbz_models::Track], seed: u64) {
    let mut state = seed ^ 0xD1B54A32D192ED03;
    for i in (1..tracks.len()).rev() {
        let j = (splitmix64(&mut state) % (i as u64 + 1)) as usize;
        tracks.swap(i, j);
    }
}

/// Best collage cover URL for a track's album (large → best variant).
fn track_album_cover(track: &qbz_models::Track) -> Option<String> {
    track
        .album
        .as_ref()
        .and_then(|a| a.image.best().cloned())
        .filter(|s| !s.is_empty())
}

/// Album id of a track (for distinct-cover dedupe in the book collage).
fn track_album_id(track: &qbz_models::Track) -> Option<String> {
    track
        .album
        .as_ref()
        .map(|a| a.id.clone())
        .filter(|s| !s.is_empty())
}

/// Build the suggestions payload for `artist_id` + `current_track_id`. All
/// queries are live Qobuz artist calls (NEVER reco_store). On the top-level
/// artist-detail failure, returns an error payload (drives the panel's error
/// branch); individual playlist-cover fetch failures are tolerated.
pub async fn load_suggestions<A>(
    runtime: &Arc<AppRuntime<A>>,
    artist_id: u64,
    current_track_id: u64,
    seed_track_name: String,
) -> SuggestionsPayload
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let artist = match runtime.core().get_artist_detail(artist_id, None, None).await {
        Ok(a) => a,
        Err(e) => {
            log::error!("[qbz-slint] suggestions get_artist_detail({artist_id}) failed: {e}");
            return SuggestionsPayload {
                artist_id: artist_id.to_string(),
                seed_track_id: current_track_id.to_string(),
                seed_track_name,
                seed_artist_id: artist_id.to_string(),
                playlist_cards: Vec::new(),
                rec_tracks: Vec::new(),
                radio_cover_urls: Vec::new(),
                error: true,
            };
        }
    };

    // ---- Recommended tracks --------------------------------------------
    // Base = tracks_appears_on (current track filtered, deduped by title).
    let mut rec: Vec<qbz_models::Track> = Vec::new();
    let mut seen_titles: HashSet<String> = HashSet::new();
    if let Some(container) = artist.tracks_appears_on.as_ref() {
        for track in &container.items {
            if track.id == current_track_id {
                continue;
            }
            let key = track.title.to_lowercase().trim().to_string();
            if key.is_empty() || !seen_titles.insert(key) {
                continue;
            }
            rec.push(track.clone());
        }
    }

    // Sparse fallback: merge artist popular tracks (dedupe by title + id).
    if rec.len() < SPARSE_THRESHOLD {
        match runtime
            .core()
            .get_artist_tracks(artist_id, FALLBACK_LIMIT, 0)
            .await
        {
            Ok(container) => {
                let existing_ids: HashSet<u64> = rec.iter().map(|t| t.id).collect();
                for track in container.items {
                    if track.id == current_track_id || existing_ids.contains(&track.id) {
                        continue;
                    }
                    let key = track.title.to_lowercase().trim().to_string();
                    if key.is_empty() || !seen_titles.insert(key) {
                        continue;
                    }
                    rec.push(track);
                }
            }
            Err(e) => log::warn!("[qbz-slint] suggestions artist-tracks fallback failed: {e}"),
        }
    }

    // Shuffle (deterministic per artist+track), take 10.
    let seed = (artist_id << 1) ^ current_track_id.wrapping_add(1);
    shuffle_tracks(&mut rec, seed);
    rec.truncate(REC_LIMIT);

    // Radio diamond collage: up to 4 distinct rec-track album covers.
    let mut radio_cover_urls: Vec<String> = Vec::new();
    for track in &rec {
        if let Some(url) = track_album_cover(track) {
            if !radio_cover_urls.contains(&url) {
                radio_cover_urls.push(url);
                if radio_cover_urls.len() >= RADIO_COVERS {
                    break;
                }
            }
        }
    }

    // ---- Curated playlist cards (first 2) ------------------------------
    let mut playlist_cards: Vec<PlaylistCard> = Vec::new();
    if let Some(playlists) = artist.playlists.as_ref() {
        for playlist in playlists.iter().take(MAX_PLAYLIST_CARDS) {
            // Fetch the full playlist to harvest up to 3 distinct album covers.
            let mut cover_urls: Vec<String> = Vec::new();
            match runtime.core().get_playlist(playlist.id).await {
                Ok(full) => {
                    if let Some(container) = full.tracks.as_ref() {
                        let mut seen_albums: HashSet<String> = HashSet::new();
                        for track in &container.items {
                            let (Some(url), Some(album_id)) =
                                (track_album_cover(track), track_album_id(track))
                            else {
                                continue;
                            };
                            if seen_albums.insert(album_id) {
                                cover_urls.push(url);
                                if cover_urls.len() >= BOOK_COVERS {
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!(
                        "[qbz-slint] suggestions get_playlist({}) failed: {e}",
                        playlist.id
                    );
                }
            }
            // Fallback to the playlist's own images when no track covers found.
            if cover_urls.is_empty() {
                if let Some(images) = playlist.images.as_ref() {
                    if let Some(img) = images.iter().find(|s| !s.is_empty()) {
                        cover_urls.push(img.clone());
                    }
                }
            }
            playlist_cards.push(PlaylistCard {
                id: playlist.id.to_string(),
                name: playlist.name.clone(),
                track_count: playlist.tracks_count,
                cover_urls,
            });
        }
    }

    SuggestionsPayload {
        artist_id: artist_id.to_string(),
        seed_track_id: current_track_id.to_string(),
        seed_track_name,
        seed_artist_id: artist_id.to_string(),
        playlist_cards,
        rec_tracks: rec,
        radio_cover_urls,
        error: false,
    }
}

/// Build a `SuggestionCard` for a playlist (book collage).
fn playlist_to_card(card: &PlaylistCard) -> SuggestionCard {
    SuggestionCard {
        kind: "playlist".into(),
        title: card.name.clone().into(),
        subtitle: format!("{} tracks", card.track_count).into(),
        cover_urls: ModelRc::new(VecModel::from(
            card.cover_urls
                .iter()
                .map(|s| slint::SharedString::from(s.as_str()))
                .collect::<Vec<_>>(),
        )),
        cover0: slint::Image::default(),
        cover1: slint::Image::default(),
        cover2: slint::Image::default(),
        cover3: slint::Image::default(),
        playlist_id: card.id.clone().into(),
        seed_track_id: "".into(),
        seed_track_name: "".into(),
        seed_artist_id: "".into(),
        badge: "qobuz".into(),
        loading: false,
    }
}

/// Build the seed "Song Radio" card (diamond collage).
fn radio_card(payload: &SuggestionsPayload) -> SuggestionCard {
    SuggestionCard {
        kind: "radio".into(),
        title: "Song Radio".into(),
        subtitle: payload.seed_track_name.clone().into(),
        cover_urls: ModelRc::new(VecModel::from(
            payload
                .radio_cover_urls
                .iter()
                .map(|s| slint::SharedString::from(s.as_str()))
                .collect::<Vec<_>>(),
        )),
        cover0: slint::Image::default(),
        cover1: slint::Image::default(),
        cover2: slint::Image::default(),
        cover3: slint::Image::default(),
        playlist_id: "".into(),
        seed_track_id: payload.seed_track_id.clone().into(),
        seed_track_name: payload.seed_track_name.clone().into(),
        seed_artist_id: payload.seed_artist_id.clone().into(),
        badge: "qbz".into(),
        loading: false,
    }
}

/// Apply the assembled suggestions to `SuggestionsState`. Runs on the event loop.
pub fn apply_suggestions(window: &AppWindow, payload: SuggestionsPayload) {
    let mut cards: Vec<SuggestionCard> =
        payload.playlist_cards.iter().map(playlist_to_card).collect();
    // The radio card always trails the playlist cards (Tauri order).
    if !payload.seed_track_id.is_empty() {
        cards.push(radio_card(&payload));
    }
    let tracks: Vec<TrackItem> = payload
        .rec_tracks
        .iter()
        .map(crate::playlist::to_item)
        .collect();

    let state = window.global::<SuggestionsState>();
    state.set_artist_id(payload.artist_id.into());
    state.set_seed_track_id(payload.seed_track_id.into());
    state.set_cards(ModelRc::new(VecModel::from(cards)));
    state.set_tracks(ModelRc::new(VecModel::from(tracks)));
    state.set_error(if payload.error { "error".into() } else { "".into() });
    state.set_loading(false);
}

/// Flip the radio card's `loading` flag (the building spinner) on/off. The
/// radio card is the LAST card in the model (Tauri order: playlists then
/// radio). Scheduled on the event loop; safe to call from any thread.
pub fn set_radio_loading(weak: &slint::Weak<AppWindow>, loading: bool) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let model = w.global::<SuggestionsState>().get_cards();
        let n = model.row_count();
        if n == 0 {
            return;
        }
        // Find the radio card (kind == "radio"); fall back to the last card.
        let idx = (0..n)
            .find(|&i| {
                model
                    .row_data(i)
                    .map(|c| c.kind.as_str() == "radio")
                    .unwrap_or(false)
            })
            .unwrap_or(n - 1);
        if let Some(mut card) = model.row_data(idx) {
            card.loading = loading;
            model.set_row_data(idx, card);
        }
    });
}

/// Clear the suggestions state before a (re)load. Runs on the event loop.
pub fn reset_suggestions(window: &AppWindow) {
    let state = window.global::<SuggestionsState>();
    state.set_cards(ModelRc::new(VecModel::from(Vec::<SuggestionCard>::new())));
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_error("".into());
    state.set_loading(true);
}

/// Artwork jobs for the assembled suggestions: per-card collage slots +
/// rec-track row thumbnails.
pub fn suggestions_artwork_jobs(payload: &SuggestionsPayload) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    // Card collage slots: playlist cards first (their order matches the model),
    // then the radio card (appended last in apply_suggestions).
    for (card_idx, card) in payload.playlist_cards.iter().enumerate() {
        for (slot, url) in card.cover_urls.iter().enumerate() {
            if !url.is_empty() {
                jobs.push(ArtworkJob {
                    url: url.clone(),
                    target: ArtworkTarget::SuggestionCardCover { card_idx, slot },
                });
            }
        }
    }
    if !payload.seed_track_id.is_empty() {
        let radio_idx = payload.playlist_cards.len();
        for (slot, url) in payload.radio_cover_urls.iter().enumerate() {
            if !url.is_empty() {
                jobs.push(ArtworkJob {
                    url: url.clone(),
                    target: ArtworkTarget::SuggestionCardCover {
                        card_idx: radio_idx,
                        slot,
                    },
                });
            }
        }
    }
    // Rec-track row thumbnails.
    for (idx, track) in payload.rec_tracks.iter().enumerate() {
        if let Some(url) = track
            .album
            .as_ref()
            .and_then(|a| a.image.smallest().cloned())
            .filter(|s| !s.is_empty())
        {
            jobs.push(ArtworkJob {
                url,
                target: ArtworkTarget::SuggestionTrackCover { idx },
            });
        }
    }
    jobs
}
