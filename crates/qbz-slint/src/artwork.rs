//! Album artwork pipeline.
//!
//! Cover images go through the shared QBZ image cache (`qbz_cache`), the
//! same disk cache the Tauri app uses — covers are never re-downloaded
//! once cached. Fetch and decode run off the UI thread; each decoded
//! cover is applied to its own `AlbumCardItem` row on the Slint event
//! loop, so artwork arriving never resets a list (POC ADR 16 and 18).

use std::sync::{Arc, Mutex};

use qbz_cache::ImageCacheService;
use slint::{ComponentHandle, Model};
use tokio::sync::Semaphore;

use crate::{AppWindow, HomeState};

/// Cap on simultaneous artwork downloads.
const MAX_CONCURRENT: usize = 16;

/// Target decode size. Cards display at 220px; 264px keeps them crisp at
/// modest DPI without holding full ~600px source textures in memory.
const DECODE_SIZE: u32 = 264;

/// Default image-cache size budget (matches the Tauri default).
pub const MAX_CACHE_BYTES: u64 = 200 * 1024 * 1024;

/// Shared, optional image cache. `None` when the cache could not be opened
/// — artwork then falls back to direct downloads.
pub type ImageCache = Arc<Mutex<Option<ImageCacheService>>>;

/// Which card an artwork download targets.
#[derive(Clone, Copy)]
pub enum ArtworkTarget {
    /// A card in `HomeState.sections[section_idx].albums[album_idx]`.
    Section { section_idx: usize, album_idx: usize },
    /// A card in `HomeState.popular[idx]`.
    Popular { idx: usize },
}

/// An artwork download job: which card, and the image URL.
pub struct ArtworkJob {
    pub target: ArtworkTarget,
    pub url: String,
}

/// Open the shared QBZ image cache.
pub fn open_cache() -> ImageCache {
    match ImageCacheService::new() {
        Ok(service) => Arc::new(Mutex::new(Some(service))),
        Err(e) => {
            log::warn!("[qbz-slint] image cache unavailable: {e}");
            Arc::new(Mutex::new(None))
        }
    }
}

/// Trim the image cache to the size budget. Runs once at startup.
pub fn spawn_evict(cache: ImageCache) {
    tokio::spawn(async move {
        if let Ok(guard) = cache.lock() {
            if let Some(service) = guard.as_ref() {
                match service.evict(MAX_CACHE_BYTES) {
                    Ok(freed) if freed > 0 => {
                        log::info!("[qbz-slint] image cache evicted {freed} bytes")
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!("[qbz-slint] image cache eviction failed: {e}"),
                }
            }
        }
    });
}

/// Spawn artwork downloads for every job. Each completion updates only its
/// own card row. Must be called from within the tokio runtime.
pub fn spawn_loads(jobs: Vec<ArtworkJob>, window: slint::Weak<AppWindow>, cache: ImageCache) {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
    for job in jobs {
        let semaphore = semaphore.clone();
        let window = window.clone();
        let cache = cache.clone();
        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.ok()?;
            let (pixels, width, height) =
                fetch_and_decode(&job.url, &cache, DECODE_SIZE).await?;
            let target = job.target;
            let _ = window.upgrade_in_event_loop(move |w| {
                apply_artwork(&w, target, &pixels, width, height);
            });
            Some(())
        });
    }
}

/// Resolve one cover image to raw RGBA8 pixels, downscaled to `decode_size`.
/// Reads from the shared cache on a hit; on a miss downloads, stores, and
/// uses the bytes. Runs on a worker thread; the result tuple is `Send`.
pub async fn fetch_and_decode(
    url: &str,
    cache: &ImageCache,
    decode_size: u32,
) -> Option<(Vec<u8>, u32, u32)> {
    let cached_path = {
        let guard = cache.lock().ok()?;
        guard.as_ref().and_then(|service| service.get(url))
    };

    let bytes: Vec<u8> = match cached_path {
        Some(path) => tokio::fs::read(&path).await.ok()?,
        None => {
            let downloaded = reqwest::get(url).await.ok()?.bytes().await.ok()?.to_vec();
            if let Ok(guard) = cache.lock() {
                if let Some(service) = guard.as_ref() {
                    let _ = service.store(url, &downloaded);
                }
            }
            downloaded
        }
    };

    let rgba = image::load_from_memory(&bytes)
        .ok()?
        .thumbnail(decode_size, decode_size)
        .to_rgba8();
    let (width, height) = rgba.dimensions();
    Some((rgba.into_raw(), width, height))
}

/// Average RGB of decoded RGBA pixels, darkened so the result works as a
/// header gradient tint. Mirrors, cheaply, the Tauri header that derives
/// its color from the artwork. Returns a dark fallback for empty input.
pub fn header_tint(pixels: &[u8]) -> (u8, u8, u8) {
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for px in pixels.chunks_exact(4) {
        if px[3] < 16 {
            continue;
        }
        r += px[0] as u64;
        g += px[1] as u64;
        b += px[2] as u64;
        n += 1;
    }
    if n == 0 {
        return (24, 24, 30);
    }
    // 0.5 keeps the tint dark enough for white text to stay readable.
    let darken = |sum: u64| ((sum / n) as f64 * 0.5) as u8;
    (darken(r), darken(g), darken(b))
}

/// Apply decoded pixels to a single card. Runs on the Slint event loop.
fn apply_artwork(
    window: &AppWindow,
    target: ArtworkTarget,
    pixels: &[u8],
    width: u32,
    height: u32,
) {
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let dst = buffer.make_mut_bytes();
    if dst.len() != pixels.len() {
        return;
    }
    dst.copy_from_slice(pixels);
    let image = slint::Image::from_rgba8(buffer);

    let home = window.global::<HomeState>();
    match target {
        ArtworkTarget::Section {
            section_idx,
            album_idx,
        } => {
            let sections = home.get_sections();
            let Some(section) = sections.row_data(section_idx) else {
                return;
            };
            let Some(mut item) = section.albums.row_data(album_idx) else {
                return;
            };
            item.artwork = image;
            section.albums.set_row_data(album_idx, item);
        }
        ArtworkTarget::Popular { idx } => {
            let popular = home.get_popular();
            let Some(mut item) = popular.row_data(idx) else {
                return;
            };
            item.artwork = image;
            popular.set_row_data(idx, item);
        }
    }
}
