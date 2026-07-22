//! Album artwork pipeline.
//!
//! Cover images go through the shared QBZ image cache (`qbz_cache`), the
//! same disk cache the Tauri app uses — covers are never re-downloaded
//! once cached. Fetch and decode run off the UI thread; each decoded
//! cover is applied to its own `AlbumCardItem` row on the Slint event
//! loop, so artwork arriving never resets a list (POC ADR 16 and 18).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

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

/// Interface-size preset multiplier for decode targets, set ONCE at startup
/// (main.rs, before any artwork job runs). Under a scaled UI every card gets
/// `preset ×` more physical pixels, so decode sizes must grow with it or
/// covers go soft at Large/XL; Small shrinks them, saving decoded-cache RAM.
static UI_SCALE_FACTOR: std::sync::OnceLock<f32> = std::sync::OnceLock::new();

pub fn set_ui_scale_factor(factor: f32) {
    let _ = UI_SCALE_FACTOR.set(factor);
}

/// Scale a base decode size by the interface-size preset, rounded up.
pub fn scaled_decode(base: u32) -> u32 {
    let factor = UI_SCALE_FACTOR.get().copied().unwrap_or(1.0);
    (base as f32 * factor).ceil() as u32
}

/// Default image-cache size budget (matches the Tauri default).
pub const MAX_CACHE_BYTES: u64 = 200 * 1024 * 1024;

/// Shared, optional image cache. `None` when the cache could not be opened
/// — artwork then falls back to direct downloads.
pub type ImageCache = Arc<Mutex<Option<ImageCacheService>>>;

/// Which card an artwork download targets.
/// (`Clone` only — `LocalAlbumById` carries a `String`, so no `Copy`.)
#[derive(Clone)]
pub enum ArtworkTarget {
    /// A card in a Discover descriptor list's embedded album section
    /// (`DiscoverState.home-sections` / `editor-sections`
    /// `[section_idx].section.albums[album_idx]`) — Slice 5's prefs-driven
    /// Home/Editor render loop. `editor` picks which list the job targets.
    DiscoverSectionAlbum {
        editor: bool,
        section_idx: usize,
        album_idx: usize,
    },
    /// A card in `HomeState.popular[idx]`.
    Popular { idx: usize },
    /// A card in `HomeState.recent[idx]`.
    Recent { idx: usize },
    /// A card in `HomeState.recent-albums[idx]`.
    RecentAlbum { idx: usize },
    /// A card in `RecentAlbumsState.albums[idx]` — the full "Recently Played
    /// Albums" page (the Home rail's "View all"). Own target because the page
    /// model has its own lifecycle, separate from the rail's (same split as
    /// HomeFavoriteAlbum vs ForYouFavoriteAlbum).
    RecentAlbumsPage { idx: usize },
    /// A card in `MostPlayedAlbumsState.albums[idx]` — the "Most Played Albums"
    /// View-all page.
    MostPlayedAlbumsPage { idx: usize },
    /// A card in `HomeState.favorite-albums.albums[idx]` — the Home tab's
    /// "Library Albums" rail (#566). Separate from `ForYouFavoriteAlbum`:
    /// the two rails share the data pipeline but not the model lifecycle.
    HomeFavoriteAlbum { idx: usize },
    /// A card in `HomeState.most-played-albums.albums[idx]` — the Home tab's
    /// "Most Played Albums" rail (local play-count ranking).
    HomeMostPlayedAlbum { idx: usize },
    /// A card in `HomeState.release-watch.albums[idx]` — the Home tab's
    /// "Release Watch" rail (#566; ForYouReleaseWatch's Home twin).
    HomeReleaseWatchAlbum { idx: usize },
    /// A tile in `HomeState.top-artists[idx]` — the Home tab's "Your Top
    /// Artists" rail (#566; ForYouTopArtist's Home twin).
    HomeTopArtist { idx: usize },
    /// A single playlist cover of `HomeState.playlists[idx]` (single cover →
    /// slot 0, unlike the 4-slot SearchPlaylistCover/FavPlaylistCover).
    HomePlaylistCover { idx: usize },
    /// A single playlist cover of `PlaylistBrowseState.playlists[idx]` — the
    /// Qobuz Playlists "View all" page. `visible` shares the same model
    /// while no search is active, so the rendered grid updates too (same
    /// contract as DiscoverBrowseAlbum).
    PlaylistBrowseCover { idx: usize },
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
    /// A cortinilla row addressed by its stable flat index: 0 = the
    /// `SearchState.top-result`, 1.. = the section rows in declaration order
    /// (the same flat-index convention the click/keyboard path uses).
    CortinillaRow { flat_index: usize },
    /// An immersive-search dropdown row in `ImmersiveState.search-sections`,
    /// addressed by its stable flat index (1..). The immersive cortinilla has
    /// NO top result (top = None), so flat-index 0 is never produced; every job
    /// targets a section row. Mirrors `CortinillaRow` but writes the immersive
    /// global instead of `SearchState`.
    ImmersiveSearchRow { flat_index: usize },
    /// A blocked-album row cover in `BlacklistState.album-items[idx]` (the
    /// Blacklist Manager Albums tab).
    BlacklistAlbum { idx: usize },
    /// A release card in `ArtistState.release-sections[section_idx]
    /// .albums[album_idx]`.
    ArtistRelease { section_idx: usize, album_idx: usize },
    /// The single "Novedad más reciente" highlight in `ArtistState.last-release`.
    ArtistLastRelease,
    /// A card in `ArtistReleasesState.albums[index]` (dedicated discography page).
    ArtistReleasesAlbum { index: usize },
    /// A Magazine story thumbnail in `ArtistState.stories[index]`.
    ArtistStory { index: usize },
    /// A row in `ArtistState.top-tracks[index]` (artist "Popular Tracks"
    /// list). Mirrors `LabelTopTrack`: Slint can't fetch network images, so
    /// the row's album-cover thumbnail only paints once this job decodes the
    /// `artwork_url` bytes into the row's `artwork` field (#631).
    ArtistTopTrack { index: usize },
    /// A curated playlist card in `ArtistState.playlists[index]` (the
    /// main-column Playlists carousel). Single cover (slot 0), like
    /// `LabelPlaylistCover`.
    ArtistPlaylistCover { index: usize },
    /// A card in the Library "All" mixed feed (`LibraryAllState.items-visible[index]`).
    /// Dispatched against the VISIBLE model, re-dispatched on each derive.
    LibraryAllCover { index: usize },
    /// A row in `ArtistState.library-tracks[index]` — the ArtistPage
    /// "In library" track list (library_by_artist seed). Row thumbnail.
    ArtistLibraryTrack { index: usize },
    /// A card in `ArtistState.library-albums[index]` — the ArtistPage
    /// "In library" album grid.
    ArtistLibraryAlbum { index: usize },
    /// A card in MusicianState.appearances[index].
    MusicianAppearance { index: usize },
    /// A card in LabelState.albums[index] (releases sub-view grid).
    LabelAlbum { index: usize },
    /// A row in LabelState.top-tracks[index] (landing).
    LabelTopTrack { index: usize },
    /// A card in LabelState.releases-section.albums[index] (landing).
    LabelReleaseAlbum { index: usize },
    /// A card in LabelState.library-albums[index] (landing, "In library" tab).
    LabelLibraryAlbum { index: usize },
    /// A row in LabelState.library-tracks[index] (landing, "In library" tab).
    LabelLibraryTrack { index: usize },
    /// A card in LabelState.critics-section.albums[index] (landing).
    LabelCriticsAlbum { index: usize },
    /// The cover of LabelState.playlists[index] (landing).
    LabelPlaylistCover { index: usize },
    /// A card in LabelState.artists[index] (landing).
    LabelArtist { index: usize },
    /// A card in LabelState.more-labels[index] (landing).
    LabelMoreLabel { index: usize },
    /// A card in AlbumState.more-from-artist.albums[index] (album-view
    /// "From the same artist" carousel).
    AlbumMoreFromArtist { index: usize },
    /// A card in AlbumState.suggestions-section.albums[index] (album-view
    /// "Listening suggestions" carousel).
    AlbumSuggestion { index: usize },
    /// A card in AlbumState.lastfm-suggestions-section.albums[index]
    /// (album-view Last.fm similar-albums carousel, under the suggestions).
    AlbumLastfmSuggestion { index: usize },
    /// A card in AwardState.albums[index] (landing preview grid AND the
    /// full AwardAlbums listing — both source the `albums` model).
    AwardAlbum { index: usize },
    /// A card in AwardState.other-awards[index] (landing carousel).
    AwardOther { index: usize },
    /// A card in LocationViewState.artists[index].
    LocationArtist { index: usize },
    /// A row in FavoritesState.tracks[index].
    FavoriteTrack { index: usize },
    /// A Favorites album cover, addressed BY ID (windowed dispatch over
    /// `albums-visible` — id-keyed delivery is immune to derive re-sorts
    /// between dispatch and apply). `gen` is the favorites-albums generation
    /// at fetch time; a stale cover (the model was replaced by a reload) is
    /// dropped.
    FavoriteAlbumById { id: String, gen: u64 },
    /// A card in DiscoverBrowseState.albums[index].
    DiscoverBrowseAlbum { index: usize },
    /// A Local Library album cover, addressed BY ID (windowed dispatch over
    /// `albums-visible` — id-keyed delivery is immune to derive re-sorts
    /// between dispatch and apply). `gen` is the albums generation at fetch
    /// time; a stale cover (the model was replaced by a reload) is dropped.
    LocalAlbumById { id: String, gen: u64 },
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
    /// 4th-tab "Recommendations" rows (external-reco engine).
    ExtRecoRecArtistCommon { index: usize },
    ExtRecoRecArtistRecent { index: usize },
    ExtRecoTopArtist { index: usize },
    ExtRecoRecAlbum { index: usize },
    ExtRecoFreshAlbum { index: usize },
    ExtRecoDeepAlbum { index: usize },
    ExtRecoTopAlbum { index: usize },
    ExtRecoWeeklyExploration { index: usize },
    ExtRecoWeeklyJams { index: usize },
    /// A card in ForYouState.favorite-albums.albums[index].
    ForYouFavoriteAlbum { index: usize },
    /// A card in ForYouState.most-played-albums.albums[index].
    ForYouMostPlayedAlbum { index: usize },
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
    /// One collage cover slot (0-3) of an immersive Suggestions card
    /// (`SuggestionsState.cards[card_idx].cover{slot}`). Playlist cards use
    /// up to 3 slots (book collage), the radio card up to 4 (diamond collage).
    SuggestionCardCover { card_idx: usize, slot: usize },
    /// A row thumbnail in the immersive Suggestions recommended-tracks list
    /// (`SuggestionsState.tracks[idx]`).
    SuggestionTrackCover { idx: usize },
    /// A row thumbnail in the playlist "Suggested Songs" section
    /// (`PlaylistSuggestionsState.rows[idx]`). 40px row art — decode small.
    PlaylistSuggestionCover { idx: usize },
    /// A cover of `PurchasesState.albums-full[index]` (the stable artwork-target
    /// full set). The apply arm writes here, then dual-sets by id into the
    /// rendered flat + grouped models (which a filter/sort/group re-derives).
    PurchaseAlbum { index: usize },
    /// A thumbnail of `PurchasesState.tracks-full[index]` (artwork-target full
    /// set). Dual-set by id into the rendered flat + grouped track models.
    PurchaseTrack { index: usize },
    /// The 224×224 header cover of the PurchaseDetailView (single image written
    /// to `PurchaseDetailState.artwork`).
    PurchaseDetailCover,
    /// A card in `PinnedState.items[idx]` — the mixed Pinned carousel (Home
    /// and For You share the ONE model). Kinds are mixed; the apply arm reads
    /// the row's `kind` to pick the field to write (album / artist `artwork`
    /// vs playlist `cover1` + dominant colour). Index-keyed, so jobs are only
    /// ever dispatched by `pinned_section::rebuild_pinned` right after it
    /// replaced the model — never from a stale row set.
    PinnedCard { idx: usize },
}

impl ArtworkTarget {
    /// Pixel size to decode the cover to. List-row thumbnails are tiny
    /// (~40px), so decoding them to the card size (264) would retain
    /// huge buffers in the model — a 2000-row playlist would hold
    /// hundreds of MB. Decode row thumbnails small.
    fn decode_size(&self) -> u32 {
        scaled_decode(match self {
            ArtworkTarget::SearchTrack { .. }
            | ArtworkTarget::FavoriteTrack { .. }
            | ArtworkTarget::MixTrack { .. }
            | ArtworkTarget::PlaylistTrack { .. }
            | ArtworkTarget::SuggestionTrackCover { .. }
            | ArtworkTarget::PlaylistSuggestionCover { .. }
            // Label/Artist "Popular Tracks" rows are the same list-row
            // thumbnail as the track targets above (~40px rendered).
            | ArtworkTarget::LabelTopTrack { .. }
            | ArtworkTarget::LabelLibraryTrack { .. }
            | ArtworkTarget::ArtistTopTrack { .. }
            | ArtworkTarget::ArtistLibraryTrack { .. }
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
        })
    }
}

/// An artwork download job: which card, and the image URL.
pub struct ArtworkJob {
    pub target: ArtworkTarget,
    pub url: String,
}

/// Artwork jobs for the mixed Pinned carousel (`PinnedState.items`). One job
/// per row with art, reading the URL from the sub-struct the row's `kind`
/// selects (album / artist `artwork-url`; playlist single-cover `url1` —
/// SearchPlaylistItem has no artwork-url field). Rows without art are
/// skipped. Build ONLY from the freshly-pushed row set (see `PinnedCard`).
pub fn pinned_artwork_jobs(rows: &[crate::PinnedItem]) -> Vec<ArtworkJob> {
    rows.iter()
        .enumerate()
        .filter_map(|(idx, row)| {
            let url = match row.kind.as_str() {
                "album" => row.album.artwork_url.as_str(),
                "artist" => row.artist.artwork_url.as_str(),
                "playlist" => row.playlist.url1.as_str(),
                _ => "",
            };
            (!url.is_empty()).then(|| ArtworkJob {
                target: ArtworkTarget::PinnedCard { idx },
                url: url.to_string(),
            })
        })
        .collect()
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
            let Some((pixels, width, height)) =
                fetch_and_decode(&job.url, &cache, decode_size).await
            else {
                // Failed fetch/decode never reaches apply_artwork — free the
                // windowed-dispatch dedupe slot here so a later band pass can
                // retry this cover instead of skipping it for the session.
                if let ArtworkTarget::FavoriteAlbumById { id, .. } = &job.target {
                    crate::favorites::album_artwork_job_done(id);
                }
                return None;
            };
            let target = job.target;
            let url = job.url;
            let _ = window.upgrade_in_event_loop(move |w| {
                apply_artwork(&w, target, &url, &pixels, width, height);
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
            let url = job.url;
            let _ = window.upgrade_in_event_loop(move |w| {
                apply_artwork(&w, target, &url, &pixels, width, height);
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
            let Some((pixels, width, height)) =
                fetch_and_decode_ref(&art, &cache, decode_size).await
            else {
                // Failed fetch/decode never reaches apply_artwork — free the
                // windowed-dispatch dedupe slot here so a later band pass can
                // retry this cover instead of skipping it for the session.
                if let ArtworkTarget::LocalAlbumById { id, .. } = &job.target {
                    crate::local_library::album_artwork_job_done(id);
                }
                return None;
            };
            let target = job.target;
            let url = job.url;
            let _ = window.upgrade_in_event_loop(move |w| {
                apply_artwork(&w, target, &url, &pixels, width, height);
            });
            Some(())
        });
    }
}

/// Artwork dispatch for the SEARCH cortinilla, whose rows mix three sources in a
/// single payload: Qobuz catalog covers (http(s) URLs), Local Library covers
/// (absolute filesystem paths) and Plex covers (`/library/…` / `/photo/…` thumb
/// paths). Each job is routed by its url's shape — http → the HTTP cache path
/// (gated offline, like Qobuz CDN covers); a Plex thumb → `PlexThumb` (tokenized
/// LAN fetch, NOT gated by induced offline); anything else → `LocalFile`
/// (`fs::read`). Plex creds empty / absent → Plex thumbs fall back to a local
/// read (which fails soft to the placeholder).
pub fn spawn_search_loads(
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
            let is_http =
                job.url.starts_with("http://") || job.url.starts_with("https://");
            let is_plex_path =
                job.url.starts_with("/library/") || job.url.starts_with("/photo/");
            let (pixels, width, height) = if is_http {
                // Qobuz CDN cover (internet) — offline-gated inside fetch_and_decode.
                fetch_and_decode(&job.url, &cache, decode_size).await?
            } else if is_plex_path && !plex_base_url.is_empty() && !plex_token.is_empty() {
                let art = ArtworkRef::PlexThumb {
                    base_url: (*plex_base_url).clone(),
                    token: (*plex_token).clone(),
                    path: job.url.clone(),
                    size: Some(decode_size),
                };
                fetch_and_decode_ref(&art, &cache, decode_size).await?
            } else {
                let art = ArtworkRef::LocalFile(job.url.clone());
                fetch_and_decode_ref(&art, &cache, decode_size).await?
            };
            let target = job.target;
            let url = job.url;
            let _ = window.upgrade_in_event_loop(move |w| {
                apply_artwork(&w, target, &url, &pixels, width, height);
            });
            Some(())
        });
    }
}

/// Resolve a remote URL to raw bytes via the shared disk cache: a hit reads
/// from disk, a miss downloads and stores. HTTP(S) only.
///
/// `gate_offline` controls the miss policy while offline mode is active:
/// `true` for genuinely-INTERNET fetches (Qobuz CDN covers — offline means
/// zero internet traffic, so a miss fails soft to the placeholder), `false`
/// for LAN Plex thumbnails (artwork of LOCAL-library Plex rows: induced
/// offline keeps Plex available by design, and a logged-out/real-offline
/// session does not imply the LAN is gone — a dead-LAN attempt fails fast
/// within `HTTP_TIMEOUT`). Disk hits always serve regardless.
async fn fetch_cached_http(url: &str, cache: &ImageCache, gate_offline: bool) -> Option<Vec<u8>> {
    let cached_path = {
        let guard = cache.lock().ok()?;
        guard.as_ref().and_then(|service| service.get(url))
    };

    match cached_path {
        Some(path) => tokio::fs::read(&path).await.ok(),
        // Offline: an internet miss must not burn a network attempt (or pin a
        // semaphore permit) — fail soft to the placeholder; nothing negative
        // is cached, so the cover retries naturally once back online.
        None if gate_offline && crate::offline_mode::engine().is_offline() => None,
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

/// A decoded cover, ready for `pixels_to_image`. `slint::Image` is `!Send`, so
/// the cache stores the RGBA tuple (which IS `Send`) and the event loop builds
/// the `slint::Image` on demand. `Arc`-wrapped so cache hits clone a pointer,
/// not the ~36KB buffer.
pub type DecodedPixels = (Arc<Vec<u8>>, u32, u32);

/// Decoded-pixel LRU. Repeat decodes of the same `(url, size)` — exactly what
/// the coverflow / queue refresh hammered every click — become a HashMap hit +
/// a cheap pixel upload instead of a full `image::load_from_memory().thumbnail()`.
/// Byte-budgeted (large now-playing decodes run ~1.44MB each as RGBA, so an
/// entry cap alone let a long shuffle session grow unbounded); the entry cap
/// stays as a backstop for many tiny entries. Insertion order approximates LRU
/// (re-insert on hit moves the entry to the back).
const DECODED_CACHE_CAP: usize = 256;

/// Byte budget for the decoded-pixel cache: 48MB, lowered to 24MB on
/// small-RAM machines (< 8GB `MemTotal` per `/proc/meminfo`, read once;
/// non-Linux / unreadable falls back to the default).
static DECODED_CACHE_BUDGET: LazyLock<usize> = LazyLock::new(|| {
    const DEFAULT: usize = 48 * 1024 * 1024;
    const SMALL: usize = 24 * 1024 * 1024;
    let small_ram = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        })
        .map(|kb| kb < 8 * 1024 * 1024)
        .unwrap_or(false);
    if small_ram {
        SMALL
    } else {
        DEFAULT
    }
});

struct DecodedCache {
    /// `(url, size)` -> decoded pixels. Insertion order = eviction order.
    map: HashMap<(String, u32), DecodedPixels>,
    /// Keys in insertion order; the front is the eviction candidate.
    order: Vec<(String, u32)>,
    /// Total pixel bytes held (`w*h*4` per entry), checked against
    /// `DECODED_CACHE_BUDGET` on insert.
    bytes: usize,
}

static DECODED_PIXEL_CACHE: LazyLock<Mutex<DecodedCache>> = LazyLock::new(|| {
    Mutex::new(DecodedCache {
        map: HashMap::new(),
        order: Vec::new(),
        bytes: 0,
    })
});

/// Decoded-pixel cache lookup for `(url, size)`. A hit returns the shared RGBA
/// tuple — callers build the `slint::Image` via [`pixels_to_image`] on the event
/// loop and SKIP the expensive decode entirely.
pub fn decoded_pixels(url: &str, size: u32) -> Option<DecodedPixels> {
    let mut cache = DECODED_PIXEL_CACHE.lock().ok()?;
    let key = (url.to_string(), size);
    let hit = cache.map.get(&key).cloned();
    if hit.is_some() {
        // Move to the back (most-recently-used).
        if let Some(pos) = cache.order.iter().position(|k| k == &key) {
            cache.order.remove(pos);
        }
        cache.order.push(key);
    }
    hit
}

/// Store decoded pixels for `(url, size)`, evicting LRU entries until both
/// the byte budget and the entry-count backstop hold.
fn store_decoded(url: &str, size: u32, pixels: &DecodedPixels) {
    let Ok(mut cache) = DECODED_PIXEL_CACHE.lock() else {
        return;
    };
    let key = (url.to_string(), size);
    let entry_bytes = pixels.0.len();
    match cache.map.insert(key.clone(), pixels.clone()) {
        None => {
            cache.order.push(key);
            cache.bytes += entry_bytes;
        }
        Some(old) => {
            // Refresh recency + swap the byte accounting on an overwrite.
            cache.bytes = cache.bytes.saturating_sub(old.0.len()) + entry_bytes;
            if let Some(pos) = cache.order.iter().position(|k| k == &key) {
                cache.order.remove(pos);
                cache.order.push(key);
            }
        }
    }
    // Never evict the just-inserted entry (it sits at the back; the len > 1
    // guard covers the degenerate single-oversized-entry case).
    while (cache.bytes > *DECODED_CACHE_BUDGET || cache.order.len() > DECODED_CACHE_CAP)
        && cache.order.len() > 1
    {
        let oldest = cache.order.remove(0);
        if let Some(evicted) = cache.map.remove(&oldest) {
            cache.bytes = cache.bytes.saturating_sub(evicted.0.len());
        }
    }
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

    // Decoded-pixel cache key: the stable resolved location for this art at this
    // decode size. A hit returns the already-decoded RGBA tuple and skips both
    // the disk read AND the `image::load_from_memory().thumbnail()` decode — this
    // is what makes a one-position queue/coverflow shift near-free (the 6 covers
    // still on screen reuse their decoded pixels instead of being re-decoded).
    // `Embedded` has no stable URL, so it is never decode-cached.
    let cache_key: Option<String> = match art {
        ArtworkRef::None | ArtworkRef::Embedded(_) => None,
        ArtworkRef::LocalFile(path) => Some(path.clone()),
        ArtworkRef::Remote(url) => Some(url.clone()),
        ArtworkRef::PlexThumb {
            base_url,
            token,
            path,
            size,
        } => Some(qbz_models::plex_thumb_url(base_url, token, path, *size)),
    };
    if let Some(key) = cache_key.as_deref() {
        if let Some((pixels, w, h)) = decoded_pixels(key, decode_size) {
            return Some(((*pixels).clone(), w, h));
        }
    }

    let bytes: Vec<u8> = match art {
        ArtworkRef::None => return None,
        ArtworkRef::Embedded(b) => b.clone(),
        ArtworkRef::LocalFile(path) => tokio::fs::read(path).await.ok()?,
        ArtworkRef::Remote(url) => fetch_cached_http(url, cache, true).await?,
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
            // LAN Plex art for LOCAL-library rows: never offline-gated (see
            // `fetch_cached_http` docs) — the queue/now-playing/album-view
            // covers of Plex rows must keep loading in every offline flavor.
            fetch_cached_http(&url, cache, false).await?
        }
    };
    let (pixels, w, h) = decode_rgba(&bytes, decode_size)?;
    if let Some(key) = cache_key {
        store_decoded(&key, decode_size, &(Arc::new(pixels.clone()), w, h));
    }
    Some((pixels, w, h))
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

/// Decode a local cover file to `decode_size` RGBA pixels, through the
/// decoded-pixel cache (keyed by path). Synchronous and `Send`-safe (no
/// `slint::Image`), so worker threads can pre-decode row covers and the
/// event loop builds the image via [`pixels_to_image`].
pub fn decode_local_pixels(path: &str, decode_size: u32) -> Option<DecodedPixels> {
    if path.is_empty() {
        return None;
    }
    if let Some(hit) = decoded_pixels(path, decode_size) {
        return Some(hit);
    }
    let bytes = std::fs::read(path).ok()?;
    let (pixels, w, h) = decode_rgba(&bytes, decode_size)?;
    let entry: DecodedPixels = (Arc::new(pixels), w, h);
    store_decoded(path, decode_size, &entry);
    Some(entry)
}

/// Bounded replacement for `slint::Image::load_from_path` on synchronous
/// UI-thread call sites: decodes to `decode_size` so only thumbnail-sized
/// pixels are retained for the model row's lifetime (`load_from_path` keeps
/// the full-resolution source in the image buffer). `None` on empty path /
/// missing file / decode failure — callers keep their fallback semantics.
pub fn load_local_cover(path: &str, decode_size: u32) -> Option<slint::Image> {
    decode_local_pixels(path, decode_size).map(|(pixels, w, h)| pixels_to_image(&pixels, w, h))
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
    url: &str,
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
        ArtworkTarget::DiscoverSectionAlbum {
            editor,
            section_idx,
            album_idx,
        } => {
            let state = window.global::<crate::DiscoverState>();
            let sections = if editor {
                state.get_editor_sections()
            } else {
                state.get_home_sections()
            };
            let Some(desc) = sections.row_data(section_idx) else {
                return;
            };
            let Some(mut item) = desc.section.albums.row_data(album_idx) else {
                return;
            };
            item.artwork = image;
            desc.section.albums.set_row_data(album_idx, item);
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
        ArtworkTarget::MostPlayedAlbumsPage { idx } => {
            let albums = window.global::<crate::MostPlayedAlbumsState>().get_albums();
            let Some(mut item) = albums.row_data(idx) else {
                return;
            };
            item.artwork = image;
            albums.set_row_data(idx, item);
        }
        ArtworkTarget::RecentAlbumsPage { idx } => {
            let albums = window.global::<crate::RecentAlbumsState>().get_albums();
            let Some(mut item) = albums.row_data(idx) else {
                return;
            };
            item.artwork = image;
            albums.set_row_data(idx, item);
        }
        ArtworkTarget::HomeFavoriteAlbum { idx } => {
            let section = home.get_favorite_albums();
            let Some(mut item) = section.albums.row_data(idx) else {
                return;
            };
            item.artwork = image;
            section.albums.set_row_data(idx, item);
        }
        ArtworkTarget::HomeMostPlayedAlbum { idx } => {
            let section = home.get_most_played_albums();
            let Some(mut item) = section.albums.row_data(idx) else {
                return;
            };
            item.artwork = image;
            section.albums.set_row_data(idx, item);
        }
        ArtworkTarget::HomeReleaseWatchAlbum { idx } => {
            let section = home.get_release_watch();
            let Some(mut item) = section.albums.row_data(idx) else {
                return;
            };
            item.artwork = image;
            section.albums.set_row_data(idx, item);
        }
        ArtworkTarget::HomeTopArtist { idx } => {
            let model = home.get_top_artists();
            let Some(mut item) = model.row_data(idx) else {
                return;
            };
            item.artwork = image;
            model.set_row_data(idx, item);
        }
        ArtworkTarget::HomePlaylistCover { idx } => {
            let model = home.get_playlists();
            if let Some(mut item) = model.row_data(idx) {
                item.cover1 = image; // single cover → slot 0
                // Single-cover Discover cards letterbox a contain-fit cover with
                // its dominant colour (1:1 with Tauri's PlaylistCardLite). The
                // decoded pixels are in hand here, so compute it once on apply.
                item.dominant_color =
                    crate::immersive::dominant_cover_color(pixels, width, height);
                model.set_row_data(idx, item);
            }
        }
        ArtworkTarget::PlaylistBrowseCover { idx } => {
            let model = window.global::<crate::PlaylistBrowseState>().get_playlists();
            if let Some(mut item) = model.row_data(idx) {
                item.cover1 = image; // single cover → slot 0
                // Same dominant-colour letterbox as HomePlaylistCover — the
                // browse grid renders the same single-cover Discover card.
                item.dominant_color =
                    crate::immersive::dominant_cover_color(pixels, width, height);
                model.set_row_data(idx, item);
            }
        }
        ArtworkTarget::SearchAlbum { idx } => {
            let model = window.global::<SearchState>().get_albums();
            if let Some(mut item) = model.row_data(idx) {
                item.artwork = image;
                model.set_row_data(idx, item);
            }
        }
        ArtworkTarget::BlacklistAlbum { idx } => {
            let model = window.global::<crate::BlacklistState>().get_album_items();
            if let Some(mut item) = model.row_data(idx) {
                item.cover = image;
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
            let state = window.global::<SearchState>();
            let model = state.get_artists();
            let mut artist_id = None;
            if let Some(mut item) = model.row_data(idx) {
                artist_id = Some(item.id.clone());
                item.artwork = image.clone();
                model.set_row_data(idx, item);
            }
            // The All-tab carousel is a SEPARATE model (`artists_carousel`,
            // built as a clone with the hero dup dropped), so the artwork
            // pipeline never reaches it. Mirror the cover into the matching
            // carousel row by id (indices differ when the artist hero drops
            // the first entry) so the carousel cards show their images.
            if let Some(aid) = artist_id {
                let carousel = state.get_artists_carousel();
                for i in 0..carousel.row_count() {
                    if let Some(mut c) = carousel.row_data(i) {
                        if c.id == aid {
                            c.artwork = image.clone();
                            carousel.set_row_data(i, c);
                            break;
                        }
                    }
                }
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
        ArtworkTarget::CortinillaRow { flat_index } => {
            let state = window.global::<SearchState>();
            // Late-arrival guard (URL match): only paint if the row STILL carries
            // the exact URL we loaded. The cortinilla re-renders on every
            // keystroke and REUSES flat indices, so a slow load from a previous
            // query would otherwise paint the wrong cover onto the new row that
            // now occupies the same flat-index — the "momentary wrong cover" the
            // image cache is too slow to avoid. Matching the URL makes a stale
            // load a true no-op.
            if flat_index == 0 {
                let mut top = state.get_top_result();
                if top.artwork_url.as_str() == url {
                    top.artwork = image;
                    state.set_top_result(top);
                }
            } else {
                let sections = state.get_sections();
                'outer: for s in 0..sections.row_count() {
                    if let Some(section) = sections.row_data(s) {
                        let rows = section.rows.clone();
                        for r in 0..rows.row_count() {
                            if let Some(mut row) = rows.row_data(r) {
                                if row.flat_index as usize == flat_index
                                    && row.artwork_url.as_str() == url
                                {
                                    row.artwork = image.clone();
                                    rows.set_row_data(r, row);
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
        }
        ArtworkTarget::ImmersiveSearchRow { flat_index } => {
            // Same late-arrival URL-match guard as `CortinillaRow`: only paint
            // when the row STILL carries the exact URL we loaded. The immersive
            // cortinilla re-renders on every keystroke and REUSES flat indices,
            // so a slow load from a previous query would otherwise paint the
            // wrong cover onto the new row that now occupies the same flat-index.
            // The immersive cortinilla has no top result, so flat-index 0 is
            // never produced — every job is a section row.
            let sections = window.global::<crate::ImmersiveState>().get_search_sections();
            'outer: for s in 0..sections.row_count() {
                if let Some(section) = sections.row_data(s) {
                    let rows = section.rows.clone();
                    for r in 0..rows.row_count() {
                        if let Some(mut row) = rows.row_data(r) {
                            if row.flat_index as usize == flat_index
                                && row.artwork_url.as_str() == url
                            {
                                row.artwork = image.clone();
                                rows.set_row_data(r, row);
                                break 'outer;
                            }
                        }
                    }
                }
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
        ArtworkTarget::ArtistLastRelease => {
            let mut item = window.global::<ArtistState>().get_last_release();
            item.artwork = image;
            window.global::<ArtistState>().set_last_release(item);
        }
        ArtworkTarget::ArtistReleasesAlbum { index } => {
            let model = window.global::<crate::ArtistReleasesState>().get_albums();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ArtistPlaylistCover { index } => {
            let model = window.global::<ArtistState>().get_playlists();
            if let Some(mut item) = model.row_data(index) {
                item.cover1 = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LibraryAllCover { index } => {
            let model = window.global::<crate::LibraryAllState>().get_items_visible();
            if let Some(mut item) = model.row_data(index) {
                item.image = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ArtistLibraryTrack { index } => {
            let model = window.global::<ArtistState>().get_library_tracks();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ArtistLibraryAlbum { index } => {
            let model = window.global::<ArtistState>().get_library_albums();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ArtistStory { index } => {
            let model = window.global::<ArtistState>().get_stories();
            if let Some(mut item) = model.row_data(index) {
                item.image = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ArtistTopTrack { index } => {
            let model = window.global::<ArtistState>().get_top_tracks();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
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
        ArtworkTarget::LabelLibraryAlbum { index } => {
            let model = window.global::<crate::LabelState>().get_library_albums();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LabelLibraryTrack { index } => {
            let model = window.global::<crate::LabelState>().get_library_tracks();
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
        ArtworkTarget::AlbumMoreFromArtist { index } => {
            let model = window.global::<crate::AlbumState>().get_more_from_artist().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::AlbumSuggestion { index } => {
            let model = window.global::<crate::AlbumState>().get_suggestions_section().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::AlbumLastfmSuggestion { index } => {
            let model = window
                .global::<crate::AlbumState>()
                .get_lastfm_suggestions_section()
                .albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::AwardAlbum { index } => {
            let model = window.global::<crate::AwardState>().get_albums();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::AwardOther { index } => {
            let model = window.global::<crate::AwardState>().get_other_awards();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::LocationArtist { index } => {
            let model = window.global::<crate::LocationViewState>().get_artists();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
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
        ArtworkTarget::FavoriteAlbumById { id, gen } => {
            // The job is done either way — free its in-flight slot so the
            // window dispatcher can re-request it after an eviction.
            crate::favorites::album_artwork_job_done(&id);
            // Drop the cover if a reload superseded the set it belongs to.
            if !crate::favorites::albums_gen_current(gen) {
                return;
            }
            // Set by id onto the full set + visible + grouped sections.
            crate::favorites::set_album_artwork(window, &id, image);
        }
        ArtworkTarget::DiscoverBrowseAlbum { index } => {
            let model = window.global::<crate::DiscoverBrowseState>().get_albums();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::PurchaseAlbum { index } => {
            let model = window.global::<crate::PurchasesState>().get_albums_full();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image.clone();
                let id = item.id.to_string();
                model.set_row_data(index, item);
                // Also reach the rendered flat + grouped models (re-derived by a
                // filter/sort/group, so they don't share `albums-full`).
                crate::purchases::set_album_artwork(window, &id, image);
            }
        }
        ArtworkTarget::PurchaseTrack { index } => {
            let model = window.global::<crate::PurchasesState>().get_tracks_full();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image.clone();
                let id = item.id.to_string();
                model.set_row_data(index, item);
                crate::purchases::set_track_artwork(window, &id, image);
            }
        }
        ArtworkTarget::PurchaseDetailCover => {
            window
                .global::<crate::PurchaseDetailState>()
                .set_artwork(image);
        }
        ArtworkTarget::LocalAlbumById { id, gen } => {
            // The job is done either way — free its in-flight slot so the
            // window dispatcher can re-request it after an eviction.
            crate::local_library::album_artwork_job_done(&id);
            // Drop the cover if a reload superseded the set it belongs to.
            if !crate::local_library::albums_gen_current(gen) {
                return;
            }
            // Set by id onto the full set + visible + grouped sections.
            crate::local_library::set_local_album_artwork(window, &id, image);
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
        ArtworkTarget::ExtRecoRecArtistCommon { index } => {
            let model = window.global::<crate::ExternalRecoState>().get_rec_artists_common();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ExtRecoRecArtistRecent { index } => {
            let model = window.global::<crate::ExternalRecoState>().get_rec_artists_recent();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ExtRecoTopArtist { index } => {
            let model = window.global::<crate::ExternalRecoState>().get_top_artists();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ExtRecoRecAlbum { index } => {
            let model = window.global::<crate::ExternalRecoState>().get_rec_albums().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ExtRecoFreshAlbum { index } => {
            let model = window.global::<crate::ExternalRecoState>().get_fresh_releases().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ExtRecoDeepAlbum { index } => {
            let model = window.global::<crate::ExternalRecoState>().get_deep_cut_albums().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ExtRecoTopAlbum { index } => {
            let model = window.global::<crate::ExternalRecoState>().get_top_albums().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ExtRecoWeeklyExploration { index } => {
            let model = window.global::<crate::ExternalRecoState>().get_weekly_exploration();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ExtRecoWeeklyJams { index } => {
            let model = window.global::<crate::ExternalRecoState>().get_weekly_jams();
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ForYouFavoriteAlbum { index } => {
            let model = window.global::<crate::ForYouState>().get_favorite_albums().albums;
            if let Some(mut item) = model.row_data(index) {
                item.artwork = image;
                model.set_row_data(index, item);
            }
        }
        ArtworkTarget::ForYouMostPlayedAlbum { index } => {
            let model = window.global::<crate::ForYouState>().get_most_played_albums().albums;
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
        ArtworkTarget::SuggestionCardCover { card_idx, slot } => {
            let model = window.global::<crate::SuggestionsState>().get_cards();
            if let Some(mut item) = model.row_data(card_idx) {
                match slot {
                    0 => item.cover0 = image,
                    1 => item.cover1 = image,
                    2 => item.cover2 = image,
                    3 => item.cover3 = image,
                    _ => return,
                }
                model.set_row_data(card_idx, item);
            }
        }
        ArtworkTarget::SuggestionTrackCover { idx } => {
            let model = window.global::<crate::SuggestionsState>().get_tracks();
            if let Some(mut item) = model.row_data(idx) {
                item.artwork = image;
                model.set_row_data(idx, item);
            }
        }
        ArtworkTarget::PlaylistSuggestionCover { idx } => {
            let model = window
                .global::<crate::PlaylistSuggestionsState>()
                .get_rows();
            if let Some(mut item) = model.row_data(idx) {
                item.artwork = image;
                model.set_row_data(idx, item);
            }
        }
        ArtworkTarget::PinnedCard { idx } => {
            let model = window.global::<crate::PinnedState>().get_items();
            if let Some(mut item) = model.row_data(idx) {
                match item.kind.as_str() {
                    "album" => item.album.artwork = image,
                    "artist" => item.artist.artwork = image,
                    "playlist" => {
                        item.playlist.cover1 = image; // single cover → slot 0
                        // Same dominant-colour letterbox as HomePlaylistCover —
                        // the pinned card renders the single-cover Discover card.
                        item.playlist.dominant_color =
                            crate::immersive::dominant_cover_color(pixels, width, height);
                    }
                    _ => return,
                }
                model.set_row_data(idx, item);
            }
        }
    }
}
