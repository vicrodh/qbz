//! `QconnectServiceState` — the public service facade exposed to the
//! Tauri layer. Owns the runtime (transport + event loop), connection
//! lifecycle, command dispatch, snapshot queries, and the controller-
//! handoff helpers used by the `v2_qconnect_*` invokes. Also hosts the
//! transport-event ingestion loop and the deferred renderer-join /
//! pending-action plumbing.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use qconnect_app::{
    QConnectQueueState, QConnectRendererState, QconnectApp, QueueCommandType, RendererReport,
    RendererReportType,
};
use qconnect_transport_ws::{NativeWsTransport, WsTransportConfig};
use serde_json::{json, Value};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::core_bridge::{CoreBridge, CoreBridgeState};

use super::corebridge::align_corebridge_queue_cursor;
use super::event_sink::TauriQconnectEventSink;
use super::queue_resolution::{
    resolve_controller_queue_item_from_snapshots, resolve_queue_item_ids_from_queue_state,
    QconnectRemoteSkipDirection,
};
use super::session::{
    build_effective_renderer_snapshot, build_visible_queue_projection,
    ensure_session_renderer_state, is_local_renderer_active, is_peer_renderer_active,
    QconnectFileAudioQualitySnapshot,
};
use super::transport::{
    default_qconnect_device_info, default_qconnect_device_info_with_name, hex_preview,
    load_persisted_device_name,
};
use super::{
    qconnect_now_ms, QconnectConnectionStatus, QconnectJoinSessionRequest,
    QconnectLifecycleState, QconnectMuteVolumeRequest, QconnectQueueVersionPayload,
    QconnectRemoteSyncState, QconnectSessionState, QconnectSetPlayerStateQueueItemPayload,
    QconnectSetPlayerStateRequest, QconnectSetVolumeRequest, QconnectVisibleQueueProjection,
    AUDIO_QUALITY_HIRES_LEVEL2, BUFFER_STATE_OK, PLAYING_STATE_PAUSED, PLAYING_STATE_PLAYING,
    PLAYING_STATE_STOPPED,
};

const JOIN_SESSION_REASON_CONTROLLER_REQUEST: i32 = 1;
const QCONNECT_PLAY_TRACK_HANDOFF_WAIT_MS: u64 = 1_500;
const QCONNECT_PLAY_TRACK_HANDOFF_POLL_MS: u64 = 50;

struct QconnectRuntime {
    app: Arc<QconnectApp<NativeWsTransport, TauriQconnectEventSink>>,
    config: WsTransportConfig,
    event_loop: JoinHandle<()>,
    sync_state: Arc<Mutex<QconnectRemoteSyncState>>,
}

#[derive(Default)]
struct QconnectServiceInner {
    runtime: Option<QconnectRuntime>,
    last_error: Option<String>,
    /// Lifecycle state — driven by transport events from the spawned event
    /// loop. See `QconnectLifecycleState` (issue #358).
    lifecycle_state: QconnectLifecycleState,
}
fn queue_payload_track_preview(payload: &Value, key: &str) -> Vec<i64> {
    payload
        .get(key)
        .and_then(Value::as_array)
        .map(|tracks| {
            tracks
                .iter()
                .filter_map(|track| track.get("track_id").and_then(Value::as_i64))
                .take(8)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}
async fn update_lifecycle_state_if_running(
    inner: &Arc<Mutex<QconnectServiceInner>>,
    app_handle: &AppHandle,
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
    let serialized = serde_json::to_value(next).unwrap_or_else(|_| json!("unknown"));
    let _ = app_handle.emit(
        "qconnect:status_changed",
        json!({
            "state": serialized,
        }),
    );
}

fn emit_qconnect_diagnostic(app_handle: &AppHandle, channel: &str, level: &str, payload: Value) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    if let Err(err) = app_handle.emit(
        "qconnect:diagnostic",
        json!({
            "ts": ts,
            "channel": channel,
            "level": level,
            "payload": payload,
        }),
    ) {
        log::warn!("[QConnect] Failed to emit diagnostic {channel}: {err}");
    }
}
pub struct QconnectServiceState {
    inner: Arc<Mutex<QconnectServiceInner>>,
    pub(super) custom_device_name: Arc<tokio::sync::RwLock<Option<String>>>,
}

impl QconnectServiceState {
    pub fn new() -> Self {
        // Load persisted device name from disk
        let saved_name = load_persisted_device_name();
        Self {
            inner: Arc::new(Mutex::new(QconnectServiceInner::default())),
            custom_device_name: Arc::new(tokio::sync::RwLock::new(saved_name)),
        }
    }

    pub async fn connect(
        &self,
        app_handle: AppHandle,
        core_bridge: Arc<RwLock<Option<CoreBridge>>>,
        config: WsTransportConfig,
    ) -> Result<QconnectConnectionStatus, String> {
        if config.endpoint_url.trim().is_empty() {
            return Err("QConnect endpoint_url is required".to_string());
        }

        let mut guard = self.inner.lock().await;
        // Idempotent connect: if a runtime is already alive (including stuck in
        // the reconnect loop), don't error — return the current status. The UI
        // toggle reads `running` so the badge stays "on" and clicking it routes
        // to disconnect, which DOES break the loop (issue #358).
        if guard.runtime.is_some() {
            log::info!(
                "[QConnect] connect() called while runtime is alive (state={:?}); returning current status",
                guard.lifecycle_state
            );
            drop(guard);
            return Ok(self.status().await);
        }
        guard.lifecycle_state = QconnectLifecycleState::Connecting;
        guard.last_error = None;

        let transport = Arc::new(NativeWsTransport::new());
        let sync_state = Arc::new(Mutex::new(QconnectRemoteSyncState::default()));
        let sink = Arc::new(TauriQconnectEventSink {
            app_handle: app_handle.clone(),
            core_bridge,
            sync_state: Arc::clone(&sync_state),
        });
        let app = Arc::new(QconnectApp::new(transport, sink));

        if let Err(err) = app.connect(config.clone()).await {
            // Don't leak `lifecycle_state = Connecting` with `runtime = None`
            // back to the frontend — `isQconnectToggleOn` treats `Connecting`
            // as on, so the toggle would stick "on" with no live runtime to
            // disconnect (issue #358).
            let msg = format!("qconnect transport connect failed: {err}");
            guard.lifecycle_state = QconnectLifecycleState::Off;
            guard.last_error = Some(msg.clone());
            return Err(msg);
        }

        let mut transport_rx = app.subscribe_transport_events();
        let app_for_loop = Arc::clone(&app);
        let app_for_errors = app_handle.clone();
        let inner_for_loop = Arc::clone(&self.inner);
        let app_handle_for_status = app_handle.clone();

        let event_loop = tauri::async_runtime::spawn(async move {
            log::info!("[QConnect/EventLoop] Started listening for transport events");
            let mut renderer_joined = false;
            let mut has_disconnected = false;
            loop {
                match transport_rx.recv().await {
                    Ok(event) => {
                        // Check for SESSION_STATE to trigger deferred renderer join
                        if !renderer_joined {
                            if let qconnect_transport_ws::TransportEvent::InboundQueueServerEvent(
                                ref evt,
                            ) = event
                            {
                                if evt.message_type() == "MESSAGE_TYPE_SRVR_CTRL_SESSION_STATE" {
                                    if let Some(session_uuid) =
                                        evt.payload.get("session_uuid").and_then(|v| v.as_str())
                                    {
                                        renderer_joined = true;
                                        deferred_renderer_join(&app_for_loop, session_uuid).await;
                                    } else {
                                        log::warn!("[QConnect] SESSION_STATE received but no session_uuid in payload: {}", evt.payload);
                                    }
                                }
                            }
                        }
                        match &event {
                            qconnect_transport_ws::TransportEvent::Connected => {
                                log::info!("[QConnect/Transport] WebSocket connected");
                            }
                            qconnect_transport_ws::TransportEvent::Disconnected => {
                                log::warn!("[QConnect/Transport] WebSocket disconnected — resetting renderer_joined flag");
                                renderer_joined = false;
                                has_disconnected = true;
                                // Surface "Reconnecting" to the UI, but only if
                                // we're not in a teardown path (Off/Exhausted).
                                // The reconnect loop will keep retrying until
                                // SessionEstablished or MaxReconnectAttemptsExceeded.
                                update_lifecycle_state_if_running(
                                    &inner_for_loop,
                                    &app_handle_for_status,
                                    QconnectLifecycleState::Reconnecting,
                                )
                                .await;
                            }
                            qconnect_transport_ws::TransportEvent::Authenticated => {
                                log::info!("[QConnect/Transport] Authenticated with JWT");
                            }
                            qconnect_transport_ws::TransportEvent::Subscribed => {
                                log::info!("[QConnect/Transport] Subscribed to channels");
                                // Re-bootstrap only after a reconnection (not on initial connect,
                                // where connect() already calls bootstrap_remote_presence).
                                if has_disconnected {
                                    log::info!("[QConnect] Re-bootstrapping after reconnect...");
                                    if let Err(err) = bootstrap_remote_presence(&app_for_loop, None).await {
                                        log::error!("[QConnect] Re-bootstrap after reconnect failed: {err}");
                                    }
                                }
                            }
                            qconnect_transport_ws::TransportEvent::KeepalivePingSent => {
                                log::debug!("[QConnect/Transport] Keepalive ping sent");
                            }
                            qconnect_transport_ws::TransportEvent::KeepalivePongReceived => {
                                log::debug!("[QConnect/Transport] Keepalive pong received");
                            }
                            qconnect_transport_ws::TransportEvent::ReconnectScheduled {
                                attempt,
                                backoff_ms,
                                reason,
                            } => {
                                log::warn!("[QConnect/Transport] Reconnect scheduled: attempt={} backoff={}ms reason={}", attempt, backoff_ms, reason);
                            }
                            qconnect_transport_ws::TransportEvent::InboundQueueServerEvent(evt) => {
                                log::info!(
                                    "[QConnect] <-- Inbound queue event: {} payload={}",
                                    evt.message_type(),
                                    evt.payload
                                );
                                emit_qconnect_diagnostic(
                                    &app_for_errors,
                                    "qconnect:inbound_queue_event",
                                    "info",
                                    json!({
                                        "message_type": evt.message_type(),
                                        "action_uuid": evt.action_uuid.clone(),
                                        "queue_version": evt.queue_version,
                                        "track_count": evt.payload.get("tracks").and_then(|value| value.as_array()).map(|tracks| tracks.len()).unwrap_or(0),
                                        "autoplay_track_count": evt.payload.get("autoplay_tracks").and_then(|value| value.as_array()).map(|tracks| tracks.len()).unwrap_or(0),
                                        "preview_track_ids": queue_payload_track_preview(&evt.payload, "tracks"),
                                        "preview_autoplay_track_ids": queue_payload_track_preview(&evt.payload, "autoplay_tracks"),
                                    }),
                                );
                            }
                            qconnect_transport_ws::TransportEvent::InboundRendererServerCommand(
                                cmd,
                            ) => {
                                log::info!(
                                    "[QConnect] <-- Inbound renderer command: {} payload={}",
                                    cmd.message_type(),
                                    cmd.payload
                                );
                            }
                            qconnect_transport_ws::TransportEvent::InboundFrameDecoded {
                                cloud_message_type,
                                payload_size,
                            } => {
                                log::info!(
                                    "[QConnect/Transport] <-- Frame decoded: cloud_type={} size={}",
                                    cloud_message_type,
                                    payload_size
                                );
                            }
                            qconnect_transport_ws::TransportEvent::InboundPayloadBytes {
                                cloud_message_type,
                                payload,
                            } => {
                                log::info!("[QConnect/Transport] <-- Payload bytes: cloud_type={} len={} hex={}", cloud_message_type, payload.len(), hex_preview(payload, 64));
                            }
                            qconnect_transport_ws::TransportEvent::OutboundSent {
                                message_type,
                                action_uuid,
                            } => {
                                log::info!(
                                    "[QConnect/Transport] --> Outbound sent: {} uuid={}",
                                    message_type,
                                    action_uuid
                                );
                            }
                            qconnect_transport_ws::TransportEvent::TransportError {
                                stage,
                                message,
                            } => {
                                log::error!(
                                    "[QConnect/Transport] Error: stage={} message={}",
                                    stage,
                                    message
                                );
                            }
                            qconnect_transport_ws::TransportEvent::CloudError {
                                msg_id,
                                code,
                                descr,
                            } => {
                                log::warn!(
                                    "[QConnect/Transport] Cloud rejected session: msg_id={} code={} descr={:?} (issue #358)",
                                    msg_id,
                                    code,
                                    descr
                                );
                                emit_qconnect_diagnostic(
                                    &app_for_errors,
                                    "qconnect:cloud_error",
                                    "warning",
                                    json!({
                                        "msg_id": msg_id,
                                        "code": code,
                                        "descr": descr,
                                    }),
                                );
                            }
                            qconnect_transport_ws::TransportEvent::InboundReceived(_envelope) => {
                                log::info!(
                                    "[QConnect/Transport] <-- InboundReceived (JSON envelope)"
                                );
                            }
                            qconnect_transport_ws::TransportEvent::SessionEstablished => {
                                log::info!(
                                    "[QConnect/Transport] Session established — backoff counters reset"
                                );
                                update_lifecycle_state_if_running(
                                    &inner_for_loop,
                                    &app_handle_for_status,
                                    QconnectLifecycleState::Connected,
                                )
                                .await;
                            }
                            qconnect_transport_ws::TransportEvent::MaxReconnectAttemptsExceeded {
                                attempts,
                                last_reason,
                            } => {
                                log::error!(
                                    "[QConnect/Transport] Max reconnect attempts exceeded: attempts={} last_reason={}",
                                    attempts,
                                    last_reason
                                );
                                emit_qconnect_diagnostic(
                                    &app_for_errors,
                                    "qconnect:max_reconnect_attempts_exceeded",
                                    "error",
                                    json!({
                                        "attempts": attempts,
                                        "last_reason": last_reason,
                                    }),
                                );
                                // Auto-stop the runtime: the transport loop
                                // already broke (Exhausted), so no more events
                                // will fire. Take the runtime out so a fresh
                                // user-initiated `connect()` succeeds, mark the
                                // lifecycle as Exhausted with the cloud-side
                                // reason, and exit the event loop. The runtime
                                // we just dropped owns this very task's
                                // JoinHandle — that's fine, dropping a handle
                                // detaches; we then `break` so the task ends
                                // naturally (issue #358).
                                let mut guard = inner_for_loop.lock().await;
                                guard.lifecycle_state = QconnectLifecycleState::Exhausted;
                                guard.last_error = Some(format!(
                                    "Reconnect attempts exhausted ({attempts}): {last_reason}"
                                ));
                                guard.runtime = None;
                                drop(guard);
                                let _ = app_handle_for_status.emit(
                                    "qconnect:status_changed",
                                    json!({
                                        "state": "exhausted",
                                        "reason": "max_reconnect_attempts_exceeded",
                                        "attempts": attempts,
                                        "last_reason": last_reason,
                                    }),
                                );
                                break;
                            }
                        }
                        if let Err(err) = app_for_loop.handle_transport_event(event).await {
                            let message = format!("qconnect app transport handling error: {err}");
                            log::error!("{message}");
                            let _ = app_for_errors.emit("qconnect:error", &message);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        log::warn!("[QConnect] Transport event lagged by {skipped} messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        log::warn!("[QConnect/EventLoop] Transport channel closed, stopping");
                        break;
                    }
                }
            }
            log::info!("[QConnect/EventLoop] Stopped");
        });

        let runtime = QconnectRuntime {
            app,
            config,
            event_loop,
            sync_state,
        };
        let runtime_app = Arc::clone(&runtime.app);
        guard.last_error = None;
        guard.runtime = Some(runtime);

        drop(guard);
        let custom_name = self.custom_device_name.read().await.clone();
        if let Err(err) = bootstrap_remote_presence(&runtime_app, custom_name).await {
            let _ = self.disconnect().await;
            let mut guard = self.inner.lock().await;
            guard.last_error = Some(format!("qconnect bootstrap failed: {err}"));
            return Err(format!("qconnect bootstrap failed: {err}"));
        }

        Ok(self.status().await)
    }

    pub async fn disconnect(&self) -> Result<QconnectConnectionStatus, String> {
        let runtime = {
            let mut guard = self.inner.lock().await;
            // Always force lifecycle to Off — the user-facing requirement is
            // that "disable QConnect" must succeed regardless of whether the
            // backend is Connecting / Reconnecting / Connected / Exhausted
            // (issue #358). The transport's shutdown_tx watch + the runtime
            // event_loop.abort below will tear down any in-flight reconnect.
            guard.lifecycle_state = QconnectLifecycleState::Off;
            guard.runtime.take()
        };

        if let Some(runtime) = runtime {
            if let Err(err) = runtime.app.disconnect().await {
                let mut guard = self.inner.lock().await;
                guard.last_error = Some(format!("qconnect disconnect failed: {err}"));
            }
            runtime.event_loop.abort();
        }

        Ok(self.status().await)
    }

    pub async fn status(&self) -> QconnectConnectionStatus {
        let (app, endpoint_url, last_error, lifecycle_state) = {
            let guard = self.inner.lock().await;
            (
                guard
                    .runtime
                    .as_ref()
                    .map(|runtime| Arc::clone(&runtime.app)),
                guard
                    .runtime
                    .as_ref()
                    .map(|runtime| runtime.config.endpoint_url.clone()),
                guard.last_error.clone(),
                guard.lifecycle_state,
            )
        };

        let transport_connected = if let Some(app) = &app {
            app.state_handle().lock().await.transport_connected
        } else {
            false
        };

        QconnectConnectionStatus {
            running: app.is_some(),
            transport_connected,
            endpoint_url,
            last_error,
            state: lifecycle_state,
        }
    }

    pub async fn send_command(
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

        let command = app.build_queue_command(command_type, payload).await;
        app.send_queue_command(command)
            .await
            .map_err(|err| format!("qconnect send command failed: {err}"))
    }

    pub async fn queue_snapshot(&self) -> Result<QConnectQueueState, String> {
        let app = {
            let guard = self.inner.lock().await;
            guard
                .runtime
                .as_ref()
                .map(|runtime| Arc::clone(&runtime.app))
                .ok_or_else(|| "QConnect service is not running".to_string())?
        };

        Ok(app.queue_state_snapshot().await)
    }

    pub async fn renderer_snapshot(&self) -> Result<QConnectRendererState, String> {
        if let Some((renderer_snapshot, _, _)) = self.effective_active_renderer_snapshot().await? {
            return Ok(renderer_snapshot);
        }

        let app = {
            let guard = self.inner.lock().await;
            guard
                .runtime
                .as_ref()
                .map(|runtime| Arc::clone(&runtime.app))
                .ok_or_else(|| "QConnect service is not running".to_string())?
        };

        Ok(app.renderer_state_snapshot().await)
    }

    pub async fn visible_queue_projection(&self) -> Result<QconnectVisibleQueueProjection, String> {
        if let Some((renderer, queue, _session)) = self.effective_active_renderer_snapshot().await?
        {
            return Ok(build_visible_queue_projection(&queue, &renderer));
        }

        let queue = self.queue_snapshot().await?;
        let renderer = self.renderer_snapshot().await?;
        Ok(build_visible_queue_projection(&queue, &renderer))
    }

    pub async fn session_snapshot(&self) -> Result<QconnectSessionState, String> {
        let sync_state = {
            let guard = self.inner.lock().await;
            guard
                .runtime
                .as_ref()
                .map(|runtime| Arc::clone(&runtime.sync_state))
                .ok_or_else(|| "QConnect service is not running".to_string())?
        };

        let state = sync_state.lock().await;
        Ok(state.session.clone())
    }

    pub async fn skip_next_if_remote(&self, app_handle: &AppHandle) -> Result<bool, String> {
        self.skip_remote_renderer_if_active(QconnectRemoteSkipDirection::Next, app_handle)
            .await
    }

    pub async fn skip_previous_if_remote(&self, app_handle: &AppHandle) -> Result<bool, String> {
        self.skip_remote_renderer_if_active(QconnectRemoteSkipDirection::Previous, app_handle)
            .await
    }

    pub(super) async fn effective_active_renderer_snapshot(
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

    pub(super) async fn effective_remote_renderer_snapshot(
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

    pub(super) async fn prime_remote_renderer_state(
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

    pub(super) async fn prime_remote_renderer_playing_state(&self, playing_state: i32) {
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

    pub async fn send_renderer_report(&self, report: RendererReport) -> Result<(), String> {
        let app = {
            let guard = self.inner.lock().await;
            guard
                .runtime
                .as_ref()
                .map(|runtime| Arc::clone(&runtime.app))
                .ok_or_else(|| "QConnect service is not running".to_string())?
        };

        app.send_renderer_report_command(report)
            .await
            .map_err(|err| format!("send renderer report failed: {err}"))
    }

    pub(super) async fn report_file_audio_quality_if_changed(
        &self,
        queue_version: qconnect_app::QueueVersion,
        audio_quality: QconnectFileAudioQualitySnapshot,
    ) -> Result<bool, String> {
        let (app, sync_state) = {
            let guard = self.inner.lock().await;
            let Some(runtime) = guard.runtime.as_ref() else {
                return Err("QConnect service is not running".to_string());
            };
            (Arc::clone(&runtime.app), Arc::clone(&runtime.sync_state))
        };

        {
            let state = sync_state.lock().await;
            if state.last_reported_file_audio_quality == Some(audio_quality) {
                return Ok(false);
            }
        }

        let report = RendererReport::new(
            RendererReportType::RndrSrvrFileAudioQualityChanged,
            Uuid::new_v4().to_string(),
            queue_version,
            serde_json::json!({
                "sampling_rate": audio_quality.sampling_rate,
                "bit_depth": audio_quality.bit_depth,
                "nb_channels": audio_quality.nb_channels,
                "audio_quality": audio_quality.audio_quality
            }),
        );

        app.send_renderer_report_command(report)
            .await
            .map_err(|err| format!("send file audio quality report failed: {err}"))?;

        let mut state = sync_state.lock().await;
        state.last_reported_file_audio_quality = Some(audio_quality);
        Ok(true)
    }

    /// Update the renderer's internal position from the frontend's actual playback position.
    /// This keeps the QConnect app's renderer state in sync with audio playback so that
    /// subsequent renderer reports (e.g. after pause/resume) include the correct position.
    pub async fn update_renderer_position(&self, position_ms: u64) {
        let guard = self.inner.lock().await;
        if let Some(runtime) = &guard.runtime {
            runtime.app.update_renderer_position(position_ms).await;
        }
    }

    pub async fn is_active(&self) -> bool {
        let guard = self.inner.lock().await;
        guard.runtime.is_some()
    }

    pub(super) async fn get_queue_version(&self) -> qconnect_app::QueueVersion {
        let guard = self.inner.lock().await;
        if let Some(runtime) = &guard.runtime {
            runtime.app.queue_state_snapshot().await.version
        } else {
            qconnect_app::QueueVersion::default()
        }
    }

    /// Get current and next queue_item_ids from the renderer state.
    /// Used to auto-fill state reports when frontend doesn't know queue_item_ids.
    pub(super) async fn get_renderer_queue_item_ids(&self) -> (Option<u64>, Option<u64>) {
        let guard = self.inner.lock().await;
        if let Some(runtime) = &guard.runtime {
            let sync_state = runtime.sync_state.lock().await;
            (
                sync_state.last_renderer_queue_item_id,
                sync_state.last_renderer_next_queue_item_id,
            )
        } else {
            (None, None)
        }
    }

    pub(super) async fn get_renderer_track_ids(&self) -> (Option<u64>, Option<u64>) {
        let guard = self.inner.lock().await;
        if let Some(runtime) = &guard.runtime {
            let sync_state = runtime.sync_state.lock().await;
            (
                sync_state.last_renderer_track_id,
                sync_state.last_renderer_next_track_id,
            )
        } else {
            (None, None)
        }
    }

    pub(super) async fn is_local_renderer_active(&self) -> bool {
        let guard = self.inner.lock().await;
        if let Some(runtime) = &guard.runtime {
            let sync_state = runtime.sync_state.lock().await;
            is_local_renderer_active(&sync_state.session)
        } else {
            false
        }
    }

    /// Resolve the current and next queue_item_ids from the QConnect queue state.
    /// Searches queue_items first, then autoplay_items, and caches the result in sync_state.
    pub(super) async fn resolve_queue_item_ids_by_track_id(
        &self,
        track_id: u64,
    ) -> (Option<u64>, Option<u64>) {
        let (app, sync_state) = {
            let guard = self.inner.lock().await;
            let Some(runtime) = guard.runtime.as_ref() else {
                return (None, None);
            };
            (Arc::clone(&runtime.app), Arc::clone(&runtime.sync_state))
        };

        let queue = app.queue_state_snapshot().await;
        let (current_qid, next_qid, next_track_id) =
            resolve_queue_item_ids_from_queue_state(&queue, track_id);

        if let Some(current_qid) = current_qid {
            let mut state = sync_state.lock().await;
            state.last_renderer_queue_item_id = Some(current_qid);
            state.last_renderer_next_queue_item_id = next_qid;
            state.last_renderer_track_id = Some(track_id);
            state.last_renderer_next_track_id = next_track_id;
            log::debug!(
                "[QConnect] Resolved queue_item_ids current={:?} next={:?} for track_id={} from queue state",
                current_qid,
                next_qid,
                track_id
            );
            (Some(current_qid), next_qid)
        } else {
            log::debug!(
                "[QConnect] Could not find track_id={} in queue state ({} queue_items, {} autoplay_items)",
                track_id,
                queue.queue_items.len(),
                queue.autoplay_items.len()
            );
            (None, None)
        }
    }

    pub(super) async fn skip_remote_renderer_if_active(
        &self,
        direction: QconnectRemoteSkipDirection,
        app_handle: &AppHandle,
    ) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((renderer, queue, session)) = remote_context else {
            let (active_renderer_id, local_renderer_id, renderer_count, reason) = {
                let guard = self.inner.lock().await;
                let Some(runtime) = guard.runtime.as_ref() else {
                    return Ok(false);
                };
                let session = runtime.sync_state.lock().await.session.clone();
                let reason = if session.active_renderer_id.is_none() {
                    "missing_active_renderer_id"
                } else if session.local_renderer_id.is_none() {
                    "missing_local_renderer_id"
                } else {
                    "active_renderer_is_local"
                };
                (
                    session.active_renderer_id,
                    session.local_renderer_id,
                    session.renderers.len(),
                    reason,
                )
            };

            emit_qconnect_diagnostic(
                app_handle,
                "qconnect:controller_skip_handoff",
                "info",
                json!({
                    "direction": match direction {
                        QconnectRemoteSkipDirection::Next => "next",
                        QconnectRemoteSkipDirection::Previous => "previous",
                    },
                    "reason": reason,
                    "active_renderer_id": active_renderer_id,
                    "local_renderer_id": local_renderer_id,
                    "renderer_count": renderer_count,
                }),
            );
            return Ok(false);
        };

        let active_renderer_id = session.active_renderer_id;
        let local_renderer_id = session.local_renderer_id;
        let resolution = resolve_controller_queue_item_from_snapshots(&queue, &renderer, direction);

        let diagnostic_payload = json!({
            "direction": match direction {
                QconnectRemoteSkipDirection::Next => "next",
                QconnectRemoteSkipDirection::Previous => "previous",
            },
            "active_renderer_id": active_renderer_id,
            "local_renderer_id": local_renderer_id,
            "queue_version": {
                "major": queue.version.major,
                "minor": queue.version.minor,
            },
            "current_position_ms": renderer.current_position_ms,
            "playing_state": renderer.playing_state,
            "target_queue_item_id": resolution.target_queue_item_id,
            "strategy": resolution.strategy,
            "queue_index": resolution.queue_index,
            "matched_track_id": resolution.matched_track_id,
            "matched_queue_item_id": resolution.matched_queue_item_id,
        });

        let Some(target_queue_item_id) = resolution.target_queue_item_id else {
            emit_qconnect_diagnostic(
                app_handle,
                "qconnect:controller_skip_handoff",
                "warn",
                diagnostic_payload,
            );
            return Err(format!(
                "remote renderer active but no {} target queue item could be resolved",
                match direction {
                    QconnectRemoteSkipDirection::Next => "next",
                    QconnectRemoteSkipDirection::Previous => "previous",
                }
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
            if let Some(bridge_state) = app_handle.try_state::<CoreBridgeState>() {
                if let Some(bridge) = bridge_state.try_get().await {
                    if let Err(err) = align_corebridge_queue_cursor(&bridge, target_track_id).await
                    {
                        log::warn!(
                            "[QConnect] Failed to align CoreBridge after remote skip handoff: {err}"
                        );
                    }
                }
            }
        }

        emit_qconnect_diagnostic(
            app_handle,
            "qconnect:controller_skip_handoff",
            "info",
            diagnostic_payload,
        );

        Ok(true)
    }

    pub(super) async fn toggle_remote_renderer_playback_if_active(
        &self,
        app_handle: &AppHandle,
    ) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((renderer, queue, session)) = remote_context else {
            let (active_renderer_id, local_renderer_id, renderer_count, reason) = {
                let guard = self.inner.lock().await;
                let Some(runtime) = guard.runtime.as_ref() else {
                    return Ok(false);
                };
                let session = runtime.sync_state.lock().await.session.clone();
                let reason = if session.active_renderer_id.is_none() {
                    "missing_active_renderer_id"
                } else if session.local_renderer_id.is_none() {
                    "missing_local_renderer_id"
                } else {
                    "active_renderer_is_local"
                };
                (
                    session.active_renderer_id,
                    session.local_renderer_id,
                    session.renderers.len(),
                    reason,
                )
            };

            emit_qconnect_diagnostic(
                app_handle,
                "qconnect:toggle_play_handoff",
                "info",
                json!({
                    "reason": reason,
                    "active_renderer_id": active_renderer_id,
                    "local_renderer_id": local_renderer_id,
                    "renderer_count": renderer_count,
                }),
            );
            return Ok(false);
        };

        let active_renderer_id = session.active_renderer_id;
        let local_renderer_id = session.local_renderer_id;
        let next_playing_state = match renderer.playing_state {
            Some(PLAYING_STATE_PLAYING) => PLAYING_STATE_PAUSED,
            _ => PLAYING_STATE_PLAYING,
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
            playing_state: Some(next_playing_state),
            current_position,
            current_queue_item,
        })
        .map_err(|err| format!("serialize toggle_play request: {err}"))?;

        self.send_command(QueueCommandType::CtrlSrvrSetPlayerState, payload)
            .await?;
        self.prime_remote_renderer_playing_state(next_playing_state)
            .await;

        emit_qconnect_diagnostic(
            app_handle,
            "qconnect:toggle_play_handoff",
            "info",
            json!({
                "active_renderer_id": active_renderer_id,
                "local_renderer_id": local_renderer_id,
                "current_playing_state": renderer.playing_state,
                "requested_playing_state": next_playing_state,
                "current_position": current_position,
                "current_queue_item_id": renderer.current_track.as_ref().map(|item| item.queue_item_id),
            }),
        );

        Ok(true)
    }

    pub(super) async fn play_remote_renderer_track_if_active(
        &self,
        track_id: u64,
        app_handle: &AppHandle,
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
            // Mark this track as a recent load attempt when the play is
            // about to happen locally — this prevents the cloud's echo
            // SetState (arriving ~1-2s later) from re-triggering a
            // redundant load while the V2 path is still buffering.
            if reason == "active_renderer_is_local" {
                let mut state = sync_state.lock().await;
                state.last_load_attempt = Some((track_id, std::time::Instant::now()));
            }
            emit_qconnect_diagnostic(
                app_handle,
                "qconnect:play_track_handoff",
                "info",
                json!({
                    "reason": reason,
                    "track_id": track_id,
                    "active_renderer_id": active_renderer_id,
                    "local_renderer_id": local_renderer_id,
                    "renderer_count": session.renderers.len(),
                }),
            );
            return Ok(false);
        }

        let deadline = tokio::time::Instant::now()
            + Duration::from_millis(QCONNECT_PLAY_TRACK_HANDOFF_WAIT_MS);
        let poll_interval = Duration::from_millis(QCONNECT_PLAY_TRACK_HANDOFF_POLL_MS);
        let mut attempts: u32 = 0;
        let (last_queue_version, last_queue_track_count) = loop {
            attempts += 1;
            let queue = app.queue_state_snapshot().await;
            let queue_version = (queue.version.major, queue.version.minor);
            let queue_track_count = queue.queue_items.len() + queue.autoplay_items.len();

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
                if let Some(bridge_state) = app_handle.try_state::<CoreBridgeState>() {
                    if let Some(bridge) = bridge_state.try_get().await {
                        if let Err(err) = align_corebridge_queue_cursor(&bridge, track_id).await {
                            log::warn!(
                                "[QConnect] Failed to align CoreBridge after remote play-track handoff: {err}"
                            );
                        }
                    }
                }

                emit_qconnect_diagnostic(
                    app_handle,
                    "qconnect:play_track_handoff",
                    "info",
                    json!({
                        "track_id": track_id,
                        "active_renderer_id": active_renderer_id,
                        "local_renderer_id": local_renderer_id,
                        "target_queue_item_id": target_queue_item_id,
                        "queue_version": {
                            "major": queue.version.major,
                            "minor": queue.version.minor,
                        },
                        "queue_track_count": queue_track_count,
                        "attempts": attempts,
                        "waited_ms": (attempts.saturating_sub(1) as u64) * QCONNECT_PLAY_TRACK_HANDOFF_POLL_MS,
                    }),
                );

                return Ok(true);
            }

            if tokio::time::Instant::now() >= deadline {
                break (Some(queue_version), queue_track_count);
            }

            tokio::time::sleep(poll_interval).await;
        };

        let renderer = self
            .effective_remote_renderer_snapshot()
            .await?
            .map(|(renderer, _, _)| renderer)
            .unwrap_or_else(QConnectRendererState::default);
        emit_qconnect_diagnostic(
            app_handle,
            "qconnect:play_track_handoff",
            "warn",
            json!({
                "reason": "track_not_present_in_remote_queue",
                "track_id": track_id,
                "active_renderer_id": active_renderer_id,
                "local_renderer_id": local_renderer_id,
                "attempts": attempts,
                "wait_timeout_ms": QCONNECT_PLAY_TRACK_HANDOFF_WAIT_MS,
                "last_queue_version": last_queue_version.map(|(major, minor)| json!({
                    "major": major,
                    "minor": minor,
                })),
                "queue_track_count": last_queue_track_count,
                "renderer_current_track_id": renderer.current_track.as_ref().map(|item| item.track_id),
                "renderer_current_queue_item_id": renderer.current_track.as_ref().map(|item| item.queue_item_id),
                "renderer_next_track_id": renderer.next_track.as_ref().map(|item| item.track_id),
                "renderer_next_queue_item_id": renderer.next_track.as_ref().map(|item| item.queue_item_id),
            }),
        );

        Err(format!(
            "remote renderer active but track {track_id} was not present in qconnect queue after {}ms",
            QCONNECT_PLAY_TRACK_HANDOFF_WAIT_MS
        ))
    }

    pub(super) async fn toggle_shuffle_if_remote(
        &self,
        app_handle: &AppHandle,
    ) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((renderer, _queue, session)) = remote_context else {
            return Ok(false);
        };

        let current_shuffle = renderer.shuffle_mode.unwrap_or(false);
        let next_shuffle = !current_shuffle;

        let payload = json!({ "shuffle_mode": next_shuffle });
        self.send_command(QueueCommandType::CtrlSrvrSetShuffleMode, payload)
            .await?;

        emit_qconnect_diagnostic(
            app_handle,
            "qconnect:toggle_shuffle_handoff",
            "info",
            json!({
                "active_renderer_id": session.active_renderer_id,
                "local_renderer_id": session.local_renderer_id,
                "current_shuffle_mode": current_shuffle,
                "requested_shuffle_mode": next_shuffle,
            }),
        );

        Ok(true)
    }

    pub(super) async fn cycle_repeat_if_remote(
        &self,
        app_handle: &AppHandle,
    ) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((renderer, _queue, session)) = remote_context else {
            return Ok(false);
        };

        // QConnect loop mode: 1 = off, 3 = repeat all, 2 = repeat one
        // Cycle: off(1) → all(3) → one(2) → off(1)
        let current_loop = renderer.loop_mode.unwrap_or(1);
        let next_loop = match current_loop {
            0 | 1 => 3, // off → all
            3 => 2,     // all → one
            _ => 1,     // one → off
        };

        let payload = json!({ "loop_mode": next_loop });
        self.send_command(QueueCommandType::CtrlSrvrSetLoopMode, payload)
            .await?;

        emit_qconnect_diagnostic(
            app_handle,
            "qconnect:cycle_repeat_handoff",
            "info",
            json!({
                "active_renderer_id": session.active_renderer_id,
                "local_renderer_id": session.local_renderer_id,
                "current_loop_mode": current_loop,
                "requested_loop_mode": next_loop,
            }),
        );

        Ok(true)
    }

    pub(super) async fn set_volume_if_remote(
        &self,
        volume: i32,
        app_handle: &AppHandle,
    ) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((_renderer, _queue, session)) = remote_context else {
            return Ok(false);
        };

        let payload = serde_json::to_value(QconnectSetVolumeRequest {
            renderer_id: session.active_renderer_id,
            volume: Some(volume),
            volume_delta: None,
        })
        .map_err(|err| format!("serialize set_volume request: {err}"))?;

        self.send_command(QueueCommandType::CtrlSrvrSetVolume, payload)
            .await?;

        emit_qconnect_diagnostic(
            app_handle,
            "qconnect:set_volume_handoff",
            "info",
            json!({
                "active_renderer_id": session.active_renderer_id,
                "local_renderer_id": session.local_renderer_id,
                "volume": volume,
            }),
        );

        Ok(true)
    }

    pub(super) async fn mute_if_remote(
        &self,
        value: bool,
        app_handle: &AppHandle,
    ) -> Result<bool, String> {
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

        emit_qconnect_diagnostic(
            app_handle,
            "qconnect:mute_handoff",
            "info",
            json!({
                "active_renderer_id": session.active_renderer_id,
                "local_renderer_id": session.local_renderer_id,
                "mute": value,
            }),
        );

        Ok(true)
    }

    pub(super) async fn set_autoplay_mode_if_remote(
        &self,
        enabled: bool,
        app_handle: &AppHandle,
    ) -> Result<bool, String> {
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

        emit_qconnect_diagnostic(
            app_handle,
            "qconnect:set_autoplay_mode_handoff",
            "info",
            json!({
                "active_renderer_id": session.active_renderer_id,
                "local_renderer_id": session.local_renderer_id,
                "autoplay_mode": enabled,
            }),
        );

        Ok(true)
    }

    pub(super) async fn autoplay_load_tracks_if_remote(
        &self,
        track_ids: Vec<u32>,
        app_handle: &AppHandle,
    ) -> Result<bool, String> {
        let remote_context = self.effective_remote_renderer_snapshot().await?;
        let Some((_renderer, _queue, session)) = remote_context else {
            return Ok(false);
        };

        if track_ids.is_empty() {
            return Ok(true); // nothing to load, but handled remotely
        }

        let payload = json!({
            "track_ids": track_ids,
            "context_uuid": uuid::Uuid::new_v4().to_string()
        });
        self.send_command(QueueCommandType::CtrlSrvrAutoplayLoadTracks, payload)
            .await?;

        emit_qconnect_diagnostic(
            app_handle,
            "qconnect:autoplay_load_tracks_handoff",
            "info",
            json!({
                "active_renderer_id": session.active_renderer_id,
                "local_renderer_id": session.local_renderer_id,
                "track_count": track_ids.len(),
            }),
        );

        Ok(true)
    }

    pub(super) async fn stop_if_remote(
        &self,
        app_handle: &AppHandle,
    ) -> Result<bool, String> {
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

        emit_qconnect_diagnostic(
            app_handle,
            "qconnect:stop_handoff",
            "info",
            json!({
                "active_renderer_id": session.active_renderer_id,
                "local_renderer_id": session.local_renderer_id,
                "current_position": current_position,
            }),
        );

        Ok(true)
    }
}


impl Default for QconnectServiceState {
    fn default() -> Self {
        Self::new()
    }
}
async fn bootstrap_remote_presence(
    app: &Arc<QconnectApp<NativeWsTransport, TauriQconnectEventSink>>,
    custom_device_name: Option<String>,
) -> Result<(), String> {
    let device_info = default_qconnect_device_info_with_name(custom_device_name.as_deref());

    // 1. Controller JoinSession first (works without session_uuid).
    //    The server will respond with session topology (AddRenderer, QueueState, etc.).
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

    // JoinSession typically responds with session/renderer controller events that are not part of
    // queue reducer correlation. Drop pending slot so queue operations are not blocked for 10s.
    clear_pending_if_matches(app, &join_action_uuid).await;

    // 2. Ask for current queue state from server
    let ask_queue_payload = serde_json::json!({});
    let ask_queue_command = app
        .build_queue_command(
            QueueCommandType::CtrlSrvrAskForQueueState,
            ask_queue_payload,
        )
        .await;
    let ask_action_uuid = app
        .send_queue_command(ask_queue_command)
        .await
        .map_err(|err| format!("send bootstrap ask_for_queue_state failed: {err}"))?;
    clear_pending_if_matches(app, &ask_action_uuid).await;

    // NOTE: Renderer JoinSession requires a session_uuid from the server (type 81 SESSION_STATE).
    // It is sent as a deferred step from the event loop when SESSION_STATE arrives.
    log::info!("[QConnect] Bootstrap complete: controller joined, queue state requested. Renderer join deferred until session_uuid received.");

    Ok(())
}

/// Deferred renderer join: called from the event loop when we receive SESSION_STATE with a session_uuid.
async fn deferred_renderer_join(
    app: &Arc<QconnectApp<NativeWsTransport, TauriQconnectEventSink>>,
    session_uuid: &str,
) {
    let device_info = default_qconnect_device_info();
    let queue_version_ref = app.queue_state_snapshot().await.version;

    log::info!(
        "[QConnect] Deferred renderer join with session_uuid={}",
        session_uuid
    );

    // 1. Renderer JoinSession with session_uuid
    let renderer_join_payload = serde_json::json!({
        "session_uuid": session_uuid,
        "device_info": serde_json::to_value(&device_info).unwrap_or_default(),
        "is_active": true,
        "reason": JOIN_SESSION_REASON_CONTROLLER_REQUEST,
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

    // 2. Send initial StateUpdated report
    let state_report_payload = serde_json::json!({
        "playing_state": PLAYING_STATE_STOPPED,
        "buffer_state": BUFFER_STATE_OK,
        "current_position": 0,
        "duration": 0,
        "queue_version": {
            "major": queue_version_ref.major,
            "minor": queue_version_ref.minor
        }
    });
    let state_report = RendererReport::new(
        RendererReportType::RndrSrvrStateUpdated,
        Uuid::new_v4().to_string(),
        queue_version_ref,
        state_report_payload,
    );
    if let Err(err) = app.send_renderer_report_command(state_report).await {
        log::error!("[QConnect] Deferred renderer state report failed: {err}");
    }

    // 3. Report volume and max audio quality
    let volume_report = RendererReport::new(
        RendererReportType::RndrSrvrVolumeChanged,
        Uuid::new_v4().to_string(),
        queue_version_ref,
        serde_json::json!({ "volume": 100 }),
    );
    if let Err(err) = app.send_renderer_report_command(volume_report).await {
        log::error!("[QConnect] Deferred renderer volume report failed: {err}");
    }

    let max_quality_report = RendererReport::new(
        RendererReportType::RndrSrvrMaxAudioQualityChanged,
        Uuid::new_v4().to_string(),
        queue_version_ref,
        serde_json::json!({ "max_audio_quality": AUDIO_QUALITY_HIRES_LEVEL2 }),
    );
    if let Err(err) = app.send_renderer_report_command(max_quality_report).await {
        log::error!("[QConnect] Deferred renderer max quality report failed: {err}");
    }

    log::info!("[QConnect] Deferred renderer join complete");

    // Re-request session state so the server sends an updated renderer list
    // (including ourselves). Without this, the frontend may not see QBZ as a
    // renderer until the next reconnect cycle.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let refresh_payload = serde_json::json!({});
    let refresh_command = app
        .build_queue_command(
            QueueCommandType::CtrlSrvrAskForQueueState,
            refresh_payload,
        )
        .await;
    if let Ok(action_uuid) = app.send_queue_command(refresh_command).await {
        clear_pending_if_matches(app, &action_uuid).await;
        log::info!("[QConnect] Re-requested session state after renderer join");
    }
}

async fn clear_pending_if_matches(
    app: &Arc<QconnectApp<NativeWsTransport, TauriQconnectEventSink>>,
    action_uuid: &str,
) {
    let state_handle = app.state_handle();
    let mut state = state_handle.lock().await;
    let pending_matches = state
        .pending
        .current()
        .map(|pending| pending.uuid == action_uuid)
        .unwrap_or(false);
    if pending_matches {
        state.pending.clear();
    }
}
