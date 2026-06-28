//! Discover > For You controller.
//!
//! Loads the personalized For You sections and pushes them into
//! `ForYouState`. Each section reuses an existing card component (album
//! Carousel, SlimCarousel, artist ArtistCarousel).
//!
//! ## Progressive, parallel loading
//!
//! The tab loads once on first open ([`spawn_for_you`]). Rather than
//! awaiting one long sequential chain of API calls and applying every
//! section at the very end (the old behaviour — up to ~9 serialized
//! round-trips before anything painted), the loader now:
//!
//!   1. Paints the local/static sections instantly (recently-played
//!      tracks + albums) before any network call.
//!   2. Fans the independent API calls out into concurrent branches
//!      (release-watch ∥ favorite-artists ∥ favorite-albums ∥
//!      album-suggest), each applying its own section the moment its
//!      data resolves, via `upgrade_in_event_loop`.
//!   3. Latches `ForYouState.loaded = true` ONLY after every branch has
//!      resolved, so the one-shot re-entry guard in `main.rs`
//!      (`ensure_for_you_loaded`) can never strand a partially-loaded
//!      tab.
//!
//! Backed sections: Release Watch (get_release_watch), Recently Played
//! Tracks / Albums (local play-history), Your Top Artists (favorites),
//! Artists to Follow (similar artists seeded from favorites), Rediscover
//! + Radio (favorite albums), More From Your Library (album/suggest),
//! Spotlight (a rotated favorite artist's page).

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::future::join_all;
use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{Album, Artist};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget, ImageCache};
use crate::{AlbumCardItem, AppWindow, DiscoverSection, ForYouState, SlimItem};

const ARTIST_SEEDS: usize = 4;
const SIMILAR_PER_SEED: u32 = 10;
const FOLLOW_MAX: usize = 18;

#[derive(Clone)]
pub struct RadioSeed {
    pub album_id: String,
    pub title: String,
    pub artist: String,
    pub artwork_url: String,
}

pub struct SpotlightData {
    pub artist_id: String,
    pub artist_name: String,
    pub category: String,
    pub image_url: String,
    pub has_top_tracks: bool,
    pub albums: Vec<AlbumCard>,
}

#[derive(Clone)]
pub struct AlbumCard {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub year: String,
    pub quality_tier: String,
    pub quality_label: String,
    pub artwork_url: String,
}

#[derive(Clone)]
pub struct TrackSlim {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub artwork_url: String,
}

#[derive(Clone)]
pub struct ArtistSlim {
    pub id: String,
    pub name: String,
    pub artwork_url: String,
    pub following: bool,
}

fn map_album(album: Album) -> AlbumCard {
    let year = album
        .release_date_original
        .as_deref()
        .and_then(|s| s.get(..4).map(|y| y.to_string()))
        .unwrap_or_default();
    let quality_tier = match album.maximum_bit_depth {
        Some(d) if d >= 24 => "hires",
        Some(_) => "cd",
        None => "",
    }
    .to_string();
    let quality_label = match (album.maximum_bit_depth, album.maximum_sampling_rate) {
        (Some(bd), Some(sr)) => format!("{}-bit / {} kHz", bd, sr),
        _ => String::new(),
    };
    AlbumCard {
        id: album.id,
        title: album.title,
        artist: album.artist.name,
        artist_id: album.artist.id.to_string(),
        year,
        quality_tier,
        quality_label,
        artwork_url: album.image.best().cloned().unwrap_or_default(),
    }
}

fn map_artist(artist: Artist, following: bool) -> ArtistSlim {
    ArtistSlim {
        id: artist.id.to_string(),
        name: artist.name,
        artwork_url: artist
            .image
            .and_then(|img| img.best().cloned())
            .unwrap_or_default(),
        following,
    }
}

// ---------------------------------------------------------------------------
// Per-section fetch helpers (network). Each returns owned, mapped data so the
// orchestrator can fire its apply the moment the call resolves.
// ---------------------------------------------------------------------------

async fn fetch_release_watch<A>(runtime: &Arc<AppRuntime<A>>) -> Vec<AlbumCard>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    match runtime.core().get_release_watch("artists", 18, 0).await {
        Ok(page) => {
            // T8: drop blacklisted flat Albums (primary OR any-artist,
            // featured-aware via album_blacklisted). Tauri release-watch
            // also runs the availability filter and bundles BOTH removals
            // into one `total` decrement — this For You carousel surfaces
            // no count (it's a fixed 18-item rail) and applies no separate
            // availability filter, so we just drop the blacklisted rows.
            let bl = if crate::artist_blacklist::is_enabled() {
                crate::artist_blacklist::ids_snapshot()
            } else {
                Default::default()
            };
            page.items
                .into_iter()
                .filter(|a| !qbz_core::core::album_blacklisted(a, &bl))
                .map(map_album)
                .collect()
        }
        Err(_) => Vec::new(),
    }
}

async fn fetch_fav_artists<A>(runtime: &Arc<AppRuntime<A>>) -> Vec<Artist>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    match runtime.core().get_favorites("artists", 50, 0).await {
        Ok(value) => {
            let items = value
                .get("artists")
                .and_then(|b| b.get("items"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            serde_json::from_value(items).unwrap_or_default()
        }
        Err(_) => Vec::new(),
    }
}

async fn fetch_fav_albums<A>(runtime: &Arc<AppRuntime<A>>) -> Vec<Album>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    match runtime.core().get_favorites("albums", 100, 0).await {
        Ok(value) => {
            let items = value
                .get("albums")
                .and_then(|b| b.get("items"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            serde_json::from_value(items).unwrap_or_default()
        }
        Err(_) => Vec::new(),
    }
}

async fn fetch_suggest<A>(runtime: &Arc<AppRuntime<A>>, album_id: &str) -> Vec<AlbumCard>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    if album_id.is_empty() {
        return Vec::new();
    }
    match runtime.core().get_album_suggest(album_id).await {
        Ok(resp) => {
            // T8: similar-albums (flat Album). Filter-then-truncate: drop
            // blacklisted BEFORE take(18) (Tauri parity — may yield fewer
            // than the limit, no backfill).
            let bl = if crate::artist_blacklist::is_enabled() {
                crate::artist_blacklist::ids_snapshot()
            } else {
                Default::default()
            };
            resp.albums
                .map(|p| p.items)
                .unwrap_or_default()
                .into_iter()
                .filter(|a| !qbz_core::core::album_blacklisted(a, &bl))
                .take(18)
                .map(map_album)
                .collect()
        }
        Err(_) => Vec::new(),
    }
}

/// Artists to Follow — similar artists seeded from up to `ARTIST_SEEDS`
/// favorites, excluding ones already followed.
///
/// The ≤4 seed calls are issued CONCURRENTLY (was a sequential await loop),
/// but the dedup + `FOLLOW_MAX` cap are then re-applied SEQUENTIALLY over the
/// joined results IN SEED ORDER — this preserves the exact membership the old
/// sequential loop produced (same `seen` set seeded with the favorite ids,
/// same first-wins dedup, same early cap), only faster.
async fn fetch_to_follow<A>(
    runtime: &Arc<AppRuntime<A>>,
    fav_artists: &[Artist],
    favorite_ids: &HashSet<u64>,
) -> Vec<ArtistSlim>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let seeds: Vec<u64> = fav_artists.iter().take(ARTIST_SEEDS).map(|a| a.id).collect();
    let futures = seeds.into_iter().map(|id| {
        let runtime = runtime.clone();
        async move { runtime.core().get_similar_artists(id, SIMILAR_PER_SEED, 0).await }
    });
    let results = join_all(futures).await; // Vec<Result<..>> in seed order

    let mut seen: HashSet<u64> = favorite_ids.clone();
    let mut to_follow: Vec<ArtistSlim> = Vec::new();
    'outer: for res in results {
        if let Ok(page) = res {
            for artist in page.items {
                if to_follow.len() >= FOLLOW_MAX {
                    break 'outer;
                }
                // T8: similar-artists surface — drop blacklisted artist ids
                // (is_blacklisted auto-gates on the enabled flag). This is
                // the v2_get_similar_artists equivalent; the carousel has no
                // surfaced total to decrement, so a drop just yields fewer
                // rows. NOT to be confused with the artist-detail page's own
                // similar list (a parity-negative left untouched).
                if crate::artist_blacklist::is_blacklisted(artist.id) {
                    continue;
                }
                if seen.insert(artist.id) {
                    to_follow.push(map_artist(artist, false));
                }
            }
        }
    }
    to_follow
}

async fn load_spotlight<A>(
    runtime: &Arc<AppRuntime<A>>,
    favorites: &[Artist],
) -> Option<SpotlightData>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    if favorites.is_empty() {
        return None;
    }
    // Rotate among the top 5 favorites by wall-clock seconds.
    let pool = favorites.len().min(5);
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as usize % pool)
        .unwrap_or(0);
    let seed = &favorites[idx];

    let page = runtime.core().get_artist_page(seed.id, None).await.ok()?;
    let image_url = page
        .images
        .as_ref()
        .and_then(|i| i.portrait.as_ref())
        .map(|p| {
            format!(
                "https://static.qobuz.com/images/artists/covers/medium/{}.{}",
                p.hash, p.format
            )
        })
        .unwrap_or_default();

    // Up to 6 albums, preferring full albums then live/ep/compilation.
    let mut seen: HashSet<String> = HashSet::new();
    let mut albums: Vec<AlbumCard> = Vec::new();
    for want in ["album", "live", "ep-single", "compilation"] {
        if albums.len() >= 6 {
            break;
        }
        let Some(groups) = page.releases.as_ref() else {
            break;
        };
        let Some(group) = groups.iter().find(|g| g.release_type == want) else {
            continue;
        };
        for rel in &group.items {
            if !seen.insert(rel.id.clone()) {
                continue;
            }
            let year = rel
                .dates
                .as_ref()
                .and_then(|d| d.original.as_deref())
                .and_then(|s| s.get(..4).map(|y| y.to_string()))
                .unwrap_or_default();
            let bd = rel.audio_info.as_ref().and_then(|a| a.maximum_bit_depth);
            let sr = rel.audio_info.as_ref().and_then(|a| a.maximum_sampling_rate);
            albums.push(AlbumCard {
                id: rel.id.clone(),
                title: rel.title.clone(),
                artist: rel
                    .artist
                    .as_ref()
                    .map(|a| a.name.display.clone())
                    .unwrap_or_else(|| page.name.display.clone()),
                artist_id: rel
                    .artist
                    .as_ref()
                    .map(|a| a.id.to_string())
                    .unwrap_or_default(),
                year,
                quality_tier: match bd {
                    Some(d) if d >= 24 => "hires",
                    Some(_) => "cd",
                    None => "",
                }
                .to_string(),
                quality_label: match (bd, sr) {
                    (Some(b), Some(r)) => format!("{}-bit / {} kHz", b, r),
                    _ => String::new(),
                },
                artwork_url: rel
                    .image
                    .as_ref()
                    .and_then(|img| img.best().cloned())
                    .unwrap_or_default(),
            });
            if albums.len() >= 6 {
                break;
            }
        }
    }

    Some(SpotlightData {
        artist_id: seed.id.to_string(),
        artist_name: page.name.display.clone(),
        category: page.artist_category.clone().unwrap_or_default(),
        image_url,
        has_top_tracks: page.top_tracks.as_ref().map(|t| !t.is_empty()).unwrap_or(false),
        albums,
    })
}

// ---------------------------------------------------------------------------
// Pure builders (no network) for the locally-derived sections.
// ---------------------------------------------------------------------------

fn recent_album_cards(list: &[crate::recently::RecentAlbum]) -> Vec<AlbumCard> {
    list.iter()
        .cloned()
        .map(|a| AlbumCard {
            id: a.id,
            title: a.title,
            artist: a.artist,
            artist_id: String::new(),
            year: String::new(),
            quality_tier: a.quality_tier,
            quality_label: a.quality_label,
            artwork_url: a.artwork_url,
        })
        .collect()
}

fn recent_track_slims() -> Vec<TrackSlim> {
    crate::recently::load()
        .into_iter()
        .take(24)
        .map(|t| TrackSlim {
            id: t.id,
            title: t.title,
            subtitle: t.subtitle,
            artwork_url: t.artwork_url,
        })
        .collect()
}

fn top_artist_slims(fav_artists: &[Artist]) -> Vec<ArtistSlim> {
    fav_artists
        .iter()
        .take(18)
        .cloned()
        .map(|a| map_artist(a, true))
        .collect()
}

/// Rediscover — favorite albums the user hasn't returned to lately. Prefers the
/// reco store's "forgotten favorites" (favorited, not played in 30d, from the
/// Tauri-shared events.db) when warm; falls back to "not in the local recents
/// cache" when reco is cold so the row never empties.
fn build_rediscover(
    fav_albums: &[Album],
    recent_ids: &HashSet<String>,
    forgotten: Option<&HashSet<String>>,
) -> Vec<AlbumCard> {
    fav_albums
        .iter()
        .filter(|a| match forgotten {
            Some(set) => set.contains(&a.id),
            None => !recent_ids.contains(&a.id),
        })
        .take(18)
        .cloned()
        .map(map_album)
        .collect()
}

/// Favorite Albums — the user's favorited albums, capped at 18, in favorite
/// order (matches Tauri's home-resolved favoriteAlbums sliced 18; unfiltered,
/// unlike Rediscover).
/// Reorder resolved albums so the reco-scored favorites lead (taste order),
/// keeping unscored albums in their original relative order. `scored` is the
/// reco favorite-album id order; `None`/empty leaves the input untouched, so a
/// cold reco store never reorders (no regression).
fn order_by_score(mut albums: Vec<Album>, scored: Option<&[String]>) -> Vec<Album> {
    if let Some(order) = scored {
        if !order.is_empty() {
            albums.sort_by_key(|a| order.iter().position(|id| id == &a.id).unwrap_or(usize::MAX));
        }
    }
    albums
}

fn build_favorite_albums(fav_albums: &[Album]) -> Vec<AlbumCard> {
    fav_albums.iter().take(18).cloned().map(map_album).collect()
}

/// Radio Stations — album-seeded tiles from recent + favorite albums,
/// deduped, capped at 12.
fn build_radio(
    recent_album_list: &[crate::recently::RecentAlbum],
    fav_albums: &[Album],
) -> Vec<RadioSeed> {
    let mut radio_seen: HashSet<String> = HashSet::new();
    let mut radio_stations: Vec<RadioSeed> = Vec::new();
    for a in recent_album_list {
        // Radio Stations seed a Qobuz album radio (/radio/album), so only
        // Qobuz-sourced albums are eligible. Locally-played / Plex albums carry
        // ids the Qobuz radio endpoint can't resolve, so they must NOT appear
        // here (they still show in "Recently Played Albums"). Empty source =
        // legacy pre-source entry, treated as Qobuz.
        if !(a.source.is_empty() || a.source.eq_ignore_ascii_case("qobuz")) {
            continue;
        }
        if radio_seen.insert(a.id.clone()) {
            radio_stations.push(RadioSeed {
                album_id: a.id.clone(),
                title: a.title.clone(),
                artist: a.artist.clone(),
                artwork_url: a.artwork_url.clone(),
            });
        }
    }
    for a in fav_albums {
        if radio_stations.len() >= 12 {
            break;
        }
        if radio_seen.insert(a.id.clone()) {
            radio_stations.push(RadioSeed {
                album_id: a.id.clone(),
                title: a.title.clone(),
                artist: a.artist.name.clone(),
                artwork_url: a.image.best().cloned().unwrap_or_default(),
            });
        }
    }
    radio_stations.truncate(12);
    radio_stations
}

// ---------------------------------------------------------------------------
// Slint model mappers.
// ---------------------------------------------------------------------------

fn album_items(cards: &[AlbumCard]) -> Vec<AlbumCardItem> {
    cards
        .iter()
        .map(|c| AlbumCardItem {
            id: c.id.clone().into(),
            title: c.title.clone().into(),
            artist: c.artist.clone().into(),
            artist_id: c.artist_id.clone().into(),
            genre: "".into(),
            year: c.year.clone().into(),
            quality_tier: c.quality_tier.clone().into(),
            quality_label: c.quality_label.clone().into(),
            ribbon: "".into(),
            ribbon_kind: "".into(),
            artwork_url: c.artwork_url.clone().into(),
            artwork: slint::Image::default(),
            ..Default::default()
        })
        .collect()
}

fn artist_items(artists: &[ArtistSlim]) -> Vec<SlimItem> {
    artists
        .iter()
        .map(|a| SlimItem {
            id: a.id.clone().into(),
            title: a.name.clone().into(),
            subtitle: "".into(),
            rank: "".into(),
            artwork_url: a.artwork_url.clone().into(),
            artwork: slint::Image::default(),
            following: a.following,
        })
        .collect()
}

fn section(title: &str, cards: &[AlbumCard]) -> DiscoverSection {
    DiscoverSection {
        title: title.into(),
        // For You sections have no Discover full-list page.
        endpoint: "".into(),
        albums: ModelRc::new(VecModel::from(album_items(cards))),
    }
}

// ---------------------------------------------------------------------------
// Per-section artwork job builders.
// ---------------------------------------------------------------------------

fn album_jobs(cards: &[AlbumCard], target: impl Fn(usize) -> ArtworkTarget) -> Vec<ArtworkJob> {
    cards
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.artwork_url.is_empty())
        .map(|(i, c)| ArtworkJob {
            url: c.artwork_url.clone(),
            target: target(i),
        })
        .collect()
}

fn artist_jobs(
    artists: &[ArtistSlim],
    target: impl Fn(usize) -> ArtworkTarget,
) -> Vec<ArtworkJob> {
    artists
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.artwork_url.is_empty())
        .map(|(i, a)| ArtworkJob {
            url: a.artwork_url.clone(),
            target: target(i),
        })
        .collect()
}

fn track_jobs(tracks: &[TrackSlim]) -> Vec<ArtworkJob> {
    tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.artwork_url.is_empty())
        .map(|(i, t)| ArtworkJob {
            url: t.artwork_url.clone(),
            target: ArtworkTarget::ForYouRecentTrack { index: i },
        })
        .collect()
}

fn radio_jobs(seeds: &[RadioSeed]) -> Vec<ArtworkJob> {
    seeds
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.artwork_url.is_empty())
        .map(|(i, r)| ArtworkJob {
            url: r.artwork_url.clone(),
            target: ArtworkTarget::ForYouRadioStation { index: i },
        })
        .collect()
}

fn spotlight_jobs(sp: &SpotlightData) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    if !sp.image_url.is_empty() {
        jobs.push(ArtworkJob {
            url: sp.image_url.clone(),
            target: ArtworkTarget::ForYouSpotlightArtist,
        });
    }
    for (i, c) in sp.albums.iter().enumerate() {
        if !c.artwork_url.is_empty() {
            jobs.push(ArtworkJob {
                url: c.artwork_url.clone(),
                target: ArtworkTarget::ForYouSpotlightAlbum { index: i },
            });
        }
    }
    jobs
}

// ---------------------------------------------------------------------------
// Per-section apply helpers. Each pushes its model on the UI thread, then
// fires its artwork jobs (async, per-row). NONE of them touches the
// `loaded` flag — that is latched once at the end of `spawn_for_you`.
// ---------------------------------------------------------------------------

fn apply_recent(
    weak: &slint::Weak<AppWindow>,
    cache: &ImageCache,
    albums: Vec<AlbumCard>,
    tracks: Vec<TrackSlim>,
) {
    let mut jobs = album_jobs(&albums, |i| ArtworkTarget::ForYouRecentAlbum { index: i });
    jobs.extend(track_jobs(&tracks));
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        let state = w.global::<ForYouState>();
        state.set_recent_albums(section(&qbz_i18n::t("Recently Played Albums"), &albums));
        let slim: Vec<SlimItem> = tracks
            .iter()
            .map(|t| SlimItem {
                id: t.id.clone().into(),
                title: t.title.clone().into(),
                subtitle: t.subtitle.clone().into(),
                rank: "".into(),
                artwork_url: t.artwork_url.clone().into(),
                artwork: slint::Image::default(),
                following: false,
            })
            .collect();
        state.set_recent_tracks(ModelRc::new(VecModel::from(slim)));
    });
    // Recently-played albums/tracks mix sources (Qobuz / Plex / local), so the
    // artwork must be routed by scheme: http -> Qobuz CDN, /library/ or /photo/
    // -> Plex thumb (tokenized LAN fetch), else a local file read. The plain
    // HTTP loader (spawn_loads) left Plex and local covers blank.
    let plex = crate::plex_settings::get();
    crate::artwork::spawn_search_loads(jobs, plex.base_url, plex.token, weak.clone(), cache.clone());
}

fn apply_release_watch(weak: &slint::Weak<AppWindow>, cache: &ImageCache, cards: Vec<AlbumCard>) {
    let jobs = album_jobs(&cards, |i| ArtworkTarget::ForYouReleaseWatch { index: i });
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        w.global::<ForYouState>()
            .set_release_watch(section(&qbz_i18n::t("Release Watch"), &cards));
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}

fn apply_top_artists(weak: &slint::Weak<AppWindow>, cache: &ImageCache, artists: Vec<ArtistSlim>) {
    let jobs = artist_jobs(&artists, |i| ArtworkTarget::ForYouTopArtist { index: i });
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        w.global::<ForYouState>()
            .set_top_artists(ModelRc::new(VecModel::from(artist_items(&artists))));
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}

fn apply_to_follow(weak: &slint::Weak<AppWindow>, cache: &ImageCache, artists: Vec<ArtistSlim>) {
    let jobs = artist_jobs(&artists, |i| ArtworkTarget::ForYouToFollow { index: i });
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        w.global::<ForYouState>()
            .set_artists_to_follow(ModelRc::new(VecModel::from(artist_items(&artists))));
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}

fn apply_rediscover(weak: &slint::Weak<AppWindow>, cache: &ImageCache, cards: Vec<AlbumCard>) {
    let jobs = album_jobs(&cards, |i| ArtworkTarget::ForYouRediscover { index: i });
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        w.global::<ForYouState>()
            .set_rediscover(section(&qbz_i18n::t("Rediscover Your Library"), &cards));
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}

fn apply_favorite_albums(weak: &slint::Weak<AppWindow>, cache: &ImageCache, cards: Vec<AlbumCard>) {
    let jobs = album_jobs(&cards, |i| ArtworkTarget::ForYouFavoriteAlbum { index: i });
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        w.global::<ForYouState>()
            .set_favorite_albums(section(&qbz_i18n::t("Favorite Albums"), &cards));
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}

fn apply_more_from_library(
    weak: &slint::Weak<AppWindow>,
    cache: &ImageCache,
    cards: Vec<AlbumCard>,
    seed_title: String,
) {
    // 1:1 with Tauri's `discovery.similarTo` = "Similar to {seed}", where the
    // seed is the album the suggestions are seeded from. Falls back to the
    // plain "More From Your Library" when there is no seed (never titleless).
    let title = if seed_title.is_empty() {
        qbz_i18n::t("More From Your Library")
    } else {
        qbz_i18n::t_args("Similar to {}", &[&seed_title])
    };
    let jobs = album_jobs(&cards, |i| ArtworkTarget::ForYouMoreFromLibrary { index: i });
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        w.global::<ForYouState>()
            .set_more_from_library(section(&title, &cards));
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}

fn apply_radio(weak: &slint::Weak<AppWindow>, cache: &ImageCache, seeds: Vec<RadioSeed>) {
    let jobs = radio_jobs(&seeds);
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        let radio: Vec<crate::RadioStationItem> = seeds
            .iter()
            .map(|r| crate::RadioStationItem {
                album_id: r.album_id.clone().into(),
                title: r.title.clone().into(),
                artist: r.artist.clone().into(),
                artwork_url: r.artwork_url.clone().into(),
                artwork: slint::Image::default(),
            })
            .collect();
        w.global::<ForYouState>()
            .set_radio_stations(ModelRc::new(VecModel::from(radio)));
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}

fn apply_spotlight(weak: &slint::Weak<AppWindow>, cache: &ImageCache, sp: Option<SpotlightData>) {
    let jobs = sp.as_ref().map(spotlight_jobs).unwrap_or_default();
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        let state = w.global::<ForYouState>();
        if let Some(sp) = &sp {
            state.set_spotlight_visible(true);
            state.set_spotlight_artist_id(sp.artist_id.clone().into());
            state.set_spotlight_name(sp.artist_name.clone().into());
            state.set_spotlight_category(sp.category.clone().into());
            state.set_spotlight_image_url(sp.image_url.clone().into());
            state.set_spotlight_has_top_tracks(sp.has_top_tracks);
            state.set_spotlight_albums(ModelRc::new(VecModel::from(album_items(&sp.albums))));
        } else {
            state.set_spotlight_visible(false);
        }
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}

// ---------------------------------------------------------------------------
// Orchestrator.
// ---------------------------------------------------------------------------

/// Set the loading flag so the skeleton shows until the first sections paint.
pub fn reset_loading(window: &AppWindow) {
    window.global::<ForYouState>().set_loading(true);
}

/// Load every For You section progressively and in parallel, then latch
/// `loaded`. Spawned once by `ensure_for_you_loaded` on first tab open.
///
/// Dependency layers:
///   - Layer 0 (instant, no network): Recently Played albums + tracks.
///   - Layer 0 (concurrent network): release-watch, favorite-artists,
///     favorite-albums, and album-suggest (common case, seeded from the most
///     recent local album).
///   - Layer 1 (after favorite-artists): Your Top Artists (immediate) then
///     Artists to Follow ∥ Spotlight.
///   - Layer 1 (after favorite-albums): Rediscover + Radio (and the
///     album-suggest fallback when there is no recent play-history seed).
///   - Latch: `loading = false` + `loaded = true` once ALL branches resolve.
pub fn spawn_for_you<A>(
    runtime: Arc<AppRuntime<A>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: ImageCache,
) where
    A: FrontendAdapter + Send + Sync + 'static,
{
    handle.spawn(async move {
        // ---- Layer 0: instant local/static sections (no await) ----
        let recent_album_list = crate::recently::load_albums();
        let recent_ids: HashSet<String> =
            recent_album_list.iter().map(|a| a.id.clone()).collect();
        let recents_seed: Option<String> = recent_album_list
            .first()
            .map(|a| a.id.clone())
            .filter(|s| !s.is_empty());
        // Seed title for the "Similar to {seed}" header — the most-recent
        // album's title (its suggestions seed the common-case row).
        let recents_seed_title: Option<String> =
            recent_album_list.first().map(|a| a.title.clone());
        let has_recents_seed = recents_seed.is_some();

        apply_recent(
            &weak,
            &image_cache,
            recent_album_cards(&recent_album_list),
            recent_track_slims(),
        );

        // ---- Branch: Release Watch (independent) ----
        let release_branch: Pin<Box<dyn Future<Output = ()> + Send>> = {
            let runtime = runtime.clone();
            let weak = weak.clone();
            let cache = image_cache.clone();
            Box::pin(async move {
                let cards = fetch_release_watch(&runtime).await;
                apply_release_watch(&weak, &cache, cards);
            })
        };

        // ---- Branch: favorite artists -> Top Artists, then To-Follow ∥ Spotlight ----
        let artists_branch: Pin<Box<dyn Future<Output = ()> + Send>> = {
            let runtime = runtime.clone();
            let weak = weak.clone();
            let cache = image_cache.clone();
            Box::pin(async move {
                let fav_artists = fetch_fav_artists(&runtime).await;
                apply_top_artists(&weak, &cache, top_artist_slims(&fav_artists));

                let favorite_ids: HashSet<u64> = fav_artists.iter().map(|a| a.id).collect();

                let follow_branch: Pin<Box<dyn Future<Output = ()> + Send>> = {
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let cache = cache.clone();
                    let fav_artists = fav_artists.clone();
                    let favorite_ids = favorite_ids.clone();
                    Box::pin(async move {
                        let to_follow =
                            fetch_to_follow(&runtime, &fav_artists, &favorite_ids).await;
                        apply_to_follow(&weak, &cache, to_follow);
                    })
                };
                let spotlight_branch: Pin<Box<dyn Future<Output = ()> + Send>> = {
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let cache = cache.clone();
                    let fav_artists = fav_artists.clone();
                    Box::pin(async move {
                        let sp = load_spotlight(&runtime, &fav_artists).await;
                        apply_spotlight(&weak, &cache, sp);
                    })
                };
                join_all(vec![follow_branch, spotlight_branch]).await;
            })
        };

        // ---- Branch: favorite albums -> Rediscover + Radio (+ suggest fallback) ----
        let albums_branch: Pin<Box<dyn Future<Output = ()> + Send>> = {
            let runtime = runtime.clone();
            let weak = weak.clone();
            let cache = image_cache.clone();
            let recent_album_list = recent_album_list.clone();
            let recent_ids = recent_ids.clone();
            Box::pin(async move {
                let fav_albums = fetch_fav_albums(&runtime).await;
                // reco: lead with the highest-scored favorites (trained taste
                // order) when the store is warm; cold -> original Qobuz order.
                let scored_fav = crate::reco::scored_favorite_album_ids(80);
                let fav_albums = order_by_score(fav_albums, scored_fav.as_deref());
                apply_favorite_albums(&weak, &cache, build_favorite_albums(&fav_albums));
                // reco: backfill genres for the resolved favorite albums so the
                // engine's top-genres has data (plays alone carry no genre).
                let genre_entries: Vec<(String, u64, String)> = fav_albums
                    .iter()
                    .filter_map(|a| {
                        a.genre
                            .as_ref()
                            .filter(|g| g.id > 0)
                            .map(|g| (a.id.clone(), g.id, g.name.clone()))
                    })
                    .collect();
                if !genre_entries.is_empty() {
                    tokio::task::spawn_blocking(move || {
                        crate::reco::backfill_album_genres(genre_entries)
                    });
                }
                // reco: prefer the reco "forgotten favorites" set when the store
                // is warm (shared events.db); fall back to the local recents
                // heuristic when cold so the Rediscover row never empties.
                let forgotten: Option<HashSet<String>> =
                    crate::reco::forgotten_favorite_album_ids(60, 30)
                        .filter(|ids| !ids.is_empty())
                        .map(|ids| ids.into_iter().collect());
                apply_rediscover(
                    &weak,
                    &cache,
                    build_rediscover(&fav_albums, &recent_ids, forgotten.as_ref()),
                );
                apply_radio(&weak, &cache, build_radio(&recent_album_list, &fav_albums));

                // Only the no-recent-history case needs the favorite-album seed;
                // the common case is handled concurrently in `suggest_branch`.
                if !has_recents_seed {
                    if let Some(id) = fav_albums
                        .first()
                        .map(|a| a.id.clone())
                        .filter(|s| !s.is_empty())
                    {
                        let seed_title = fav_albums
                            .first()
                            .map(|a| a.title.clone())
                            .unwrap_or_default();
                        let cards = fetch_suggest(&runtime, &id).await;
                        apply_more_from_library(&weak, &cache, cards, seed_title);
                    }
                }
            })
        };

        // ---- Branch: More From Your Library (common case — recent-album seed) ----
        let suggest_branch: Pin<Box<dyn Future<Output = ()> + Send>> = {
            let runtime = runtime.clone();
            let weak = weak.clone();
            let cache = image_cache.clone();
            Box::pin(async move {
                if let Some(id) = recents_seed {
                    let cards = fetch_suggest(&runtime, &id).await;
                    apply_more_from_library(
                        &weak,
                        &cache,
                        cards,
                        recents_seed_title.unwrap_or_default(),
                    );
                }
            })
        };

        join_all(vec![release_branch, artists_branch, albums_branch, suggest_branch]).await;

        // ---- All branches resolved: latch loaded so re-entry is a no-op ----
        let _ = weak.upgrade_in_event_loop(|w| {
            let state = w.global::<ForYouState>();
            state.set_loading(false);
            state.set_loaded(true);
        });
    });
}
