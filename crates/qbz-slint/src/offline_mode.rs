//! Slint-side glue for the shared offline-MODE engine.
//!
//! Offline MODE = the app operating without Qobuz — NOT the offline CACHE
//! (downloads; that glue lives in `offline.rs` / `offline_cache.rs`). The
//! engine, connectivity actor and persisted settings are frontend-agnostic
//! (`qbz_app::offline_mode`, ADR-006); this module only owns the process
//! globals and the per-user binding, following the `tray_settings.rs`
//! template.
//!
//! It also owns the per-user `SubscriptionStateStore` binding (D4): the
//! login flows record valid/ineligible verdicts here, and the grace check
//! consults it. The purge-at-activation consumer mirrors Tauri's
//! `session_lifecycle.rs` (the Slint build never opened the store before).

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use slint::ComponentHandle;

use qbz_app::offline_mode::{Connectivity, ConnectivityActor, OfflineMode, OfflineModeEngine, OfflineStatus};
use qbz_app::settings::subscription::SubscriptionStateStore;
use qbz_app::user_data::UserDataPaths;

use crate::{AppWindow, OfflineState, SettingsState};

/// Process-global engine. Exists from first use; per-user state binds via
/// [`init_for_user`], connectivity via [`start`].
static ENGINE: LazyLock<Arc<OfflineModeEngine>> =
    LazyLock::new(|| Arc::new(OfflineModeEngine::new()));

/// The connectivity actor, spawned once per process by [`start`].
static CONNECTIVITY: OnceLock<ConnectivityActor> = OnceLock::new();

/// Per-user subscription state (D4). `None` until a session (online or
/// offline) is activated; consumers fail open in that window.
static SUBSCRIPTION: Mutex<Option<SubscriptionStateStore>> = Mutex::new(None);

pub fn engine() -> Arc<OfflineModeEngine> {
    Arc::clone(&ENGINE)
}

/// Spawn the connectivity actor and attach it to the engine. Called once
/// from `main` after the tokio runtime is up (both spawns need the runtime
/// context); the monitoring runs for the whole app lifetime, login screen
/// included (the restore flow and the D2 recovery banner read it).
pub fn start() {
    if CONNECTIVITY.get().is_some() {
        return;
    }
    let actor = ConnectivityActor::spawn();
    engine().attach_connectivity(&actor);
    if CONNECTIVITY.set(actor).is_err() {
        log::warn!("[qbz-slint] offline mode: connectivity actor already started");
    } else {
        log::info!("[qbz-slint] offline mode: connectivity monitoring started");
    }
}

/// Force an immediate connectivity re-probe (Settings "Check now").
pub fn request_recheck() {
    if let Some(actor) = CONNECTIVITY.get() {
        actor.request_recheck();
    }
}

/// Settings > Offline "Check now": flag the in-flight state (the status
/// row's button flips to "Checking..."), then force an actor re-probe.
/// The flag clears on the next engine broadcast ([`apply_status`]) — or
/// after a short timeout when the verdict comes back unchanged, since the
/// actor only broadcasts state flips.
pub fn check_now(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let set_weak = weak.clone();
    let _ = set_weak.upgrade_in_event_loop(|w| {
        w.global::<SettingsState>().set_offline_checking(true);
    });
    request_recheck();
    handle.spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        let _ = weak.upgrade_in_event_loop(|w| {
            w.global::<SettingsState>().set_offline_checking(false);
        });
    });
}

/// Seed the Settings > Offline MODE toggle states from the persisted
/// engine store. Fired by the panel's `init` (`OfflineModeActions.load`),
/// so every mount of Settings > Offline re-reads them — the same lazy-load
/// hook LocalLibrarySettings uses. Best-effort: pre-session reads (no
/// store bound) keep the defaults.
pub fn seed_settings(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        let settings = match tokio::task::spawn_blocking(|| engine().settings()).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                log::warn!("[qbz-slint] offline mode settings read failed: {e}");
                return;
            }
            Err(e) => {
                log::error!("[qbz-slint] offline mode settings seed task failed: {e}");
                return;
            }
        };
        let _ = weak.upgrade_in_event_loop(move |w| {
            let st = w.global::<SettingsState>();
            st.set_offline_mode_enabled(settings.manual_offline_mode);
            st.set_offline_show_network_folders(settings.show_network_folders_in_manual_offline);
        });
    });
}

/// Mirror every engine status change into the `OfflineState` Slint global
/// (login affordances + the D2 recovery banner read it). Also seeds
/// `has-previous-session` once; `enter_shell` refreshes it after a
/// successful login. Spawned once from `main` right after [`start`]
/// (needs the tokio runtime context and a created window).
pub fn start_ui_forwarder(weak: slint::Weak<AppWindow>) {
    let has_previous = UserDataPaths::load_last_user_id().is_some();
    let seed_weak = weak.clone();
    let _ = seed_weak.upgrade_in_event_loop(move |w| {
        w.global::<OfflineState>()
            .set_has_previous_session(has_previous);
    });

    tokio::spawn(async move {
        let mut rx = engine().subscribe();
        loop {
            let status = *rx.borrow_and_update();
            let _ = weak.upgrade_in_event_loop(move |w| apply_status(&w, status));
            if rx.changed().await.is_err() {
                break;
            }
        }
    });
}

/// Push one engine status snapshot into the Slint global (UI thread).
fn apply_status(w: &AppWindow, status: OfflineStatus) {
    let state = w.global::<OfflineState>();
    state.set_offline(status.is_offline());
    state.set_mode(match status.mode {
        OfflineMode::Online => 0,
        OfflineMode::RealOffline => 1,
        OfflineMode::InducedOffline => 2,
    });
    state.set_connectivity(match status.connectivity {
        Connectivity::Unknown => 0,
        Connectivity::Up => 1,
        Connectivity::Down => 2,
    });
    state.set_captive_portal(status.captive_portal);
    state.set_show_recovery_banner(status.show_recovery_banner());
    // A status broadcast resolves any in-flight Settings "Check now".
    w.global::<SettingsState>().set_offline_checking(false);
}

/// `<data_dir>/qbz/users/<user_id>/` — the per-user directory both the
/// engine store and the subscription store live in. Matches the Tauri
/// per-user path (and `tray_settings::user_dir`).
pub fn user_data_dir(user_id: u64) -> Option<PathBuf> {
    Some(
        dirs::data_dir()?
            .join("qbz")
            .join("users")
            .join(user_id.to_string()),
    )
}

/// Bind the engine + subscription store to the active user's data dir.
/// Called on every session activation (login, restore, offline entry),
/// AFTER `crate::offline::activate` so the purge consumer can reach the
/// offline cache. Best-effort: failures are logged, never block entry.
///
/// Must run within the tokio runtime context (the purge check spawns).
pub fn init_for_user(base_dir: &Path) {
    if let Err(e) = engine().init_for_user(base_dir) {
        log::error!("[qbz-slint] offline mode engine init failed: {e}");
    }
    match SubscriptionStateStore::new_at(base_dir) {
        Ok(store) => {
            if let Ok(mut guard) = SUBSCRIPTION.lock() {
                *guard = Some(store);
            }
        }
        // Fail-open: no store means no recorded invalidity, playback allowed.
        Err(e) => log::error!("[qbz-slint] subscription state store open failed: {e}"),
    }
    spawn_subscription_purge_check();
}

/// Drop the per-user state on logout. The engine keeps its in-memory mode
/// (the Qobuz gate must survive the transition); only the stores close.
pub fn teardown() {
    engine().teardown();
    if let Ok(mut guard) = SUBSCRIPTION.lock() {
        *guard = None;
    }
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// D4 producer: a successful login verdict. Clears any running grace clock.
pub fn subscription_mark_valid() {
    let now = now_unix_secs();
    match SUBSCRIPTION.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(store) => {
                if let Err(e) = store.mark_valid(now) {
                    log::error!("[qbz-slint] subscription mark_valid failed: {e}");
                }
            }
            None => log::warn!("[qbz-slint] subscription mark_valid: no store open"),
        },
        Err(e) => log::error!("[qbz-slint] subscription store lock poisoned: {e}"),
    }
}

/// D4 producer: an EXPLICIT ineligible-account login verdict
/// (`ApiError::IneligibleUser`). Generic 401/network errors must never
/// reach this — the grace clock only starts on a real verdict.
///
/// An ineligible verdict can arrive before any session activation (the
/// failed login never activates), so when no store is open this falls back
/// to transiently opening the LAST user's store.
pub fn subscription_mark_invalid() {
    let now = now_unix_secs();
    if let Ok(guard) = SUBSCRIPTION.lock() {
        if let Some(store) = guard.as_ref() {
            if let Err(e) = store.mark_invalid(now) {
                log::error!("[qbz-slint] subscription mark_invalid failed: {e}");
            }
            return;
        }
    }
    let Some(user_id) = UserDataPaths::load_last_user_id() else {
        log::warn!("[qbz-slint] subscription mark_invalid: no previous user, skipping");
        return;
    };
    let Some(dir) = user_data_dir(user_id) else {
        log::warn!("[qbz-slint] subscription mark_invalid: data dir unavailable");
        return;
    };
    match SubscriptionStateStore::new_at(&dir) {
        Ok(store) => {
            if let Err(e) = store.mark_invalid(now) {
                log::error!("[qbz-slint] subscription mark_invalid failed: {e}");
            }
        }
        Err(e) => log::error!("[qbz-slint] subscription state store open failed: {e}"),
    }
}

/// D4 consumer: may the offline cache serve FULL tracks right now? Binary —
/// within the 30-day grace window yes, past it no; there is NO 30-second
/// preview path. Fail-open `true` when no store is bound. Consumed by the
/// playback gating (`playback::offline_playability`).
pub fn offline_playback_allowed() -> bool {
    let now = now_unix_secs();
    SUBSCRIPTION
        .lock()
        .ok()
        .and_then(|guard| {
            guard
                .as_ref()
                .map(|store| store.offline_playback_allowed(now).unwrap_or(true))
        })
        .unwrap_or(true)
}

/// Mirror of Tauri's activation-time purge consumer
/// (`session_lifecycle.rs` `activate_session`, lines ~237-264): when the
/// subscription has been invalid past the grace window, purge the offline
/// cache once and record the purge. Runs detached so session entry never
/// blocks on it.
fn spawn_subscription_purge_check() {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        log::warn!("[qbz-slint] subscription purge check: no tokio runtime, skipped");
        return;
    };
    handle.spawn(async move {
        let now = now_unix_secs();
        // Read the verdict without holding the lock across awaits.
        let should_purge = SUBSCRIPTION
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .and_then(|store| store.should_purge_offline_cache(now).ok())
            })
            .unwrap_or(false);
        if !should_purge {
            return;
        }

        log::warn!(
            "[qbz-slint] Subscription invalid beyond the grace window. Purging offline cache."
        );
        let Some(off) = crate::offline::get().await else {
            // init order puts offline::activate before init_for_user; this
            // only triggers if that ordering regresses. Re-checked next
            // activation, so the purge is deferred, not lost.
            log::warn!("[qbz-slint] purge deferred: offline cache not active");
            return;
        };
        if let Err(e) = qbz_offline_cache::purge_all_cached_files(&off, &off.library_db).await {
            log::error!("[qbz-slint] failed to purge offline cache: {e}");
            return;
        }
        // Resync the in-memory cached-ids set the track rows read.
        crate::offline_cache::load_cached_ids().await;
        if let Ok(guard) = SUBSCRIPTION.lock() {
            if let Some(store) = guard.as_ref() {
                let _ = store.mark_offline_cache_purged(now);
            }
        }
        log::info!("[qbz-slint] offline cache purged (subscription grace elapsed)");
    });
}
