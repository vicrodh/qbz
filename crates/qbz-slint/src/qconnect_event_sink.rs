//! Slint `QconnectEventSink` (piece b, Phase S).
//!
//! Receives `QconnectAppEvent`s from the qconnect-app crate and dispatches them
//! into the Slint `SlintRendererEngine` (the renderer seam), the shared
//! `QconnectRemoteSyncState` accumulator, and — eventually — the Slint UI.
//!
//! This mirrors the Tauri `src-tauri/src/qconnect/event_sink.rs` arm-for-arm,
//! but routes through the frontend-agnostic `qconnect_app::renderer` orchestration
//! (materialize / apply / loop-mode / cursor-align) and the relocated projection
//! helpers (`build_session_renderer_snapshot` / `cache_renderer_snapshot`) instead
//! of the Tauri `CoreBridge` wrappers. The session-management critical section is
//! delegated to `QconnectApp::apply_session_management_event`; only the post-lock
//! work the returned `SessionApplyOutcome` asks for runs here.
//!
//! UI surfacing (the `is-remote`/badge state, the six toasts, the auto-skip on
//! PlaybackError) is intentionally deferred to the QConnect UI-polish step — those
//! arms log a TODO for now. The renderer-critical arms (QueueUpdated,
//! RendererCommandApplied, SessionManagementEvent, RendererUpdated) are fully
//! wired so this device functions AS a renderer.

use std::sync::{Arc, OnceLock, Weak};

use async_trait::async_trait;
use qbz_app::shell::AppRuntime;
use qconnect_app::{
    build_session_renderer_snapshot, cache_renderer_snapshot, is_peer_renderer_active,
    renderer_allows_remote_volume, QconnectApp, QconnectAppEvent, QconnectEventSink,
    QconnectRemoteSyncState, QconnectRendererEngine, RendererCommand, RendererReport,
    RendererReportType,
};
use qconnect_transport_ws::NativeWsTransport;
use serde_json::Value;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::adapter::SlintAdapter;
use crate::qconnect_engine::SlintRendererEngine;
use crate::qconnect_transport::{resolve_local_identity, BUFFER_STATE_OK};
use crate::AppWindow;

/// Concrete `QconnectApp` type used by the Slint adapter.
pub type SlintQconnectApp = QconnectApp<NativeWsTransport, SlintQconnectEventSink>;

pub struct SlintQconnectEventSink {
    /// Renderer seam — forwards the `qconnect_app::renderer` orchestration onto
    /// `runtime.core()` + the protected player.
    engine: SlintRendererEngine,
    /// Shared runtime — used to refresh the Slint now-playing card + queue
    /// sidebar from the (remotely-mutated) core state after inbound events.
    runtime: Arc<AppRuntime<SlintAdapter>>,
    /// THE shared remote-sync accumulator (one Mutex, shared with `QconnectApp`).
    sync_state: Arc<Mutex<QconnectRemoteSyncState>>,
    /// Late-bound weak handle to the owning app, wired via `set_app` after the
    /// app is built FROM this sink. Used to emit renderer reports (e.g.
    /// is_active=true after SetActive(true)) and to drive the session-apply +
    /// freeze/watchdog without an ownership cycle.
    app: Arc<OnceLock<Weak<SlintQconnectApp>>>,
    /// Window handle for UI surfacing: the DEV modal, the device picker, and the
    /// cast-aware now-playing state (is-remote / cast-target / volume-locked).
    window: slint::Weak<AppWindow>,
    /// FIX #13: previous "a peer is the active renderer" state, tracked across
    /// `apply_session_management_event` calls. On a false->true transition (QBZ
    /// becomes a CONTROLLER) we fire one `ask_for_active_renderer_state` to fetch
    /// the peer's full state (incl. `current_queue_item_id`), so the bar/queue
    /// resolve the peer's CURRENT track immediately instead of staying stale
    /// until the peer changes track. Edge-detected to avoid spamming on every
    /// periodic state-update frame.
    last_peer_active: std::sync::atomic::AtomicBool,
}

impl SlintQconnectEventSink {
    pub fn new(
        engine: SlintRendererEngine,
        runtime: Arc<AppRuntime<SlintAdapter>>,
        sync_state: Arc<Mutex<QconnectRemoteSyncState>>,
        window: slint::Weak<AppWindow>,
    ) -> Self {
        Self {
            engine,
            runtime,
            sync_state,
            app: Arc::new(OnceLock::new()),
            window,
            last_peer_active: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Rebuild the DEV-modal status block (session topology / renderer roles /
    /// queue) from the live sync state + app snapshot, and push it to the modal.
    async fn refresh_dev_status(&self) {
        let Some(app) = self.app.get().and_then(Weak::upgrade) else {
            return;
        };
        let queue = app.queue_state_snapshot().await;
        let status = {
            let st = self.sync_state.lock().await;
            let session = &st.session;
            let role = match (session.active_renderer_id, session.local_renderer_id) {
                (Some(a), Some(l)) if a == l => "renderer (this device active)",
                (Some(_), Some(_)) => "controller (peer active)",
                (Some(_), None) => "joined (local id pending)",
                _ => "no active renderer",
            };
            let renderers = if session.renderers.is_empty() {
                "  (none)".to_string()
            } else {
                session
                    .renderers
                    .iter()
                    .map(|r| {
                        let local = if Some(r.renderer_id) == session.local_renderer_id {
                            " LOCAL"
                        } else {
                            ""
                        };
                        let active = if Some(r.renderer_id) == session.active_renderer_id {
                            " ACTIVE"
                        } else {
                            ""
                        };
                        format!(
                            "  #{} {}{}{}",
                            r.renderer_id,
                            r.friendly_name.clone().unwrap_or_default(),
                            local,
                            active
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            format!(
                "Role: {role}\nsession_uuid: {}\nactive_renderer_id: {:?}   local_renderer_id: {:?}\nqueue: v{}.{}  items={}  autoplay={}\nrenderers:\n{}",
                session
                    .session_uuid
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                session.active_renderer_id,
                session.local_renderer_id,
                queue.version.major,
                queue.version.minor,
                queue.queue_items.len(),
                queue.autoplay_items.len(),
                renderers,
            )
        };
        crate::qconnect_service::dev_set_status(&self.window, status);
    }

    /// Rebuild the QConnect device-picker model from the live session topology
    /// and push it to `QconnectDevState` (devices + active-renderer-id). Mirrors
    /// the Tauri renderer-list source. Maps `session.renderers` -> rows, marking
    /// the local device (`is-local`, rendered as "Play here") and the active one.
    async fn refresh_device_list(&self) {
        let (devices, active_id) = {
            let st = self.sync_state.lock().await;
            let session = &st.session;
            let devices: Vec<crate::QconnectDevice> = session
                .renderers
                .iter()
                .map(|r| crate::QconnectDevice {
                    renderer_id: r.renderer_id,
                    name: r
                        .friendly_name
                        .clone()
                        .unwrap_or_else(|| "Unknown device".to_string())
                        .into(),
                    is_local: Some(r.renderer_id) == session.local_renderer_id,
                    is_active: Some(r.renderer_id) == session.active_renderer_id,
                    icon: device_icon_key(r.device_type, r.friendly_name.as_deref().unwrap_or(""))
                        .into(),
                })
                .collect();
            (devices, session.active_renderer_id.unwrap_or(-1))
        };

        let _ = self.window.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            let dev = w.global::<crate::QconnectDevState>();
            dev.set_devices(slint::ModelRc::new(slint::VecModel::from(devices)));
            dev.set_active_renderer_id(active_id);
        });
    }

    /// Push the cast-aware now-playing state (is-remote / cast-target /
    /// volume-locked) to `NowPlayingState` from the live session topology.
    /// Replaces the old `TODO(slint-qconnect-ui)`. `is-remote` is true when a
    /// PEER renderer owns playback; `cast-target` is its friendly name;
    /// `volume-locked` is true when that renderer disallows remote volume.
    async fn refresh_now_playing_remote_state(&self) {
        // Badge gate: if the transport is down (terminal teardown OR a transient
        // reconnect blip), the renderer/controller badge must read NOT remote. The
        // in-memory session can still name a peer as active_renderer_id long after
        // the session ended (freeze sets playing_state=UNKNOWN but leaves
        // active_renderer_id, and disconnect() only runs on the user toggle, not on
        // transport-drop / reconnect-exhausted). Mirrors the Tauri
        // fetchQconnectRuntimeState early-return on !transport_connected. On
        // reconnect, TransportConnected re-runs this refresh and the repopulated
        // session restores the badge.
        let transport_connected = match self.app.get().and_then(Weak::upgrade) {
            Some(app) => app.state_handle().lock().await.transport_connected,
            None => false,
        };
        let (is_remote, cast_target, volume_locked) = {
            let st = self.sync_state.lock().await;
            let session = &st.session;
            let is_remote = transport_connected && is_peer_renderer_active(session);
            let active = session.active_renderer_id;
            let active_info = active.and_then(|active_id| {
                session.renderers.iter().find(|r| r.renderer_id == active_id)
            });
            let cast_target = if is_remote {
                active_info
                    .and_then(|r| r.friendly_name.clone())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let volume_locked = is_remote
                && active_info
                    .map(|r| !renderer_allows_remote_volume(r))
                    .unwrap_or(false);
            (is_remote, cast_target, volume_locked)
        };

        let _ = self.window.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            let nps = w.global::<crate::NowPlayingState>();
            nps.set_is_remote(is_remote);
            nps.set_cast_target(cast_target.into());
            nps.set_volume_locked(volume_locked);
        });
    }

    /// Refresh the Slint now-playing card + queue sidebar from the current core
    /// state. The inbound renderer orchestration (materialize / apply) mutates
    /// the core player+queue but does NOT touch the UI; without this the card +
    /// queue stay on whatever was loaded at connect time while the audio follows
    /// the remote controller. Reads core `current_track()` so it is authoritative
    /// regardless of the order QueueUpdated / SetState arrive in.
    async fn refresh_local_ui(&self) {
        crate::playback::refresh_now_playing_meta(&self.runtime, &self.window).await;
        crate::playback::refresh_sidebar(true);
        self.refresh_transport_modes().await;
    }

    /// Reflect the cloud-authoritative shuffle/repeat on the bar buttons from the
    /// materialized CORE queue. The inbound SetShuffleMode / QueueUpdated update
    /// the CORE queue (the cloud's order + flags), but never the UI button — this
    /// pushes them. Lightweight (no card/art refresh) so it is safe to call on a
    /// standalone shuffle/loop command without resetting the now-playing card.
    /// Matches Tauri, whose button reads `queue.shuffle` (not a per-renderer
    /// field the cloud never populates for a peer).
    async fn refresh_transport_modes(&self) {
        let qs = self.runtime.core().get_queue_state().await;
        let shuffle_on = qs.shuffle;
        let repeat_mode = match qs.repeat {
            qbz_models::RepeatMode::Off => 0,
            qbz_models::RepeatMode::All => 1,
            qbz_models::RepeatMode::One => 2,
        };
        let _ = self.window.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            let np = w.global::<crate::NowPlayingState>();
            np.set_shuffle(shuffle_on);
            np.set_repeat_mode(repeat_mode);
        });
    }

    /// Wire the owning app after construction. Idempotent (OnceLock).
    pub fn set_app(&self, app: &Arc<SlintQconnectApp>) {
        let _ = self.app.set(Arc::downgrade(app));
    }

    /// Emit a StateUpdated report announcing this renderer is now active. Sent
    /// after SetActive(true) is applied so the controller learns we are ready.
    async fn report_active_renderer_ready(&self) {
        let Some(app) = self.app.get().and_then(Weak::upgrade) else {
            return;
        };
        let queue_version = app.queue_state_snapshot().await.version;
        let report = RendererReport::new(
            RendererReportType::RndrSrvrStateUpdated,
            Uuid::new_v4().to_string(),
            queue_version,
            serde_json::json!({
                "is_active": true,
                "buffer_state": BUFFER_STATE_OK,
                "queue_version": {
                    "major": queue_version.major,
                    "minor": queue_version.minor
                }
            }),
        );
        if let Err(err) = app.send_renderer_report_command(report).await {
            log::warn!("[QConnect] Failed to report active-renderer-ready: {err}");
        }
    }

    /// Apply a server session-management event by delegating the locked critical
    /// section to qconnect-app, then running the post-lock renderer-engine work
    /// the returned `SessionApplyOutcome` asks for. Mirrors the Tauri
    /// `apply_session_management_event`; the post-lock ordering (loop mode ->
    /// local-playback handoff -> projection -> freeze -> watchdog) is identical.
    async fn apply_session_management_event(&self, message_type: &str, payload: &Value) {
        let Some(app) = self.app.get().and_then(Weak::upgrade) else {
            return;
        };
        let identity = resolve_local_identity();
        let outcome = app
            .apply_session_management_event(message_type, payload, &identity)
            .await;

        if let Some(loop_mode) = outcome.apply_loop_mode {
            if let Err(err) = qconnect_app::renderer::apply_remote_loop_mode(&self.engine, loop_mode)
                .await
            {
                log::warn!("[QConnect] Failed to apply remote loop mode: {err}");
            }
        }

        if outcome.sync_local_playback {
            self.sync_local_playback_for_renderer_ownership().await;
        }

        if let Some(renderer_id) = outcome.remote_projection_renderer_id {
            self.sync_active_renderer_projection(renderer_id).await;
        }

        if let Some(renderer_id) = outcome.disconnected_renderer_id {
            app.freeze_active_renderer_projection(
                renderer_id,
                QconnectAppEvent::RendererDisconnected { renderer_id },
            )
            .await;
        }

        if let Some((renderer_id, generation)) = outcome.watchdog_arm {
            app.arm_renderer_watchdog(renderer_id, generation);
        }

        // FIX #13: when QBZ transitions INTO controller mode (a PEER becomes the
        // active renderer), the peer's periodic state-update frames carry
        // `current_queue_item_id: null` (position-only), so on the transition the
        // cursor/projection can't resolve the peer's CURRENT track and the bar/
        // queue stay stale until the peer next changes track. Fetch the peer's
        // FULL state once on the false->true edge so the existing align +
        // projection + poll-loop refresh resolve the real current track now.
        let peer_active_now = {
            let state = self.sync_state.lock().await;
            is_peer_renderer_active(&state.session)
        };
        let was_peer_active = self
            .last_peer_active
            .swap(peer_active_now, std::sync::atomic::Ordering::Relaxed);
        if peer_active_now && !was_peer_active {
            if let Err(err) = app.ask_for_active_renderer_state().await {
                log::warn!(
                    "[QConnect] controller entry: ask_for_active_renderer_state failed: {err}"
                );
            }
        }
    }

    /// When an active PEER renderer now owns playback, stop our local playback so
    /// the two don't double-play. Mirrors the Tauri helper, with the engine seam
    /// in place of `CoreBridge`.
    async fn sync_local_playback_for_renderer_ownership(&self) {
        let peer_renderer_active = {
            let state = self.sync_state.lock().await;
            is_peer_renderer_active(&state.session)
        };
        if !peer_renderer_active {
            return;
        }

        let playback_state = self.engine.get_playback_state();
        if playback_state.track_id == 0 {
            return;
        }

        log::info!(
            "[QConnect] Stopping local playback because active renderer is a peer (track_id={})",
            playback_state.track_id
        );
        if let Err(err) = self.engine.stop() {
            log::warn!("[QConnect] Failed to stop local playback after renderer handoff: {err}");
        }
    }

    /// Refresh the cached projection for the active renderer and, when a peer owns
    /// playback, align the local queue cursor to the peer's current track (so the
    /// controller view + a later takeover land on the right track). Mirrors the
    /// Tauri helper.
    async fn sync_active_renderer_projection(&self, renderer_id: i32) {
        let (queue_state, renderer_state, session_loop_mode, should_align_engine) = {
            let state = self.sync_state.lock().await;
            let Some(active_renderer_id) = state.session.active_renderer_id else {
                return;
            };
            if active_renderer_id != renderer_id {
                return;
            }

            (
                state.last_remote_queue_state.clone(),
                state
                    .session_renderer_states
                    .get(&active_renderer_id)
                    .cloned(),
                state.session_loop_mode,
                state.session.local_renderer_id != Some(active_renderer_id),
            )
        };

        let (Some(queue_state), Some(renderer_state)) = (queue_state, renderer_state) else {
            return;
        };

        let renderer_snapshot =
            build_session_renderer_snapshot(&queue_state, Some(&renderer_state), session_loop_mode);
        {
            let mut state = self.sync_state.lock().await;
            cache_renderer_snapshot(&mut state, &renderer_snapshot);
        }

        if !should_align_engine {
            return;
        }

        let Some(current_track) = renderer_snapshot.current_track.as_ref() else {
            return;
        };

        if let Err(err) =
            qconnect_app::renderer::align_queue_cursor(&self.engine, current_track.track_id).await
        {
            log::warn!("[QConnect] Failed to sync peer renderer cursor into engine: {err}");
        }
    }
}

#[async_trait]
impl QconnectEventSink for SlintQconnectEventSink {
    async fn on_event(&self, event: QconnectAppEvent) {
        match &event {
            QconnectAppEvent::SessionManagementEvent {
                message_type,
                payload,
            } => {
                log::info!(
                    "[QConnect] Session management: {} payload={}",
                    message_type,
                    serde_json::to_string(payload).unwrap_or_else(|_| "?".to_string())
                );
                self.apply_session_management_event(message_type, payload)
                    .await;
            }
            QconnectAppEvent::RendererUpdated(renderer_state) => {
                log::info!(
                    "[QConnect] Renderer updated: playing_state={:?} volume={:?} position={:?}",
                    renderer_state.playing_state,
                    renderer_state.volume,
                    renderer_state.current_position_ms,
                );
                let mut sync_state = self.sync_state.lock().await;
                cache_renderer_snapshot(&mut sync_state, renderer_state);
            }
            QconnectAppEvent::QueueUpdated(queue_state) => {
                log::debug!(
                    "[QConnect] QueueUpdated: items={} shuffle_mode={} version={}.{}",
                    queue_state.queue_items.len(),
                    queue_state.shuffle_mode,
                    queue_state.version.major,
                    queue_state.version.minor,
                );
                {
                    let mut sync_state = self.sync_state.lock().await;
                    sync_state.last_remote_queue_state = Some(queue_state.clone());
                }
                if let Err(err) = qconnect_app::renderer::materialize_remote_queue(
                    &self.engine,
                    &self.sync_state,
                    queue_state,
                )
                .await
                {
                    log::warn!("[QConnect] Failed to materialize remote queue: {err}");
                }
                // Reflect the remote queue change in the QBZ UI (queue sidebar +
                // now-playing card). materialize already set the core queue +
                // cursor; this just pushes it to Slint.
                self.refresh_local_ui().await;
            }
            QconnectAppEvent::RendererCommandApplied { command, state } => {
                log::info!("[QConnect] Renderer command applied: {:?}", command);
                let became_active =
                    matches!(command, RendererCommand::SetActive { active: true });
                if let Err(err) = qconnect_app::renderer::apply_renderer_command(
                    &self.engine,
                    &self.sync_state,
                    command,
                    state,
                )
                .await
                {
                    log::warn!("[QConnect] Failed to apply renderer command: {err}");
                } else if became_active {
                    self.report_active_renderer_ready().await;
                }
                // A SetState changes the current track / play-state — reflect it
                // in the QBZ now-playing card + queue cursor highlight. A
                // standalone SetShuffleMode / SetLoopMode does NOT move the track,
                // so only refresh the lightweight shuffle/repeat button state (a
                // full refresh would reset the now-playing card position/art).
                if matches!(command, RendererCommand::SetState { .. }) {
                    self.refresh_local_ui().await;
                } else if matches!(
                    command,
                    RendererCommand::SetShuffleMode { .. } | RendererCommand::SetLoopMode { .. }
                ) {
                    self.refresh_transport_modes().await;
                }
            }
            QconnectAppEvent::RendererUnreachable { renderer_id } => {
                log::warn!("[QConnect] Renderer {renderer_id} unreachable");
                crate::toast::error_weak(&self.window, "Qobuz Connect renderer unreachable");
            }
            QconnectAppEvent::RendererDisconnected { renderer_id } => {
                log::warn!("[QConnect] Renderer {renderer_id} disconnected");
                crate::toast::error_weak(&self.window, "Qobuz Connect renderer disconnected");
            }
            QconnectAppEvent::PlaybackError {
                queue_item_id,
                error_type,
                ..
            } => {
                // TODO(slint-qconnect-ui): when QBZ is the controller, auto-skip the
                // current item. For now surface the failure.
                log::warn!(
                    "[QConnect] Playback error on queue_item {queue_item_id}: {error_type:?}"
                );
                crate::toast::error_weak(&self.window, "Track unavailable on Qobuz Connect");
            }
            QconnectAppEvent::ResyncComplete => {
                log::info!("[QConnect] Post-reconnect resync complete");
            }
            QconnectAppEvent::LifecycleChanged { state } => {
                // TODO(slint-qconnect-ui): drive the connect badge state.
                log::info!("[QConnect] Lifecycle -> {state:?} (UI badge TODO)");
            }
            QconnectAppEvent::Diagnostic {
                channel, level, ..
            } => {
                log::debug!("[QConnect] diagnostic {channel} [{level}]");
            }
            _ => {}
        }

        // DEV diagnostics: log every event (with a relative timestamp) + refresh
        // the live status block, so the QconnectDevModal reflects QC state at
        // runtime without a rebuild.
        crate::qconnect_service::dev_push_event(&self.window, dev_event_line(&event));
        self.refresh_dev_status().await;

        // Controller-mode UI: rebuild the device picker + push the cast-aware
        // now-playing state (is-remote / cast-target / volume-locked) from the
        // live session topology after every event.
        self.refresh_device_list().await;
        self.refresh_now_playing_remote_state().await;
    }
}

/// Map a renderer's `device_type` (+ a name heuristic for web players) to a
/// device-icon key, mirroring the Tauri `QconnectBadge.resolveDeviceType`:
/// 6 = mobile, 5 = computer (or "web" when the name says web player/browser),
/// anything else (3/4/…) = speaker/receiver.
fn device_icon_key(device_type: Option<i32>, friendly_name: &str) -> &'static str {
    match device_type.unwrap_or(5) {
        6 => "mobile",
        5 => {
            let name = friendly_name.to_ascii_lowercase();
            if name.contains("web player") || name.contains("browser") {
                "web"
            } else {
                "computer"
            }
        }
        _ => "speaker",
    }
}

/// Format a QConnect event into a one-line DEV-log entry. Big payloads
/// (QueueUpdated / SessionManagement) are summarized; the rest use Debug.
fn dev_event_line(event: &QconnectAppEvent) -> String {
    match event {
        QconnectAppEvent::SessionManagementEvent { message_type, .. } => {
            format!("SESSION {message_type}")
        }
        QconnectAppEvent::QueueUpdated(q) => format!(
            "QueueUpdated v{}.{} items={} shuffle={}",
            q.version.major,
            q.version.minor,
            q.queue_items.len(),
            q.shuffle_mode
        ),
        QconnectAppEvent::RendererUpdated(r) => format!(
            "RendererUpdated playing={:?} pos={:?}ms vol={:?}",
            r.playing_state, r.current_position_ms, r.volume
        ),
        QconnectAppEvent::RendererCommandApplied { command, .. } => format!("Cmd {command:?}"),
        QconnectAppEvent::RendererUnreachable { renderer_id } => {
            format!("RendererUnreachable #{renderer_id}")
        }
        QconnectAppEvent::RendererDisconnected { renderer_id } => {
            format!("RendererDisconnected #{renderer_id}")
        }
        QconnectAppEvent::PlaybackError {
            queue_item_id,
            error_type,
            ..
        } => format!("PlaybackError qid={queue_item_id} {error_type:?}"),
        QconnectAppEvent::LifecycleChanged { state } => format!("Lifecycle {state:?}"),
        QconnectAppEvent::ResyncComplete => "ResyncComplete".to_string(),
        QconnectAppEvent::TransportConnected => "TransportConnected".to_string(),
        QconnectAppEvent::TransportDisconnected => "TransportDisconnected".to_string(),
        QconnectAppEvent::Diagnostic { channel, level, .. } => format!("diag {channel} [{level}]"),
        other => format!("{other:?}"),
    }
}
