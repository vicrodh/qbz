//! Playback / queue controller.
//!
//! Owns the orchestration between the UI and `QbzCore`'s player + queue.
//! Albums and tracks are turned into a `Vec<QueueTrack>`, handed to the
//! core's `QueueManager`, and then played audibly through
//! `Player::play_track` (the self-contained "fetch URL → download → play"
//! path — the protected bit-perfect audio backend is untouched).
//!
//! There is no event stream from the player, so a `tokio` poll task reads
//! `Player::get_playback_event()` a few times a second and pushes the
//! values onto the `NowPlayingState` global. The same task drives
//! auto-advance when a track ends.

use std::sync::{Arc, OnceLock};

use qbz_app::shell::AppRuntime;
use qbz_models::{Quality, QueueTrack, RepeatMode};
use slint::ComponentHandle;

use crate::adapter::SlintAdapter;
use crate::queue::QueueController;
use crate::{AppWindow, NowPlayingState};

/// The Queue sidebar controller, published once the shell is up so the
/// playback paths (album/track play, skip, auto-advance) can refresh the
/// sidebar after every queue mutation.
static QUEUE_CONTROLLER: OnceLock<QueueController> = OnceLock::new();

/// Register the Queue sidebar controller. Called once during shell setup.
pub fn set_queue_controller(controller: QueueController) {
    let _ = QUEUE_CONTROLLER.set(controller);
}

/// Refresh the Queue sidebar from the current core queue state. No-op
/// before the controller is registered. `with_favorites` re-pulls the
/// favorite-track cache as well (used after a fresh play starts).
fn refresh_sidebar(with_favorites: bool) {
    if let Some(controller) = QUEUE_CONTROLLER.get() {
        if with_favorites {
            controller.refresh_with_favorites();
        } else {
            controller.refresh();
        }
    }
}

/// Shared post-track-change step: update the now-playing card, record the
/// play in the recently-played store, and start audio for `track_id`.
/// Used by the queue controller's play paths.
pub async fn after_track_change(
    runtime: &Runtime,
    weak: &slint::Weak<AppWindow>,
    track_id: u64,
) {
    refresh_now_playing_meta(runtime, weak).await;
    record_recent(runtime).await;
    play_audible(runtime, weak, track_id).await;
    // Warm the cache for the upcoming tracks so the next transition can be
    // gapless (a cached track plays via `play_data`, which the audio
    // engine's gapless engine supports; a streamed track does not).
    kick_prefetch(runtime).await;
}

/// How many upcoming queue tracks to prefetch into the player cache.
/// Two tracks ahead is enough headroom for gapless without holding an
/// excessive number of HiRes payloads in memory. Matches the spirit of
/// Tauri's `v2_prefetch_count` (which is host-tuned; the Slint MVP uses
/// a fixed small value).
const PREFETCH_LOOKAHEAD: usize = 2;

/// Maximum concurrent prefetch downloads — mirrors Tauri's
/// `v2_max_concurrent_prefetch` default for normal hosts.
const MAX_CONCURRENT_PREFETCH: usize = 2;

/// Shared semaphore bounding concurrent prefetch downloads across all
/// `kick_prefetch` calls.
static PREFETCH_SEMAPHORE: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(MAX_CONCURRENT_PREFETCH);

/// Peek the next `PREFETCH_LOOKAHEAD` upcoming queue tracks and spawn a
/// background download for each one not already cached. Each download
/// goes into the player's L1/L2 cache via `Player::prefetch_into_cache`
/// so the track later plays via `play_data` (a cache hit) and is gapless
/// eligible. Concurrency is bounded by `PREFETCH_SEMAPHORE`.
async fn kick_prefetch(runtime: &Runtime) {
    let upcoming = runtime.core().peek_upcoming(PREFETCH_LOOKAHEAD).await;
    if upcoming.is_empty() {
        return;
    }
    for track in upcoming {
        let track_id = track.id;
        // Local tracks never need a Qobuz prefetch.
        if track.is_local {
            continue;
        }
        let player = runtime.core().player();
        if player.is_track_cached(track_id) {
            continue;
        }
        let runtime = runtime.clone();
        tokio::spawn(async move {
            let _permit = match PREFETCH_SEMAPHORE.acquire().await {
                Ok(permit) => permit,
                Err(_) => return,
            };
            let client_lock = runtime.core().client();
            let guard = client_lock.read().await;
            let Some(client) = guard.as_ref() else {
                return;
            };
            let player = runtime.core().player();
            if let Err(e) = player
                .prefetch_into_cache(client, track_id, PLAYBACK_QUALITY)
                .await
            {
                log::debug!("[qbz-slint] prefetch: track {track_id} failed: {e}");
            }
        });
    }
}

/// Streaming quality used for all playback in the MVP — highest tier,
/// the player falls back internally when it is not available.
const PLAYBACK_QUALITY: Quality = Quality::UltraHiRes;

/// Convenience alias for the runtime handle threaded through every call.
type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Run the audible step for `track_id`: grab the Qobuz client and call
/// the player's self-contained `play_track`. Errors are logged, not
/// surfaced — the poll loop keeps the UI consistent regardless.
async fn play_audible(runtime: &Runtime, weak: &slint::Weak<AppWindow>, track_id: u64) {
    // Source-aware: a LOCAL user file plays from disk via the play_data seam.
    // Offline-cached + Qobuz keep the existing tier-walk below (unchanged), so
    // streaming playback can't regress. The current queue track tells us which
    // path to take via its `source`; the id guard avoids mis-routing when the
    // current track and `track_id` momentarily disagree. Auto-advance, skip and
    // play-all all flow through here, so they become source-aware for free.
    if let Some(qt) = runtime.core().current_track().await {
        if qt.id == track_id && qt.source.as_deref() == Some("local") {
            play_local_file_audible(runtime, track_id).await;
            return;
        }
    }
    // Offline-cached copy (preferred, decrypted to FLAC + played via play_data)
    // -> player L1/L2 -> Qobuz network. The offline handle is None before
    // login. The sink drives the padlock while a CMAF bundle decrypts.
    let offline = crate::offline::get().await;
    let sink = crate::offline_cache::row_sink(weak.clone());
    if let Err(e) = runtime
        .core()
        .play_track_resolved(track_id, PLAYBACK_QUALITY, offline.as_deref(), Some(&sink))
        .await
    {
        log::error!("[qbz-slint] playback: play_track {track_id} failed: {e}");
    }
}

/// Audible step for a LOCAL user file: read it off-thread and hand the bytes
/// to the player's `play_data` seam (which extracts the sample rate + drives
/// the PROTECTED device init, untouched here). CUE virtual tracks share one
/// file, so seek to the track start. `row_id` is the library row id. Called
/// by `play_audible` when the current queue track's source is `"local"`.
async fn play_local_file_audible(runtime: &Runtime, row_id: u64) {
    let info = tokio::task::spawn_blocking(move || {
        crate::library_db::with_db(|db| db.get_track(row_id as i64))
    })
    .await
    .ok()
    .flatten()
    .flatten()
    .map(|t| (t.file_path, t.cue_start_secs));
    let Some((path, cue)) = info else {
        log::error!("[qbz-slint] local play: track {row_id} not found");
        return;
    };
    let read_path = path.clone();
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&read_path))
        .await
        .ok()
        .and_then(Result::ok);
    let Some(bytes) = bytes else {
        log::error!("[qbz-slint] local play: failed to read {path}");
        return;
    };
    if let Err(e) = runtime.core().player().play_data(bytes, row_id) {
        log::error!("[qbz-slint] local play: play_data {row_id} failed: {e}");
        return;
    }
    if let Some(start) = cue {
        if start > 0.0 {
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            let _ = runtime.core().player().seek(start as u64);
        }
    }
}

/// Set a Local Library queue and start playback at `start`. Source-aware
/// `play_audible` routes each track (local file vs offline vs Qobuz) and
/// auto-advance flows through the same path, so a mixed-source album/list
/// plays through. UI-thread async step.
async fn play_local_tracks_now(
    runtime: &Runtime,
    weak: &slint::Weak<AppWindow>,
    tracks: Vec<qbz_library::LocalTrack>,
    start: usize,
) {
    if tracks.is_empty() {
        return;
    }
    let queue: Vec<QueueTrack> = tracks.iter().map(local_queue_track).collect();
    let start = start.min(queue.len() - 1);
    let play_id = queue[start].id;
    runtime.core().set_queue(queue, Some(start)).await;
    after_track_change(runtime, weak, play_id).await;
}

/// Play a local/offline album (metadata-grouped): the whole album becomes the
/// queue and auto-advances. `album_id` is the metadata group key.
pub fn play_local_album(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
    start_track_id: Option<i64>,
) {
    handle.spawn(async move {
        let tracks = tokio::task::spawn_blocking(move || {
            let mut tracks = crate::local_library::fetch_album_tracks_blocking(&album_id);
            fill_missing_covers(&mut tracks);
            tracks
        })
        .await
        .unwrap_or_default();
        // Start at the requested track (a row click in the album detail) or
        // the top (play-all).
        let start = match start_track_id {
            Some(tid) => tracks.iter().position(|t| t.id == tid).unwrap_or(0),
            None => 0,
        };
        play_local_tracks_now(&runtime, &weak, tracks, start).await;
    });
}

/// Play everything under a folder (recursive), in path order — the whole
/// subtree becomes the queue. Mirrors `play_local_album` but sources the
/// tracks from the folder hierarchy instead of a metadata group.
pub fn play_local_folder_recursive(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    folder_path: String,
) {
    handle.spawn(async move {
        let tracks = tokio::task::spawn_blocking(move || {
            let mut tracks = crate::library_db::with_db(|db| {
                db.list_folder_tracks_recursive(&folder_path, false)
            })
            .unwrap_or_default();
            fill_missing_covers(&mut tracks);
            tracks
        })
        .await
        .unwrap_or_default();
        if tracks.is_empty() {
            return;
        }
        play_local_tracks_now(&runtime, &weak, tracks, 0).await;
    });
}

/// Play a folder's DIRECT tracks (non-recursive) starting at `start_track_id`
/// — the folder's own track list becomes the queue. Used by the tree-mode
/// detail pane when a track row is clicked.
pub fn play_local_folder_tracks_from(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    folder_path: String,
    start_track_id: i64,
) {
    handle.spawn(async move {
        let tracks = tokio::task::spawn_blocking(move || {
            let mut tracks =
                crate::library_db::with_db(|db| db.list_folder_tracks(&folder_path, false))
                    .unwrap_or_default();
            fill_missing_covers(&mut tracks);
            tracks
        })
        .await
        .unwrap_or_default();
        if tracks.is_empty() {
            return;
        }
        let start = tracks
            .iter()
            .position(|t| t.id == start_track_id)
            .unwrap_or(0);
        play_local_tracks_now(&runtime, &weak, tracks, start).await;
    });
}

/// Play the Tracks-tab list starting at `start_track_id`: the matching set
/// (current search) becomes the queue, so playback continues down the list.
pub fn play_local_tracks_from(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    query: String,
    start_track_id: i64,
) {
    handle.spawn(async move {
        let tracks = tokio::task::spawn_blocking(move || {
            let mut tracks =
                crate::library_db::with_db(|db| db.search_with_filter(query.trim(), 0, true, false))
                    .unwrap_or_default();
            fill_missing_covers(&mut tracks);
            tracks
        })
        .await
        .unwrap_or_default();
        let start = tracks
            .iter()
            .position(|t| t.id == start_track_id)
            .unwrap_or(0);
        play_local_tracks_now(&runtime, &weak, tracks, start).await;
    });
}

/// Build a `QueueTrack` from a local-library row. Mirrors Tauri's
/// `local_track_to_queue_track`: `file://` artwork, kHz sample rate, the real
/// source. Offline copies carry the Qobuz id (so the shared resolver finds
/// them) + `source = "qobuz_download"`; user files carry the library row id +
/// `source = "local"`.
fn local_queue_track(track: &qbz_library::LocalTrack) -> QueueTrack {
    let artwork_url = track.artwork_path.as_ref().map(|p| {
        if p.starts_with("file://") {
            p.clone()
        } else {
            format!("file://{p}")
        }
    });
    let sample_rate_khz = if track.sample_rate >= 1000.0 {
        track.sample_rate / 1000.0
    } else {
        track.sample_rate
    };
    let is_offline = track.source.as_deref() == Some("qobuz_download");
    QueueTrack {
        id: if is_offline {
            track.qobuz_track_id.unwrap_or(track.id) as u64
        } else {
            track.id as u64
        },
        title: track.title.clone(),
        version: None,
        artist: track.artist.clone(),
        album: track.album_group_title.clone(),
        duration_secs: track.duration_secs,
        artwork_url,
        hires: track.bit_depth.map(|d| d > 16).unwrap_or(false),
        bit_depth: track.bit_depth,
        sample_rate: Some(sample_rate_khz),
        is_local: true,
        album_id: Some(track.album_group_key.clone()),
        artist_id: None,
        streamable: true,
        source: Some(if is_offline {
            "qobuz_download".to_string()
        } else {
            "local".to_string()
        }),
        parental_warning: false,
        source_item_id_hint: None,
    }
}

/// Fill `artwork_path` for tracks that lack one, from a cover image in the
/// track's folder (the offline-cache writes `cover.jpg` there but doesn't
/// always backfill the index) — so the cover that exists on disk reaches the
/// now-playing bar + queue, not just the album grid. Runs off-thread (fs),
/// memoized per folder so a whole album costs one stat.
pub fn fill_missing_covers(tracks: &mut [qbz_library::LocalTrack]) {
    use std::collections::HashMap;
    let mut memo: HashMap<String, Option<String>> = HashMap::new();
    for t in tracks.iter_mut() {
        if t.artwork_path.as_deref().is_some_and(|s| !s.is_empty()) {
            continue;
        }
        let p = std::path::Path::new(&t.file_path);
        let folder = if p.is_dir() {
            p.to_path_buf()
        } else {
            match p.parent() {
                Some(d) => d.to_path_buf(),
                None => continue,
            }
        };
        let key = folder.to_string_lossy().into_owned();
        let cover = memo
            .entry(key)
            .or_insert_with(|| {
                ["cover.jpg", "cover.png", "folder.jpg", "front.jpg"]
                    .iter()
                    .map(|n| folder.join(n))
                    .find(|c| c.is_file())
                    .map(|c| c.to_string_lossy().into_owned())
            })
            .clone();
        if cover.is_some() {
            t.artwork_path = cover;
        }
    }
}

/// Resolve the now-playing cover and apply it to `NowPlayingState`.
///
/// Takes a source-aware [`qbz_models::ArtworkRef`] so local-library and Plex
/// covers reach the now-playing bar, not just remote Qobuz URLs.
fn load_now_playing_artwork(weak: slint::Weak<AppWindow>, art: qbz_models::ArtworkRef) {
    if art.is_empty() {
        return;
    }
    let Some(cache) = crate::artwork::shared_cache() else {
        return;
    };
    tokio::spawn(async move {
        let Some((pixels, w, h)) =
            crate::artwork::fetch_and_decode_ref(&art, &cache, 160).await
        else {
            return;
        };
        let _ = weak.upgrade_in_event_loop(move |win| {
            let img = crate::artwork::pixels_to_image(&pixels, w, h);
            win.global::<NowPlayingState>().set_artwork(img);
        });
    });
}

/// `M:SS` for the elapsed string.
fn fmt_elapsed(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// `-M:SS` for the remaining string.
fn fmt_remaining(position: u64, duration: u64) -> String {
    let left = duration.saturating_sub(position);
    format!("-{}:{:02}", left / 60, left % 60)
}

/// Push the now-playing values for the current queue track onto
/// `NowPlayingState`. Called when a new track starts so the song card
/// updates immediately (the poll loop only refreshes position/progress).
async fn refresh_now_playing_meta(runtime: &Runtime, weak: &slint::Weak<AppWindow>) {
    let state = runtime.core().get_queue_state().await;
    let Some(track) = state.current_track else {
        let _ = weak.upgrade_in_event_loop(|w| {
            w.global::<NowPlayingState>().set_has_track(false);
        });
        return;
    };

    let title = match track.version.as_deref().filter(|v| !v.is_empty()) {
        Some(version) => format!("{} ({version})", track.title),
        None => track.title.clone(),
    };
    let artist = track.artist.clone();
    let album = track.album.clone();
    let album_id = track.album_id.clone().unwrap_or_default();
    let artist_id = track.artist_id.map(|id| id.to_string()).unwrap_or_default();
    let track_id = track.id.to_string();
    let duration = track.duration_secs;
    let artwork = track.artwork_ref();

    let _ = weak.upgrade_in_event_loop(move |w| {
        let np = w.global::<NowPlayingState>();
        np.set_has_track(true);
        np.set_title(title.into());
        np.set_artist(artist.into());
        np.set_album(album.into());
        np.set_album_id(album_id.into());
        np.set_artist_id(artist_id.into());
        np.set_track_id(track_id.into());
        np.set_duration_secs(duration as i32);
        np.set_position_secs(0);
        np.set_progress(0.0);
        np.set_cache(0.0);
        np.set_elapsed("0:00".into());
        np.set_remaining(fmt_remaining(0, duration).into());
        np.set_playing(true);
        // Clear the previous cover so it does not linger while the new
        // one resolves.
        np.set_artwork(slint::Image::default());
    });

    load_now_playing_artwork(weak.clone(), artwork);
}

/// Build a `QueueTrack` for the queue from the catalog `Track`, filling
/// the album metadata from `album_meta` (the track's own album summary is
/// often partial in album responses).
fn make_queue_track(
    track: &qbz_models::Track,
    album_id: &str,
    album_title: &str,
    album_artist: &str,
    album_artwork: &str,
) -> QueueTrack {
    QueueTrack {
        id: track.id,
        title: track.title.clone(),
        version: track.version.clone(),
        artist: track
            .performer
            .as_ref()
            .map(|p| p.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| album_artist.to_string()),
        album: album_title.to_string(),
        duration_secs: track.duration as u64,
        artwork_url: if album_artwork.is_empty() {
            None
        } else {
            Some(album_artwork.to_string())
        },
        hires: track.hires,
        bit_depth: track.maximum_bit_depth,
        sample_rate: track.maximum_sampling_rate,
        is_local: false,
        album_id: Some(album_id.to_string()),
        artist_id: track.performer.as_ref().map(|p| p.id),
        streamable: track.streamable,
        source: Some("qobuz".to_string()),
        parental_warning: track.parental_warning,
        source_item_id_hint: Some(album_id.to_string()),
    }
}

/// Build the album-level metadata (genre, release date, quality) captured
/// when an album is fetched for playback, so `record_recent` can stamp the
/// Recently Played card with the same genre + release date + quality badge
/// the Discover carousels show. Mirrors Tauri's `album_to_card_meta`, which
/// reads these straight off the `Album`.
fn album_card_meta(album: &qbz_models::Album) -> crate::recently::AlbumMeta {
    let genre = album
        .genre
        .as_ref()
        .map(|g| g.name.clone())
        .unwrap_or_default();
    let release_date = album.release_date_original.clone().unwrap_or_default();
    // The album summary carries its own max bit depth / sample rate, which
    // are more reliable than a single track's for the card badge.
    let (quality_tier, quality_label) =
        recent_quality(album.maximum_bit_depth, album.maximum_sampling_rate);
    crate::recently::AlbumMeta {
        genre,
        release_date,
        quality_tier,
        quality_label,
    }
}

/// Quality tier + exact-quality label from a queue track's bit depth /
/// sample rate, matching the discover card badge format.
fn recent_quality(bit_depth: Option<u32>, sample_rate: Option<f64>) -> (String, String) {
    let tier = match bit_depth {
        Some(d) if d >= 24 => "hires",
        Some(_) => "cd",
        None => "",
    };
    let label = match (bit_depth, sample_rate) {
        (Some(bd), Some(sr)) => {
            let t = if bd >= 24 { "Hi-Res" } else { "CD" };
            let rate = if (sr.fract()).abs() < f64::EPSILON {
                format!("{}", sr as i64)
            } else {
                format!("{sr}")
            };
            format!("{t}: {bd}-bit / {rate} kHz")
        }
        _ => String::new(),
    };
    (tier.to_string(), label)
}

/// Record the currently playing queue track in the recently-played store
/// so the Discover "Recently Played" sections fill.
async fn record_recent(runtime: &Runtime) {
    let state = runtime.core().get_queue_state().await;
    let Some(track) = state.current_track else {
        return;
    };
    let artwork = track.artwork_url.clone().unwrap_or_default();
    let album_id = track.album_id.clone().unwrap_or_default();
    // Prefer the album-level metadata captured at album-fetch time (genre,
    // release date, and the album's own max quality) over the single
    // track's values — the `album/get` track summaries are often partial.
    let meta = crate::recently::album_meta(&album_id).unwrap_or_default();
    let (track_tier, track_label) = recent_quality(track.bit_depth, track.sample_rate);
    let quality_tier = if !meta.quality_tier.is_empty() {
        meta.quality_tier
    } else {
        track_tier
    };
    let quality_label = if !meta.quality_label.is_empty() {
        meta.quality_label
    } else {
        track_label
    };
    crate::recently::record(crate::recently::RecentTrack {
        id: track.id.to_string(),
        title: track.title.clone(),
        subtitle: track.artist.clone(),
        artwork_url: artwork.clone(),
        album_id,
        album_title: track.album.clone(),
        album_artist: track.artist.clone(),
        album_artwork_url: artwork,
        quality_tier,
        quality_label,
        genre: meta.genre,
        release_date: meta.release_date,
        artist_id: track.artist_id,
    });
    // Per-artist play count — feeds the discovery filter "skip
    // artists I already know" (HavingCount > threshold). artist_id
    // is optional on QueueTrack; skip when absent.
    if let Some(artist_id) = track.artist_id {
        crate::play_history::record_play(artist_id, &track.artist);
    }
}

/// Play `album_id` from `start_index`: fetch the album, build the queue,
/// hand it to the core, and start audio on the start track.
pub fn play_album(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
    start_index: usize,
) {
    handle.spawn(async move {
        let album = match runtime.core().get_album(&album_id).await {
            Ok(album) => album,
            Err(e) => {
                log::error!("[qbz-slint] playback: get_album {album_id} failed: {e}");
                crate::toast::error_weak(&weak, "Couldn't load this album");
                return;
            }
        };

        let album_title = album.title.clone();
        let album_artist = album.artist.name.clone();
        let album_artwork = album.image.best().cloned().unwrap_or_default();
        // Cache the album's genre / release date / quality so the Recently
        // Played card the play records below carries them (no extra fetch).
        crate::recently::remember_album_meta(&album.id, album_card_meta(&album));

        let tracks: Vec<QueueTrack> = album
            .tracks
            .as_ref()
            .map(|container| container.items.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|track| {
                make_queue_track(
                    track,
                    &album.id,
                    &album_title,
                    &album_artist,
                    &album_artwork,
                )
            })
            .collect();

        if tracks.is_empty() {
            log::warn!("[qbz-slint] playback: album {album_id} has no tracks");
            crate::toast::error_weak(&weak, "This album has no playable tracks");
            return;
        }

        let start = start_index.min(tracks.len() - 1);
        let start_track_id = tracks[start].id;

        runtime.core().set_queue(tracks, Some(start)).await;
        after_track_change(&runtime, &weak, start_track_id).await;
        refresh_sidebar(true);
    });
}

/// Play the artist's top tracks as a fresh queue, starting at the
/// first track. Wired to the Popular Tracks "play all" CircleAction
/// in ArtistPageView. Re-fetches the artist page so the queue
/// carries the same audio metadata the page row uses.
pub fn play_artist_top_tracks(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    artist_id: String,
) {
    handle.spawn(async move {
        let id: u64 = match artist_id.parse() {
            Ok(id) => id,
            Err(_) => {
                log::warn!("[qbz-slint] play-top: invalid artist id {artist_id}");
                return;
            }
        };
        let page = match runtime.core().get_artist_page(id, None).await {
            Ok(page) => page,
            Err(e) => {
                log::error!("[qbz-slint] play-top: get_artist_page failed: {e}");
                crate::toast::error_weak(&weak, "Couldn't load this artist");
                return;
            }
        };
        let artist_name = page.name.display.clone();
        let tracks: Vec<QueueTrack> = page
            .top_tracks
            .unwrap_or_default()
            .into_iter()
            .map(|track| make_top_track_queue(track, &artist_name))
            .collect();
        if tracks.is_empty() {
            log::warn!("[qbz-slint] play-top: artist {artist_id} has no top tracks");
            crate::toast::error_weak(&weak, "No top tracks available for this artist");
            return;
        }
        let start_track_id = tracks[0].id;
        runtime.core().set_queue(tracks, Some(0)).await;
        after_track_change(&runtime, &weak, start_track_id).await;
        refresh_sidebar(true);
    });
}

/// Build a QueueTrack from a /artist/page top_tracks entry. The page
/// response carries a thinner audio_info than /album/get tracks; fall
/// back to sensible defaults when fields are absent.
fn make_top_track_queue(
    track: qbz_models::PageArtistTrack,
    artist_fallback: &str,
) -> QueueTrack {
    let audio = track.audio_info.as_ref();
    let album_id = track.album.as_ref().map(|a| a.id.clone());
    let album_title = track
        .album
        .as_ref()
        .map(|a| a.title.clone())
        .unwrap_or_default();
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|a| a.image.as_ref())
        .and_then(|img| img.best().cloned());
    let artist_name = track
        .artist
        .as_ref()
        .map(|a| a.name.display.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| artist_fallback.to_string());
    let artist_id = track.artist.as_ref().map(|a| a.id);
    let hires = audio
        .and_then(|a| a.maximum_bit_depth)
        .map(|b| b > 16)
        .unwrap_or(false);
    QueueTrack {
        id: track.id,
        title: track.title,
        version: track.version,
        artist: artist_name,
        album: album_title,
        duration_secs: track.duration.unwrap_or(0) as u64,
        artwork_url,
        hires,
        bit_depth: audio.and_then(|a| a.maximum_bit_depth),
        sample_rate: audio.and_then(|a| a.maximum_sampling_rate),
        is_local: false,
        album_id: album_id.clone(),
        artist_id,
        streamable: track
            .rights
            .as_ref()
            .and_then(|r| r.streamable)
            .unwrap_or(true),
        source: Some("qobuz".to_string()),
        parental_warning: track.parental_warning.unwrap_or(false),
        source_item_id_hint: album_id,
    }
}

/// Play a single track immediately as a one-track queue.
pub fn play_track_now(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    track_id: u64,
) {
    handle.spawn(async move {
        let track = match runtime.core().get_track(track_id).await {
            Ok(track) => track,
            Err(e) => {
                log::error!("[qbz-slint] playback: get_track {track_id} failed: {e}");
                return;
            }
        };

        let (album_id, album_title, album_artwork) = match track.album.as_ref() {
            Some(album) => (
                album.id.clone(),
                album.title.clone(),
                album.image.best().cloned().unwrap_or_default(),
            ),
            None => (String::new(), String::new(), String::new()),
        };
        let album_artist = track
            .performer
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default();

        let queue_track = make_queue_track(
            &track,
            &album_id,
            &album_title,
            &album_artist,
            &album_artwork,
        );

        runtime.core().set_queue(vec![queue_track], Some(0)).await;
        after_track_change(&runtime, &weak, track_id).await;
        refresh_sidebar(true);
    });
}

/// Build a queue from a list of catalog tracks (each carrying its own
/// album) and start playback at `start_index`. Shared by radio
/// (start 0) and the mix views (start at the clicked track).
pub fn play_tracks(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    tracks: Vec<qbz_models::Track>,
    start_index: usize,
) -> bool {
    let queue: Vec<QueueTrack> = tracks
        .iter()
        .map(|track| {
            let (album_id, album_title, album_artwork) = track
                .album
                .as_ref()
                .map(|a| {
                    (
                        a.id.clone(),
                        a.title.clone(),
                        a.image.best().cloned().unwrap_or_default(),
                    )
                })
                .unwrap_or_default();
            let album_artist = track
                .performer
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default();
            make_queue_track(track, &album_id, &album_title, &album_artist, &album_artwork)
        })
        .collect();
    if queue.is_empty() {
        return false;
    }
    let start = start_index.min(queue.len() - 1);
    let first_id = queue[start].id;
    handle.spawn(async move {
        runtime.core().set_queue(queue, Some(start)).await;
        after_track_change(&runtime, &weak, first_id).await;
        refresh_sidebar(true);
    });
    true
}

/// Build a queue from a Qobuz radio track list and start it.
fn play_radio_response(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    tracks: Vec<qbz_models::Track>,
) -> bool {
    let handle = tokio::runtime::Handle::current();
    play_tracks(runtime, weak, handle, tracks, 0)
}

/// Start a Qobuz artist radio (`/radio/artist`). Kept as the simpler
/// alternative to the smart pool builder; the artist "radio" action
/// uses the smart builder, this remains available for an explicit
/// "Qobuz radio" choice.
#[allow(dead_code)]
pub fn play_artist_radio(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    artist_id: String,
) {
    handle.spawn(async move {
        match runtime.core().get_radio_artist(&artist_id).await {
            Ok(resp) => {
                let tracks = resp.tracks.map(|p| p.items).unwrap_or_default();
                if !play_radio_response(runtime, weak, tracks) {
                    log::warn!("[qbz-slint] artist radio {artist_id} returned no tracks");
                }
            }
            Err(e) => log::error!("[qbz-slint] artist radio {artist_id} failed: {e}"),
        }
    });
}

/// Start a smart artist radio via the local qbz-radio pool builder
/// (richer than the plain Qobuz `/radio/artist`).
pub fn play_smart_artist_radio(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    artist_id: String,
) {
    handle.spawn(async move {
        let Ok(aid) = artist_id.parse::<u64>() else {
            log::warn!("[qbz-slint] smart radio: bad artist id {artist_id}");
            return;
        };
        match runtime.core().create_smart_artist_radio(aid).await {
            Ok(tracks) => {
                if !play_radio_response(runtime, weak, tracks) {
                    log::warn!("[qbz-slint] smart artist radio {aid} returned no tracks");
                }
            }
            Err(e) => log::error!("[qbz-slint] smart artist radio {aid} failed: {e}"),
        }
    });
}

/// Start a Qobuz track radio (`/radio/track`).
pub fn play_track_radio(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    track_id: String,
) {
    handle.spawn(async move {
        match runtime.core().get_radio_track(&track_id).await {
            Ok(resp) => {
                let tracks = resp.tracks.map(|p| p.items).unwrap_or_default();
                if !play_radio_response(runtime, weak, tracks) {
                    log::warn!("[qbz-slint] track radio {track_id} returned no tracks");
                }
            }
            Err(e) => log::error!("[qbz-slint] track radio {track_id} failed: {e}"),
        }
    });
}

/// Start a Qobuz album radio (`/radio/album`).
pub fn play_album_radio(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
) {
    handle.spawn(async move {
        match runtime.core().get_radio_album(&album_id).await {
            Ok(resp) => {
                let tracks = resp.tracks.map(|p| p.items).unwrap_or_default();
                if !play_radio_response(runtime, weak, tracks) {
                    log::warn!("[qbz-slint] album radio {album_id} returned no tracks");
                }
            }
            Err(e) => log::error!("[qbz-slint] album radio {album_id} failed: {e}"),
        }
    });
}

/// Enqueue an album's tracks at the end of the current queue.
pub fn enqueue_album(runtime: Runtime, _weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle, album_id: String) {
    handle.spawn(async move {
        let album = match runtime.core().get_album(&album_id).await {
            Ok(album) => album,
            Err(e) => {
                log::error!("[qbz-slint] playback: enqueue get_album {album_id} failed: {e}");
                return;
            }
        };
        let album_title = album.title.clone();
        let album_artist = album.artist.name.clone();
        let album_artwork = album.image.best().cloned().unwrap_or_default();
        crate::recently::remember_album_meta(&album.id, album_card_meta(&album));
        let tracks: Vec<QueueTrack> = album
            .tracks
            .as_ref()
            .map(|container| container.items.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|track| {
                make_queue_track(track, &album.id, &album_title, &album_artist, &album_artwork)
            })
            .collect();
        if tracks.is_empty() {
            return;
        }
        runtime.core().add_tracks(tracks).await;
        refresh_sidebar(false);
    });
}

/// Insert an album's tracks immediately after the current track ("Play next").
///
/// The core's `add_track_next` inserts a single track after the current index,
/// so the album tracks are inserted in reverse order to land in the right
/// sequence — mirroring Tauri's `v2_add_tracks_to_queue_next`.
pub fn enqueue_album_next(
    runtime: Runtime,
    _weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
) {
    handle.spawn(async move {
        let album = match runtime.core().get_album(&album_id).await {
            Ok(album) => album,
            Err(e) => {
                log::error!("[qbz-slint] playback: play-next get_album {album_id} failed: {e}");
                return;
            }
        };
        let album_title = album.title.clone();
        let album_artist = album.artist.name.clone();
        let album_artwork = album.image.best().cloned().unwrap_or_default();
        crate::recently::remember_album_meta(&album.id, album_card_meta(&album));
        let tracks: Vec<QueueTrack> = album
            .tracks
            .as_ref()
            .map(|container| container.items.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|track| {
                make_queue_track(track, &album.id, &album_title, &album_artist, &album_artwork)
            })
            .collect();
        if tracks.is_empty() {
            return;
        }
        // Insert in reverse so the tracks end up in the correct order.
        for track in tracks.into_iter().rev() {
            runtime.core().add_track_next(track).await;
        }
        refresh_sidebar(false);
    });
}

/// Enqueue a single track at the end of the current queue.
pub fn enqueue_track(runtime: Runtime, _weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle, track_id: u64) {
    handle.spawn(async move {
        let track = match runtime.core().get_track(track_id).await {
            Ok(track) => track,
            Err(e) => {
                log::error!("[qbz-slint] playback: enqueue get_track {track_id} failed: {e}");
                return;
            }
        };
        let (album_id, album_title, album_artwork) = match track.album.as_ref() {
            Some(album) => (
                album.id.clone(),
                album.title.clone(),
                album.image.best().cloned().unwrap_or_default(),
            ),
            None => (String::new(), String::new(), String::new()),
        };
        let album_artist = track
            .performer
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let queue_track =
            make_queue_track(&track, &album_id, &album_title, &album_artist, &album_artwork);
        runtime.core().add_track(queue_track).await;
        refresh_sidebar(false);
    });
}

/// Insert a single track immediately after the current track ("Play next").
pub fn play_track_next(
    runtime: Runtime,
    _weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    track_id: u64,
) {
    handle.spawn(async move {
        let track = match runtime.core().get_track(track_id).await {
            Ok(track) => track,
            Err(e) => {
                log::error!("[qbz-slint] playback: play-next get_track {track_id} failed: {e}");
                return;
            }
        };
        let (album_id, album_title, album_artwork) = match track.album.as_ref() {
            Some(album) => (
                album.id.clone(),
                album.title.clone(),
                album.image.best().cloned().unwrap_or_default(),
            ),
            None => (String::new(), String::new(), String::new()),
        };
        let album_artist = track
            .performer
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let queue_track =
            make_queue_track(&track, &album_id, &album_title, &album_artist, &album_artwork);
        runtime.core().add_track_next(queue_track).await;
        refresh_sidebar(false);
    });
}

/// Append (or insert-next) a batch of already-fetched tracks to the queue
/// without re-fetching them. Used by the favorites bulk bar.
pub fn enqueue_tracks(
    runtime: Runtime,
    handle: tokio::runtime::Handle,
    tracks: Vec<qbz_models::Track>,
    next: bool,
) {
    if tracks.is_empty() {
        return;
    }
    handle.spawn(async move {
        // For "play next" each insert lands right after the current track,
        // so reverse the batch to preserve the selection's order.
        let ordered: Vec<qbz_models::Track> = if next {
            tracks.into_iter().rev().collect()
        } else {
            tracks
        };
        for track in ordered {
            let (album_id, album_title, album_artwork) = track
                .album
                .as_ref()
                .map(|a| {
                    (
                        a.id.clone(),
                        a.title.clone(),
                        a.image.best().cloned().unwrap_or_default(),
                    )
                })
                .unwrap_or_default();
            let album_artist = track
                .performer
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let qt =
                make_queue_track(&track, &album_id, &album_title, &album_artist, &album_artwork);
            if next {
                runtime.core().add_track_next(qt).await;
            } else {
                runtime.core().add_track(qt).await;
            }
        }
        refresh_sidebar(false);
    });
}

/// Append (or insert-next) a batch of already-loaded LocalLibrary rows to the
/// queue. Mirrors `enqueue_tracks` but for `LocalTrack`: `local_queue_track`
/// builds source-aware QueueTracks (is_local=true; "local"/"qobuz_download")
/// so `play_audible` routes user files through the protected `play_data` seam
/// and offline copies through `play_track_resolved`. Reversed for "play next"
/// to preserve selection order.
pub fn enqueue_local_tracks(
    runtime: Runtime,
    handle: tokio::runtime::Handle,
    tracks: Vec<qbz_library::LocalTrack>,
    next: bool,
) {
    if tracks.is_empty() {
        return;
    }
    handle.spawn(async move {
        let ordered: Vec<qbz_library::LocalTrack> = if next {
            tracks.into_iter().rev().collect()
        } else {
            tracks
        };
        for track in &ordered {
            let qt = local_queue_track(track);
            if next {
                runtime.core().add_track_next(qt).await;
            } else {
                runtime.core().add_track(qt).await;
            }
        }
        refresh_sidebar(false);
    });
}

/// Toggle play / pause on the live player.
pub fn toggle_play_pause(runtime: Runtime, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        let playing = runtime.core().get_playback_state().is_playing;
        let result = if playing {
            runtime.core().pause()
        } else {
            runtime.core().resume()
        };
        if let Err(e) = result {
            log::error!("[qbz-slint] playback: toggle play/pause failed: {e}");
        }
    });
}

/// Advance to the next queue track and play it.
pub fn next(runtime: Runtime, weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        let Some(track) = runtime.core().next_track().await else {
            log::info!("[qbz-slint] playback: end of queue");
            return;
        };
        let track_id = track.id;
        after_track_change(&runtime, &weak, track_id).await;
        refresh_sidebar(true);
    });
}

/// Go to the previous queue track and play it.
pub fn previous(runtime: Runtime, weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        let Some(track) = runtime.core().previous_track().await else {
            log::info!("[qbz-slint] playback: start of queue");
            return;
        };
        let track_id = track.id;
        after_track_change(&runtime, &weak, track_id).await;
        refresh_sidebar(true);
    });
}

/// Seek to `fraction` (0..1) of the current track's duration.
pub fn seek(runtime: Runtime, handle: tokio::runtime::Handle, fraction: f32) {
    handle.spawn(async move {
        let state = runtime.core().get_playback_state();
        if state.duration == 0 {
            return;
        }
        let fraction = fraction.clamp(0.0, 1.0);
        let position = (fraction as f64 * state.duration as f64).round() as u64;
        if let Err(e) = runtime.core().seek(position) {
            log::error!("[qbz-slint] playback: seek failed: {e}");
        }
    });
}

/// Mute state and the volume to restore on unmute. `PREMUTE_VOLUME`
/// holds the f32 level as bits; `MUTED` is the authoritative flag.
static MUTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PREMUTE_VOLUME: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Set the player volume from `fraction` (0..1). A non-zero level clears
/// any active mute, so dragging the slider or stepping volume unmutes.
pub fn set_volume(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    fraction: f32,
) {
    handle.spawn(async move {
        let fraction = fraction.clamp(0.0, 1.0);
        if let Err(e) = runtime.core().set_volume(fraction) {
            log::error!("[qbz-slint] playback: set_volume failed: {e}");
        }
        if fraction > 0.0 && MUTED.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<NowPlayingState>().set_muted(false);
            });
        }
    });
}

/// Toggle mute: silence the player and remember the level, or restore it.
pub fn toggle_mute(runtime: Runtime, weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    use std::sync::atomic::Ordering;
    handle.spawn(async move {
        if MUTED.swap(false, Ordering::Relaxed) {
            // Unmute — restore the stored level.
            let restored = f32::from_bits(PREMUTE_VOLUME.load(Ordering::Relaxed));
            let restored = if restored > 0.0 { restored } else { 0.7 };
            let _ = runtime.core().set_volume(restored);
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<NowPlayingState>().set_muted(false);
            });
        } else {
            // Mute — stash the current level, then drop to zero.
            let current = runtime.core().player().get_playback_event().volume;
            let current = if current > 0.0 { current } else { 0.7 };
            PREMUTE_VOLUME.store(current.to_bits(), Ordering::Relaxed);
            MUTED.store(true, Ordering::Relaxed);
            let _ = runtime.core().set_volume(0.0);
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<NowPlayingState>().set_muted(true);
            });
        }
    });
}

/// Toggle shuffle on the queue and reflect the new state on NowPlayingState.
pub fn toggle_shuffle(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    handle.spawn(async move {
        let on = runtime.core().toggle_shuffle().await;
        let _ = weak.upgrade_in_event_loop(move |w| {
            w.global::<NowPlayingState>().set_shuffle(on);
        });
    });
}

/// Cycle the repeat mode Off -> All -> One -> Off and reflect it.
pub fn cycle_repeat(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    handle.spawn(async move {
        let next = match runtime.core().get_queue_state().await.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        runtime.core().set_repeat_mode(next).await;
        let mode: i32 = match next {
            RepeatMode::Off => 0,
            RepeatMode::All => 1,
            RepeatMode::One => 2,
        };
        let _ = weak.upgrade_in_event_loop(move |w| {
            w.global::<NowPlayingState>().set_repeat_mode(mode);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_pads_seconds() {
        assert_eq!(fmt_elapsed(0), "0:00");
        assert_eq!(fmt_elapsed(9), "0:09");
        assert_eq!(fmt_elapsed(65), "1:05");
        assert_eq!(fmt_elapsed(605), "10:05");
    }

    #[test]
    fn remaining_counts_down_and_pads() {
        assert_eq!(fmt_remaining(0, 200), "-3:20");
        assert_eq!(fmt_remaining(195, 200), "-0:05");
        assert_eq!(fmt_remaining(200, 200), "-0:00");
        // Position past duration must not underflow.
        assert_eq!(fmt_remaining(250, 200), "-0:00");
    }

}

/// Start the playback poll loop. Runs for the app lifetime: every ~450ms
/// it reads the player event and pushes position / progress onto
/// `NowPlayingState`. When a track ends it auto-advances the queue.
pub fn start_poll_loop(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    let spawn_handle = handle.clone();
    spawn_handle.spawn(async move {
        // Track whether the last poll observed an active track, so the
        // end-of-track edge is detected once rather than every tick.
        let mut last_track_id: u64 = 0;
        let mut was_playing = false;
        let mut seen_position: u64 = 0;
        // Track id we have already fired a gapless prefetch for, so the
        // 450ms ticker does not re-request it every tick.
        let mut gapless_requested_for: u64 = 0;

        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(450));
        loop {
            ticker.tick().await;

            let event = runtime.core().player().get_playback_event();

            let track_id = event.track_id;
            let position = event.position;
            let duration = event.duration;
            let is_playing = event.is_playing;
            let volume = event.volume;
            // Streaming buffer fill, for the seek-bar cache overlay.
            let cache = event.buffer_progress.unwrap_or(0.0);

            // --- Seamless gapless transition detection -------------------
            // When the audio engine performs a gapless handoff the track
            // changes WITHOUT a stop: `track_id` becomes the previously
            // gapless-queued id while `is_playing` stays true. Detect that
            // edge — a track-id change while still playing, where the new
            // id is not the end-of-track edge — and sync the core queue
            // pointer + refresh metadata WITHOUT calling the audible play
            // path (the player is already playing it).
            let seamless_change = track_id != 0
                && last_track_id != 0
                && track_id != last_track_id
                && is_playing
                && was_playing;
            if seamless_change {
                // A track-id change while still playing is EITHER a real
                // gapless hand-off (the engine started the prefetched next
                // track) OR a manual new-track play that just replaced the
                // queue. Only the former should advance the core queue
                // pointer: a real gapless next IS the current upcoming track.
                // For a manual play the queue is already correct, and calling
                // next_track() would push the pointer one past what's actually
                // playing — desyncing now-playing from the queue (the reported
                // erratic mismatch).
                let is_gapless_advance = runtime
                    .core()
                    .peek_upcoming(1)
                    .await
                    .first()
                    .map(|t| t.id)
                    == Some(track_id);
                if is_gapless_advance {
                    log::info!(
                        "[qbz-slint] [GAPLESS] seamless transition {last_track_id} -> {track_id}"
                    );
                    let _ = runtime.core().next_track().await;
                    refresh_now_playing_meta(&runtime, &weak).await;
                    record_recent(&runtime).await;
                    refresh_sidebar(true);
                    // Prefetch the successors of the now-current track.
                    kick_prefetch(&runtime).await;
                    gapless_requested_for = 0;
                }
                // Resync the edge trackers either way so this change is not
                // re-detected on the next tick.
                last_track_id = track_id;
                seen_position = position;
                was_playing = is_playing;
                continue;
            }

            // --- Gapless prefetch trigger --------------------------------
            // When the engine signals it wants the next track pre-queued
            // (`gapless_ready`) and nothing is queued yet
            // (`gapless_next_track_id == 0`), resolve the next upcoming
            // queue track, fetch its bytes (L1 -> L2 -> CMAF download),
            // and hand them to `Player::play_next`. The
            // `gapless_requested_for` guard stops the 450ms ticker from
            // re-firing while the download is in flight.
            if event.gapless_ready
                && event.gapless_next_track_id == 0
                && track_id != 0
                && gapless_requested_for != track_id
            {
                let upcoming = runtime.core().peek_upcoming(1).await;
                if let Some(next) = upcoming.into_iter().next() {
                    // Never queue the current track as its own next.
                    if next.id != track_id && !next.is_local {
                        gapless_requested_for = track_id;
                        let runtime = runtime.clone();
                        let weak = weak.clone();
                        let next_id = next.id;
                        tokio::spawn(async move {
                            // Shared tier-walk: L1/L2 (player cache) -> offline
                            // -> network, then hand the bytes to play_next.
                            let offline = crate::offline::get().await;
                            let sink = crate::offline_cache::row_sink(weak.clone());
                            if let Some(data) = runtime
                                .core()
                                .fetch_for_gapless_resolved(
                                    next_id,
                                    PLAYBACK_QUALITY,
                                    offline.as_deref(),
                                    Some(&sink),
                                )
                                .await
                            {
                                let player = runtime.core().player();
                                if let Err(e) = player.play_next(data, next_id) {
                                    log::warn!(
                                        "[qbz-slint] [GAPLESS] play_next {next_id} failed: {e}"
                                    );
                                } else {
                                    log::info!(
                                        "[qbz-slint] [GAPLESS] queued track {next_id} for gapless"
                                    );
                                }
                            }
                        });
                    }
                }
            }

            // Detect end-of-track: there was a track, it has reached the
            // end (position within the duration) and is no longer playing.
            let track_ended = was_playing
                && !is_playing
                && last_track_id != 0
                && (track_id == 0 || track_id == last_track_id)
                && duration > 0
                && seen_position + 2 >= duration;

            // Push the live values onto NowPlayingState.
            let progress = if duration > 0 {
                (position as f32 / duration as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let elapsed = fmt_elapsed(position);
            let remaining = fmt_remaining(position, duration);
            let _ = weak.upgrade_in_event_loop(move |w| {
                let np = w.global::<NowPlayingState>();
                np.set_position_secs(position as i32);
                if duration > 0 {
                    np.set_duration_secs(duration as i32);
                }
                np.set_progress(progress);
                np.set_cache(cache);
                np.set_elapsed(elapsed.into());
                np.set_remaining(remaining.into());
                np.set_playing(is_playing);
                np.set_volume(volume.clamp(0.0, 1.0));
            });

            if track_id != 0 {
                last_track_id = track_id;
                seen_position = position;
            }
            was_playing = is_playing;

            // Auto-advance on track end.
            if track_ended {
                last_track_id = 0;
                was_playing = false;
                seen_position = 0;
                gapless_requested_for = 0;
                if let Some(track) = runtime.core().next_track().await {
                    let next_id = track.id;
                    after_track_change(&runtime, &weak, next_id).await;
                    refresh_sidebar(true);
                } else {
                    log::info!("[qbz-slint] playback: queue finished");
                    let _ = weak.upgrade_in_event_loop(|w| {
                        let np = w.global::<NowPlayingState>();
                        np.set_playing(false);
                        np.set_progress(0.0);
                        np.set_position_secs(0);
                    });
                }
            }
        }
    });
}
