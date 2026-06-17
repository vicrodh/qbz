//! LabelReleasesView controller — loads the label header (name +
//! image from /label/page) and the paginated album catalog (from
//! /label/getAlbums), pushing them into `LabelState`.
//!
//! Mirrors Tauri's LabelReleasesView.svelte data flow. The rich
//! sort / filter / group-by-artist controls there are deferred; this
//! port covers the header, the album grid, and load-more pagination.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{Album, LabelExploreResponse, Track};
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::album_map::{map_album, sort_album_items, tier, to_item, AlbumCard};
use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{
    AlbumCardItem, AppWindow, DiscoverSection, JumpNavTab, LabelState, SearchPlaylistItem,
    SlimItem, TrackItem,
};

/// Page size for the album catalog. Tauri pulls 500 at a time; keep
/// the same so a typical label loads in one shot.
pub const PAGE_SIZE: u32 = 500;

pub struct LabelData {
    pub id: String,
    pub name: String,
    pub image_url: String,
    pub albums: Vec<AlbumCard>,
    pub total: usize,
    pub has_more: bool,
}

/// Fetch the label page (name + image) and the first album page.
pub async fn load_label<A>(
    runtime: &Arc<AppRuntime<A>>,
    label_id: u64,
    fallback_name: &str,
) -> Result<LabelData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let page = runtime
        .core()
        .get_label_page(label_id)
        .await
        .map_err(|e| e.to_string())?;

    let albums_page = runtime
        .core()
        .get_label_albums(label_id, PAGE_SIZE, 0, None, None, None, None, None)
        .await
        .map_err(|e| e.to_string())?;

    let name = if page.name.is_empty() {
        fallback_name.to_string()
    } else {
        page.name
    };
    let image_url = extract_label_image(page.image.as_ref());
    let item_count = albums_page.items.len();
    let total = albums_page
        .total
        .map(|t| t as usize)
        .unwrap_or(item_count);
    // /label/getAlbums caps each page below the full catalog; trust the
    // `has_more` flag, falling back to a total comparison when it's absent.
    let has_more = albums_page.has_more.unwrap_or(total > item_count);
    let albums = albums_page.items.into_iter().map(map_album).collect();

    Ok(LabelData {
        id: label_id.to_string(),
        name,
        image_url,
        albums,
        total,
        has_more,
    })
}

/// Fetch one more album page for the load-more affordance. Returns the
/// new cards, the (best-known) total, and whether more pages remain.
pub async fn load_more_albums<A>(
    runtime: &Arc<AppRuntime<A>>,
    label_id: u64,
    offset: u32,
) -> Result<(Vec<AlbumCard>, usize, bool), String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let page = runtime
        .core()
        .get_label_albums(label_id, PAGE_SIZE, offset, None, None, None, None, None)
        .await
        .map_err(|e| e.to_string())?;
    let item_count = page.items.len();
    let loaded = offset as usize + item_count;
    let total = page.total.map(|t| t as usize).unwrap_or(loaded);
    // More pages remain when the API says so, or when this page came back
    // full (a short page means the catalog is exhausted).
    let has_more = page
        .has_more
        .unwrap_or(item_count >= PAGE_SIZE as usize || total > loaded);
    let albums = page.items.into_iter().map(map_album).collect();
    Ok((albums, total, has_more))
}

/// Extract the best URL from /label/page's flexible image value. It
/// can be a bare string or an object with mega/extralarge/large/...
/// keys (mirrors the Svelte extraction order). Reused by the favorites
/// Labels tab, whose wire `image` is a bare string per the Android DTO.
pub(crate) fn extract_label_image(image: Option<&serde_json::Value>) -> String {
    let Some(image) = image else {
        return String::new();
    };
    if let Some(s) = image.as_str() {
        return s.to_string();
    }
    for key in ["mega", "extralarge", "large", "thumbnail", "small"] {
        if let Some(s) = image.get(key).and_then(|v| v.as_str()) {
            return s.to_string();
        }
    }
    String::new()
}

pub fn apply_label(window: &AppWindow, data: LabelData) {
    let items: Vec<AlbumCardItem> = data.albums.into_iter().map(to_item).collect();
    let state = window.global::<LabelState>();
    state.set_id(data.id.into());
    state.set_name(data.name.into());
    state.set_image_url(data.image_url.into());
    state.set_albums(ModelRc::new(VecModel::from(items)));
    state.set_total(data.total as i32);
    state.set_has_more(data.has_more);
    state.set_loading(false);
    derive_releases(window);
}

/// Re-derive the releases sub-view's rendered list (`visible` / `grouped`)
/// from the full loaded catalog + the toolbar state (sort / Hi-Res filter /
/// search / group-by-artist). Mirrors Tauri's client-side `$derived`
/// processing. Search is a local filter over the loaded catalog. Artwork
/// stays live in the common case because the no-filter, default-sort path
/// shares the `albums` model (the DiscoverBrowse pattern).
pub fn derive_releases(window: &AppWindow) {
    let state = window.global::<LabelState>();
    let albums = state.get_albums();
    let count = albums.row_count();
    let full: Vec<AlbumCardItem> = (0..count).filter_map(|i| albums.row_data(i)).collect();

    let sort = state.get_sort_by().to_string();
    let hires = state.get_filter_hires();
    let group = state.get_group_by_artist();
    let query_owned = state.get_search_query().to_lowercase();
    let query = query_owned.trim();

    let hires_count = full
        .iter()
        .filter(|a| a.quality_tier.as_str() == "hires")
        .count();

    // Fast path: default order, no filter/search, flat → render the live
    // `albums` model directly so artwork keeps updating in place.
    if !hires && query.is_empty() && sort == "newest" && !group {
        state.set_visible(albums.clone());
        state.set_grouped(ModelRc::new(VecModel::from(Vec::<DiscoverSection>::new())));
        state.set_shown(full.len() as i32);
        state.set_hires_count(hires_count as i32);
        return;
    }

    let mut filtered: Vec<AlbumCardItem> = full
        .into_iter()
        .filter(|a| {
            (!hires || a.quality_tier.as_str() == "hires")
                && (query.is_empty()
                    || a.title.to_lowercase().contains(query)
                    || a.artist.to_lowercase().contains(query))
        })
        .collect();
    sort_album_items(&mut filtered, &sort);
    let shown = filtered.len();

    if group {
        // One section per artist, in first-appearance (sorted) order.
        let mut sections: Vec<DiscoverSection> = Vec::new();
        let mut buckets: Vec<Vec<AlbumCardItem>> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        for item in filtered {
            let key = item.artist.to_string();
            let idx = *index.entry(key.clone()).or_insert_with(|| {
                buckets.push(Vec::new());
                sections.push(DiscoverSection {
                    title: if key.is_empty() {
                        "Unknown".into()
                    } else {
                        key.clone().into()
                    },
                    endpoint: "".into(),
                    albums: ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())),
                });
                buckets.len() - 1
            });
            buckets[idx].push(item);
        }
        for (i, bucket) in buckets.into_iter().enumerate() {
            sections[i].albums = ModelRc::new(VecModel::from(bucket));
        }
        state.set_grouped(ModelRc::new(VecModel::from(sections)));
        state.set_visible(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
    } else {
        state.set_visible(ModelRc::new(VecModel::from(filtered)));
        state.set_grouped(ModelRc::new(VecModel::from(Vec::<DiscoverSection>::new())));
    }
    state.set_shown(shown as i32);
    state.set_hires_count(hires_count as i32);
}

pub fn append_albums(window: &AppWindow, albums: Vec<AlbumCard>, total: usize, has_more: bool) {
    let state = window.global::<LabelState>();
    let model = state.get_albums();
    let mut combined: Vec<AlbumCardItem> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .collect();
    combined.extend(albums.into_iter().map(to_item));
    state.set_albums(ModelRc::new(VecModel::from(combined)));
    state.set_total(total as i32);
    state.set_has_more(has_more);
    state.set_load_more_loading(false);
    derive_releases(window);
}

/// Apply the decoded label header image. Runs on the Slint event loop.
pub fn apply_image(window: &AppWindow, pixels: &[u8], width: u32, height: u32) {
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let dst = buffer.make_mut_bytes();
    if dst.len() != pixels.len() {
        return;
    }
    dst.copy_from_slice(pixels);
    window
        .global::<LabelState>()
        .set_image(slint::Image::from_rgba8(buffer));
}

pub fn reset_label(window: &AppWindow) {
    let state = window.global::<LabelState>();
    state.set_name("".into());
    state.set_image_url("".into());
    state.set_albums(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
    state.set_visible(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
    state.set_grouped(ModelRc::new(VecModel::from(Vec::<DiscoverSection>::new())));
    state.set_total(0);
    state.set_has_more(false);
    state.set_loading(true);
    state.set_load_more_loading(false);
    // Reset the toolbar to defaults for the fresh label.
    state.set_sort_by("newest".into());
    state.set_filter_hires(false);
    state.set_group_by_artist(false);
    state.set_search_query("".into());
    state.set_shown(0);
    state.set_hires_count(0);
}

/// Artwork jobs for the label album grid — same pipeline the
/// Discover cards use.
pub fn artwork_jobs(data: &LabelData) -> Vec<ArtworkJob> {
    data.albums
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.artwork_url.is_empty())
        .map(|(i, a)| ArtworkJob {
            url: a.artwork_url.clone(),
            target: ArtworkTarget::LabelAlbum { index: i },
        })
        .collect()
}

// ======================================================================
//  Landing page (LabelPageView) — the rich label page: header +
//  popular tracks + releases / critics / playlists / artists /
//  more-labels carousels. Mirrors Tauri's LabelView.svelte. Data comes
//  from /label/page (top_tracks/releases/playlists/top_artists), the
//  first /label/getAlbums page (releases carousel), /label/explore
//  (more labels), and the user's favorite-labels set (follow state).
// ======================================================================

/// Plain, `Send` payload for the rich label landing page.
pub struct LabelPagePayload {
    pub id: String,
    pub name: String,
    pub image_url: String,
    pub description: String,
    pub description_short: String,
    pub description_truncated: bool,
    pub is_following: bool,
    pub top_tracks: Vec<TopTrack>,
    pub releases: Vec<AlbumCard>,
    pub critics: Vec<AlbumCard>,
    pub playlists: Vec<PlaylistSlim>,
    pub artists: Vec<ArtistSlim>,
    pub more_labels: Vec<LabelSlim>,
    /// Catalog tracks kept for "Play all" — deserialized from the page
    /// top_tracks and queued verbatim (mirrors Tauri's buildTopTracksQueue).
    pub play_tracks: Vec<Track>,
}

#[derive(Clone)]
pub struct TopTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub album_id: String,
    pub artwork_url: String,
    pub duration: String,
    pub quality_tier: String,
    pub quality_detail: String,
}

#[derive(Clone)]
pub struct PlaylistSlim {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub image_url: String,
}

#[derive(Clone)]
pub struct ArtistSlim {
    pub id: String,
    pub name: String,
    pub image_url: String,
}

#[derive(Clone)]
pub struct LabelSlim {
    pub id: String,
    pub name: String,
    pub image_url: String,
    pub following: bool,
}

// Catalog tracks for the landing's "Play all", cached on the UI thread
// (set in `apply_label_page`, read by the play-top media action).
thread_local! {
    static PLAY_TOP_TRACKS: RefCell<Vec<Track>> = const { RefCell::new(Vec::new()) };
}

/// The label's popular tracks as a play-ready queue source.
pub fn top_tracks_for_play() -> Vec<Track> {
    PLAY_TOP_TRACKS.with(|c| c.borrow().clone())
}

/// Fetch + map the full label landing page.
pub async fn load_label_page<A>(
    runtime: &Arc<AppRuntime<A>>,
    label_id: u64,
    fallback_name: &str,
) -> Result<LabelPagePayload, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let page = runtime
        .core()
        .get_label_page(label_id)
        .await
        .map_err(|e| e.to_string())?;

    // Favorite-label ids — seed the header + per-card follow state.
    let follow_ids = favorite_label_ids(runtime).await;

    let name = if page.name.is_empty() {
        fallback_name.to_string()
    } else {
        page.name.clone()
    };
    let image_url = extract_label_image(page.image.as_ref());

    // Description (HTML-stripped) + truncation for the header read-more.
    let description = page
        .description
        .as_deref()
        .map(crate::strip_html::strip_html)
        .unwrap_or_default();
    let description_short = truncate_words(&description, 360);
    let description_truncated = description_short != description;

    // Popular tracks → display rows + play-all queue source.
    let raw_top = page.top_tracks.clone().unwrap_or_default();
    let top_tracks: Vec<TopTrack> = raw_top.iter().map(parse_top_track).collect();
    let play_tracks: Vec<Track> = raw_top
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Track>(v).ok())
        .collect();

    // Critics' Picks — the releases container whose id mentions
    // award/critic/press (mirrors LabelView.svelte:402-413).
    let critics: Vec<AlbumCard> = page
        .releases
        .as_ref()
        .and_then(|containers| {
            containers
                .iter()
                .find(|c| {
                    c.id
                        .as_deref()
                        .map(|id| {
                            let id = id.to_lowercase();
                            id.contains("award") || id.contains("critic") || id.contains("press")
                        })
                        .unwrap_or(false)
                })
                .and_then(|c| c.data.as_ref())
                .and_then(|d| d.items.as_ref())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| serde_json::from_value::<Album>(v.clone()).ok())
                        .map(map_album)
                        .collect()
                })
        })
        .unwrap_or_default();

    let playlists: Vec<PlaylistSlim> = page
        .playlists
        .as_ref()
        .and_then(|p| p.items.as_ref())
        .map(|items| items.iter().map(parse_playlist).collect())
        .unwrap_or_default();

    let artists: Vec<ArtistSlim> = page
        .top_artists
        .as_ref()
        .and_then(|a| a.items.as_ref())
        .map(|items| items.iter().map(parse_artist).collect())
        .unwrap_or_default();

    // Releases carousel — first 20 from /label/getAlbums.
    let releases: Vec<AlbumCard> = match runtime
        .core()
        .get_label_albums(label_id, 20, 0, None, None, None, None, None)
        .await
    {
        Ok(p) => p.items.into_iter().map(map_album).collect(),
        Err(e) => {
            log::warn!("[qbz-slint] label releases carousel failed: {e}");
            Vec::new()
        }
    };

    // More labels — /label/explore minus the current label; seed follow.
    let more_labels: Vec<LabelSlim> = match runtime.core().get_label_explore(20, 0).await {
        Ok(resp) => parse_more_labels(&resp, label_id, &follow_ids),
        Err(e) => {
            log::warn!("[qbz-slint] label explore failed: {e}");
            Vec::new()
        }
    };

    Ok(LabelPagePayload {
        id: label_id.to_string(),
        name,
        image_url,
        description,
        description_short,
        description_truncated,
        is_following: follow_ids.contains(&label_id),
        top_tracks,
        releases,
        critics,
        playlists,
        artists,
        more_labels,
        play_tracks,
    })
}

/// The user's favorite-label ids (for the header + more-labels follow
/// state). Best-effort: an error yields an empty set.
async fn favorite_label_ids<A>(runtime: &Arc<AppRuntime<A>>) -> HashSet<u64>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    match runtime.core().get_favorites("label", 500, 0).await {
        Ok(v) => v
            .get("labels")
            .and_then(|l| l.get("items"))
            .and_then(|i| i.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|it| it.get("id").and_then(|x| x.as_u64()))
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => HashSet::new(),
    }
}

fn parse_top_track(raw: &Value) -> TopTrack {
    let id = raw
        .get("id")
        .and_then(|v| v.as_u64())
        .map(|n| n.to_string())
        .unwrap_or_default();
    let title = raw
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let duration = raw.get("duration").and_then(|v| v.as_u64()).unwrap_or(0);
    let album = raw.get("album");
    let album_id = album
        .and_then(|a| a.get("id"))
        .map(value_to_string)
        .unwrap_or_default();
    let artwork_url = album
        .and_then(|a| a.get("image"))
        .map(parse_image_value)
        .unwrap_or_default();
    // performer OR artist (the page uses either).
    let perf = raw.get("performer").or_else(|| raw.get("artist"));
    let artist = perf
        .and_then(|p| p.get("name"))
        .map(name_display)
        .unwrap_or_default();
    let artist_id = perf
        .and_then(|p| p.get("id"))
        .map(value_to_string)
        .unwrap_or_default();
    let bit_depth = raw
        .get("audio_info")
        .and_then(|a| a.get("maximum_bit_depth"))
        .and_then(|v| v.as_u64())
        .or_else(|| raw.get("maximum_bit_depth").and_then(|v| v.as_u64()));
    let sample_rate = raw
        .get("audio_info")
        .and_then(|a| a.get("maximum_sampling_rate"))
        .and_then(|v| v.as_f64())
        .or_else(|| raw.get("maximum_sampling_rate").and_then(|v| v.as_f64()));
    let bit_depth = bit_depth.map(|b| b as u32);
    TopTrack {
        id,
        title,
        artist,
        artist_id,
        album_id,
        artwork_url,
        duration: mmss(duration as u32),
        quality_tier: tier(bit_depth).to_string(),
        quality_detail: crate::quality::detail(bit_depth, sample_rate),
    }
}

fn parse_playlist(raw: &Value) -> PlaylistSlim {
    let id = raw.get("id").map(value_to_string).unwrap_or_default();
    let title = raw
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let owner = raw
        .get("owner")
        .and_then(|o| o.get("name"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Qobuz")
        .to_string();
    let track_count = raw.get("tracks_count").and_then(|v| v.as_u64()).unwrap_or(0);
    let subtitle = format!("{owner} · {track_count}");
    PlaylistSlim {
        id,
        title,
        subtitle,
        image_url: parse_playlist_image(raw),
    }
}

fn parse_artist(raw: &Value) -> ArtistSlim {
    ArtistSlim {
        id: raw.get("id").map(value_to_string).unwrap_or_default(),
        name: raw.get("name").map(name_display).unwrap_or_default(),
        image_url: parse_artist_image(raw),
    }
}

fn parse_more_labels(
    resp: &LabelExploreResponse,
    current: u64,
    follow: &HashSet<u64>,
) -> Vec<LabelSlim> {
    let Some(items) = resp.items.as_ref() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.get("id").and_then(|x| x.as_u64())?;
            if id == current {
                return None;
            }
            let name = item
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string();
            let image_url = item
                .get("image")
                .map(parse_explore_image)
                .unwrap_or_default();
            Some(LabelSlim {
                id: id.to_string(),
                name,
                image_url,
                following: follow.contains(&id),
            })
        })
        .collect()
}

// --- Value helpers (mirror the Svelte getX helpers) ----------------------

/// String for an id-ish Value (string verbatim, number stringified).
fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Display name: `{display}` object form or a bare string.
fn name_display(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    v.get("display")
        .and_then(|d| d.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Best URL out of an album `image` Value (string or {large|...}).
fn parse_image_value(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    for key in ["large", "extralarge", "medium", "thumbnail", "small"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            return s.to_string();
        }
    }
    String::new()
}

/// getArtistImageUrl: image{large|extralarge|medium|thumbnail|small} ->
/// picture(string) -> images.portrait hash (medium covers).
fn parse_artist_image(raw: &Value) -> String {
    if let Some(image) = raw.get("image") {
        let url = parse_image_value(image);
        if !url.is_empty() {
            return url;
        }
    }
    if let Some(pic) = raw.get("picture").and_then(|v| v.as_str()) {
        if !pic.is_empty() {
            return pic.to_string();
        }
    }
    if let Some(portrait) = raw.get("images").and_then(|i| i.get("portrait")) {
        if let (Some(hash), Some(format)) = (
            portrait.get("hash").and_then(|v| v.as_str()),
            portrait.get("format").and_then(|v| v.as_str()),
        ) {
            return format!(
                "https://static.qobuz.com/images/artists/covers/medium/{hash}.{format}"
            );
        }
    }
    String::new()
}

/// getPlaylistImage: image.rectangle -> image.covers[0] ->
/// image{large|thumbnail|small} -> images300[0] -> images150[0] ->
/// images[0].
fn parse_playlist_image(raw: &Value) -> String {
    if let Some(image) = raw.get("image") {
        if let Some(rect) = image.get("rectangle").and_then(|v| v.as_str()) {
            if !rect.is_empty() {
                return rect.to_string();
            }
        }
        if let Some(cover) = image
            .get("covers")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
        {
            return cover.to_string();
        }
        for key in ["large", "thumbnail", "small"] {
            if let Some(s) = image.get(key).and_then(|v| v.as_str()) {
                return s.to_string();
            }
        }
    }
    for key in ["images300", "images150", "images"] {
        if let Some(s) = raw
            .get(key)
            .and_then(|a| a.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
        {
            return s.to_string();
        }
    }
    String::new()
}

/// parseLabelExploreImage: string or {large|thumbnail|small}.
fn parse_explore_image(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    for key in ["large", "thumbnail", "small"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            return s.to_string();
        }
    }
    String::new()
}

fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Word-boundary truncation with an ellipsis (mirrors artist::truncate_words).
fn truncate_words(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max).collect();
    let cut = truncated.rfind(' ').unwrap_or(truncated.len());
    format!("{}…", truncated[..cut].trim_end())
}

// --- Slint mapping -------------------------------------------------------

fn top_track_to_item(t: &TopTrack) -> TrackItem {
    TrackItem {
        // Label landing top-tracks are out of Task 6 row-stamping scope.
        is_blacklisted: false,
        id: t.id.clone().into(),
        number: "".into(),
        title: t.title.clone().into(),
        artist: t.artist.clone().into(),
        album: "".into(),
        duration: t.duration.clone().into(),
        quality_tier: t.quality_tier.clone().into(),
        quality_detail: t.quality_detail.clone().into(),
        explicit: false,
        selected: false,
        artwork_url: t.artwork_url.clone().into(),
        artwork: slint::Image::default(),
        is_favorite: crate::fav_cache::is_favorite(&t.id),
        artist_id: t.artist_id.clone().into(),
        album_id: t.album_id.clone().into(),
        removing: false,
        cache_status: if crate::offline_cache::is_cached(&t.id) { 3 } else { 0 },
        cache_progress: 0.0,
        source: "qobuz".into(),
        unlocking: false,
        // Disc grouping is album-detail only; flat lists carry none.
        disc_header_number: 0,
    }
}

fn playlist_to_item(p: &PlaylistSlim) -> SearchPlaylistItem {
    SearchPlaylistItem {
        id: p.id.clone().into(),
        title: p.title.clone().into(),
        subtitle: p.subtitle.clone().into(),
        cover_count: if p.image_url.is_empty() { 0 } else { 1 },
        url1: p.image_url.clone().into(),
        url2: "".into(),
        url3: "".into(),
        url4: "".into(),
        cover1: slint::Image::default(),
        cover2: slint::Image::default(),
        cover3: slint::Image::default(),
        cover4: slint::Image::default(),
        // Label-landing playlist cards carry no category subtag, and a
        // transparent dominant-colour is the sentinel for "no letterbox":
        // the collage keeps the legacy cover-fit (the contain + dominant-
        // colour treatment is Discover-only).
        category: "".into(),
        dominant_color: slint::Color::from_argb_u8(0, 0, 0, 0),
    }
}

fn artist_to_item(a: &ArtistSlim) -> SlimItem {
    SlimItem {
        id: a.id.clone().into(),
        title: a.name.clone().into(),
        subtitle: "".into(),
        rank: "".into(),
        artwork_url: a.image_url.clone().into(),
        artwork: slint::Image::default(),
        following: false,
    }
}

fn label_to_item(l: &LabelSlim) -> SlimItem {
    SlimItem {
        id: l.id.clone().into(),
        title: l.name.clone().into(),
        subtitle: "".into(),
        rank: "".into(),
        artwork_url: l.image_url.clone().into(),
        artwork: slint::Image::default(),
        following: l.following,
    }
}

fn section(title: &str, cards: &[AlbumCard]) -> DiscoverSection {
    DiscoverSection {
        title: title.into(),
        endpoint: "".into(),
        albums: ModelRc::new(VecModel::from(
            cards.iter().cloned().map(to_item).collect::<Vec<_>>(),
        )),
    }
}

/// Apply the landing payload to `LabelState`. Runs on the Slint event loop.
pub fn apply_label_page(window: &AppWindow, payload: LabelPagePayload) {
    PLAY_TOP_TRACKS.with(|c| *c.borrow_mut() = payload.play_tracks.clone());

    let top_tracks: Vec<TrackItem> = payload.top_tracks.iter().map(top_track_to_item).collect();
    let playlists: Vec<SearchPlaylistItem> =
        payload.playlists.iter().map(playlist_to_item).collect();
    let artists: Vec<SlimItem> = payload.artists.iter().map(artist_to_item).collect();
    let more_labels: Vec<SlimItem> = payload.more_labels.iter().map(label_to_item).collect();
    let releases = section("Releases", &payload.releases);
    let critics = section("Critics' Picks", &payload.critics);
    let jump_tabs = build_label_jump_tabs(&payload);

    let state = window.global::<LabelState>();
    state.set_id(payload.id.into());
    state.set_name(payload.name.into());
    state.set_image_url(payload.image_url.into());
    state.set_description(payload.description.into());
    state.set_description_short(payload.description_short.into());
    state.set_description_truncated(payload.description_truncated);
    state.set_is_following(payload.is_following);
    state.set_top_tracks(ModelRc::new(VecModel::from(top_tracks)));
    state.set_releases_section(releases);
    state.set_critics_section(critics);
    state.set_playlists(ModelRc::new(VecModel::from(playlists)));
    state.set_artists(ModelRc::new(VecModel::from(artists)));
    state.set_more_labels(ModelRc::new(VecModel::from(more_labels)));
    state.set_jump_tabs(ModelRc::new(VecModel::from(jump_tabs)));
    state.set_page_loaded(true);
    state.set_loading(false);
}

/// Build the JUMP TO tabs for the landing — only sections with content.
/// anchor-y values are layout-derived estimates (variable header/grid
/// heights make exact numbers impractical; the estimate lands the user
/// inside the right section). Mirrors artist::build_jump_tabs.
fn build_label_jump_tabs(payload: &LabelPagePayload) -> Vec<JumpNavTab> {
    const HEADER_GUESS: f32 = 360.0;
    const SECTION_SPACER: f32 = 40.0;
    const CAROUSEL: f32 = 320.0;
    const POPULAR_HEADER: f32 = 46.0;
    const POPULAR_ROW: f32 = 50.0;
    const POPULAR_TAIL: f32 = 40.0;

    let mut tabs: Vec<JumpNavTab> = Vec::new();
    tabs.push(JumpNavTab {
        id: "about".into(),
        label: "About".into(),
        anchor_y: 0.0,
    });
    let mut cursor = HEADER_GUESS;

    if !payload.top_tracks.is_empty() {
        tabs.push(JumpNavTab {
            id: "popular".into(),
            label: "Popular Tracks".into(),
            anchor_y: cursor,
        });
        let rows = payload.top_tracks.len().min(5) as f32;
        cursor += POPULAR_HEADER + rows * POPULAR_ROW + POPULAR_TAIL;
    }
    let push_carousel = |tabs: &mut Vec<JumpNavTab>, id: &str, label: &str, present: bool, cursor: &mut f32| {
        if present {
            tabs.push(JumpNavTab {
                id: id.into(),
                label: label.into(),
                anchor_y: *cursor,
            });
            *cursor += SECTION_SPACER + CAROUSEL;
        }
    };
    push_carousel(&mut tabs, "releases", "Releases", !payload.releases.is_empty(), &mut cursor);
    push_carousel(&mut tabs, "critics", "Critics' Picks", !payload.critics.is_empty(), &mut cursor);
    push_carousel(&mut tabs, "playlists", "Playlists", !payload.playlists.is_empty(), &mut cursor);
    push_carousel(&mut tabs, "artists", "Artists", !payload.artists.is_empty(), &mut cursor);
    push_carousel(&mut tabs, "labels", "More Labels", !payload.more_labels.is_empty(), &mut cursor);
    tabs
}

/// Artwork jobs for the landing sections (top-track thumbs + carousels).
pub fn page_artwork_jobs(payload: &LabelPagePayload) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    let push = |jobs: &mut Vec<ArtworkJob>, url: &str, target: ArtworkTarget| {
        if !url.is_empty() {
            jobs.push(ArtworkJob {
                url: url.to_string(),
                target,
            });
        }
    };
    for (i, t) in payload.top_tracks.iter().enumerate() {
        push(&mut jobs, &t.artwork_url, ArtworkTarget::LabelTopTrack { index: i });
    }
    for (i, a) in payload.releases.iter().enumerate() {
        push(&mut jobs, &a.artwork_url, ArtworkTarget::LabelReleaseAlbum { index: i });
    }
    for (i, a) in payload.critics.iter().enumerate() {
        push(&mut jobs, &a.artwork_url, ArtworkTarget::LabelCriticsAlbum { index: i });
    }
    for (i, p) in payload.playlists.iter().enumerate() {
        push(&mut jobs, &p.image_url, ArtworkTarget::LabelPlaylistCover { index: i });
    }
    for (i, a) in payload.artists.iter().enumerate() {
        push(&mut jobs, &a.image_url, ArtworkTarget::LabelArtist { index: i });
    }
    for (i, l) in payload.more_labels.iter().enumerate() {
        push(&mut jobs, &l.image_url, ArtworkTarget::LabelMoreLabel { index: i });
    }
    jobs
}

/// Current follow state for `label_id` — the header when it's the open
/// label, else the matching More-Labels card.
pub fn label_following_state(window: &AppWindow, label_id: &str) -> bool {
    let state = window.global::<LabelState>();
    if state.get_id().as_str() == label_id {
        return state.get_is_following();
    }
    let model = state.get_more_labels();
    for i in 0..model.row_count() {
        if let Some(item) = model.row_data(i) {
            if item.id.as_str() == label_id {
                return item.following;
            }
        }
    }
    false
}

/// Name of a More-Labels card by id (nav-history fallback for a card click).
pub fn more_label_name(window: &AppWindow, label_id: &str) -> String {
    let model = window.global::<LabelState>().get_more_labels();
    for i in 0..model.row_count() {
        if let Some(item) = model.row_data(i) {
            if item.id.as_str() == label_id {
                return item.title.to_string();
            }
        }
    }
    String::new()
}

/// Optimistically reflect a follow toggle — flips the header state when
/// it's the current label, and the matching more-labels card.
pub fn mark_label_followed(window: &AppWindow, label_id: &str, following: bool) {
    let state = window.global::<LabelState>();
    if state.get_id().as_str() == label_id {
        state.set_is_following(following);
    }
    let model = state.get_more_labels();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.id.as_str() == label_id {
                item.following = following;
                model.set_row_data(i, item);
            }
        }
    }
}

/// Clear the landing state before loading a new label.
pub fn reset_label_page(window: &AppWindow) {
    let state = window.global::<LabelState>();
    state.set_name("".into());
    state.set_image_url("".into());
    state.set_image(slint::Image::default());
    state.set_description("".into());
    state.set_description_short("".into());
    state.set_description_truncated(false);
    state.set_is_following(false);
    state.set_top_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_releases_section(DiscoverSection::default());
    state.set_critics_section(DiscoverSection::default());
    state.set_playlists(ModelRc::new(VecModel::from(Vec::<SearchPlaylistItem>::new())));
    state.set_artists(ModelRc::new(VecModel::from(Vec::<SlimItem>::new())));
    state.set_more_labels(ModelRc::new(VecModel::from(Vec::<SlimItem>::new())));
    state.set_jump_tabs(ModelRc::new(VecModel::from(Vec::<JumpNavTab>::new())));
    state.set_page_loaded(false);
    state.set_loading(true);
}
