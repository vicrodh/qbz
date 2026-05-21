//! Qobuz mix detail views (DailyQ / WeeklyQ / FavQ / TopQ).
//!
//! Opened from the For You Qobuz Mixes tiles. Each mix resolves to a
//! track list (built from the data the Slint MVP can source) that the
//! MixView renders and plays:
//!   - DailyQ / WeeklyQ — `/dynamic/suggest` seeded from the local
//!     play-history track ids.
//!   - FavQ — the user's favorite tracks, shuffled.
//!   - TopQ — tracks aggregated from the user's playlists.
//!
//! (Tauri's exact mix-generation — listened-track analysis payloads,
//! playlist play-stats ranking — is approximated; the same surfaces
//! and playback result, sourced from available backend.)

use std::sync::{LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::Track;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AppWindow, MixState, SearchTrackItem};

/// The currently-loaded mix track list, kept so play-all / per-track
/// play can build the queue without re-fetching.
static CURRENT_MIX: LazyLock<Mutex<Vec<Track>>> = LazyLock::new(|| Mutex::new(Vec::new()));

pub fn mix_meta(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "daily" => ("DailyQ", "Elevate your day with a customized selection of music."),
        "weekly" => ("WeeklyQ", "A fresh mix every week."),
        "fav" => ("FavQ", "A fresh shuffle from your personal library."),
        "top" => ("TopQ", "From your most-played playlists."),
        _ => ("Mix", ""),
    }
}

pub async fn load_mix<A>(runtime: &AppRuntime<A>, kind: &str) -> Vec<Track>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    match kind {
        "daily" | "weekly" => {
            let seeds: Vec<u64> = crate::recently::load()
                .into_iter()
                .filter_map(|t| t.id.parse::<u64>().ok())
                .take(50)
                .collect();
            runtime
                .core()
                .get_dynamic_suggest(&seeds, 50)
                .await
                .unwrap_or_default()
        }
        "fav" => {
            let mut tracks = favorite_tracks(runtime).await;
            shuffle(&mut tracks);
            tracks
        }
        "top" => playlist_tracks(runtime).await,
        _ => Vec::new(),
    }
}

async fn favorite_tracks<A>(runtime: &AppRuntime<A>) -> Vec<Track>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    match runtime.core().get_favorites("tracks", 200, 0).await {
        Ok(value) => {
            let items = value
                .get("tracks")
                .and_then(|b| b.get("items"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            serde_json::from_value(items).unwrap_or_default()
        }
        Err(_) => Vec::new(),
    }
}

async fn playlist_tracks<A>(runtime: &AppRuntime<A>) -> Vec<Track>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let Ok(playlists) = runtime.core().get_user_playlists().await else {
        return Vec::new();
    };
    let mut out: Vec<Track> = Vec::new();
    for pl in playlists.into_iter().take(5) {
        if out.len() >= 100 {
            break;
        }
        if let Ok(full) = runtime.core().get_playlist(pl.id).await {
            if let Some(container) = full.tracks {
                out.extend(container.items);
            }
        }
    }
    out.truncate(100);
    out
}

/// Lightweight, deterministic-per-call shuffle (no rng dep).
fn shuffle(tracks: &mut [Track]) {
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        | 1;
    let n = tracks.len();
    for i in (1..n).rev() {
        // xorshift
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % (i as u64 + 1)) as usize;
        tracks.swap(i, j);
    }
}

fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn to_item(track: &Track) -> SearchTrackItem {
    let mut title = track.title.clone();
    if let Some(v) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({v})");
    }
    SearchTrackItem {
        id: track.id.to_string().into(),
        title: title.into(),
        artist: track
            .performer
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default()
            .into(),
        duration: mmss(track.duration).into(),
        quality_tier: match track.maximum_bit_depth {
            Some(d) if d >= 24 => "hires",
            Some(_) => "cd",
            None => "",
        }
        .into(),
        explicit: track.parental_warning,
        artwork_url: track
            .album
            .as_ref()
            .and_then(|a| a.image.best().cloned())
            .unwrap_or_default()
            .into(),
        artwork: slint::Image::default(),
    }
}

pub fn apply_mix(window: &AppWindow, kind: &str, tracks: Vec<Track>) {
    let (title, subtitle) = mix_meta(kind);
    let items: Vec<SearchTrackItem> = tracks.iter().map(to_item).collect();
    if let Ok(mut cur) = CURRENT_MIX.lock() {
        *cur = tracks;
    }
    let state = window.global::<MixState>();
    state.set_kind(kind.into());
    state.set_title(title.into());
    state.set_subtitle(subtitle.into());
    state.set_tracks(ModelRc::new(VecModel::from(items)));
    state.set_loading(false);
}

pub fn reset_mix(window: &AppWindow, kind: &str) {
    let (title, subtitle) = mix_meta(kind);
    let state = window.global::<MixState>();
    state.set_kind(kind.into());
    state.set_title(title.into());
    state.set_subtitle(subtitle.into());
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<SearchTrackItem>::new())));
    state.set_loading(true);
}

/// The cached mix track list (for play-all / per-track play).
pub fn current_tracks() -> Vec<Track> {
    CURRENT_MIX.lock().map(|c| c.clone()).unwrap_or_default()
}

/// Index of `track_id` within the current mix (for play-from-here).
pub fn index_of(track_id: &str) -> usize {
    CURRENT_MIX
        .lock()
        .ok()
        .and_then(|c| c.iter().position(|t| t.id.to_string() == track_id))
        .unwrap_or(0)
}

pub fn artwork_jobs(tracks: &[Track]) -> Vec<ArtworkJob> {
    tracks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.album
                .as_ref()
                .and_then(|a| a.image.best().cloned())
                .filter(|u| !u.is_empty())
                .map(|url| ArtworkJob {
                    url,
                    target: ArtworkTarget::MixTrack { index: i },
                })
        })
        .collect()
}
