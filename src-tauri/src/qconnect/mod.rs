mod corebridge;
mod event_sink;
mod queue_resolution;
mod session;
mod track_loading;
mod transport;
mod types;
pub use session::{QconnectRendererInfo, QconnectSessionState};
pub use types::*;
use corebridge::align_corebridge_queue_cursor;
use event_sink::TauriQconnectEventSink;
use transport::{
    default_qconnect_device_info, default_qconnect_device_info_with_name, hex_preview,
    load_persisted_device_name, persist_device_name, resolve_default_qconnect_device_name,
    resolve_qconnect_device_uuid, resolve_system_hostname, resolve_transport_config,
};
use queue_resolution::{
    find_cursor_index_by_track_id, resolve_controller_queue_item_from_snapshots,
    resolve_queue_item_ids_from_queue_state, QconnectRemoteSkipDirection,
};
use session::{
    build_effective_renderer_snapshot, build_visible_queue_projection,
    ensure_session_renderer_state, is_local_renderer_active, is_peer_renderer_active,
    QconnectFileAudioQualitySnapshot, QconnectRendererReportDebugEvent,
    QconnectSessionRendererState,
};

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use qbz_models::{QueueTrack, RepeatMode, Track};
use qconnect_app::{
    evaluate_remote_queue_admission, resolve_handoff_intent, QConnectQueueState,
    QConnectRendererState, QconnectApp, QueueCommandType, RendererReport, RendererReportType,
};
use qconnect_transport_ws::{NativeWsTransport, WsTransportConfig};
use serde_json::{json, Value};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{
    core_bridge::{CoreBridge, CoreBridgeState},
    runtime::{CommandRequirement, RuntimeError, RuntimeManagerState},
    AppState,
};

pub(super) const PLAYING_STATE_UNKNOWN: i32 = 0;
pub(super) const PLAYING_STATE_STOPPED: i32 = 1;
pub(super) const PLAYING_STATE_PLAYING: i32 = 2;
pub(super) const PLAYING_STATE_PAUSED: i32 = 3;
const BUFFER_STATE_OK: i32 = 2;
const QCONNECT_REMOTE_QUEUE_SOURCE: &str = "qobuz_connect_remote";
// AudioQuality enum: 0=unknown, 1=mp3, 2=cd, 3=hires_l1, 4=hires_l2(192k), 5=hires_l3(384k)
const AUDIO_QUALITY_UNKNOWN: i32 = 0;
pub(super) const AUDIO_QUALITY_MP3: i32 = 1;
const AUDIO_QUALITY_CD: i32 = 2;
const AUDIO_QUALITY_HIRES_LEVEL1: i32 = 3;
pub(super) const AUDIO_QUALITY_HIRES_LEVEL2: i32 = 4;
const AUDIO_QUALITY_HIRES_LEVEL3: i32 = 5;
const DEFAULT_QCONNECT_CHANNEL_COUNT: i32 = 2;
// JoinSessionReason: 0=unknown, 1=controller_request, 2=reconnection
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


#[derive(Debug, Default)]
pub(super) struct QconnectRemoteSyncState {
    pub(super) last_renderer_queue_item_id: Option<u64>,
    pub(super) last_renderer_next_queue_item_id: Option<u64>,
    pub(super) last_renderer_track_id: Option<u64>,
    pub(super) last_renderer_next_track_id: Option<u64>,
    pub(super) last_renderer_playing_state: Option<i32>,
    pub(super) last_materialized_start_index: Option<usize>,
    pub(super) last_materialized_core_shuffle_order: Option<Vec<usize>>,
    pub(super) last_reported_file_audio_quality: Option<QconnectFileAudioQualitySnapshot>,
    pub(super) last_applied_queue_state: Option<QConnectQueueState>,
    pub(super) last_remote_queue_state: Option<QConnectQueueState>,
    pub(super) session_loop_mode: Option<i32>,
    /// Session topology — stored from session management events (types 81-87).
    pub(super) session: QconnectSessionState,
    pub(super) session_renderer_states: HashMap<i32, QconnectSessionRendererState>,
    /// Track of the most recent load attempt across paths (V2 play
    /// handoff and ensure_remote_track_loaded). Used to suppress
    /// redundant reloads when an echo SetState arrives during the
    /// in-progress buffer/decode window of a previously triggered load.
    pub(super) last_load_attempt: Option<(u64, std::time::Instant)>,
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

/// Update `QconnectServiceInner::lifecycle_state` from inside the spawned
/// transport-event loop, but only while the runtime is still alive. If
/// `disconnect()` already took the runtime, or `MaxReconnectAttemptsExceeded`
/// already moved us to `Exhausted`, we must not stomp those terminal states
/// with a late `Reconnecting`/`Connected` arriving from the broadcast queue
/// (issue #358).
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


pub(super) fn qconnect_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) fn qconnect_repeat_mode_from_loop_mode(loop_mode: i32) -> Option<RepeatMode> {
    // QConnect protocol loop mode values:
    // 1 = off, 2 = repeat one, 3 = repeat all.
    match loop_mode {
        0 | 1 => Some(RepeatMode::Off),
        2 => Some(RepeatMode::One),
        3 => Some(RepeatMode::All),
        _ => None,
    }
}




pub struct QconnectServiceState {
    inner: Arc<Mutex<QconnectServiceInner>>,
    custom_device_name: Arc<tokio::sync::RwLock<Option<String>>>,
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

    async fn effective_active_renderer_snapshot(
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

    async fn effective_remote_renderer_snapshot(
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

    async fn report_file_audio_quality_if_changed(
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

    async fn get_queue_version(&self) -> qconnect_app::QueueVersion {
        let guard = self.inner.lock().await;
        if let Some(runtime) = &guard.runtime {
            runtime.app.queue_state_snapshot().await.version
        } else {
            qconnect_app::QueueVersion::default()
        }
    }

    /// Get current and next queue_item_ids from the renderer state.
    /// Used to auto-fill state reports when frontend doesn't know queue_item_ids.
    async fn get_renderer_queue_item_ids(&self) -> (Option<u64>, Option<u64>) {
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

    async fn get_renderer_track_ids(&self) -> (Option<u64>, Option<u64>) {
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

    async fn is_local_renderer_active(&self) -> bool {
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
    async fn resolve_queue_item_ids_by_track_id(
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

    async fn skip_remote_renderer_if_active(
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

    async fn toggle_remote_renderer_playback_if_active(
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

    async fn play_remote_renderer_track_if_active(
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

    async fn toggle_shuffle_if_remote(
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

    async fn cycle_repeat_if_remote(
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

    async fn set_volume_if_remote(
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

    async fn mute_if_remote(
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

    async fn set_autoplay_mode_if_remote(
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

    async fn autoplay_load_tracks_if_remote(
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

    async fn stop_if_remote(
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







pub(super) fn model_track_to_core_queue_track(track: &Track) -> QueueTrack {
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|album| album.image.best().cloned());
    let artist = track
        .performer
        .as_ref()
        .map(|performer| performer.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album = track
        .album
        .as_ref()
        .map(|album| album.title.clone())
        .unwrap_or_else(|| "Unknown Album".to_string());
    let album_id = track.album.as_ref().and_then(|album| {
        let trimmed = album.id.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let artist_id = track.performer.as_ref().map(|performer| performer.id);

    QueueTrack {
        id: track.id,
        title: track.title.clone(),
        artist,
        album,
        duration_secs: track.duration as u64,
        artwork_url,
        hires: track.hires,
        bit_depth: track.maximum_bit_depth,
        sample_rate: track.maximum_sampling_rate,
        is_local: false,
        album_id: album_id.clone(),
        artist_id,
        streamable: track.streamable,
        source: Some(QCONNECT_REMOTE_QUEUE_SOURCE.to_string()),
        parental_warning: track.parental_warning,
        source_item_id_hint: album_id,
    }
}

pub(super) fn normalize_volume_to_fraction(volume: i32) -> f32 {
    volume.clamp(0, 100) as f32 / 100.0
}

fn classify_qconnect_audio_quality(sample_rate: u32, bit_depth: u32) -> i32 {
    if sample_rate == 0 || bit_depth == 0 {
        AUDIO_QUALITY_UNKNOWN
    } else if sample_rate >= 384_000 {
        AUDIO_QUALITY_HIRES_LEVEL3
    } else if sample_rate >= 192_000 {
        AUDIO_QUALITY_HIRES_LEVEL2
    } else if bit_depth > 16 || sample_rate > 48_000 {
        AUDIO_QUALITY_HIRES_LEVEL1
    } else if sample_rate >= 44_100 {
        AUDIO_QUALITY_CD
    } else {
        AUDIO_QUALITY_MP3
    }
}

fn build_qconnect_file_audio_quality_snapshot(
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
        audio_quality: classify_qconnect_audio_quality(sample_rate, bit_depth),
    })
}

async fn resolve_active_playback_audio_quality(
    app_handle: &AppHandle,
) -> Option<QconnectFileAudioQualitySnapshot> {
    if let Some(bridge_state) = app_handle.try_state::<CoreBridgeState>() {
        if let Some(bridge) = bridge_state.try_get().await {
            if let Some(snapshot) = build_qconnect_file_audio_quality_snapshot(
                bridge.player().state.get_sample_rate(),
                bridge.player().state.get_bit_depth(),
                DEFAULT_QCONNECT_CHANNEL_COUNT,
            ) {
                return Some(snapshot);
            }
        }
    }

    let app_state = app_handle.try_state::<AppState>()?;
    build_qconnect_file_audio_quality_snapshot(
        app_state.player.state.get_sample_rate(),
        app_state.player.state.get_bit_depth(),
        DEFAULT_QCONNECT_CHANNEL_COUNT,
    )
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


#[tauri::command]
pub async fn v2_qconnect_connect(
    options: Option<QconnectConnectOptions>,
    service: State<'_, QconnectServiceState>,
    core_bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
    app_state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<QconnectConnectionStatus, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresClientInit)
        .await?;

    let config = resolve_transport_config(options.unwrap_or_default(), &app_state)
        .await
        .map_err(RuntimeError::Internal)?;

    service
        .connect(app_handle, core_bridge.0.clone(), config)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_disconnect(
    service: State<'_, QconnectServiceState>,
) -> Result<QconnectConnectionStatus, RuntimeError> {
    service.disconnect().await.map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_status(
    service: State<'_, QconnectServiceState>,
) -> Result<QconnectConnectionStatus, RuntimeError> {
    Ok(service.status().await)
}

#[tauri::command]
pub async fn v2_qconnect_send_command(
    request: QconnectSendCommandRequest,
    service: State<'_, QconnectServiceState>,
) -> Result<String, RuntimeError> {
    service
        .send_command(
            request.command_type.to_queue_command_type(),
            request.payload,
        )
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_evaluate_queue_admission(
    origin: QconnectTrackOrigin,
) -> Result<QconnectAdmissionResult, RuntimeError> {
    log::info!("[QConnect] evaluate_queue_admission: origin={origin:?}");
    let core_origin = origin.into_core_origin();
    let decision = evaluate_remote_queue_admission(core_origin);
    let handoff_intent = resolve_handoff_intent(core_origin);

    log::info!(
        "[QConnect] evaluate_queue_admission: accepted={} reason={}",
        decision.accepted,
        decision.reason
    );

    Ok(QconnectAdmissionResult {
        accepted: decision.accepted,
        reason: decision.reason.to_string(),
        origin,
        handoff_intent: QconnectHandoffIntent::from_core(handoff_intent),
    })
}

#[tauri::command]
pub async fn v2_qconnect_send_command_with_admission(
    request: QconnectSendCommandWithAdmissionRequest,
    service: State<'_, QconnectServiceState>,
    app_handle: AppHandle,
) -> Result<String, RuntimeError> {
    log::info!(
        "[QConnect] send_command_with_admission: type={:?} origin={:?}",
        request.command_type,
        request.origin
    );

    if request.command_type.requires_remote_queue_admission() {
        let core_origin = request.origin.into_core_origin();
        let decision = evaluate_remote_queue_admission(core_origin);
        if !decision.accepted {
            log::warn!(
                "[QConnect] send_command_with_admission: BLOCKED reason={}",
                decision.reason
            );
            let blocked_event = QconnectAdmissionBlockedEvent {
                command_type: request.command_type,
                origin: request.origin,
                reason: decision.reason.to_string(),
                handoff_intent: QconnectHandoffIntent::from_core(resolve_handoff_intent(
                    core_origin,
                )),
            };

            if let Err(err) = app_handle.emit("qconnect:admission_blocked", &blocked_event) {
                log::warn!("[QConnect] Failed to emit admission_blocked event: {err}");
            }

            return Err(RuntimeError::Internal(format!(
                "qconnect admission blocked: {}",
                decision.reason
            )));
        }
        log::info!("[QConnect] send_command_with_admission: admission ACCEPTED");
    }

    match service
        .send_command(
            request.command_type.to_queue_command_type(),
            request.payload,
        )
        .await
    {
        Ok(uuid) => {
            log::info!("[QConnect] send_command_with_admission: sent uuid={}", crate::log_sanitize::mask_uuid(&uuid));
            Ok(uuid)
        }
        Err(err) => {
            log::error!("[QConnect] send_command_with_admission: FAILED err={err}");
            Err(RuntimeError::Internal(err))
        }
    }
}

#[tauri::command]
pub async fn v2_qconnect_join_session(
    request: QconnectJoinSessionRequest,
    service: State<'_, QconnectServiceState>,
) -> Result<String, RuntimeError> {
    let payload = serde_json::to_value(request)
        .map_err(|err| RuntimeError::Internal(format!("serialize join_session request: {err}")))?;
    service
        .send_command(QueueCommandType::CtrlSrvrJoinSession, payload)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_set_player_state(
    request: QconnectSetPlayerStateRequest,
    service: State<'_, QconnectServiceState>,
) -> Result<String, RuntimeError> {
    let payload = serde_json::to_value(request).map_err(|err| {
        RuntimeError::Internal(format!("serialize set_player_state request: {err}"))
    })?;
    service
        .send_command(QueueCommandType::CtrlSrvrSetPlayerState, payload)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_toggle_play_if_remote(
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    service
        .toggle_remote_renderer_playback_if_active(&app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_qconnect_play_track_if_remote(
    trackId: i64,
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    let track_id = u64::try_from(trackId).map_err(|_| {
        RuntimeError::Internal(format!("invalid track id for remote handoff: {trackId}"))
    })?;
    service
        .play_remote_renderer_track_if_active(track_id, &app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_skip_next_if_remote(
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    service
        .skip_next_if_remote(&app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_skip_previous_if_remote(
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    service
        .skip_previous_if_remote(&app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_qconnect_set_volume_if_remote(
    volume: i32,
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    service
        .set_volume_if_remote(volume, &app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_mute_if_remote(
    value: bool,
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    service
        .mute_if_remote(value, &app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_set_autoplay_mode_if_remote(
    enabled: bool,
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    service
        .set_autoplay_mode_if_remote(enabled, &app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_autoplay_load_tracks_if_remote(
    track_ids: Vec<u32>,
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    service
        .autoplay_load_tracks_if_remote(track_ids, &app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_stop_if_remote(
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    service
        .stop_if_remote(&app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_toggle_shuffle_if_remote(
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    service
        .toggle_shuffle_if_remote(&app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_cycle_repeat_if_remote(
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<bool, RuntimeError> {
    service
        .cycle_repeat_if_remote(&app_handle)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_set_active_renderer(
    request: QconnectSetActiveRendererRequest,
    service: State<'_, QconnectServiceState>,
) -> Result<String, RuntimeError> {
    let payload = serde_json::to_value(request).map_err(|err| {
        RuntimeError::Internal(format!("serialize set_active_renderer request: {err}"))
    })?;
    service
        .send_command(QueueCommandType::CtrlSrvrSetActiveRenderer, payload)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_set_volume(
    request: QconnectSetVolumeRequest,
    service: State<'_, QconnectServiceState>,
) -> Result<String, RuntimeError> {
    if request.volume.is_some() && request.volume_delta.is_some() {
        return Err(RuntimeError::Internal(
            "set_volume request must use either 'volume' or 'volume_delta', not both".to_string(),
        ));
    }
    if request.volume.is_none() && request.volume_delta.is_none() {
        return Err(RuntimeError::Internal(
            "set_volume request must provide one of: volume, volume_delta".to_string(),
        ));
    }

    let payload = serde_json::to_value(request)
        .map_err(|err| RuntimeError::Internal(format!("serialize set_volume request: {err}")))?;
    service
        .send_command(QueueCommandType::CtrlSrvrSetVolume, payload)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_set_loop_mode(
    request: QconnectSetLoopModeRequest,
    service: State<'_, QconnectServiceState>,
) -> Result<String, RuntimeError> {
    let payload = serde_json::to_value(request)
        .map_err(|err| RuntimeError::Internal(format!("serialize set_loop_mode request: {err}")))?;
    service
        .send_command(QueueCommandType::CtrlSrvrSetLoopMode, payload)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_mute_volume(
    request: QconnectMuteVolumeRequest,
    service: State<'_, QconnectServiceState>,
) -> Result<String, RuntimeError> {
    let payload = serde_json::to_value(request)
        .map_err(|err| RuntimeError::Internal(format!("serialize mute_volume request: {err}")))?;
    service
        .send_command(QueueCommandType::CtrlSrvrMuteVolume, payload)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_set_max_audio_quality(
    request: QconnectSetMaxAudioQualityRequest,
    service: State<'_, QconnectServiceState>,
) -> Result<String, RuntimeError> {
    let payload = serde_json::to_value(request).map_err(|err| {
        RuntimeError::Internal(format!("serialize set_max_audio_quality request: {err}"))
    })?;
    service
        .send_command(QueueCommandType::CtrlSrvrSetMaxAudioQuality, payload)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_ask_for_renderer_state(
    request: QconnectAskForRendererStateRequest,
    service: State<'_, QconnectServiceState>,
) -> Result<String, RuntimeError> {
    let payload = serde_json::to_value(request).map_err(|err| {
        RuntimeError::Internal(format!("serialize ask_for_renderer_state request: {err}"))
    })?;
    service
        .send_command(QueueCommandType::CtrlSrvrAskForRendererState, payload)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_queue_snapshot(
    service: State<'_, QconnectServiceState>,
) -> Result<QConnectQueueState, RuntimeError> {
    service
        .queue_snapshot()
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_renderer_snapshot(
    service: State<'_, QconnectServiceState>,
) -> Result<QConnectRendererState, RuntimeError> {
    service
        .renderer_snapshot()
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub async fn v2_qconnect_session_snapshot(
    service: State<'_, QconnectServiceState>,
) -> Result<QconnectSessionState, RuntimeError> {
    service
        .session_snapshot()
        .await
        .map_err(RuntimeError::Internal)
}

/// Report current playback state to QConnect server.
/// Called by frontend on state transitions (play/pause, track change) and periodic position updates.
/// Auto-fills queue_item_ids from renderer state when the frontend passes null.
/// If renderer state has no queue_item_id, falls back to looking up by current_track_id
/// in the QConnect queue state.
/// Fire-and-forget: errors are logged but do not block playback.
#[tauri::command]
pub async fn v2_qconnect_report_playback_state(
    playing_state: i32,
    current_position: Option<i32>,
    duration: Option<i32>,
    current_queue_item_id: Option<i32>,
    next_queue_item_id: Option<i32>,
    current_track_id: Option<i64>,
    app_handle: AppHandle,
    service: State<'_, QconnectServiceState>,
) -> Result<(), RuntimeError> {
    if !service.is_active().await {
        return Ok(());
    }

    let requested_current_qid = current_queue_item_id;
    let requested_next_qid = next_queue_item_id;
    let mut resolution_strategy = if current_queue_item_id.is_some() {
        "frontend_provided".to_string()
    } else {
        "renderer_snapshot".to_string()
    };
    let (renderer_current_track_id, _renderer_next_track_id) =
        service.get_renderer_track_ids().await;
    let current_track_id_u64 = current_track_id
        .filter(|track_id| *track_id > 0)
        .map(|track_id| track_id as u64);

    // Auto-fill queue_item_ids from renderer state if not provided by frontend.
    // The frontend doesn't know about QConnect queue_item_ids, but the renderer
    // state tracks them from server SET_STATE commands.
    let (mut resolved_current_qid, resolved_next_qid) = if current_queue_item_id.is_some() {
        (current_queue_item_id, next_queue_item_id)
    } else {
        let (renderer_current, renderer_next) = service.get_renderer_queue_item_ids().await;
        (
            renderer_current.and_then(|id| i32::try_from(id).ok()),
            renderer_next.and_then(|id| i32::try_from(id).ok()),
        )
    };
    let mut resolved_next_qid = resolved_next_qid;

    // Prefer a fresh queue lookup whenever the local track differs from the
    // cached renderer snapshot, or when the queue-derived cursor diverges
    // from the stale renderer cursor after a queue mutation (insert/reorder).
    let mut queue_lookup_report_strategy: Option<&'static str> = None;
    if let Some(track_id) = current_track_id_u64 {
        let (queue_current_qid, queue_next_qid) =
            service.resolve_queue_item_ids_by_track_id(track_id).await;
        let queue_current_qid_i32 = queue_current_qid.and_then(|qid| i32::try_from(qid).ok());
        let queue_next_qid_i32 = queue_next_qid.and_then(|next_qid| i32::try_from(next_qid).ok());

        queue_lookup_report_strategy = determine_queue_lookup_report_strategy(
            requested_current_qid,
            Some(track_id),
            renderer_current_track_id,
            resolved_current_qid,
            resolved_next_qid,
            queue_current_qid_i32,
            queue_next_qid_i32,
        );

        if let Some(strategy) = queue_lookup_report_strategy {
            resolved_current_qid = queue_current_qid_i32;
            if requested_next_qid.is_none() {
                resolved_next_qid = queue_next_qid_i32;
            }
            resolution_strategy = strategy.to_string();
        }
    }

    if resolved_current_qid.is_none() && requested_current_qid.is_none() {
        resolution_strategy = "unresolved".to_string();
    }

    let queue_version = service.get_queue_version().await;
    let should_report_queue_item_ids = should_report_queue_item_ids_for_renderer_state(
        requested_current_qid,
        queue_lookup_report_strategy,
        service.is_local_renderer_active().await,
        resolved_current_qid,
    );

    let should_skip_due_to_stale_renderer = should_skip_renderer_report_due_to_stale_snapshot(
        current_track_id,
        requested_current_qid,
        resolved_current_qid,
        renderer_current_track_id,
    );

    if should_skip_due_to_stale_renderer {
        resolution_strategy = "suppressed_stale_renderer_snapshot_mismatch".to_string();
        if let Err(err) = app_handle.emit(
            "qconnect:renderer_report_debug",
            &QconnectRendererReportDebugEvent {
                requested_current_queue_item_id: requested_current_qid,
                requested_next_queue_item_id: requested_next_qid,
                resolved_current_queue_item_id: resolved_current_qid,
                resolved_next_queue_item_id: resolved_next_qid,
                sent_current_queue_item_id: None,
                sent_next_queue_item_id: None,
                report_queue_item_ids: should_report_queue_item_ids,
                current_track_id,
                playing_state,
                current_position,
                duration,
                queue_version: QconnectQueueVersionPayload {
                    major: queue_version.major,
                    minor: queue_version.minor,
                },
                resolution_strategy,
            },
        ) {
            log::debug!("[QConnect] Failed to emit stale renderer report debug event: {err}");
        }

        if let Some(pos) = current_position {
            if pos >= 0 {
                service.update_renderer_position(pos as u64).await;
            }
        }

        return Ok(());
    }

    log::debug!(
        "[QConnect/Report] Periodic state report: playing={} pos={:?} dur={:?} qid={:?} next_qid={:?} track_id={:?} qv={}.{}",
        playing_state, current_position, duration,
        resolved_current_qid, resolved_next_qid, current_track_id,
        queue_version.major, queue_version.minor
    );

    let sent_current_qid = if should_report_queue_item_ids {
        resolved_current_qid
    } else {
        None
    };
    let sent_next_qid = if should_report_queue_item_ids {
        resolved_next_qid
    } else {
        None
    };

    // Keep periodic interval reports conservative, but allow transition reports
    // to carry queue_item_ids once they are re-resolved from the current track.
    let report = RendererReport::new(
        RendererReportType::RndrSrvrStateUpdated,
        Uuid::new_v4().to_string(),
        queue_version,
        serde_json::json!({
            "playing_state": playing_state,
            "buffer_state": BUFFER_STATE_OK,
            "current_position": current_position,
            "duration": duration,
            "current_queue_item_id": sent_current_qid,
            "next_queue_item_id": sent_next_qid,
            "queue_version": {
                "major": queue_version.major,
                "minor": queue_version.minor
            }
        }),
    );

    if let Err(err) = service.send_renderer_report(report).await {
        log::warn!("[QConnect] Failed to report playback state: {err}");
    }

    if let Some(audio_quality) = resolve_active_playback_audio_quality(&app_handle).await {
        if let Err(err) = service
            .report_file_audio_quality_if_changed(queue_version, audio_quality)
            .await
        {
            log::warn!("[QConnect] Failed to report file audio quality: {err}");
        }
    }

    if let Err(err) = app_handle.emit(
        "qconnect:renderer_report_debug",
        &QconnectRendererReportDebugEvent {
            requested_current_queue_item_id: requested_current_qid,
            requested_next_queue_item_id: requested_next_qid,
            resolved_current_queue_item_id: resolved_current_qid,
            resolved_next_queue_item_id: resolved_next_qid,
            sent_current_queue_item_id: sent_current_qid,
            sent_next_queue_item_id: sent_next_qid,
            report_queue_item_ids: should_report_queue_item_ids,
            current_track_id,
            playing_state,
            current_position,
            duration,
            queue_version: QconnectQueueVersionPayload {
                major: queue_version.major,
                minor: queue_version.minor,
            },
            resolution_strategy,
        },
    ) {
        log::debug!("[QConnect] Failed to emit renderer report debug event: {err}");
    }

    // Keep the QConnect app's renderer position in sync with the actual playback position.
    // This ensures renderer reports triggered by server commands (pause/resume/next)
    // include the real position instead of a stale value.
    if let Some(pos) = current_position {
        if pos >= 0 {
            service.update_renderer_position(pos as u64).await;
        }
    }

    Ok(())
}

fn should_skip_renderer_report_due_to_stale_snapshot(
    current_track_id: Option<i64>,
    requested_current_qid: Option<i32>,
    resolved_current_qid: Option<i32>,
    renderer_current_track_id: Option<u64>,
) -> bool {
    if requested_current_qid.is_some() || resolved_current_qid.is_some() {
        return false;
    }

    let Some(local_track_id) = current_track_id.filter(|track_id| *track_id > 0) else {
        return false;
    };

    let Some(renderer_track_id) = renderer_current_track_id else {
        return false;
    };

    renderer_track_id != local_track_id as u64
}

fn determine_queue_lookup_report_strategy(
    requested_current_qid: Option<i32>,
    current_track_id: Option<u64>,
    renderer_current_track_id: Option<u64>,
    renderer_current_qid: Option<i32>,
    renderer_next_qid: Option<i32>,
    queue_current_qid: Option<i32>,
    queue_next_qid: Option<i32>,
) -> Option<&'static str> {
    if requested_current_qid.is_some() {
        return None;
    }

    let Some(track_id) = current_track_id else {
        return None;
    };
    let Some(queue_current_qid) = queue_current_qid else {
        return None;
    };

    if renderer_current_track_id != Some(track_id) {
        return Some("queue_lookup_track_transition");
    }

    if renderer_current_qid != Some(queue_current_qid) || renderer_next_qid != queue_next_qid {
        return Some("queue_lookup_queue_drift");
    }

    None
}

fn should_report_queue_item_ids_for_renderer_state(
    requested_current_qid: Option<i32>,
    queue_lookup_report_strategy: Option<&'static str>,
    local_renderer_active: bool,
    resolved_current_qid: Option<i32>,
) -> bool {
    requested_current_qid.is_some()
        || queue_lookup_report_strategy.is_some()
        || (local_renderer_active && resolved_current_qid.is_some())
}

/// Report volume change to QConnect server.
#[tauri::command]
pub async fn v2_qconnect_report_volume(
    volume: i32,
    service: State<'_, QconnectServiceState>,
) -> Result<(), RuntimeError> {
    if !service.is_active().await {
        return Ok(());
    }

    let queue_version = service.get_queue_version().await;
    let report = RendererReport::new(
        RendererReportType::RndrSrvrVolumeChanged,
        Uuid::new_v4().to_string(),
        queue_version,
        serde_json::json!({ "volume": volume }),
    );

    if let Err(err) = service.send_renderer_report(report).await {
        log::warn!("[QConnect] Failed to report volume change: {err}");
    }

    Ok(())
}

#[tauri::command]
pub async fn v2_qconnect_get_device_name(
    service: State<'_, QconnectServiceState>,
) -> Result<String, RuntimeError> {
    let custom = service.custom_device_name.read().await;
    if let Some(ref name) = *custom {
        if !name.trim().is_empty() {
            return Ok(name.clone());
        }
    }
    // Fall back to env var → default
    Ok(std::env::var("QBZ_QCONNECT_DEVICE_NAME")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(resolve_default_qconnect_device_name))
}

#[tauri::command]
pub async fn v2_qconnect_set_device_name(
    name: String,
    service: State<'_, QconnectServiceState>,
) -> Result<(), RuntimeError> {
    let trimmed = name.trim().to_string();
    let mut guard = service.custom_device_name.write().await;
    if trimmed.is_empty() {
        *guard = None;
        persist_device_name(None);
    } else {
        *guard = Some(trimmed.clone());
        persist_device_name(Some(&trimmed));
    }
    Ok(())
}

#[tauri::command]
pub fn v2_get_hostname() -> Result<String, RuntimeError> {
    Ok(resolve_system_hostname())
}


#[cfg(test)]
mod tests {
    use super::queue_resolution::{
        resolve_controller_queue_item_from_snapshots, resolve_queue_item_ids_from_queue_state,
        QconnectRemoteSkipDirection,
    };
    use super::session::{
        find_unique_renderer_id, refresh_local_renderer_id, QconnectFileAudioQualitySnapshot,
    };
    use super::transport::{
        decode_hex_channel, default_qconnect_device_info, parse_subscribe_channels,
    };
    use super::{
        build_qconnect_file_audio_quality_snapshot, classify_qconnect_audio_quality,
        determine_queue_lookup_report_strategy, normalize_volume_to_fraction,
        should_skip_renderer_report_due_to_stale_snapshot, QconnectHandoffIntent,
        QconnectOutboundCommandType, QconnectRendererInfo, QconnectSessionState,
        QconnectTrackOrigin, AUDIO_QUALITY_HIRES_LEVEL1,
    };
    use qbz_models::RepeatMode;
    use qconnect_app::{
        resolve_handoff_intent, QConnectQueueState, QConnectRendererState, QueueCommandType,
    };
    use qconnect_core::QueueItem;
    use serde_json::json;

    #[test]
    fn decodes_hex_channels() {
        assert_eq!(decode_hex_channel("02").expect("decode"), vec![0x02]);
        assert_eq!(
            decode_hex_channel("0A0B").expect("decode"),
            vec![0x0A, 0x0B]
        );
    }

    #[test]
    fn parses_multiple_channels() {
        let channels =
            parse_subscribe_channels(vec!["02".to_string(), "0A0B".to_string()]).expect("channels");
        assert_eq!(channels, vec![vec![0x02], vec![0x0A, 0x0B]]);
    }

    #[test]
    fn normalizes_renderer_volume() {
        assert!((normalize_volume_to_fraction(58) - 0.58).abs() < f32::EPSILON);
        assert!((normalize_volume_to_fraction(-5) - 0.0).abs() < f32::EPSILON);
        assert!((normalize_volume_to_fraction(125) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn maps_outbound_command_type_to_protocol_command_type() {
        assert_eq!(
            QconnectOutboundCommandType::JoinSession.to_queue_command_type(),
            QueueCommandType::CtrlSrvrJoinSession
        );
        assert_eq!(
            QconnectOutboundCommandType::SetPlayerState.to_queue_command_type(),
            QueueCommandType::CtrlSrvrSetPlayerState
        );
        assert_eq!(
            QconnectOutboundCommandType::SetActiveRenderer.to_queue_command_type(),
            QueueCommandType::CtrlSrvrSetActiveRenderer
        );
        assert_eq!(
            QconnectOutboundCommandType::SetVolume.to_queue_command_type(),
            QueueCommandType::CtrlSrvrSetVolume
        );
        assert_eq!(
            QconnectOutboundCommandType::AskForRendererState.to_queue_command_type(),
            QueueCommandType::CtrlSrvrAskForRendererState
        );
    }

    #[test]
    fn flags_commands_that_require_remote_queue_admission() {
        assert!(QconnectOutboundCommandType::QueueAddTracks.requires_remote_queue_admission());
        assert!(QconnectOutboundCommandType::QueueLoadTracks.requires_remote_queue_admission());
        assert!(QconnectOutboundCommandType::QueueInsertTracks.requires_remote_queue_admission());
        assert!(QconnectOutboundCommandType::SetQueueState.requires_remote_queue_admission());
        assert!(QconnectOutboundCommandType::AutoplayLoadTracks.requires_remote_queue_admission());
        assert!(!QconnectOutboundCommandType::QueueRemoveTracks.requires_remote_queue_admission());
        assert!(!QconnectOutboundCommandType::ClearQueue.requires_remote_queue_admission());
        assert!(!QconnectOutboundCommandType::SetVolume.requires_remote_queue_admission());
    }

    #[test]
    fn maps_qconnect_track_origin_to_core_origin_and_handoff() {
        let local_core_origin = QconnectTrackOrigin::LocalLibrary.into_core_origin();
        assert_eq!(
            QconnectHandoffIntent::from_core(resolve_handoff_intent(local_core_origin)),
            QconnectHandoffIntent::ContinueLocally
        );

        let qobuz_core_origin = QconnectTrackOrigin::QobuzOnline.into_core_origin();
        assert_eq!(
            QconnectHandoffIntent::from_core(resolve_handoff_intent(qobuz_core_origin)),
            QconnectHandoffIntent::SendToConnect
        );
    }

    #[test]
    fn refreshes_local_renderer_id_from_exact_device_uuid_match() {
        let local_device_uuid = super::resolve_qconnect_device_uuid();
        let mut session = QconnectSessionState {
            renderers: vec![
                QconnectRendererInfo {
                    renderer_id: 1,
                    device_uuid: Some("peer-device".to_string()),
                    friendly_name: Some("BlitzPhone16ProMax".to_string()),
                    brand: Some("Apple".to_string()),
                    model: Some("iPhone".to_string()),
                    device_type: Some(6),
                },
                QconnectRendererInfo {
                    renderer_id: 6,
                    device_uuid: Some(local_device_uuid),
                    friendly_name: Some("QBZ Desktop".to_string()),
                    brand: Some("QBZ".to_string()),
                    model: Some("QBZ".to_string()),
                    device_type: Some(5),
                },
            ],
            ..Default::default()
        };

        refresh_local_renderer_id(&mut session);

        assert_eq!(session.local_renderer_id, Some(6));
    }

    #[test]
    fn refreshes_local_renderer_id_from_unique_fingerprint_when_uuid_missing() {
        // Use the runtime-resolved local device info so the test stays correct
        // regardless of hostname / env-var-driven device name overrides.
        let local_device_info = default_qconnect_device_info();
        let mut session = QconnectSessionState {
            renderers: vec![
                QconnectRendererInfo {
                    renderer_id: 1,
                    device_uuid: None,
                    friendly_name: Some("BlitzPhone16ProMax".to_string()),
                    brand: Some("Apple".to_string()),
                    model: Some("iPhone".to_string()),
                    device_type: Some(6),
                },
                QconnectRendererInfo {
                    renderer_id: 6,
                    device_uuid: None,
                    friendly_name: local_device_info.friendly_name.clone(),
                    brand: local_device_info.brand.clone(),
                    model: local_device_info.model.clone(),
                    device_type: local_device_info.device_type,
                },
            ],
            ..Default::default()
        };

        refresh_local_renderer_id(&mut session);

        assert_eq!(session.local_renderer_id, Some(6));
    }

    #[test]
    fn does_not_guess_local_renderer_id_when_fingerprint_is_ambiguous() {
        let mut session = QconnectSessionState {
            renderers: vec![
                QconnectRendererInfo {
                    renderer_id: 6,
                    device_uuid: None,
                    friendly_name: Some("QBZ Desktop".to_string()),
                    brand: Some("QBZ".to_string()),
                    model: Some("QBZ".to_string()),
                    device_type: Some(5),
                },
                QconnectRendererInfo {
                    renderer_id: 9,
                    device_uuid: None,
                    friendly_name: Some("QBZ Desktop".to_string()),
                    brand: Some("QBZ".to_string()),
                    model: Some("QBZ".to_string()),
                    device_type: Some(5),
                },
            ],
            ..Default::default()
        };

        refresh_local_renderer_id(&mut session);

        assert_eq!(session.local_renderer_id, None);
        assert_eq!(
            find_unique_renderer_id(&session, |renderer| renderer.device_type == Some(5)),
            None
        );
    }

    #[test]
    fn skips_renderer_report_when_local_track_and_renderer_snapshot_disagree() {
        assert!(should_skip_renderer_report_due_to_stale_snapshot(
            Some(388712168),
            None,
            None,
            Some(193849747),
        ));
    }

    #[test]
    fn does_not_skip_renderer_report_when_snapshot_matches_local_track() {
        assert!(!should_skip_renderer_report_due_to_stale_snapshot(
            Some(388712168),
            None,
            None,
            Some(388712168),
        ));
    }

    #[test]
    fn does_not_skip_renderer_report_once_current_queue_item_id_is_resolved() {
        assert!(!should_skip_renderer_report_due_to_stale_snapshot(
            Some(388712168),
            None,
            Some(42),
            Some(193849747),
        ));
    }

    #[test]
    fn detects_queue_lookup_track_transition() {
        assert_eq!(
            determine_queue_lookup_report_strategy(
                None,
                Some(57608710),
                Some(59952963),
                Some(59952963_i32),
                Some(1),
                Some(1),
                Some(2),
            ),
            Some("queue_lookup_track_transition"),
        );
    }

    #[test]
    fn detects_queue_lookup_queue_drift_when_next_item_changes() {
        assert_eq!(
            determine_queue_lookup_report_strategy(
                None,
                Some(123452387),
                Some(123452387),
                Some(123452387_i32),
                Some(1),
                Some(0),
                Some(12),
            ),
            Some("queue_lookup_queue_drift"),
        );
    }

    #[test]
    fn does_not_force_queue_lookup_when_renderer_snapshot_matches_queue() {
        assert_eq!(
            determine_queue_lookup_report_strategy(
                None,
                Some(123452387),
                Some(123452387),
                Some(123452387_i32),
                Some(1),
                Some(123452387_i32),
                Some(1),
            ),
            None,
        );
    }

    #[test]
    fn keeps_reporting_queue_item_ids_while_local_renderer_is_active() {
        assert!(super::should_report_queue_item_ids_for_renderer_state(
            None,
            None,
            true,
            Some(12),
        ));
    }

    #[test]
    fn does_not_force_queue_item_ids_for_peer_renderer_without_explicit_lookup() {
        assert!(!super::should_report_queue_item_ids_for_renderer_state(
            None,
            None,
            false,
            Some(12),
        ));
    }

    #[test]
    fn maps_qconnect_loop_mode_to_repeat_mode() {
        assert_eq!(
            super::qconnect_repeat_mode_from_loop_mode(0),
            Some(RepeatMode::Off)
        );
        assert_eq!(
            super::qconnect_repeat_mode_from_loop_mode(1),
            Some(RepeatMode::Off)
        );
        assert_eq!(
            super::qconnect_repeat_mode_from_loop_mode(2),
            Some(RepeatMode::One)
        );
        assert_eq!(
            super::qconnect_repeat_mode_from_loop_mode(3),
            Some(RepeatMode::All)
        );
        assert_eq!(super::qconnect_repeat_mode_from_loop_mode(99), None);
    }

    #[test]
    fn resolves_current_and_next_queue_item_ids_from_queue_order() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 4, "minor": 1 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 59952963, "queue_item_id": 59952963 },
                { "track_context_uuid": "ctx", "track_id": 57608710, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 2013968, "queue_item_id": 2 }
            ],
            "shuffle_mode": false,
            "shuffle_order": null,
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");

        assert_eq!(
            resolve_queue_item_ids_from_queue_state(&queue, 57608710),
            (Some(1), Some(2), Some(2013968)),
        );
    }

    #[test]
    fn normalizes_placeholder_current_queue_item_id_to_zero() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 8, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
                { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
                { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 }
            ],
            "shuffle_mode": false,
            "shuffle_order": null,
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");

        assert_eq!(
            resolve_queue_item_ids_from_queue_state(&queue, 126886853),
            (Some(0), Some(10), Some(123452387)),
        );
    }

    #[test]
    fn builds_effective_remote_renderer_snapshot_from_session_cursor() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 4, "minor": 1 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 126886862, "queue_item_id": 126886862 },
                { "track_context_uuid": "ctx", "track_id": 25584418, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 25120807, "queue_item_id": 2 }
            ],
            "shuffle_mode": false,
            "shuffle_order": null,
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");
        let renderer_state = super::QconnectSessionRendererState {
            active: Some(true),
            playing_state: Some(super::PLAYING_STATE_PLAYING),
            current_position_ms: Some(19_999),
            current_queue_item_id: Some(0),
            updated_at_ms: 12_345,
            ..Default::default()
        };

        let snapshot = super::session::build_session_renderer_snapshot(&queue, Some(&renderer_state), None);

        assert_eq!(snapshot.active, Some(true));
        assert_eq!(snapshot.playing_state, Some(super::PLAYING_STATE_PLAYING));
        assert_eq!(snapshot.current_position_ms, Some(19_999));
        assert_eq!(
            snapshot
                .current_track
                .as_ref()
                .map(|item| (item.track_id, item.queue_item_id)),
            Some((126886862, 0)),
        );
        assert_eq!(
            snapshot
                .next_track
                .as_ref()
                .map(|item| (item.track_id, item.queue_item_id)),
            Some((25584418, 1)),
        );
    }

    #[test]
    fn session_renderer_snapshot_uses_session_loop_mode_when_renderer_loop_mode_missing() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 10, "minor": 1 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 126886862, "queue_item_id": 126886862 }
            ],
            "shuffle_mode": false,
            "shuffle_order": null,
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");

        let snapshot = super::session::build_session_renderer_snapshot(
            &queue,
            Some(&super::QconnectSessionRendererState::default()),
            Some(2),
        );

        assert_eq!(snapshot.loop_mode, Some(2));
    }

    #[test]
    fn effective_renderer_snapshot_prefers_session_cursor_over_stale_app_snapshot() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 10, "minor": 1 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 126886862, "queue_item_id": 126886862 },
                { "track_context_uuid": "ctx", "track_id": 25584418, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 25120807, "queue_item_id": 2 },
                { "track_context_uuid": "ctx", "track_id": 25584411, "queue_item_id": 3 }
            ],
            "shuffle_mode": false,
            "shuffle_order": null,
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");
        let base_renderer = QConnectRendererState {
            active: Some(true),
            playing_state: Some(super::PLAYING_STATE_PAUSED),
            current_position_ms: Some(3_000),
            current_track: Some(QueueItem {
                track_context_uuid: "ctx".to_string(),
                track_id: 126886862,
                queue_item_id: 0,
            }),
            next_track: Some(QueueItem {
                track_context_uuid: "ctx".to_string(),
                track_id: 25584418,
                queue_item_id: 1,
            }),
            updated_at_ms: 111,
            ..Default::default()
        };
        let renderer_state = super::QconnectSessionRendererState {
            active: Some(true),
            playing_state: Some(super::PLAYING_STATE_PAUSED),
            current_position_ms: Some(15_000),
            current_queue_item_id: Some(2),
            updated_at_ms: 222,
            ..Default::default()
        };

        let snapshot = super::build_effective_renderer_snapshot(
            &queue,
            &base_renderer,
            Some(&renderer_state),
            None,
        );

        assert_eq!(snapshot.current_position_ms, Some(15_000));
        assert_eq!(snapshot.updated_at_ms, 222);
        assert_eq!(
            snapshot
                .current_track
                .as_ref()
                .map(|item| (item.track_id, item.queue_item_id)),
            Some((25120807, 2)),
        );
        assert_eq!(
            snapshot
                .next_track
                .as_ref()
                .map(|item| (item.track_id, item.queue_item_id)),
            Some((25584411, 3)),
        );
    }

    #[test]
    fn effective_renderer_snapshot_preserves_authoritative_renderer_next_track() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 22, "minor": 4 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 43013244, "queue_item_id": 0 },
                { "track_context_uuid": "ctx", "track_id": 43013245, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 43013246, "queue_item_id": 2 },
                { "track_context_uuid": "ctx", "track_id": 43013247, "queue_item_id": 3 },
                { "track_context_uuid": "ctx", "track_id": 43013248, "queue_item_id": 4 },
                { "track_context_uuid": "ctx", "track_id": 43013249, "queue_item_id": 5 },
                { "track_context_uuid": "ctx", "track_id": 43013250, "queue_item_id": 6 },
                { "track_context_uuid": "ctx", "track_id": 43013251, "queue_item_id": 7 },
                { "track_context_uuid": "ctx", "track_id": 43013252, "queue_item_id": 8 }
            ],
            "shuffle_mode": true,
            "shuffle_order": [0, 3, 6, 4, 5, 1, 7, 8, 2],
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");
        let base_renderer = QConnectRendererState {
            active: Some(true),
            playing_state: Some(super::PLAYING_STATE_PLAYING),
            current_position_ms: Some(41_000),
            current_track: Some(QueueItem {
                track_context_uuid: "ctx".to_string(),
                track_id: 43013244,
                queue_item_id: 0,
            }),
            next_track: Some(QueueItem {
                track_context_uuid: "ctx".to_string(),
                track_id: 43013251,
                queue_item_id: 7,
            }),
            updated_at_ms: 123,
            ..Default::default()
        };

        let snapshot = super::build_effective_renderer_snapshot(&queue, &base_renderer, None, None);

        assert_eq!(
            snapshot
                .current_track
                .as_ref()
                .map(|item| (item.track_id, item.queue_item_id)),
            Some((43013244, 0)),
        );
        assert_eq!(
            snapshot
                .next_track
                .as_ref()
                .map(|item| (item.track_id, item.queue_item_id)),
            Some((43013251, 7)),
        );
    }

    #[test]
    fn visible_queue_projection_respects_remote_shuffle_order() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 40, "minor": 1 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 101, "queue_item_id": 0 },
                { "track_context_uuid": "ctx", "track_id": 102, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 103, "queue_item_id": 2 },
                { "track_context_uuid": "ctx", "track_id": 104, "queue_item_id": 3 },
                { "track_context_uuid": "ctx", "track_id": 105, "queue_item_id": 4 }
            ],
            "shuffle_mode": true,
            "shuffle_order": [0, 3, 1, 4, 2],
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");
        let renderer = QConnectRendererState {
            current_track: Some(QueueItem {
                track_context_uuid: "ctx".to_string(),
                track_id: 101,
                queue_item_id: 0,
            }),
            next_track: Some(QueueItem {
                track_context_uuid: "ctx".to_string(),
                track_id: 104,
                queue_item_id: 3,
            }),
            ..Default::default()
        };

        let projection = super::build_visible_queue_projection(&queue, &renderer);

        assert_eq!(
            projection
                .current_track
                .as_ref()
                .map(|item| (item.track_id, item.queue_item_id)),
            Some((101, 0)),
        );
        assert_eq!(
            projection
                .upcoming_tracks
                .iter()
                .map(|item| item.queue_item_id)
                .collect::<Vec<u64>>(),
            vec![3, 1, 4, 2],
        );
    }

    #[test]
    fn visible_queue_projection_can_infer_current_from_next_anchor() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 41, "minor": 1 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 201, "queue_item_id": 0 },
                { "track_context_uuid": "ctx", "track_id": 202, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 203, "queue_item_id": 2 },
                { "track_context_uuid": "ctx", "track_id": 204, "queue_item_id": 3 }
            ],
            "shuffle_mode": true,
            "shuffle_order": [0, 3, 1, 2],
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");
        let renderer = QConnectRendererState {
            current_track: None,
            next_track: Some(QueueItem {
                track_context_uuid: "ctx".to_string(),
                track_id: 204,
                queue_item_id: 3,
            }),
            ..Default::default()
        };

        let projection = super::build_visible_queue_projection(&queue, &renderer);

        assert_eq!(
            projection
                .current_track
                .as_ref()
                .map(|item| (item.track_id, item.queue_item_id)),
            Some((201, 0)),
        );
        assert_eq!(
            projection
                .upcoming_tracks
                .iter()
                .map(|item| item.queue_item_id)
                .collect::<Vec<u64>>(),
            vec![3, 1, 2],
        );
    }

    #[test]
    fn resolves_core_shuffle_order_with_current_and_renderer_next_anchor() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 31, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 72930174, "queue_item_id": 0 },
                { "track_context_uuid": "ctx", "track_id": 72930175, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 72930176, "queue_item_id": 2 },
                { "track_context_uuid": "ctx", "track_id": 72930177, "queue_item_id": 3 },
                { "track_context_uuid": "ctx", "track_id": 72930178, "queue_item_id": 4 },
                { "track_context_uuid": "ctx", "track_id": 72930179, "queue_item_id": 5 },
                { "track_context_uuid": "ctx", "track_id": 72930180, "queue_item_id": 6 },
                { "track_context_uuid": "ctx", "track_id": 72930181, "queue_item_id": 7 },
                { "track_context_uuid": "ctx", "track_id": 72930182, "queue_item_id": 8 },
                { "track_context_uuid": "ctx", "track_id": 72930183, "queue_item_id": 9 }
            ],
            "shuffle_mode": true,
            "shuffle_order": [8, 5, 1, 9, 3, 4, 0, 6, 2, 7],
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");

        assert_eq!(
            super::queue_resolution::resolve_core_shuffle_order(
                &queue,
                Some(0),
                Some(72930174),
                Some(8),
                Some(72930182)
            ),
            Some(vec![0, 8, 5, 1, 9, 3, 4, 6, 2, 7]),
        );
    }

    #[test]
    fn resolves_core_shuffle_order_keeps_current_first_for_resumed_remote_shuffle() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 30, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 43013244, "queue_item_id": 0 },
                { "track_context_uuid": "ctx", "track_id": 43013245, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 43013246, "queue_item_id": 2 },
                { "track_context_uuid": "ctx", "track_id": 43013247, "queue_item_id": 3 },
                { "track_context_uuid": "ctx", "track_id": 43013248, "queue_item_id": 4 },
                { "track_context_uuid": "ctx", "track_id": 43013249, "queue_item_id": 5 },
                { "track_context_uuid": "ctx", "track_id": 43013250, "queue_item_id": 6 },
                { "track_context_uuid": "ctx", "track_id": 43013251, "queue_item_id": 7 },
                { "track_context_uuid": "ctx", "track_id": 43013252, "queue_item_id": 8 }
            ],
            "shuffle_mode": true,
            "shuffle_order": [0, 3, 6, 4, 5, 1, 7, 8, 2],
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");

        assert_eq!(
            super::queue_resolution::resolve_core_shuffle_order(
                &queue,
                Some(8),
                Some(43013252),
                Some(2),
                Some(43013246)
            ),
            Some(vec![8, 2, 0, 3, 6, 4, 5, 1, 7]),
        );
    }

    #[test]
    fn reloads_remote_track_only_when_track_id_changed() {
        let playback_state = qbz_player::PlaybackState {
            is_playing: false,
            position: 0,
            duration: 279,
            track_id: 193849747,
            volume: 1.0,
        };

        // Same track: do not reload, even if buffering still in progress.
        assert!(!super::track_loading::should_reload_remote_track(
            &playback_state,
            193849747,
        ));
        // Different track: reload.
        assert!(super::track_loading::should_reload_remote_track(
            &playback_state,
            126886862,
        ));
    }

    #[test]
    fn resolves_next_queue_item_id_from_shuffle_order() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 4, "minor": 1 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 10, "queue_item_id": 100 },
                { "track_context_uuid": "ctx", "track_id": 20, "queue_item_id": 200 },
                { "track_context_uuid": "ctx", "track_id": 30, "queue_item_id": 300 }
            ],
            "shuffle_mode": true,
            "shuffle_order": [2, 0, 1],
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");

        assert_eq!(
            resolve_queue_item_ids_from_queue_state(&queue, 10),
            (Some(100), Some(200), Some(20)),
        );
    }

    #[test]
    fn classifies_24_bit_streams_as_hires_level1() {
        assert_eq!(
            classify_qconnect_audio_quality(44_100, 24),
            AUDIO_QUALITY_HIRES_LEVEL1
        );
        assert_eq!(
            build_qconnect_file_audio_quality_snapshot(96_000, 24, 2),
            Some(QconnectFileAudioQualitySnapshot {
                sampling_rate: 96_000,
                bit_depth: 24,
                nb_channels: 2,
                audio_quality: AUDIO_QUALITY_HIRES_LEVEL1,
            }),
        );
    }

    #[test]
    fn materialization_reapplies_same_version_when_shuffle_order_changes() {
        let previous: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 28, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 1, "queue_item_id": 0 },
                { "track_context_uuid": "ctx", "track_id": 2, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 3, "queue_item_id": 2 }
            ],
            "shuffle_mode": true,
            "shuffle_order": null,
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 10
        }))
        .expect("previous queue state");

        let next: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 28, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 1, "queue_item_id": 0 },
                { "track_context_uuid": "ctx", "track_id": 2, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 3, "queue_item_id": 2 }
            ],
            "shuffle_mode": true,
            "shuffle_order": [0, 2, 1],
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 20
        }))
        .expect("next queue state");

        assert!(super::corebridge::queue_state_needs_materialization(
            Some(&previous),
            &next
        ));
    }

    #[test]
    fn materialization_skips_identical_snapshot_even_if_timestamp_changes() {
        let previous: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 28, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 1, "queue_item_id": 0 },
                { "track_context_uuid": "ctx", "track_id": 2, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 3, "queue_item_id": 2 }
            ],
            "shuffle_mode": true,
            "shuffle_order": [0, 2, 1],
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 10
        }))
        .expect("previous queue state");

        let next: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 28, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 1, "queue_item_id": 0 },
                { "track_context_uuid": "ctx", "track_id": 2, "queue_item_id": 1 },
                { "track_context_uuid": "ctx", "track_id": 3, "queue_item_id": 2 }
            ],
            "shuffle_mode": true,
            "shuffle_order": [0, 2, 1],
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 20
        }))
        .expect("next queue state");

        assert!(!super::corebridge::queue_state_needs_materialization(
            Some(&previous),
            &next
        ));
    }

    #[test]
    fn resolves_remote_next_target_using_renderer_next_queue_item_id() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 8, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
                { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
                { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 }
            ],
            "shuffle_mode": false,
            "shuffle_order": null,
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");
        let renderer: QConnectRendererState = serde_json::from_value(json!({
            "current_track": { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
            "next_track": { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 },
            "current_position_ms": 64_000,
            "playing_state": 2,
            "updated_at_ms": 0
        }))
        .expect("renderer state");

        assert_eq!(
            resolve_controller_queue_item_from_snapshots(
                &queue,
                &renderer,
                QconnectRemoteSkipDirection::Next,
            ),
            super::queue_resolution::QconnectControllerQueueItemResolution {
                target_queue_item_id: Some(1),
                strategy: "renderer_next_queue_item_id_verified",
                queue_index: Some(2),
                matched_track_id: Some(126886854),
                matched_queue_item_id: Some(1),
            }
        );
    }

    #[test]
    fn resolves_remote_previous_to_restart_first_cloud_item_when_mid_track() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 8, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
                { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
                { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 }
            ],
            "shuffle_mode": false,
            "shuffle_order": null,
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");
        let renderer: QConnectRendererState = serde_json::from_value(json!({
            "current_track": { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
            "next_track": { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 },
            "current_position_ms": 64_000,
            "playing_state": 2,
            "updated_at_ms": 0
        }))
        .expect("renderer state");

        assert_eq!(
            resolve_controller_queue_item_from_snapshots(
                &queue,
                &renderer,
                QconnectRemoteSkipDirection::Previous,
            ),
            super::queue_resolution::QconnectControllerQueueItemResolution {
                target_queue_item_id: Some(0),
                strategy: "restart_current_queue_item",
                queue_index: Some(0),
                matched_track_id: Some(126886853),
                matched_queue_item_id: Some(0),
            }
        );
    }

    #[test]
    fn resolves_remote_previous_to_prior_item_when_near_track_start() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 8, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
                { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
                { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 }
            ],
            "shuffle_mode": false,
            "shuffle_order": null,
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");
        let renderer: QConnectRendererState = serde_json::from_value(json!({
            "current_track": { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
            "next_track": { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 },
            "current_position_ms": 2_000,
            "playing_state": 2,
            "updated_at_ms": 0
        }))
        .expect("renderer state");

        assert_eq!(
            resolve_controller_queue_item_from_snapshots(
                &queue,
                &renderer,
                QconnectRemoteSkipDirection::Previous,
            ),
            super::queue_resolution::QconnectControllerQueueItemResolution {
                target_queue_item_id: Some(0),
                strategy: "queue_item_before_current",
                queue_index: Some(0),
                matched_track_id: Some(126886853),
                matched_queue_item_id: Some(0),
            }
        );
    }

    #[test]
    fn resolves_remote_previous_to_prior_item_even_mid_track() {
        let queue: QConnectQueueState = serde_json::from_value(json!({
            "version": { "major": 8, "minor": 2 },
            "queue_items": [
                { "track_context_uuid": "ctx", "track_id": 126886853, "queue_item_id": 126886853 },
                { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
                { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 }
            ],
            "shuffle_mode": false,
            "shuffle_order": null,
            "autoplay_mode": false,
            "autoplay_loading": false,
            "autoplay_items": [],
            "updated_at_ms": 0
        }))
        .expect("queue state");
        let renderer: QConnectRendererState = serde_json::from_value(json!({
            "current_track": { "track_context_uuid": "ctx", "track_id": 123452387, "queue_item_id": 10 },
            "next_track": { "track_context_uuid": "ctx", "track_id": 126886854, "queue_item_id": 1 },
            "current_position_ms": 64_000,
            "playing_state": 2,
            "updated_at_ms": 0
        }))
        .expect("renderer state");

        assert_eq!(
            resolve_controller_queue_item_from_snapshots(
                &queue,
                &renderer,
                QconnectRemoteSkipDirection::Previous,
            ),
            super::queue_resolution::QconnectControllerQueueItemResolution {
                target_queue_item_id: Some(0),
                strategy: "queue_item_before_current",
                queue_index: Some(0),
                matched_track_id: Some(126886853),
                matched_queue_item_id: Some(0),
            }
        );
    }
}
