//! Discover > Recommendations (the 4th tab) controller.
//!
//! Wires the frontend-agnostic `qbz-external-reco` engine to the Slint frontend:
//! a [`RecoCatalog`] over `QbzCore`, the per-user resolution-cache lifecycle,
//! the local-history gather from the reco store, the scrobbler-username gate,
//! and the apply into `ExternalRecoState`. Lazy-loads on first tab open (mirrors
//! `ensure_for_you_loaded`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use qbz_app::shell::AppRuntime;
use qbz_external_reco::{
    build_external_carousels, ArtistReco, ExternalCarousels, LastFmHandle, ListenBrainzHandle,
    LocalHistory, RecoCache, RecoCatalog, RecoInputs, TrackReco,
};
use qbz_integrations::{LastFmClient, ListenBrainzClient, MusicBrainzClient};
use qbz_models::{Album, Artist, Track};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::artwork::{ArtworkJob, ArtworkTarget, ImageCache};
use crate::{AlbumCardItem, AppWindow, DiscoverSection, ExternalRecoState, SlimItem};

/// Per-user cache directory (the rec->Qobuz resolution cache lives at
/// `<dir>/external_reco_cache.db`). `None` outside an active session.
static CACHE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Bind the per-user cache dir on session activation (next to
/// `crate::reco::init_for_user`). Opening the cache opportunistically cleans
/// expired rows.
pub fn init_for_user(base_dir: &Path) {
    if let Ok(mut g) = CACHE_DIR.lock() {
        *g = Some(base_dir.to_path_buf());
    }
    // Best-effort: open once to run cleanup; the per-build open reuses the file.
    if let Ok(cache) = RecoCache::open_at(base_dir) {
        let _ = cache.cleanup_expired();
    }
}

/// Drop the per-user cache binding on logout. (Not strictly required: every
/// session activation re-binds via `init_for_user`; kept for symmetry.)
#[allow(dead_code)]
pub fn teardown() {
    if let Ok(mut g) = CACHE_DIR.lock() {
        *g = None;
    }
}

// ---------------------------------------------------------------------------
// RecoCatalog over QbzCore (errors -> empty; "no data" is never an error).
// ---------------------------------------------------------------------------

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

    async fn artist_top_tracks(&self, artist_id: u64, limit: usize) -> Vec<Track> {
        self.runtime
            .core()
            .get_artist_tracks(artist_id, limit as u32, 0)
            .await
            .map(|c| c.items)
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

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Lazy-load the Recommendations tab the first time it is opened. No-op once
/// loaded (the data persists for the session).
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
    spawn(runtime.clone(), weak.clone(), handle, image_cache.clone());
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
) {
    handle.spawn(async move {
        let cfg = crate::scrobbler_settings::get();

        // Read clients (public reads — Last.fm via the proxy by username, LB by
        // username with an optional token for higher limits, MB direct).
        let lastfm_client = LastFmClient::new();
        let lb_client = ListenBrainzClient::new();
        if cfg.listenbrainz_is_authed() {
            lb_client
                .restore_token(
                    cfg.listenbrainz_token.clone(),
                    cfg.listenbrainz_username.clone(),
                )
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
            played_track_ids: crate::reco::recent_track_ids(2000)
                .map(|v| v.into_iter().collect())
                .unwrap_or_default(),
        };

        let catalog = CoreRecoCatalog {
            runtime: runtime.clone(),
        };

        // Open the resolution cache for this build (reuses the on-disk file).
        let cache = CACHE_DIR
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .and_then(|dir| RecoCache::open_at(&dir).ok())
            .map(Mutex::new);

        let inputs = RecoInputs {
            lastfm,
            listenbrainz,
            musicbrainz: &mb_client,
            catalog: &catalog,
            cache: cache.as_ref(),
            local,
            rotation_seed: rotation_seed(),
        };

        let result = build_external_carousels(inputs).await;
        apply(&weak, &image_cache, result);

        let _ = weak.upgrade_in_event_loop(|w| {
            let s = w.global::<ExternalRecoState>();
            s.set_loading(false);
            s.set_loaded(true);
        });
    });
}

// ---------------------------------------------------------------------------
// Slint model mappers + apply
// ---------------------------------------------------------------------------

fn slim_from_artist(a: &ArtistReco) -> SlimItem {
    SlimItem {
        id: a.qobuz_artist_id.to_string().into(),
        title: a.name.clone().into(),
        subtitle: "".into(),
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
    }
}

fn album_card(a: &qbz_external_reco::AlbumReco) -> AlbumCardItem {
    AlbumCardItem {
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

fn apply(weak: &slint::Weak<AppWindow>, cache: &ImageCache, data: ExternalCarousels) {
    // Artwork jobs built from the resolved rows (owned String urls).
    let mut jobs: Vec<ArtworkJob> = Vec::new();
    let push_artist = |jobs: &mut Vec<ArtworkJob>, rows: &[ArtistReco], f: fn(usize) -> ArtworkTarget| {
        for (i, a) in rows.iter().enumerate() {
            if !a.image_url.is_empty() {
                jobs.push(ArtworkJob {
                    url: a.image_url.clone(),
                    target: f(i),
                });
            }
        }
    };
    let push_track = |jobs: &mut Vec<ArtworkJob>, rows: &[TrackReco], f: fn(usize) -> ArtworkTarget| {
        for (i, t) in rows.iter().enumerate() {
            if !t.artwork_url.is_empty() {
                jobs.push(ArtworkJob {
                    url: t.artwork_url.clone(),
                    target: f(i),
                });
            }
        }
    };
    push_artist(&mut jobs, &data.similar_artists, |i| {
        ArtworkTarget::ExtRecoSimilarArtist { index: i }
    });
    push_track(&mut jobs, &data.similar_tracks, |i| {
        ArtworkTarget::ExtRecoSimilarTrack { index: i }
    });
    push_track(&mut jobs, &data.rediscover_tracks, |i| {
        ArtworkTarget::ExtRecoRediscoverTrack { index: i }
    });
    push_track(&mut jobs, &data.deep_cut_tracks, |i| {
        ArtworkTarget::ExtRecoDeepCutTrack { index: i }
    });
    for (i, a) in data.top_albums.iter().enumerate() {
        if !a.artwork_url.is_empty() {
            jobs.push(ArtworkJob {
                url: a.artwork_url.clone(),
                target: ArtworkTarget::ExtRecoTopAlbum { index: i },
            });
        }
    }
    push_artist(&mut jobs, &data.top_artists, |i| {
        ArtworkTarget::ExtRecoTopArtist { index: i }
    });

    // Convert to Slint models INSIDE the event-loop closure: SlimItem /
    // AlbumCardItem hold a slint::Image (!Send), so they cannot cross the thread
    // boundary. `data` is plain String/u64 (Send) — move it in and map on the UI
    // thread (mirrors foryou.rs apply_*).
    let w = weak.clone();
    let _ = w.upgrade_in_event_loop(move |w| {
        let s = w.global::<ExternalRecoState>();
        s.set_editorial_fallback(data.editorial_fallback);
        s.set_similar_artists(ModelRc::new(VecModel::from(
            data.similar_artists.iter().map(slim_from_artist).collect::<Vec<_>>(),
        )));
        s.set_similar_tracks(ModelRc::new(VecModel::from(
            data.similar_tracks.iter().map(slim_from_track).collect::<Vec<_>>(),
        )));
        s.set_rediscover_tracks(ModelRc::new(VecModel::from(
            data.rediscover_tracks.iter().map(slim_from_track).collect::<Vec<_>>(),
        )));
        s.set_deep_cut_tracks(ModelRc::new(VecModel::from(
            data.deep_cut_tracks.iter().map(slim_from_track).collect::<Vec<_>>(),
        )));
        s.set_top_artists(ModelRc::new(VecModel::from(
            data.top_artists.iter().map(slim_from_artist).collect::<Vec<_>>(),
        )));
        s.set_top_albums(DiscoverSection {
            title: qbz_i18n::t("Top albums on Qobuz").into(),
            endpoint: "".into(),
            albums: ModelRc::new(VecModel::from(
                data.top_albums.iter().map(album_card).collect::<Vec<_>>(),
            )),
        });
    });

    crate::artwork::spawn_loads(jobs, weak.clone(), cache.clone());
}
