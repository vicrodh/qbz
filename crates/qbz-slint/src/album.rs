//! Album detail controller.
//!
//! Fetches a full album through `QbzCore`, maps it to plain (Send) data
//! on the worker thread, and applies it to the `AlbumState` global on the
//! Slint event loop.

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{Album, Track};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AlbumState, AlbumTrackItem, AppWindow};

/// Plain, `Send` album data produced on the worker thread.
pub struct AlbumData {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    /// Pre-formatted "year • label • genre • N tracks • duration".
    pub info_line: String,
    pub quality_tier: String,
    pub artwork_url: String,
    /// Record label name, for the sidebar (empty when unknown).
    pub label: String,
    /// Editorial award names, for the sidebar.
    pub awards: Vec<String>,
    pub tracks: Vec<TrackData>,
}

pub struct TrackData {
    pub id: String,
    pub number: String,
    pub title: String,
    pub artist: String,
    pub duration: String,
    pub quality_tier: String,
    pub explicit: bool,
}

/// Fetch and map a full album by id.
pub async fn load_album<A>(runtime: &Arc<AppRuntime<A>>, album_id: &str) -> Result<AlbumData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let album = runtime
        .core()
        .get_album(album_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(map_album(album))
}

fn map_album(album: Album) -> AlbumData {
    let artist = album.artist.name.clone();
    let artist_id = album.artist.id.to_string();

    let year = album
        .release_date_original
        .as_deref()
        .and_then(|date| date.get(0..4))
        .unwrap_or("")
        .to_string();

    let mut parts: Vec<String> = Vec::new();
    if !year.is_empty() {
        parts.push(year);
    }
    if let Some(label) = album.label.as_ref().filter(|l| !l.name.is_empty()) {
        parts.push(label.name.clone());
    }
    if let Some(genre) = album.genre.as_ref().filter(|g| !g.name.is_empty()) {
        parts.push(genre.name.clone());
    }
    if let Some(count) = album.tracks_count {
        parts.push(format!("{count} tracks"));
    }
    if let Some(duration) = album.duration {
        parts.push(format_duration(duration));
    }
    let info_line = parts.join("   •   ");

    let quality_tier = tier(album.maximum_bit_depth).to_string();
    let artwork_url = album.image.best().cloned().unwrap_or_default();
    let label = album
        .label
        .as_ref()
        .map(|l| l.name.clone())
        .unwrap_or_default();
    let awards = album
        .awards
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|a| a.name.clone())
        .filter(|n| !n.is_empty())
        .collect();
    let tracks = album
        .tracks
        .map(|container| container.items)
        .unwrap_or_default()
        .into_iter()
        .map(map_track)
        .collect();

    AlbumData {
        id: album.id,
        title: album.title,
        artist,
        artist_id,
        info_line,
        quality_tier,
        artwork_url,
        label,
        awards,
        tracks,
    }
}

fn map_track(track: Track) -> TrackData {
    let mut title = track.title;
    if let Some(version) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({version})");
    }
    TrackData {
        id: track.id.to_string(),
        number: track.track_number.to_string(),
        title,
        artist: track.performer.map(|p| p.name).unwrap_or_default(),
        duration: mmss(track.duration),
        quality_tier: tier(track.maximum_bit_depth).to_string(),
        explicit: track.parental_warning,
    }
}

/// 24-bit and up is Hi-Res, anything else with depth info is CD-quality.
fn tier(bit_depth: Option<u32>) -> &'static str {
    match bit_depth {
        Some(depth) if depth >= 24 => "hires",
        Some(_) => "cd",
        None => "",
    }
}

/// `m:ss` track duration.
fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// `Xh Ym` / `Ym` album duration.
fn format_duration(secs: u32) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// Apply album data to the `AlbumState` global. Runs on the Slint event loop.
pub fn apply_album(window: &AppWindow, data: AlbumData) {
    let tracks: Vec<AlbumTrackItem> = data
        .tracks
        .into_iter()
        .map(|track| AlbumTrackItem {
            id: track.id.into(),
            number: track.number.into(),
            title: track.title.into(),
            artist: track.artist.into(),
            duration: track.duration.into(),
            quality_tier: track.quality_tier.into(),
            explicit: track.explicit,
        })
        .collect();

    let awards: Vec<slint::SharedString> =
        data.awards.into_iter().map(Into::into).collect();

    let state = window.global::<AlbumState>();
    state.set_id(data.id.into());
    state.set_title(data.title.into());
    state.set_artist(data.artist.into());
    state.set_artist_id(data.artist_id.into());
    state.set_info_line(data.info_line.into());
    state.set_quality_tier(data.quality_tier.into());
    state.set_label(data.label.into());
    state.set_awards(ModelRc::new(VecModel::from(awards)));
    state.set_tracks(ModelRc::new(VecModel::from(tracks)));
}

/// Clear album state and show an empty track list (used when opening a new
/// album so the previous one does not flash).
pub fn reset_album(window: &AppWindow) {
    let state = window.global::<AlbumState>();
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<AlbumTrackItem>::new())));
    state.set_artwork(slint::Image::default());
    state.set_loading(true);
}

/// Apply decoded header artwork pixels. Runs on the Slint event loop.
pub fn apply_artwork(window: &AppWindow, pixels: &[u8], width: u32, height: u32) {
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let dst = buffer.make_mut_bytes();
    if dst.len() != pixels.len() {
        return;
    }
    dst.copy_from_slice(pixels);
    let (r, g, b) = crate::artwork::header_tint(pixels);
    let state = window.global::<AlbumState>();
    state.set_artwork(slint::Image::from_rgba8(buffer));
    state.set_header_color(slint::Color::from_rgb_u8(r, g, b));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmss_pads_seconds() {
        assert_eq!(mmss(5), "0:05");
        assert_eq!(mmss(65), "1:05");
        assert_eq!(mmss(225), "3:45");
    }

    #[test]
    fn duration_drops_zero_hours() {
        assert_eq!(format_duration(2700), "45m");
        assert_eq!(format_duration(3720), "1h 2m");
    }

    #[test]
    fn tier_classifies_bit_depth() {
        assert_eq!(tier(Some(24)), "hires");
        assert_eq!(tier(Some(16)), "cd");
        assert_eq!(tier(None), "");
    }
}
