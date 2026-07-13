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
use qbz_models::{Quality, QueueTrack, RepeatMode, Track};
use qconnect_app::renderer::{PLAYING_STATE_PAUSED, PLAYING_STATE_PLAYING};
use slint::{ComponentHandle, Model, ModelRc};

use crate::adapter::SlintAdapter;
use crate::queue::QueueController;
use crate::{
    AlbumState, AppWindow, ArtistState, ContentView, FavoritesState, ImmersiveState, LabelState,
    NavState, NowPlayingState, PlaylistState, PurchaseDetailState, PurchasesState, SearchState,
    TrackItem,
};

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
pub(crate) fn refresh_sidebar(with_favorites: bool) {
    if let Some(controller) = QUEUE_CONTROLLER.get() {
        if with_favorites {
            controller.refresh_with_favorites();
        } else {
            controller.refresh();
        }
    }
}

/// Apply Plex quality updates to any queued track (by `rating_key`) and, if
/// the CURRENTLY-playing track was among them, re-push the now-playing stamp so
/// the player-bar quality badge agrees with the freshly-hydrated value. Reaches
/// the runtime through the global queue controller (the hydration path runs in
/// a detail-view context that does not carry the runtime). No-op before the
/// controller is registered or when nothing matches. `updates` is
/// `(rating_key, bit_depth, sample_rate_khz)`.
pub fn apply_plex_quality_to_queue(updates: Vec<(String, Option<u32>, Option<f64>)>) {
    if updates.is_empty() {
        return;
    }
    let Some(controller) = QUEUE_CONTROLLER.get() else {
        return;
    };
    let runtime = controller.runtime().clone();
    let weak = controller.weak().clone();
    controller.handle().spawn(async move {
        let current_patched = runtime.core().patch_plex_queue_quality(&updates).await;
        if current_patched {
            refresh_now_playing_meta(&runtime, &weak).await;
            // Keep the invariant: every refresh_now_playing_meta is paired with a
            // sidebar repaint so QueueState.now-playing (quality badge included)
            // never lags the bar. Same-track patch, so `false` (no fav pull).
            refresh_sidebar(false);
        }
    });
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
    // Persist the session (queue + current track + position) so a restart can
    // restore it. No-op unless `persist_session` is on.
    crate::session_persist::capture_and_save(runtime).await;
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
    // Offline: the prefetch is a pure NETWORK warmer (offline-cached tracks
    // play through the offline tier without it), so skip entirely — every
    // attempt would just bounce off the API offline gate and spam the log.
    if crate::offline_mode::engine().is_offline() {
        return;
    }
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
                .prefetch_into_cache(client, track_id, playback_quality())
                .await
            {
                log::debug!("[qbz-slint] prefetch: track {track_id} failed: {e}");
            }
        });
    }
}

/// Streaming quality for playback, resolved at playback time from the
/// persisted Settings preference (`ui_prefs.streaming_quality`, the
/// Settings > Audio dropdown). Unset/unknown keys fall back to the
/// highest tier (Hi-Res+ = `Quality::UltraHiRes`, the previous hardcoded
/// behavior); the player still falls back internally when the requested
/// tier is not available (#590).
fn playback_quality() -> Quality {
    crate::ui_prefs::streaming_quality_for_key(&crate::ui_prefs::load().streaming_quality)
}

/// Convenience alias for the runtime handle threaded through every call.
type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// The track id whose audible fetch/resolve is currently in flight (the
/// "loading" track). Set the instant a play is initiated (top of
/// `play_audible`, before the multi-second Plex/Qobuz/local resolve) and
/// read by the poll loop to clear the spinner once THAT track's audio is
/// actually advancing. A NEW play overwrites it, so a superseded fetch never
/// keeps the spinner up for the wrong track. `0` = nothing loading.
static PENDING_PLAY_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Epoch-millis when the in-flight play was initiated — the poll-loop watchdog
/// force-clears the spinner if audio never starts within `LOADING_WATCHDOG_MS`
/// (a play the engine accepted but that silently never advances — e.g. an
/// undecodable-but-valid-looking file — would otherwise spin forever).
static PENDING_PLAY_AT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Generous ceiling: a real fetch (even a large hi-res Plex whole-file
/// download on a slow LAN) starts audio well under this; only a silently-stuck
/// play crosses it.
const LOADING_WATCHDOG_MS: u64 = 45_000;

/// Mark `track_id` as the in-flight play and raise the now-playing "loading"
/// flag (drives the fetch spinner on the bar, the active track row, and the
/// album play button). Source-agnostic — covers Plex (~10s resolve), the
/// Qobuz tier-walk, and slow local reads.
fn set_loading(weak: &slint::Weak<AppWindow>, track_id: u64) {
    PENDING_PLAY_ID.store(track_id, std::sync::atomic::Ordering::Relaxed);
    PENDING_PLAY_AT_MS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<NowPlayingState>().set_loading(true);
    });
}

/// Clear the loading flag if (and only if) the in-flight play is still
/// `track_id` — so a fetch that has been superseded by a newer play does not
/// wipe the newer play's spinner. Pass `0` to force-clear unconditionally
/// (queue finished / hard stop).
fn clear_loading(weak: &slint::Weak<AppWindow>, track_id: u64) {
    if track_id != 0
        && PENDING_PLAY_ID.load(std::sync::atomic::Ordering::Relaxed) != track_id
    {
        return;
    }
    PENDING_PLAY_ID.store(0, std::sync::atomic::Ordering::Relaxed);
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<NowPlayingState>().set_loading(false);
    });
}

/// Maximum consecutive offline-unavailable tracks the queue walk skips
/// before giving up (Tauri #467 parity: `MAX_OFFLINE_SKIPS = 5`).
const MAX_OFFLINE_SKIPS: usize = 5;

/// Consecutive auto-skips over tracks whose play failed with a TERMINAL
/// "unavailable" error (Tauri #467 parity: the Svelte playbackService kept
/// `consecutiveSkips` capped at `MAX_CONSECUTIVE_SKIPS = 5`). Reset by the
/// poll loop the moment any track actually starts producing audio.
static UNAVAILABLE_SKIPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
const MAX_UNAVAILABLE_SKIPS: u32 = 5;

/// True when a stringified play error means the track cannot play now or
/// ever at any quality — as opposed to a transient network/server failure
/// (those are already retried with backoff inside the client and must NOT
/// cost the user a good track). The `ApiError` Display texts survive the
/// `Result<(), String>` flattening in `Player::play_track` ("Failed to get
/// stream URL: {ApiError}"), so a substring match is the same pragmatic
/// contract the Tauri frontend used (`errorStr.includes(...)`).
fn is_terminal_unavailable(e: &str) -> bool {
    e.contains("no longer available") // ApiError::TrackUnavailable
        || e.contains("not streamable") // ApiError::NonStreamable
        || e.contains("No valid quality available") // ApiError::NoQualityAvailable
}

/// Offline playability verdict for one queue track (offline-MODE slice 3d).
#[derive(PartialEq)]
enum OfflinePlayability {
    Playable,
    /// No offline source for this track (Qobuz without a cached copy, or
    /// Plex under REAL offline).
    Unavailable,
    /// The track IS offline-cached but the D4 subscription grace window has
    /// elapsed — gets its own honest message.
    GraceExpired,
    /// A LOCAL user file whose indexed path is not on disk right now —
    /// typically an unmounted network drive. Checked online AND offline
    /// (library content is never hidden, so playback is where this must
    /// surface) — gets the "is the drive mounted?" message.
    FileMissing,
}

/// Cheap existence guard for a LOCAL queue track's underlying file: resolve
/// the indexed path (ephemeral store, or one indexed library-DB read) and
/// stat it with `Path::exists()`. Unresolvable id/path → `true` (don't
/// invent a skip; the play path has its own not-found handling).
///
/// D-STATE CAVEAT: `exists()` on an UNMOUNTED path returns false instantly
/// (the path simply isn't there) — that is the case this guards. But a stat
/// on a DEAD-yet-still-MOUNTED NFS/CIFS share can block in uninterruptible
/// sleep (D state). This is therefore only ever called from the async layer
/// (the advance walk and the play fast-fail run on the tokio runtime;
/// `play_local_file_audible` checks inside `spawn_blocking`) and NEVER from
/// the audio callback thread — a worst-case hang stalls an advance, not the
/// audio pipeline. Do NOT add mount probing here; the hot path stays one
/// stat per advance.
fn local_track_file_exists(track: &QueueTrack) -> bool {
    let path = if crate::ephemeral::is_ephemeral_id(track.id as i64) {
        crate::ephemeral::get_track(track.id as i64).map(|row| row.file_path)
    } else {
        crate::library_db::with_db(|db| db.get_track(track.id as i64))
            .flatten()
            .map(|row| row.file_path)
    };
    match path {
        Some(p) => std::path::Path::new(&p).exists(),
        None => true,
    }
}

/// Decide whether `track` can play under the CURRENT offline status.
/// Local / ephemeral user files → existence-checked regardless of
/// online/offline (the library never hides network-folder content — see
/// local_library.rs's NETWORK-FOLDER VISIBILITY note — so an unmounted
/// drive is caught here, at playback time).
/// Online → otherwise always playable (the normal path pays one status read).
/// Offline:
/// - plex → induced offline only (a LAN Plex server may be reachable;
///   under real offline it is not — Tauri parity)
/// - qobuz (incl. "qobuz_download" copies, which keep the real Qobuz id)
///   → offline-cached AND within the D4 subscription grace window
fn offline_playability(track: &QueueTrack) -> OfflinePlayability {
    if matches!(track.source.as_deref(), Some("local") | Some("ephemeral")) {
        return if local_track_file_exists(track) {
            OfflinePlayability::Playable
        } else {
            OfflinePlayability::FileMissing
        };
    }
    let status = crate::offline_mode::engine().status();
    if !status.is_offline() {
        return OfflinePlayability::Playable;
    }
    if track.is_local {
        return OfflinePlayability::Playable;
    }
    match track.source.as_deref() {
        // ("local" / "ephemeral" never reach here — handled above.)
        Some("plex") => {
            if status.mode == qbz_app::offline_mode::OfflineMode::InducedOffline {
                OfflinePlayability::Playable
            } else {
                OfflinePlayability::Unavailable
            }
        }
        _ => {
            if !crate::offline_cache::is_cached(&track.id.to_string()) {
                OfflinePlayability::Unavailable
            } else if !crate::offline_mode::offline_playback_allowed() {
                OfflinePlayability::GraceExpired
            } else {
                OfflinePlayability::Playable
            }
        }
    }
}

/// Boolean form of [`offline_playability`] for the advance/prefetch walks.
fn offline_track_playable(track: &QueueTrack) -> bool {
    offline_playability(track) == OfflinePlayability::Playable
}

/// Move the queue cursor forward/backward to the next playable track.
/// Online this returns the immediate neighbor on the first iteration unless
/// that neighbor is a LOCAL file whose path is gone (unmounted drive) — the
/// only possible online skip. Offline it also skips unavailable tracks.
/// Bounded at [`MAX_OFFLINE_SKIPS`] consecutive (Tauri #467 parity); on
/// exhaustion (bound hit, or queue edge after at least one skip) playback
/// stops and ONE toast reports it — worded for the drive when every skip was
/// a missing local file, for offline otherwise.
///
/// The gapless-prefetched target never passes through here: a gapless
/// hand-off happens inside the audio engine and surfaces to the poll loop
/// as a seamless track-id change (no advance call), so the "never skip the
/// gapless target" exemption is structural.
async fn advance_to_playable(
    runtime: &Runtime,
    weak: &slint::Weak<AppWindow>,
    forward: bool,
) -> Option<QueueTrack> {
    let mut skips = 0usize;
    let mut missing_files = 0usize;
    // One message for the whole walk: when every skipped track was a local
    // file that isn't on disk, point at the drive; any other mix keeps the
    // offline wording (online, FileMissing is the only possible skip).
    let walk_toast = |skips: usize, missing_files: usize| {
        if missing_files == skips {
            qbz_i18n::t("Files not available — is the drive mounted?")
        } else {
            qbz_i18n::t("No tracks available offline")
        }
    };
    loop {
        let step = if forward {
            runtime.core().next_track().await
        } else {
            runtime.core().previous_track().await
        };
        let Some(track) = step else {
            // Queue edge. Quiet when nothing was skipped (the normal end of
            // queue); one toast when the walk dropped tracks on the way.
            if skips > 0 {
                crate::toast::show_weak(
                    weak,
                    walk_toast(skips, missing_files),
                    crate::ToastKind::Warning,
                );
            }
            return None;
        };
        match offline_playability(&track) {
            OfflinePlayability::Playable => return Some(track),
            OfflinePlayability::FileMissing => missing_files += 1,
            _ => {}
        }
        skips += 1;
        log::info!(
            "[qbz-slint] advance: skipping unavailable track {} ({skips}/{MAX_OFFLINE_SKIPS})",
            track.id
        );
        if skips >= MAX_OFFLINE_SKIPS {
            if let Err(e) = runtime.core().stop() {
                log::warn!("[qbz-slint] advance: stop after skip bound failed: {e}");
            }
            crate::toast::show_weak(
                weak,
                walk_toast(skips, missing_files),
                crate::ToastKind::Warning,
            );
            return None;
        }
    }
}

/// When the queue is exhausted and `InfiniteRadio` autoplay is on, build a
/// smart artist radio seeded by the just-finished track and start it,
/// replacing the spent queue. Returns `true` if a radio was started.
///
/// Tauri appends the radio then retries `next()`; that can't be ported 1:1
/// because `QueueManager::next()` nulls `current_index` at the queue edge, so a
/// retry replays the old queue from index 1 rather than the radio. Starting the
/// radio fresh via `play_tracks` reaches Tauri's *intended* behavior and chains
/// correctly — each radio's end re-seeds the next, giving true infinite play.
async fn try_infinite_refill(
    runtime: &Runtime,
    weak: &slint::Weak<AppWindow>,
    seed_track_id: u64,
) -> bool {
    let Some(controller) = QUEUE_CONTROLLER.get() else {
        return false;
    };
    if !controller.is_infinite_play() || seed_track_id == 0 {
        return false;
    }
    let artist_id = match runtime.core().get_track(seed_track_id).await {
        Ok(track) => match track.performer.as_ref().map(|p| p.id) {
            Some(id) => id,
            None => {
                log::warn!(
                    "[qbz-slint] infinite radio: seed track {seed_track_id} has no performer"
                );
                return false;
            }
        },
        Err(e) => {
            log::warn!("[qbz-slint] infinite radio: get_track {seed_track_id} failed: {e}");
            return false;
        }
    };
    match runtime.core().create_smart_artist_radio(artist_id).await {
        // play_tracks replaces the spent queue and starts the radio; it already
        // drops blacklisted tracks, so its `false` means nothing playable.
        Ok(tracks) if !tracks.is_empty() => play_tracks(
            runtime.clone(),
            weak.clone(),
            tokio::runtime::Handle::current(),
            tracks,
            0,
        ),
        Ok(_) => false,
        Err(e) => {
            log::warn!("[qbz-slint] infinite radio: build failed: {e}");
            false
        }
    }
}

/// Run the audible step for `track_id`: grab the Qobuz client and call
/// the player's self-contained `play_track`. Errors are logged, not
/// surfaced — the poll loop keeps the UI consistent regardless.
async fn play_audible(runtime: &Runtime, weak: &slint::Weak<AppWindow>, track_id: u64) {
    // Offline fast-fail (slice 3d): refuse unplayable tracks BEFORE the
    // spinner/fetch. Every explicit play path (album/track/playlist/radio)
    // funnels through here after moving the queue cursor; the advance walks
    // pre-filter via `advance_to_playable`, so a refusal here means the user
    // explicitly picked an unavailable track.
    if crate::offline_mode::engine().is_offline() {
        if let Some(qt) = runtime.core().current_track().await {
            if qt.id == track_id {
                match offline_playability(&qt) {
                    OfflinePlayability::Playable => {}
                    OfflinePlayability::GraceExpired => {
                        log::info!(
                            "[qbz-slint] offline: refused track {track_id} (subscription grace expired)"
                        );
                        crate::toast::show_weak(
                            weak,
                            qbz_i18n::t("Offline listening period expired — reconnect to verify your subscription"),
                            crate::ToastKind::Warning,
                        );
                        return;
                    }
                    OfflinePlayability::Unavailable => {
                        log::info!(
                            "[qbz-slint] offline: refused track {track_id} (not available offline)"
                        );
                        crate::toast::show_weak(
                            weak,
                            qbz_i18n::t("Track not available offline"),
                            crate::ToastKind::Warning,
                        );
                        return;
                    }
                    OfflinePlayability::FileMissing => {
                        log::info!(
                            "[qbz-slint] local play: refused track {track_id} (file missing — unmounted drive?)"
                        );
                        crate::toast::show_weak(
                            weak,
                            qbz_i18n::t("File not available — is the drive mounted?"),
                            crate::ToastKind::Warning,
                        );
                        return;
                    }
                }
            }
        }
    }
    // Raise the fetch spinner the instant playback is requested — BEFORE the
    // resolve/download/buffer below (the Plex resolve alone is ~10s). The bar
    // already adopted the new track meta in `refresh_now_playing_meta`; this
    // bridges the silent gap until the poll loop sees the audio advancing.
    set_loading(weak, track_id);
    // CAST (Chromecast / DLNA): when a renderer is connected it owns playback —
    // route the new track to the renderer instead of starting the local audio
    // backend (no double audio). Takes priority over QConnect below.
    if let Some(cast) = crate::cast_service::service() {
        if cast.is_casting().await {
            if let Some(qt) = runtime.core().current_track().await {
                if let Err(e) = cast.cast_track(&qt).await {
                    log::warn!("[Cast] play new track {track_id} failed: {e}");
                    crate::toast::show_weak(weak, qbz_i18n::t("Failed to cast track"), crate::ToastKind::Error);
                }
            }
            clear_loading(weak, track_id);
            return;
        }
    }
    // QConnect CONTROLLER mode: when a PEER renderer owns playback, route the
    // new play to the peer instead of playing locally. `play_on_peer_if_active`
    // returns false in every non-controller situation (disconnected, renderer
    // mode where active == local, no peer), so the existing local path below
    // runs byte-unchanged and renderer / local playback do not regress.
    if let Some(svc) = crate::qconnect_service::service() {
        if svc.play_on_peer_if_active(track_id).await {
            // A peer owns audio: there is no local fetch wait, so drop the
            // spinner immediately (the peer-state branch in the poll loop owns
            // the bar from here).
            clear_loading(weak, track_id);
            return;
        }
    }
    // Source-aware: a LOCAL user file plays from disk via the play_data seam.
    // Offline-cached + Qobuz keep the existing tier-walk below (unchanged), so
    // streaming playback can't regress. The current queue track tells us which
    // path to take via its `source`; the id guard avoids mis-routing when the
    // current track and `track_id` momentarily disagree. Auto-advance, skip and
    // play-all all flow through here, so they become source-aware for free.
    if let Some(qt) = runtime.core().current_track().await {
        if qt.id == track_id {
            match qt.source.as_deref() {
                Some("local") | Some("ephemeral") => {
                    play_local_file_audible(runtime, weak, track_id).await;
                    return;
                }
                Some("plex") => {
                    // The string rating_key rides in `source_item_id_hint`;
                    // fall back to the numeric id (= rating_key for the common
                    // numeric-key case) if the hint is absent.
                    let rating_key = qt
                        .source_item_id_hint
                        .clone()
                        .unwrap_or_else(|| track_id.to_string());
                    play_plex_audible(runtime, weak, track_id, rating_key, qt.duration_secs)
                        .await;
                    return;
                }
                _ => {}
            }
        }
    }
    // Offline-cached copy (preferred, decrypted to FLAC + played via play_data)
    // -> player L1/L2 -> Qobuz network. The offline handle is None before
    // login. The sink drives the padlock while a CMAF bundle decrypts.
    let offline = crate::offline::get().await;
    let sink = crate::offline_cache::row_sink(weak.clone());
    // Session resume: if this is the track restored at launch, start it at the
    // saved position (consumed once); any other track starts from 0.
    let start_position_secs = crate::session_persist::take_resume_for(track_id);
    match runtime
        .core()
        .play_track_resolved(
            track_id,
            playback_quality(),
            offline.as_deref(),
            Some(&sink),
            start_position_secs,
        )
        .await
    {
        Ok(()) => {
            // Player also cancels superseded play_track work; this gates any
            // post-success UI side effects if another play already owns the spinner.
            let still_current = PENDING_PLAY_ID.load(std::sync::atomic::Ordering::Relaxed)
                == track_id;
            if !still_current {
                log::info!(
                    "[qbz-slint] playback: play_track {track_id} completed but was superseded"
                );
            }
        }
        Err(e) => {
            log::error!("[qbz-slint] playback: play_track {track_id} failed: {e}");
            // Superseded fetch: the user already started another play while this
            // one was resolving — that newer play owns the cursor; do NOT skip.
            let still_current = PENDING_PLAY_ID.load(std::sync::atomic::Ordering::Relaxed)
                == track_id;
            // The fetch failed: no audio will advance, so the poll loop would never
            // clear the spinner. Drop it now (only if this play is still current).
            clear_loading(weak, track_id);
            // Tauri-parity regression fix: an unavailable track used to be
            // auto-skipped by the frontend (playbackService `autoSkipToNext`,
            // bounded, issue #467). Without this the queue cursor parks on the
            // dead track and playback stops. Terminal errors only — transient
            // failures were already retried by the client and should not skip.
            if still_current && is_terminal_unavailable(&e) {
                auto_skip_unavailable(runtime, weak, track_id).await;
            }
        }
    }
}

/// Advance past a track whose play failed with a terminal "unavailable"
/// error, mirroring the Tauri frontend's `autoSkipToNext`: toast, honor
/// stop-after, bounded consecutive counter, then reuse the real advance
/// machinery. `after_track_change` re-enters `play_audible`, so this is an
/// async recursion — bounded by `MAX_UNAVAILABLE_SKIPS` (counter reset in
/// the poll loop on real audio). The signature RETURNS a boxed `dyn Future
/// + Send` instead of being an `async fn`: the recursion makes the future's
/// Send-ness self-referential, and with an inferred (`impl Future`) type the
/// compiler hits a query cycle ("cannot satisfy ...: Send"). Declaring the
/// concrete boxed type in the signature is what cuts the cycle — the same
/// shape the `async_recursion` macro expands to.
fn auto_skip_unavailable<'a>(
    runtime: &'a Runtime,
    weak: &'a slint::Weak<AppWindow>,
    failed_track_id: u64,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
    crate::toast::show_weak(
        weak,
        qbz_i18n::t("This track is no longer available"),
        crate::ToastKind::Warning,
    );
    // Stop-after-this-song on the failed track: halt exactly like the
    // natural end-of-track arm would (no advance, queue intact); the
    // marker is one-shot and must be consumed here or it would leak onto
    // a track it was never armed for.
    if failed_track_id != 0
        && runtime.core().consume_stop_after_if(failed_track_id).await
    {
        set_viz_paused(runtime, true);
        let _ = weak.upgrade_in_event_loop(|w| {
            w.global::<NowPlayingState>().set_playing(false);
        });
        return;
    }
    let skips = UNAVAILABLE_SKIPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    if skips > MAX_UNAVAILABLE_SKIPS {
        log::warn!(
            "[qbz-slint] playback: {MAX_UNAVAILABLE_SKIPS} consecutive unavailable tracks — stopping the skip walk"
        );
        crate::toast::show_weak(
            weak,
            qbz_i18n::t("No available tracks to play"),
            crate::ToastKind::Warning,
        );
        set_viz_paused(runtime, true);
        let _ = weak.upgrade_in_event_loop(|w| {
            w.global::<NowPlayingState>().set_playing(false);
        });
        return;
    }
    if let Some(track) = advance_to_playable(runtime, weak, true).await {
        let next_id = track.id;
        after_track_change(runtime, weak, next_id).await;
        refresh_sidebar(true);
    }
    })
}

/// Audible step for a LOCAL user file: read it off-thread and hand the bytes
/// to the player's `play_data` seam (which extracts the sample rate + drives
/// the PROTECTED device init, untouched here). CUE virtual tracks share one
/// file, so seek to the track start. `row_id` is the library row id. Called
/// by `play_audible` when the current queue track's source is `"local"`.
async fn play_local_file_audible(
    runtime: &Runtime,
    weak: &slint::Weak<AppWindow>,
    row_id: u64,
) {
    // Ephemeral tracks (synthetic id >= 2^48) resolve from the in-memory
    // session, never the DB. Everything downstream (read bytes, play_data, CUE
    // seek) is identical to a real local file.
    //
    // FAST PATH for CUE virtual tracks: all the tracks of a CUE album share ONE
    // big audio file. If that file is already loaded in the player (the loaded
    // track is ephemeral and points at the same path), DON'T re-read + re-decode
    // the whole FLAC — just seek to the new track's start. Re-reading a multi-
    // hundred-MB single-file album on every track click was "infierno de lento".
    // The seekbar then reports absolute file time (accepted limitation, as in
    // the Tauri build); the now-playing title/artist still update from the queue
    // cursor.
    if crate::ephemeral::is_ephemeral_id(row_id as i64) {
        if let Some(target) = crate::ephemeral::get_track(row_id as i64) {
            let loaded_id = runtime.core().player().state.current_track_id();
            if runtime.core().player().has_loaded_audio()
                && crate::ephemeral::is_ephemeral_id(loaded_id as i64)
                && crate::ephemeral::get_track(loaded_id as i64)
                    .map(|l| l.file_path == target.file_path)
                    .unwrap_or(false)
            {
                let pos = target.cue_start_secs.unwrap_or(0.0).max(0.0);
                let _ = runtime.core().player().seek(pos as u64);
                return;
            }
        }
    }
    let info = if crate::ephemeral::is_ephemeral_id(row_id as i64) {
        crate::ephemeral::get_track(row_id as i64).map(|t| (t.file_path, t.cue_start_secs))
    } else {
        tokio::task::spawn_blocking(move || {
            crate::library_db::with_db(|db| db.get_track(row_id as i64))
        })
        .await
        .ok()
        .flatten()
        .flatten()
        .map(|t| (t.file_path, t.cue_start_secs))
    };
    let Some((path, cue)) = info else {
        log::error!("[qbz-slint] local play: track {row_id} not found");
        clear_loading(weak, row_id);
        return;
    };
    // DSD (.dsf/.dff): converted to PCM on the fly by the player (qbz-dsd
    // Phase 1) — streamed from disk, never slurped through play_data. Errors
    // here are expected user-facing cases (DST-compressed DFF, >2ch) → toast
    // + stop, the queue stays usable.
    if qbz_dsd::is_dsd_path(std::path::Path::new(&path)) {
        let exists_path = path.clone();
        let exists = tokio::task::spawn_blocking(move || {
            std::path::Path::new(&exists_path).exists()
        })
        .await
        .unwrap_or(false);
        if !exists {
            log::error!("[qbz-slint] local play: DSD file not available at {path}");
            crate::toast::show_weak(
                weak,
                qbz_i18n::t("File not available — is the drive mounted?"),
                crate::ToastKind::Warning,
            );
            clear_loading(weak, row_id);
            return;
        }
        if let Err(e) = runtime
            .core()
            .player()
            .play_dsd_file(std::path::PathBuf::from(&path), row_id)
        {
            log::error!("[qbz-slint] local play: play_dsd_file {row_id} failed: {e}");
            crate::toast::show_weak(
                weak,
                format!("{}: {e}", qbz_i18n::t("Cannot play DSD file")),
                crate::ToastKind::Warning,
            );
            clear_loading(weak, row_id);
        }
        return;
    }
    // PLAYBACK LOCK (owner verdict 2026-06-10): the library never hides
    // network-folder content, so an unmounted drive surfaces HERE — one cheap
    // `Path::exists()` stat before the read, with friendly feedback instead
    // of a silent log-only failure. Runs inside spawn_blocking, never on the
    // audio callback thread: an unmounted path returns false instantly; only
    // a dead-but-still-mounted NFS/CIFS share could block, and then it blocks
    // a pool thread, not audio (see `local_track_file_exists`).
    let read_path = path.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        if !std::path::Path::new(&read_path).exists() {
            return None;
        }
        std::fs::read(&read_path).ok()
    })
    .await
    .ok()
    .flatten();
    let Some(bytes) = bytes else {
        log::error!("[qbz-slint] local play: file not available at {path}");
        crate::toast::show_weak(
            weak,
            qbz_i18n::t("File not available — is the drive mounted?"),
            crate::ToastKind::Warning,
        );
        clear_loading(weak, row_id);
        return;
    };
    if let Err(e) = runtime.core().player().play_data(bytes, row_id) {
        log::error!("[qbz-slint] local play: play_data {row_id} failed: {e}");
        clear_loading(weak, row_id);
        return;
    }
    if let Some(start) = cue {
        if start > 0.0 {
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            let _ = runtime.core().player().seek(start as u64);
        }
    }
}

/// Audible step for a Plex track: PROGRESSIVE STREAMING.
///
/// Resolves ONLY the direct-play part URL (no body download — that was the
/// ~10s stall) and feeds it into the player's progressive streaming sink via
/// the shared `remote_stream` feeder (the same one QConnect uses). Playback
/// starts as soon as the initial buffer fills (~1s), not after the whole FLAC
/// lands. The feeder decodes the same original bytes and drives the PROTECTED
/// device init from the DECODED stream (bit-perfect), so the Plex
/// `sampling_rate_hz`/`bit_depth` are display-only and never touched here.
///
/// On any streaming-setup failure (resolve / probe / sink open) it falls back
/// to the old whole-file `plex_resolve_track_media` + `play_data` so a server
/// that breaks streaming still plays. The loading spinner is cleared only on
/// the hard-error paths; the poll loop clears it on the first decoded-audio
/// edge (same as Qobuz / QConnect).
///
/// `play_id` is the queue id (numeric rating key); `rating_key` is the string
/// key the resolve needs; `duration_secs` comes from the queue track.
async fn play_plex_audible(
    runtime: &Runtime,
    weak: &slint::Weak<AppWindow>,
    play_id: u64,
    rating_key: String,
    duration_secs: u64,
) {
    let cfg = crate::plex_settings::get();
    if cfg.base_url.is_empty() || cfg.token.is_empty() {
        log::error!("[qbz-slint] plex play: no Plex credentials configured");
        clear_loading(weak, play_id);
        return;
    }

    // 1. Resolve JUST the direct-play part URL — no body download.
    let loc = match qbz_plex::plex_resolve_part_url(
        cfg.base_url.clone(),
        cfg.token.clone(),
        rating_key.clone(),
    )
    .await
    {
        Ok(l) => l,
        Err(e) => {
            log::error!("[qbz-slint] plex play: resolve {rating_key} failed: {e}");
            clear_loading(weak, play_id);
            return;
        }
    };

    if !loc.direct_play_confirmed {
        // Not a direct `/library/parts/.../file` part: the server may force a
        // transcode and the streamed bytes would not be bit-perfect. Fall back
        // to the whole-file resolve so the track still plays.
        log::warn!(
            "[qbz-slint] plex play: {rating_key} part is not a direct /file part ({}); \
             full-download fallback",
            loc.part_key
        );
        plex_full_download_fallback(runtime, weak, play_id, rating_key).await;
        return;
    }

    // 2. Stream the part URL progressively (same feeder QConnect uses). On
    //    setup failure (probe / sink open), fall back to whole-file download.
    let player = runtime.core().player();
    match crate::remote_stream::stream_remote_track_into_player(
        &player,
        play_id,
        duration_secs,
        0, // Plex callers don't resume mid-track; start at 0 (like QConnect).
        &loc.part_url,
        "Plex",
    )
    .await
    {
        Ok(()) => {
            // Buffering started — the poll loop clears the spinner on the first
            // decoded-audio edge. Nothing else to do here.
        }
        Err(e) => {
            log::warn!(
                "[qbz-slint] plex play: streaming setup for {play_id} failed ({e}); \
                 full-download fallback"
            );
            plex_full_download_fallback(runtime, weak, play_id, rating_key).await;
        }
    }
}

/// Whole-file Plex fallback: resolve + download the entire part body and hand
/// it to `play_data`. Slow (the original ~10s path), but keeps a track playable
/// when progressive streaming setup fails or the part is not direct-play.
async fn plex_full_download_fallback(
    runtime: &Runtime,
    weak: &slint::Weak<AppWindow>,
    play_id: u64,
    rating_key: String,
) {
    let cfg = crate::plex_settings::get();
    match qbz_plex::plex_resolve_track_media(cfg.base_url, cfg.token, rating_key.clone()).await {
        Ok(r) => {
            if let Err(e) = runtime.core().player().play_data(r.bytes, play_id) {
                log::error!("[qbz-slint] plex play: fallback play_data {play_id} failed: {e}");
                clear_loading(weak, play_id);
            }
        }
        Err(e) => {
            log::error!("[qbz-slint] plex play: fallback resolve {rating_key} failed: {e}");
            clear_loading(weak, play_id);
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
    // Push the new queue onto the sidebar model — without this the Queue
    // panel kept showing the previous queue until it was reopened or its tab
    // toggled. The sibling play paths (play_local_album / play_local_tracks_from
    // / the Qobuz play-all paths) already do this; this shared helper backs all
    // five Local Library entry points, so it was the one path that omitted it.
    refresh_sidebar(true);
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

/// If the track currently playing is from an ephemeral folder, stop it and
/// clear the queue + now-playing chrome. Mirrors Tauri's
/// `wipeEphemeralPlaybackArtifacts`: called when the ephemeral session is
/// cleared or replaced, so a stale ephemeral track (whose synthetic id will be
/// reused by the next session) can't linger in the bar or false-highlight a row
/// in the newly-loaded folder.
pub async fn wipe_ephemeral_if_playing(runtime: &Runtime, weak: &slint::Weak<AppWindow>) {
    let is_eph = runtime
        .core()
        .current_track()
        .await
        .map(|t| crate::ephemeral::is_ephemeral_id(t.id as i64))
        .unwrap_or(false);
    if !is_eph {
        return;
    }
    let _ = runtime.core().stop();
    runtime.core().clear_queue(false).await;
    clear_loading(weak, 0);
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<NowPlayingState>().set_has_track(false);
    });
    refresh_sidebar(true);
}

/// Play the whole ephemeral folder (every album, scan order). The in-memory
/// snapshot becomes the queue; playback routes through the shared local-file
/// seam (the synthetic ids resolve via `crate::ephemeral`).
pub fn play_ephemeral_all(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    handle.spawn(async move {
        let tracks = crate::ephemeral::tracks_snapshot();
        play_local_tracks_now(&runtime, &weak, tracks, 0).await;
    });
}

/// Play one ephemeral album (its tracks become the queue, in scan order).
pub fn play_ephemeral_album(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    group_key: String,
) {
    handle.spawn(async move {
        let tracks = crate::ephemeral::album_tracks(&group_key);
        play_local_tracks_now(&runtime, &weak, tracks, 0).await;
    });
}

/// Play one ephemeral track — its album group becomes the queue, starting at
/// the clicked track (mirrors Tauri's `playEphemeralTrack`).
pub fn play_ephemeral_track(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    track_id: i64,
) {
    handle.spawn(async move {
        let Some(track) = crate::ephemeral::get_track(track_id) else {
            return;
        };
        let key = crate::ephemeral::ephemeral_album_key(&track);
        let tracks = crate::ephemeral::album_tracks(&key);
        let start = tracks
            .iter()
            .position(|t| t.id == track_id)
            .unwrap_or(0);
        play_local_tracks_now(&runtime, &weak, tracks, start).await;
    });
}

/// Replace the queue with an ephemeral selection identified by intent.
pub fn ephemeral_play(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    kind: String,
    arg: String,
) {
    match kind.as_str() {
        "all" => play_ephemeral_all(runtime, weak, handle),
        "album" => play_ephemeral_album(runtime, weak, handle, arg),
        "track" => {
            if let Ok(id) = arg.parse::<i64>() {
                play_ephemeral_track(runtime, weak, handle, id);
            }
        }
        _ => {}
    }
}

/// Append an ephemeral selection to the CURRENT queue (no replace).
pub fn ephemeral_enqueue(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    kind: String,
    arg: String,
) {
    handle.spawn(async move {
        let tracks = match kind.as_str() {
            "all" => crate::ephemeral::tracks_snapshot(),
            "album" => crate::ephemeral::album_tracks(&arg),
            "track" => arg
                .parse::<i64>()
                .ok()
                .and_then(crate::ephemeral::get_track)
                .into_iter()
                .collect(),
            _ => Vec::new(),
        };
        if tracks.is_empty() {
            return;
        }
        let queue: Vec<QueueTrack> = tracks.iter().map(local_queue_track).collect();
        runtime.core().add_tracks(queue).await;
        refresh_sidebar(true);
        crate::toast::success_weak(&weak, qbz_i18n::t("Added to queue"));
    });
}

/// Either play the ephemeral selection now, or — if a queue is already active —
/// prompt add-to-queue vs clear-and-play. Only the ephemeral pane uses this
/// (user decision 2026-06-06: ephemeral-only, dialog-on-play).
pub fn ephemeral_play_or_prompt(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    kind: String,
    arg: String,
) {
    let rt = runtime.clone();
    let wk = weak.clone();
    let hd = handle.clone();
    handle.spawn(async move {
        let active = rt.core().current_track().await.is_some();
        if active {
            // "Add to queue" only when the existing queue is itself all-ephemeral
            // (no mixing ephemeral with persistent tracks).
            let (queue, _) = rt.core().get_all_queue_tracks().await;
            let enqueue_allowed = !queue.is_empty()
                && queue.iter().all(|t| {
                    crate::ephemeral::is_ephemeral_id(t.id as i64)
                        || t.source.as_deref() == Some("ephemeral")
                });
            let k = kind.clone();
            let a = arg.clone();
            let _ = wk.upgrade_in_event_loop(move |w| {
                let s = w.global::<crate::EphemeralPlayChoiceState>();
                s.set_intent_kind(k.into());
                s.set_intent_arg(a.into());
                s.set_enqueue_allowed(enqueue_allowed);
                s.set_open(true);
            });
        } else {
            ephemeral_play(rt, wk, hd, kind, arg);
        }
    });
}

/// Play an explicit list of local tracks (already resolved — e.g. one album
/// VERSION), starting at `start`. `shuffle` enables shuffle mode after the
/// queue is set. Used by the dedicated Local album view so it plays the SHOWN
/// version, never a re-merged metadata group.
pub fn play_local_tracks(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    tracks: Vec<qbz_library::LocalTrack>,
    start: usize,
    shuffle: bool,
) {
    if tracks.is_empty() {
        return;
    }
    handle.spawn(async move {
        play_local_tracks_now(&runtime, &weak, tracks, start).await;
        if shuffle {
            // No set_shuffle on core — toggle until it's on.
            let mut on = runtime.core().toggle_shuffle().await;
            if !on {
                on = runtime.core().toggle_shuffle().await;
            }
            let _ = weak.upgrade_in_event_loop(move |w| {
                w.global::<NowPlayingState>().set_shuffle(on);
            });
            // `play_local_tracks_now` already refreshed the sidebar BEFORE this
            // inline toggle, so the UP NEXT list is still in pre-shuffle order —
            // re-pull it now that shuffle reordered the queue. `false` = no fav
            // network pull.
            refresh_sidebar(false);
        }
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
/// (Superseded by the instant in-memory-cache path; kept for the full-list
/// option.)
#[allow(dead_code)]
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
pub(crate) fn local_queue_track(track: &qbz_library::LocalTrack) -> QueueTrack {
    // Source-aware: offline copies read as Qobuz downloads (carry the Qobuz id
    // so the shared resolver finds them); ephemeral tracks keep their synthetic
    // high id + an "ephemeral" tag so playback routes to the in-memory store;
    // everything else is a real local user file.
    let src = match track.source.as_deref() {
        Some("qobuz_download") => "qobuz_download",
        Some("ephemeral") => "ephemeral",
        Some("plex") => "plex",
        _ => "local",
    };
    let is_offline = src == "qobuz_download";
    let is_plex = src == "plex";
    // Artwork: a Plex row carries a raw server-relative thumb path
    // (`/library/metadata/.../thumb/...`); it must stay RAW so the now-playing
    // bar, queue panel, and MPRIS resolve it to a tokenized `PlexThumb` from
    // current creds. `file://`-prefixing it (as for real local files) poisons
    // it into a local-read miss on all three surfaces.
    let artwork_url = track.artwork_path.as_ref().map(|p| {
        if is_plex || p.starts_with("file://") {
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
        // Local tracks have no Qobuz album-version concept.
        album_version: None,
        duration_secs: track.duration_secs,
        artwork_url,
        hires: track.bit_depth.map(|d| d > 16).unwrap_or(false),
        bit_depth: track.bit_depth,
        sample_rate: Some(sample_rate_khz),
        is_local: true,
        // album_id is the navigation key (now-playing "go to album", Recently
        // Played, record_recent). For Plex the track's `album_group_key` is the
        // per-edition SPLIT key (plex:album:<parentRatingKey>) which the album
        // cache is NOT keyed by — recover the content-hash album key instead so
        // open-album finds it. Local files: the group key is already the right
        // navigation key.
        album_id: Some(if is_plex {
            qbz_plex::plex_album_key(&track.artist, &track.album)
        } else {
            track.album_group_key.clone()
        }),
        artist_id: None,
        streamable: true,
        source: Some(src.to_string()),
        parental_warning: false,
        // For Plex, carry the string rating_key (the numeric queue `id` is a
        // hashed/parsed form; the resolve needs the original key). Persisted in
        // the session queue store so playback survives a restart.
        source_item_id_hint: if is_plex {
            Some(track.file_path.clone())
        } else {
            None
        },
        // Container origin is stamped by the play path (stamp_queue_context);
        // the generic builder leaves it unset.
        context_kind: None,
        context_id: None,
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
            // Robust on-disk cover lookup (cover/folder/front/art/<album>.jpg,
            // any image as a last resort) — shared with the Folders subcards.
            .or_insert_with(|| crate::local_library::find_folder_cover(&folder))
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

/// Resolve a HIGHER-res now-playing cover (~300px) and apply it to
/// `NowPlayingState.artwork-large`. Feeds the hover preview that floats above
/// the bar's small art, so the ~220px popup is crisp instead of an upscale of
/// the 160px bar art. Mirrors [`load_now_playing_artwork`] but decodes larger
/// and writes the separate `artwork-large` slot. Source-aware via the SAME
/// `ArtworkRef` funnel (the caller passes a `Some(300)` ref).
fn load_now_playing_artwork_large(weak: slint::Weak<AppWindow>, art: qbz_models::ArtworkRef) {
    if art.is_empty() {
        return;
    }
    let Some(cache) = crate::artwork::shared_cache() else {
        return;
    };
    tokio::spawn(async move {
        // Decode at high resolution (was 300 — the hover-preview size). The
        // IMMERSIVE view grows this cover to 600-800px, so a 300px decode was
        // upscaled and looked blurry on large windows. Decoding up to 1000px lets
        // the FULL source resolution through (Qobuz typically serves ~600); the
        // immersive's native-size cap then grows the art sharply up to that real
        // resolution instead of upscaling a tiny decode.
        let Some((pixels, w, h)) =
            crate::artwork::fetch_and_decode_ref(&art, &cache, 1000).await
        else {
            return;
        };
        // ALL pixel crunching stays HERE, off the UI thread. The decode is up
        // to 1000px, and the four cover-derived visuals below each used to run
        // their own full-size-to-tiny resize (plus a full buffer copy) INSIDE
        // the event loop — a visible stall at every track boundary on weak
        // hardware. Only the finished Send carriers (SharedPixelBuffer +
        // Colors) cross into the event loop; the !Send slint::Image is built
        // there, matching the pixels_to_image pattern.

        // Full-res cover buffer for the hover preview / immersive cover.
        // Mirrors pixels_to_image's length guard (None → empty image).
        let artwork_buf = {
            let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
            let dst = buf.make_mut_bytes();
            if dst.len() == pixels.len() {
                dst.copy_from_slice(&pixels);
                Some(buf)
            } else {
                None
            }
        };

        // One shared downscale (8x8 + 16x16, consuming `pixels` — no copy)
        // feeds atmosphere + glow + spectrum + lyrics accent with the exact
        // sampling inputs each helper used to compute for itself.
        let analysis = crate::immersive::cover_tiny_samples(pixels, w, h).map(
            |(tiny8, tiny16)| {
                let (bg_pixels, bg_w, bg_h) =
                    crate::immersive::atmosphere_from_tiny8(&tiny8);
                let bg_buf = {
                    let mut buf =
                        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(bg_w, bg_h);
                    let dst = buf.make_mut_bytes();
                    if dst.len() == bg_pixels.len() {
                        dst.copy_from_slice(&bg_pixels);
                        Some(buf)
                    } else {
                        None
                    }
                };
                let glow = crate::immersive::glow_color(&tiny8);
                let (spec_primary, spec_secondary) =
                    crate::immersive::spectrum_colors(&tiny16);
                let spec_accent = crate::immersive::lyrics_accent_color(&tiny16);
                (bg_buf, glow, spec_primary, spec_secondary, spec_accent)
            },
        );

        let _ = weak.upgrade_in_event_loop(move |win| {
            let img = match artwork_buf {
                Some(buf) => slint::Image::from_rgba8(buf),
                None => slint::Image::default(),
            };
            // The hover-preview cover is ALWAYS needed (independent of the
            // immersive overlay), so set it unconditionally first.
            win.global::<NowPlayingState>().set_artwork_large(img);

            // ALWAYS (re)apply the immersive ambient atmosphere (Codex's
            // blurred moving background) + glow + spectrum colors from this cover.
            // The overlay is conditionally mounted so applying while closed is
            // cheap; the track-change reset clears bg-image, so this MUST run
            // unconditionally — a URL dedupe here left bg-image empty after the
            // reset and the atmosphere fell back to the raw (sharp) cover.
            if let Some((bg_buf, glow, spec_primary, spec_secondary, spec_accent)) = analysis {
                let imm = win.global::<ImmersiveState>();
                if let Some(bg_buf) = bg_buf {
                    imm.set_bg_image(slint::Image::from_rgba8(bg_buf));
                }
                imm.set_glow_color(glow);
                imm.set_spectrum_primary(spec_primary);
                imm.set_spectrum_secondary(spec_secondary);
                imm.set_lyrics_accent(spec_accent);
                // Feed the same album-art triad to the wgpu shader underlay so the
                // immersive shaders (Plasma/Tunnel/Aurora) are album-colored instead
                // of hardcoded. Pushed on track change; read on every shader frame
                // (thread-local — must be written on the UI thread, so it stays here).
                crate::shader_underlay::set_palette(spec_primary, spec_secondary, spec_accent);
            }
        });
    });
}

/// Mirror the playing/paused state onto the visualizer tap so the FFT producer
/// parks while nothing plays (paused/stopped it would otherwise re-FFT the
/// stale ring buffer at 30fps — the NPB Large dock idled at ~2.5% CPU). Called
/// next to every `NowPlayingState.set_playing` flip so the producer stays
/// consistent with the UI-thread drain gate (visualizer.rs), which keys off the
/// same flag. Atomic store — safe from any thread, never blocks; a paused park
/// self-wakes within 200ms after resume (no unpark required).
fn set_viz_paused(runtime: &Runtime, paused: bool) {
    if let Some(tap) = runtime.visualizer_tap() {
        tap.set_paused(paused);
    }
}

/// Wall-clock now in milliseconds. Used by the poll loop to extrapolate the
/// peer renderer's position (`position_ms + (now - updated_at_ms)`) while
/// QBZ is CONTROLLING a peer (the local player is stopped, so the seek bar
/// must follow the peer instead). Mirrors the Svelte `qconnectRemoteClockMs`.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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

/// Seed the seek bar + timers on `NowPlayingState` to a fixed position (UI
/// thread). Used by the session restore so the bar shows the resume point
/// immediately — `refresh_now_playing_meta` resets these to 0, and the poll
/// loop only catches up once playback actually starts.
pub(crate) fn seed_seek_display(w: &AppWindow, position_secs: u64, duration_secs: u64) {
    let np = w.global::<NowPlayingState>();
    let progress = if duration_secs > 0 {
        (position_secs as f32 / duration_secs as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    np.set_duration_secs(duration_secs as i32);
    np.set_position_secs(position_secs as i32);
    np.set_progress(progress);
    np.set_elapsed(fmt_elapsed(position_secs).into());
    np.set_remaining(fmt_remaining(position_secs, duration_secs).into());
}

/// Push the now-playing values for the current queue track onto
/// `NowPlayingState`. Called when a new track starts so the song card
/// updates immediately (the poll loop only refreshes position/progress).
/// Last track id we fired a desktop notification for. `refresh_now_playing_meta`
/// runs on resume/seek too, so we de-dupe to only notify on an actual track
/// change. `u64::MAX` = "nothing notified yet" (no real track id collides).
static NOTIFY_LAST_TRACK: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(u64::MAX);

/// User gate for desktop "now playing" notifications (Settings › Appearance ›
/// System Notifications). Seeded from `ui_prefs.system_notifications` at startup
/// and flipped live by the toggle. Default ON. Read off the poll thread, so an
/// atomic (not the UI-thread AppearanceState) is the source of truth here.
pub static NOTIFICATIONS_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

/// Last `(track id, resolved art URL)` pushed to the OS media controls'
/// metadata. `refresh_now_playing_meta` re-runs on resume/seek/quality-patch,
/// so metadata is only re-pushed when this key actually changes — the
/// track-id dedupe extended to the art field (B11). `None` = nothing pushed
/// yet / cleared.
static MPRIS_LAST_META: std::sync::Mutex<Option<(u64, Option<String>)>> =
    std::sync::Mutex::new(None);

/// Force-flag for the poll loop's per-tick dirty-guards (`last_ui_push` /
/// `last_remote_ui_push` in `start_poll_loop`). `refresh_now_playing_meta`
/// seeds the bar OPTIMISTICALLY (position 0 / playing true / purchases
/// mirror) before audio actually starts; when the play is then refused or
/// fails (offline refusal, fetch error) the engine snapshot never moves, so
/// the guards would skip the corrective push forever and the bar would stick
/// on "playing". Set after every optimistic seed; the loop consumes it at the
/// top of the next tick and re-pushes engine/peer truth unconditionally.
static FORCE_UI_REPUSH: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Catalog-MAX stream params of the current track, cached at every
/// track-change meta push so the poll loop can compare the DELIVERED stream
/// (PlaybackEvent.sample_rate / bit_depth) against the track's advertised
/// max without an async queue read per tick (#590 follow-up: the badge keeps
/// the max; a downgrade arrow + tooltip surface the true delivered quality).
/// Rate in Hz (same normalization as NowPlayingState.sample-rate-hz); 0 =
/// unknown. Bits: 1 = DSD (nominal 1-bit — exempt from the bit comparison).
static TRACK_MAX_RATE_HZ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static TRACK_MAX_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Compare-and-record the MPRIS metadata dedupe key. Returns `true` when
/// `key` differs from the last pushed value (→ caller pushes now), recording
/// it as the new last-pushed value. A poisoned lock falls back to pushing.
fn mpris_meta_changed(key: &(u64, Option<String>)) -> bool {
    match MPRIS_LAST_META.lock() {
        Ok(mut last) => {
            if last.as_ref() == Some(key) {
                false
            } else {
                *last = Some(key.clone());
                true
            }
        }
        Err(_) => true,
    }
}

pub(crate) async fn refresh_now_playing_meta(runtime: &Runtime, weak: &slint::Weak<AppWindow>) {
    let state = runtime.core().get_queue_state().await;
    let Some(track) = state.current_track else {
        // No current track → clear the tray tooltip (Linux) + stop media controls.
        // Reset the notify guard so replaying the same track after a stop fires.
        NOTIFY_LAST_TRACK.store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
        // Reset the metadata dedupe too, so replaying the same track after a
        // stop re-pushes MPRIS metadata.
        if let Ok(mut last) = MPRIS_LAST_META.lock() {
            *last = None;
        }
        if let Some(t) = crate::tray::handle() {
            t.clear_track();
        }
        if let Some(mc) = crate::media_controls::handle() {
            mc.set_playback(qbz_media_controls::PlaybackStatus::Stopped, None);
        }
        // Track -> null resets the lyrics state (Tauri parity,
        // lyricsStore.ts:560-562).
        crate::lyrics::on_track_cleared(weak.clone());
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
    // Album with its release variant appended ("Octavarium (2009 Remaster)") for
    // the now-playing bar, MPRIS, and the desktop notification. Scrobbling keeps
    // the CLEAN `album` (below) so Last.fm doesn't fragment stats across editions.
    let album_display = match track
        .album_version
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(v) => format!("{album} ({v})"),
        None => album.clone(),
    };
    let album_id = track.album_id.clone().unwrap_or_default();
    let artist_id = track.artist_id.map(|id| id.to_string()).unwrap_or_default();
    // "Playing from" origin for the now-playing song-card layers button. Derived
    // from the CURRENT track's own per-track stamp (context_kind/context_id set
    // at enqueue time by stamp_queue_context) and re-published on EVERY track
    // change below — so the button always carries the right source for the track
    // that is actually playing and is never a stale single global. When the
    // track carries no container origin (bare single-track / favorites / mix /
    // search / restored-session play) it falls back to the track's own album.
    let (context_kind, context_id) = match (
        track.context_kind.as_deref().filter(|s| !s.is_empty()),
        track.context_id.as_deref().filter(|s| !s.is_empty()),
    ) {
        (Some(kind), Some(id)) => (kind.to_string(), id.to_string()),
        _ => ("album".to_string(), album_id.clone()),
    };
    let track_id_num = track.id;
    let track_id = track.id.to_string();
    // Ephemeral tracks have no DB row → metadata-bound actions (favorite,
    // add-to-playlist, track-info) are gated off in the UI via this flag.
    let is_ephemeral = crate::ephemeral::is_ephemeral_id(track.id as i64);
    // Normalized source for the UI ("qobuz" | "local" | "plex" | ...). Qobuz
    // tracks coerce a None source to "qobuz"; local tracks to "local". Gates the
    // Qobuz-only Track-info trigger.
    let source = track
        .source
        .clone()
        .unwrap_or_else(|| if track.is_local { "local" } else { "qobuz" }.to_string());
    let duration = track.duration_secs;
    // Plex-aware: a Plex track carries a raw `/library/...` thumb path that
    // must resolve to a tokenized `PlexThumb` (from current creds) so the
    // now-playing bar, MPRIS (`to_mpris_url`), and the desktop notification all
    // get the fetchable cover. For non-Plex tracks this is identical to
    // `artwork_ref()` (it falls back cleanly).
    let plex = crate::plex_settings::get();
    // Two refs from the same track: MPRIS / desktop-notification art wants a
    // larger image, so it gets the raw full-res Plex path (`size: None`). The
    // now-playing bar renders small and decodes to 160 (see
    // `load_now_playing_artwork`), so it requests a 160px server-side
    // transcode — downloading ~what it renders instead of the full original.
    // For non-Plex tracks both collapse to the same `artwork_ref()`.
    let artwork = track.artwork_ref_with_plex(&plex.base_url, &plex.token, None);
    let bar_artwork = track.artwork_ref_with_plex(&plex.base_url, &plex.token, Some(160));
    // Higher-res cover for the hover preview that floats above the bar art. Same
    // source-aware funnel; a ~300px server-side transcode so the ~220px popup is
    // crisp without paying for the full original. One extra fetch per track.
    // Immersive-grade size: Plex covers transcode at 1000px (the immersive grows
    // the cover large). Qobuz ignores the hint (its URL is fixed) — the high-res
    // comes from the larger decode in load_now_playing_artwork_large.
    let preview_artwork = track.artwork_ref_with_plex(&plex.base_url, &plex.token, Some(1000));
    // Quality badge: tier from bit depth (24-bit+ = Hi-Res), exact detail line
    // reused from the shared formatter so it matches the track-row badges.
    // bit_depth == 1 marks DSD (1-bit stream, sample_rate = DSD bit rate) —
    // the generic detail would read "1-bit / 2.8224 kHz".
    let quality_tier = match track.bit_depth {
        Some(1) => "hires",
        Some(d) if d >= 24 => "hires",
        Some(_) => "cd",
        None if track.hires => "hires",
        None => "",
    };
    let quality_detail = if quality_tier.is_empty() {
        String::new()
    } else if track.bit_depth == Some(1) {
        crate::quality::dsd_multiple_label(track.sample_rate)
    } else {
        crate::quality::detail(track.bit_depth, track.sample_rate)
    };
    // Cache the catalog max for the poll loop's downgrade detection (#590
    // follow-up). Rate normalized to Hz exactly like the sample-rate-hz push
    // below (`track.sample_rate` is Hz when >= 1000, else kHz); 0 = unknown.
    TRACK_MAX_RATE_HZ.store(
        track.sample_rate.map_or(0, |sr| {
            if sr >= 1000.0 {
                sr as u32
            } else {
                (sr * 1000.0) as u32
            }
        }),
        std::sync::atomic::Ordering::Relaxed,
    );
    TRACK_MAX_BITS.store(
        track.bit_depth.unwrap_or(0),
        std::sync::atomic::Ordering::Relaxed,
    );

    // Mirror the now-playing metadata into the system tray tooltip (Linux).
    if let Some(t) = crate::tray::handle() {
        t.set_track(title.clone(), artist.clone(), album.clone());
    }

    // LOCAL-FIRST artwork for the desktop NOTIFICATION (B11): remote covers
    // resolve through the shared disk-image cache first — the notify pipeline
    // strips `file://` and decodes the bytes by CONTENT, so the cache's
    // extension-less `<md5>.img` copy is fine there and saves a re-download.
    //
    // MPRIS is different: widgets resolve `mpris:artUrl` file:// URIs through
    // the freedesktop mime database, which maps `*.img` BY EXTENSION to a
    // disk-image type (`application/vnd.efi.img`) — the cached copy is
    // rejected and the widget shows no art (the B11 regression: online plays
    // almost always cache-hit, so every push carried the dead .img URL).
    // ONLINE, MPRIS therefore keeps the remote https URL untouched (widgets
    // fetch it themselves — the production-proven Tauri
    // `normalizeCoverUrlForMetadata` contract). OFFLINE keeps slice-3b's
    // exact semantics: a hit hands MPRIS the file:// copy (nothing else can
    // load — better than no art for widgets that do sniff content), a miss
    // gives no art (widgets can't fetch https), while the notification keeps
    // the remote URL so its own md5 disk cache can still serve it (the
    // offline flag below blocks the download). Local/Plex refs keep their
    // normal URL (already file:// / LAN Plex).
    let offline = crate::offline_mode::engine().is_offline();
    let mut mpris_art = artwork.to_mpris_url();
    let mut notify_art = mpris_art.clone();
    if let qbz_models::ArtworkRef::Remote(url) = &artwork {
        match crate::artwork::cached_file_url_for(url) {
            Some(cached) => {
                notify_art = Some(cached.clone());
                if offline {
                    mpris_art = Some(cached);
                }
            }
            None if offline => {
                mpris_art = None;
            }
            None => {}
        }
    }

    // Push to the OS media controls (MPRIS / SMTC / MediaRemote). The app icon
    // GNOME shows comes from the MPRIS DesktopEntry; `art_url` is the album art
    // (`mpris:artUrl`) — remote covers pass through online (widgets fetch
    // https; never the .img cache copy, see the resolution block above),
    // offline cache hits become a file:// URI. Metadata is de-duped on
    // (track id, resolved art): this refresh re-runs on
    // resume/seek/quality-patch with identical values, so only an actual
    // change re-pushes. `set_playback` stays unconditional.
    if let Some(mc) = crate::media_controls::handle() {
        if mpris_meta_changed(&(track.id, mpris_art.clone())) {
            mc.set_metadata(&qbz_media_controls::TrackMeta {
                title: title.clone(),
                artist: artist.clone(),
                album: album_display.clone(),
                duration: (duration > 0).then(|| std::time::Duration::from_secs(duration as u64)),
                art_url: mpris_art,
            });
        }
        mc.set_playback(
            qbz_media_controls::PlaybackStatus::Playing,
            Some(std::time::Duration::ZERO),
        );
    }

    // Desktop "now playing" notification (1:1 with the Tauri path). De-dupe so
    // only an actual track change fires; skip while a remote QConnect renderer
    // drives playback (matches the Svelte `skipIfRemote`). Fire-and-forget.
    if NOTIFY_LAST_TRACK.swap(track.id, std::sync::atomic::Ordering::Relaxed) != track.id {
        // Lyrics prefetch — third rider on the same de-duped track-change
        // edge. Tauri prefetches on EVERY track change regardless of panel
        // visibility (lyricsStore.ts:545-565); same here. Deliberately NOT
        // inside the skip-if-remote spawn below: lyrics follow the QConnect
        // peer's track (Q7). Fire-and-forget; the stale-response guard (F2)
        // lives in `lyrics::on_track_changed`.
        crate::lyrics::on_track_changed(weak.clone(), &track);
        // Discord Rich Presence: push the new track on this de-duped
        // track-change edge (no-op + no IPC when not opted in). Mirrors the
        // Tauri service's track_id transition push.
        crate::discord_rpc::push(runtime, &tokio::runtime::Handle::current());
        // Warm the NEXT queued track's lyrics in the background so the panel is
        // instant when it becomes current (cache-only; no UI). Generated here
        // because Tauri only ever fetches the CURRENT track.
        if let Some(next) = runtime.core().queue().read().await.peek_next() {
            crate::lyrics::prefetch_lyrics(&next);
        }
        let notify_meta = qbz_media_controls::NotificationMeta {
            title: title.clone(),
            artist: artist.clone(),
            album: album_display.clone(),
            bit_depth: track.bit_depth,
            sample_rate: track.sample_rate,
            art_url: notify_art,
        };
        // Source-agnostic scrobbling (Last.fm + ListenBrainz). Fires on the
        // SAME de-duped track-change edge as the notification, so resume/seek
        // (which also re-run this fn) do NOT re-fire. Feeds the normalized
        // QueueTrack text (Qobuz, local, AND Plex) with the version-enriched
        // title (#360 parity). Skipped when a remote QConnect renderer drives
        // playback — never scrobble a peer's audio.
        let scrobble_meta = crate::scrobble::ScrobbleMeta {
            artist: artist.clone(),
            track: title.clone(),
            album: (!album.is_empty()).then(|| album.clone()),
            duration_secs: duration,
        };
        tokio::spawn(async move {
            if let Some(svc) = crate::qconnect_service::service() {
                if svc.is_peer_active().await {
                    return;
                }
            }
            crate::scrobble::on_track_changed(scrobble_meta);
            // Scrobbling above is independent of the notification gate; only the
            // desktop notification honors the System Notifications toggle.
            if NOTIFICATIONS_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
                qbz_media_controls::show_track_notification(notify_meta, offline).await;
            }
        });
    }

    // Seed the "+" flyout's album-collection entry from the favorite-album
    // cache — the SAME source the card/header toggles flip — so the entry
    // renders add vs remove honestly for the new track. Kept live between
    // track changes by set_album_row_favorite (main.rs).
    let album_favorite = crate::fav_cache::is_album_favorite(&album_id);
    // The bar is seeded playing=true below — wake the visualizer producer with
    // it (stored BEFORE the UI post so the drain gate never opens while the
    // producer is still marked paused).
    set_viz_paused(runtime, false);
    let _ = weak.upgrade_in_event_loop(move |w| {
        let np = w.global::<NowPlayingState>();
        np.set_has_track(true);
        np.set_title(title.into());
        np.set_artist(artist.into());
        np.set_album(album_display.into());
        np.set_album_id(album_id.into());
        np.set_album_favorite(album_favorite);
        np.set_artist_id(artist_id.into());
        // Re-publish the "playing from" origin for THIS track every change — the
        // authoritative source for the song-card layers button (no stale global).
        np.set_context_kind(context_kind.into());
        np.set_context_id(context_id.into());
        np.set_track_id(track_id.into());
        // Mirror the active track + playing flag onto the Purchases globals so a
        // purchase track-row highlights/animates when it is the now-playing one.
        mirror_now_playing_to_purchases(&w, track_id_num, true);
        np.set_is_ephemeral(is_ephemeral);
        np.set_source(source.into());
        np.set_quality_tier(quality_tier.into());
        np.set_quality_detail(quality_detail.into());
        // Numeric stream params for the Spectral Ribbon overlay. `track.sample_rate`
        // is Hz when >= 1000, else kHz — normalize to Hz.
        np.set_sample_rate_hz(track.sample_rate.map_or(0, |sr| {
            if sr >= 1000.0 {
                sr as i32
            } else {
                (sr * 1000.0) as i32
            }
        }));
        np.set_bit_depth(track.bit_depth.unwrap_or(0) as i32);
        // Reset the EFFECTIVE (delivered) quality on every track change — the
        // poll loop re-derives it from the engine's PlaybackEvent once the new
        // stream opens (the old track's downgrade state must not linger).
        np.set_effective_sample_rate_hz(0);
        np.set_effective_bit_depth(0);
        np.set_quality_downgraded(false);
        np.set_quality_true_detail("".into());
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
        // Clear the hover-preview cover too, exactly like the bar art, so the
        // floating preview never shows the previous track while the new high-res
        // cover resolves.
        np.set_artwork_large(slint::Image::default());
        // Do NOT clear the immersive atmosphere bg here. Blanking it caused a
        // visible BACKGROUND FLICKER on a click-driven track change (the async
        // 300px decode + blur takes a beat, so the bg went blank then back).
        // Let the previous blurred ambient bg persist until
        // load_now_playing_artwork_large swaps in the new one — a brief stale
        // blur is imperceptible; a blank/raw-cover fallback is not.
    });

    // The bar was just seeded optimistically — make the poll loop's next tick
    // re-push engine/peer truth even if the raw snapshot is unchanged (see
    // FORCE_UI_REPUSH). Set AFTER the closure post above: the corrective push
    // is also an event-loop post, so FIFO ordering keeps it after the seed.
    FORCE_UI_REPUSH.store(true, std::sync::atomic::Ordering::Relaxed);

    load_now_playing_artwork(weak.clone(), bar_artwork);
    load_now_playing_artwork_large(weak.clone(), preview_artwork);
}

/// Mirror the now-playing track id + active flag onto the two Purchases globals
/// (`PurchasesState` + `PurchaseDetailState`), which drive the purchase
/// track-row highlight (`active`) and the playing-bars (`playing`). Unlike the
/// album/favorites views (which bind directly to `NowPlayingState`), the
/// Purchases views were built with their own `active-track-id` / `playback-active`
/// `in` properties, so Rust must push the values. Matches Svelte, where both
/// views receive `activeTrackId={currentTrack?.id}` + `isPlaybackActive={isPlaying}`
/// (+page.svelte:6970-6984). Event-loop only (`w` is already upgraded).
fn mirror_now_playing_to_purchases(w: &AppWindow, track_id: u64, is_playing: bool) {
    // active-track-id is compared against the row's string id; 0 → "" (no row
    // highlighted), matching the Svelte `null` active id.
    let id_str: slint::SharedString = if track_id == 0 {
        slint::SharedString::new()
    } else {
        track_id.to_string().into()
    };
    let purchases = w.global::<PurchasesState>();
    purchases.set_active_track_id(id_str.clone());
    purchases.set_playback_active(is_playing);
    let detail = w.global::<PurchaseDetailState>();
    detail.set_active_track_id(id_str);
    detail.set_playback_active(is_playing);
}

/// Record the playback CONTEXT — the source the queue was launched from — on
/// `NowPlayingState`, so the song-card layers button can navigate back to it.
/// Stamp every queued track with the container it was launched FROM, so the
/// now-playing song-card "playing from" button — re-derived per track in
/// `refresh_now_playing_meta` — always points at the right source for whatever
/// is actually playing. Pass the same `kind`/`id` the old
/// `set_now_playing_context` call used at that play path ("album"/album_id,
/// "artist"/artist_id, "playlist"/playlist_id, "label"/label_id). This replaces
/// the single-global approach that went stale across track changes: the origin
/// now travels WITH each track and is republished on every advance (gapless
/// included), never cached.
pub(crate) fn stamp_queue_context(tracks: &mut [QueueTrack], kind: &str, id: &str) {
    for t in tracks.iter_mut() {
        t.context_kind = Some(kind.to_string());
        t.context_id = Some(id.to_string());
    }
}

/// LEGACY single-global context setter. Superseded by per-track
/// `stamp_queue_context` + the per-track republish in
/// `refresh_now_playing_meta` (which is now authoritative). Retained for the
/// miniplayer mirror path / potential reuse; no live caller remains.
#[allow(dead_code)]
pub fn set_now_playing_context(weak: &slint::Weak<AppWindow>, kind: &str, id: &str) {
    let kind = kind.to_string();
    let id = id.to_string();
    let _ = weak.upgrade_in_event_loop(move |w| {
        let np = w.global::<NowPlayingState>();
        np.set_context_kind(kind.into());
        np.set_context_id(id.into());
    });
}

/// Build a `QueueTrack` for the queue from the catalog `Track`, filling
/// the album metadata from `album_meta` (the track's own album summary is
/// often partial in album responses).
pub(crate) fn make_queue_track(
    track: &qbz_models::Track,
    album_id: &str,
    album_title: &str,
    album_artist: &str,
    album_artwork: &str,
    album_version: Option<&str>,
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
        album_version: album_version
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string),
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
        // Container origin is stamped by the play path (stamp_queue_context);
        // the generic builder leaves it unset so single-track / search plays
        // fall back to the track's own album.
        context_kind: None,
        context_id: None,
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
        source: track.source.clone().unwrap_or_else(|| "qobuz".to_string()),
    });
    // Per-artist play count — feeds the discovery filter "skip
    // artists I already know" (HavingCount > threshold). artist_id
    // is optional on QueueTrack; skip when absent.
    if let Some(artist_id) = track.artist_id {
        crate::play_history::record_play(artist_id, &track.artist);
    }
    // reco: log this play for taste scoring. The helper gates to Qobuz-catalog
    // sources only (local/plex/ephemeral ids don't resolve against the Qobuz
    // catalog and would poison the home seeds). SQLite is blocking, so it runs
    // on the blocking pool, off the async record_recent path.
    let (rid, ralb, rart, rsrc) = (
        track.id,
        track.album_id.clone(),
        track.artist_id,
        track.source.clone(),
    );
    tokio::task::spawn_blocking(move || {
        crate::reco::log_play_gated(rid, ralb, rart, rsrc.as_deref());
    });
    // Home rails auto-refresh: the recently-played store just changed, so
    // notify the UI layer — it re-reads the LOCAL store into the Home rails
    // NOW if the Home view is showing (small JSON read, cached artwork), or
    // leaves them dirty for the next Home mount. Reaches the window through
    // the global queue controller, like apply_plex_quality_to_queue
    // (record_recent does not carry a weak).
    if let Some(controller) = QUEUE_CONTROLLER.get() {
        crate::note_recent_store_changed(
            controller.weak().clone(),
            controller.handle().clone(),
        );
    }
}

/// THE single queue-drop predicate for an already-built `QueueTrack` (Task 7).
/// Returns `true` when the track must be removed from a play/shuffle/queue-next/
/// queue-later builder. Delegates to `artist_blacklist::is_track_blacklisted`,
/// the SAME underlying source-guard + per-id check the row greyout
/// (`stamp_row`) uses — so the queue can never diverge from the rendered list.
///
/// `QueueTrack` carries `source` + `artist_id` (performer) but NOT a composer
/// id, so this leg is performer-only. Builders that still hold the full
/// catalog `Track` (album / playlist / artist-top) ALSO filter at the `Track`
/// level via `track_is_blacklisted_full` below, which adds the composer leg
/// (D-FEAT). Local / Plex / no-id tracks => kept (fail-open).
fn queue_track_blacklisted(track: &QueueTrack) -> bool {
    let source = track.source.as_deref().unwrap_or("qobuz");
    crate::artist_blacklist::is_track_blacklisted(
        source,
        track.artist_id,
        None,
        track.album_id.as_deref(),
    )
}

/// Drop blacklisted entries from a freshly-built `QueueTrack` queue. Keeps
/// local / Plex / no-id tracks (fail-open). The single filter every builder
/// applies before handing the queue to the core.
fn filter_blacklisted_queue(queue: Vec<QueueTrack>) -> Vec<QueueTrack> {
    queue
        .into_iter()
        .filter(|t| !queue_track_blacklisted(t))
        .collect()
}

/// `Track`-level drop predicate (performer OR composer — full D-FEAT), for
/// builders that still hold the catalog `Track` before mapping to QueueTrack.
/// `album_primary` is the album's primary-artist id used as the row fallback
/// when the track carries no performer (album surfaces only — mirror the album
/// row stamp `track.artist_id ?? album.artist_id`). Always treated as Qobuz
/// (these builders only run on Qobuz catalog tracks; local/Plex play paths are
/// separate). Shares the underlying `is_blacklisted` check with the row stamp.
fn track_is_blacklisted_full(track: &Track, album_primary: Option<u64>) -> bool {
    let performer = track
        .performer
        .as_ref()
        .map(|p| p.id)
        .or(album_primary);
    let composer = track.composer.as_ref().map(|c| c.id);
    crate::artist_blacklist::is_track_blacklisted(
        "qobuz",
        performer,
        composer,
        track.album.as_ref().map(|a| a.id.as_str()),
    )
}

/// Play `album_id` from `start_index`: fetch the album, build the queue,
/// hand it to the core, and start audio on the start track.
/// Fetch an album and build its play queue (genre/quality meta cached for
/// the Recently Played card). Shared by `play_album` (start at a positional
/// index) and `play_album_from` (start at a clicked track id). Returns None
/// and toasts on failure / an empty album.
async fn fetch_album_for_play(
    runtime: &Runtime,
    weak: &slint::Weak<AppWindow>,
    album_id: &str,
) -> Option<Vec<QueueTrack>> {
    let album = match runtime.core().get_album(album_id).await {
        Ok(album) => album,
        Err(e) => {
            log::error!("[qbz-slint] playback: get_album {album_id} failed: {e}");
            crate::toast::error_weak(weak, qbz_i18n::t("Couldn't load this album"));
            return None;
        }
    };

    let album_title = album.title.clone();
    let album_artist = album.artist.name.clone();
    let album_artwork = album.image.best().cloned().unwrap_or_default();
    // Album's primary artist id — the fallback blacklist key for tracks whose
    // own performer id is missing (D-FEAT album rule: track.artist ?? album).
    let album_primary = Some(album.artist.id);
    // Cache the album's genre / release date / quality so the Recently
    // Played card the play records carries them (no extra fetch).
    crate::recently::remember_album_meta(&album.id, album_card_meta(&album));

    let raw_tracks = album
        .tracks
        .as_ref()
        .map(|container| container.items.as_slice())
        .unwrap_or_default();

    // Genuinely empty album → keep the existing "no playable tracks" toast.
    if raw_tracks.is_empty() {
        log::warn!("[qbz-slint] playback: album {album_id} has no tracks");
        crate::toast::error_weak(weak, qbz_i18n::t("This album has no playable tracks"));
        return None;
    }

    // D-FIX-b: the Tauri `buildAlbumQueueTracks` did NOT filter, so playing an
    // album where a blacklisted artist is FEATURED still queued that track.
    // Filter the raw catalog tracks here (composer-aware, album-primary
    // fallback) BEFORE mapping to QueueTrack so play-all / play-from / shuffle
    // all skip blacklisted (performer OR composer OR featured) tracks.
    let tracks: Vec<QueueTrack> = raw_tracks
        .iter()
        .filter(|track| !track_is_blacklisted_full(track, album_primary))
        .map(|track| {
            make_queue_track(
                track,
                &album.id,
                &album_title,
                &album_artist,
                &album_artwork,
                album.version.as_deref(),
            )
        })
        .collect();

    if tracks.is_empty() {
        // Every track was blacklisted → silent early-return (no toast), Tauri
        // 0-playable parity for the album builders.
        log::warn!(
            "[qbz-slint] playback: album {album_id} fully filtered by blacklist; nothing to play"
        );
        return None;
    }
    Some(tracks)
}

pub fn play_album(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
    start_index: usize,
) {
    handle.spawn(async move {
        let Some(mut tracks) = fetch_album_for_play(&runtime, &weak, &album_id).await else {
            return;
        };
        stamp_queue_context(&mut tracks, "album", &album_id);
        let start = start_index.min(tracks.len() - 1);
        let start_track_id = tracks[start].id;
        runtime.core().set_queue(tracks, Some(start)).await;
        after_track_change(&runtime, &weak, start_track_id).await;
        refresh_sidebar(true);
    });
}

/// Play an album starting at the clicked track id (queues the tracks that
/// follow). `visible_ids` is the album view's VISIBLE row order — the queue
/// is reordered/filtered to match it, so the album track-search filter is
/// respected. Anchoring on the id keeps the start correct regardless.
pub fn play_album_from(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
    visible_ids: Vec<String>,
    clicked_id: String,
) {
    handle.spawn(async move {
        let Some(tracks) = fetch_album_for_play(&runtime, &weak, &album_id).await else {
            return;
        };
        let mut tracks = reorder_queue_by_visible(tracks, &visible_ids);
        stamp_queue_context(&mut tracks, "album", &album_id);
        let start = tracks
            .iter()
            .position(|t| t.id.to_string() == clicked_id)
            .unwrap_or(0);
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
/// Fetch the artist page and build the Popular-tracks play queue. Shared by
/// `play_artist_top_tracks` (start at 0) and `play_artist_top_from` (start at
/// a clicked track id). Returns None and toasts on failure / no top tracks.
async fn fetch_artist_top_for_play(
    runtime: &Runtime,
    weak: &slint::Weak<AppWindow>,
    artist_id: &str,
) -> Option<Vec<QueueTrack>> {
    let id: u64 = match artist_id.parse() {
        Ok(id) => id,
        Err(_) => {
            log::warn!("[qbz-slint] play-top: invalid artist id {artist_id}");
            return None;
        }
    };
    let page = match runtime.core().get_artist_page(id, None).await {
        Ok(page) => page,
        Err(e) => {
            log::error!("[qbz-slint] play-top: get_artist_page failed: {e}");
            crate::toast::error_weak(weak, qbz_i18n::t("Couldn't load this artist"));
            return None;
        }
    };
    let artist_name = page.name.display.clone();
    let raw: Vec<QueueTrack> = page
        .top_tracks
        .unwrap_or_default()
        .into_iter()
        .map(|track| make_top_track_queue(track, &artist_name))
        .collect();
    if raw.is_empty() {
        log::warn!("[qbz-slint] play-top: artist {artist_id} has no top tracks");
        crate::toast::error_weak(weak, qbz_i18n::t("No top tracks available for this artist"));
        return None;
    }
    // Drop blacklisted top tracks (a featured/blacklisted performer can appear
    // in another artist's Popular list). Silent early-return when 0 remain.
    let tracks = filter_blacklisted_queue(raw);
    if tracks.is_empty() {
        log::warn!("[qbz-slint] play-top: artist {artist_id} fully filtered by blacklist");
        return None;
    }
    Some(tracks)
}

pub fn play_artist_top_tracks(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    artist_id: String,
) {
    handle.spawn(async move {
        let Some(mut tracks) = fetch_artist_top_for_play(&runtime, &weak, &artist_id).await else {
            return;
        };
        stamp_queue_context(&mut tracks, "artist", &artist_id);
        let start_track_id = tracks[0].id;
        runtime.core().set_queue(tracks, Some(0)).await;
        after_track_change(&runtime, &weak, start_track_id).await;
        refresh_sidebar(true);
    });
}

/// Enqueue (play-next or append) a subset of the artist's Popular tracks,
/// identified by catalog id. Re-fetches the page (like the play-all path),
/// filters to `ids`, preserves the page order, and queues — QConnect-aware
/// (mirrors `enqueue_queue_tracks`). Drives both the bulk bar (selection)
/// and the section "more" menu (all ids).
pub fn enqueue_artist_top_selected(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    artist_id: String,
    ids: Vec<String>,
    next: bool,
) {
    if ids.is_empty() {
        return;
    }
    handle.spawn(async move {
        let Some(all) = fetch_artist_top_for_play(&runtime, &weak, &artist_id).await else {
            return;
        };
        let want: std::collections::HashSet<u64> =
            ids.iter().filter_map(|s| s.parse::<u64>().ok()).collect();
        let tracks: Vec<QueueTrack> =
            all.into_iter().filter(|qt| want.contains(&qt.id)).collect();
        if tracks.is_empty() {
            return;
        }
        if let Some(svc) = crate::qconnect_service::service() {
            let routed: Vec<(u64, Option<String>)> =
                tracks.iter().map(|qt| (qt.id, qt.source.clone())).collect();
            let handled = if next {
                svc.play_next_batch_on_peer_if_active(&routed).await
            } else {
                svc.add_to_queue_batch_on_peer_if_active(&routed).await
            };
            if handled {
                return;
            }
        }
        if next {
            for track in tracks.into_iter().rev() {
                runtime.core().add_track_next(track).await;
            }
        } else {
            runtime.core().add_tracks(tracks).await;
        }
        refresh_sidebar(false);
        crate::toast::success_weak(&weak, if next { qbz_i18n::t("Playing next") } else { qbz_i18n::t("Added to queue") });
    });
}

/// Shuffle-play ALL of the artist's Popular tracks (section "more" menu).
/// Re-fetches, xorshift-shuffles (same seedless mix as `play_album_shuffled`),
/// and replaces the queue.
pub fn play_artist_top_shuffled(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    artist_id: String,
) {
    handle.spawn(async move {
        let Some(mut tracks) = fetch_artist_top_for_play(&runtime, &weak, &artist_id).await else {
            return;
        };
        if tracks.is_empty() {
            return;
        }
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
            | 1;
        for i in (1..tracks.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let j = (seed % (i as u64 + 1)) as usize;
            tracks.swap(i, j);
        }
        stamp_queue_context(&mut tracks, "artist", &artist_id);
        let start_track_id = tracks[0].id;
        runtime.core().set_queue(tracks, Some(0)).await;
        after_track_change(&runtime, &weak, start_track_id).await;
        refresh_sidebar(true);
    });
}

/// Play the artist's Popular tracks starting at the clicked track id (queues
/// the tracks that follow it). `visible_ids` is the Popular-tracks VISIBLE row
/// order — the queue is reordered/filtered to match, so the in-page search
/// filter is respected. Re-fetches the page like `play_artist_top_tracks`.
pub fn play_artist_top_from(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    artist_id: String,
    visible_ids: Vec<String>,
    clicked_id: String,
) {
    handle.spawn(async move {
        let Some(tracks) = fetch_artist_top_for_play(&runtime, &weak, &artist_id).await else {
            return;
        };
        let mut tracks = reorder_queue_by_visible(tracks, &visible_ids);
        stamp_queue_context(&mut tracks, "artist", &artist_id);
        let start = tracks
            .iter()
            .position(|t| t.id.to_string() == clicked_id)
            .unwrap_or(0);
        let start_track_id = tracks[start].id;
        runtime.core().set_queue(tracks, Some(start)).await;
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
        album_version: None,
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
        // Stamped "artist" by the artist play paths; unset here.
        context_kind: None,
        context_id: None,
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
            None,
        );

        runtime.core().set_queue(vec![queue_track], Some(0)).await;
        after_track_change(&runtime, &weak, track_id).await;
        refresh_sidebar(true);
    });
}

/// "m:ss" / "h:mm:ss" -> seconds (for a queue row built off a display string).
fn mmss_to_secs(s: &str) -> u64 {
    s.split(':')
        .filter_map(|p| p.trim().parse::<u64>().ok())
        .fold(0u64, |acc, v| acc * 60 + v)
}

/// Build a `QueueTrack` from a visible Slint `TrackItem` row. Used for views
/// that render Qobuz tracks but keep no full-`Track` cache (search): the
/// audio is resolved by id at play time, so the row's display fields suffice
/// to seed the queue. Returns None for rows whose id is not numeric.
fn track_item_to_queue(it: &TrackItem) -> Option<QueueTrack> {
    let id = it.id.as_str().parse::<u64>().ok()?;
    let album_id = {
        let a = it.album_id.to_string();
        if a.is_empty() {
            None
        } else {
            Some(a)
        }
    };
    Some(QueueTrack {
        id,
        title: it.title.to_string(),
        version: None,
        artist: it.artist.to_string(),
        album: it.album.to_string(),
        album_version: None,
        duration_secs: mmss_to_secs(it.duration.as_str()),
        artwork_url: {
            let u = it.artwork_url.to_string();
            if u.is_empty() {
                None
            } else {
                Some(u)
            }
        },
        hires: it.quality_tier.as_str() == "hires",
        bit_depth: None,
        sample_rate: None,
        is_local: it.source.as_str() == "local",
        album_id: album_id.clone(),
        artist_id: it.artist_id.as_str().parse::<u64>().ok(),
        streamable: true,
        source: {
            let s = it.source.to_string();
            Some(if s.is_empty() {
                "qobuz".to_string()
            } else {
                s
            })
        },
        parental_warning: it.explicit,
        source_item_id_hint: album_id,
        // Stamped by the play path when launched from a container; unset here.
        context_kind: None,
        context_id: None,
    })
}

/// The ids of a view's VISIBLE `TrackItem` model rows, in order.
fn model_ids(model: &ModelRc<TrackItem>) -> Vec<String> {
    (0..model.row_count())
        .filter_map(|i| model.row_data(i).map(|it| it.id.to_string()))
        .collect()
}

/// Re-order (and filter) a freshly-built queue to match a view's VISIBLE
/// order: keep only the tracks the user can see, in the order they see them.
/// Used by the re-fetch views (album / artist top tracks) so an active
/// in-page search filter is respected. Empty `visible_ids` (or no overlap)
/// leaves the canonical order untouched.
fn reorder_queue_by_visible(queue: Vec<QueueTrack>, visible_ids: &[String]) -> Vec<QueueTrack> {
    if visible_ids.is_empty() {
        return queue;
    }
    let pos: std::collections::HashMap<String, usize> = queue
        .iter()
        .enumerate()
        .map(|(i, q)| (q.id.to_string(), i))
        .collect();
    let order: Vec<usize> = visible_ids
        .iter()
        .filter_map(|id| pos.get(id).copied())
        .collect();
    if order.is_empty() {
        return queue;
    }
    let mut slots: Vec<Option<QueueTrack>> = queue.into_iter().map(Some).collect();
    order.iter().filter_map(|&i| slots[i].take()).collect()
}

/// Build a play queue (+ start index) from a view's VISIBLE `TrackItem`
/// model, starting at `clicked_id`. The model IS the visible order, so this
/// never goes out of sync with what the user sees. Used by views with no
/// full-`Track` cache (search).
fn queue_from_model(
    model: &ModelRc<TrackItem>,
    clicked_id: &str,
) -> (Vec<QueueTrack>, Option<usize>) {
    let mut queue: Vec<QueueTrack> = Vec::with_capacity(model.row_count());
    let mut found: Option<usize> = None;
    for i in 0..model.row_count() {
        if let Some(it) = model.row_data(i) {
            if let Some(qt) = track_item_to_queue(&it) {
                if it.id.as_str() == clicked_id {
                    found = Some(queue.len());
                }
                queue.push(qt);
            }
        }
    }
    // `found` is None when the clicked track is not a list row (e.g. the
    // search "most popular" hero card) — the caller decides what to do.
    (queue, found)
}

/// Build a play queue (+ start index) from a view's VISIBLE `TrackItem`
/// model and its authoritative `Vec<Track>` cache: the queue follows the
/// visible order (so custom sort / search filter are respected) and starts
/// at `clicked_id`. Falls back to the cache order if the visible/cache
/// mapping comes up empty.
fn order_by_visible(
    model: &ModelRc<TrackItem>,
    cache: Vec<Track>,
    clicked_id: &str,
) -> Option<(Vec<Track>, usize)> {
    let visible_ids: Vec<String> = (0..model.row_count())
        .filter_map(|i| model.row_data(i).map(|it| it.id.to_string()))
        .collect();
    let by_id: std::collections::HashMap<String, Track> =
        cache.iter().map(|t| (t.id.to_string(), t.clone())).collect();
    let ordered: Vec<Track> = visible_ids
        .iter()
        .filter_map(|id| by_id.get(id).cloned())
        .collect();
    // The clicked track must resolve inside the visible list; if it does not
    // (orphan/hero row, or a cache miss), return None so the caller plays just
    // that track rather than starting the queue at the wrong row.
    let idx = ordered
        .iter()
        .position(|t| t.id.to_string() == clicked_id)?;
    Some((ordered, idx))
}

/// Hand a prebuilt `QueueTrack` queue to the core and start at `start`.
/// Callers guard against an empty queue.
fn play_queue(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    queue: Vec<QueueTrack>,
    start: usize,
) {
    let start = start.min(queue.len() - 1);
    let first_id = queue[start].id;
    handle.spawn(async move {
        runtime.core().set_queue(queue, Some(start)).await;
        after_track_change(&runtime, &weak, first_id).await;
        refresh_sidebar(true);
    });
}

/// Per-row "play this track" for EVERY tracklist surface. Builds the queue
/// from the CURRENT view's VISIBLE list and starts at the clicked track, so
/// the tracks that visually follow it play next — regardless of context
/// (playlist custom sort, album, favorites filter, artist top tracks, ...).
///
/// This is the single entry point for clicking/double-clicking a track row.
/// It replaces a scatter of per-view paths that each got it wrong: the album
/// row played a lone track (no queue), and the playlist/mix rows always
/// started at track 1 (the clicked id was read from the wrong media-action
/// slot). Do NOT reintroduce per-view play arms — route everything here.
pub fn play_track_in_context(
    window: &AppWindow,
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    clicked_id: &str,
) {
    let view = window.global::<NavState>().get_view();
    // Playback context is now stamped PER-TRACK on the enqueued queue (see
    // stamp_queue_context) and republished every track change in
    // refresh_now_playing_meta. The playlist/label branches below stamp their
    // container; favorites/mix/search/single-track leave the tracks unstamped so
    // the song-card layers button falls back to each track's own album. No
    // global clear needed — a fresh set_queue replaces the whole queue, and an
    // unstamped current track derives the album fallback authoritatively.
    match view {
        // Views with an authoritative Vec<Track> cache: order it by the
        // visible model so sort/filter are respected.
        ContentView::Playlist => {
            // LOCAL playlist detail (id "local:<uuid>") — queue from its
            // own resolved snapshot + the D8 offline-only stamp. The
            // offline sidecar rendering of a MIXED playlist (D11.a) plays
            // from the same snapshot (its rows resolve locally), and so
            // does the ONLINE mixed detail (Seam B: source-aware
            // QueueTracks; QConnect admission rejects the non-Qobuz rows
            // per-track at push time). The now-playing context stays
            // ("playlist", id) — anything Qobuz-bound that reads it
            // re-resolves Qobuz membership, so sidecar rows are excluded
            // from the context by construction (Tauri :1825 parity).
            if window.global::<PlaylistState>().get_is_local()
                || window.global::<PlaylistState>().get_offline_subset()
                || crate::playlist::is_mixed()
            {
                if crate::local_playlist::play_from_visible(
                    window,
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    clicked_id,
                ) {
                    return;
                }
            } else if let Some((tracks, idx)) = order_by_visible(
                &window.global::<PlaylistState>().get_tracks(),
                crate::playlist::current_tracks(),
                clicked_id,
            ) {
                let ctx_id = window.global::<PlaylistState>().get_id().to_string();
                play_tracks_ctx(
                    runtime,
                    weak,
                    handle,
                    tracks,
                    idx,
                    Some(("playlist".to_string(), ctx_id)),
                );
                return;
            }
        }
        ContentView::Favorites => {
            if let Some((tracks, idx)) = order_by_visible(
                &window.global::<FavoritesState>().get_tracks_visible(),
                crate::favorites::play_tracks(),
                clicked_id,
            ) {
                play_tracks(runtime, weak, handle, tracks, idx);
                return;
            }
        }
        ContentView::Label => {
            if let Some((tracks, idx)) = order_by_visible(
                &window.global::<LabelState>().get_top_tracks(),
                crate::label::top_tracks_for_play(),
                clicked_id,
            ) {
                let ctx_id = window.global::<LabelState>().get_id().to_string();
                play_tracks_ctx(
                    runtime,
                    weak,
                    handle,
                    tracks,
                    idx,
                    Some(("label".to_string(), ctx_id)),
                );
                return;
            }
        }
        ContentView::Mix => {
            // Mix has no custom sort/filter, so the cache order is the
            // visible order; anchor on the clicked id.
            let tracks = crate::mix::current_tracks();
            if tracks.iter().any(|t| t.id.to_string() == clicked_id) {
                let idx = crate::mix::index_of(clicked_id);
                play_tracks(runtime, weak, handle, tracks, idx);
                return;
            }
        }
        // Search keeps no full-Track cache — build the queue straight off
        // the visible model (Qobuz tracks resolve by id at play time).
        ContentView::Search => {
            let model = window.global::<SearchState>().get_tracks();
            let (queue, found) = queue_from_model(&model, clicked_id);
            if found.is_some() {
                // Drop blacklisted rows that visually follow the clicked track,
                // then re-anchor the start on the clicked id (it can't itself be
                // blacklisted — greyed rows are inert). Empty => nothing to do.
                let queue = filter_blacklisted_queue(queue);
                if let Some(idx) = queue.iter().position(|q| q.id.to_string() == clicked_id) {
                    play_queue(runtime, weak, handle, queue, idx);
                }
                return;
            }
            // The "most popular" hero is a top-track card, not a results row.
            // Play it as the queue head, then the visible results, so it acts
            // like a first-class track (clicking it queues what follows).
            let ss = window.global::<SearchState>();
            if ss.get_most_popular_kind().as_str() == "track" {
                let hero = ss.get_most_popular_track();
                if hero.id.as_str() == clicked_id {
                    if let Some(hq) = track_item_to_queue(&hero) {
                        // Filter the trailing results; keep the hero at the head.
                        let mut q = filter_blacklisted_queue(queue);
                        q.insert(0, hq);
                        play_queue(runtime, weak, handle, q, 0);
                        return;
                    }
                }
            }
        }
        // Re-fetch views: build the queue from the catalog, reorder it to the
        // VISIBLE row order (so an in-page search filter is respected), and
        // start at the clicked id. (Local albums are routed earlier.)
        ContentView::Album => {
            let album_id = window.global::<AlbumState>().get_id().to_string();
            if !album_id.is_empty() {
                let visible_ids = model_ids(&window.global::<AlbumState>().get_tracks());
                play_album_from(
                    runtime,
                    weak,
                    handle,
                    album_id,
                    visible_ids,
                    clicked_id.to_string(),
                );
                return;
            }
        }
        ContentView::Artist => {
            let artist_id = window.global::<ArtistState>().get_id().to_string();
            if !artist_id.is_empty() {
                let visible_ids = model_ids(&window.global::<ArtistState>().get_top_tracks());
                play_artist_top_from(
                    runtime,
                    weak,
                    handle,
                    artist_id,
                    visible_ids,
                    clicked_id.to_string(),
                );
                return;
            }
        }
        _ => {}
    }
    // No resolvable list context (Home, Discover, ...): play just the track.
    if let Ok(tid) = clicked_id.parse::<u64>() {
        play_track_now(runtime, weak, handle, tid);
    }
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
    play_tracks_ctx(runtime, weak, handle, tracks, start_index, None)
}

/// Like [`play_tracks`] but stamps every built queue track with the container
/// it was launched FROM (`Some((kind, id))`) so the now-playing "playing from"
/// button resolves to the right source per track. Pass `None` for flat lists
/// with no container origin (radio / mix / favorites) — those fall back to each
/// track's own album.
pub fn play_tracks_ctx(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    tracks: Vec<qbz_models::Track>,
    start_index: usize,
    context: Option<(String, String)>,
) -> bool {
    // Drop blacklisted tracks (performer OR composer — D-FEAT) before building
    // the queue. Shared sink for radio results, the mix views, and album
    // shuffle, so this single filter covers all three. No album-primary
    // fallback here (these are flat track lists, not an album context).
    let mut queue: Vec<QueueTrack> = tracks
        .iter()
        .filter(|track| !track_is_blacklisted_full(track, None))
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
            make_queue_track(track, &album_id, &album_title, &album_artist, &album_artwork, None)
        })
        .collect();
    // Stamp the container origin onto every track so the "playing from" button
    // is correct for whichever one is current (republished per change).
    if let Some((kind, id)) = &context {
        stamp_queue_context(&mut queue, kind, id);
    }
    if queue.is_empty() {
        // Either nothing was passed, or every track was blacklisted. Silent
        // early-return (the caller logs); radio callers surface their existing
        // "returned no tracks" warning, matching Tauri's empty->error path.
        return false;
    }
    let start = start_index.min(queue.len() - 1);
    let first_id = queue[start].id;
    handle.spawn(async move {
        runtime.core().set_queue(queue, Some(start)).await;
        // QConnect: when WE are the active renderer, push the new queue to the
        // peers immediately (self-gates to a no-op when not connected or a peer
        // owns playback). Covers infinite-play refills + album/radio plays so a
        // controller's UI reflects the freshly-built queue.
        if let Some(svc) = crate::qconnect_service::service() {
            svc.sync_local_queue_if_changed().await;
        }
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

/// The Qobuz `/radio/*` endpoints return MINIMAL track objects (no performer /
/// album), so the queue rows would show no artist. Re-fetch full metadata by id
/// (order-preserving via `get_tracks_batch`), falling back to the originals if
/// the batch fails or is empty.
async fn enrich_radio_tracks(
    runtime: &Runtime,
    tracks: Vec<qbz_models::Track>,
) -> Vec<qbz_models::Track> {
    let ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
    if ids.is_empty() {
        return tracks;
    }
    match runtime.core().get_tracks_batch(&ids).await {
        Ok(full) if !full.is_empty() => full,
        _ => tracks,
    }
}

/// Start a Qobuz artist radio (`/radio/artist`). The simpler alternative to
/// the smart pool builder: wired to ArtistView's "Qobuz Radio" choice, while
/// the "QBZ Radio" choice and the plain `("artist","radio")` action use the
/// smart builder.
pub fn play_artist_radio(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    artist_id: String,
) {
    handle.spawn(async move {
        match runtime.core().get_radio_artist(&artist_id).await {
            Ok(resp) => {
                let tracks = enrich_radio_tracks(&runtime, resp.tracks.items).await;
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
                let tracks = enrich_radio_tracks(&runtime, resp.tracks.items).await;
                if !play_radio_response(runtime, weak, tracks) {
                    log::warn!("[qbz-slint] track radio {track_id} returned no tracks");
                }
            }
            Err(e) => log::error!("[qbz-slint] track radio {track_id} failed: {e}"),
        }
    });
}

/// Start a smart track radio via the local qbz-radio pool builder (richer than
/// the plain Qobuz `/radio/track`). The track-row menu only carries the track
/// id, so fetch the track first to seed the builder with its performer.
pub fn play_smart_track_radio(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    track_id: String,
) {
    handle.spawn(async move {
        let Ok(tid) = track_id.parse::<u64>() else {
            log::warn!("[qbz-slint] smart track radio: bad track id {track_id}");
            return;
        };
        let track = match runtime.core().get_track(tid).await {
            Ok(t) => t,
            Err(e) => {
                log::error!("[qbz-slint] smart track radio: get_track {tid} failed: {e}");
                return;
            }
        };
        let Some(aid) = track.performer.as_ref().map(|a| a.id) else {
            log::warn!("[qbz-slint] smart track radio: track {tid} has no performer");
            return;
        };
        match runtime
            .core()
            .create_smart_track_radio(tid, aid, track.title.clone())
            .await
        {
            Ok(tracks) => {
                if !play_radio_response(runtime, weak, tracks) {
                    log::warn!("[qbz-slint] smart track radio {tid} returned no tracks");
                }
            }
            Err(e) => log::error!("[qbz-slint] smart track radio {tid} failed: {e}"),
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
                let tracks = enrich_radio_tracks(&runtime, resp.tracks.items).await;
                if !play_radio_response(runtime, weak, tracks) {
                    log::warn!("[qbz-slint] album radio {album_id} returned no tracks");
                }
            }
            Err(e) => log::error!("[qbz-slint] album radio {album_id} failed: {e}"),
        }
    });
}

/// Enqueue an album's tracks at the end of the current queue.
pub fn enqueue_album(runtime: Runtime, weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle, album_id: String) {
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
        // Drop blacklisted tracks (composer-aware, album-primary fallback)
        // before enqueueing — same predicate as album play-all (D-FIX-b).
        let album_primary = Some(album.artist.id);
        let tracks: Vec<QueueTrack> = album
            .tracks
            .as_ref()
            .map(|container| container.items.as_slice())
            .unwrap_or_default()
            .iter()
            .filter(|track| !track_is_blacklisted_full(track, album_primary))
            .map(|track| {
                make_queue_track(track, &album.id, &album_title, &album_artist, &album_artwork, album.version.as_deref())
            })
            .collect();
        if tracks.is_empty() {
            return;
        }
        // QConnect CONTROLLER mode: route the whole album to the peer's queue when
        // a PEER renderer owns playback. All-or-nothing admission inside the
        // router; returns false when no peer is active, so the local append runs.
        if let Some(svc) = crate::qconnect_service::service() {
            let routed: Vec<(u64, Option<String>)> =
                tracks.iter().map(|qt| (qt.id, qt.source.clone())).collect();
            if svc.add_to_queue_batch_on_peer_if_active(&routed).await {
                return;
            }
        }
        runtime.core().add_tracks(tracks).await;
        refresh_sidebar(false);
        crate::toast::success_weak(&weak, qbz_i18n::t("Added to queue"));
    });
}

/// Play an album with its tracks in a fresh random order (the header Shuffle
/// button). Fetches the album, shuffles the raw track list with the same
/// SystemTime-seeded xorshift Fisher-Yates the playlist shuffle uses (no `rand`
/// dependency), then plays from the top via the shared `play_tracks`.
pub fn play_album_shuffled(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
) {
    let play_handle = handle.clone();
    handle.spawn(async move {
        let album = match runtime.core().get_album(&album_id).await {
            Ok(album) => album,
            Err(e) => {
                log::error!("[qbz-slint] playback: shuffle get_album {album_id} failed: {e}");
                return;
            }
        };
        // D-FEAT: capture the album's primary artist BEFORE moving `tracks`,
        // so the shuffle path applies the SAME album-primary fallback as
        // play-all (fetch_album_for_play). Without it a performer-less album
        // track on a blacklisted artist's album would survive shuffle but be
        // dropped by play-all — an asymmetry on the same album.
        let album_primary = Some(album.artist.id);
        let mut tracks: Vec<qbz_models::Track> =
            album.tracks.map(|container| container.items).unwrap_or_default();
        tracks.retain(|track| !track_is_blacklisted_full(track, album_primary));
        if tracks.is_empty() {
            return;
        }
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
            | 1;
        for i in (1..tracks.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let j = (seed % (i as u64 + 1)) as usize;
            tracks.swap(i, j);
        }
        play_tracks(runtime, weak, play_handle, tracks, 0);
    });
}

/// Insert an album's tracks immediately after the current track ("Play next").
///
/// The core's `add_track_next` inserts a single track after the current index,
/// so the album tracks are inserted in reverse order to land in the right
/// sequence — mirroring Tauri's `v2_add_tracks_to_queue_next`.
pub fn enqueue_album_next(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
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
        // Drop blacklisted tracks (composer-aware, album-primary fallback)
        // before play-next — same predicate as album play-all (D-FIX-b).
        let album_primary = Some(album.artist.id);
        let tracks: Vec<QueueTrack> = album
            .tracks
            .as_ref()
            .map(|container| container.items.as_slice())
            .unwrap_or_default()
            .iter()
            .filter(|track| !track_is_blacklisted_full(track, album_primary))
            .map(|track| {
                make_queue_track(track, &album.id, &album_title, &album_artist, &album_artwork, album.version.as_deref())
            })
            .collect();
        if tracks.is_empty() {
            return;
        }
        // QConnect CONTROLLER mode: route the whole album to the peer (single
        // QueueInsertTracks in NATURAL order — the server preserves block order).
        // All-or-nothing admission inside the router; false when no peer is active.
        if let Some(svc) = crate::qconnect_service::service() {
            let routed: Vec<(u64, Option<String>)> =
                tracks.iter().map(|qt| (qt.id, qt.source.clone())).collect();
            if svc.play_next_batch_on_peer_if_active(&routed).await {
                return;
            }
        }
        // Insert in reverse so the tracks end up in the correct order.
        for track in tracks.into_iter().rev() {
            runtime.core().add_track_next(track).await;
        }
        refresh_sidebar(false);
        crate::toast::success_weak(&weak, qbz_i18n::t("Playing next"));
    });
}

/// Enqueue a single track at the end of the current queue.
pub fn enqueue_track(runtime: Runtime, weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle, track_id: u64) {
    handle.spawn(async move {
        // QConnect CONTROLLER mode: when a PEER renderer owns playback, route the
        // add-to-queue to the peer's queue instead of mutating only the LOCAL
        // queue (the peer never sees a local-only enqueue). Returns false in every
        // non-controller situation, so the local append below runs unchanged.
        if let Some(svc) = crate::qconnect_service::service() {
            // Single-track enqueue always builds a Qobuz catalog track
            // (`make_queue_track` source = "qobuz"), so it is always castable.
            if svc
                .add_to_queue_on_peer_if_active(track_id, Some("qobuz"))
                .await
            {
                return;
            }
        }
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
            make_queue_track(&track, &album_id, &album_title, &album_artist, &album_artwork, None);
        runtime.core().add_track(queue_track).await;
        refresh_sidebar(false);
        crate::toast::success_weak(&weak, qbz_i18n::t("Added to queue"));
    });
}

/// Insert a single track immediately after the current track ("Play next").
pub fn play_track_next(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    track_id: u64,
) {
    handle.spawn(async move {
        // QConnect CONTROLLER mode: route "Play next" to the peer's queue (insert
        // after the peer's current track) instead of mutating only the LOCAL queue.
        // Returns false in every non-controller situation, so the local insert
        // below runs unchanged.
        if let Some(svc) = crate::qconnect_service::service() {
            // Single-track play-next always builds a Qobuz catalog track
            // (`make_queue_track` source = "qobuz"), so it is always castable.
            if svc
                .play_next_on_peer_if_active(track_id, Some("qobuz"))
                .await
            {
                return;
            }
        }
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
            make_queue_track(&track, &album_id, &album_title, &album_artist, &album_artwork, None);
        runtime.core().add_track_next(queue_track).await;
        refresh_sidebar(false);
        crate::toast::success_weak(&weak, qbz_i18n::t("Playing next"));
    });
}

/// Play a whole playlist (by id) NOW — replace the queue with the playlist's
/// tracks and start at the first one. Fetches the tracks fresh, so it works
/// from any playlist CARD (Discover / Search / Label carousels) without a
/// PlaylistView open, unlike the `play-all` arm (which reads the open detail's
/// PlaylistState). Mirrors `enqueue_playlist`'s fetch + mixed-sidecar interleave
/// but calls `set_queue` instead of `add_tracks`, like `play_album`.
pub fn play_playlist(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    playlist_id: String,
) {
    let Ok(pid) = playlist_id.parse::<u64>() else {
        return;
    };
    handle.spawn(async move {
        let playlist = match runtime.core().get_playlist(pid).await {
            Ok(playlist) => playlist,
            Err(e) => {
                log::error!("[qbz-slint] playback: play get_playlist {pid} failed: {e}");
                return;
            }
        };
        let qobuz_tracks: Vec<Track> = playlist.tracks.map(|c| c.items).unwrap_or_default();
        // Same mixed-playlist merge as `enqueue_playlist`: interleave the
        // local/Plex sidecar rows at their stored slots so a card play carries
        // every row WITH its source. Pure-Qobuz playlists read an empty sidecar.
        let qobuz_count = qobuz_tracks.len() as u32;
        let sidecar = tokio::task::spawn_blocking(move || {
            crate::local_playlist::read_sidecar_rows_blocking(pid, qobuz_count, true)
        })
        .await
        .unwrap_or_default();
        let rows = crate::playlist::interleave_rows(qobuz_tracks, sidecar);
        // Drop blacklisted Qobuz rows (performer; local/Plex rows kept by the
        // source guard). Silent early-return when nothing playable remains.
        let mut tracks: Vec<QueueTrack> = filter_blacklisted_queue(
            rows.iter()
                .filter_map(|row| crate::local_playlist::row_queue_track(&row.item))
                .collect(),
        );
        if tracks.is_empty() {
            return;
        }
        stamp_queue_context(&mut tracks, "playlist", &playlist_id);
        let start_track_id = tracks[0].id;
        runtime.core().set_queue(tracks, Some(0)).await;
        after_track_change(&runtime, &weak, start_track_id).await;
        refresh_sidebar(true);
    });
}

/// Enqueue a whole playlist (by id) at the end of the queue, or immediately
/// after the current track when `next`. Fetches the playlist's tracks fresh,
/// so it works from any playlist CARD (carousels, search, favorites) — not just
/// the currently-open PlaylistView. Mirrors the album enqueue paths.
pub fn enqueue_playlist(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    playlist_id: String,
    next: bool,
) {
    let Ok(pid) = playlist_id.parse::<u64>() else {
        return;
    };
    handle.spawn(async move {
        let playlist = match runtime.core().get_playlist(pid).await {
            Ok(playlist) => playlist,
            Err(e) => {
                log::error!("[qbz-slint] playback: enqueue get_playlist {pid} failed: {e}");
                return;
            }
        };
        let qobuz_tracks: Vec<Track> = playlist.tracks.map(|c| c.items).unwrap_or_default();
        // MIXED playlists (T2 fix-forward, spec §1.3): merge the local/Plex
        // sidecar rows at their stored slots so a card/hero enqueue carries
        // EVERY row WITH its source — Tauri's hero arms rebuild catalog-only
        // tracks and drop `source`, crashing plex auto-advance; our merged
        // rows enqueue as the source-aware QueueTracks the detail plays.
        // Pure-Qobuz playlists read an empty sidecar and are unchanged.
        let qobuz_count = qobuz_tracks.len() as u32;
        let sidecar = tokio::task::spawn_blocking(move || {
            crate::local_playlist::read_sidecar_rows_blocking(pid, qobuz_count, true)
        })
        .await
        .unwrap_or_default();
        let rows = crate::playlist::interleave_rows(qobuz_tracks, sidecar);
        // Drop blacklisted Qobuz rows (performer; local/Plex rows kept by the
        // source guard). Silent early-return when nothing playable remains.
        let tracks: Vec<QueueTrack> = filter_blacklisted_queue(
            rows.iter()
                .filter_map(|row| crate::local_playlist::row_queue_track(&row.item))
                .collect(),
        );
        if tracks.is_empty() {
            return;
        }
        // QConnect CONTROLLER mode: route the whole playlist to the peer's queue
        // (insert-next or append). All-or-nothing admission inside the router;
        // returns false when no peer is active, so the local path runs unchanged.
        if let Some(svc) = crate::qconnect_service::service() {
            let routed: Vec<(u64, Option<String>)> =
                tracks.iter().map(|qt| (qt.id, qt.source.clone())).collect();
            let handled = if next {
                svc.play_next_batch_on_peer_if_active(&routed).await
            } else {
                svc.add_to_queue_batch_on_peer_if_active(&routed).await
            };
            if handled {
                return;
            }
        }
        if next {
            // Reverse so the inserted block keeps the playlist's order.
            for track in tracks.into_iter().rev() {
                runtime.core().add_track_next(track).await;
            }
        } else {
            runtime.core().add_tracks(tracks).await;
        }
        refresh_sidebar(false);
        crate::toast::success_weak(&weak, if next { qbz_i18n::t("Playing next") } else { qbz_i18n::t("Added to queue") });
    });
}

/// Append (or insert-next) a batch of already-fetched tracks to the queue
/// without re-fetching them. Used by the favorites bulk bar.
/// Resolve a list of Qobuz track ids (order-preserving) and enqueue them at the
/// end of the queue (or play-next). Backs the external-reco Weekly rows'
/// "add to queue" button (P7). Toasts the outcome.
pub fn enqueue_track_ids(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    ids: Vec<u64>,
    next: bool,
) {
    if ids.is_empty() {
        return;
    }
    handle.spawn(async move {
        match runtime.core().get_tracks_batch(&ids).await {
            Ok(tracks) if !tracks.is_empty() => {
                let n = tracks.len();
                enqueue_tracks(runtime, tokio::runtime::Handle::current(), tracks, next);
                crate::toast::success_weak(
                    &weak,
                    qbz_i18n::tf(
                        "Added {} track to queue",
                        "Added {} tracks to queue",
                        n as i64,
                        &[&n.to_string()],
                    ),
                );
            }
            Ok(_) => crate::toast::error_weak(&weak, qbz_i18n::t("No tracks to add")),
            Err(e) => {
                log::error!("[qbz-slint] enqueue_track_ids: get_tracks_batch failed: {e}");
                crate::toast::error_weak(&weak, qbz_i18n::t("Failed to add tracks"));
            }
        }
    });
}

pub fn enqueue_tracks(
    runtime: Runtime,
    handle: tokio::runtime::Handle,
    tracks: Vec<qbz_models::Track>,
    next: bool,
) {
    if tracks.is_empty() {
        return;
    }
    // Drop blacklisted tracks (performer OR composer — D-FEAT) from the bulk
    // batch before routing/enqueueing. Silent early-return when 0 remain.
    let tracks: Vec<qbz_models::Track> = tracks
        .into_iter()
        .filter(|track| !track_is_blacklisted_full(track, None))
        .collect();
    if tracks.is_empty() {
        return;
    }
    handle.spawn(async move {
        // QConnect CONTROLLER mode: route the batch to the peer's queue when a
        // PEER renderer owns playback. The favorites bulk bar holds Qobuz catalog
        // tracks (source defaults to "qobuz" => castable); all-or-nothing admission
        // inside the router refuses the whole batch if any item is local/plex.
        // Returns false when no peer is active, so the local loop runs unchanged.
        if let Some(svc) = crate::qconnect_service::service() {
            let routed: Vec<(u64, Option<String>)> =
                tracks.iter().map(|track| (track.id, None)).collect();
            let handled = if next {
                svc.play_next_batch_on_peer_if_active(&routed).await
            } else {
                svc.add_to_queue_batch_on_peer_if_active(&routed).await
            };
            if handled {
                return;
            }
        }
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
                make_queue_track(&track, &album_id, &album_title, &album_artist, &album_artwork, None);
            if next {
                runtime.core().add_track_next(qt).await;
            } else {
                runtime.core().add_track(qt).await;
            }
        }
        refresh_sidebar(false);
    });
}

/// Append (or insert-next) a batch of already-built, SOURCE-AWARE
/// QueueTracks — the playlist detail's per-row / bulk Play next + Add to
/// queue route their snapshot rows here (local/plex/cached rows keep their
/// source, so `play_audible` resolves each through its own path). QConnect
/// CONTROLLER mode rides the same batch admission as `enqueue_playlist`:
/// all-or-nothing — a non-castable (local/plex) row refuses the whole batch
/// with a toast while a peer owns playback, exactly like the other
/// source-typed batch paths.
pub fn enqueue_queue_tracks(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    tracks: Vec<QueueTrack>,
    next: bool,
) {
    if tracks.is_empty() {
        return;
    }
    // Drop blacklisted Qobuz rows (performer; local/plex/cached rows kept by the
    // source guard). Silent early-return when nothing playable remains.
    let tracks = filter_blacklisted_queue(tracks);
    if tracks.is_empty() {
        return;
    }
    handle.spawn(async move {
        if let Some(svc) = crate::qconnect_service::service() {
            let routed: Vec<(u64, Option<String>)> =
                tracks.iter().map(|qt| (qt.id, qt.source.clone())).collect();
            let handled = if next {
                svc.play_next_batch_on_peer_if_active(&routed).await
            } else {
                svc.add_to_queue_batch_on_peer_if_active(&routed).await
            };
            if handled {
                return;
            }
        }
        if next {
            // Reverse so the inserted block keeps the selection's order.
            for track in tracks.into_iter().rev() {
                runtime.core().add_track_next(track).await;
            }
        } else {
            runtime.core().add_tracks(tracks).await;
        }
        refresh_sidebar(false);
        crate::toast::success_weak(&weak, if next { qbz_i18n::t("Playing next") } else { qbz_i18n::t("Added to queue") });
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
///
/// Resume is only valid when the audio engine actually holds a loaded stream.
/// When the player has NO loaded audio but the queue has a current track —
/// e.g. a freshly materialized QConnect renderer queue whose cursor sits on a
/// track that was never loaded, or a cold cursor after the queue ended — a
/// bare `resume()` fails with "cannot resume - no audio data available" and the
/// user sees a dead Play button. In that case LOAD and play the current queue
/// track instead, so Play works from a cold cursor. A normal pause leaves the
/// stream loaded (`has_loaded_audio` stays true), so the pause/resume path is
/// unchanged.
pub fn toggle_play_pause(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    handle.spawn(async move {
        if runtime.core().get_playback_state().is_playing {
            if let Err(e) = runtime.core().pause() {
                log::error!("[qbz-slint] playback: pause failed: {e}");
            } else {
                // Park the visualizer producer on the edge, ahead of the
                // 450ms poll tick that mirrors the flag onto the bar.
                set_viz_paused(&runtime, true);
            }
            // Persist the paused position so a restart resumes near where the
            // user stopped (no-op unless `persist_session` is on).
            crate::session_persist::capture_and_save(&runtime).await;
            return;
        }
        // Not playing: resume an existing stream, or cold-start the current
        // queue track when nothing is loaded.
        if runtime.core().player().has_loaded_audio() {
            if let Err(e) = runtime.core().resume() {
                log::error!("[qbz-slint] playback: resume failed: {e}");
            } else {
                // Wake the producer on the edge — resume must feel instant.
                set_viz_paused(&runtime, false);
            }
            return;
        }
        match runtime.core().current_track().await {
            Some(track) => {
                log::info!(
                    "[qbz-slint] playback: play with no loaded audio -> cold-starting current track {}",
                    track.id
                );
                after_track_change(&runtime, &weak, track.id).await;
                refresh_sidebar(true);
            }
            None => {
                log::info!(
                    "[qbz-slint] playback: toggle play ignored (no loaded audio, empty queue)"
                );
            }
        }
    });
}

/// Advance to the next queue track and play it. Offline, unavailable
/// tracks are skipped (bounded — see `advance_to_playable`).
pub fn next(runtime: Runtime, weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        let Some(track) = advance_to_playable(&runtime, &weak, true).await else {
            log::info!("[qbz-slint] playback: end of queue");
            return;
        };
        let track_id = track.id;
        after_track_change(&runtime, &weak, track_id).await;
        refresh_sidebar(true);
    });
}

/// Go to the previous queue track and play it. Offline, unavailable
/// tracks are skipped (bounded — see `advance_to_playable`).
pub fn previous(runtime: Runtime, weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        let Some(track) = advance_to_playable(&runtime, &weak, false).await else {
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

/// Read the authoritative local mute flag (used by the QConnect controller
/// gate to compute the target mute value to forward to a remote renderer).
pub fn is_muted() -> bool {
    MUTED.load(std::sync::atomic::Ordering::Relaxed)
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
        // The ONLY visible effect of a shuffle toggle is the reordered UP NEXT
        // list (the current track stays current, so the NPB itself doesn't
        // change). That list lives in QueueState.upcoming-page / coverflow-tracks
        // and is pushed ONLY by the queue controller, so without this re-pull the
        // button looked dead. `false` skips the network favorite re-pull (shuffle
        // never changes favorites) — cheap and offline-safe.
        refresh_sidebar(false);
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
///
/// Guarded against double-start: the shell can now be entered twice per
/// process (offline session, then the D2 recovery login runs the full
/// online entry over it) and a second loop would double the track-end
/// auto-advance.
pub fn start_poll_loop(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let spawn_handle = handle.clone();
    spawn_handle.spawn(async move {
        // Track whether the last poll observed an active track, so the
        // end-of-track edge is detected once rather than every tick.
        let mut last_track_id: u64 = 0;
        let mut was_playing = false;
        let mut seen_position: u64 = 0;
        // Throttle for the periodic session-position save (every ~11 ticks ≈ 5s).
        let mut save_pos_tick: u64 = 0;
        // Track id we have already fired a gapless prefetch for, so the
        // 450ms ticker does not re-request it every tick.
        let mut gapless_requested_for: u64 = 0;

        // QConnect renderer-report throttle: the official client reports
        // RndrSrvrStateUpdated ~every 2s while playing PLUS immediately on a
        // transition (track / play-state change). At a 450ms tick, ~4 ticks ≈ 2s.
        let mut last_reported_track_id: u64 = 0;
        let mut last_reported_playing = false;
        let mut report_tick: u64 = 0;
        const QCONNECT_REPORT_EVERY_N_TICKS: u64 = 4;

        // QConnect CONTROLLER mode: the peer's last-seen current track id. When
        // the peer advances a track on its own, this edge-detects the change so
        // the bar/queue meta refresh from the core cursor (which the sink already
        // aligned to the peer's track). Reset to 0 when the peer branch is not
        // taken so re-entering controller mode refreshes meta.
        let mut last_peer_track_id: u64 = 0;

        // Dirty-guards for the per-tick UI pushes. Slint Property::set has no
        // equality check, so re-pushing identical values every 450ms dirties
        // bindings and forces a full-window repaint even when fully idle.
        // Each snapshot holds everything its push closure depends on (f32s as
        // bits); when unchanged, the upgrade_in_event_loop is skipped. Reset
        // to None whenever another owner (peer/cast poll) may have written the
        // bar, so returning to this branch re-pushes unconditionally.
        let mut last_ui_push: Option<(u64, u64, u64, bool, u32, u32, u32, u32, u32)> = None;
        let mut last_remote_ui_push = None;

        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(450));
        loop {
            ticker.tick().await;

            // A meta refresh outside this loop just seeded the bar
            // optimistically (position 0 / playing true — see FORCE_UI_REPUSH).
            // Drop both dirty-guards so this tick re-pushes engine/peer truth
            // even when the raw snapshot did not move (refused/failed play,
            // paused track hit by a mid-track Plex quality patch).
            if FORCE_UI_REPUSH.swap(false, std::sync::atomic::Ordering::Relaxed) {
                last_ui_push = None;
                last_remote_ui_push = None;
            }

            // Surface audio-stream failures as a toast (#508/#534/#500): the
            // player records a user-readable message when a stream fails to
            // open, but the drain lived in Tauri's polling loop and was never
            // ported — ALSA Direct failures left the bar frozen at 0:00 with
            // no explanation. take_stream_error_message() drains exactly once
            // per recorded error, so each failed attempt toasts once. Inside a
            // Flatpak/Snap sandbox direct hw: access is blocked by design —
            // when the failure looks ALSA-shaped, say that instead of the raw
            // error.
            if let Some(msg) = runtime.core().player().state.take_stream_error_message() {
                let sandboxed = !matches!(
                    qbz_audio::health::detect_sandbox(),
                    qbz_audio::health::Sandbox::None
                );
                let lower = msg.to_lowercase();
                let text = if sandboxed && (lower.contains("alsa") || lower.contains("hw:")) {
                    qbz_i18n::t(
                        "Direct ALSA device access is blocked inside Flatpak/Snap — switch the audio backend to PipeWire or System Default",
                    )
                } else {
                    format!("{}: {}", qbz_i18n::t("Audio output error"), msg)
                };
                crate::toast::error_weak(&weak, text);
            }

            // --- QConnect CONTROLLER mode: peer-state reflection ----------
            // When QBZ is CONTROLLING a peer renderer, the event sink stops the
            // LOCAL player, so `get_playback_event()` reports track_id == 0 / not
            // playing and the seek bar would freeze. While a peer owns playback,
            // drive the bar from the peer's renderer snapshot instead: title /
            // artist / art come from the materialized local core queue (the sink
            // aligns the core cursor to the peer's track), only position / playing
            // / duration come from the peer. Returns None in every NON-controller
            // situation (disconnected, renderer mode where active == local, no
            // active renderer), so the local path below runs byte-unchanged.
            if let Some(remote) = match crate::qconnect_service::service() {
                Some(svc) => svc.remote_now_playing().await,
                None => None,
            } {
                // The peer changed track on its own → refresh the bar/queue meta
                // from the core cursor (the event sink already aligned it to the
                // peer's track via sync_active_renderer_projection). This resets
                // position to 0, which is correct on a track change; the per-tick
                // position push below immediately re-applies the peer's real
                // position. Done BEFORE the per-tick push so peer values win.
                if remote.track_id != last_peer_track_id {
                    refresh_now_playing_meta(&runtime, &weak).await;
                    refresh_sidebar(true);
                    last_peer_track_id = remote.track_id;
                }
                // Lyrics follow the peer (Q7): publish the RAW renderer
                // anchor; the 30Hz sync engine extrapolates between poll
                // ticks exactly like the position extrapolation below.
                crate::lyrics_sync::publish_remote_anchor(
                    remote.position_ms,
                    remote.updated_at_ms,
                    remote.playing,
                );
                // Duration from the core queue's current track (aligned to the
                // peer's track by the sink). Zero when unknown — clamp is skipped.
                let duration_secs = runtime
                    .core()
                    .current_track()
                    .await
                    .map(|track| track.duration_secs)
                    .unwrap_or(0);
                let duration_ms = duration_secs.saturating_mul(1000);
                // Extrapolate position while playing; clamp to the track length.
                let mut position_ms = remote.position_ms;
                if remote.playing && remote.updated_at_ms > 0 {
                    position_ms = position_ms.saturating_add(now_ms().saturating_sub(remote.updated_at_ms));
                }
                if duration_ms > 0 && position_ms > duration_ms {
                    position_ms = duration_ms;
                }
                let position_secs = position_ms / 1000;
                let progress = if duration_ms > 0 {
                    (position_ms as f32 / duration_ms as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let playing = remote.playing;
                // Reflect the PEER's actual volume on the bar so a drag starts
                // from a safe level (never QBZ's local 100). When the peer hasn't
                // reported a volume, clamp to 50% — the AVR-nuke safety default.
                let remote_volume = remote
                    .volume
                    .map(|v| (v as f32 / 100.0).clamp(0.0, 1.0))
                    .unwrap_or(0.5);
                // Reflect the PEER's shuffle/repeat state on the bar buttons. As
                // controller these were never pushed (only the local toggle paths
                // set them), so the buttons looked dead even when the remote toggle
                // worked. Pure UI reflection of the cloud's reported state — no
                // local order is generated (WS-authoritative for shuffle order).
                let shuffle_on = remote.shuffle_mode;
                let repeat_mode = remote.repeat_mode;
                // Skip the UI hop when nothing the push depends on changed
                // since the last push (see the dirty-guard comment above).
                // While playing, `position_ms` extrapolates every tick, so
                // pushes proceed; paused, the bar stops being repainted.
                // track_id guarantees the push after a peer track change (the
                // meta refresh above just reset the bar's position to 0).
                let remote_snapshot = (
                    remote.track_id,
                    position_ms,
                    duration_secs,
                    playing,
                    remote_volume.to_bits(),
                    shuffle_on,
                    repeat_mode,
                );
                if last_remote_ui_push != Some(remote_snapshot) {
                    last_remote_ui_push = Some(remote_snapshot);
                    // Keep the visualizer producer in step with the same flag
                    // the drain gate reads (a paused PEER parks it too; while
                    // the peer plays, the local buffer is stale — historical
                    // behavior, the bars simply freeze).
                    set_viz_paused(&runtime, !playing);
                    let elapsed = fmt_elapsed(position_secs);
                    let remaining = fmt_remaining(position_secs, duration_secs);
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        let np = w.global::<NowPlayingState>();
                        np.set_position_secs(position_secs as i32);
                        if duration_secs > 0 {
                            np.set_duration_secs(duration_secs as i32);
                        }
                        np.set_progress(progress);
                        np.set_seekable_max(1.0);
                        np.set_elapsed(elapsed.into());
                        np.set_remaining(remaining.into());
                        np.set_playing(playing);
                        np.set_volume(remote_volume);
                        np.set_shuffle(shuffle_on);
                        np.set_repeat_mode(repeat_mode);
                    });
                }
                // A peer owns audio — there is no local fetch wait, so the bar's
                // fetch spinner must never linger here. Force-clear it (only on
                // the edge — avoid re-posting set_loading(false) every tick).
                if PENDING_PLAY_ID.load(std::sync::atomic::Ordering::Relaxed) != 0 {
                    clear_loading(&weak, 0);
                }
                // Reset the LOCAL edge trackers so when control returns to QBZ
                // the end-of-track / gapless / transition logic re-detects from a
                // clean slate (the local player was stopped while peer-active).
                last_track_id = 0;
                was_playing = false;
                seen_position = 0;
                // The peer push owns the bar now — force the local branch to
                // re-push on return even if its raw values coincide.
                last_ui_push = None;
                continue;
            }

            // --- CAST mode: the cast service's own 1s poll owns the bar -------
            // While a Chromecast/DLNA renderer is connected the local player is
            // stopped, so the local path below would push position=0 /
            // playing=false / local-volume every 450ms and fight the cast poll
            // (seekbar flicker, wrong play icon, dead volume bar). Skip it
            // entirely; cast_service::poll_once drives the bar AND publishes the
            // lyrics anchor. Checked BEFORE clear_remote_anchor() below so the
            // local tick doesn't wipe the cast poll's lyrics anchor (auto-follow).
            if let Some(cast) = crate::cast_service::service() {
                if cast.is_casting().await {
                    last_track_id = 0;
                    was_playing = false;
                    seen_position = 0;
                    // The cast poll owns the bar while casting — force a fresh
                    // local push on return even if the raw values coincide.
                    last_ui_push = None;
                    // Same invariant for the PEER edge trackers: the remote
                    // branch above did not run, so without this reset a
                    // re-taken peer whose snapshot happens to match the last
                    // controller push would be skipped, leaving the cast
                    // renderer's values on the bar (mirrors the local path's
                    // reset below).
                    last_peer_track_id = 0;
                    last_remote_ui_push = None;
                    continue;
                }
            }

            // Not in controller mode (no peer / returned to local): reset the
            // peer-track edge var so re-entering the peer state refreshes meta.
            last_peer_track_id = 0;
            last_remote_ui_push = None;
            // Lyrics position source back to the local player (Q7 resolver).
            crate::lyrics_sync::clear_remote_anchor();

            let event = runtime.core().player().get_playback_event();

            let track_id = event.track_id;
            let position = event.position;
            let duration = event.duration;
            let is_playing = event.is_playing;
            let volume = event.volume;
            // DELIVERED stream params (what the engine actually decodes after
            // the streaming-quality downgrade, #590). 0 = unknown / no stream.
            let eff_rate_hz = event.sample_rate.unwrap_or(0);
            let eff_bits = event.bit_depth.unwrap_or(0);
            // Persist the live position periodically (~5s) while playing so a
            // crash keeps a near-current resume point (no-op unless
            // `persist_session` is on; `position` is in seconds).
            save_pos_tick = save_pos_tick.wrapping_add(1);
            if is_playing && track_id != 0 && save_pos_tick % 11 == 0 {
                crate::session_persist::save_position(position);
            }
            // Streaming buffer fill, for the seek-bar cache overlay.
            let cache = event.buffer_progress.unwrap_or(0.0);
            // Seek lock: while streaming (`buffer_progress` is Some), the user
            // can only seek up to what has downloaded; fully-available tracks
            // (None) seek freely.
            let seekable_max = event.buffer_progress.map(|p| p.clamp(0.0, 1.0)).unwrap_or(1.0);

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
                // The audio engine advanced to `track_id` on its own — EITHER
                // a real gapless hand-off (it started the prefetched next
                // track) OR a manual new-track play that just replaced the
                // queue. Rather than guess which (the old peek-based heuristic
                // missed cases and left the card stale while the seek bar kept
                // moving — the reported populate bug), reconcile the queue
                // pointer + the now-playing card to whatever is ACTUALLY
                // playing. `sync_current_to_id` moves the pointer only when it
                // lags (a real advance); a manual play already moved it, so it
                // reports `moved == false` and we skip the double bookkeeping.
                if let Some((_, moved)) =
                    runtime.core().sync_current_to_id(track_id).await
                {
                    // Always refresh so title/art/meta match the live track.
                    refresh_now_playing_meta(&runtime, &weak).await;
                    // Pair the sidebar NOW PLAYING repaint with the NPB repaint
                    // UNCONDITIONALLY: the sidebar's QueueState.now-playing is a
                    // persistent property that otherwise holds a prior queue's
                    // track when the pointer was already aligned (moved==false,
                    // e.g. a manual play that set the queue before the audio
                    // surfaced the new id). `false` avoids a per-transition fav
                    // network pull. record_recent/kick_prefetch stay moved-gated
                    // below (they must not double-fire on a non-move).
                    refresh_sidebar(false);
                    if moved {
                        log::info!(
                            "[qbz-slint] [GAPLESS] seamless transition {last_track_id} -> {track_id}"
                        );
                        record_recent(&runtime).await;
                        refresh_sidebar(true);
                        // Prefetch the successors of the now-current track.
                        kick_prefetch(&runtime).await;
                    }
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
            // Stop-after guard (last condition, short-circuited): if the CURRENT
            // track is marked "stop after this", suppress the gapless pre-queue so
            // it ends naturally and the track-end handler can fire
            // `consume_stop_after_if`. Mirrors the Tauri `setGaplessGetNextTrackId`
            // null-return for the marked track. Without this the engine seamlessly
            // hands off and the marker never fires.
            if event.gapless_ready
                && event.gapless_next_track_id == 0
                && track_id != 0
                && gapless_requested_for != track_id
                && runtime.core().get_stop_after().await != Some(track_id)
            {
                let upcoming = runtime.core().peek_upcoming(1).await;
                if let Some(next) = upcoming.into_iter().next() {
                    // Never queue the current track as its own next. Offline,
                    // an unavailable successor is not pre-queued either (the
                    // same playable rule as the advance walk) — the track-end
                    // auto-advance then skips it properly instead of the
                    // engine gapless-handing into a refused fetch.
                    if next.id != track_id && !next.is_local && offline_track_playable(&next) {
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
                                    playback_quality(),
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
                    } else if next.id != track_id && next.is_local {
                        // LOCAL gapless (DSD plan Phase 2): resolve the file
                        // path and hand it to the engine's gapless queue —
                        // DSD goes through play_next_dsd (DoP append when a
                        // DoP stream is live, else an in-memory converted
                        // WAV); other local formats feed their raw bytes to
                        // the normal play_next. CUE virtual tracks are
                        // skipped (they share one album image file).
                        gapless_requested_for = track_id;
                        let runtime = runtime.clone();
                        let next_id = next.id;
                        tokio::spawn(async move {
                            let info = if crate::ephemeral::is_ephemeral_id(next_id as i64) {
                                crate::ephemeral::get_track(next_id as i64)
                                    .map(|t| (t.file_path, t.cue_start_secs))
                            } else {
                                tokio::task::spawn_blocking(move || {
                                    crate::library_db::with_db(|db| db.get_track(next_id as i64))
                                })
                                .await
                                .ok()
                                .flatten()
                                .flatten()
                                .map(|t| (t.file_path, t.cue_start_secs))
                            };
                            let Some((path, cue)) = info else { return };
                            if cue.is_some() {
                                return;
                            }
                            let rt2 = runtime.clone();
                            let res = tokio::task::spawn_blocking(move || {
                                let p = std::path::PathBuf::from(&path);
                                let player = rt2.core().player();
                                if qbz_dsd::is_dsd_path(&p) {
                                    player.play_next_dsd(p, next_id)
                                } else {
                                    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
                                    player.play_next(bytes, next_id)
                                }
                            })
                            .await;
                            match res {
                                Ok(Ok(())) => log::info!(
                                    "[qbz-slint] [GAPLESS] queued local track {next_id} for gapless"
                                ),
                                Ok(Err(e)) => log::info!(
                                    "[qbz-slint] [GAPLESS] local pre-queue {next_id} skipped: {e}"
                                ),
                                Err(e) => log::warn!(
                                    "[qbz-slint] [GAPLESS] local pre-queue task failed: {e}"
                                ),
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

            // Push the live values onto NowPlayingState — but only when
            // something the push depends on actually changed (see the
            // dirty-guard comment above). While playing, `position` advances,
            // so pushes proceed; fully idle (track_id == 0, nothing playing)
            // the UI hop is skipped entirely and the window stays clean.
            // The purchases mirror is a pure function of (track_id,
            // is_playing) — both in the snapshot — so it is safely skipped
            // with the rest. The miniplayer mirror is NOT: it fans out
            // main-window state that changes without moving this snapshot
            // (async meta/lyrics/artwork arrivals, mute, cast/qconnect
            // flags), so while the mini is open it keeps ticking below.
            let ui_snapshot = (
                track_id,
                position,
                duration,
                is_playing,
                volume.to_bits(),
                cache.to_bits(),
                seekable_max.to_bits(),
                eff_rate_hz,
                eff_bits,
            );
            if last_ui_push != Some(ui_snapshot) {
                last_ui_push = Some(ui_snapshot);
                // Effective-vs-max quality (#590 follow-up): the badge keeps
                // advertising the catalog max; when the DELIVERED stream is
                // below it, flip the downgrade flag + format the true line for
                // the AudioStamp tooltip. The 0.9 rate guard avoids flagging
                // 44.1-vs-48 kHz family mismatches (Tauri QualityBadge.svelte
                // parity). DSD (nominal 1-bit, on either side) is exempt from
                // the bit-depth comparison — its depth is not comparable to
                // PCM's (mirrors the DSD special-case in the meta seed).
                let max_rate_hz =
                    TRACK_MAX_RATE_HZ.load(std::sync::atomic::Ordering::Relaxed);
                let max_bits = TRACK_MAX_BITS.load(std::sync::atomic::Ordering::Relaxed);
                let dsd = max_bits == 1 || eff_bits == 1;
                // DSD is exempt ENTIRELY (not just the depth arm): past Phase 1
                // the DoP/native paths play DSD bit-perfect, and its carrier/PCM
                // rate vs the DSD "max" is apples-to-oranges, so any compare would
                // flag a false downgrade arrow on a bit-perfect DSD stream.
                let downgraded = !dsd
                    && ((eff_rate_hz > 0
                        && max_rate_hz > 0
                        && (eff_rate_hz as f64) < max_rate_hz as f64 * 0.9)
                        || (eff_bits > 0 && max_bits > 0 && eff_bits < max_bits));
                // True delivered line, via the shared formatter so it matches
                // the badge style ("16-bit / 44.1 kHz"). Native DSD streams
                // (1-bit) go through the DSD label instead — the generic
                // detail would read "1-bit / 2822.4 kHz".
                let true_detail = if eff_bits == 1 {
                    crate::quality::dsd_multiple_label(
                        (eff_rate_hz > 0).then_some(eff_rate_hz as f64),
                    )
                } else if eff_rate_hz > 0 || eff_bits > 0 {
                    crate::quality::detail(
                        (eff_bits > 0).then_some(eff_bits),
                        (eff_rate_hz > 0).then_some(eff_rate_hz as f64),
                    )
                } else {
                    String::new()
                };
                // Mirror engine truth onto the visualizer tap alongside the
                // set_playing push below. This is the catch-all: EVERY local
                // transition (pause/resume from any surface — MPRIS, tray,
                // hotkey, QConnect renderer command — plus stop, track end,
                // seek-while-paused snapshots) lands here within one 450ms
                // tick; the direct edge sites above only shave latency.
                set_viz_paused(&runtime, !is_playing);
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
                    np.set_seekable_max(seekable_max);
                    np.set_elapsed(elapsed.into());
                    np.set_remaining(remaining.into());
                    np.set_playing(is_playing);
                    np.set_volume(volume.clamp(0.0, 1.0));
                    // Effective quality for the downgrade arrow + tooltip.
                    np.set_effective_sample_rate_hz(eff_rate_hz as i32);
                    np.set_effective_bit_depth(eff_bits as i32);
                    np.set_quality_downgraded(downgraded);
                    np.set_quality_true_detail(true_detail.into());
                    // Keep the Purchases globals in step with play/pause + the live
                    // track id every tick (the meta-apply seeds them on a track
                    // change; this follows pause/resume + the engine's own id flips).
                    // track_id 0 (idle) → "" active id (no row highlighted).
                    mirror_now_playing_to_purchases(&w, track_id, is_playing);
                    // REQ-1 fan-out: mirror to the miniplayer window (no-op when
                    // the mini is closed). Single tick, no second poll loop.
                    crate::miniplayer::mirror_tick(&w);
                });
            } else if crate::miniplayer::is_open() {
                // Snapshot unchanged, but the mini window is open: its mirror
                // must still tick (it copies main-window state — lyrics
                // status/lines, meta, artwork on the track edge — that other
                // async paths update without moving the playback snapshot).
                let _ = weak.upgrade_in_event_loop(move |w| {
                    crate::miniplayer::mirror_tick(&w);
                });
            }

            // Clear the fetch spinner once the audio for the in-flight play is
            // actually advancing: a non-zero track with the clock moving
            // (`position > 0`) is unambiguous proof the requested track started
            // (is_playing alone can flip true transiently before the sink emits
            // the id). Keyed to PENDING_PLAY_ID so a superseded fetch doesn't
            // wipe a newer play's spinner; the keyed clear is a no-op if the
            // current audio is a different (already-cleared) id.
            if track_id != 0 && is_playing && position > 0 {
                clear_loading(&weak, track_id);
                // Real audio ends any unavailable-skip streak (Tauri parity:
                // `consecutiveSkips = 0` on successful play).
                UNAVAILABLE_SKIPS.store(0, std::sync::atomic::Ordering::SeqCst);
            } else {
                // Watchdog: a play the engine accepted but that never advances
                // (undecodable-but-valid-looking file, zero-frame stream) would
                // otherwise spin forever. Force-clear after the generous ceiling.
                let pending = PENDING_PLAY_ID.load(std::sync::atomic::Ordering::Relaxed);
                if pending != 0
                    && now_ms().saturating_sub(
                        PENDING_PLAY_AT_MS.load(std::sync::atomic::Ordering::Relaxed),
                    ) > LOADING_WATCHDOG_MS
                {
                    log::warn!(
                        "[qbz-slint] loading watchdog: track {pending} never started after {}ms, clearing spinner",
                        LOADING_WATCHDOG_MS
                    );
                    clear_loading(&weak, 0);
                }
            }

            // --- QConnect: outbound renderer state report -----------------
            // When QBZ is the ACTIVE LOCAL renderer (controlled by a remote
            // controller like the iOS app), report our playback state so the
            // controller's seek bar + current-track follow. Mirrors the official
            // client: position/duration in MILLISECONDS, ~2s periodic while
            // playing + an immediate report on every transition. The service
            // self-gates on is_local_renderer_active (no-op when we're a peer
            // controller or not connected) and resolves the queue_item_ids.
            report_tick = report_tick.wrapping_add(1);
            if track_id != 0 {
                let transition =
                    track_id != last_reported_track_id || is_playing != last_reported_playing;
                let periodic = is_playing && report_tick % QCONNECT_REPORT_EVERY_N_TICKS == 0;
                if transition || periodic {
                    if let Some(svc) = crate::qconnect_service::service() {
                        let playing_state = if is_playing {
                            PLAYING_STATE_PLAYING
                        } else {
                            PLAYING_STATE_PAUSED
                        };
                        let position_ms = (position as i64) * 1000;
                        let duration_ms = (duration as i64) * 1000;
                        svc.report_playback_state(playing_state, position_ms, duration_ms, track_id)
                            .await;
                        // On a track change, also reconcile the session queue: if the
                        // user started a new album/playlist on QBZ, push it so the
                        // controller (iOS) follows. Self-gates + echo-suppresses.
                        if transition {
                            svc.sync_local_queue_if_changed().await;
                        }
                    }
                    last_reported_track_id = track_id;
                    last_reported_playing = is_playing;
                }
            }

            if track_id != 0 {
                last_track_id = track_id;
                seen_position = position;
            }
            // Reflect play/pause into the tray tooltip on transition only
            // (Linux), so the "Middle-click to pause/play" hint stays correct
            // without spamming the updater channel every tick.
            if is_playing != was_playing {
                if let Some(t) = crate::tray::handle() {
                    t.set_playing(is_playing);
                }
                if let Some(mc) = crate::media_controls::handle() {
                    let status = if is_playing {
                        qbz_media_controls::PlaybackStatus::Playing
                    } else {
                        qbz_media_controls::PlaybackStatus::Paused
                    };
                    mc.set_playback(status, Some(std::time::Duration::from_secs(position as u64)));
                }
                // Discord Rich Presence: re-push on the play/pause edge so the
                // "Playing" / "Paused at mm:ss" state + timestamps stay correct
                // (no-op when not opted in). Mirrors the Tauri service.
                crate::discord_rpc::push(&runtime, &tokio::runtime::Handle::current());
            }
            was_playing = is_playing;

            // Auto-advance on track end. Offline, unavailable tracks are
            // skipped (bounded — see `advance_to_playable`); exhaustion
            // lands in the queue-finished arm below.
            if track_ended {
                // Seed for InfiniteRadio: the track that just ended is still the
                // current one (advance hasn't moved the cursor yet).
                let ended_track_id = runtime
                    .core()
                    .current_track()
                    .await
                    .map(|t| t.id)
                    .unwrap_or(0);
                // Stop-after-this-song: if the track that just ended is the marked
                // one, HALT here (pause) — do NOT advance, do NOT infinite-refill.
                // The queue stays intact and the finished track stays parked in
                // now-playing. `consume_stop_after_if` is one-shot (clears the
                // marker). Mirrors the Tauri end-of-track `consumeStopAfterIf` ->
                // stopPlayback + early-return, ahead of any repeat/shuffle.
                if ended_track_id != 0
                    && runtime.core().consume_stop_after_if(ended_track_id).await
                {
                    if let Err(e) = runtime.core().pause() {
                        log::warn!("[qbz-slint] stop-after: pause failed: {e}");
                    }
                    // Stop counts as paused for the visualizer tap.
                    set_viz_paused(&runtime, true);
                    last_track_id = 0;
                    was_playing = false;
                    seen_position = 0;
                    gapless_requested_for = 0;
                    refresh_sidebar(true);
                    let _ = weak.upgrade_in_event_loop(|w| {
                        let np = w.global::<NowPlayingState>();
                        np.set_playing(false);
                    });
                    continue;
                }
                last_track_id = 0;
                was_playing = false;
                seen_position = 0;
                gapless_requested_for = 0;
                if let Some(track) = advance_to_playable(&runtime, &weak, true).await {
                    let next_id = track.id;
                    after_track_change(&runtime, &weak, next_id).await;
                    refresh_sidebar(true);
                } else if try_infinite_refill(&runtime, &weak, ended_track_id).await {
                    // InfiniteRadio started a fresh smart radio (play_tracks
                    // replaced the queue and refreshes the sidebar itself).
                } else {
                    log::info!("[qbz-slint] playback: queue finished");
                    // Nothing more will play — force-clear any lingering spinner
                    // and park the visualizer producer (stop counts as paused).
                    clear_loading(&weak, 0);
                    set_viz_paused(&runtime, true);
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
