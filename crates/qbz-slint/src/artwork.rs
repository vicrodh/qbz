//! Album artwork pipeline.
//!
//! Cover images are fetched and decoded off the UI thread, then applied to
//! individual `AlbumCardItem` rows on the Slint event loop. Only the
//! affected row is updated, so artwork arriving never resets a list
//! (POC ADR, sections 16 and 18).

use std::sync::Arc;

use slint::{ComponentHandle, Model};
use tokio::sync::Semaphore;

use crate::{AppWindow, HomeState};

/// Cap on simultaneous artwork downloads.
const MAX_CONCURRENT: usize = 16;

/// An artwork download job: which card, and the image URL.
pub struct ArtworkJob {
    pub section_idx: usize,
    pub album_idx: usize,
    pub url: String,
}

/// Spawn artwork downloads for every job. Each completion updates only its
/// own card row. Must be called from within the tokio runtime.
pub fn spawn_loads(jobs: Vec<ArtworkJob>, window: slint::Weak<AppWindow>) {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
    for job in jobs {
        let semaphore = semaphore.clone();
        let window = window.clone();
        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.ok()?;
            let (pixels, width, height) = fetch_and_decode(&job.url).await?;
            let _ = window.upgrade_in_event_loop(move |w| {
                apply_artwork(&w, job.section_idx, job.album_idx, &pixels, width, height);
            });
            Some(())
        });
    }
}

/// Download and decode one cover image into raw RGBA8 pixels. Runs on a
/// worker thread; the result tuple is `Send`.
async fn fetch_and_decode(url: &str) -> Option<(Vec<u8>, u32, u32)> {
    let bytes = reqwest::get(url).await.ok()?.bytes().await.ok()?;
    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some((rgba.into_raw(), width, height))
}

/// Apply decoded pixels to a single card row. Runs on the Slint event loop.
fn apply_artwork(
    window: &AppWindow,
    section_idx: usize,
    album_idx: usize,
    pixels: &[u8],
    width: u32,
    height: u32,
) {
    let sections = window.global::<HomeState>().get_sections();
    let Some(section) = sections.row_data(section_idx) else {
        return;
    };
    let Some(mut item) = section.albums.row_data(album_idx) else {
        return;
    };

    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let dst = buffer.make_mut_bytes();
    if dst.len() != pixels.len() {
        return;
    }
    dst.copy_from_slice(pixels);

    item.artwork = slint::Image::from_rgba8(buffer);
    section.albums.set_row_data(album_idx, item);
}
