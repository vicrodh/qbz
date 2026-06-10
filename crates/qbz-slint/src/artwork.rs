//! Album artwork pipeline.
//!
//! Cover images go through the shared QBZ image cache (`qbz_cache`), the
//! same disk cache the Tauri app uses — covers are never re-downloaded
//! once cached. Fetch and decode run off the UI thread; each decoded
//! cover is applied to its own `AlbumCardItem` row on the Slint event
//! loop, so artwork arriving never resets a list (POC ADR 16 and 18).

use std::sync::{Arc, Mutex};

use qbz_cache::ImageCacheService;
use qbz_models::ArtworkRef;
use slint::{ComponentHandle, Model};
use tokio::sync::Semaphore;

use crate::{AppWindow, ArtistState, HomeState, SearchState};

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
    /// A card in `HomeState.recent[idx]`.
    Recent { idx: usize },
    /// A card in `HomeState.recent-albums[idx]`.
    RecentAlbum { idx: usize },
    /// A row in `SearchState.albums[idx]`.
    SearchAlbum { idx: usize },
    /// A row in `SearchState.tracks[idx]`.
    SearchTrack { idx: usize },
    /// A row in `SearchState.artists[idx]`.
    SearchArtist { idx: usize },
    /// One collage cover slot (0-3) of `SearchState.playlists[idx]`.
    SearchPlaylistCover { idx: usize, slot: usize },
    /// One micro-collage cover slot (0-3) of `SidebarState.entries[idx]`.
    SidebarPlaylistCover { idx: usize, slot: usize },
    /// The most-popular search hero (its kind is read from SearchState).
    SearchMostPopular,
    /// A release card in `ArtistState.release-sections[section_idx]
    /// .albums[album_idx]`.
    ArtistRelease { section_idx: usize, album_idx: usize },
    /// A card in MusicianState.appearances[index].
    MusicianAppearance { index: usize },
    /// A card in LabelState.albums[index] (releases sub-view grid).
    LabelAlbum { index: usize },
    /// A row in LabelState.top-tracks[index] (landing).
    LabelTopTrack { index: usize },
    /// A card in LabelState.releases-section.albums[index] (landing).
    LabelReleaseAlbum { index: usize },
    /// A card in LabelState.critics-section.albums[index] (landing).
    LabelCriticsAlbum { index: usize },
    /// The cover of LabelState.playlists[index] (landing).
    LabelPlaylistCover { index: usize },
    /// A card in LabelState.artists[index] (landing).
    LabelArtist { index: usize },
    /// A card in LabelState.more-labels[index] (landing).
    LabelMoreLabel { index: usize },
    /// A card in LocationViewState.artists[index].
    LocationArtist { index: usize },
    /// A row in FavoritesState.tracks[index].
    FavoriteTrack { index: usize },
    /// A card in FavoritesState.albums[index].
    FavoriteAlbum { index: usize },
    /// A card in DiscoverBrowseState.albums[index].
    DiscoverBrowseAlbum { index: usize },
    /// A card in LocalLibraryState.albums[index] (Local Library grid). `gen`
    /// is the albums generation at fetch time; a stale cover (the model was
    /// replaced by a search/sort/retry) is dropped on apply.
    LocalAlbumCard { index: usize, gen: u64 },
    /// A card in LocalLibraryState.folders[index] (Folders-flat grid).
    LocalFolderCard { index: usize },
    /// A subfolder cover card in LocalLibraryState.folder-detail-subfolders[index]
    /// (Folders-tree detail pane).
    LocalFolderDetailCard { index: usize },
    /// A card in LocalLibraryState.artists-selected-albums[index] (the
    /// Artists tab right pane — the selected artist's albums).
    LocalArtistAlbumCard { index: usize },
    /// A rail-row avatar in LocalLibraryState.artists (Artists tab). Addressed
    /// by its index in the FLAT master; the apply arm resolves index -> name
    /// and routes through the name-keyed dual-setter (grouped sections are
    /// re-derived, so they must be matched by name). `gen` drops a stale paint
    /// after a reload/rescan.
    LocalArtistRowImage { index: usize, gen: u64 },
    /// The cover of the dedicated Local Library album view (LocalAlbumState).
    LocalAlbumViewCover,
    /// A card in FavoritesState.artists[index].
    FavoriteArtist { index: usize },
    /// A card in FavoritesState.labels[index].
    FavoriteLabel { index: usize },
    /// One collage cover slot (0-3) of a favorites playlist card. `following`
    /// picks the sub-tab source model (Following vs Library/favorites).
    FavPlaylistCover { following: bool, index: usize, slot: usize },
    /// An album card in the favorites Artists sidepanel — section `section`
    /// of `FavoritesState.selected-artist-sections`, album `index`.
    FavoriteArtistAlbum { section: usize, index: usize },
    /// A card in ForYouState.release-watch.albums[index].
    ForYouReleaseWatch { index: usize },
    /// A card in ForYouState.recent-albums.albums[index].
    ForYouRecentAlbum { index: usize },
    /// A row in ForYouState.recent-tracks[index].
    ForYouRecentTrack { index: usize },
    /// A tile in ForYouState.top-artists[index].
    ForYouTopArtist { index: usize },
    /// A tile in ForYouState.artists-to-follow[index].
    ForYouToFollow { index: usize },
    /// A tile in ForYouState.radio-stations[index].
    ForYouRadioStation { index: usize },
    /// A card in ForYouState.more-from-library.albums[index].
    ForYouMoreFromLibrary { index: usize },
    /// A card in ForYouState.rediscover.albums[index].
    ForYouRediscover { index: usize },
    /// The Spotlight artist portrait.
    ForYouSpotlightArtist,
    /// A card in ForYouState.spotlight-albums[index].
    ForYouSpotlightAlbum { index: usize },
    /// A row in MixState.tracks[index].
    MixTrack { index: usize },
    /// A row in PlaylistState.tracks[index].
    PlaylistTrack { index: usize },
    /// The PlaylistState header cover.
    PlaylistCover,
    /// One collage cover slot (0-3) of
    /// `PlaylistManagerState.playlists[index]`.
    PmPlaylistCover { index: usize, slot: usize },
    /// One collage cover slot (0-3) of a tree row's playlist
    /// (`PlaylistManagerState.tree[index].playlist`).
    PmTreeCover { index: usize, slot: usize },
    /// One mosaic cover slot (0-8) of a My QBZ Mixtapes-grid card
    /// (`MyQbzState.mixtapes[index]`). Up to 9 slots (3x3 Collections);
    /// mixtapes use only 0-3.
    MyQbzMixtapeCover { index: usize, slot: usize },
    /// One mosaic cover slot (0-8) of a My QBZ Collections-grid card
    /// (`MyQbzState.collections[index]`).
    MyQbzCollectionCover { index: usize, slot: usize },
    /// A row thumbnail in the My QBZ collection-detail item list
    /// (`MyQbzDetailState.items[index]`). Matched by item position on apply so
    /// a later sort/filter keeps the cover (the rendered model is re-derived).
    MyQbzDetailRow { position: i32 },
    /// One hero-mosaic cover slot (0-8) of the My QBZ collection-detail view
    /// (`MyQbzDetailState.cover{N}`).
    MyQbzDetailCover { slot: usize },
}

impl ArtworkTarget {
    /// Pixel size to decode the cover to. List-row thumbnails are tiny
    /// (~40px), so decoding them to the card size (264) would retain
    /// huge buffers in the model — a 2000-row playlist would hold
    /// hundreds of MB. Decode row thumbnails small.
    fn decode_size(&self) -> u32 {
        match self {
            ArtworkTarget::SearchTrack { .. }
            | ArtworkTarget::FavoriteTrack { .. }
            | ArtworkTarget::MixTrack { .. }
            | ArtworkTarget::PlaylistTrack { .. }
            | ArtworkTarget::LocalArtistRowImage { .. } => 96,
            // Sidebar micro-collage tiles render at ~10-20px; decode tiny.
            ArtworkTarget::SidebarPlaylistCover { .. } => 48,
            // Playlist Manager collage tiles render at ~70-140px.
            ArtworkTarget::PmPlaylistCover { .. } | ArtworkTarget::PmTreeCover { .. } => 160,
            // My QBZ mosaic tiles render at ~60-92px (184/2 or 184/3 grid).
            ArtworkTarget::MyQbzMixtapeCover { .. }
            | ArtworkTarget::MyQbzCollectionCover { .. }
            // Hero mosaic tiles render at ~62-93px (186/2 or 186/3 grid).
            | ArtworkTarget::MyQbzDetailCover { .. } => 160,
            // Detail list-row thumbnails render at 36px.
            ArtworkTarget::MyQbzDetailRow { .. } => 96,
            _ => DECODE_SIZE,
        }
    }
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

/// Process-wide handle to the image cache, so controllers that are not
/// threaded the cache explicitly (the playback controller) can still
/// resolve cover art. Set once at startup.
static SHARED_CACHE: std::sync::OnceLock<ImageCache> = std::sync::OnceLock::new();

/// Total-request timeout: without one, a half-up network (DNS blackhole,
/// captive portal) pins a `MAX_CONCURRENT` semaphore permit indefinitely.
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Process-wide async HTTP client for artwork downloads. A single client pools
/// connections / reuses keep-alive across all concurrent artwork jobs
/// (`MAX_CONCURRENT` = 16), instead of `reqwest::get` building a fresh client +
/// connection pool on every cache miss (the fd-churn / deferred EMFILE risk).
/// rustls per the workspace default; if the builder fails, fall back to the
/// default client (equivalent to `Client::new()`), keeping this infallible —
/// matching the silent-fallback style of `open_cache`.
static HTTP: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .unwrap_or_default()
});

/// Publish the image cache for `shared_cache()` consumers. Call once.
pub fn set_shared_cache(cache: ImageCache) {
    let _ = SHARED_CACHE.set(cache);
}

/// The shared image cache, if `set_shared_cache` has run.
pub fn shared_cache() -> Option<ImageCache> {
    SHARED_CACHE.get().cloned()
}

/// Disk-cache lookup for a remote artwork URL: the cached file's path, or
/// `None` on miss / unopened cache. Never touches the network — offline
/// consumers (MPRIS art, artwork save-as) use this instead of downloading.
pub fn cached_path_for(url: &str) -> Option<std::path::PathBuf> {
    let cache = shared_cache()?;
    let guard = cache.lock().ok()?;
    guard.as_ref()?.get(url)
}

/// `file://` form of [`cached_path_for`], for the MPRIS `artUrl` property.
pub fn cached_file_url_for(url: &str) -> Option<String> {
    let path = cached_path_for(url)?;
    ArtworkRef::LocalFile(path.to_string_lossy().into_owned()).to_mpris_url()
}

/// Build a Slint image from decoded RGBA8 pixels. Returns an empty image
/// if the buffer length does not match the dimensions.
pub fn pixels_to_image(pixels: &[u8], width: u32, height: u32) -> slint::Image {
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let dst = buffer.make_mut_bytes();
    if dst.len() != pixels.len() {
        return slint::Image::default();
    }
    dst.copy_from_slice(pixels);
    slint::Image::from_rgba8(buffer)
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
            let decode_size = job.target.decode_size();
            let (pixels, width, height) =
                fetch_and_decode(&job.url, &cache, decode_size).await?;
            let target = job.target;
            let _ = window.upgrade_in_event_loop(move |w| {
                apply_artwork(&w, target, &pixels, width, height);
            });
            Some(())
        });
    }
}

/// Like `spawn_loads`, but each job's `url` is a LOCAL filesystem path
/// (Local Library covers) rather than an HTTP URL. Decodes via the
/// source-aware `ArtworkRef::LocalFile` instead of the HTTP cache path.
pub fn spawn_local_loads(jobs: Vec<ArtworkJob>, window: slint::Weak<AppWindow>, cache: ImageCache) {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
    for job in jobs {
        let semaphore = semaphore.clone();
        let window = window.clone();
        let cache = cache.clone();
        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.ok()?;
            let decode_size = job.target.decode_size();
            let art = ArtworkRef::LocalFile(job.url.clone());
            let (pixels, width, height) = fetch_and_decode_ref(&art, &cache, decode_size).await?;
            let target = job.target;
            let _ = window.upgrade_in_event_loop(move |w| {
                apply_artwork(&w, target, &pixels, width, height);
            });
            Some(())
        });
    }
}

/// Like `spawn_local_loads`, but Plex-aware: each job's `url` is either a
/// LOCAL filesystem path (Local Library covers) OR a raw Plex thumbnail path
/// (`/library/...`, `/photo/...`). When the path looks like a Plex thumb AND
/// Plex creds are present, the job is decoded via `ArtworkRef::PlexThumb`
/// (tokenized HTTP through the disk cache); otherwise it falls back to the
/// local-file path. The path prefix is self-describing, so no per-job source
/// tag is needed (local covers are always absolute filesystem paths, never
/// `/library/` or `/photo/`). Used by the Local Library Albums grid + album
/// detail so Plex albums render covers. Browse-only; the queue/now-playing
/// /MPRIS artwork is unaffected.
pub fn spawn_local_or_plex_loads(
    jobs: Vec<ArtworkJob>,
    plex_base_url: String,
    plex_token: String,
    window: slint::Weak<AppWindow>,
    cache: ImageCache,
) {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let plex_base_url = Arc::new(plex_base_url);
    let plex_token = Arc::new(plex_token);
    for job in jobs {
        let semaphore = semaphore.clone();
        let window = window.clone();
        let cache = cache.clone();
        let plex_base_url = plex_base_url.clone();
        let plex_token = plex_token.clone();
        tokio::spawn(async move {
            let _permit = semaphore.acquire().await.ok()?;
            let decode_size = job.target.decode_size();
            let is_plex_path =
                job.url.starts_with("/library/") || job.url.starts_with("/photo/");
            let art = if is_plex_path && !plex_base_url.is_empty() && !plex_token.is_empty() {
                ArtworkRef::PlexThumb {
                    base_url: (*plex_base_url).clone(),
                    token: (*plex_token).clone(),
                    path: job.url.clone(),
                    // Request a server-side transcode at the surface's decode
                    // size (grid cards 264, row thumbs 96, …) instead of the
                    // full-res original.
                    size: Some(decode_size),
                }
            } else {
                ArtworkRef::LocalFile(job.url.clone())
            };
            let (pixels, width, height) = fetch_and_decode_ref(&art, &cache, decode_size).await?;
            let target = job.target;
            let _ = window.upgrade_in_event_loop(move |w| {
                apply_artwork(&w, target, &pixels, width, height);
            });
            Some(())
        });
    }
}

/// Resolve a remote URL to raw bytes via the shared disk cache: a hit reads
/// from disk, a miss downloads and stores. HTTP(S) only.
async fn fetch_cached_http(url: &str, cache: &ImageCache) -> Option<Vec<u8>> {
    let cached_path = {
        let guard = cache.lock().ok()?;
        guard.as_ref().and_then(|service| service.get(url))
    };

    match cached_path {
        Some(path) => tokio::fs::read(&path).await.ok(),
        // Offline: a miss must not burn a network attempt (or pin a semaphore
        // permit) — fail soft to the placeholder; nothing negative is cached,
        // so the cover retries naturally once back online.
        None if crate::offline_mode::engine().is_offline() => None,
        None => {
            let downloaded = HTTP.get(url).send().await.ok()?.bytes().await.ok()?.to_vec();
            if let Ok(guard) = cache.lock() {
                if let Some(service) = guard.as_ref() {
                    let _ = service.store(url, &downloaded);
                }
            }
            Some(downloaded)
        }
    }
}

/// Decode raw image bytes to RGBA8, downscaled to `decode_size`.
fn decode_rgba(bytes: &[u8], decode_size: u32) -> Option<(Vec<u8>, u32, u32)> {
    let rgba = image::load_from_memory(bytes)
        .ok()?
        .thumbnail(decode_size, decode_size)
        .to_rgba8();
    let (width, height) = rgba.dimensions();
    Some((rgba.into_raw(), width, height))
}

/// Resolve an [`ArtworkRef`] to raw RGBA8 pixels, downscaled to
/// `decode_size`, regardless of origin. This is the source-aware entry
/// point that fixes local/Plex artwork never reaching the UI: HTTP and Plex
/// thumbnails go through the disk cache, local files are read directly, and
/// embedded bytes decode in place. Runs on a worker thread; the result tuple
/// is `Send`.
pub async fn fetch_and_decode_ref(
    art: &ArtworkRef,
    cache: &ImageCache,
    decode_size: u32,
) -> Option<(Vec<u8>, u32, u32)> {
    if art.is_empty() {
        return None;
    }
    let bytes: Vec<u8> = match art {
        ArtworkRef::None => return None,
        ArtworkRef::Embedded(b) => b.clone(),
        ArtworkRef::LocalFile(path) => tokio::fs::read(path).await.ok()?,
        ArtworkRef::Remote(url) => fetch_cached_http(url, cache).await?,
        ArtworkRef::PlexThumb {
            base_url,
            token,
            path,
            size,
        } => {
            // Shared builder: `Some(size)` → server-side transcode (downscaled),
            // `None` → raw full-res. The cache key is this final URL, so each
            // surface's transcode size caches independently.
            let url = qbz_models::plex_thumb_url(base_url, token, path, *size);
            fetch_cached_http(&url, cache).await?
        }
    };
    decode_rgba(&bytes, decode_size)
}

/// Resolve one cover image (by remote URL) to raw RGBA8 pixels. Kept for the
/// many card/row jobs that already hold a URL; source-aware call sites
/// (local library, Plex) use [`fetch_and_decode_ref`].
pub async fn fetch_and_decode(
    url: &str,
    cache: &ImageCache,
    decode_size: u32,
) -> Option<(Vec<u8>, u32, u32)> {
    fetch_and_decode_ref(&ArtworkRef::Remote(url.to_string()), cache, decode_size).await
}

/// Representative color of decoded RGBA pixels for the header gradient.
///
/// A plain average desaturates badly (everything trends grey), so the
/// average is saturation-boosted off its own mean and then normalized to
/// a fixed peak brightness — the result keeps the cover's hue and reads
/// as a clear tinted band against the dark surface. Dark fallback for
/// empty input.
pub fn header_tint(pixels: &[u8]) -> (u8, u8, u8) {
    let (mut r, mut g, mut b, mut n) = (0f64, 0f64, 0f64, 0u64);
    for px in pixels.chunks_exact(4) {
        if px[3] < 16 {
            continue;
        }
        r += px[0] as f64;
        g += px[1] as f64;
        b += px[2] as f64;
        n += 1;
    }
    if n == 0 {
        return (34, 34, 42);
    }
    let nf = n as f64;
    let (mut r, mut g, mut b) = (r / nf, g / nf, b / nf);

    // Saturation boost: push each channel away from the average's mean.
    let mean = (r + g + b) / 3.0;
    let boost = 2.1;
    let saturate = |c: f64| (mean + (c - mean) * boost).clamp(0.0, 255.0);
    r = saturate(r);
    g = saturate(g);
    b = saturate(b);

    // Normalize the brightest channel to a fixed peak so the tint is
    // always clearly visible — bright enough to perceive, dark enough to
    // keep white text readable. Caps the scale so a near-black cover is
    // only modestly lifted.
    let peak = r.max(g).max(b).max(1.0);
    let scale = (138.0 / peak).min(1.7);
    (
        (r * scale) as u8,
        (g * scale) as u8,
        (b * scale) as u8,
    )
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
        ArtworkTarget::Recent { idx } => {
            let recent = home.get_recent();
            let Some(mut item) = recent.row_data(idx) else {
                return;
            };
            item.artwork = image;
            recent.set_row_data(idx, item);
        }
        ArtworkTarget::RecentAlbum { idx } => {
            let albums = home.get_recent_albums();
            let Some(mut item) = albums.row_data(idx) else {
                return;
            };
            item.artwork = image;
            albums.set_row_data(idx, item);
        }
        ArtworkTarget::SearchAlbum { idx } => {
            let model = window.global::<SearchState>().get_albums();
            if let Some(mut item) = model.row_data(idx) {
                item.artwork = image;
                model.set_row_data(idx, item);
            }
        }
        ArtworkTarget::SearchTrack { idx } => {
            let model = window.global::<SearchState>().get_tracks();
            if let Some(mut item) = model.row_data(idx) {
                item.artwork = image;
                model.set_row_data(idx, item);
            }
        }
        ArtworkTarget::SearchArtist { idx } => {
            let model = window.global::<SearchState>().get_artists();
            if let Some(mut item) = model.row_data(idx) {
                item.artwork = image;
                model.set_row_data(idx, item);
            }
        }
        ArtworkTarget::SearchPlaylistCover { idx, slot } => {
            let model = window.global::<SearchState>().get_playlists();
            if let Some(mut item) = model.row_data(idx) {
                match slot {
                    0 => item.cover1 = image,
                    1 => item.cover2 = image,
                    2 => item.cover3 = image,
                    3 => item.cover4 = image,
                    _ => return,
                }
                model.set_row_data(idx, item);
            }
        }
        ArtworkTarget::SidebarPlaylistCover { idx, slot } => {
            let model = window.global::<crate::SidebarState>().get_entries();
            if let Some(mut item) = model.row_data(idx) {
                match slot {
                    0 => item.cover1 = image,
                    1 => item.cover2 = image,
                    2 => item.cover3 = image,
                    3 => item.cover4 = image,
                    _ => return,
                }
                model.set_row_data(idx, item);
            }
        }
        ArtworkTarget::SearchMostPopular => {
            let state = window.global::<SearchState>();
            match state.get_most_popular_kind().as_str() {
                "album" => {
                    let mut it = state.get_most_popular_album();
                    it.artwork = image;
                    state.set_most_popular_album(it);
                }
                "artist" => {
                    let mut it = state.get_most_popular_artist();
                    it.artwork = image;
                    state.set_most_popular_artist(it);
                }
                "track" => {
                    let mut it = state.get_most_popular_track();
                    it.artwork = image;
                    state.set_most_popular_track(it);
                }
                _ => {}
            }
        }
        ArtworkTarget::ArtistRelease {
            section_idx,
            album_idx,
        } => {
            let sections = window.global::<ArtistState>().get_release_sections();
            let Some(section) = sections.row_data(section_idx) else {
                return;
            };
            let Some(mut item) = section.albums.row_data(album_idx) else {
                return;
            };
            item.artwork = image;
            section.albums.set_row_data(album_idx, item);
        }
        ArtworkTarget::MusicianAppearance { index } => {
            let model = window.global::<crate::MusicianState>().get_appearances();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LabelAlbum { index } => {
            let model = window.global::<crate::LabelState>().get_albums();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LabelTopTrack { index } => {
            let model = window.global::<crate::LabelState>().get_top_tracks();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LabelReleaseAlbum { index } => {
            let model = window.global::<crate::LabelState>().get_releases_section().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LabelCriticsAlbum { index } => {
            let model = window.global::<crate::LabelState>().get_critics_section().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LabelPlaylistCover { index } => {
            let model = window.global::<crate::LabelState>().get_playlists();
            if let Some(mut item) = model.row_data(index) {
                item.cover1 = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LabelArtist { index } => {
            let model = window.global::<crate::LabelState>().get_artists();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LabelMoreLabel { index } => {
            let model = window.global::<crate::LabelState>().get_more_labels();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LocationArtist { index } => {
            let model = window.global::<crate::LocationViewState>().get_artists();
            if let Some(mut item) = model.row_data(index) {
                item.image = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::FavoriteTrack { index } => {
            let model = window.global::<crate::FavoritesState>().get_tracks();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image.clone();
                let id = item.id.to_string();
                model.set_row_data(index, item);
                // Also reach the rendered (possibly sorted/grouped) model.
                crate::favorites::set_track_artwork(window, &id, image);
            }
        }
        ArtworkTarget::FavoriteAlbum { index } => {
            let model = window.global::<crate::FavoritesState>().get_albums();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image.clone();
                let id = item.id.to_string();
                model.set_row_data(index, item);
                // Also reach the rendered visible/grouped model (clones when
                // a sort/group is active — they don't share `albums`).
                crate::favorites::set_album_artwork(window, &id, image);
            }
        }
        ArtworkTarget::DiscoverBrowseAlbum { index } => {
            let model = window.global::<crate::DiscoverBrowseState>().get_albums();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LocalAlbumCard { index, gen } => {
            // Drop the cover if a reload superseded the set it belongs to.
            if !crate::local_library::albums_gen_current(gen) {
                return;
            }
            let model = window.global::<crate::LocalLibraryState>().get_albums();
            if let Some(item) = model.row_data(index) {
                // Dual-set by id onto the full set + visible + grouped sections.
                let id = item.id.to_string();
                crate::local_library::set_local_album_artwork(window, &id, image);
            }
        }
        ArtworkTarget::LocalFolderCard { index } => {
            let model = window.global::<crate::LocalLibraryState>().get_folders();
            if let Some(item) = model.row_data(index) {
                // Dual-set by id onto the full set + visible + grouped sections.
                let id = item.id.to_string();
                crate::local_library::set_local_folder_artwork(window, &id, image);
            }
        }
        ArtworkTarget::LocalFolderDetailCard { index } => {
            let model = window
                .global::<crate::LocalLibraryState>()
                .get_folder_detail_subfolders();
            if let Some(item) = model.row_data(index) {
                let path = item.path.to_string();
                crate::local_library::set_folder_detail_subfolder_artwork(window, &path, image);
            }
        }
        ArtworkTarget::LocalArtistAlbumCard { index } => {
            let model = window
                .global::<crate::LocalLibraryState>()
                .get_artists_selected_albums();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LocalArtistRowImage { index, gen } => {
            // Drop a portrait whose artists list was superseded by a reload.
            if crate::local_library::artists_img_gen_current() == gen {
                let s = window.global::<crate::LocalLibraryState>();
                if let Some(item) = s.get_artists().row_data(index) {
                    let name = item.name.to_string();
                    crate::local_library::set_artist_row_image(window, &name, image);
                }
            }
        }
        ArtworkTarget::LocalAlbumViewCover => {
            window.global::<crate::LocalAlbumState>().set_cover(image);
        }
        ArtworkTarget::FavoriteArtist { index } => {
            let model = window.global::<crate::FavoritesState>().get_artists();
            if let Some(mut item) = model.row_data(index) {
                let id = item.id.to_string();
                item.image = image.clone();
                model.set_row_data(index, item);
                // Also reach the rendered (visible + grouped/sidepanel) models.
                crate::favorites::set_artist_image(window, &id, image);
            }
        }
        ArtworkTarget::FavoriteLabel { index } => {
            let model = window.global::<crate::FavoritesState>().get_labels();
            if let Some(mut item) = model.row_data(index) {
                item.image = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::FavPlaylistCover { following, index, slot } => {
            let st = window.global::<crate::FavoritesState>();
            let model = if following {
                st.get_playlists_following()
            } else {
                st.get_playlists_favorites()
            };
            if let Some(mut item) = model.row_data(index) {
                let id = item.id.to_string();
                match slot {
                    0 => item.cover1 = image.clone(),
                    1 => item.cover2 = image.clone(),
                    2 => item.cover3 = image.clone(),
                    _ => item.cover4 = image.clone(),
                }
                model.set_row_data(index, item);
                // Also reach the rendered (possibly search-filtered) model.
                crate::favorites::set_playlist_cover(window, &id, slot, image);
            }
        }
        ArtworkTarget::FavoriteArtistAlbum { section, index } => {
            let sections = window
                .global::<crate::FavoritesState>()
                .get_selected_artist_sections();
            if let Some(sec) = sections.row_data(section) {
                if let Some(mut item) = sec.albums.row_data(index) {
                    item.artwork = image;
                    sec.albums.set_row_data(index, item);
                }
            }
        }
        ArtworkTarget::ForYouReleaseWatch { index } => {
            let model = window.global::<crate::ForYouState>().get_release_watch().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ForYouRecentAlbum { index } => {
            let model = window.global::<crate::ForYouState>().get_recent_albums().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ForYouRecentTrack { index } => {
            let model = window.global::<crate::ForYouState>().get_recent_tracks();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ForYouTopArtist { index } => {
            let model = window.global::<crate::ForYouState>().get_top_artists();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ForYouToFollow { index } => {
            let model = window.global::<crate::ForYouState>().get_artists_to_follow();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ForYouRadioStation { index } => {
            let model = window.global::<crate::ForYouState>().get_radio_stations();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ForYouMoreFromLibrary { index } => {
            let model = window.global::<crate::ForYouState>().get_more_from_library().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ForYouRediscover { index } => {
            let model = window.global::<crate::ForYouState>().get_rediscover().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ForYouSpotlightArtist => {
            window
                .global::<crate::ForYouState>()
                .set_spotlight_image(image);
        }
        ArtworkTarget::ForYouSpotlightAlbum { index } => {
            let model = window.global::<crate::ForYouState>().get_spotlight_albums();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::MixTrack { index } => {
            let model = window.global::<crate::MixState>().get_tracks();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::PlaylistTrack { index } => {
            // Resolve into the stable FULL_ITEMS + the visible row (by
            // id) so sorting/filtering keeps the artwork.
            crate::playlist::set_track_artwork(window, index, image);
        }
        ArtworkTarget::PlaylistCover => {
            window.global::<crate::PlaylistState>().set_cover(image);
        }
        ArtworkTarget::PmPlaylistCover { index, slot } => {
            let model = window.global::<crate::PlaylistManagerState>().get_playlists();
            if let Some(mut item) = model.row_data(index) {
                match slot {
                    0 => item.cover1 = image,
                    1 => item.cover2 = image,
                    2 => item.cover3 = image,
                    3 => item.cover4 = image,
                    _ => return,
                }
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::PmTreeCover { index, slot } => {
            let model = window.global::<crate::PlaylistManagerState>().get_tree();
            if let Some(mut row) = model.row_data(index) {
                match slot {
                    0 => row.playlist.cover1 = image,
                    1 => row.playlist.cover2 = image,
                    2 => row.playlist.cover3 = image,
                    3 => row.playlist.cover4 = image,
                    _ => return,
                }
                model.set_row_data(index, row);
            }
        }
        ArtworkTarget::MyQbzMixtapeCover { index, slot } => {
            let model = window.global::<crate::MyQbzState>().get_mixtapes();
            if let Some(mut item) = model.row_data(index) {
                crate::myqbz::set_mosaic_cover(&mut item, slot, image);
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::MyQbzCollectionCover { index, slot } => {
            let model = window.global::<crate::MyQbzState>().get_collections();
            if let Some(mut item) = model.row_data(index) {
                crate::myqbz::set_mosaic_cover(&mut item, slot, image);
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::MyQbzDetailRow { position } => {
            crate::myqbz_detail::set_row_artwork(window, position, image);
        }
        ArtworkTarget::MyQbzDetailCover { slot } => {
            crate::myqbz_detail::set_hero_cover(window, slot, image);
        }
    }
}
