//! Slint-side glue for the shared lyrics engine (`crates/qbz-lyrics`).
//!
//! The engine (Qobuz-first chain, providers, per-user SQLite cache) is
//! frontend-agnostic (ADR-006); this module owns only the process global and
//! the bindings, following the `offline_mode.rs` / `fav_cache.rs` template:
//!
//! - [`init_for_user`] binds the per-user cache on every session activation
//!   (login, restore, offline entry — next to `offline_mode::init_for_user`);
//!   [`teardown`] drops it on logout. The cache file is the SAME
//!   `<user cache dir>/lyrics/lyrics.db` Tauri uses.
//! - [`on_track_changed`] is the fetch rider on the `NOTIFY_LAST_TRACK`
//!   de-duped track-change edge in `playback::refresh_now_playing_meta`
//!   (the scrobble/notification seam). Tauri prefetches on EVERY track change
//!   regardless of panel visibility (`lyricsStore.ts:545-565` — "Always
//!   prefetch lyrics when a new track starts"); same here. Deliberately NO
//!   skip-if-remote: lyrics follow the QConnect peer's track (Q7).
//! - [`on_track_cleared`] resets `LyricsState` when the queue empties
//!   (track -> null resets the store, `lyricsStore.ts:560-562`).
//! - The sidebar's `init` fires `LyricsState.panel-opened()` (conditional
//!   mount, ADR-010) -> main.rs re-runs the same fetch path for the current
//!   track; the duplicate-fetch guard makes that a no-op while still loaded
//!   (Tauri `lastFetchedTrackId` guard, `lyricsStore.ts:352-354`).
//!
//! Stale-response guard (F2): every spawned fetch captures its request
//! identity; the service echoes it back (`request_track_id`/`request_key`)
//! and the response is DROPPED unless it still matches the latest requested
//! track — a late response can never overwrite the current track's lyrics
//! (the documented Tauri race, review §1.6).
//!
//! The parsed [`LyricsDoc`] (native Qobuz word stamps included) is held
//! Rust-side in [`CURRENT_DOC`]; the UI model only carries the line list +
//! a `has-words` flag. The S4 sync engine consumes the doc for the
//! word-anchored karaoke fill.
//!
//! Translation (Qobuz API v10): the default flow fetches original-only and
//! pushes `translation-available` (doc `translation_langs` non-empty), which
//! gates the sidebar's floating toggle. Toggling ON refetches the current
//! track WITH the resolved target (`LyricsRequest.language`) via
//! [`enable_translation`] and flips `show-translation` only when a
//! translation actually arrived — any gap toasts and reverts (fail soft,
//! never an error state). Toggling OFF ([`disable_translation`]) is pure
//! UI. The toggle is PER-TRACK (owner decision 2026-07-23): it applies
//! only to the song it was enabled on — every track change resets it and
//! fetches original-only.

use std::sync::{Arc, Mutex, OnceLock};

use slint::{ComponentHandle, ModelRc, VecModel};

use qbz_lyrics::{
    build_cache_key, LyricsData, LyricsDoc, LyricsOutcome, LyricsProvider, LyricsProviders,
    LyricsRequest, LyricsResponse, LyricsService, LyricsSourceKind,
};
use qbz_models::QueueTrack;
use qbz_qobuz::{QobuzClient, QobuzLyricsDocument};

use crate::{AppWindow, LyricsLineItem, LyricsState};

// `LyricsState.status` values (keep in sync with `ui/state.slint`).
// `READY` is pub(crate): the sync engine (`lyrics_sync`) gates on it.
const STATUS_IDLE: i32 = 0;
const STATUS_LOADING: i32 = 1;
pub(crate) const STATUS_READY: i32 = 2;
const STATUS_NOT_FOUND: i32 = 3;
const STATUS_ERROR: i32 = 4;
const STATUS_OFFLINE: i32 = 5;

/// Process-global lyrics service. Installed on the first session activation;
/// the per-user cache handle re-binds via `init_at` on every activation.
static SERVICE: OnceLock<Arc<LyricsService>> = OnceLock::new();

/// The shared core client lock, kept for the translation "Auto" resolution
/// (the account `language_code` lives on the client's session). Installed
/// alongside [`SERVICE`] on the first session activation.
static CLIENT: OnceLock<Arc<tokio::sync::RwLock<Option<QobuzClient>>>> = OnceLock::new();

/// Identity of the latest requested track + whether a Found result has been
/// committed for it. `key` doubles as the stale guard (F2) and the
/// duplicate-fetch guard (`loaded` mirrors Tauri's `status === 'loaded'`).
struct CurrentLyrics {
    key: String,
    loaded: bool,
}

static CURRENT: Mutex<CurrentLyrics> = Mutex::new(CurrentLyrics {
    key: String::new(),
    loaded: false,
});

/// Active translation session (Qobuz v10), the Rust-side source of truth
/// behind `LyricsState.show-translation`: `enabled` = the toggle is ON,
/// `language` = the target it was resolved with (kept so track changes
/// re-request the same target — spec §B: translation follows across tracks).
struct TranslationSession {
    enabled: bool,
    language: Option<String>,
}

static TRANSLATION: Mutex<TranslationSession> = Mutex::new(TranslationSession {
    enabled: false,
    language: None,
});

/// The parsed document for the current track, held Rust-side so the native
/// Qobuz word timestamps survive for the S4 sync engine (the UI model only
/// carries text + line bounds + a has-words flag).
static CURRENT_DOC: Mutex<Option<LyricsDoc>> = Mutex::new(None);

/// Read access to the current parsed document for the sync engine: the
/// closure runs under the lock (keep it short — it executes on the UI
/// thread at the engine's tick rate). A poisoned lock degrades to `None`.
pub(crate) fn with_current_doc<R>(f: impl FnOnce(Option<&LyricsDoc>) -> R) -> R {
    match CURRENT_DOC.lock() {
        Ok(guard) => f(guard.as_ref()),
        Err(_) => f(None),
    }
}

/// Production providers over the shared core client lock. The lock is read
/// at CALL time (never cached), so re-inits of the Qobuz client are always
/// picked up; a missing client (pre-login, offline boot) errors out, which
/// the chain treats as silent degradation to the external fallbacks.
struct SharedClientProviders {
    client: Arc<tokio::sync::RwLock<Option<QobuzClient>>>,
}

#[async_trait::async_trait]
impl LyricsProviders for SharedClientProviders {
    async fn qobuz(
        &self,
        track_id: u64,
        language: Option<&str>,
    ) -> Result<Option<QobuzLyricsDocument>, String> {
        let guard = self.client.read().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| "Qobuz client not initialized".to_string())?;
        client
            .get_lyrics(track_id, language)
            .await
            .map_err(|e| e.to_string())
    }

    async fn lrclib(
        &self,
        title: &str,
        artist: &str,
        duration_secs: Option<u64>,
    ) -> Result<Option<LyricsData>, String> {
        qbz_lyrics::providers::fetch_lrclib(title, artist, duration_secs).await
    }

    async fn ovh(&self, title: &str, artist: &str) -> Option<LyricsData> {
        qbz_lyrics::providers::fetch_lyrics_ovh(title, artist).await
    }
}

/// Bind the per-user lyrics cache — the SAME `lyrics/lyrics.db` under the
/// per-user CACHE dir that Tauri's `session_lifecycle.rs:229` uses, so both
/// frontends share one cache. Called on every session activation; the first
/// call installs the process-global service over the core's client lock
/// (one lock per process, so later calls reuse the installed providers).
/// Best-effort: failures are logged, never block entry. Must run within the
/// tokio runtime context (the bind is spawned).
pub fn init_for_user(client: Arc<tokio::sync::RwLock<Option<QobuzClient>>>, user_id: u64) {
    // Keep a handle for the translation "Auto" resolution (the account
    // language lives on the client's session). Same lock per process, so a
    // later activation's re-set is a no-op.
    let _ = CLIENT.set(client.clone());
    let service = SERVICE
        .get_or_init(move || {
            Arc::new(LyricsService::new(Arc::new(SharedClientProviders {
                client,
            })))
        })
        .clone();
    let Some(cache_dir) = dirs::cache_dir().map(|d| {
        d.join("qbz")
            .join("users")
            .join(user_id.to_string())
    }) else {
        log::error!("[qbz-slint] lyrics cache init: cache directory unavailable");
        return;
    };
    tokio::spawn(async move {
        match service.init_at(&cache_dir).await {
            Ok(()) => log::info!("[qbz-slint] lyrics cache bound for user {user_id}"),
            Err(e) => log::error!("[qbz-slint] lyrics cache init failed: {e}"),
        }
    });
}

/// Drop the per-user cache handle + the in-memory track state on logout.
/// Clearing `CURRENT` also invalidates any in-flight fetch (its stale guard
/// no longer matches), so a late response from the previous session never
/// lands in the next one.
pub fn teardown() {
    if let Ok(mut current) = CURRENT.lock() {
        current.key.clear();
        current.loaded = false;
    }
    if let Ok(mut doc) = CURRENT_DOC.lock() {
        *doc = None;
    }
    reset_translation_session();
    if let Some(service) = SERVICE.get().cloned() {
        tokio::spawn(async move {
            service.teardown().await;
        });
    }
}

/// Join the current doc's lines with `\n` and copy them to the clipboard —
/// the flyout's "Copy lyrics" action (Tauri `copyLyrics`,
/// `LyricsControlsPopover.svelte:39-55`). The button is gated UI-side on
/// ready + lines>0; an empty doc is a silent no-op. The clipboard write is
/// fire-and-forget on a blocking thread (arboard), so the toast reports the
/// dispatch like Tauri reports its `writeText` success.
pub fn copy_current_lyrics(weak: &slint::Weak<AppWindow>) {
    let text = with_current_doc(|doc| {
        doc.map(|d| {
            d.lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
    })
    .unwrap_or_default();
    if text.is_empty() {
        return;
    }
    crate::share::copy_to_clipboard(text);
    crate::toast::success_weak(weak, qbz_i18n::t("Lyrics copied"));
}

/// Refresh the Settings cache row from the REAL per-user lyrics.db (F1
/// fix-forward: entries AND size from the bound per-user path, via
/// `qbz_lyrics` cache_stats). Pushes `cache-loaded=false` when the service
/// or cache is unavailable (the row then shows no stats line).
pub fn refresh_cache_stats(handle: &tokio::runtime::Handle, weak: slint::Weak<AppWindow>) {
    let Some(service) = SERVICE.get().cloned() else {
        return;
    };
    handle.spawn(async move {
        let stats = service.cache_stats().await;
        let _ = weak.upgrade_in_event_loop(move |w| {
            let state = w.global::<LyricsState>();
            match stats {
                Ok(stats) => {
                    state.set_cache_entries_text(stats.entries.to_string().into());
                    state.set_cache_size(
                        crate::offline_manager::human_size(stats.size_bytes).into(),
                    );
                    state.set_cache_loaded(true);
                }
                Err(e) => {
                    log::warn!("[qbz-slint] lyrics cache stats failed: {e}");
                    state.set_cache_loaded(false);
                }
            }
        });
    });
}

/// Clear the per-user lyrics cache (Settings row action), then re-push the
/// stats. Mirrors Tauri's `v2_lyrics_clear_cache` flow with a result toast.
pub fn clear_cache(handle: &tokio::runtime::Handle, weak: slint::Weak<AppWindow>) {
    let Some(service) = SERVICE.get().cloned() else {
        return;
    };
    let handle_for_refresh = handle.clone();
    handle.spawn(async move {
        match service.clear_cache().await {
            Ok(()) => {
                crate::toast::success_weak(&weak, qbz_i18n::t("Lyrics cache cleared"));
                refresh_cache_stats(&handle_for_refresh, weak);
            }
            Err(e) => {
                log::error!("[qbz-slint] lyrics cache clear failed: {e}");
                crate::toast::error_weak(&weak, qbz_i18n::t("Failed to clear lyrics cache"));
            }
        }
    });
}

// ---- Translation (Qobuz API v10) -------------------------------------------

/// Whether the translation toggle is currently ON (Rust-side source of truth
/// behind `LyricsState.show-translation`). main.rs reads it to route the
/// toggle callback. Per-track (owner 2026-07-23): reset on every track
/// change in [`on_track_changed`].
pub fn translation_enabled() -> bool {
    TRANSLATION.lock().map(|t| t.enabled).unwrap_or(false)
}

fn reset_translation_session() {
    if let Ok(mut t) = TRANSLATION.lock() {
        t.enabled = false;
        t.language = None;
    }
}

/// Account language (ISO 639-1) from the shared client's session — the
/// "Auto" resolution's first hop. None pre-login / pre-v10 sessions.
async fn account_language() -> Option<String> {
    let client = CLIENT.get()?.clone();
    let guard = client.read().await;
    guard.as_ref()?.session_language_code().await
}

/// Resolve the effective translation target (spec §B.3): the explicit
/// setting when set; "Auto" = account `language_code`, falling back to the
/// UI locale. `None` when nothing resolves — the caller fails soft (toast +
/// revert); the lyrics view never enters an error state over translation.
async fn resolve_target_language() -> Option<String> {
    let pref = crate::lyrics_prefs::load().translation_language;
    if pref != "auto" {
        return Some(pref);
    }
    if let Some(lang) = account_language().await {
        return Some(lang);
    }
    let ui_locale = qbz_i18n::current_language();
    (!ui_locale.is_empty()).then(|| ui_locale.to_string())
}

/// Toggle OFF: drop the session and re-render original-only. Pure UI — the
/// committed line model keeps its (now hidden) translation texts; NO
/// network (spec §B.3).
pub fn disable_translation(weak: &slint::Weak<AppWindow>) {
    reset_translation_session();
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<LyricsState>().set_show_translation(false);
    });
}

/// Toast + revert, for every translation failure path (toggle ON with no
/// current track, no resolvable target, fetch error, or a response without
/// a translation). Fail soft: the current lyrics view is left untouched.
pub fn translation_unavailable(weak: &slint::Weak<AppWindow>) {
    reset_translation_session();
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<LyricsState>().set_show_translation(false);
    });
    crate::toast::error_weak(
        weak,
        qbz_i18n::t("Translation is not available right now. Please try again later."),
    );
}

/// Toggle ON: refetch the current track's lyrics WITH the resolved target
/// language (spec §B.3). The committed doc then carries the embedded
/// translation and `show-translation` flips on — but ONLY when the response
/// actually holds one; any gap toasts and reverts (Android
/// `player_lyrics_translation_error` parity). When the current doc already
/// holds the translation for the target (toggle off -> on on the same
/// track), the flip is local — no refetch.
pub fn enable_translation(weak: slint::Weak<AppWindow>, track: &QueueTrack) {
    let Some(service) = SERVICE.get().cloned() else {
        translation_unavailable(&weak);
        return;
    };
    let source = source_kind(track);
    let track_id = (source == LyricsSourceKind::Qobuz).then_some(track.id);
    let duration_secs = (track.duration_secs > 0).then_some(track.duration_secs);
    let key = request_identity(
        track_id,
        &build_cache_key(track.title.trim(), track.artist.trim(), duration_secs),
    );
    let request = LyricsRequest {
        track_id,
        source,
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: (!track.album.is_empty()).then(|| track.album.clone()),
        duration_secs,
        offline: crate::offline_mode::engine().is_offline(),
        // Resolved inside the task (the session read is async).
        language: None,
    };
    tokio::spawn(async move {
        let Some(target) = resolve_target_language().await else {
            translation_unavailable(&weak);
            return;
        };
        // Fast path: the current doc already carries this translation.
        let already = with_current_doc(|doc| {
            doc.and_then(|d| d.translation.as_ref())
                .and_then(|t| t.lang.as_deref())
                == Some(target.as_str())
        });
        if already {
            if let Ok(mut t) = TRANSLATION.lock() {
                t.enabled = true;
                t.language = Some(target);
            }
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<LyricsState>().set_show_translation(true);
            });
            return;
        }
        let mut request = request;
        request.language = Some(target.clone());
        let result = service.get(request).await;
        // Same stale guard (F2) as the track-change path: a superseded
        // response is dropped whole.
        let response_key = match &result {
            Ok(response) => {
                request_identity(response.request_track_id, &response.request_key)
            }
            Err(_) => key.clone(),
        };
        {
            let mut current = CURRENT.lock().expect("lyrics CURRENT lock poisoned");
            if current.key != response_key {
                return;
            }
            if matches!(
                result.as_ref().map(|r| &r.outcome),
                Ok(LyricsOutcome::Found(_))
            ) {
                current.loaded = true;
            }
        }
        match result {
            Ok(response) if matches!(response.outcome, LyricsOutcome::Found(_)) => {
                let has_translation = match &response.outcome {
                    LyricsOutcome::Found(found) => found.doc.translation.is_some(),
                    _ => false,
                };
                if has_translation {
                    if let Ok(mut t) = TRANSLATION.lock() {
                        t.enabled = true;
                        t.language = Some(target);
                    }
                }
                apply_result(weak.clone(), Ok(response));
                let _ = weak.upgrade_in_event_loop(move |w| {
                    w.global::<LyricsState>().set_show_translation(has_translation);
                });
                if !has_translation {
                    // The track lacks the target language — the original
                    // committed above stays; toast + revert (spec §B.4).
                    translation_unavailable(&weak);
                }
            }
            // NotFound / offline-miss: keep the current view untouched.
            Ok(_) => translation_unavailable(&weak),
            // Fetch error: NEVER an error state because of translation.
            Err(e) => {
                log::warn!("[qbz-slint] translation refetch failed: {e}");
                translation_unavailable(&weak);
            }
        }
    });
}

/// Qobuz vs non-Qobuz from the queue track's source tag. `qobuz_download`
/// rows carry the REAL Qobuz catalog id (`local_queue_track`), so the Qobuz
/// primary applies to them too; local user files / ephemeral folders / Plex
/// have synthetic ids and go straight to the metadata-keyed fallback chain.
fn source_kind(track: &QueueTrack) -> LyricsSourceKind {
    match track.source.as_deref() {
        Some("local") | Some("ephemeral") | Some("plex") => LyricsSourceKind::NonQobuz,
        // None | "qobuz" | "qobuz_download"
        _ => LyricsSourceKind::Qobuz,
    }
}

/// One identity string for a request/response pair: the track id when the
/// Qobuz primary applies, else the metadata cache key. Matches the echo the
/// service returns (`request_track_id` / `request_key`).
fn request_identity(track_id: Option<u64>, cache_key: &str) -> String {
    match track_id {
        Some(id) => format!("id:{id}"),
        None => format!("key:{cache_key}"),
    }
}

fn provider_label(provider: LyricsProvider) -> &'static str {
    match provider {
        LyricsProvider::Lrclib => "LRCLIB",
        LyricsProvider::Ovh => "lyrics.ovh",
        // First-party — no attribution needed (spec §3.5).
        LyricsProvider::Qobuz => "",
    }
}

/// The fetch rider — called inside the `NOTIFY_LAST_TRACK` guard of
/// `refresh_now_playing_meta` on every real track-change edge, and from the
/// panel-open path for the current track. Fire-and-forget: pushes the
/// loading state immediately, resolves through the engine off-loop, and
/// commits the response only if the track is still current (F2).
/// Resolve + CACHE lyrics for a track in the BACKGROUND without touching the
/// UI — warms the lyrics cache for an upcoming queued track so the panel is
/// instant when that track becomes current (the user's "prefetch the next
/// track's lyrics" request; Tauri only fetches the current one). Fire-and-
/// forget; skipped offline so we never spend network warming ahead.
pub fn prefetch_lyrics(track: &QueueTrack) {
    let Some(service) = SERVICE.get().cloned() else {
        return;
    };
    if crate::offline_mode::engine().is_offline() {
        return;
    }
    let source = source_kind(track);
    let request = LyricsRequest {
        track_id: (source == LyricsSourceKind::Qobuz).then_some(track.id),
        source,
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: (!track.album.is_empty()).then(|| track.album.clone()),
        duration_secs: (track.duration_secs > 0).then_some(track.duration_secs),
        offline: false,
        // Prefetch warms the original-only entry; the translation toggle
        // (UI task) triggers the refetch-with-language when needed.
        language: None,
    };
    tokio::spawn(async move {
        // Resolution upserts into lyrics.db; the result is discarded (no UI).
        // On advance, on_track_changed hits this warm cache instantly.
        let _ = service.get(request).await;
    });
}

pub fn on_track_changed(weak: slint::Weak<AppWindow>, track: &QueueTrack) {
    let Some(service) = SERVICE.get().cloned() else {
        log::debug!("[qbz-slint] lyrics fetch skipped: service not installed");
        return;
    };

    // Per-track translation toggle (owner 2026-07-23): a new track always
    // starts original-only — any translation the user enabled was for the
    // PREVIOUS song only.
    reset_translation_session();

    let source = source_kind(track);
    // F4 contract, explicit: the RAW title goes to the engine (fallback
    // providers match on the unversioned title; Qobuz looks up by id). The
    // header meta shows the version-enriched display title like the bar.
    let display_title = match track.version.as_deref().filter(|v| !v.is_empty()) {
        Some(version) => format!("{} ({version})", track.title),
        None => track.title.clone(),
    };
    let artist = track.artist.clone();
    let request = LyricsRequest {
        track_id: (source == LyricsSourceKind::Qobuz).then_some(track.id),
        source,
        title: track.title.clone(),
        artist: artist.clone(),
        album: (!track.album.is_empty()).then(|| track.album.clone()),
        duration_secs: (track.duration_secs > 0).then_some(track.duration_secs),
        // Offline as data, not lookup (spec §2.2.4): the engine verdict is
        // read here and travels with the request.
        offline: crate::offline_mode::engine().is_offline(),
        // Default flow always fetches original-only (spec §B.1): the
        // translation toggle is per-track (owner 2026-07-23) and was reset
        // above; enabling it refetches WITH a language via
        // [`enable_translation`].
        language: None,
    };
    let key = request_identity(
        request.track_id,
        &build_cache_key(
            request.title.trim(),
            request.artist.trim(),
            request.duration_secs,
        ),
    );

    // Duplicate-fetch guard (Tauri parity, lyricsStore.ts:352-354): skip only
    // when the SAME track is already loaded; not-found/error states re-fetch
    // on the next trigger (e.g. panel re-open).
    {
        let mut current = CURRENT.lock().expect("lyrics CURRENT lock poisoned");
        if current.key == key && current.loaded {
            return;
        }
        current.key = key.clone();
        current.loaded = false;
    }

    // Loading state + header meta, immediately (the spinner shows even for a
    // fast cache hit — Tauri does the same).
    {
        let display_title = display_title.clone();
        let artist = artist.clone();
        let _ = weak.clone().upgrade_in_event_loop(move |w| {
            let state = w.global::<LyricsState>();
            state.set_status(STATUS_LOADING);
            state.set_track_title(display_title.into());
            state.set_track_artist(artist.into());
            state.set_lines(ModelRc::new(VecModel::default()));
            state.set_synced(false);
            state.set_active_index(-1);
            state.set_line_progress(0.0);
            state.set_fill_anim_ms(0);
            state.set_provider("".into());
            state.set_provider_label("".into());
            state.set_error("".into());
            // Hide the toggle while loading — the availability of the NEXT
            // track's doc is unknown until its commit. The toggle itself is
            // per-track: it always starts OFF for a new track.
            state.set_translation_available(false);
            state.set_show_translation(false);
        });
    }

    tokio::spawn(async move {
        let result = service.get(request).await;
        // Stale guard (F2): match the response echo against the LATEST
        // requested identity; a superseded response is dropped whole.
        let response_key = match &result {
            Ok(response) => {
                request_identity(response.request_track_id, &response.request_key)
            }
            Err(_) => key.clone(),
        };
        {
            let mut current = CURRENT.lock().expect("lyrics CURRENT lock poisoned");
            if current.key != response_key {
                return;
            }
            if matches!(
                result.as_ref().map(|r| &r.outcome),
                Ok(LyricsOutcome::Found(_))
            ) {
                current.loaded = true;
            }
        }
        apply_result(weak, result);
    });
}

/// Reset everything when the queue empties (track -> null), mirroring the
/// Tauri store reset (`lyricsStore.ts:560-562`).
pub fn on_track_cleared(weak: slint::Weak<AppWindow>) {
    if let Ok(mut current) = CURRENT.lock() {
        current.key.clear();
        current.loaded = false;
    }
    if let Ok(mut doc) = CURRENT_DOC.lock() {
        *doc = None;
    }
    reset_translation_session();
    let _ = weak.upgrade_in_event_loop(|w| {
        let state = w.global::<LyricsState>();
        state.set_status(STATUS_IDLE);
        state.set_lines(ModelRc::new(VecModel::default()));
        state.set_synced(false);
        state.set_active_index(-1);
        state.set_line_progress(0.0);
        state.set_fill_anim_ms(0);
        state.set_track_title("".into());
        state.set_track_artist("".into());
        state.set_provider("".into());
        state.set_provider_label("".into());
        state.set_error("".into());
        state.set_translation_available(false);
        state.set_show_translation(false);
    });
}

/// Map the engine response into `LyricsState` (UI thread push).
fn apply_result(weak: slint::Weak<AppWindow>, result: Result<LyricsResponse, String>) {
    let (status, items, synced, provider, label, error, translation_available) = match result {
        Ok(response) => match response.outcome {
            LyricsOutcome::Found(found) => {
                let translation = found.doc.translation.as_deref();
                let items: Vec<LyricsLineItem> = found
                    .doc
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(i, line)| LyricsLineItem {
                        text: line.text.clone().into(),
                        time_ms: line.time_ms.map(|v| v as i32).unwrap_or(-1),
                        end_ms: line.end_ms.map(|v| v as i32).unwrap_or(-1),
                        has_words: line.words.is_some(),
                        // 1:1 with the rendered lines — the core filters the
                        // translation lines the same way as the originals;
                        // a missing entry degrades to "" (fail soft).
                        translation_text: translation
                            .and_then(|t| t.lines.get(i))
                            .map(|l| l.text.clone())
                            .unwrap_or_default()
                            .into(),
                    })
                    .collect();
                let synced = found.doc.synced;
                let provider = found.doc.provider;
                // The toggle lights only when Qobuz advertises translations
                // for this track (spec §B.2).
                let translation_available = !found.doc.translation_langs.is_empty();
                if let Ok(mut doc) = CURRENT_DOC.lock() {
                    *doc = Some(found.doc);
                }
                (
                    STATUS_READY,
                    items,
                    synced,
                    provider.as_str(),
                    provider_label(provider),
                    String::new(),
                    translation_available,
                )
            }
            LyricsOutcome::NotFound => {
                if let Ok(mut doc) = CURRENT_DOC.lock() {
                    *doc = None;
                }
                (STATUS_NOT_FOUND, Vec::new(), false, "", "", String::new(), false)
            }
            // Typed offline miss (F3) — the view maps it to a translated
            // string, never a hardcoded message.
            LyricsOutcome::NotAvailableOffline => {
                if let Ok(mut doc) = CURRENT_DOC.lock() {
                    *doc = None;
                }
                (STATUS_OFFLINE, Vec::new(), false, "", "", String::new(), false)
            }
        },
        Err(e) => {
            if let Ok(mut doc) = CURRENT_DOC.lock() {
                *doc = None;
            }
            log::warn!("[qbz-slint] lyrics fetch failed: {e}");
            (STATUS_ERROR, Vec::new(), false, "", "", e, false)
        }
    };
    let _ = weak.upgrade_in_event_loop(move |w| {
        let state = w.global::<LyricsState>();
        state.set_lines(ModelRc::new(VecModel::from(items)));
        state.set_synced(synced);
        state.set_provider(provider.into());
        state.set_provider_label(label.into());
        state.set_error(error.into());
        state.set_translation_available(translation_available);
        state.set_active_index(-1);
        state.set_line_progress(0.0);
        state.set_fill_anim_ms(0);
        state.set_status(status);
        // One immediate engine pass so a freshly committed doc lands on the
        // correct line right away — even while PAUSED (Tauri computes once on
        // load, lyricsStore.ts:386-389); continuous ticking stays gated on
        // playing.
        crate::lyrics_sync::kick();
    });
}
