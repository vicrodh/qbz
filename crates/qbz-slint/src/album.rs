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
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::{AlbumState, TrackItem, AppWindow};

thread_local! {
    /// The current album's full, unfiltered track list — kept so the
    /// track search can filter against it without a re-fetch. UI thread
    /// only, hence `thread_local`.
    static FULL_TRACKS: RefCell<Vec<TrackItem>> = RefCell::new(Vec::new());
    /// The current album's RAW catalog tracks (qbz_models::Track), kept so
    /// the multi-select bulk actions (enqueue / cache) can resolve the
    /// selected rows to full Track objects without a re-fetch — mirrors the
    /// `play` Vec the favorites tab keeps. UI thread only.
    static PLAY_TRACKS: RefCell<Vec<Track>> = RefCell::new(Vec::new());
}

/// Plain, `Send` album data produced on the worker thread.
pub struct AlbumData {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    /// Pre-formatted "year • label • genre • N tracks • duration".
    pub info_line: String,
    /// Meta-line segment BEFORE the label (the year) — rendered with the
    /// label as a clickable link in the header.
    pub meta_pre: String,
    /// Meta-line segment AFTER the label (genre • N tracks • duration).
    pub meta_post: String,
    pub quality_tier: String,
    /// "24-bit / 96 kHz" — the quality-badge detail line.
    pub quality_detail: String,
    /// Editorial description / review (HTML stripped). May be empty.
    pub description: String,
    /// Short, truncated description for the header (full text in a modal).
    pub description_short: String,
    /// Half-length truncation used when the content area is space-constrained.
    pub description_shorter: String,
    pub artwork_url: String,
    /// Record label name, for the sidebar (empty when unknown).
    pub label: String,
    /// Record label id, so the sidebar label card can navigate to the label
    /// page ("" when unknown).
    pub label_id: String,
    /// Editorial award names, for the sidebar.
    pub awards: Vec<String>,
    /// True when the album bundles a downloadable booklet/liner-notes PDF
    /// (Qobuz goodies) — gates the header booklet button.
    pub has_booklet: bool,
    pub tracks: Vec<TrackData>,
    /// Raw catalog tracks, kept for the multi-select bulk actions.
    pub raw_tracks: Vec<Track>,
}

pub struct TrackData {
    pub id: String,
    pub number: String,
    pub title: String,
    pub artist: String,
    /// Performer id for the clickable artist link ("" = plain text).
    pub artist_id: String,
    /// Album id for the clickable album link ("" = plain text). Album view
    /// leaves this empty (its rows belong to the album being viewed, so the
    /// apply layer stamps the viewed album's id); artist top-tracks set it
    /// per-track since they span different albums.
    pub album_id: String,
    pub duration: String,
    pub quality_tier: String,
    pub quality_detail: String,
    pub explicit: bool,
    /// Disc/media number (Qobuz `media_number`, defaulting to 1 when absent).
    /// Used after mapping to decide where the "Disc N" headers fall when the
    /// album spans more than one disc.
    pub disc: u32,
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

    // Meta line, split around the label so the header can render the label
    // as a clickable link (Tauri's `date • [label] • genre • N tracks •
    // duration`). `meta_pre` = the segment before the label (the year);
    // `meta_post` = everything after (genre / tracks / duration). The full
    // `info_line` (label inlined as plain text) stays the fallback for when
    // there is no label id to navigate to.
    let label_name = album
        .label
        .as_ref()
        .filter(|l| !l.name.is_empty())
        .map(|l| l.name.clone());
    let genre_str = album
        .genre
        .as_ref()
        .filter(|g| !g.name.is_empty())
        .map(|g| g.name.clone());
    let tracks_str = album.tracks_count.map(|count| format!("{count} tracks"));
    let duration_str = album.duration.map(format_duration);

    let mut pre_parts: Vec<String> = Vec::new();
    if !year.is_empty() {
        pre_parts.push(year.clone());
    }
    let mut post_parts: Vec<String> = Vec::new();
    if let Some(g) = &genre_str {
        post_parts.push(g.clone());
    }
    if let Some(tc) = &tracks_str {
        post_parts.push(tc.clone());
    }
    if let Some(d) = &duration_str {
        post_parts.push(d.clone());
    }

    let meta_pre = pre_parts.join("   •   ");
    let meta_post = post_parts.join("   •   ");

    let mut all_parts = pre_parts.clone();
    if let Some(l) = &label_name {
        all_parts.push(l.clone());
    }
    all_parts.extend(post_parts.clone());
    let info_line = all_parts.join("   •   ");

    let quality_tier = tier(album.maximum_bit_depth).to_string();
    let quality_detail = crate::quality::detail(album.maximum_bit_depth, album.maximum_sampling_rate);
    let description = album
        .description
        .as_deref()
        .map(crate::strip_html::strip_html)
        .unwrap_or_default();
    // The header description fills the full width to the right of the
    // artwork, so a longer truncation keeps it from looking like a thin
    // strip; the Read more modal still holds the complete text.
    let description_short = truncate_words(&description, 360);
    // Half the cutoff for the space-constrained layout (tracks get priority).
    let description_shorter = truncate_words(&description, 180);
    let artwork_url = album.image.best().cloned().unwrap_or_default();
    let label = album
        .label
        .as_ref()
        .map(|l| l.name.clone())
        .unwrap_or_default();
    let label_id = album
        .label
        .as_ref()
        .map(|l| l.id.to_string())
        .unwrap_or_default();
    let awards = album
        .awards
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|a| a.name.clone())
        .filter(|n| !n.is_empty())
        .collect();
    // Booklet present when the album carries any goody with a usable URL.
    let has_booklet = album
        .goodies
        .as_deref()
        .map(|goodies| goodies.iter().any(|g| !g.url.is_empty()))
        .unwrap_or(false);
    let raw_tracks: Vec<Track> = album
        .tracks
        .map(|container| container.items)
        .unwrap_or_default();
    let tracks = raw_tracks.iter().cloned().map(map_track).collect();

    AlbumData {
        id: album.id,
        title: album.title,
        artist,
        artist_id,
        info_line,
        meta_pre,
        meta_post,
        quality_tier,
        quality_detail,
        description,
        description_short,
        description_shorter,
        artwork_url,
        label,
        label_id,
        awards,
        has_booklet,
        tracks,
        raw_tracks,
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
    let (artist, artist_id) = track
        .performer
        .map(|p| (p.name, p.id.to_string()))
        .unwrap_or_default();
    TrackData {
        id: track.id.to_string(),
        number: track.track_number.to_string(),
        title,
        artist,
        artist_id,
        // The album view stamps the viewed album's id at the apply layer.
        album_id: String::new(),
        duration: mmss(track.duration),
        quality_tier: tier(track.maximum_bit_depth).to_string(),
        quality_detail: crate::quality::detail(
            track.maximum_bit_depth,
            track.maximum_sampling_rate,
        ),
        explicit: track.parental_warning,
        // Tauri: `disc = track.media_number ?? 1`.
        disc: track.media_number.unwrap_or(1),
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
    // These rows belong to the album currently being viewed, so the
    // album link target is this album's own id (the album column is not
    // shown here, but album-id keeps the row model complete).
    let album_id: slint::SharedString = data.id.clone().into();
    // Multi-disc grouping (Tauri PurchaseAlbumDetailView: groupByDisc +
    // isMultiDisc). The album is "multi-disc" when its tracks span more than
    // one distinct disc number; only then do we emit "Disc N" headers. The
    // header is stamped on the first track of each disc run, and tracks stay
    // in their delivered order (Qobuz returns them disc-then-track ordered).
    let is_multi_disc = {
        let mut seen: Option<u32> = None;
        let mut multi = false;
        for track in &data.tracks {
            match seen {
                Some(d) if d != track.disc => {
                    multi = true;
                    break;
                }
                _ => seen = Some(track.disc),
            }
        }
        multi
    };
    let mut prev_disc: Option<u32> = None;
    let tracks: Vec<TrackItem> = data
        .tracks
        .into_iter()
        .map(|track| {
            // Stamp the disc number on the first row of each disc run, but
            // only for multi-disc albums (single-disc renders flat → 0). The
            // delegate renders `@tr("Disc") <n>` above the row when this is
            // > 0, matching the Tauri `{$t('album.disc')} {discNum}` markup.
            let disc_header_number = if is_multi_disc && prev_disc != Some(track.disc) {
                track.disc as i32
            } else {
                0
            };
            prev_disc = Some(track.disc);
            TrackItem {
            id: track.id.clone().into(),
            number: track.number.into(),
            title: track.title.into(),
            artist: track.artist.into(),
            album: "".into(),
            duration: track.duration.into(),
            quality_tier: track.quality_tier.into(),
            quality_detail: track.quality_detail.into(),
            explicit: track.explicit,
            selected: false,
            artwork_url: "".into(),
            artwork: slint::Image::default(),
            is_favorite: crate::fav_cache::is_favorite(&track.id),
            artist_id: track.artist_id.into(),
            album_id: album_id.clone(),
            removing: false,
            cache_status: if crate::offline_cache::is_cached(&track.id) { 3 } else { 0 },
            cache_progress: 0.0,
            // Qobuz album-detail rows; local albums override via map_local_track.
            source: "qobuz".into(),
            unlocking: false,
            disc_header_number,
            }
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
    state.set_meta_pre(data.meta_pre.into());
    state.set_meta_post(data.meta_post.into());
    state.set_quality_tier(data.quality_tier.into());
    state.set_quality_detail(data.quality_detail.into());
    state.set_description(data.description.into());
    state.set_description_short(data.description_short.into());
    state.set_description_shorter(data.description_shorter.into());
    state.set_label(data.label.into());
    state.set_label_id(data.label_id.into());
    state.set_awards(ModelRc::new(VecModel::from(awards)));
    state.set_has_booklet(data.has_booklet);
    // Fully cached = every track already has a ready (3) offline copy. Kept
    // live afterwards by set_row_cache_status as downloads complete.
    let album_fully_cached =
        !tracks.is_empty() && tracks.iter().all(|t| t.cache_status == 3);
    state.set_album_fully_cached(album_fully_cached);
    // Seed the header heart from the favorite-album cache (kept in sync with
    // the server at login + on every toggle).
    state.set_is_favorite(crate::fav_cache::is_album_favorite(album_id.as_str()));
    state.set_favorite_loading(false);

    // Keep the unfiltered list for the track search + the raw tracks for the
    // multi-select bulk actions, then show them all.
    FULL_TRACKS.with(|cell| *cell.borrow_mut() = tracks.clone());
    PLAY_TRACKS.with(|cell| *cell.borrow_mut() = data.raw_tracks);
    // A freshly loaded album starts out of select mode with nothing selected.
    state.set_multi_select(false);
    state.set_selected_count(0);
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
    PLAY_TRACKS.with(|cell| cell.borrow_mut().clear());
    let state = window.global::<AlbumState>();
    state.set_multi_select(false);
    state.set_selected_count(0);
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_artwork(slint::Image::default());
    // Clear the booklet gate so the previous album's value doesn't linger.
    state.set_has_booklet(false);
    state.set_album_fully_cached(false);
    state.set_is_favorite(false);
    state.set_favorite_loading(false);
    // Default to a Qobuz album; the local-album loader opts in.
    state.set_is_local(false);
    state.set_loading(true);
}

// ==================== Multi-select (track selection) ====================

/// Toggle multi-select mode on the album track list. Leaving the mode
/// clears any current selection.
pub fn set_multi_select(window: &AppWindow, on: bool) {
    let state = window.global::<AlbumState>();
    state.set_multi_select(on);
    if !on {
        clear_selection(window);
    }
}

/// Recompute the "N selected" count from the track rows.
pub fn recount_selected(window: &AppWindow) {
    let state = window.global::<AlbumState>();
    let model = state.get_tracks();
    let count = (0..model.row_count())
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    state.set_selected_count(count as i32);
}

/// Select every row, or clear if all are already selected (the toggle the
/// "Select all" bulk button drives — same semantics as the favorites bar).
pub fn select_all(window: &AppWindow) {
    let model = window.global::<AlbumState>().get_tracks();
    let total = model.row_count();
    let selected = (0..total)
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    let target = selected != total;
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

/// Clear the selection (uncheck all), keeping multi-select mode on.
pub fn clear_selection(window: &AppWindow) {
    let model = window.global::<AlbumState>().get_tracks();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.selected {
                item.selected = false;
                model.set_row_data(i, item);
            }
        }
    }
    window.global::<AlbumState>().set_selected_count(0);
}

/// The catalog ids of the currently selected rows (for add-to-playlist /
/// add-to-favorites — Qobuz catalog ids only).
pub fn selected_ids(window: &AppWindow) -> Vec<String> {
    let model = window.global::<AlbumState>().get_tracks();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|t| t.selected)
        .map(|t| t.id.to_string())
        .filter(|s| s.parse::<u64>().is_ok())
        .collect()
}

/// The full catalog Track objects for the selected rows (for enqueue /
/// cache), resolved from the stashed raw album tracks by id.
pub fn selected_play_tracks(window: &AppWindow) -> Vec<Track> {
    let ids: std::collections::HashSet<String> = selected_ids(window).into_iter().collect();
    PLAY_TRACKS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|t| ids.contains(&t.id.to_string()))
            .cloned()
            .collect()
    })
}

/// The full catalog Track objects for one disc of the open album (for the
/// per-disc "Disc N" header menu), resolved from the stashed raw album tracks.
/// `disc` matches `media_number` (defaulting to 1, exactly as `map_track`
/// stamps `TrackData.disc`), so it lines up with the rendered "Disc N" header.
/// Preserves the delivered (disc-then-track) order.
pub fn disc_play_tracks(disc: i32) -> Vec<Track> {
    PLAY_TRACKS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|t| t.media_number.unwrap_or(1) as i32 == disc)
            .cloned()
            .collect()
    })
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
