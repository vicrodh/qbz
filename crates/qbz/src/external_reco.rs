//! Discover > Recommendations (the 4th tab) controller.
//!
//! Wires the `qbz-external-reco` engine to Slint: a RecoCatalog over QbzCore,
//! the per-user resolution-cache lifecycle, the scrobbler-username gate, and a
//! PROGRESSIVE apply — each row paints the moment its builder resolves (the For
//! You branch pattern), so the tab fills in incrementally instead of all at once.
//!
//! Lineup: Recommended Artists + Recommended Albums (Last.fm), Fresh Releases +
//! Weekly Exploration/Jams (ListenBrainz), Deep-cut albums, and a Qobuz editorial
//! cold-start fallback.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use qbz_app::shell::AppRuntime;
use qbz_external_reco::{
    build_deep_cut_albums, build_editorial, build_fresh_releases, build_rec_albums,
    build_rec_artists_common, build_rec_artists_recent, build_similar_albums_seeded,
    build_weekly_exploration, build_weekly_jams, gather_history, is_cold_start, AlbumReco,
    ArtistReco, ExternalCarousels, LastFmHandle, ListenBrainzHandle, LocalHistory, RecoCache,
    RecoCatalog, RecoInputs, TrackReco,
};
use qbz_integrations::{LastFmClient, ListenBrainzClient, MusicBrainzClient};
use qbz_models::{Album, Artist, Track};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::artwork::{ArtworkJob, ArtworkTarget, ImageCache};
use crate::{AlbumCardItem, AppWindow, DiscoverSection, ExternalRecoState, SlimItem};

static CACHE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init_for_user(base_dir: &Path) {
    if let Ok(mut g) = CACHE_DIR.lock() {
        *g = Some(base_dir.to_path_buf());
    }
    match RecoCache::open_at(base_dir) {
        Ok(cache) => {
            let _ = cache.cleanup_expired();
            log::info!("[reco] cache initialized at {}", base_dir.display());
        }
        Err(e) => log::warn!("[reco] cache open failed at {}: {e}", base_dir.display()),
    }
}

#[allow(dead_code)]
pub fn teardown() {
    if let Ok(mut g) = CACHE_DIR.lock() {
        *g = None;
    }
}

// ── RecoCatalog over QbzCore (errors -> empty) ──────────────────────────────

struct CoreRecoCatalog {
    runtime: Arc<AppRuntime<SlintAdapter>>,
}

#[async_trait]
impl RecoCatalog for CoreRecoCatalog {
    async fn search_tracks(&self, query: &str, limit: usize) -> Vec<Track> {
        self.runtime
            .core()
            .search_tracks(query, limit as u32, 0, None)
            .await
            .map(|p| p.items)
            .unwrap_or_default()
    }
    async fn search_artists(&self, query: &str, limit: usize) -> Vec<Artist> {
        self.runtime
            .core()
            .search_artists(query, limit as u32, 0, None)
            .await
            .map(|p| p.items)
            .unwrap_or_default()
    }
    async fn search_albums(&self, query: &str, limit: usize) -> Vec<Album> {
        self.runtime
            .core()
            .search_albums(query, limit as u32, 0, None)
            .await
            .map(|p| p.items)
            .unwrap_or_default()
    }
    async fn artist_top_tracks(&self, artist_id: u64, limit: usize) -> Vec<Track> {
        self.runtime
            .core()
            .get_artist_tracks(artist_id, limit as u32, 0)
            .await
            .map(|c| c.items)
            .unwrap_or_default()
    }
    async fn artist_albums(&self, artist_id: u64, limit: usize) -> Vec<Album> {
        self.runtime
            .core()
            .get_artist_albums(artist_id, Some(limit as u32), Some(0))
            .await
            .map(|a| a.items)
            .unwrap_or_default()
    }
    async fn featured_albums(&self, kind: &str, limit: usize) -> Vec<Album> {
        self.runtime
            .core()
            .get_featured_albums(kind, limit as u32, 0, None)
            .await
            .map(|p| p.items)
            .unwrap_or_default()
    }
    async fn get_artist(&self, artist_id: u64) -> Option<Artist> {
        self.runtime.core().get_artist(artist_id).await.ok()
    }
}

// ── Loader ──────────────────────────────────────────────────────────────────

pub fn ensure_loaded(
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    weak: &slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: &ImageCache,
) {
    let Some(w) = weak.upgrade() else {
        return;
    };
    if w.global::<ExternalRecoState>().get_loaded() {
        return;
    }
    w.global::<ExternalRecoState>().set_loading(true);
    spawn(runtime.clone(), weak.clone(), handle, image_cache.clone(), false);
}

/// Force a full rebuild of the Recommendations tab, bypassing the instant
/// results-cache paint (the "Refresh now" action). Resets the loaded/loading
/// latches and runs `spawn` with `force = true`, which skips the cache-read
/// early-return so every row is rebuilt and the results blob is overwritten.
pub fn force_reload(
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    weak: &slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: &ImageCache,
) {
    let Some(w) = weak.upgrade() else {
        return;
    };
    let s = w.global::<ExternalRecoState>();
    s.set_loaded(false);
    s.set_loading(true);
    spawn(runtime.clone(), weak.clone(), handle, image_cache.clone(), true);
}

/// 30-day TTL for the album-page Last.fm row's resolved result. Similar-artist
/// data is stable; a long window keeps Last.fm/Qobuz traffic near-zero on repeat
/// opens, while still refreshing often enough that an emerging artist's growing
/// similar set is picked up within a month.
const LASTFM_SIMILAR_TTL_SECS: i64 = 30 * 86_400;

/// Album page: build the Last.fm "similar albums" row for `seed_artist`
/// (the open album's primary artist), excluding albums already shown by the
/// Qobuz `/album/suggest` row (`exclude_pairs`/`exclude_ids`). Returns empty
/// when Last.fm is not connected. Reuses the same catalog, resolution cache,
/// and rotation as the Discover Recommendations tab.
///
/// The resolved result is cached per `album_id` for 30 days (the same
/// `RecoCache` results store the Discover tab uses) so re-opening an album
/// makes ZERO Last.fm/Qobuz calls. Only non-empty results are cached, so a
/// transient Last.fm failure (empty result) re-fetches on the next open instead
/// of hiding the row for the whole window.
pub async fn load_similar_albums_seeded(
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    album_id: &str,
    seed_artist: &str,
    exclude_pairs: &[(String, String)],
    exclude_ids: &std::collections::HashSet<String>,
) -> Vec<AlbumReco> {
    let cfg = crate::scrobbler_settings::get();
    if !cfg.lastfm_is_authed() || cfg.lastfm_username.is_empty() {
        return Vec::new();
    }
    let cache_dir = CACHE_DIR.lock().ok().and_then(|g| g.clone());
    let cache = match &cache_dir {
        Some(dir) => RecoCache::open_at(dir).ok().map(Mutex::new),
        None => None,
    };
    let cache_key = format!("album_lastfm:{album_id}");

    // Cache hit: return the resolved row without any Last.fm/Qobuz traffic.
    if let Some(c) = &cache {
        if let Some(json) = c
            .lock()
            .ok()
            .and_then(|g| g.get_results(&cache_key, LASTFM_SIMILAR_TTL_SECS))
        {
            if let Ok(cached) = serde_json::from_str::<Vec<AlbumReco>>(&json) {
                return cached;
            }
        }
    }

    let lastfm_client = LastFmClient::new();
    let mb_client = MusicBrainzClient::new();
    let catalog = CoreRecoCatalog {
        runtime: runtime.clone(),
    };
    let inputs = RecoInputs {
        lastfm: Some(LastFmHandle {
            username: cfg.lastfm_username.clone(),
            client: &lastfm_client,
        }),
        listenbrainz: None,
        musicbrainz: &mb_client,
        catalog: &catalog,
        cache: cache.as_ref(),
        local: LocalHistory::default(),
        rotation_seed: rotation_seed(),
    };
    let mut recos = build_similar_albums_seeded(&inputs, seed_artist, exclude_pairs).await;
    // Drop any that resolved to a Qobuz id already shown by the Qobuz row
    // (the pre-resolution artist|title dedup can miss these).
    recos.retain(|r| !exclude_ids.contains(&r.qobuz_album_id));

    // Cache only a non-empty result (an empty one is likely transient).
    if !recos.is_empty() {
        if let Some(c) = &cache {
            if let (Ok(g), Ok(json)) = (c.lock(), serde_json::to_string(&recos)) {
                g.put_results(&cache_key, &json);
            }
        }
    }
    recos
}

fn rotation_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
}

fn spawn(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: ImageCache,
    force: bool,
) {
    handle.spawn(async move {
        let cfg = crate::scrobbler_settings::get();

        let lastfm_client = LastFmClient::new();
        let lb_client = ListenBrainzClient::new();
        if cfg.listenbrainz_is_authed() {
            lb_client
                .restore_token(cfg.listenbrainz_token.clone(), cfg.listenbrainz_username.clone())
                .await;
        }
        let mb_client = MusicBrainzClient::new();

        let lastfm = if cfg.lastfm_is_authed() && !cfg.lastfm_username.is_empty() {
            Some(LastFmHandle {
                username: cfg.lastfm_username.clone(),
                client: &lastfm_client,
            })
        } else {
            None
        };
        let listenbrainz = if cfg.listenbrainz_is_authed() && !cfg.listenbrainz_username.is_empty() {
            Some(ListenBrainzHandle {
                username: cfg.listenbrainz_username.clone(),
                client: &lb_client,
            })
        } else {
            None
        };

        let local = LocalHistory {
            known_artist_ids: crate::reco::known_artist_ids(2).unwrap_or_default(),
            ..Default::default()
        };

        let catalog = CoreRecoCatalog {
            runtime: runtime.clone(),
        };
        let cache_dir = CACHE_DIR.lock().ok().and_then(|g| g.clone());
        let cache = match &cache_dir {
            Some(dir) => match RecoCache::open_at(dir) {
                Ok(c) => Some(Mutex::new(c)),
                Err(e) => {
                    log::warn!("[reco] spawn: cache open failed ({e}) — running uncached");
                    None
                }
            },
            None => {
                log::warn!("[reco] spawn: cache dir not set (init_for_user not run?) — running uncached");
                None
            }
        };

        let inputs = RecoInputs {
            lastfm,
            listenbrainz,
            musicbrainz: &mb_client,
            catalog: &catalog,
            cache: cache.as_ref(),
            local,
            rotation_seed: rotation_seed(),
        };

        let source_key = format!(
            "results:lf={}:lb={}",
            inputs.lastfm.is_some(),
            inputs.listenbrainz.is_some()
        );
        log::info!(
            "[reco] spawn: lastfm={} listenbrainz={} source_key={source_key} force={force}",
            inputs.lastfm.is_some(),
            inputs.listenbrainz.is_some()
        );

        // Effective results-cache window (Recommendations setting -> seconds).
        let ttl_secs = crate::discover_prefs::reco_cache_ttl_secs();

        // 1. Results cache: paint the NON-weekly rows INSTANTLY if a fresh
        // (within the configured window) build is cached. The Weekly
        // Exploration/Jams rows are NOT trusted from this blob — they follow
        // ListenBrainz's own weekly cadence and have their own per-week cache, so
        // we ALWAYS (re)build them via build_and_apply_weeklies (cheap on a
        // weekly-cache hit). This is what stops a single transient empty build
        // from hiding them for the whole window.
        //
        // A FORCED reload (the "Refresh now" action) skips this block entirely so
        // the tab always rebuilds from scratch; the rebuild overwrites the blob
        // via put_results below.
        if !force {
            if let Some(cache_mutex) = inputs.cache {
                let cached = cache_mutex
                    .lock()
                    .ok()
                    .and_then(|g| g.get_results(&source_key, ttl_secs));
                if let Some(json) = cached {
                    if let Ok(result) = serde_json::from_str::<ExternalCarousels>(&json) {
                        apply_all(&weak, &image_cache, result);
                        // The non-weekly rows painted instantly from the blob; the
                        // two Weekly rows rebuild from their own per-week cache, so
                        // show their skeletons until build_and_apply_weeklies fills
                        // them (instant on a weekly-cache hit).
                        if inputs.listenbrainz.is_some() {
                            let w = weak.clone();
                            let _ = w.upgrade_in_event_loop(|w| {
                                let s = w.global::<ExternalRecoState>();
                                s.set_pending_weekly_exploration(true);
                                s.set_pending_weekly_jams(true);
                            });
                        }
                        build_and_apply_weeklies(&inputs, &weak, &image_cache).await;
                        latch_loaded(&weak);
                        return;
                    }
                }
            }
        }

        // 2. Cache miss / stale: tell the user we're working, then build.
        crate::toast::info_weak(&weak, qbz_i18n::t("Generating recommendations…"));

        // Show per-row skeletons for the rows we're about to build, so the slow
        // rows (Weekly) read as "still loading" rather than absent during the
        // progressive paint. Each builder clears its own flag as it resolves.
        let cold_start = is_cold_start(&inputs);
        {
            let w = weak.clone();
            let _ = w.upgrade_in_event_loop(move |w| {
                set_pending(&w, cold_start);
            });
        }

        let collector: Arc<Mutex<ExternalCarousels>> = Arc::new(Mutex::new(ExternalCarousels::default()));

        if cold_start {
            let (albums, artists) = build_editorial(&inputs).await;
            if let Ok(mut g) = collector.lock() {
                g.editorial_fallback = true;
                g.top_albums = albums.clone();
                g.top_artists = artists.clone();
            }
            apply_albums(&weak, &image_cache, albums, AlbumRow::TopAlbums);
            apply_artists(&weak, &image_cache, artists, ArtistRow::TopArtists);
        } else {
            let history = gather_history(&inputs).await;
            let col = &collector;
            // Progressive: each branch paints its row AND collects it for the cache.
            let b_common = async {
                let r = build_rec_artists_common(&inputs, &history).await;
                if let Ok(mut g) = col.lock() {
                    g.rec_artists_common = r.clone();
                }
                apply_artists(&weak, &image_cache, r, ArtistRow::RecArtistsCommon);
            };
            let b_recent = async {
                let r = build_rec_artists_recent(&inputs, &history).await;
                if let Ok(mut g) = col.lock() {
                    g.rec_artists_recent = r.clone();
                }
                apply_artists(&weak, &image_cache, r, ArtistRow::RecArtistsRecent);
            };
            let b_albums = async {
                let r = build_rec_albums(&inputs, &history).await;
                if let Ok(mut g) = col.lock() {
                    g.rec_albums = r.clone();
                }
                apply_albums(&weak, &image_cache, r, AlbumRow::RecAlbums);
            };
            let b_fresh = async {
                let r = build_fresh_releases(&inputs).await;
                if let Ok(mut g) = col.lock() {
                    g.fresh_releases = r.clone();
                }
                apply_albums(&weak, &image_cache, r, AlbumRow::FreshReleases);
            };
            let b_explore = async {
                let r = build_weekly_exploration(&inputs).await;
                if let Ok(mut g) = col.lock() {
                    g.weekly_exploration = r.clone();
                }
                apply_tracks(&weak, &image_cache, r, TrackRow::WeeklyExploration);
            };
            let b_jams = async {
                let r = build_weekly_jams(&inputs).await;
                if let Ok(mut g) = col.lock() {
                    g.weekly_jams = r.clone();
                }
                apply_tracks(&weak, &image_cache, r, TrackRow::WeeklyJams);
            };
            let b_deep = async {
                let r = build_deep_cut_albums(&inputs).await;
                if let Ok(mut g) = col.lock() {
                    g.deep_cut_albums = r.clone();
                }
                apply_albums(&weak, &image_cache, r, AlbumRow::DeepCuts);
            };
            tokio::join!(b_common, b_recent, b_albums, b_fresh, b_explore, b_jams, b_deep);
        }

        // 3. Store the built result for instant future opens (48h TTL). GUARD
        // against poisoning the cache with a TRANSIENT ListenBrainz failure
        // (rate-limit / network / token-not-yet-restored): if LB is connected
        // but EVERY LB-sourced row (Weekly Exploration/Jams + Fresh Releases)
        // came back empty, skip the write so the next open re-fetches —
        // otherwise the empty result would hide those rows for the full 48h.
        // (Owner-reported: the Weeklys showed once, then vanished on restart.)
        if let Some(cache_mutex) = inputs.cache {
            let lb_all_empty = collector
                .lock()
                .map(|g| {
                    g.weekly_exploration.is_empty()
                        && g.weekly_jams.is_empty()
                        && g.fresh_releases.is_empty()
                })
                .unwrap_or(true);
            if inputs.listenbrainz.is_some() && lb_all_empty {
                log::warn!(
                    "[reco] ListenBrainz connected but all LB rows empty — skipping \
                     the results-cache write (likely transient; next open re-fetches)"
                );
            } else {
                let json = collector.lock().ok().and_then(|g| serde_json::to_string(&*g).ok());
                if let (Ok(guard), Some(json)) = (cache_mutex.lock(), json) {
                    guard.put_results(&source_key, &json);
                }
            }
        }

        latch_loaded(&weak);
    });
}

fn latch_loaded(weak: &slint::Weak<AppWindow>) {
    let _ = weak.upgrade_in_event_loop(|w| {
        let s = w.global::<ExternalRecoState>();
        s.set_loading(false);
        s.set_loaded(true);
        // Defensive: every builder clears its own pending flag as it resolves;
        // this guarantees no skeleton can stick after the whole build settles.
        clear_all_pending(&w);
    });
}

/// Mark the rows the controller is about to build as pending, so their per-row
/// skeletons show immediately while the builders run.
fn set_pending(w: &AppWindow, cold_start: bool) {
    let s = w.global::<ExternalRecoState>();
    if cold_start {
        s.set_pending_top_albums(true);
        s.set_pending_top_artists(true);
    } else {
        s.set_pending_rec_artists_common(true);
        s.set_pending_rec_artists_recent(true);
        s.set_pending_rec_albums(true);
        s.set_pending_fresh_releases(true);
        s.set_pending_weekly_exploration(true);
        s.set_pending_weekly_jams(true);
        s.set_pending_deep_cut_albums(true);
    }
}

fn clear_all_pending(w: &AppWindow) {
    let s = w.global::<ExternalRecoState>();
    s.set_pending_rec_artists_common(false);
    s.set_pending_rec_artists_recent(false);
    s.set_pending_rec_albums(false);
    s.set_pending_fresh_releases(false);
    s.set_pending_weekly_exploration(false);
    s.set_pending_weekly_jams(false);
    s.set_pending_deep_cut_albums(false);
    s.set_pending_top_albums(false);
    s.set_pending_top_artists(false);
}

/// Paint the NON-weekly rows from a cached 48h blob (empty rows self-hide). The
/// two Weekly rows are intentionally NOT painted here — they are (re)built from
/// their own per-week cache by `build_and_apply_weeklies`, so the blob can never
/// pin a stale/empty weekly for the 48h window.
fn apply_all(weak: &slint::Weak<AppWindow>, cache: &ImageCache, r: ExternalCarousels) {
    apply_artists(weak, cache, r.rec_artists_common, ArtistRow::RecArtistsCommon);
    apply_artists(weak, cache, r.rec_artists_recent, ArtistRow::RecArtistsRecent);
    apply_albums(weak, cache, r.rec_albums, AlbumRow::RecAlbums);
    apply_albums(weak, cache, r.fresh_releases, AlbumRow::FreshReleases);
    apply_albums(weak, cache, r.deep_cut_albums, AlbumRow::DeepCuts);
    apply_albums(weak, cache, r.top_albums, AlbumRow::TopAlbums);
    apply_artists(weak, cache, r.top_artists, ArtistRow::TopArtists);
}

/// Build + paint the two Weekly rows from their own per-week cache (cheap on a
/// hit; one ListenBrainz `createdfor` call + a SQLite read). Used on the
/// instant-paint path so the weeklies follow ListenBrainz's weekly cadence
/// independently of the 48h results blob. The full-build path paints them via
/// its own `b_explore`/`b_jams` branches, which call the same cache-backed
/// builders.
async fn build_and_apply_weeklies(
    inputs: &RecoInputs<'_>,
    weak: &slint::Weak<AppWindow>,
    image_cache: &ImageCache,
) {
    if inputs.listenbrainz.is_none() {
        return;
    }
    let (explore, jams) =
        tokio::join!(build_weekly_exploration(inputs), build_weekly_jams(inputs));
    apply_tracks(weak, image_cache, explore, TrackRow::WeeklyExploration);
    apply_tracks(weak, image_cache, jams, TrackRow::WeeklyJams);
}

// ── Per-row apply (models built on the UI thread; slint::Image is !Send) ────

#[derive(Clone, Copy)]
enum ArtistRow {
    RecArtistsCommon,
    RecArtistsRecent,
    TopArtists,
}
#[derive(Clone, Copy)]
enum AlbumRow {
    RecAlbums,
    FreshReleases,
    DeepCuts,
    TopAlbums,
}
#[derive(Clone, Copy)]
enum TrackRow {
    WeeklyExploration,
    WeeklyJams,
}

/// Read the backing Qobuz track ids of one external-reco Weekly TRACK row
/// (Weekly Exploration / Weekly Jams) for the P7 title-adjacent buttons.
/// Returns the whole backing list (not just the 24 visible), in row order.
pub fn list_track_ids(window: &AppWindow, section: &str) -> Vec<u64> {
    let s = window.global::<ExternalRecoState>();
    let model = match section {
        "weekly-exploration" => s.get_weekly_exploration(),
        "weekly-jams" => s.get_weekly_jams(),
        _ => return Vec::new(),
    };
    model
        .iter()
        .filter_map(|it| it.id.as_str().parse::<u64>().ok())
        .collect()
}

fn slim_from_artist(a: &ArtistReco) -> SlimItem {
    let id = a.qobuz_artist_id.to_string();
    SlimItem {
        // Pin badge state from the per-user pinned store (kept live by
        // main::set_artist_row_pinned when a pin toggles anywhere). First:
        // it must borrow `id` before the `id:` initializer moves it.
        is_pinned: crate::pinned::is_pinned("artist", &id),
        id: id.into(),
        title: a.name.clone().into(),
        subtitle: a.subtitle.clone().into(),
        rank: "".into(),
        artwork_url: a.image_url.clone().into(),
        artwork: slint::Image::default(),
        following: false,
    }
}
fn slim_from_track(t: &TrackReco) -> SlimItem {
    SlimItem {
        id: t.qobuz_track_id.to_string().into(),
        title: t.title.clone().into(),
        subtitle: t.artist.clone().into(),
        rank: "".into(),
        artwork_url: t.artwork_url.clone().into(),
        artwork: slint::Image::default(),
        following: false,
        // Track slims render pin-less rows — tracks are not pinnable.
        is_pinned: false,
    }
}
pub(crate) fn album_card(a: &AlbumReco) -> AlbumCardItem {
    AlbumCardItem {
        plays: 0,
        // Favorite heart state from the login-seeded cache (kept live by
        // main::set_album_row_favorite when a favorite toggles anywhere).
        is_favorite: crate::fav_cache::is_album_favorite(&a.qobuz_album_id),
        // Pin badge state from the per-user pinned store (kept live by
        // main::set_album_row_pinned when a pin toggles anywhere).
        is_pinned: crate::pinned::is_pinned("album", &a.qobuz_album_id),
        id: a.qobuz_album_id.clone().into(),
        title: a.title.clone().into(),
        artist: a.artist.clone().into(),
        artist_id: a.artist_id.clone().into(),
        genre: "".into(),
        year: a.year.clone().into(),
        quality_tier: a.quality_tier.clone().into(),
        quality_label: a.quality_label.clone().into(),
        ribbon: "".into(),
        ribbon_kind: "".into(),
        artwork_url: a.artwork_url.clone().into(),
        artwork: slint::Image::default(),
        ..Default::default()
    }
}

fn apply_artists(
    weak: &slint::Weak<AppWindow>,
    cache: &ImageCache,
    rows: Vec<ArtistReco>,
    which: ArtistRow,
) {
    let jobs: Vec<ArtworkJob> = rows
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.image_url.is_empty())
        .map(|(i, a)| ArtworkJob {
            url: a.image_url.clone(),
            target: match which {
                ArtistRow::RecArtistsCommon => ArtworkTarget::ExtRecoRecArtistCommon { index: i },
                ArtistRow::RecArtistsRecent => ArtworkTarget::ExtRecoRecArtistRecent { index: i },
                ArtistRow::TopArtists => ArtworkTarget::ExtRecoTopArtist { index: i },
            },
        })
        .collect();
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        let model = ModelRc::new(VecModel::from(
            rows.iter().map(slim_from_artist).collect::<Vec<_>>(),
        ));
        let s = w.global::<ExternalRecoState>();
        match which {
            ArtistRow::RecArtistsCommon => {
                s.set_rec_artists_common(model);
                s.set_pending_rec_artists_common(false);
            }
            ArtistRow::RecArtistsRecent => {
                s.set_rec_artists_recent(model);
                s.set_pending_rec_artists_recent(false);
            }
            ArtistRow::TopArtists => {
                s.set_top_artists(model);
                s.set_pending_top_artists(false);
            }
        }
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}

fn apply_tracks(
    weak: &slint::Weak<AppWindow>,
    cache: &ImageCache,
    rows: Vec<TrackReco>,
    which: TrackRow,
) {
    let jobs: Vec<ArtworkJob> = rows
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.artwork_url.is_empty())
        .map(|(i, t)| ArtworkJob {
            url: t.artwork_url.clone(),
            target: match which {
                TrackRow::WeeklyExploration => ArtworkTarget::ExtRecoWeeklyExploration { index: i },
                TrackRow::WeeklyJams => ArtworkTarget::ExtRecoWeeklyJams { index: i },
            },
        })
        .collect();
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        let model = ModelRc::new(VecModel::from(
            rows.iter().map(slim_from_track).collect::<Vec<_>>(),
        ));
        let s = w.global::<ExternalRecoState>();
        match which {
            TrackRow::WeeklyExploration => {
                s.set_weekly_exploration(model);
                s.set_pending_weekly_exploration(false);
            }
            TrackRow::WeeklyJams => {
                s.set_weekly_jams(model);
                s.set_pending_weekly_jams(false);
            }
        }
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}

fn album_row_title(which: AlbumRow) -> String {
    match which {
        AlbumRow::RecAlbums => qbz_i18n::t("Recommended Albums"),
        AlbumRow::FreshReleases => qbz_i18n::t("Fresh Releases"),
        AlbumRow::DeepCuts => qbz_i18n::t("Deep cuts from artists you know"),
        AlbumRow::TopAlbums => qbz_i18n::t("Top albums on Qobuz"),
    }
}

fn apply_albums(
    weak: &slint::Weak<AppWindow>,
    cache: &ImageCache,
    rows: Vec<AlbumReco>,
    which: AlbumRow,
) {
    let jobs: Vec<ArtworkJob> = rows
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.artwork_url.is_empty())
        .map(|(i, a)| ArtworkJob {
            url: a.artwork_url.clone(),
            target: match which {
                AlbumRow::RecAlbums => ArtworkTarget::ExtRecoRecAlbum { index: i },
                AlbumRow::FreshReleases => ArtworkTarget::ExtRecoFreshAlbum { index: i },
                AlbumRow::DeepCuts => ArtworkTarget::ExtRecoDeepAlbum { index: i },
                AlbumRow::TopAlbums => ArtworkTarget::ExtRecoTopAlbum { index: i },
            },
        })
        .collect();
    let title = album_row_title(which);
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        let section = DiscoverSection {
            title: title.into(),
            endpoint: "".into(),
            albums: ModelRc::new(VecModel::from(
                rows.iter().map(album_card).collect::<Vec<_>>(),
            )),
        };
        let s = w.global::<ExternalRecoState>();
        match which {
            AlbumRow::RecAlbums => {
                s.set_rec_albums(section);
                s.set_pending_rec_albums(false);
            }
            AlbumRow::FreshReleases => {
                s.set_fresh_releases(section);
                s.set_pending_fresh_releases(false);
            }
            AlbumRow::DeepCuts => {
                s.set_deep_cut_albums(section);
                s.set_pending_deep_cut_albums(false);
            }
            AlbumRow::TopAlbums => {
                s.set_top_albums(section);
                s.set_pending_top_albums(false);
            }
        }
    });
    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}
