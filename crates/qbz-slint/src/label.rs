//! LabelReleasesView controller — loads the label header (name +
//! image from /label/page) and the paginated album catalog (from
//! /label/getAlbums), pushing them into `LabelState`.
//!
//! Mirrors Tauri's LabelReleasesView.svelte data flow. The rich
//! sort / filter / group-by-artist controls there are deferred; this
//! port covers the header, the album grid, and load-more pagination.

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::Album;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AlbumCardItem, AppWindow, LabelState};

/// Page size for the album catalog. Tauri pulls 500 at a time; keep
/// the same so a typical label loads in one shot.
pub const PAGE_SIZE: u32 = 500;

pub struct LabelData {
    pub id: String,
    pub name: String,
    pub image_url: String,
    pub albums: Vec<AlbumCard>,
    pub total: usize,
}

#[derive(Clone)]
pub struct AlbumCard {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub genre: String,
    pub year: String,
    pub quality_tier: String,
    pub quality_label: String,
    pub artwork_url: String,
    // List-row extras (AlbumListRow columns; ignored by the grid card).
    pub quality_detail: String, // "24-bit / 96 kHz"
    pub track_count: String,    // "12"
    pub plain_year: String,     // "1973"
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
    let total = albums_page.total.unwrap_or(albums_page.items.len() as u32) as usize;
    let albums = albums_page.items.into_iter().map(map_album).collect();

    Ok(LabelData {
        id: label_id.to_string(),
        name,
        image_url,
        albums,
        total,
    })
}

/// Fetch one more album page for the load-more affordance.
pub async fn load_more_albums<A>(
    runtime: &Arc<AppRuntime<A>>,
    label_id: u64,
    offset: u32,
) -> Result<(Vec<AlbumCard>, usize), String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let page = runtime
        .core()
        .get_label_albums(label_id, PAGE_SIZE, offset, None, None, None, None, None)
        .await
        .map_err(|e| e.to_string())?;
    let total = page.total.unwrap_or(0) as usize;
    let albums = page.items.into_iter().map(map_album).collect();
    Ok((albums, total))
}

fn map_album(album: Album) -> AlbumCard {
    let year = album
        .release_date_original
        .as_deref()
        .and_then(|s| s.get(..4).map(|y| y.to_string()))
        .unwrap_or_default();
    let quality_tier = tier(album.maximum_bit_depth).to_string();
    let quality_label = match (album.maximum_bit_depth, album.maximum_sampling_rate) {
        (Some(bd), Some(sr)) => format!("{}-bit / {} kHz", bd, sr),
        _ => String::new(),
    };
    let genre = album.genre.map(|g| g.name).unwrap_or_default();
    let quality_detail = match (album.maximum_bit_depth, album.maximum_sampling_rate) {
        (Some(bd), Some(sr)) => format!("{}-bit / {} kHz", bd, sr),
        _ => String::new(),
    };
    let track_count = album
        .tracks_count
        .filter(|n| *n > 0)
        .map(|n| n.to_string())
        .unwrap_or_default();
    AlbumCard {
        id: album.id,
        title: album.title,
        artist: album.artist.name,
        artist_id: album.artist.id.to_string(),
        genre,
        year: year.clone(),
        quality_tier,
        quality_label,
        artwork_url: album.image.best().cloned().unwrap_or_default(),
        quality_detail,
        track_count,
        plain_year: year,
    }
}

fn tier(bit_depth: Option<u32>) -> &'static str {
    match bit_depth {
        Some(b) if b > 16 => "hires",
        Some(_) => "cd",
        None => "",
    }
}

/// Extract the best URL from /label/page's flexible image value. It
/// can be a bare string or an object with mega/extralarge/large/...
/// keys (mirrors the Svelte extraction order).
fn extract_label_image(image: Option<&serde_json::Value>) -> String {
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

fn to_item(card: AlbumCard) -> AlbumCardItem {
    AlbumCardItem {
        id: card.id.into(),
        title: card.title.into(),
        artist: card.artist.into(),
        artist_id: card.artist_id.into(),
        genre: card.genre.into(),
        year: card.year.into(),
        quality_tier: card.quality_tier.into(),
        quality_label: card.quality_label.into(),
        ribbon: "".into(),
        ribbon_kind: "".into(),
        artwork_url: card.artwork_url.into(),
        artwork: slint::Image::default(),
        // List-row extras — feed the AlbumListRow columns (QUALITY /
        // TRACKS / YEAR) for the list view toggle. SOURCE is hidden in
        // this single-source (Qobuz) context.
        quality_detail: card.quality_detail.into(),
        track_count: card.track_count.into(),
        plain_year: card.plain_year.into(),
        ..Default::default()
    }
}

pub fn apply_label(window: &AppWindow, data: LabelData) {
    let items: Vec<AlbumCardItem> = data.albums.into_iter().map(to_item).collect();
    let state = window.global::<LabelState>();
    state.set_id(data.id.into());
    state.set_name(data.name.into());
    state.set_image_url(data.image_url.into());
    state.set_albums(ModelRc::new(VecModel::from(items)));
    state.set_total(data.total as i32);
    state.set_loading(false);
}

pub fn append_albums(window: &AppWindow, albums: Vec<AlbumCard>, total: usize) {
    let state = window.global::<LabelState>();
    let model = state.get_albums();
    let mut combined: Vec<AlbumCardItem> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .collect();
    combined.extend(albums.into_iter().map(to_item));
    state.set_albums(ModelRc::new(VecModel::from(combined)));
    state.set_total(total as i32);
    state.set_load_more_loading(false);
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
    state.set_total(0);
    state.set_loading(true);
    state.set_load_more_loading(false);
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
