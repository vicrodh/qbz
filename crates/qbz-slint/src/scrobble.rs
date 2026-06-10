//! Settings > Integrations — scrobbler (Last.fm + ListenBrainz) auth controller
//! AND the source-agnostic now-playing / scrobble fire.
//!
//! Source-agnostic by construction: the fire path reads the CURRENT
//! `qbz_models::QueueTrack`'s already-normalized `artist` / `album` / title
//! (Qobuz, local, AND Plex all funnel through it) and feeds plain text — no
//! artwork (Last.fm has no image field; ListenBrainz takes only optional MB
//! IDs / ISRC / duration). The clients live in `qbz-integrations` and are
//! called directly (no Tauri command seam).
//!
//! Two firing edges (mirrors the Svelte `playbackService.ts`):
//!   - now-playing: fires immediately on a track-change edge (skipped offline).
//!   - scrobble: armed at `min(50% of duration, 240s)` after the change; a
//!     monotonic `SCROBBLE_GEN` guard self-cancels a stale timer if the track
//!     changed before it fires (the Svelte `clearTimeout` equivalent). Like
//!     Tauri, pause does NOT stop the clock.
//!
//! Offline behavior: engine offline OR call failure queues the scrobble —
//! Last.fm into the SHARED per-user `offline_settings.db` `scrobble_queue`
//! (same rows Tauri queues/flushes), ListenBrainz into the SHARED per-user
//! `listenbrainz_v2.db` `listen_queue`. A watcher on the offline-mode engine
//! drains both queues on every offline -> online edge (manual-flag exits
//! included), plus once at shell entry.
//!
//! Persistence lives in `crate::scrobbler_settings` (the per-user
//! `scrobbler_settings.db`); the auth flows seed/clear it. ListenBrainz
//! credentials are ALSO written through to the shared `ListenBrainzCache`
//! credentials row, so the Tauri build sees the same sign-in (and a Tauri
//! sign-in seeds this build at shell entry).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use slint::{ComponentHandle, Weak};

use qbz_app::offline_mode::OfflineModeStore;
use qbz_integrations::listenbrainz::cache::ListenBrainzCache;
use qbz_integrations::listenbrainz::AdditionalInfo;
use qbz_integrations::{LastFmClient, ListenBrainzClient};

use crate::scrobbler_settings;
use crate::{AppWindow, ScrobbleState};

// ----------------------------------------------------------------------------
// Status helper — Slint uses inline @tr, so we resolve to a plain English
// label Rust-side. `kind`: 0 none, 1 info, 2 connected/ok, 3 error. Mirrors
// `plex_auth::set_status`.
// ----------------------------------------------------------------------------

fn set_status(weak: &Weak<AppWindow>, text: String, kind: i32) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<ScrobbleState>();
        s.set_status_text(text.into());
        s.set_status_kind(kind);
    });
}

// ----------------------------------------------------------------------------
// Last.fm pending-token bridge. `get_token` returns a request token the user
// authorizes in the browser; `get_session` (the confirm step) exchanges it.
// The two steps are separate UI callbacks, so the token is stashed here.
// ----------------------------------------------------------------------------

static LASTFM_PENDING_TOKEN: Mutex<Option<String>> = Mutex::new(None);

// ----------------------------------------------------------------------------
// Tokio runtime handle — captured at shell entry so the fire path (which runs
// from the playback poll, not a UI callback) can spawn network tasks.
// ----------------------------------------------------------------------------

static RT_HANDLE: Mutex<Option<tokio::runtime::Handle>> = Mutex::new(None);

fn rt_handle() -> Option<tokio::runtime::Handle> {
    RT_HANDLE.lock().ok().and_then(|g| g.clone())
}

/// One-shot guard for the engine-watch flush task (lives for the process).
static FLUSH_WATCHER: OnceLock<()> = OnceLock::new();

/// Per-user runtime start, called from `init_shell_for_user` AFTER
/// `scrobbler_settings::init_for_user`. Captures the tokio handle for the
/// fire path, seeds ListenBrainz credentials from the SHARED cache when this
/// build has none (a Tauri sign-in carries over), starts the offline-engine
/// flush watcher (once per process), and kicks an initial queue flush.
pub fn start(handle: tokio::runtime::Handle) {
    if let Ok(mut g) = RT_HANDLE.lock() {
        *g = Some(handle.clone());
    }

    handle.spawn(async move {
        seed_listenbrainz_from_shared_cache().await;
        if !crate::offline_mode::engine().is_offline() {
            flush_offline_queues().await;
        }
    });

    let watcher_handle = handle.clone();
    FLUSH_WATCHER.get_or_init(move || {
        watcher_handle.spawn(async move {
            let mut rx = crate::offline_mode::engine().subscribe();
            let mut was_offline = rx.borrow_and_update().is_offline();
            loop {
                if rx.changed().await.is_err() {
                    break;
                }
                let offline = rx.borrow_and_update().is_offline();
                if was_offline && !offline {
                    log::info!("[qbz-slint] scrobblers: back online, flushing queues");
                    flush_offline_queues().await;
                }
                was_offline = offline;
            }
        });
    });
}

/// Adopt the shared `ListenBrainzCache` credentials (the row the Tauri build
/// persists to) when this build's store has no LB token yet. Enable flags are
/// NOT touched — scrobbling stays opt-in per build.
async fn seed_listenbrainz_from_shared_cache() {
    if scrobbler_settings::get().listenbrainz_is_authed() {
        return;
    }
    let Some(path) = listenbrainz_cache_path() else {
        return;
    };
    let creds = tokio::task::spawn_blocking(move || {
        ListenBrainzCache::new(&path).and_then(|c| c.get_credentials())
    })
    .await;
    if let Ok(Ok((Some(token), Some(user_name)))) = creds {
        if !token.is_empty() {
            log::info!("[qbz-slint] adopting ListenBrainz credentials from shared cache");
            scrobbler_settings::set_listenbrainz_token(&token, &user_name);
        }
    }
}

// ============================================================================
// Auth + settings callbacks (bound in main.rs).
// ============================================================================

/// Panel init: seed `ScrobbleState` from the persisted store.
pub fn load(weak: Weak<AppWindow>) {
    let cfg = scrobbler_settings::get();
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<ScrobbleState>();
        s.set_enabled(cfg.enabled);
        s.set_ui_collapsed(cfg.ui_collapsed);
        s.set_lastfm_enabled(cfg.lastfm_enabled);
        s.set_lastfm_authed(cfg.lastfm_is_authed());
        s.set_lastfm_username(cfg.lastfm_username.clone().into());
        s.set_lastfm_auth_url("".into());
        s.set_lastfm_busy(false);
        s.set_listenbrainz_enabled(cfg.listenbrainz_enabled);
        s.set_listenbrainz_authed(cfg.listenbrainz_is_authed());
        s.set_listenbrainz_username(cfg.listenbrainz_username.clone().into());
        s.set_listenbrainz_token_input("".into());
        s.set_listenbrainz_busy(false);
        s.set_status_text("".into());
        s.set_status_kind(0);
    });
}

pub fn enable_toggle(weak: Weak<AppWindow>, enabled: bool) {
    scrobbler_settings::set_enabled(enabled);
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.global::<ScrobbleState>().set_enabled(enabled);
    });
}

pub fn collapse_toggle(collapsed: bool) {
    scrobbler_settings::set_ui_collapsed(collapsed);
}

// --- Last.fm -----------------------------------------------------------------

pub fn lastfm_enable_toggle(weak: Weak<AppWindow>, enabled: bool) {
    scrobbler_settings::set_lastfm_enabled(enabled);
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.global::<ScrobbleState>().set_lastfm_enabled(enabled);
    });
}

/// Step 1: request a token, open the Last.fm authorize URL in the browser, and
/// reveal the "Finish" affordance. Mirrors the Svelte `v2_lastfm_get_auth_url`
/// + open path (system browser, like `plex_auth::open_auth_url`).
pub fn lastfm_connect(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let _ = weak.upgrade_in_event_loop(|w| w.global::<ScrobbleState>().set_lastfm_busy(true));
    handle.spawn(async move {
        let client = LastFmClient::new();
        match client.get_token().await {
            Ok((token, auth_url)) => {
                if let Ok(mut g) = LASTFM_PENDING_TOKEN.lock() {
                    *g = Some(token);
                }
                let auth_url_ui = auth_url.clone();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let s = w.global::<ScrobbleState>();
                    s.set_lastfm_busy(false);
                    s.set_lastfm_auth_url(auth_url_ui.into());
                });
                // Open the browser to authorize.
                if let Err(e) = open::that(&auth_url) {
                    log::warn!("[qbz-slint] open Last.fm auth url failed: {e}");
                }
                set_status(
                    &weak,
                    "Authorize QBZ in your browser, then click \"Finish\"".to_string(),
                    1,
                );
            }
            Err(e) => {
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<ScrobbleState>().set_lastfm_busy(false);
                });
                set_status(&weak, format!("Error: {e}"), 3);
                crate::toast::error_weak(&weak, "Last.fm sign-in failed to start");
            }
        }
    });
}

/// Re-open the stored authorize URL (in case the browser did not launch).
pub fn lastfm_open_auth_url(weak: Weak<AppWindow>) {
    let url = weak
        .upgrade()
        .map(|w| w.global::<ScrobbleState>().get_lastfm_auth_url().to_string())
        .unwrap_or_default();
    if url.is_empty() {
        return;
    }
    if let Err(e) = open::that(&url) {
        log::warn!("[qbz-slint] open Last.fm auth url failed: {e}");
    }
}

/// Step 2 (the user clicked "Finish" after authorizing): exchange the pending
/// token for a session key + username and persist it. Mirrors
/// `v2_lastfm_complete_auth`.
pub fn lastfm_confirm(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let token = LASTFM_PENDING_TOKEN.lock().ok().and_then(|g| g.clone());
    let Some(token) = token else {
        set_status(&weak, "Start the sign-in first".to_string(), 3);
        return;
    };
    let _ = weak.upgrade_in_event_loop(|w| w.global::<ScrobbleState>().set_lastfm_busy(true));
    handle.spawn(async move {
        let mut client = LastFmClient::new();
        match client.get_session(&token).await {
            Ok(session) => {
                scrobbler_settings::set_lastfm_session(&session.key, &session.name);
                // Default the per-service flag ON the first time we connect, so
                // scrobbling starts without an extra toggle.
                if !scrobbler_settings::get().lastfm_enabled {
                    scrobbler_settings::set_lastfm_enabled(true);
                }
                if let Ok(mut g) = LASTFM_PENDING_TOKEN.lock() {
                    *g = None;
                }
                let username = session.name.clone();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let s = w.global::<ScrobbleState>();
                    s.set_lastfm_busy(false);
                    s.set_lastfm_authed(true);
                    s.set_lastfm_enabled(true);
                    s.set_lastfm_username(username.into());
                    s.set_lastfm_auth_url("".into());
                });
                set_status(&weak, format!("Connected as {}", session.name), 2);
            }
            Err(e) => {
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<ScrobbleState>().set_lastfm_busy(false);
                });
                set_status(
                    &weak,
                    format!("Error: {e} (did you authorize in the browser?)"),
                    3,
                );
            }
        }
    });
}

pub fn lastfm_disconnect(weak: Weak<AppWindow>) {
    scrobbler_settings::disconnect_lastfm();
    if let Ok(mut g) = LASTFM_PENDING_TOKEN.lock() {
        *g = None;
    }
    let _ = weak.upgrade_in_event_loop(|w| {
        let s = w.global::<ScrobbleState>();
        s.set_lastfm_authed(false);
        s.set_lastfm_username("".into());
        s.set_lastfm_auth_url("".into());
        s.set_lastfm_busy(false);
    });
    set_status(&weak, "Last.fm disconnected".to_string(), 1);
}

// --- ListenBrainz ------------------------------------------------------------

pub fn listenbrainz_enable_toggle(weak: Weak<AppWindow>, enabled: bool) {
    scrobbler_settings::set_listenbrainz_enabled(enabled);
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.global::<ScrobbleState>().set_listenbrainz_enabled(enabled);
    });
}

/// Save + validate a ListenBrainz user token. Mirrors `v2_listenbrainz_connect`
/// — validated against `/validate-token`, then persisted to this build's store
/// AND the shared `ListenBrainzCache` (so the Tauri build picks it up).
pub fn listenbrainz_set_token(weak: Weak<AppWindow>, handle: tokio::runtime::Handle, token: String) {
    let token = token.trim().to_string();
    if token.is_empty() {
        set_status(&weak, "Paste your ListenBrainz user token first".to_string(), 3);
        return;
    }
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<ScrobbleState>().set_listenbrainz_busy(true)
    });
    handle.spawn(async move {
        let client = ListenBrainzClient::new();
        match client.set_token(&token).await {
            Ok(info) => {
                scrobbler_settings::set_listenbrainz_token(&token, &info.user_name);
                if !scrobbler_settings::get().listenbrainz_enabled {
                    scrobbler_settings::set_listenbrainz_enabled(true);
                }
                // Write-through to the shared cache (Tauri reads it at session
                // start). Best-effort.
                if let Some(path) = listenbrainz_cache_path() {
                    let tok = token.clone();
                    let name = info.user_name.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        ListenBrainzCache::new(&path)
                            .and_then(|c| c.save_credentials(&tok, &name))
                    })
                    .await;
                }
                let username = info.user_name.clone();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let s = w.global::<ScrobbleState>();
                    s.set_listenbrainz_busy(false);
                    s.set_listenbrainz_authed(true);
                    s.set_listenbrainz_enabled(true);
                    s.set_listenbrainz_username(username.into());
                    s.set_listenbrainz_token_input("".into());
                });
                set_status(&weak, format!("Connected as {}", info.user_name), 2);
            }
            Err(e) => {
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<ScrobbleState>().set_listenbrainz_busy(false);
                });
                set_status(&weak, format!("Error: {e}"), 3);
            }
        }
    });
}

pub fn listenbrainz_disconnect(weak: Weak<AppWindow>) {
    scrobbler_settings::disconnect_listenbrainz();
    // Clear the shared cache credentials too (mirrors Tauri's disconnect).
    if let Some(path) = listenbrainz_cache_path() {
        if let Some(handle) = rt_handle() {
            handle.spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    ListenBrainzCache::new(&path).and_then(|c| c.clear_credentials())
                })
                .await;
            });
        }
    }
    let _ = weak.upgrade_in_event_loop(|w| {
        let s = w.global::<ScrobbleState>();
        s.set_listenbrainz_authed(false);
        s.set_listenbrainz_username("".into());
        s.set_listenbrainz_token_input("".into());
        s.set_listenbrainz_busy(false);
    });
    set_status(&weak, "ListenBrainz disconnected".to_string(), 1);
}

// ============================================================================
// Fire + schedule (source-agnostic; called from `refresh_now_playing_meta`).
// ============================================================================

/// Normalized track facts the fire path needs. Built from the CURRENT
/// `QueueTrack` on the de-duped track-change edge; the title is the
/// version-enriched display title so remixes/editions scrobble correctly
/// (issue #360 parity with the Svelte `formatTrackTitle` path).
#[derive(Clone)]
pub struct ScrobbleMeta {
    pub artist: String,
    pub track: String,
    /// `None` when empty — clients take `Option<&str>` for album.
    pub album: Option<String>,
    pub duration_secs: u64,
}

/// Monotonic generation, bumped on every track change so a delayed scrobble
/// timer that fires after the user skipped is dropped (the Svelte
/// `clearTimeout` equivalent). Like Tauri, pause/stop do NOT cancel it.
static SCROBBLE_GEN: AtomicU64 = AtomicU64::new(0);

/// `min(50% of duration, 240s)` in seconds — the Last.fm rule, applied to both
/// services (matches the Svelte `Math.min(durationSecs * 0.5, 240) * 1000`).
fn scrobble_delay_secs(duration_secs: u64) -> u64 {
    (duration_secs / 2).min(240)
}

/// Track-change entry point. Fires now-playing immediately for each enabled +
/// authed service, then arms a delayed scrobble. No-op when no service is
/// active. Called from `refresh_now_playing_meta` on the de-duped track-change
/// edge (after the QConnect peer-active gate), so it is NOT re-armed on
/// resume/seek and never fires for a remote renderer's audio.
pub fn on_track_changed(meta: ScrobbleMeta) {
    // Always bump the generation so any in-flight stale timer self-cancels.
    let my_gen = SCROBBLE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let cfg = scrobbler_settings::get();
    if !cfg.lastfm_active() && !cfg.listenbrainz_active() {
        return;
    }
    let Some(handle) = rt_handle() else {
        return;
    };
    handle.spawn(async move {
        // Now-playing immediately (skipped while offline — needs network and
        // is not worth queueing; matches the Svelte path).
        if !crate::offline_mode::engine().is_offline() {
            send_now_playing(&meta, &cfg).await;
        }

        // Delayed scrobble at min(dur/2, 240s).
        let wait = scrobble_delay_secs(meta.duration_secs);
        if wait > 0 {
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
        // Self-cancel if a newer track change superseded us.
        if SCROBBLE_GEN.load(Ordering::SeqCst) != my_gen {
            return;
        }
        send_scrobble(&meta).await;
    });
}

/// Optional ListenBrainz extras — duration is the only one the QueueTrack
/// carries (no ISRC / MB IDs on the queue model yet).
fn lb_info(duration_secs: u64) -> Option<AdditionalInfo> {
    Some(AdditionalInfo {
        duration_ms: (duration_secs > 0).then_some(duration_secs * 1000),
        ..Default::default()
    })
}

/// Fire "now playing" for each enabled service. Failures only log — the
/// scrobble path is what queues.
async fn send_now_playing(meta: &ScrobbleMeta, cfg: &scrobbler_settings::ScrobblerSettings) {
    let album = meta.album.as_deref();
    if cfg.lastfm_active() {
        let client = LastFmClient::with_session_key(cfg.lastfm_session_key.clone());
        if let Err(e) = client
            .update_now_playing(&meta.artist, &meta.track, album)
            .await
        {
            log::debug!("[qbz-slint] Last.fm now-playing failed: {e}");
        }
    }
    if cfg.listenbrainz_active() {
        let client = ListenBrainzClient::new();
        client
            .restore_token(cfg.listenbrainz_token.clone(), cfg.listenbrainz_username.clone())
            .await;
        if let Err(e) = client
            .submit_playing_now(&meta.artist, &meta.track, album, lb_info(meta.duration_secs))
            .await
        {
            log::debug!("[qbz-slint] ListenBrainz now-playing failed: {e}");
        }
    }
}

/// Fire the actual scrobble for each enabled service. Engine offline OR call
/// failure queues it — Last.fm to the shared `scrobble_queue`, ListenBrainz to
/// the shared `listen_queue`. Re-reads settings in case the user disconnected
/// while the timer waited.
async fn send_scrobble(meta: &ScrobbleMeta) {
    let cfg = scrobbler_settings::get();
    let album = meta.album.as_deref();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let offline = crate::offline_mode::engine().is_offline();

    if cfg.lastfm_active() {
        let sent = if offline {
            false
        } else {
            let client = LastFmClient::with_session_key(cfg.lastfm_session_key.clone());
            match client
                .scrobble(&meta.artist, &meta.track, album, timestamp as u64)
                .await
            {
                Ok(()) => {
                    log::info!(
                        "[qbz-slint] Last.fm scrobbled: {} - {}",
                        meta.artist,
                        meta.track
                    );
                    true
                }
                Err(e) => {
                    log::warn!("[qbz-slint] Last.fm scrobble failed ({e}); queueing for later");
                    false
                }
            }
        };
        if !sent {
            queue_lastfm(meta, timestamp).await;
        }
    }

    if cfg.listenbrainz_active() {
        let sent = if offline {
            false
        } else {
            let client = ListenBrainzClient::new();
            client
                .restore_token(cfg.listenbrainz_token.clone(), cfg.listenbrainz_username.clone())
                .await;
            match client
                .submit_listen(
                    &meta.artist,
                    &meta.track,
                    album,
                    timestamp,
                    lb_info(meta.duration_secs),
                )
                .await
            {
                Ok(()) => {
                    log::info!(
                        "[qbz-slint] ListenBrainz scrobbled: {} - {}",
                        meta.artist,
                        meta.track
                    );
                    true
                }
                Err(e) => {
                    log::warn!(
                        "[qbz-slint] ListenBrainz scrobble failed ({e}); queueing for later"
                    );
                    false
                }
            }
        };
        if !sent {
            queue_listenbrainz(meta, timestamp).await;
        }
    }
}

/// Queue a Last.fm scrobble into the SHARED per-user `offline_settings.db`
/// `scrobble_queue` (the table Tauri queues into and flushes from).
async fn queue_lastfm(meta: &ScrobbleMeta, timestamp: i64) {
    let Some(dir) = scrobbler_settings::user_dir() else {
        return;
    };
    let artist = meta.artist.clone();
    let track = meta.track.clone();
    let album = meta.album.clone();
    let _ = tokio::task::spawn_blocking(move || {
        match OfflineModeStore::new_at(&dir) {
            Ok(store) => {
                if let Err(e) = store.queue_scrobble(&artist, &track, album.as_deref(), timestamp)
                {
                    log::warn!("[qbz-slint] queue Last.fm scrobble failed: {e}");
                }
            }
            Err(e) => log::warn!("[qbz-slint] open offline settings store failed: {e}"),
        }
    })
    .await;
}

/// Queue a ListenBrainz listen into the SHARED per-user
/// `ListenBrainzCache.listen_queue` (the canonical LB offline store).
async fn queue_listenbrainz(meta: &ScrobbleMeta, timestamp: i64) {
    let Some(path) = listenbrainz_cache_path() else {
        return;
    };
    let artist = meta.artist.clone();
    let track = meta.track.clone();
    let album = meta.album.clone();
    let duration_ms = (meta.duration_secs > 0).then_some(meta.duration_secs * 1000);
    let _ = tokio::task::spawn_blocking(move || match ListenBrainzCache::new(&path) {
        Ok(cache) => {
            if let Err(e) = cache.queue_listen(
                timestamp,
                &artist,
                &track,
                album.as_deref(),
                None,
                None,
                None,
                None,
                duration_ms,
            ) {
                log::warn!("[qbz-slint] queue ListenBrainz listen failed: {e}");
            }
        }
        Err(e) => log::warn!("[qbz-slint] open ListenBrainz cache failed: {e}"),
    })
    .await;
}

/// `<user_dir>/cache/listenbrainz_v2.db` — the SAME per-user file Tauri's
/// `ListenBrainzV2State::init_cache_at` opens, so credentials and the offline
/// listen queue are shared across frontends.
fn listenbrainz_cache_path() -> Option<PathBuf> {
    let dir = scrobbler_settings::user_dir()?.join("cache");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("listenbrainz_v2.db"))
}

// ============================================================================
// Offline flush — drain both queues (shell entry + every offline->online edge).
// ============================================================================

async fn flush_offline_queues() {
    flush_lastfm_queue().await;
    flush_listenbrainz_queue().await;
}

/// Flush the Last.fm queue: up to 50 per pass (the Last.fm batch limit),
/// oldest first; entries older than 14 days are dropped (marked sent) since
/// Last.fm rejects them — both mirror the Svelte `flushScrobbleQueue`. Stops
/// at the first network failure (still offline / flaky) and retries on the
/// next edge. Cleans up sent rows older than 7 days afterwards.
async fn flush_lastfm_queue() {
    let cfg = scrobbler_settings::get();
    if !cfg.lastfm_is_authed() {
        return;
    }
    let Some(dir) = scrobbler_settings::user_dir() else {
        return;
    };
    let pending = match tokio::task::spawn_blocking({
        let dir = dir.clone();
        move || OfflineModeStore::new_at(&dir).and_then(|s| s.get_queued_scrobbles(50))
    })
    .await
    {
        Ok(Ok(p)) => p,
        _ => return,
    };
    if pending.is_empty() {
        return;
    }

    let client = LastFmClient::with_session_key(cfg.lastfm_session_key.clone());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let cutoff = now - 14 * 86400;
    let mut sent_ids: Vec<i64> = Vec::new();
    for item in pending {
        if item.timestamp < cutoff {
            // Too old for Last.fm — drop it (mark sent so it stops re-trying).
            sent_ids.push(item.id);
            continue;
        }
        match client
            .scrobble(
                &item.artist,
                &item.track,
                item.album.as_deref(),
                item.timestamp as u64,
            )
            .await
        {
            Ok(()) => sent_ids.push(item.id),
            Err(e) => {
                log::warn!(
                    "[qbz-slint] Last.fm flush stopped at {} - {}: {e}",
                    item.artist,
                    item.track
                );
                break; // still offline / failing — retry on the next edge
            }
        }
    }
    if !sent_ids.is_empty() {
        let count = sent_ids.len();
        let _ = tokio::task::spawn_blocking(move || {
            OfflineModeStore::new_at(&dir).and_then(|s| {
                s.mark_scrobbles_sent(&sent_ids)?;
                s.cleanup_sent_scrobbles(7)
            })
        })
        .await;
        log::info!("[qbz-slint] Last.fm flush: {count} scrobble(s) sent/cleared");
    }
}

/// Flush the ListenBrainz queue from the shared cache. Stops at the first
/// failure and retries on the next edge.
async fn flush_listenbrainz_queue() {
    let cfg = scrobbler_settings::get();
    if !cfg.listenbrainz_is_authed() {
        return;
    }
    let Some(path) = listenbrainz_cache_path() else {
        return;
    };
    let pending = match tokio::task::spawn_blocking({
        let path = path.clone();
        move || ListenBrainzCache::new(&path).and_then(|c| c.get_pending_listens(500))
    })
    .await
    {
        Ok(Ok(p)) => p,
        _ => return,
    };
    if pending.is_empty() {
        return;
    }

    let client = ListenBrainzClient::new();
    client
        .restore_token(cfg.listenbrainz_token.clone(), cfg.listenbrainz_username.clone())
        .await;
    let mut sent_ids: Vec<i64> = Vec::new();
    for item in pending {
        let info = AdditionalInfo {
            recording_mbid: item.recording_mbid.clone(),
            release_mbid: item.release_mbid.clone(),
            artist_mbids: item.artist_mbids.clone(),
            isrc: item.isrc.clone(),
            duration_ms: item.duration_ms,
            ..Default::default()
        };
        if client
            .submit_listen(
                &item.artist_name,
                &item.track_name,
                item.release_name.as_deref(),
                item.listened_at,
                Some(info),
            )
            .await
            .is_ok()
        {
            sent_ids.push(item.id);
        } else {
            break; // still failing — retry on the next edge
        }
    }
    if !sent_ids.is_empty() {
        let count = sent_ids.len();
        let _ = tokio::task::spawn_blocking(move || {
            ListenBrainzCache::new(&path).and_then(|c| c.mark_listens_sent(&sent_ids))
        })
        .await;
        log::info!("[qbz-slint] ListenBrainz flush: {count} listen(s) sent");
    }
}
