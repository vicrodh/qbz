//! Playlist detail view controller.
//!
//! Fetches a playlist through `QbzCore`, maps it to the shared
//! TrackItem rows + header metadata, and applies it to `PlaylistState`.
//! Mirrors `mix.rs`: a cached track list backs play-all / per-track
//! play, and an artwork-jobs pass resolves the row covers + header
//! cover off-thread.

use std::sync::{LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::Track;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AppWindow, PlaylistState, TrackItem};

/// The currently-loaded playlist tracks, for play-all / per-track play.
static CURRENT: LazyLock<Mutex<Vec<Track>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Plain, `Send` playlist data produced on the worker thread.
pub struct PlaylistData {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub description: String,
    pub cover_url: String,
    pub tracks: Vec<Track>,
}

pub async fn load<A>(runtime: &AppRuntime<A>, playlist_id: u64) -> Option<PlaylistData>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let pl = match runtime.core().get_playlist(playlist_id).await {
        Ok(pl) => pl,
        Err(e) => {
            log::error!("[qbz-slint] load playlist {playlist_id} failed: {e}");
            return None;
        }
    };
    let tracks = pl.tracks.map(|c| c.items).unwrap_or_default();
    // Header cover: the server-composed playlist image, else the first
    // track's album cover.
    let cover_url = pl
        .images
        .as_ref()
        .and_then(|imgs| imgs.first().cloned())
        .or_else(|| {
            tracks
                .first()
                .and_then(|t| t.album.as_ref())
                .and_then(|a| a.image.best().cloned())
        })
        .unwrap_or_default();
    Some(PlaylistData {
        id: pl.id.to_string(),
        name: pl.name,
        owner: pl.owner.name,
        description: pl
            .description
            .map(|d| crate::strip_html::strip_html(&d))
            .unwrap_or_default(),
        cover_url,
        tracks,
    })
}

fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn total_duration(tracks: &[Track]) -> String {
    let secs: u64 = tracks.iter().map(|t| t.duration as u64).sum();
    let mins = secs / 60;
    if mins >= 60 {
        format!("{} h {} min", mins / 60, mins % 60)
    } else {
        format!("{} min", mins)
    }
}

fn to_item(track: &Track) -> TrackItem {
    let mut title = track.title.clone();
    if let Some(v) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({v})");
    }
    TrackItem {
        id: track.id.to_string().into(),
        number: "".into(),
        title: title.into(),
        artist: track
            .performer
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default()
            .into(),
        album: track
            .album
            .as_ref()
            .map(|a| a.title.clone())
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
        selected: false,
        artwork_url: track
            .album
            .as_ref()
            .and_then(|a| a.image.best().cloned())
            .unwrap_or_default()
            .into(),
        artwork: slint::Image::default(),
    }
}

pub fn reset(window: &AppWindow) {
    let state = window.global::<PlaylistState>();
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_track_count(0);
    state.set_total_duration("".into());
    state.set_cover(slint::Image::default());
    state.set_loading(true);
}

pub fn apply(window: &AppWindow, data: PlaylistData) {
    let items: Vec<TrackItem> = data.tracks.iter().map(to_item).collect();
    let count = data.tracks.len() as i32;
    let duration = total_duration(&data.tracks);
    if let Ok(mut cur) = CURRENT.lock() {
        *cur = data.tracks;
    }
    let state = window.global::<PlaylistState>();
    state.set_id(data.id.into());
    state.set_name(data.name.into());
    state.set_owner(data.owner.into());
    state.set_description(data.description.into());
    state.set_cover_url(data.cover_url.into());
    state.set_tracks(ModelRc::new(VecModel::from(items)));
    state.set_track_count(count);
    state.set_total_duration(duration.into());
    state.set_loading(false);
}

/// Artwork jobs for the loaded playlist — one per row plus the header
/// cover (resolved into PlaylistState.cover).
pub fn artwork_jobs(data: &PlaylistData) -> Vec<ArtworkJob> {
    let mut jobs: Vec<ArtworkJob> = data
        .tracks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.album
                .as_ref()
                .and_then(|a| a.image.best().cloned())
                .map(|url| ArtworkJob {
                    url,
                    target: ArtworkTarget::PlaylistTrack { index: i },
                })
        })
        .collect();
    if !data.cover_url.is_empty() {
        jobs.push(ArtworkJob {
            url: data.cover_url.clone(),
            target: ArtworkTarget::PlaylistCover,
        });
    }
    jobs
}

pub fn current_tracks() -> Vec<Track> {
    CURRENT.lock().map(|c| c.clone()).unwrap_or_default()
}

/// The current playlist tracks in a fresh random order (Shuffle).
pub fn shuffled_tracks() -> Vec<Track> {
    let mut tracks = current_tracks();
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        | 1;
    for i in (1..tracks.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % (i as u64 + 1)) as usize;
        tracks.swap(i, j);
    }
    tracks
}

pub fn index_of(track_id: &str) -> usize {
    CURRENT
        .lock()
        .ok()
        .and_then(|c| c.iter().position(|t| t.id.to_string() == track_id))
        .unwrap_or(0)
}

// ==================== Multi-select edit mode ====================

use slint::Model;

/// Recount selected rows into PlaylistState.selected-count.
pub fn recount_selected(window: &AppWindow) {
    let model = window.global::<PlaylistState>().get_tracks();
    let count = (0..model.row_count())
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count() as i32;
    window.global::<PlaylistState>().set_selected_count(count);
}

/// Enter/leave edit mode. Leaving clears any selection.
pub fn set_multi_select(window: &AppWindow, on: bool) {
    let state = window.global::<PlaylistState>();
    if !on {
        let model = state.get_tracks();
        for i in 0..model.row_count() {
            if let Some(mut item) = model.row_data(i) {
                if item.selected {
                    item.selected = false;
                    model.set_row_data(i, item);
                }
            }
        }
        state.set_selected_count(0);
    }
    state.set_multi_select_mode(on);
}

/// Toggle select-all: select every row, or clear if all are selected.
pub fn select_all(window: &AppWindow) {
    let model = window.global::<PlaylistState>().get_tracks();
    let total = model.row_count();
    let selected = (0..total)
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    let target = selected != total; // if not all selected -> select all
    for i in 0..total {
        if let Some(mut item) = model.row_data(i) {
            if item.selected != target {
                item.selected = target;
                model.set_row_data(i, item);
            }
        }
    }
    recount_selected(window);
}

/// The ids of the currently-selected rows.
pub fn selected_ids(window: &AppWindow) -> Vec<u64> {
    let model = window.global::<PlaylistState>().get_tracks();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|t| t.selected)
        .filter_map(|t| t.id.parse::<u64>().ok())
        .collect()
}
