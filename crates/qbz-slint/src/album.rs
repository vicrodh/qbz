//! Album detail controller.
//!
//! Fetches a full album through `QbzCore`, maps it to plain (Send) data
//! on the worker thread, and applies it to the `AlbumState` global on the
//! Slint event loop.

use std::cell::RefCell;
use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{Album, Track};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AlbumState, TrackItem, AppWindow};

thread_local! {
    /// The current album's full, unfiltered track list — kept so the
    /// track search can filter against it without a re-fetch. UI thread
    /// only, hence `thread_local`.
    static FULL_TRACKS: RefCell<Vec<TrackItem>> = RefCell::new(Vec::new());
}

/// Plain, `Send` album data produced on the worker thread.
pub struct AlbumData {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    /// Pre-formatted "year • label • genre • N tracks • duration".
    pub info_line: String,
    pub quality_tier: String,
    /// "24-bit / 96 kHz" — the quality-badge detail line.
    pub quality_detail: String,
    /// Editorial description / review (HTML stripped). May be empty.
    pub description: String,
    /// Short, truncated description for the header (full text in a modal).
    pub description_short: String,
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
    let quality_detail = quality_detail(album.maximum_bit_depth, album.maximum_sampling_rate);
    let description = album
        .description
        .as_deref()
        .map(crate::strip_html::strip_html)
        .unwrap_or_default();
    // The header description fills the full width to the right of the
    // artwork, so a longer truncation keeps it from looking like a thin
    // strip; the Read more modal still holds the complete text.
    let description_short = truncate_words(&description, 360);
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
        quality_detail,
        description,
        description_short,
        artwork_url,
        label,
        awards,
        tracks,
    }
}

/// Truncate text to at most `max` characters, cutting back to the last
/// word boundary and appending an ellipsis. Returns the text unchanged
/// when it is already short enough.
fn truncate_words(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max).collect();
    let cut = truncated.rfind(' ').unwrap_or(truncated.len());
    format!("{}…", truncated[..cut].trim_end())
}

/// "24-bit / 96 kHz" — the quality-badge detail string.
fn quality_detail(bit_depth: Option<u32>, sample_rate: Option<f64>) -> String {
    let hi_res = matches!(bit_depth, Some(depth) if depth >= 24);
    let depth = bit_depth.unwrap_or(if hi_res { 24 } else { 16 });
    let rate = sample_rate.unwrap_or(if hi_res { 96.0 } else { 44.1 });
    let rate = if rate.fract().abs() < f64::EPSILON {
        format!("{}", rate as i64)
    } else {
        format!("{rate}")
    };
    format!("{depth}-bit / {rate} kHz")
}

/// Crude HTML strip for Qobuz album descriptions. Break and paragraph
// The previous local strip_html lived here; moved to
// `crate::strip_html` so artist and album views share the same
// paragraph + entity handling and pick up the same future
// improvements.

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
    let tracks: Vec<TrackItem> = data
        .tracks
        .into_iter()
        .map(|track| TrackItem {
            id: track.id.into(),
            number: track.number.into(),
            title: track.title.into(),
            artist: track.artist.into(),
            album: "".into(),
            duration: track.duration.into(),
            quality_tier: track.quality_tier.into(),
            explicit: track.explicit,
            selected: false,
            artwork_url: "".into(),
            artwork: slint::Image::default(),
        })
        .collect();

    let awards: Vec<slint::SharedString> =
        data.awards.into_iter().map(Into::into).collect();

    let has_custom_cover = crate::custom_artwork::album_cover(&data.id).is_some();
    let artwork_url = data.artwork_url.clone();

    let state = window.global::<AlbumState>();
    state.set_id(data.id.into());
    state.set_title(data.title.into());
    state.set_artwork_url(artwork_url.into());
    state.set_has_custom_cover(has_custom_cover);
    state.set_artist(data.artist.into());
    state.set_artist_id(data.artist_id.into());
    state.set_info_line(data.info_line.into());
    state.set_quality_tier(data.quality_tier.into());
    state.set_quality_detail(data.quality_detail.into());
    state.set_description(data.description.into());
    state.set_description_short(data.description_short.into());
    state.set_label(data.label.into());
    state.set_awards(ModelRc::new(VecModel::from(awards)));

    // Keep the unfiltered list for the track search, then show it all.
    FULL_TRACKS.with(|cell| *cell.borrow_mut() = tracks.clone());
    state.set_tracks(ModelRc::new(VecModel::from(tracks)));
}

/// Filter the visible track list by `query` (case-insensitive match on
/// title or artist), against the unfiltered list kept in `FULL_TRACKS`.
/// Runs on the Slint event loop.
pub fn filter_tracks(window: &AppWindow, query: &str) {
    let needle = query.trim().to_lowercase();
    let filtered: Vec<TrackItem> = FULL_TRACKS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|track| {
                needle.is_empty()
                    || track.title.as_str().to_lowercase().contains(&needle)
                    || track.artist.as_str().to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    });
    window
        .global::<AlbumState>()
        .set_tracks(ModelRc::new(VecModel::from(filtered)));
}

/// Clear album state and show an empty track list (used when opening a new
/// album so the previous one does not flash).
pub fn reset_album(window: &AppWindow) {
    FULL_TRACKS.with(|cell| cell.borrow_mut().clear());
    let state = window.global::<AlbumState>();
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
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
