//! Slint QConnect service (pieces c + d, Phase S).
//!
//! `SlintQconnectService` is the connect-flow facade for the Slint frontend: it
//! owns the connection lifecycle and reproduces the Tauri
//! `QconnectServiceState::connect` recipe (build transport -> one shared
//! sync-state Mutex -> sink -> `QconnectApp::new` -> `set_app` -> `connect` ->
//! subscribe transport events BEFORE the spawn -> spawn `run_session_loop`), plus
//! controller bootstrap (JoinSession + AskForQueueState) and the deferred
//! renderer-join.
//!
//! `SlintSessionLoopHost` implements the frontend-agnostic
//! `qconnect_app::SessionLoopHost` so the shared session loop drives lifecycle,
//! reconnect bootstrap/resync, deferred renderer-join, and reconnect-exhausted
//! teardown through this adapter — exactly as `TauriSessionLoopHost` does.

use std::sync::Arc;

use std::time::{SystemTime, UNIX_EPOCH};

use qbz_app::shell::AppRuntime;
use qconnect_app::queue_resolution::{
    find_cursor_index_by_queue_item_id, find_cursor_index_by_track_id, ordered_queue_cursors,
    resolve_controller_queue_item_from_snapshots, resolve_queue_item_ids_from_queue_state,
    QconnectRemoteSkipDirection,
};
use qconnect_app::renderer::{
    PLAYING_STATE_PAUSED, PLAYING_STATE_PLAYING, PLAYING_STATE_STOPPED,
};
use qconnect_app::{
    build_effective_renderer_snapshot, ensure_session_renderer_state, is_local_renderer_active,
    is_peer_renderer_active, renderer_allows_remote_volume, QConnectQueueState,
    QConnectRendererState, QconnectApp, QconnectAppEvent, QconnectEventSink,
    queue_item_snapshot_for_cursor, QconnectFileAudioQualitySnapshot, QconnectLifecycleState,
    QconnectRemoteSyncState, QconnectSessionState, QueueCommandType, RendererReport,
    RendererReportType, SessionLoopHost,
};
use qconnect_transport_ws::{NativeWsTransport, WsTransportConfig};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::adapter::SlintAdapter;
use crate::qconnect_engine::SlintRendererEngine;
use crate::qconnect_event_sink::{SlintQconnectApp, SlintQconnectEventSink};
use crate::qconnect_transport::{
    build_set_position_player_state_request, default_qconnect_device_info,
    default_qconnect_device_info_with_name, load_persisted_device_name, resolve_transport_config,
    QconnectJoinSessionRequest, QconnectMuteVolumeRequest, QconnectQueueVersionPayload,
    QconnectSetPlayerStateQueueItemPayload, QconnectSetPlayerStateRequest, QconnectSetVolumeRequest,
    AUDIO_QUALITY_HIRES_LEVEL2, BUFFER_STATE_OK,
};
use crate::AppWindow;

const QCONNECT_PLAY_TRACK_HANDOFF_WAIT_MS: u64 = 1_500;
const QCONNECT_PLAY_TRACK_HANDOFF_POLL_MS: u64 = 50;

/// Wall-clock now in ms (mirrors the Tauri `qconnect_now_ms`, which is
/// Tauri-local; reimplemented inline here per the controller-port spec).
fn qconnect_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Reduced peer-renderer playback snapshot for the now-playing seek bar while
/// QBZ is CONTROLLING a peer. Sourced from the effective remote renderer
/// snapshot; the poll loop extrapolates position from `position_ms` +
/// (now - `updated_at_ms`) while `playing`. Avoids leaking core types into the
/// playback module. Mirrors the Svelte `effectiveCurrentTime` derivation.
pub struct RemoteNowPlaying {
    pub position_ms: u64,
    pub updated_at_ms: u64,
    pub playing: bool,
    /// Peer renderer's reported volume (0..=100). `None` when the peer hasn't
    /// reported a volume yet — the bar then clamps to a safe 50% instead of
    /// reflecting QBZ's local 100, so a drag never nukes the AVR.
    pub volume: Option<i32>,
    /// The peer's current track id (from the effective remote renderer
    /// snapshot's `current_track`; 0 when none). The poll loop edge-detects a
    /// change against its last-seen value to refresh the bar/queue meta when
    /// the peer advances a track on its own.
    pub track_id: u64,
    /// The peer's shuffle flag, so the controller bar's shuffle button reflects
    /// the REMOTE state (the poll loop only updated this for local playback).
    pub shuffle_mode: bool,
    /// The peer's repeat mode, already mapped to the UI's `repeat-mode`
    /// (0=off, 1=all, 2=one) from the QConnect wire loop_mode (1=off, 3=all,
    /// 2=one), so the controller bar's repeat button reflects the REMOTE state.
    pub repeat_mode: i32,
}

/// Process-wide QConnect service singleton (one per app, like the playback
/// QueueController). Initialized once at shell setup; the connect trigger + the
/// future `*_if_remote` transport routing reach it through `service()`.
static SERVICE: std::sync::OnceLock<Arc<SlintQconnectService>> = std::sync::OnceLock::new();

/// Initialize the QConnect service singleton (idempotent — a second call returns
/// the existing instance, ignoring the new args).
pub fn init_service(runtime: Runtime, window: slint::Weak<AppWindow>) -> Arc<SlintQconnectService> {
    SERVICE
        .get_or_init(|| Arc::new(SlintQconnectService::new(runtime, window)))
        .clone()
}

/// The initialized QConnect service, if shell setup has run.
pub fn service() -> Option<Arc<SlintQconnectService>> {
    SERVICE.get().cloned()
}

// ---- DEV diagnostics (QconnectDevModal) ------------------------------------
// A rolling, runtime-inspectable event log + live status block, so QConnect can
// be debugged WITHOUT a rebuild (Slint builds are slow). Populated by the event
// sink; rendered by `ui/shell/QconnectDevModal.slint`.

static DEV_LOG: std::sync::OnceLock<std::sync::Mutex<std::collections::VecDeque<String>>> =
    std::sync::OnceLock::new();
static DEV_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
const DEV_LOG_CAP: usize = 150;

fn dev_log_text(push: Option<String>, clear: bool) -> String {
    let buf = DEV_LOG.get_or_init(|| std::sync::Mutex::new(std::collections::VecDeque::new()));
    let mut guard = buf.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if clear {
        guard.clear();
    }
    if let Some(line) = push {
        guard.push_front(line);
        while guard.len() > DEV_LOG_CAP {
            guard.pop_back();
        }
    }
    guard.iter().cloned().collect::<Vec<_>>().join("\n")
}

/// Append a formatted event line (with a relative timestamp) to the DEV log and
/// push the joined text to the modal. Called for every inbound QConnect event.
pub fn dev_push_event(weak: &slint::Weak<AppWindow>, line: String) {
    let start = DEV_START.get_or_init(std::time::Instant::now);
    let ms = start.elapsed().as_millis();
    let text = dev_log_text(Some(format!("[{ms}ms] {line}")), false);
    let _ = weak.upgrade_in_event_loop(move |w| {
        use slint::ComponentHandle;
        w.global::<crate::QconnectDevState>().set_log_text(text.into());
    });
}

/// Replace the DEV status block (session topology / renderer roles / queue).
pub fn dev_set_status(weak: &slint::Weak<AppWindow>, status: String) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        use slint::ComponentHandle;
        w.global::<crate::QconnectDevState>()
            .set_status(status.into());
    });
}

/// Clear the DEV event log (wired to `QconnectDevState.clear()`).
pub fn dev_clear(weak: &slint::Weak<AppWindow>) {
    let text = dev_log_text(None, true);
    let _ = weak.upgrade_in_event_loop(move |w| {
        use slint::ComponentHandle;
        w.global::<crate::QconnectDevState>().set_log_text(text.into());
    });
}

struct SlintQconnectRuntime {
    app: Arc<SlintQconnectApp>,
    #[allow(dead_code)] // retained for status/endpoint reporting in the UI step
    config: WsTransportConfig,
    event_loop: tokio::task::JoinHandle<()>,
    #[allow(dead_code)] // shared with the app + sink; kept for future snapshot queries
    sync_state: Arc<Mutex<QconnectRemoteSyncState>>,
}

#[derive(Default)]
struct SlintQconnectInner {
    runtime: Option<SlintQconnectRuntime>,
    last_error: Option<String>,
    lifecycle_state: QconnectLifecycleState,
}

/// Dedup + gate a lifecycle transition: only emit while a runtime is alive and
/// the state actually changes. Mirrors the Tauri `update_lifecycle_state_if_running`.
async fn update_lifecycle_state_if_running(
    inner: &Arc<Mutex<SlintQconnectInner>>,
    sink: &SlintQconnectEventSink,
    next: QconnectLifecycleState,
) {
    let mut guard = inner.lock().await;
    if guard.runtime.is_none() {
        return;
    }
    if guard.lifecycle_state == next {
        return;
    }
    guard.lifecycle_state = next;
    drop(guard);
    sink.on_event(QconnectAppEvent::LifecycleChanged { state: next })
        .await;
}

pub struct SlintQconnectService {
    inner: Arc<Mutex<SlintQconnectInner>>,
    runtime: Runtime,
    window: slint::Weak<AppWindow>,
    #[allow(dead_code)] // wired to the device-name settings UI in a later step
    custom_device_name: Arc<tokio::sync::RwLock<Option<String>>>,
    /// Track-id list of the queue we last pushed to the session (controller-side
    /// queue sync). Guards against re-pushing the same queue before the cloud's
    /// echo `QueueUpdated` lands (which would otherwise double-push on the next
    /// track tick). Cleared on disconnect.
    last_pushed_queue_ids: Mutex<Option<Vec<u64>>>,
}

/// Slint-local mirror of the Tauri `QconnectVisibleQueueProjection` reduced to
/// the pieces the reorder payload needs: the current track's queue_item_id (the
/// anchor) and the ordered upcoming queue_item_ids. Built from the cloud queue +
/// renderer snapshot via the shared `qconnect-app` cursor helpers (no Tauri-local
/// code). Stores only ids so it needs no `qconnect-core::QueueItem` import.
struct VisibleUpcomingProjection {
    current_track_qid: Option<u64>,
    upcoming_qids: Vec<u64>,
}

/// Rebuild the visible upcoming projection (current anchor + ordered upcoming
/// queue_item_ids) from a cloud queue + renderer snapshot, mirroring the Tauri
/// `build_visible_queue_projection` using the SHARED qconnect-app cursor helpers.
fn build_visible_upcoming_projection(
    queue: &QConnectQueueState,
    renderer: &QConnectRendererState,
) -> VisibleUpcomingProjection {
    let cursors = ordered_queue_cursors(queue);

    let current_index = find_cursor_index_by_queue_item_id(
        &cursors,
        queue,
        renderer.current_track.as_ref().map(|i| i.queue_item_id),
    )
    .or_else(|| {
        find_cursor_index_by_track_id(
            &cursors,
            queue,
            renderer.current_track.as_ref().map(|i| i.track_id),
        )
    });

    let next_index = find_cursor_index_by_queue_item_id(
        &cursors,
        queue,
        renderer.next_track.as_ref().map(|i| i.queue_item_id),
    )
    .or_else(|| {
        find_cursor_index_by_track_id(
            &cursors,
            queue,
            renderer.next_track.as_ref().map(|i| i.track_id),
        )
    });

    // (current_track, start_index): the upcoming list starts AFTER the current
    // track; if the current is unknown but the next is, infer the current from
    // the cursor before next. Mirrors the Tauri projection.
    let (current_track_qid, start_index) = if let Some(index) = current_index {
        (
            queue_item_snapshot_for_cursor(queue, cursors[index]).map(|i| i.queue_item_id),
            index.saturating_add(1),
        )
    } else if let Some(index) = next_index {
        let inferred = index
            .checked_sub(1)
            .and_then(|c| cursors.get(c).copied())
            .and_then(|cur| queue_item_snapshot_for_cursor(queue, cur))
            .map(|i| i.queue_item_id);
        (inferred, index)
    } else {
        (None, 0)
    };

    let upcoming_qids = cursors
        .into_iter()
        .skip(start_index)
        .filter_map(|cur| queue_item_snapshot_for_cursor(queue, cur))
        .map(|i| i.queue_item_id)
        .collect();

    VisibleUpcomingProjection {
        current_track_qid,
        upcoming_qids,
    }
}

/// 1:1 port of the Tauri `build_qconnect_reorder_payload`. `from_index`/
/// `to_index` index INTO the visible upcoming list. None when out of range,
/// `Some({})` for a no-op, else the wire payload (moved id + insert_after anchor).
fn build_reorder_payload(
    projection: &VisibleUpcomingProjection,
    from_index: usize,
    to_index: usize,
) -> Option<Value> {
    let len = projection.upcoming_qids.len();
    if from_index >= len || to_index >= len {
        return None;
    }
    if from_index == to_index {
        return Some(json!({}));
    }

    let mut ids = projection.upcoming_qids.clone();
    let moved = ids.remove(from_index);
    let insert_position = if from_index < to_index {
        to_index.saturating_sub(1)
    } else {
        to_index
    };
    let insert_after = if insert_position == 0 {
        projection.current_track_qid
    } else {
        ids.get(insert_position - 1).copied()
    };

    Some(json!({
        "queue_item_ids": [moved as i64],
        "insert_after": insert_after.map(|v| v as i64),
        "autoplay_reset": false,
        "autoplay_loading": false,
    }))
}

impl SlintQconnectService {
    pub fn new(runtime: Runtime, window: slint::Weak<AppWindow>) -> Self {
        let saved_name = load_persisted_device_name();
        Self {
            inner: Arc::new(Mutex::new(SlintQconnectInner::default())),
            runtime,
            window,
            custom_device_name: Arc::new(tokio::sync::RwLock::new(saved_name)),
            last_pushed_queue_ids: Mutex::new(None),
        }
    }

    pub async fn is_running(&self) -> bool {
        self.inner.lock().await.runtime.is_some()
    }

    /// D5 (offline-MODE): force-disconnect on every transition INTO offline
    /// (induced or real), so an established session never outlives the offline
    /// gate. Spawned once at service init (main.rs, next to `init_service`).
    /// Idempotent: skips when no runtime is alive. Uses the SAME `disconnect()`
    /// path as the UI toggle (force-Offs lifecycle, shuts the transport down —
    /// which also kills its 60s idle-retry rearm — disarms the watchdog and
    /// clears the renderer/cast UI), then clears the bar's connected flag
    /// exactly like the manual toggle does. Deliberately NO auto-reconnect on
    /// the online edge: QConnect only reconnects through its existing
    /// user-facing flows (D5).
    pub fn spawn_offline_force_disconnect(self: &Arc<Self>, handle: &tokio::runtime::Handle) {
        let service = Arc::clone(self);
        handle.spawn(async move {
            let mut rx = crate::offline_mode::engine().subscribe();
            let mut was_offline = rx.borrow_and_update().is_offline();
            loop {
                if rx.changed().await.is_err() {
                    break;
                }
                let is_offline = rx.borrow_and_update().is_offline();
                let entered_offline = is_offline && !was_offline;
                was_offline = is_offline;
                if !entered_offline {
                    continue;
                }
                if !service.is_running().await {
                    continue;
                }
                log::info!(
                    "[QConnect] Offline mode entered; force-disconnecting Qobuz Connect (D5)"
                );
                dev_push_event(&service.window, "-> force-disconnect (offline mode)".to_string());
                if let Err(err) = service.disconnect().await {
                    log::warn!("[QConnect] offline force-disconnect failed: {err}");
                }
                // Mirror the manual toggle's tail: the bar's connect toggle off.
                let _ = service.window.upgrade_in_event_loop(|w| {
                    use slint::ComponentHandle;
                    w.global::<crate::NowPlayingState>().set_qconnect_connected(false);
                });
            }
        });
    }

    /// Report this device's playback state to the cloud while QBZ is the ACTIVE
    /// LOCAL renderer (driven by the playback poll loop). Mirrors the Tauri
    /// `v2_qconnect_report_playback_state` essentials: self-gates on
    /// is_local_renderer_active (no-op when not connected, or when a PEER owns
    /// playback), resolves the current/next queue_item_id from the playing track
    /// (the frontend doesn't track qids), sends a RndrSrvrStateUpdated, and keeps
    /// the app's renderer position in sync. `position_ms`/`duration_ms` are in
    /// MILLISECONDS (the QConnect protocol unit).
    pub async fn report_playback_state(
        &self,
        playing_state: i32,
        position_ms: i64,
        duration_ms: i64,
        track_id: u64,
    ) {
        let (app, sync_state) = {
            let guard = self.inner.lock().await;
            match guard.runtime.as_ref() {
                Some(runtime) => (Arc::clone(&runtime.app), Arc::clone(&runtime.sync_state)),
                None => return,
            }
        };

        // Only report when WE are the active renderer. When a peer renderer owns
        // playback (QBZ is acting as a controller) the renderer reports come from
        // the peer, not us.
        {
            let state = sync_state.lock().await;
            if !is_local_renderer_active(&state.session) {
                return;
            }
        }

        let (current_qid, next_qid) =
            resolve_queue_item_ids_by_track_id(&app, &sync_state, track_id).await;
        let queue_version = app.queue_state_snapshot().await.version;

        let report = RendererReport::new(
            RendererReportType::RndrSrvrStateUpdated,
            Uuid::new_v4().to_string(),
            queue_version,
            json!({
                "playing_state": playing_state,
                "buffer_state": BUFFER_STATE_OK,
                "current_position": position_ms,
                "duration": duration_ms,
                "current_queue_item_id": current_qid,
                "next_queue_item_id": next_qid,
                "queue_version": {
                    "major": queue_version.major,
                    "minor": queue_version.minor
                }
            }),
        );
        if let Err(err) = app.send_renderer_report_command(report).await {
            log::warn!("[QConnect] Failed to report playback state: {err}");
        }

        if position_ms >= 0 {
            app.update_renderer_position(position_ms as u64).await;
        }

        // Report the live output format so the controller shows the correct
        // quality badge (CD / Hi-Res). Reads the player's current output
        // (sample_rate/bit_depth); channels default to stereo. Both reports dedup
        // internally in qconnect-app, so calling them every report tick is cheap.
        let player = self.runtime.core().player();
        let sample_rate = player.state.get_sample_rate();
        let bit_depth = player.state.get_bit_depth();
        if let Some(snapshot) =
            build_file_audio_quality_snapshot(sample_rate, bit_depth, QCONNECT_RENDERER_CHANNELS)
        {
            if let Err(err) = app
                .report_file_audio_quality_if_changed(queue_version, snapshot)
                .await
            {
                log::warn!("[QConnect] Failed to report file audio quality: {err}");
            }
            if let Err(err) = app
                .report_device_audio_quality_if_changed(
                    queue_version,
                    snapshot.sampling_rate,
                    snapshot.bit_depth,
                    snapshot.nb_channels,
                )
                .await
            {
                log::warn!("[QConnect] Failed to report device audio quality: {err}");
            }
        }
    }

    /// Establish the QConnect session. Gated on an initialized API client (the
    /// qws/createToken discovery needs it). Idempotent: a second call while a
    /// runtime is alive is a no-op (the UI toggle stays on).
    pub async fn connect(&self) -> Result<(), String> {
        if !self.runtime.core().is_api_initialized().await {
            return Err("Qobuz API is not initialized; cannot start Qobuz Connect".to_string());
        }
        // D5 (offline-MODE): QConnect is not available in ANY offline mode,
        // induced or real — refuse before touching the transport. Sessions that
        // were already up when offline was entered are torn down by the
        // force-disconnect watcher (`spawn_offline_force_disconnect`).
        if crate::offline_mode::engine().is_offline() {
            log::info!("[QConnect] connect() refused: offline mode active (D5)");
            crate::toast::error_weak(&self.window, "Qobuz Connect is unavailable while offline");
            return Err("Qobuz Connect is unavailable while offline".to_string());
        }

        // Claim the connect slot ATOMICALLY before the transport-config await, so
        // two concurrent connect()s can't both build a runtime (the second would
        // overwrite + leak the first's transport + event-loop task). A live
        // runtime OR an in-flight `Connecting` both short-circuit to a no-op —
        // the re-check the adversarial review flagged (`runtime.is_some()` only)
        // had a TOCTOU window across the await; the `Connecting` sentinel closes it.
        {
            let mut guard = self.inner.lock().await;
            if guard.runtime.is_some()
                || guard.lifecycle_state == QconnectLifecycleState::Connecting
            {
                log::info!(
                    "[QConnect] connect() called while already {:?}; no-op",
                    guard.lifecycle_state
                );
                return Ok(());
            }
            guard.lifecycle_state = QconnectLifecycleState::Connecting;
            guard.last_error = None;
        }

        let config = match resolve_transport_config(&self.runtime).await {
            Ok(config) => config,
            Err(err) => {
                // Release the Connecting claim so a later retry can proceed.
                let mut guard = self.inner.lock().await;
                if guard.runtime.is_none() {
                    guard.lifecycle_state = QconnectLifecycleState::Off;
                }
                return Err(err);
            }
        };

        let transport = Arc::new(NativeWsTransport::new());
        let sync_state = Arc::new(Mutex::new(QconnectRemoteSyncState::default()));
        let engine = SlintRendererEngine::new(Arc::clone(&self.runtime));
        let sink = Arc::new(SlintQconnectEventSink::new(
            engine,
            Arc::clone(&self.runtime),
            Arc::clone(&sync_state),
            self.window.clone(),
        ));
        let app = Arc::new(QconnectApp::new(
            Arc::clone(&transport) as Arc<NativeWsTransport>,
            Arc::clone(&sink),
            Arc::clone(&sync_state),
        ));
        // Wire the owning app into the sink so it can emit reports (e.g.
        // is_active=true after SetActive(true)) and drive session-apply.
        sink.set_app(&app);

        if let Err(err) = app.connect(config.clone()).await {
            let mut guard = self.inner.lock().await;
            guard.lifecycle_state = QconnectLifecycleState::Off;
            let msg = format!("qconnect transport connect failed: {err}");
            guard.last_error = Some(msg.clone());
            return Err(msg);
        }

        // Subscribe to transport events SYNCHRONOUSLY here — after connect()
        // returns and BEFORE the spawn / any further await — so the receiver is
        // live before the WS handshake emits Connected / Subscribed /
        // SessionEstablished / SESSION_STATE. tokio broadcast has no replay; a
        // receiver created inside the spawned loop would race + drop those.
        let transport_rx = app.subscribe_transport_events();
        let idle_retry_active = config.reconnect_idle_retry_ms > 0;
        let host: Arc<dyn SessionLoopHost> = Arc::new(SlintSessionLoopHost {
            app: Arc::clone(&app),
            sync_state: Arc::clone(&sync_state),
            inner: Arc::clone(&self.inner),
            sink: Arc::clone(&sink),
            runtime: Arc::clone(&self.runtime),
        });
        let app_for_loop = Arc::clone(&app);
        let event_loop = tokio::spawn(async move {
            app_for_loop
                .run_session_loop(host, transport_rx, idle_retry_active)
                .await;
        });

        let runtime_app = Arc::clone(&app);
        {
            let mut guard = self.inner.lock().await;
            guard.last_error = None;
            guard.runtime = Some(SlintQconnectRuntime {
                app,
                config,
                event_loop,
                sync_state,
            });
        }

        let custom_name = self.custom_device_name.read().await.clone();
        if let Err(err) = bootstrap_remote_presence(&runtime_app, custom_name).await {
            let _ = self.disconnect().await;
            let mut guard = self.inner.lock().await;
            guard.last_error = Some(format!("qconnect bootstrap failed: {err}"));
            return Err(format!("qconnect bootstrap failed: {err}"));
        }

        // D5 race close: offline may have been entered while this connect was
        // in flight (the watcher skips while no runtime is alive). Re-check now
        // that the runtime is set; any later flip is the watcher's job.
        if crate::offline_mode::engine().is_offline() {
            log::info!("[QConnect] offline mode entered during connect(); tearing down (D5)");
            let _ = self.disconnect().await;
            return Err("Qobuz Connect is unavailable while offline".to_string());
        }

        Ok(())
    }

    pub async fn disconnect(&self) -> Result<(), String> {
        let runtime = {
            let mut guard = self.inner.lock().await;
            // Always force Off — "disable QConnect" must succeed regardless of the
            // current lifecycle (issue #358). The transport shutdown + the loop
            // abort tear down any in-flight reconnect.
            guard.lifecycle_state = QconnectLifecycleState::Off;
            guard.runtime.take()
        };

        *self.last_pushed_queue_ids.lock().await = None;

        if let Some(runtime) = runtime {
            // FIX #20: disarm the in-flight liveness watchdog BEFORE aborting the
            // event loop. A watchdog armed while a peer was playing fires
            // ~seconds later and emits RendererUnreachable (spurious error toast +
            // lingering golden cast badge) if we don't bump the generation it
            // captured. run_renderer_watchdog no-ops on a generation mismatch
            // (app.rs `run_renderer_watchdog`), so bumping the shared field here
            // neutralizes any pending watchdog task. `watchdog_generation` is a
            // public field on QconnectRemoteSyncState; mutate it directly.
            {
                let mut state = runtime.sync_state.lock().await;
                state.watchdog_generation = state.watchdog_generation.wrapping_add(1);
                // Clear the session topology + per-renderer cache so that any
                // event still in flight when the loop aborts cannot recompute
                // `is_peer_renderer_active() == true` and resurrect the golden
                // render badge AFTER we clear is-remote/cast-target below
                // (tokio `abort()` is not instantaneous; the sink can run one
                // more `refresh_now_playing_remote_state` from a stale session).
                state.session = QconnectSessionState::default();
                state.session_renderer_states.clear();
            }
            if let Err(err) = runtime.app.disconnect().await {
                let mut guard = self.inner.lock().await;
                guard.last_error = Some(format!("qconnect disconnect failed: {err}"));
            }
            runtime.event_loop.abort();
        }

        // Clear the UI: no session = no renderers in the picker, and no remote
        // playback state. Without this the last device list + is-remote/cast
        // state linger after turning Connect off.
        let _ = self.window.upgrade_in_event_loop(|w| {
            use slint::ComponentHandle;
            let dev = w.global::<crate::QconnectDevState>();
            dev.set_devices(slint::ModelRc::new(
                slint::VecModel::<crate::QconnectDevice>::default(),
            ));
            dev.set_active_renderer_id(-1);
            let np = w.global::<crate::NowPlayingState>();
            np.set_is_remote(false);
            np.set_cast_target("".into());
            np.set_volume_locked(false);
        });

        Ok(())
    }

    /// Controller-side queue sync: when the LOCAL queue differs from the session
    /// queue (the user started a new album/playlist on QBZ while connected), push
    /// it to the session so the controller (e.g. the iOS app) sees it. Called from
    /// the playback poll loop on each track transition.
    ///
    /// Echo-safe by construction: the inbound materialize path never calls this,
    /// and we skip when the local queue already equals the cloud's last-applied
    /// queue OR the last queue we pushed. Admission mirrors the webplayer's
    /// `assessQconnectQueueSync` — all-or-nothing: a queue containing any local /
    /// Plex track is refused whole (a renderer can only play Qobuz catalog ids;
    /// offline qobuz_download IS eligible — its id is the real Qobuz id).
    pub async fn sync_local_queue_if_changed(&self) {
        let (app, sync_state) = {
            let guard = self.inner.lock().await;
            match guard.runtime.as_ref() {
                Some(runtime) => (Arc::clone(&runtime.app), Arc::clone(&runtime.sync_state)),
                None => return,
            }
        };

        // Only push while WE are the active renderer (the user is driving QBZ).
        {
            let state = sync_state.lock().await;
            if !is_local_renderer_active(&state.session) {
                return;
            }
        }

        let (tracks, current_index) = self.runtime.core().get_all_queue_tracks().await;
        if tracks.is_empty() {
            return;
        }
        let ordered_ids: Vec<u64> = tracks.iter().map(|track| track.id).collect();

        // Echo-suppress: skip when this is the cloud's current queue (materialized
        // inbound) so our own adoption / a remote queue change never bounces back.
        {
            let state = sync_state.lock().await;
            if let Some(applied) = &state.last_applied_queue_state {
                let applied_ids: Vec<u64> =
                    applied.queue_items.iter().map(|item| item.track_id).collect();
                if applied_ids == ordered_ids {
                    return;
                }
            }
        }
        // ...and skip when we already pushed this exact queue (cloud echo pending).
        {
            let pushed = self.last_pushed_queue_ids.lock().await;
            if pushed.as_deref() == Some(ordered_ids.as_slice()) {
                return;
            }
        }

        // Admission: refuse the whole push if any track isn't Qobuz-castable.
        let all_eligible = tracks.iter().all(|track| {
            let source = track
                .source
                .as_deref()
                .unwrap_or("qobuz")
                .to_ascii_lowercase();
            source != "local" && source != "plex" && track.id > 0
        });
        if !all_eligible {
            log::info!("[QConnect] Local queue has non-Qobuz tracks; not casting to Connect");
            crate::toast::error_weak(&self.window, "Mixed queue — not cast to Qobuz Connect");
            dev_push_event(&self.window, "-> queue push REFUSED (mixed/non-Qobuz)".to_string());
            // Remember it so we don't re-toast on every track tick within this queue.
            *self.last_pushed_queue_ids.lock().await = Some(ordered_ids);
            return;
        }

        let count = ordered_ids.len();
        let track_ids: Vec<i64> = ordered_ids.iter().map(|id| *id as i64).collect();
        let start_index = current_index.unwrap_or(0);
        let payload = json!({
            "track_ids": track_ids,
            "queue_position": start_index,
            "shuffle_mode": false,
            "shuffle_pivot_index": start_index,
            "context_uuid": Uuid::new_v4().to_string(),
            "autoplay_reset": true,
            "autoplay_loading": false,
        });
        let command = app
            .build_queue_command(QueueCommandType::CtrlSrvrQueueLoadTracks, payload)
            .await;
        match app.send_queue_command(command).await {
            Ok(_) => {
                log::info!(
                    "[QConnect] Pushed local queue to Connect ({count} tracks, start={start_index})"
                );
                dev_push_event(
                    &self.window,
                    format!("-> QueueLoadTracks {count} tracks start={start_index}"),
                );
                *self.last_pushed_queue_ids.lock().await = Some(ordered_ids);
            }
            Err(err) => log::warn!("[QConnect] Failed to push local queue: {err}"),
        }
    }

    /// Controller play-routing: when QBZ is CONTROLLING a peer renderer and the
    /// user plays a new album/track on QBZ, route it to the peer instead of
    /// playing it locally. Returns `true` when handled remotely (caller MUST
    /// NOT play locally) and `false` when no peer is active (caller plays
    /// locally — the existing behavior runs byte-unchanged).
    ///
    /// Unlike `sync_local_queue_if_changed`, the queue push here is
    /// UNCONDITIONAL (no echo-gate, no is_local_renderer_active gate) — the user
    /// just issued a fresh play, so the current core queue IS what should run on
    /// the peer. Admission is the SAME all-or-nothing rule: a queue containing
    /// any local / Plex track is refused whole (a renderer can only play Qobuz
    /// catalog ids; offline qobuz_download IS eligible). On refusal it toasts and
    /// returns `true` (handled — do NOT fall back to playing a mixed queue
    /// locally that we just declined to cast). On any send error it logs and
    /// still returns `true` (a peer owns playback; falling back to local audio
    /// would double-play).
    pub async fn play_on_peer_if_active(&self, track_id: u64) -> bool {
        let (app, peer_active) = {
            let guard = self.inner.lock().await;
            let Some(runtime) = guard.runtime.as_ref() else {
                return false;
            };
            let peer_active = {
                let state = runtime.sync_state.lock().await;
                is_peer_renderer_active(&state.session)
            };
            (Arc::clone(&runtime.app), peer_active)
        };
        if !peer_active {
            return false;
        }

        // (a) Push the CURRENT core queue to the peer (unconditional).
        let (tracks, current_index) = self.runtime.core().get_all_queue_tracks().await;
        if tracks.is_empty() {
            return false;
        }
        let ordered_ids: Vec<u64> = tracks.iter().map(|track| track.id).collect();

        // Admission: refuse the whole push if any track isn't Qobuz-castable.
        let all_eligible = tracks.iter().all(|track| {
            let source = track
                .source
                .as_deref()
                .unwrap_or("qobuz")
                .to_ascii_lowercase();
            source != "local" && source != "plex" && track.id > 0
        });
        if !all_eligible {
            log::info!("[QConnect] Local queue has non-Qobuz tracks; not casting to Connect");
            crate::toast::error_weak(&self.window, "Mixed queue — not cast to Qobuz Connect");
            dev_push_event(&self.window, "-> queue push REFUSED (mixed/non-Qobuz)".to_string());
            // Remember it so the poll loop's sync doesn't re-toast this queue.
            *self.last_pushed_queue_ids.lock().await = Some(ordered_ids);
            // Handled: do NOT play a refused queue locally.
            return true;
        }

        let count = ordered_ids.len();
        let track_ids: Vec<i64> = ordered_ids.iter().map(|id| *id as i64).collect();
        let start_index = current_index.unwrap_or(0);
        let payload = json!({
            "track_ids": track_ids,
            "queue_position": start_index,
            "shuffle_mode": false,
            "shuffle_pivot_index": start_index,
            "context_uuid": Uuid::new_v4().to_string(),
            "autoplay_reset": true,
            "autoplay_loading": false,
        });
        let command = app
            .build_queue_command(QueueCommandType::CtrlSrvrQueueLoadTracks, payload)
            .await;
        match app.send_queue_command(command).await {
            Ok(_) => {
                log::info!(
                    "[QConnect] play_on_peer: pushed queue ({count} tracks, start={start_index})"
                );
                dev_push_event(
                    &self.window,
                    format!("-> play_on_peer QueueLoadTracks {count} start={start_index}"),
                );
                *self.last_pushed_queue_ids.lock().await = Some(ordered_ids);
            }
            Err(err) => {
                log::warn!("[QConnect] play_on_peer: queue push failed: {err}");
                // Still handled: a peer owns playback; never fall back to local.
                return true;
            }
        }

        // (b) SetPlayerState the peer to the requested track (polls the peer
        // queue until the track appears). Errors are logged, never fall back.
        match self.play_remote_renderer_track_if_active(track_id).await {
            Ok(_) => {}
            Err(err) => {
                log::warn!("[QConnect] play_on_peer: play_remote_track failed: {err}");
            }
        }
        true
    }

    /// Controller play-next routing: when QBZ is CONTROLLING a peer renderer and
    /// the user does "Play next" on QBZ, route the track to the peer's queue
    /// (insert right after the peer's CURRENT track) instead of mutating only the
    /// LOCAL queue (which the peer never sees). Returns `true` when handled (the
    /// caller MUST NOT enqueue locally) and `false` when no peer is active (the
    /// caller does the existing local insert, byte-unchanged).
    ///
    /// Mirrors the webplayer `queue_insert_tracks` path: a single command with a
    /// fresh `context_uuid`, `autoplay_reset: false`, `autoplay_loading: false`,
    /// and `insert_after` = the renderer's current queue_item_id (omitted when
    /// unknown). Admission is the SAME single-track rule as the queue sync: a
    /// `local` / `plex` track is refused (a renderer can't play a local/Plex id;
    /// offline `qobuz_download` IS eligible). On refusal it toasts + returns
    /// `true` (handled — do NOT add it locally while controlling). The cloud
    /// echoes a `QueueUpdated` that `materialize_remote_queue` applies to the
    /// local queue, so we never mutate the local queue here (avoids divergence).
    pub async fn play_next_on_peer_if_active(&self, track_id: u64, source: Option<&str>) -> bool {
        let peer_active = self.is_peer_renderer_active().await;
        if !peer_active {
            return false;
        }

        if !self.is_track_castable(track_id, source) {
            log::info!(
                "[QConnect] play_next_on_peer: track {track_id} not Qobuz-castable; refusing"
            );
            crate::toast::error_weak(&self.window, "Track not castable to Qobuz Connect");
            dev_push_event(
                &self.window,
                format!("-> play_next REFUSED (non-Qobuz track {track_id})"),
            );
            // Handled: do NOT add a non-castable track to the local queue while a
            // peer owns playback.
            return true;
        }

        // Resolve insert_after from the peer's current track (omit when unknown).
        let insert_after = self
            .effective_remote_renderer_snapshot()
            .await
            .ok()
            .flatten()
            .and_then(|(renderer, _queue, _session)| {
                renderer
                    .current_track
                    .as_ref()
                    .and_then(|item| i64::try_from(item.queue_item_id).ok())
            });

        let mut payload = json!({
            "track_ids": [track_id as i64],
            "context_uuid": Uuid::new_v4().to_string(),
            "autoplay_reset": false,
            "autoplay_loading": false,
        });
        if let Some(insert_after) = insert_after {
            payload["insert_after"] = json!(insert_after);
        }

        match self
            .send_command(QueueCommandType::CtrlSrvrQueueInsertTracks, payload)
            .await
        {
            Ok(_) => {
                log::info!(
                    "[QConnect] play_next_on_peer: inserted track {track_id} (after={insert_after:?})"
                );
                dev_push_event(
                    &self.window,
                    format!("-> play_next QueueInsertTracks {track_id} after={insert_after:?}"),
                );
            }
            Err(err) => {
                log::warn!("[QConnect] play_next_on_peer: insert failed: {err}");
                // Still handled: a peer owns playback; never fall back to local.
            }
        }
        true
    }

    /// Controller add-to-queue routing: when QBZ is CONTROLLING a peer renderer
    /// and the user does "Add to queue" on QBZ, append the track to the peer's
    /// queue instead of mutating only the LOCAL queue. Returns `true` when handled
    /// and `false` when no peer is active (caller does the existing local append).
    ///
    /// Mirrors the webplayer `queue_add_tracks` path: a single append command with
    /// a fresh `context_uuid`, `autoplay_reset: false`, `autoplay_loading: false`.
    /// Same admission + echo handling as `play_next_on_peer_if_active`.
    pub async fn add_to_queue_on_peer_if_active(&self, track_id: u64, source: Option<&str>) -> bool {
        let peer_active = self.is_peer_renderer_active().await;
        if !peer_active {
            return false;
        }

        if !self.is_track_castable(track_id, source) {
            log::info!(
                "[QConnect] add_to_queue_on_peer: track {track_id} not Qobuz-castable; refusing"
            );
            crate::toast::error_weak(&self.window, "Track not castable to Qobuz Connect");
            dev_push_event(
                &self.window,
                format!("-> add_to_queue REFUSED (non-Qobuz track {track_id})"),
            );
            return true;
        }

        let payload = json!({
            "track_ids": [track_id as i64],
            "context_uuid": Uuid::new_v4().to_string(),
            "autoplay_reset": false,
            "autoplay_loading": false,
        });

        match self
            .send_command(QueueCommandType::CtrlSrvrQueueAddTracks, payload)
            .await
        {
            Ok(_) => {
                log::info!("[QConnect] add_to_queue_on_peer: appended track {track_id}");
                dev_push_event(
                    &self.window,
                    format!("-> add_to_queue QueueAddTracks {track_id}"),
                );
            }
            Err(err) => {
                log::warn!("[QConnect] add_to_queue_on_peer: append failed: {err}");
            }
        }
        true
    }

    /// Controller add-to-queue routing for a MULTI-track batch (album / playlist /
    /// favorites bulk). Same contract as the single-track
    /// `add_to_queue_on_peer_if_active`, but admission is ALL-OR-NOTHING: if ANY
    /// track in the batch is non-castable (`local` / `plex`), the WHOLE batch is
    /// refused (toast + `return true`, nothing routed and nothing added locally) —
    /// mirroring the queue-sync's all-or-nothing rule. A single
    /// `CtrlSrvrQueueAddTracks` carries every id (the protocol `track_ids` is a
    /// full `Vec`), and the cloud echoes a `QueueUpdated` that
    /// `materialize_remote_queue` applies locally, so we never mutate the local
    /// queue here. Returns `false` only when no peer is active (caller appends
    /// locally) or the batch is empty (caller no-ops).
    pub async fn add_to_queue_batch_on_peer_if_active(
        &self,
        tracks: &[(u64, Option<String>)],
    ) -> bool {
        if !self.is_peer_renderer_active().await {
            return false;
        }
        if tracks.is_empty() {
            return false;
        }

        if let Some((bad_id, _)) = tracks
            .iter()
            .find(|(id, source)| !self.is_track_castable(*id, source.as_deref()))
        {
            log::info!(
                "[QConnect] add_to_queue_batch_on_peer: track {bad_id} not Qobuz-castable; refusing whole batch"
            );
            crate::toast::error_weak(&self.window, "Some tracks can't be cast to Qobuz Connect");
            dev_push_event(
                &self.window,
                format!(
                    "-> add_to_queue REFUSED (non-Qobuz track {bad_id} in batch of {})",
                    tracks.len()
                ),
            );
            return true;
        }

        let ids: Vec<i64> = tracks.iter().map(|(id, _)| *id as i64).collect();
        let count = ids.len();
        let payload = json!({
            "track_ids": ids,
            "context_uuid": Uuid::new_v4().to_string(),
            "autoplay_reset": false,
            "autoplay_loading": false,
        });

        match self
            .send_command(QueueCommandType::CtrlSrvrQueueAddTracks, payload)
            .await
        {
            Ok(_) => {
                log::info!("[QConnect] add_to_queue_batch_on_peer: appended {count} tracks");
                dev_push_event(
                    &self.window,
                    format!("-> add_to_queue QueueAddTracks {count} tracks"),
                );
            }
            Err(err) => {
                log::warn!("[QConnect] add_to_queue_batch_on_peer: append failed: {err}");
            }
        }
        true
    }

    /// Controller play-next routing for a MULTI-track batch. Same all-or-nothing
    /// admission as `add_to_queue_batch_on_peer_if_active`. The server
    /// `CtrlSrvrQueueInsertTracks` inserts the whole `track_ids` block right after
    /// `insert_after` and PRESERVES the list order, so the ids are passed in
    /// NATURAL order here (unlike the LOCAL fall-through, which reverses per-track
    /// `add_track_next` inserts to achieve the same effect).
    pub async fn play_next_batch_on_peer_if_active(
        &self,
        tracks: &[(u64, Option<String>)],
    ) -> bool {
        if !self.is_peer_renderer_active().await {
            return false;
        }
        if tracks.is_empty() {
            return false;
        }

        if let Some((bad_id, _)) = tracks
            .iter()
            .find(|(id, source)| !self.is_track_castable(*id, source.as_deref()))
        {
            log::info!(
                "[QConnect] play_next_batch_on_peer: track {bad_id} not Qobuz-castable; refusing whole batch"
            );
            crate::toast::error_weak(&self.window, "Some tracks can't be cast to Qobuz Connect");
            dev_push_event(
                &self.window,
                format!(
                    "-> play_next REFUSED (non-Qobuz track {bad_id} in batch of {})",
                    tracks.len()
                ),
            );
            return true;
        }

        // Resolve insert_after from the peer's current track (omit when unknown).
        let insert_after = self
            .effective_remote_renderer_snapshot()
            .await
            .ok()
            .flatten()
            .and_then(|(renderer, _queue, _session)| {
                renderer
                    .current_track
                    .as_ref()
                    .and_then(|item| i64::try_from(item.queue_item_id).ok())
            });

        let ids: Vec<i64> = tracks.iter().map(|(id, _)| *id as i64).collect();
        let count = ids.len();
        let mut payload = json!({
            "track_ids": ids,
            "context_uuid": Uuid::new_v4().to_string(),
            "autoplay_reset": false,
            "autoplay_loading": false,
        });
        if let Some(insert_after) = insert_after {
            payload["insert_after"] = json!(insert_after);
        }

        match self
            .send_command(QueueCommandType::CtrlSrvrQueueInsertTracks, payload)
            .await
        {
            Ok(_) => {
                log::info!(
                    "[QConnect] play_next_batch_on_peer: inserted {count} tracks (after={insert_after:?})"
                );
                dev_push_event(
                    &self.window,
                    format!("-> play_next QueueInsertTracks {count} tracks after={insert_after:?}"),
                );
            }
            Err(err) => {
                log::warn!("[QConnect] play_next_batch_on_peer: insert failed: {err}");
            }
        }
        true
    }

    /// True when a PEER renderer currently owns playback (controller mode). Reads
    /// the session under the sync-state lock. Shared by the play-next /
    /// add-to-queue routing entry points.
    async fn is_peer_renderer_active(&self) -> bool {
        let guard = self.inner.lock().await;
        let Some(runtime) = guard.runtime.as_ref() else {
            return false;
        };
        let state = runtime.sync_state.lock().await;
        is_peer_renderer_active(&state.session)
    }

    /// Single-track castability check, mirroring `sync_local_queue_if_changed`'s
    /// all-or-nothing rule: castable = a positive id whose `source` is not
    /// `local` / `plex` (offline `qobuz_download` IS eligible; the default/None
    /// is treated as `qobuz`).
    fn is_track_castable(&self, track_id: u64, source: Option<&str>) -> bool {
        let source = source.unwrap_or("qobuz").to_ascii_lowercase();
        track_id > 0 && source != "local" && source != "plex"
    }

    // -----------------------------------------------------------------------
    // CONTROLLER-mode transport routing (`*_if_remote`). Mirror of the Tauri
    // `src-tauri/src/qconnect/service.rs` adapter. Return contract:
    // `Ok(true)` = handled remotely (do NOT run local), `Ok(false)` = fall back
    // to the local path. The load-bearing safety property: every method begins
    // with `effective_remote_renderer_snapshot()`, which returns `Some` ONLY
    // when a PEER renderer is active (both active+local ids Some AND differ).
    // In every non-controller situation it returns `Ok(false)` and the existing
    // local path runs verbatim. Diagnostics are emitted via log + dev_push_event
    // (no Tauri AppHandle in this crate).
    // -----------------------------------------------------------------------

    /// Send a controller command to the cloud. Mirrors the Tauri
    /// `QconnectServiceState::send_command`, including the pending-transport
    /// clear for superseded `CtrlSrvrSetPlayerState` actions.
    async fn send_command(
        &self,
        command_type: QueueCommandType,
        payload: Value,
    ) -> Result<String, String> {
        let app = {
            let guard = self.inner.lock().await;
            guard
                .runtime
                .as_ref()
                .map(|runtime| Arc::clone(&runtime.app))
                .ok_or_else(|| "QConnect service is not running".to_string())?
        };

        if matches!(command_type, QueueCommandType::CtrlSrvrSetPlayerState) {
            let state_handle = app.state_handle();
            let mut state = state_handle.lock().await;
            let should_clear_transport_pending = state
                .pending
                .current()
                .map(|pending| pending.is_transport_control_action)
                .unwrap_or(false);
            if should_clear_transport_pending {
                log::info!(
                    "[QConnect] Clearing superseded pending transport control before sending next SET_PLAYER_STATE"
                );
                state.pending.clear();
            }
        }

        if matches!(command_type, QueueCommandType::CtrlSrvrSetVolume) {
            // A rapid volume drag fires SetVolume faster than the cloud echoes
            // SrvrCtrlVolumeChanged. Supersede the in-flight volume command
            // (latest-wins) so a drag never spams "pending queue action already
            // active". Mirrors the SetPlayerState supersede above.
            let state_handle = app.state_handle();
            let mut state = state_handle.lock().await;
            let should_clear_volume_pending = state
                .pending
                .current()
                .map(|pending| pending.is_set_volume_action)
                .unwrap_or(false);
            if should_clear_volume_pending {
                log::info!(
                    "[QConnect] Clearing superseded pending volume before sending next SET_VOLUME"
                );
                state.pending.clear();
            }
        }

        // Dev diagnostic: dump the EXACT outbound payload for the player-state
        // command (pause/resume/seek/skip) so a test can diff QBZ's command
        // field-for-field against a working controller (e.g. WebPlayer pausing an
        // iOS renderer). Gated to SetPlayerState so a volume drag never spams it.
        // Answers "do we log who sends what when QBZ != renderer": yes, now.
        if matches!(command_type, QueueCommandType::CtrlSrvrSetPlayerState) {
            log::info!("[QConnect] --> outbound SetPlayerState payload={payload}");
            dev_push_event(&self.window, format!("-> SetPlayerState {payload}"));
        }

        let command = app.build_queue_command(command_type, payload).await;
        app.send_queue_command(command)
            .await
            .map_err(|err| format!("qconnect send command failed: {err}"))
    }

    /// Update the app's cached renderer position (controller optimistic seek).
    async fn update_renderer_position(&self, position_ms: u64) {
        let guard = self.inner.lock().await;
        if let Some(runtime) = &guard.runtime {
            runtime.app.update_renderer_position(position_ms).await;
        }
    }

    /// Best-effort local cursor alignment after a remote handoff so a later
    /// local takeover ("Play here") continues at the right track.
    /// `sync_current_to_id` only moves the queue pointer; it never starts
    /// audible playback. Never fails the handoff.
    async fn align_local_cursor(&self, track_id: u64) {
        if self
            .runtime
            .core()
            .sync_current_to_id(track_id)
            .await
            .is_none()
        {
            log::warn!(
                "[QConnect] cursor align: track {track_id} not found in local queue (best-effort)"
            );
        }
    }

    /// The active renderer's effective snapshot (base local view merged with the
    /// cloud's cached per-renderer state + session loop mode). Returns `None`
    /// when not connected or no active renderer. Mirrors the Tauri
    /// `effective_active_renderer_snapshot`.
    pub(crate) async fn effective_active_renderer_snapshot(
        &self,
    ) -> Result<
        Option<(
            QConnectRendererState,
            QConnectQueueState,
            QconnectSessionState,
        )>,
        String,
    > {
        let (app, sync_state) = {
            let guard = self.inner.lock().await;
            let Some(runtime) = guard.runtime.as_ref() else {
                return Ok(None);
            };
            (Arc::clone(&runtime.app), Arc::clone(&runtime.sync_state))
        };

        let queue = app.queue_state_snapshot().await;
        let base_renderer = app.renderer_state_snapshot().await;
        let state = sync_state.lock().await;
        let session = state.session.clone();
        let Some(active_renderer_id) = session.active_renderer_id else {
            return Ok(None);
        };

        let renderer_state = state
            .session_renderer_states
            .get(&active_renderer_id)
            .cloned();
        let renderer = build_effective_renderer_snapshot(
            &queue,
            &base_renderer,
            renderer_state.as_ref(),
            state.session_loop_mode,
        );

        Ok(Some((renderer, queue, session)))
    }

    /// Like `effective_active_renderer_snapshot` but gated: returns `Some` ONLY
    /// when a PEER renderer is active (controller mode). Mirrors the Tauri
    /// `effective_remote_renderer_snapshot`. This is the gate for all
    /// `*_if_remote` methods.
    pub(crate) async fn effective_remote_renderer_snapshot(
        &self,
    ) -> Result<
        Option<(
            QConnectRendererState,
            QConnectQueueState,
            QconnectSessionState,
        )>,
        String,
    > {
        let Some((renderer, queue, session)) = self.effective_active_renderer_snapshot().await?
        else {
            return Ok(None);
        };

        if !is_peer_renderer_active(&session) {
            return Ok(None);
        }

        Ok(Some((renderer, queue, session)))
    }

    /// True when a PEER renderer currently owns playback (controller mode);
    /// false when not connected or when this device is the active renderer.
    /// Used by the audio-settings force-100 path to SKIP forcing local volume
    /// to 100% while controlling a peer (the bit-perfect lock is lifted then).
    pub async fn is_peer_active(&self) -> bool {
        let guard = self.inner.lock().await;
        let Some(runtime) = guard.runtime.as_ref() else {
            return false;
        };
        let state = runtime.sync_state.lock().await;
        is_peer_renderer_active(&state.session)
    }

    /// Reduced peer-renderer playback snapshot for the now-playing seek bar.
    /// Returns `Some` ONLY when a PEER renderer is active (controller mode); the
    /// poll loop then drives the bar from the peer and skips its local body. When
    /// `None`, the poll loop falls through to the local player path verbatim.
    /// Sources position / updated_at / playing from the effective remote renderer
    /// snapshot (`playing_state == PLAYING`). Title/artist/art come from the
    /// materialized local core queue, so only these three fields are needed here.
    pub async fn remote_now_playing(&self) -> Option<RemoteNowPlaying> {
        let (renderer, queue, _session) =
            self.effective_remote_renderer_snapshot().await.ok().flatten()?;
        Some(RemoteNowPlaying {
            position_ms: renderer.current_position_ms.unwrap_or(0),
            updated_at_ms: renderer.updated_at_ms,
            playing: renderer.playing_state == Some(PLAYING_STATE_PLAYING),
            volume: renderer.volume,
            track_id: renderer
                .current_track
                .as_ref()
                .map(|item| item.track_id)
                .unwrap_or(0),
            // The shuffle BUTTON reflects the cloud-authoritative QUEUE shuffle
            // flag, NOT the per-renderer `renderer.shuffle_mode` — the cloud never
            // populates the per-renderer shuffle field for a peer (it stays None),
            // so reading it lit the button only by luck (worked for one peer type,
            // not the other). `QConnectQueueState.shuffle_mode` is always present.
            // Matches Tauri (queueStore `isShuffle = queueState.shuffle`).
            shuffle_mode: queue.shuffle_mode,
            // QConnect wire loop_mode (1=off, 3=all, 2=one) -> UI repeat-mode
            // (0=off, 1=all, 2=one). Unknown / off -> 0.
            repeat_mode: match renderer.loop_mode {
                Some(3) => 1,
                Some(2) => 2,
                _ => 0,
            },
        })
    }

    /// Optimistically apply a queue_item_id / playing_state / position to the
    /// active peer renderer's cached state, so the UI doesn't bounce back before
    /// the cloud echo. Mirrors the Tauri `prime_remote_renderer_state`.
    async fn prime_remote_renderer_state(
        &self,
        queue_item_id: u64,
        playing_state: Option<i32>,
        current_position_ms: Option<u64>,
    ) {
        let guard = self.inner.lock().await;
        let Some(runtime) = guard.runtime.as_ref() else {
            return;
        };

        let mut sync_state = runtime.sync_state.lock().await;
        let Some(active_renderer_id) = sync_state.session.active_renderer_id else {
            return;
        };
        if sync_state.session.local_renderer_id == Some(active_renderer_id) {
            return;
        }

        let renderer_state = ensure_session_renderer_state(&mut sync_state, active_renderer_id);
        renderer_state.current_queue_item_id = Some(queue_item_id);
        if let Some(playing_state) = playing_state {
            renderer_state.playing_state = Some(playing_state);
        }
        if let Some(current_position_ms) = current_position_ms {
            renderer_state.current_position_ms = Some(current_position_ms);
        }
        renderer_state.updated_at_ms = qconnect_now_ms();
    }

    /// Optimistically apply only a playing_state to the active peer renderer.
    /// Mirrors the Tauri `prime_remote_renderer_playing_state`.
    async fn prime_remote_renderer_playing_state(&self, playing_state: i32) {
        let guard = self.inner.lock().await;
        let Some(runtime) = guard.runtime.as_ref() else {
            return;
        };

        let mut sync_state = runtime.sync_state.lock().await;
        let Some(active_renderer_id) = sync_state.session.active_renderer_id else {
            return;
        };
        if sync_state.session.local_renderer_id == Some(active_renderer_id) {
            return;
        }

        let renderer_state = ensure_session_renderer_state(&mut sync_state, active_renderer_id);
        renderer_state.playing_state = Some(playing_state);
        renderer_state.updated_at_ms = qconnect_now_ms();
    }

    pub async fn skip_next_if_remote(&self) -> Result<bool, String> {
        self.skip_remote_renderer_if_active(QconnectRemoteSkipDirection::Next)
            .await
    }

    pub async fn skip_previous_if_remote(&self) -> Result<bool, String> {
        self.skip_remote_renderer_if_active(QconnectRemoteSkipDirection::Previous)
            .await
    }

    /// Skip the active PEER renderer next/previous. Mirrors the Tauri
    /// `skip_remote_renderer_if_active`.
    async fn skip_remote_renderer_if_active(
        &self,
        direction: QconnectRemoteSkipDirection,
    ) -> Result<bool, String> {
        let direction_label = match direction {
            QconnectRemoteSkipDirection::Next => "next",
            QconnectRemoteSkipDirection::Previous => "previous",
        };

        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((renderer, queue, session)) = remote_context else {
            let reason = {
                let guard = self.inner.lock().await;
                let Some(runtime) = guard.runtime.as_ref() else {
                    return Ok(false);
                };
                let session = runtime.sync_state.lock().await.session.clone();
                if session.active_renderer_id.is_none() {
                    "missing_active_renderer_id"
                } else if session.local_renderer_id.is_none() {
                    "missing_local_renderer_id"
                } else {
                    "active_renderer_is_local"
                }
            };
            log::info!("[QConnect] skip {direction_label} handoff skipped: {reason}");
            dev_push_event(
                &self.window,
                format!("controller skip {direction_label}: local ({reason})"),
            );
            return Ok(false);
        };

        let resolution =
            resolve_controller_queue_item_from_snapshots(&queue, &renderer, direction);

        let Some(target_queue_item_id) = resolution.target_queue_item_id else {
            log::warn!(
                "[QConnect] skip {direction_label} handoff: no target queue item resolved (strategy={})",
                resolution.strategy
            );
            dev_push_event(
                &self.window,
                format!("controller skip {direction_label}: NO TARGET ({})", resolution.strategy),
            );
            return Err(format!(
                "remote renderer active but no {direction_label} target queue item could be resolved"
            ));
        };

        let target_queue_item_id_i32 = i32::try_from(target_queue_item_id)
            .map_err(|_| format!("target queue item id out of range: {target_queue_item_id}"))?;
        let payload = serde_json::to_value(QconnectSetPlayerStateRequest {
            playing_state: renderer.playing_state,
            current_position: Some(0),
            current_queue_item: Some(QconnectSetPlayerStateQueueItemPayload {
                queue_version: Some(QconnectQueueVersionPayload {
                    major: queue.version.major,
                    minor: queue.version.minor,
                }),
                id: Some(target_queue_item_id_i32),
            }),
        })
        .map_err(|err| format!("serialize controller skip payload: {err}"))?;

        self.send_command(QueueCommandType::CtrlSrvrSetPlayerState, payload)
            .await?;
        self.prime_remote_renderer_state(target_queue_item_id, renderer.playing_state, Some(0))
            .await;
        if let Some(target_track_id) = resolution.matched_track_id {
            self.align_local_cursor(target_track_id).await;
        }

        log::info!(
            "[QConnect] skip {direction_label} handoff -> queue_item {target_queue_item_id} (strategy={})",
            resolution.strategy
        );
        dev_push_event(
            &self.window,
            format!("controller skip {direction_label} -> qid {target_queue_item_id} (active={:?})", session.active_renderer_id),
        );

        Ok(true)
    }

    /// Toggle play/pause on the active PEER renderer. Mirrors the Tauri
    /// `toggle_remote_renderer_playback_if_active`.
    pub async fn toggle_remote_renderer_playback_if_active(&self) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((renderer, _queue, session)) = remote_context else {
            let reason = {
                let guard = self.inner.lock().await;
                let Some(runtime) = guard.runtime.as_ref() else {
                    return Ok(false);
                };
                let session = runtime.sync_state.lock().await.session.clone();
                if session.active_renderer_id.is_none() {
                    "missing_active_renderer_id"
                } else if session.local_renderer_id.is_none() {
                    "missing_local_renderer_id"
                } else {
                    "active_renderer_is_local"
                }
            };
            log::info!("[QConnect] toggle_play handoff skipped: {reason}");
            dev_push_event(&self.window, format!("controller toggle_play: local ({reason})"));
            return Ok(false);
        };

        let next_playing_state = match renderer.playing_state {
            Some(PLAYING_STATE_PLAYING) => PLAYING_STATE_PAUSED,
            _ => PLAYING_STATE_PLAYING,
        };
        // BARE play/pause: send ONLY `playing_state` — no `current_position`, no
        // `current_queue_item`. Evidence (controller-of-iOS log 2026-06-05,
        // 23:07:59): iOS ACCEPTS the pause (it reports playing_state=PAUSED) but
        // then AUTO-RESUMES to PLAYING within the same second, with NO play command
        // from QBZ in between — even though the qid (0) and queue_version (12.4) QBZ
        // sent were CORRECT (verified against iOS's own SetState). Attaching a
        // (possibly-stale) `current_position` + a `current_queue_item` makes iOS
        // treat the command as a SEEK / set-state and bounce back to playing; a
        // pure transport toggle needs neither — the renderer pauses/resumes its own
        // current item in place. WebPlayer-as-renderer (verified working) pauses
        // fine on a bare command too, so this does not regress it. (Only remote
        // VOLUME is genuinely refused by iOS.)
        let payload = serde_json::to_value(QconnectSetPlayerStateRequest {
            playing_state: Some(next_playing_state),
            current_position: None,
            current_queue_item: None,
        })
        .map_err(|err| format!("serialize toggle_play request: {err}"))?;

        self.send_command(QueueCommandType::CtrlSrvrSetPlayerState, payload)
            .await?;
        self.prime_remote_renderer_playing_state(next_playing_state)
            .await;

        log::info!("[QConnect] toggle_play handoff -> playing_state {next_playing_state}");
        dev_push_event(
            &self.window,
            format!("controller toggle_play -> {next_playing_state} (active={:?})", session.active_renderer_id),
        );

        Ok(true)
    }

    /// Hand off a "play this track" to the active PEER renderer. Polls the cloud
    /// queue until the track appears, then SetPlayerState to it. Mirrors the
    /// Tauri `play_remote_renderer_track_if_active`.
    pub async fn play_remote_renderer_track_if_active(
        &self,
        track_id: u64,
    ) -> Result<bool, String> {
        let (app, session, sync_state) = {
            let guard = self.inner.lock().await;
            let Some(runtime) = guard.runtime.as_ref() else {
                return Ok(false);
            };
            let session = runtime.sync_state.lock().await.session.clone();
            (
                Arc::clone(&runtime.app),
                session,
                Arc::clone(&runtime.sync_state),
            )
        };

        let active_renderer_id = session.active_renderer_id;
        let local_renderer_id = session.local_renderer_id;
        let early_return_reason = if active_renderer_id.is_none() {
            Some("missing_active_renderer_id")
        } else if local_renderer_id.is_none() {
            Some("missing_local_renderer_id")
        } else if active_renderer_id == local_renderer_id {
            Some("active_renderer_is_local")
        } else {
            None
        };

        if let Some(reason) = early_return_reason {
            if reason == "active_renderer_is_local" {
                let mut state = sync_state.lock().await;
                state.last_load_attempt = Some((track_id, std::time::Instant::now()));
            }
            log::info!("[QConnect] play_track handoff skipped: {reason} (track {track_id})");
            dev_push_event(
                &self.window,
                format!("controller play_track {track_id}: local ({reason})"),
            );
            return Ok(false);
        }

        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_millis(QCONNECT_PLAY_TRACK_HANDOFF_WAIT_MS);
        let poll_interval =
            std::time::Duration::from_millis(QCONNECT_PLAY_TRACK_HANDOFF_POLL_MS);
        let mut attempts: u32 = 0;
        loop {
            attempts += 1;
            let queue = app.queue_state_snapshot().await;

            let (resolved_queue_item_id, _, _) =
                resolve_queue_item_ids_from_queue_state(&queue, track_id);

            if let Some(target_queue_item_id) = resolved_queue_item_id {
                let target_queue_item_id_i32 =
                    i32::try_from(target_queue_item_id).map_err(|_| {
                        format!("target queue item id out of range: {target_queue_item_id}")
                    })?;

                let payload = serde_json::to_value(QconnectSetPlayerStateRequest {
                    playing_state: Some(PLAYING_STATE_PLAYING),
                    current_position: Some(0),
                    current_queue_item: Some(QconnectSetPlayerStateQueueItemPayload {
                        queue_version: Some(QconnectQueueVersionPayload {
                            major: queue.version.major,
                            minor: queue.version.minor,
                        }),
                        id: Some(target_queue_item_id_i32),
                    }),
                })
                .map_err(|err| format!("serialize play_track handoff payload: {err}"))?;

                self.send_command(QueueCommandType::CtrlSrvrSetPlayerState, payload)
                    .await?;
                self.prime_remote_renderer_state(
                    target_queue_item_id,
                    Some(PLAYING_STATE_PLAYING),
                    Some(0),
                )
                .await;
                self.align_local_cursor(track_id).await;

                log::info!(
                    "[QConnect] play_track handoff -> qid {target_queue_item_id} (track {track_id}, attempts={attempts})"
                );
                dev_push_event(
                    &self.window,
                    format!("controller play_track {track_id} -> qid {target_queue_item_id}"),
                );

                return Ok(true);
            }

            if tokio::time::Instant::now() >= deadline {
                break;
            }

            tokio::time::sleep(poll_interval).await;
        }

        log::warn!(
            "[QConnect] play_track handoff: track {track_id} not present in remote queue after {QCONNECT_PLAY_TRACK_HANDOFF_WAIT_MS}ms"
        );
        dev_push_event(
            &self.window,
            format!("controller play_track {track_id}: NOT IN QUEUE (timeout)"),
        );
        Err(format!(
            "remote renderer active but track {track_id} was not present in qconnect queue after {QCONNECT_PLAY_TRACK_HANDOFF_WAIT_MS}ms"
        ))
    }

    /// True whenever a QConnect transport/session is established (renderer OR
    /// controller). Mirrors the Tauri `status().transport_connected` gate that
    /// `v2_toggle_shuffle` / `v2_set_repeat_mode` use: shuffle/repeat are
    /// QUEUE-state operations the cloud OWNS, so they go to the cloud whenever
    /// connected, regardless of who is the active renderer.
    async fn transport_connected(&self) -> bool {
        let app = {
            let guard = self.inner.lock().await;
            match guard.runtime.as_ref() {
                Some(runtime) => Arc::clone(&runtime.app),
                None => return false,
            }
        };
        app.state_handle().lock().await.transport_connected
    }

    /// Toggle shuffle through the CLOUD whenever connected (renderer OR
    /// controller) — exactly like Tauri `v2_toggle_shuffle`, which gates on
    /// `transport_connected`, NOT on a peer being active.
    ///
    /// WS-AUTHORITATIVE (load-bearing): QBZ sends ONLY `{shuffle_mode,
    /// shuffle_seed, shuffle_pivot_queue_item_id}` — never a local order. The
    /// cloud generates the order and echoes it; QBZ applies ONLY that echoed
    /// order (inbound SetShuffleMode is flag-only + materialize applies the
    /// cloud's `shuffled_track_indexes`). The local `playback::toggle_shuffle`
    /// path (which DOES invent a local random order — the documented failure
    /// mode) is reachable ONLY when NOT connected (this returns `Ok(false)` then,
    /// so the caller runs it offline). The previous peer-only gate let that local
    /// path run while connected-as-renderer, which both did nothing visible AND
    /// risked the divergent-order bug.
    pub async fn toggle_shuffle_if_remote(&self) -> Result<bool, String> {
        if !self.transport_connected().await {
            // Offline: caller runs the local shuffle path.
            return Ok(false);
        }
        // UN-gated snapshot: returns Some even when QBZ ITSELF is the active
        // renderer (effective_remote_* returns None then). Mirrors Tauri
        // queue_snapshot()/renderer_snapshot().
        let Some((renderer, queue, session)) = self.effective_active_renderer_snapshot().await?
        else {
            // Connected but no active renderer yet — do NOT fall through to the
            // local reshuffle (it would diverge from the cloud). Handled no-op.
            return Ok(true);
        };

        // Toggle from the cloud-authoritative QUEUE shuffle flag (matches Tauri
        // `!queue.shuffle_mode`), not the per-renderer field which the cloud
        // never populates for a peer.
        let next_shuffle = !queue.shuffle_mode;

        // The cloud REQUIRES a `shuffle_seed` when enabling ("shuffleSeed is
        // undefined" otherwise) and uses it to GENERATE the order — QBZ supplies
        // only the seed + pivot, never an order. No `rand` crate here (unlike
        // Tauri); seed from the wall clock, masked to i32::MAX for the wire
        // `fixed32`. Pivot keeps the current track at the front. Mirrors the
        // Tauri `apply_qconnect_shuffle_mode` payload.
        let shuffle_seed: Option<u32> = next_shuffle.then(|| {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u32)
                .unwrap_or(1);
            nanos & (i32::MAX as u32)
        });
        let pivot_queue_item_id =
            qconnect_app::queue_resolution::resolve_qconnect_shuffle_pivot(&queue, &renderer);

        let payload = json!({
            "shuffle_mode": next_shuffle,
            "shuffle_seed": shuffle_seed.map(i64::from),
            "shuffle_pivot_queue_item_id": pivot_queue_item_id
                .and_then(|value| i32::try_from(value).ok())
                .map(i64::from),
            "autoplay_reset": false,
            "autoplay_loading": false,
        });
        self.send_command(QueueCommandType::CtrlSrvrSetShuffleMode, payload)
            .await?;

        log::info!("[QConnect] shuffle -> {next_shuffle} (cloud, active={:?})", session.active_renderer_id);
        dev_push_event(
            &self.window,
            format!("shuffle -> {next_shuffle} (active={:?})", session.active_renderer_id),
        );

        Ok(true)
    }

    /// Cycle repeat through the CLOUD whenever connected (renderer OR
    /// controller) — like Tauri `v2_set_repeat_mode` (gate = transport_connected).
    /// QConnect loop wire values: 1=off, 3=all, 2=one; cycle off->all->one->off.
    /// Returns `Ok(false)` ONLY when NOT connected so the caller runs local.
    pub async fn cycle_repeat_if_remote(&self) -> Result<bool, String> {
        if !self.transport_connected().await {
            return Ok(false);
        }
        let Some((renderer, _queue, session)) = self.effective_active_renderer_snapshot().await?
        else {
            return Ok(true);
        };

        let current_loop = renderer.loop_mode.unwrap_or(1);
        let next_loop = match current_loop {
            0 | 1 => 3, // off -> all
            3 => 2,     // all -> one
            _ => 1,     // one -> off
        };

        let payload = json!({ "loop_mode": next_loop });
        self.send_command(QueueCommandType::CtrlSrvrSetLoopMode, payload)
            .await?;

        log::info!("[QConnect] repeat -> {next_loop} (cloud, active={:?})", session.active_renderer_id);
        dev_push_event(
            &self.window,
            format!("repeat -> {next_loop} (active={:?})", session.active_renderer_id),
        );

        Ok(true)
    }

    /// Reorder the upcoming queue through the CLOUD whenever connected (renderer
    /// OR controller) — like Tauri `v2_move_queue_track` (gate = transport_connected,
    /// NOT peer-active; queue order is cloud-owned). WS-AUTHORITATIVE: QBZ sends
    /// only `{queue_item_ids:[moved], insert_after}`; the cloud reorders and echoes
    /// a QueueUpdated that materialize applies. The local `move_track` path runs
    /// ONLY when NOT connected (this returns `Ok(false)` then).
    ///
    /// `from_q` / `to_q` are queue-wide UPCOMING indices (0 = first upcoming).
    pub async fn reorder_upcoming_if_remote(
        &self,
        from_q: usize,
        to_q: usize,
    ) -> Result<bool, String> {
        if !self.transport_connected().await {
            return Ok(false);
        }
        // UN-gated snapshot (Some even when QBZ itself is the active renderer),
        // matching toggle_shuffle_if_remote.
        let Some((renderer, queue, session)) = self.effective_active_renderer_snapshot().await?
        else {
            // Connected but no active renderer yet — handled no-op (do NOT fall
            // through to a local reorder that would diverge from the cloud).
            return Ok(true);
        };

        let projection = build_visible_upcoming_projection(&queue, &renderer);
        let len = projection.upcoming_qids.len();
        if len == 0 {
            return Ok(true);
        }
        // Clamp into the projection's [0, len) index space (the core path may pass
        // to_q == len for an append-to-end slot).
        let from_index = from_q.min(len - 1);
        let to_index = to_q.min(len - 1);
        if from_index == to_index {
            return Ok(true);
        }

        let Some(payload) = build_reorder_payload(&projection, from_index, to_index) else {
            return Ok(true); // out of range / nothing to do — handled
        };

        self.send_command(QueueCommandType::CtrlSrvrQueueReorderTracks, payload)
            .await?;

        log::info!(
            "[QConnect] reorder upcoming {from_index} -> {to_index} (cloud, active={:?})",
            session.active_renderer_id
        );
        dev_push_event(
            &self.window,
            format!(
                "reorder {from_index} -> {to_index} (active={:?})",
                session.active_renderer_id
            ),
        );
        Ok(true)
    }

    /// Set volume on the active PEER renderer. Special case: if the renderer
    /// disallows remote volume, return `Ok(true)` (handled no-op) so the
    /// frontend does NOT fall back to local volume. Mirrors the Tauri
    /// `set_volume_if_remote`.
    pub async fn set_volume_if_remote(&self, volume: i32) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((_renderer, _queue, session)) = remote_context else {
            return Ok(false);
        };

        if let Some(active_id) = session.active_renderer_id {
            if let Some(info) = session
                .renderers
                .iter()
                .find(|r| r.renderer_id == active_id)
            {
                if !renderer_allows_remote_volume(info) {
                    log::info!(
                        "[QConnect] set_volume_if_remote short-circuited: renderer {active_id} disallows remote volume"
                    );
                    dev_push_event(
                        &self.window,
                        "controller volume: renderer disallows remote volume (no-op)".to_string(),
                    );
                    return Ok(true);
                }
            }
        }

        let payload = serde_json::to_value(QconnectSetVolumeRequest {
            renderer_id: session.active_renderer_id,
            volume: Some(volume),
            volume_delta: None,
        })
        .map_err(|err| format!("serialize set_volume request: {err}"))?;

        self.send_command(QueueCommandType::CtrlSrvrSetVolume, payload)
            .await?;

        log::info!("[QConnect] set_volume handoff -> {volume}");
        dev_push_event(
            &self.window,
            format!("controller volume -> {volume} (active={:?})", session.active_renderer_id),
        );

        Ok(true)
    }

    /// Mute/unmute the active PEER renderer. Mirrors the Tauri `mute_if_remote`.
    pub async fn mute_if_remote(&self, value: bool) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((_renderer, _queue, session)) = remote_context else {
            return Ok(false);
        };

        let payload = serde_json::to_value(QconnectMuteVolumeRequest {
            renderer_id: session.active_renderer_id,
            value,
        })
        .map_err(|err| format!("serialize mute_volume request: {err}"))?;

        self.send_command(QueueCommandType::CtrlSrvrMuteVolume, payload)
            .await?;

        log::info!("[QConnect] mute handoff -> {value}");
        dev_push_event(
            &self.window,
            format!("controller mute -> {value} (active={:?})", session.active_renderer_id),
        );

        Ok(true)
    }

    /// Set autoplay mode on the active PEER renderer. Mirrors the Tauri
    /// `set_autoplay_mode_if_remote`.
    pub async fn set_autoplay_mode_if_remote(&self, enabled: bool) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((_renderer, _queue, session)) = remote_context else {
            return Ok(false);
        };

        let payload = json!({
            "autoplay_mode": enabled,
            "autoplay_reset": true,
            "autoplay_loading": false
        });
        self.send_command(QueueCommandType::CtrlSrvrSetAutoplayMode, payload)
            .await?;

        log::info!("[QConnect] set_autoplay_mode handoff -> {enabled}");
        dev_push_event(
            &self.window,
            format!("controller autoplay -> {enabled} (active={:?})", session.active_renderer_id),
        );

        Ok(true)
    }

    /// Load autoplay tracks onto the active PEER renderer. Empty list = handled
    /// no-op. Mirrors the Tauri `autoplay_load_tracks_if_remote`.
    pub async fn autoplay_load_tracks_if_remote(
        &self,
        track_ids: Vec<u32>,
    ) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((_renderer, _queue, session)) = remote_context else {
            return Ok(false);
        };

        if track_ids.is_empty() {
            return Ok(true); // nothing to load, but handled remotely
        }

        let track_count = track_ids.len();
        let payload = json!({
            "track_ids": track_ids,
            "context_uuid": Uuid::new_v4().to_string()
        });
        self.send_command(QueueCommandType::CtrlSrvrAutoplayLoadTracks, payload)
            .await?;

        log::info!("[QConnect] autoplay_load_tracks handoff -> {track_count} tracks");
        dev_push_event(
            &self.window,
            format!("controller autoplay_load {track_count} (active={:?})", session.active_renderer_id),
        );

        Ok(true)
    }

    /// Stop the active PEER renderer. Mirrors the Tauri `stop_if_remote`.
    pub async fn stop_if_remote(&self) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((renderer, queue, session)) = remote_context else {
            return Ok(false);
        };

        let current_position = renderer
            .current_position_ms
            .and_then(|value| i32::try_from(value).ok());
        let current_queue_item = renderer.current_track.as_ref().and_then(|item| {
            i32::try_from(item.queue_item_id).ok().map(|queue_item_id| {
                QconnectSetPlayerStateQueueItemPayload {
                    queue_version: Some(QconnectQueueVersionPayload {
                        major: queue.version.major,
                        minor: queue.version.minor,
                    }),
                    id: Some(queue_item_id),
                }
            })
        });

        let payload = serde_json::to_value(QconnectSetPlayerStateRequest {
            playing_state: Some(PLAYING_STATE_STOPPED),
            current_position,
            current_queue_item,
        })
        .map_err(|err| format!("serialize stop request: {err}"))?;

        self.send_command(QueueCommandType::CtrlSrvrSetPlayerState, payload)
            .await?;
        self.prime_remote_renderer_playing_state(PLAYING_STATE_STOPPED)
            .await;

        log::info!("[QConnect] stop handoff");
        dev_push_event(
            &self.window,
            format!("controller stop (active={:?})", session.active_renderer_id),
        );

        Ok(true)
    }

    /// Seek the active PEER renderer to `position_ms`. `playing_state` is not
    /// touched (a seek must not toggle play/pause). Mirrors the Tauri
    /// `set_position_if_remote`.
    pub async fn set_position_if_remote(&self, position_ms: i64) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((renderer, queue, session)) = remote_context else {
            return Ok(false);
        };

        let current_queue_item_id = renderer
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id);

        let request = build_set_position_player_state_request(
            position_ms,
            current_queue_item_id,
            QconnectQueueVersionPayload {
                major: queue.version.major,
                minor: queue.version.minor,
            },
        );
        let payload = serde_json::to_value(request)
            .map_err(|err| format!("serialize set_position request: {err}"))?;

        self.send_command(QueueCommandType::CtrlSrvrSetPlayerState, payload)
            .await?;

        if position_ms >= 0 {
            self.update_renderer_position(position_ms as u64).await;
        }

        log::info!("[QConnect] set_position handoff -> {position_ms}ms");
        dev_push_event(
            &self.window,
            format!("controller seek -> {position_ms}ms (active={:?})", session.active_renderer_id),
        );

        Ok(true)
    }

    /// Switch the active renderer (device picker / "Play here"). Thin wrapper
    /// over `QconnectApp::send_set_active_renderer` (guard + clear-pending).
    /// Mirrors the Tauri `v2_qconnect_set_active_renderer`.
    pub async fn set_active_renderer(&self, renderer_id: i32) -> Result<bool, String> {
        let app = {
            let guard = self.inner.lock().await;
            guard
                .runtime
                .as_ref()
                .map(|runtime| Arc::clone(&runtime.app))
                .ok_or_else(|| "QConnect service is not running".to_string())?
        };
        let handled = app.send_set_active_renderer(renderer_id).await?;
        dev_push_event(
            &self.window,
            format!("controller set_active_renderer -> {renderer_id} (sent={handled})"),
        );
        Ok(handled)
    }
}

/// Slint-side implementation of the shared session-loop seams (piece c). Holds
/// the handles the loop reaches back into: the app (renderer join + state reads),
/// the shared sync accumulator, the service inner (lifecycle gating + teardown),
/// the sink (lifecycle emit), and the runtime (track duration read for the join).
struct SlintSessionLoopHost {
    app: Arc<SlintQconnectApp>,
    sync_state: Arc<Mutex<QconnectRemoteSyncState>>,
    inner: Arc<Mutex<SlintQconnectInner>>,
    sink: Arc<SlintQconnectEventSink>,
    runtime: Runtime,
}

#[async_trait::async_trait]
impl SessionLoopHost for SlintSessionLoopHost {
    async fn update_lifecycle(&self, state: QconnectLifecycleState) {
        update_lifecycle_state_if_running(&self.inner, &self.sink, state).await;
    }

    async fn bootstrap_after_reconnect(&self) {
        // D5 (offline-MODE): never re-bootstrap presence while offline. The
        // force-disconnect watcher is tearing the session down on the offline
        // edge; a transport reconnect that sneaks in before that lands (induced
        // offline keeps the network up) must stay dormant, not re-join.
        if crate::offline_mode::engine().is_offline() {
            log::info!("[QConnect] Reconnect bootstrap suppressed: offline mode active (D5)");
            return;
        }
        if let Err(err) = bootstrap_remote_presence(&self.app, None).await {
            log::error!("[QConnect] Re-bootstrap after reconnect failed: {err}");
        }
    }

    async fn deferred_renderer_join(&self, session_uuid: String, reason: i32) {
        deferred_renderer_join(
            &self.app,
            &self.sync_state,
            &self.runtime,
            &session_uuid,
            reason,
        )
        .await;
    }

    async fn on_reconnect_exhausted(
        &self,
        attempts: u32,
        last_reason: String,
        idle_retry_active: bool,
    ) -> bool {
        {
            let mut guard = self.inner.lock().await;
            guard.lifecycle_state = QconnectLifecycleState::Exhausted;
            guard.last_error = Some(format!(
                "Reconnect attempts exhausted ({attempts}): {last_reason}"
            ));
            if !idle_retry_active {
                // Legacy terminate path: drop the runtime so a fresh connect()
                // succeeds. Dropping it detaches this task's own JoinHandle (fine
                // — the loop breaks right after) (#358).
                guard.runtime = None;
            }
        }
        // TODO(slint-qconnect-ui): surface the Exhausted lifecycle on the badge.
        log::warn!("[QConnect] Reconnect exhausted ({attempts}): {last_reason}");
        !idle_retry_active
    }

    async fn on_loop_error(&self, message: String) {
        // TODO(slint-qconnect-ui): surface as a toast (Tauri emits qconnect:error).
        log::error!("[QConnect] session loop error: {message}");
    }
}

const QCONNECT_RENDERER_CHANNELS: i32 = 2;
const AUDIO_QUALITY_UNKNOWN: i32 = 0;
const AUDIO_QUALITY_MP3: i32 = 1;
const AUDIO_QUALITY_CD: i32 = 2;
const AUDIO_QUALITY_HIRES_L1: i32 = 3;
const AUDIO_QUALITY_HIRES_L2: i32 = 4;
const AUDIO_QUALITY_HIRES_L3: i32 = 5;

/// Classify a (sample_rate, bit_depth) output into the QConnect AudioQuality
/// level. Pure mirror of the Tauri `classify_qconnect_audio_quality`.
fn classify_audio_quality(sample_rate: u32, bit_depth: u32) -> i32 {
    if sample_rate == 0 || bit_depth == 0 {
        AUDIO_QUALITY_UNKNOWN
    } else if sample_rate >= 384_000 {
        AUDIO_QUALITY_HIRES_L3
    } else if sample_rate >= 192_000 {
        AUDIO_QUALITY_HIRES_L2
    } else if bit_depth > 16 || sample_rate > 48_000 {
        AUDIO_QUALITY_HIRES_L1
    } else if sample_rate >= 44_100 {
        AUDIO_QUALITY_CD
    } else {
        AUDIO_QUALITY_MP3
    }
}

/// Build a file-audio-quality snapshot from the live output format, or None when
/// the format isn't known yet. Pure mirror of the Tauri
/// `build_qconnect_file_audio_quality_snapshot`.
fn build_file_audio_quality_snapshot(
    sample_rate: u32,
    bit_depth: u32,
    nb_channels: i32,
) -> Option<QconnectFileAudioQualitySnapshot> {
    if sample_rate == 0 || bit_depth == 0 {
        return None;
    }
    Some(QconnectFileAudioQualitySnapshot {
        sampling_rate: sample_rate as i32,
        bit_depth: bit_depth as i32,
        nb_channels,
        audio_quality: classify_audio_quality(sample_rate, bit_depth),
    })
}

/// Resolve the current + next `queue_item_id` for a playing `track_id` from the
/// cloud queue snapshot, caching the result into the sync accumulator. Mirrors
/// the Tauri `resolve_queue_item_ids_by_track_id`. Used by the renderer report so
/// the controller can map our playback to its queue rows.
async fn resolve_queue_item_ids_by_track_id(
    app: &Arc<SlintQconnectApp>,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    track_id: u64,
) -> (Option<u64>, Option<u64>) {
    let queue = app.queue_state_snapshot().await;
    let (current_qid, next_qid, next_track_id) =
        qconnect_app::queue_resolution::resolve_queue_item_ids_from_queue_state(&queue, track_id);

    if let Some(current_qid) = current_qid {
        let mut state = sync_state.lock().await;
        state.last_renderer_queue_item_id = Some(current_qid);
        state.last_renderer_next_queue_item_id = next_qid;
        state.last_renderer_track_id = Some(track_id);
        state.last_renderer_next_track_id = next_track_id;
        (Some(current_qid), next_qid)
    } else {
        (None, None)
    }
}

/// Controller-side bootstrap: JoinSession (works without a session_uuid) then ask
/// for the current queue state. The renderer-side join is deferred until the
/// server sends SESSION_STATE with a session_uuid (handled in the session loop).
/// Mirrors the Tauri `bootstrap_remote_presence`.
async fn bootstrap_remote_presence(
    app: &Arc<SlintQconnectApp>,
    custom_device_name: Option<String>,
) -> Result<(), String> {
    let device_info = default_qconnect_device_info_with_name(custom_device_name.as_deref());

    let join_payload = serde_json::to_value(QconnectJoinSessionRequest {
        session_uuid: None,
        device_info: Some(device_info),
    })
    .map_err(|err| format!("serialize join_session bootstrap payload: {err}"))?;

    let join_command = app
        .build_queue_command(QueueCommandType::CtrlSrvrJoinSession, join_payload)
        .await;
    let join_action_uuid = app
        .send_queue_command(join_command)
        .await
        .map_err(|err| format!("send bootstrap ctrl_srvr_join_session failed: {err}"))?;
    // JoinSession responds with session/renderer controller events not part of
    // queue reducer correlation. Drop the pending slot so queue ops aren't blocked.
    app.clear_pending_if_matches(&join_action_uuid).await;

    let ask_queue_command = app
        .build_queue_command(QueueCommandType::CtrlSrvrAskForQueueState, json!({}))
        .await;
    let ask_action_uuid = app
        .send_queue_command(ask_queue_command)
        .await
        .map_err(|err| format!("send bootstrap ask_for_queue_state failed: {err}"))?;
    app.clear_pending_if_matches(&ask_action_uuid).await;

    log::info!(
        "[QConnect] Bootstrap complete: controller joined, queue state requested. Renderer join deferred until session_uuid received."
    );
    Ok(())
}

/// Deferred renderer join: called from the session loop when SESSION_STATE with a
/// session_uuid arrives. Idempotent per uuid (P1-8). Mirrors the Tauri
/// `deferred_renderer_join`, reading the current track duration via
/// `runtime.core().get_track` instead of the Tauri CoreBridge.
async fn deferred_renderer_join(
    app: &Arc<SlintQconnectApp>,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    runtime: &Runtime,
    session_uuid: &str,
    join_reason: i32,
) {
    let already_joined = {
        let st = sync_state.lock().await;
        st.last_joined_session_uuid.as_deref() == Some(session_uuid)
    };
    if already_joined {
        log::info!(
            "[QConnect] Deferred join skipped (already joined session_uuid={session_uuid}); re-asking renderer state"
        );
        if let Err(err) = app.ask_for_active_renderer_state().await {
            log::warn!("[QConnect] Idempotent-join AskForRendererState failed: {err}");
        }
        return;
    }

    let device_info = default_qconnect_device_info();
    let queue_version_ref = app.queue_state_snapshot().await.version;

    log::info!("[QConnect] Deferred renderer join with session_uuid={session_uuid}");

    // 1. Renderer JoinSession with session_uuid.
    // Do NOT auto-steal the render on a fresh connect: join as an AVAILABLE
    // renderer (is_active=false), not the active one. Joining with is_active=true
    // on every connect made QBZ grab playback from whatever peer was rendering
    // the instant it came online (the "se robó solo apenas lo encendí" behavior),
    // and the self-state echo from the post-join AskForRendererState then reset
    // the cursor to the queue head. Taking over is now explicit (the device
    // picker's "Play here", or the phone selecting QBZ — both arrive as a
    // SET_ACTIVE command). Only a post-drop RECONNECTION rejoins as active, so a
    // network blip mid-render does not lose the render.
    let join_as_active = join_reason == qconnect_app::JOIN_SESSION_REASON_RECONNECTION;
    let renderer_join_payload = json!({
        "session_uuid": session_uuid,
        "device_info": serde_json::to_value(&device_info).unwrap_or_default(),
        "is_active": join_as_active,
        "reason": join_reason,
        "initial_state": {
            "playing_state": PLAYING_STATE_STOPPED,
            "buffer_state": BUFFER_STATE_OK,
            "current_position": 0,
            "duration": 0,
            "queue_version": {
                "major": queue_version_ref.major,
                "minor": queue_version_ref.minor
            }
        }
    });
    let renderer_join_report = RendererReport::new(
        RendererReportType::RndrSrvrJoinSession,
        Uuid::new_v4().to_string(),
        queue_version_ref,
        renderer_join_payload,
    );
    if let Err(err) = app.send_renderer_report_command(renderer_join_report).await {
        log::error!("[QConnect] Deferred renderer join failed: {err}");
        return;
    }

    // 2. Initial StateUpdated report. At join time (e.g. reconnect mid-playback)
    // we may already have a current track, so resolve the real duration + current/
    // next queue_item_ids instead of hardcoding nulls.
    let renderer = app.renderer_state_snapshot().await;
    let queue = app.queue_state_snapshot().await;
    let current_track_id = renderer.current_track.as_ref().map(|item| item.track_id);
    let (current_qid, next_qid, _) = current_track_id
        .map(|tid| {
            qconnect_app::queue_resolution::resolve_queue_item_ids_from_queue_state(&queue, tid)
        })
        .unwrap_or((None, None, None));
    let duration_secs = match current_track_id {
        Some(track_id) => runtime
            .core()
            .get_track(track_id)
            .await
            .map(|track| u64::from(track.duration))
            .unwrap_or(0),
        None => 0,
    };
    let mut state_report_payload = json!({
        "playing_state": PLAYING_STATE_STOPPED,
        "buffer_state": BUFFER_STATE_OK,
        "current_position": 0,
        "duration": duration_secs,
        "queue_version": {
            "major": queue_version_ref.major,
            "minor": queue_version_ref.minor
        }
    });
    if let Some(qid) = current_qid {
        state_report_payload["current_queue_item_id"] = json!(qid);
    }
    if let Some(qid) = next_qid {
        state_report_payload["next_queue_item_id"] = json!(qid);
    }
    let state_report = RendererReport::new(
        RendererReportType::RndrSrvrStateUpdated,
        Uuid::new_v4().to_string(),
        queue_version_ref,
        state_report_payload,
    );
    if let Err(err) = app.send_renderer_report_command(state_report).await {
        log::error!("[QConnect] Deferred renderer state report failed: {err}");
    }

    // 3. Report volume and max audio quality.
    let volume_report = RendererReport::new(
        RendererReportType::RndrSrvrVolumeChanged,
        Uuid::new_v4().to_string(),
        queue_version_ref,
        json!({ "volume": 100 }),
    );
    if let Err(err) = app.send_renderer_report_command(volume_report).await {
        log::error!("[QConnect] Deferred renderer volume report failed: {err}");
    }

    let max_quality_report = RendererReport::new(
        RendererReportType::RndrSrvrMaxAudioQualityChanged,
        Uuid::new_v4().to_string(),
        queue_version_ref,
        json!({ "max_audio_quality": AUDIO_QUALITY_HIRES_LEVEL2 }),
    );
    if let Err(err) = app.send_renderer_report_command(max_quality_report).await {
        log::error!("[QConnect] Deferred renderer max quality report failed: {err}");
    }

    log::info!("[QConnect] Deferred renderer join complete");

    // Re-request session state so the server sends an updated renderer list
    // (including ourselves). Without this, the UI may not see QBZ as a renderer
    // until the next reconnect cycle.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let refresh_command = app
        .build_queue_command(QueueCommandType::CtrlSrvrAskForQueueState, json!({}))
        .await;
    if let Ok(action_uuid) = app.send_queue_command(refresh_command).await {
        app.clear_pending_if_matches(&action_uuid).await;
        log::info!("[QConnect] Re-requested session state after renderer join");
    }

    // Resync the active renderer's full state too, so a reconnect rejoin restores
    // renderer state.
    if let Err(err) = app.ask_for_active_renderer_state().await {
        log::warn!("[QConnect] Post-join AskForRendererState failed: {err}");
    }

    // Record this session_uuid so a subsequent SESSION_STATE with the same uuid
    // takes the idempotent fast-path above.
    {
        let mut st = sync_state.lock().await;
        st.last_joined_session_uuid = Some(session_uuid.to_string());
    }
}
