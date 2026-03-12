use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use async_trait::async_trait;
use qbz_models::{Quality, QueueTrack, Track};
use qconnect_app::{
    evaluate_remote_queue_admission, resolve_handoff_intent, HandoffIntent, QConnectQueueState,
    QConnectRendererState, QconnectApp, QconnectAppEvent, QconnectEventSink, QueueCommandType,
    RendererCommand, RendererReport, RendererReportType, TrackOrigin,
};
use qconnect_transport_ws::{NativeWsTransport, WsTransportConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{
    core_bridge::{CoreBridge, CoreBridgeState},
    runtime::{CommandRequirement, RuntimeError, RuntimeManagerState},
    AppState,
};

const PLAYING_STATE_UNKNOWN: i32 = 0;
const PLAYING_STATE_STOPPED: i32 = 1;
const PLAYING_STATE_PLAYING: i32 = 2;
const PLAYING_STATE_PAUSED: i32 = 3;
const BUFFER_STATE_OK: i32 = 2;
const QCONNECT_QWS_TOKEN_KIND: &str = "jwt_qws";
const QCONNECT_QWS_CREATE_TOKEN_PATH: &str = "/qws/createToken";
const DEFAULT_QCONNECT_DEVICE_NAME: &str = "QBZ Desktop";
const DEFAULT_QCONNECT_DEVICE_BRAND: &str = "QBZ";
const DEFAULT_QCONNECT_DEVICE_MODEL: &str = "QBZ";
const DEFAULT_QCONNECT_DEVICE_TYPE: i32 = 5; // computer
const DEFAULT_QCONNECT_SOFTWARE_PREFIX: &str = "qbz";
const QCONNECT_REMOTE_QUEUE_SOURCE: &str = "qobuz_connect_remote";
// AudioQuality enum: 0=unknown, 1=mp3, 2=cd, 3=hires_l1, 4=hires_l2(192k), 5=hires_l3(384k)
const AUDIO_QUALITY_MP3: i32 = 1;
const AUDIO_QUALITY_HIRES_LEVEL2: i32 = 4;
// VolumeRemoteControl enum: 0=unknown, 1=not_allowed, 2=allowed
const VOLUME_REMOTE_CONTROL_ALLOWED: i32 = 2;
// JoinSessionReason: 0=unknown, 1=controller_request, 2=reconnection
const JOIN_SESSION_REASON_CONTROLLER_REQUEST: i32 = 1;
static QCONNECT_DEVICE_UUID: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectConnectOptions {
    pub endpoint_url: Option<String>,
    pub jwt_qws: Option<String>,
    pub reconnect_backoff_ms: Option<u64>,
    pub reconnect_backoff_max_ms: Option<u64>,
    pub connect_timeout_ms: Option<u64>,
    pub keepalive_interval_ms: Option<u64>,
    pub qcloud_proto: Option<u32>,
    pub subscribe_channels_hex: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectConnectionStatus {
    pub running: bool,
    pub transport_connected: bool,
    pub endpoint_url: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QconnectOutboundCommandType {
    JoinSession,
    SetPlayerState,
    SetActiveRenderer,
    SetVolume,
    SetLoopMode,
    MuteVolume,
    SetMaxAudioQuality,
    AskForRendererState,
    QueueAddTracks,
    QueueLoadTracks,
    QueueInsertTracks,
    QueueRemoveTracks,
    QueueReorderTracks,
    ClearQueue,
    SetShuffleMode,
    SetAutoplayMode,
    AutoplayLoadTracks,
    AutoplayRemoveTracks,
    SetQueueState,
    AskForQueueState,
}

impl QconnectOutboundCommandType {
    const fn to_queue_command_type(self) -> QueueCommandType {
        match self {
            Self::JoinSession => QueueCommandType::CtrlSrvrJoinSession,
            Self::SetPlayerState => QueueCommandType::CtrlSrvrSetPlayerState,
            Self::SetActiveRenderer => QueueCommandType::CtrlSrvrSetActiveRenderer,
            Self::SetVolume => QueueCommandType::CtrlSrvrSetVolume,
            Self::SetLoopMode => QueueCommandType::CtrlSrvrSetLoopMode,
            Self::MuteVolume => QueueCommandType::CtrlSrvrMuteVolume,
            Self::SetMaxAudioQuality => QueueCommandType::CtrlSrvrSetMaxAudioQuality,
            Self::AskForRendererState => QueueCommandType::CtrlSrvrAskForRendererState,
            Self::QueueAddTracks => QueueCommandType::CtrlSrvrQueueAddTracks,
            Self::QueueLoadTracks => QueueCommandType::CtrlSrvrQueueLoadTracks,
            Self::QueueInsertTracks => QueueCommandType::CtrlSrvrQueueInsertTracks,
            Self::QueueRemoveTracks => QueueCommandType::CtrlSrvrQueueRemoveTracks,
            Self::QueueReorderTracks => QueueCommandType::CtrlSrvrQueueReorderTracks,
            Self::ClearQueue => QueueCommandType::CtrlSrvrClearQueue,
            Self::SetShuffleMode => QueueCommandType::CtrlSrvrSetShuffleMode,
            Self::SetAutoplayMode => QueueCommandType::CtrlSrvrSetAutoplayMode,
            Self::AutoplayLoadTracks => QueueCommandType::CtrlSrvrAutoplayLoadTracks,
            Self::AutoplayRemoveTracks => QueueCommandType::CtrlSrvrAutoplayRemoveTracks,
            Self::SetQueueState => QueueCommandType::CtrlSrvrSetQueueState,
            Self::AskForQueueState => QueueCommandType::CtrlSrvrAskForQueueState,
        }
    }

    const fn requires_remote_queue_admission(self) -> bool {
        matches!(
            self,
            Self::QueueAddTracks
                | Self::QueueLoadTracks
                | Self::QueueInsertTracks
                | Self::SetQueueState
                | Self::AutoplayLoadTracks
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QconnectSendCommandRequest {
    pub command_type: QconnectOutboundCommandType,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QconnectTrackOrigin {
    QobuzOnline,
    QobuzOfflineCache,
    LocalLibrary,
    Plex,
    ExternalUnknown,
}

impl QconnectTrackOrigin {
    const fn into_core_origin(self) -> TrackOrigin {
        match self {
            Self::QobuzOnline => TrackOrigin::QobuzOnline,
            Self::QobuzOfflineCache => TrackOrigin::QobuzOfflineCache,
            Self::LocalLibrary => TrackOrigin::LocalLibrary,
            Self::Plex => TrackOrigin::Plex,
            Self::ExternalUnknown => TrackOrigin::ExternalUnknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QconnectHandoffIntent {
    ContinueLocally,
    SendToConnect,
}

impl QconnectHandoffIntent {
    const fn from_core(intent: HandoffIntent) -> Self {
        match intent {
            HandoffIntent::ContinueLocally => Self::ContinueLocally,
            HandoffIntent::SendToConnect => Self::SendToConnect,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QconnectAdmissionResult {
    pub accepted: bool,
    pub reason: String,
    pub origin: QconnectTrackOrigin,
    pub handoff_intent: QconnectHandoffIntent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QconnectSendCommandWithAdmissionRequest {
    pub command_type: QconnectOutboundCommandType,
    pub origin: QconnectTrackOrigin,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QconnectAdmissionBlockedEvent {
    pub command_type: QconnectOutboundCommandType,
    pub origin: QconnectTrackOrigin,
    pub reason: String,
    pub handoff_intent: QconnectHandoffIntent,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectDeviceCapabilitiesPayload {
    pub min_audio_quality: Option<i32>,
    pub max_audio_quality: Option<i32>,
    pub volume_remote_control: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectDeviceInfoPayload {
    pub device_uuid: Option<String>,
    pub friendly_name: Option<String>,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub device_type: Option<i32>,
    pub capabilities: Option<QconnectDeviceCapabilitiesPayload>,
    pub software_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectJoinSessionRequest {
    pub session_uuid: Option<String>,
    pub device_info: Option<QconnectDeviceInfoPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QconnectQueueVersionPayload {
    pub major: u64,
    pub minor: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSetPlayerStateQueueItemPayload {
    pub queue_version: Option<QconnectQueueVersionPayload>,
    pub id: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSetPlayerStateRequest {
    pub playing_state: Option<i32>,
    pub current_position: Option<i32>,
    pub current_queue_item: Option<QconnectSetPlayerStateQueueItemPayload>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSetActiveRendererRequest {
    pub renderer_id: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSetVolumeRequest {
    pub renderer_id: Option<i32>,
    pub volume: Option<i32>,
    pub volume_delta: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSetLoopModeRequest {
    pub loop_mode: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectMuteVolumeRequest {
    pub renderer_id: Option<i32>,
    pub value: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSetMaxAudioQualityRequest {
    pub renderer_id: Option<i32>,
    pub max_audio_quality: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectAskForRendererStateRequest {
    pub renderer_id: Option<i32>,
}

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
}

#[derive(Clone)]
struct TauriQconnectEventSink {
    app_handle: AppHandle,
    core_bridge: Arc<RwLock<Option<CoreBridge>>>,
    sync_state: Arc<Mutex<QconnectRemoteSyncState>>,
}

#[derive(Debug, Default)]
struct QconnectRemoteSyncState {
    last_renderer_queue_item_id: Option<u64>,
    last_renderer_next_queue_item_id: Option<u64>,
    last_renderer_track_id: Option<u64>,
    last_renderer_next_track_id: Option<u64>,
    last_renderer_playing_state: Option<i32>,
    last_applied_queue_version: Option<(u64, u64)>,
    /// Session topology — stored from session management events (types 81-87).
    session: QconnectSessionState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSessionState {
    pub session_uuid: Option<String>,
    pub active_renderer_id: Option<i32>,
    pub renderers: Vec<QconnectRendererInfo>,
}

#[derive(Debug, Clone, Serialize)]
struct QconnectRendererReportDebugEvent {
    requested_current_queue_item_id: Option<i32>,
    requested_next_queue_item_id: Option<i32>,
    resolved_current_queue_item_id: Option<i32>,
    resolved_next_queue_item_id: Option<i32>,
    sent_current_queue_item_id: Option<i32>,
    sent_next_queue_item_id: Option<i32>,
    current_track_id: Option<i64>,
    playing_state: i32,
    current_position: Option<i32>,
    duration: Option<i32>,
    queue_version: QconnectQueueVersionPayload,
    resolution_strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QconnectRendererInfo {
    pub renderer_id: i32,
    pub friendly_name: Option<String>,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub device_type: Option<i32>,
}

#[async_trait]
impl QconnectEventSink for TauriQconnectEventSink {
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
                sync_state.last_renderer_queue_item_id = renderer_state
                    .current_track
                    .as_ref()
                    .map(|item| item.queue_item_id);
                sync_state.last_renderer_next_queue_item_id = renderer_state
                    .next_track
                    .as_ref()
                    .map(|item| item.queue_item_id);
                sync_state.last_renderer_track_id = renderer_state
                    .current_track
                    .as_ref()
                    .map(|item| item.track_id);
                sync_state.last_renderer_next_track_id = renderer_state
                    .next_track
                    .as_ref()
                    .map(|item| item.track_id);
                sync_state.last_renderer_playing_state = renderer_state.playing_state;
            }
            QconnectAppEvent::QueueUpdated(queue_state) => {
                if let Err(err) = materialize_remote_queue_to_corebridge(
                    &self.core_bridge,
                    &self.sync_state,
                    queue_state,
                )
                .await
                {
                    log::warn!(
                        "[QConnect] Failed to materialize remote queue in CoreBridge: {err}"
                    );
                }
            }
            QconnectAppEvent::RendererCommandApplied { command, state } => {
                log::info!(
                    "[QConnect] Renderer command applied: {:?}",
                    command
                );
                if let Err(err) =
                    apply_renderer_command_to_corebridge(&self.core_bridge, command, state).await
                {
                    log::warn!("[QConnect] Failed to apply renderer command to CoreBridge: {err}");
                }
            }
            _ => {}
        }

        if let Err(err) = self.app_handle.emit("qconnect:event", &event) {
            log::warn!("[QConnect] Failed to emit tauri event: {err}");
        }
    }
}

impl TauriQconnectEventSink {
    async fn apply_session_management_event(&self, message_type: &str, payload: &Value) {
        let mut state = self.sync_state.lock().await;
        match message_type {
            "MESSAGE_TYPE_SRVR_CTRL_SESSION_STATE" => {
                if let Some(uuid) = payload.get("session_uuid").and_then(Value::as_str) {
                    state.session.session_uuid = Some(uuid.to_string());
                }
                if let Some(id) = payload.get("active_renderer_id").and_then(Value::as_i64) {
                    state.session.active_renderer_id = Some(id as i32);
                }
            }
            "MESSAGE_TYPE_SRVR_CTRL_ADD_RENDERER" => {
                if let Some(renderer_id) = payload.get("renderer_id").and_then(Value::as_i64) {
                    let renderer_id = renderer_id as i32;
                    // Don't add duplicates
                    if !state
                        .session
                        .renderers
                        .iter()
                        .any(|r| r.renderer_id == renderer_id)
                    {
                        let device_info = payload.get("device_info");
                        state.session.renderers.push(QconnectRendererInfo {
                            renderer_id,
                            friendly_name: device_info
                                .and_then(|d| d.get("friendly_name"))
                                .and_then(Value::as_str)
                                .map(String::from),
                            brand: device_info
                                .and_then(|d| d.get("brand"))
                                .and_then(Value::as_str)
                                .map(String::from),
                            model: device_info
                                .and_then(|d| d.get("model"))
                                .and_then(Value::as_str)
                                .map(String::from),
                            device_type: device_info
                                .and_then(|d| d.get("device_type"))
                                .and_then(Value::as_i64)
                                .map(|v| v as i32),
                        });
                    }
                }
            }
            "MESSAGE_TYPE_SRVR_CTRL_UPDATE_RENDERER" => {
                if let Some(renderer_id) = payload.get("renderer_id").and_then(Value::as_i64) {
                    let renderer_id = renderer_id as i32;
                    if let Some(existing) = state
                        .session
                        .renderers
                        .iter_mut()
                        .find(|r| r.renderer_id == renderer_id)
                    {
                        let device_info = payload.get("device_info");
                        if let Some(name) = device_info
                            .and_then(|d| d.get("friendly_name"))
                            .and_then(Value::as_str)
                        {
                            existing.friendly_name = Some(name.to_string());
                        }
                    }
                }
            }
            "MESSAGE_TYPE_SRVR_CTRL_REMOVE_RENDERER" => {
                if let Some(renderer_id) = payload.get("renderer_id").and_then(Value::as_i64) {
                    let renderer_id = renderer_id as i32;
                    state
                        .session
                        .renderers
                        .retain(|r| r.renderer_id != renderer_id);
                }
            }
            "MESSAGE_TYPE_SRVR_CTRL_ACTIVE_RENDERER_CHANGED" => {
                if let Some(id) = payload
                    .get("active_renderer_id")
                    .and_then(Value::as_i64)
                {
                    state.session.active_renderer_id = Some(id as i32);
                }
            }
            _ => {}
        }
    }
}

pub struct QconnectServiceState {
    inner: Arc<Mutex<QconnectServiceInner>>,
}

impl QconnectServiceState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(QconnectServiceInner::default())),
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
        if guard.runtime.is_some() {
            return Err("QConnect service is already running".to_string());
        }

        let transport = Arc::new(NativeWsTransport::new());
        let sync_state = Arc::new(Mutex::new(QconnectRemoteSyncState::default()));
        let sink = Arc::new(TauriQconnectEventSink {
            app_handle: app_handle.clone(),
            core_bridge,
            sync_state: Arc::clone(&sync_state),
        });
        let app = Arc::new(QconnectApp::new(transport, sink));

        app.connect(config.clone())
            .await
            .map_err(|err| format!("qconnect transport connect failed: {err}"))?;

        let mut transport_rx = app.subscribe_transport_events();
        let app_for_loop = Arc::clone(&app);
        let app_for_errors = app_handle.clone();

        let event_loop = tauri::async_runtime::spawn(async move {
            log::info!("[QConnect/EventLoop] Started listening for transport events");
            let mut renderer_joined = false;
            loop {
                match transport_rx.recv().await {
                    Ok(event) => {
                        // Check for SESSION_STATE to trigger deferred renderer join
                        if !renderer_joined {
                            if let qconnect_transport_ws::TransportEvent::InboundQueueServerEvent(ref evt) = event {
                                if evt.message_type() == "MESSAGE_TYPE_SRVR_CTRL_SESSION_STATE" {
                                    if let Some(session_uuid) = evt.payload.get("session_uuid").and_then(|v| v.as_str()) {
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
                                log::warn!("[QConnect/Transport] WebSocket disconnected");
                            }
                            qconnect_transport_ws::TransportEvent::Authenticated => {
                                log::info!("[QConnect/Transport] Authenticated with JWT");
                            }
                            qconnect_transport_ws::TransportEvent::Subscribed => {
                                log::info!("[QConnect/Transport] Subscribed to channels");
                            }
                            qconnect_transport_ws::TransportEvent::KeepalivePingSent => {
                                log::debug!("[QConnect/Transport] Keepalive ping sent");
                            }
                            qconnect_transport_ws::TransportEvent::KeepalivePongReceived => {
                                log::debug!("[QConnect/Transport] Keepalive pong received");
                            }
                            qconnect_transport_ws::TransportEvent::ReconnectScheduled { attempt, backoff_ms, reason } => {
                                log::warn!("[QConnect/Transport] Reconnect scheduled: attempt={} backoff={}ms reason={}", attempt, backoff_ms, reason);
                            }
                            qconnect_transport_ws::TransportEvent::InboundQueueServerEvent(evt) => {
                                log::info!("[QConnect] <-- Inbound queue event: {} payload={}", evt.message_type(), evt.payload);
                            }
                            qconnect_transport_ws::TransportEvent::InboundRendererServerCommand(cmd) => {
                                log::info!("[QConnect] <-- Inbound renderer command: {} payload={}", cmd.message_type(), cmd.payload);
                            }
                            qconnect_transport_ws::TransportEvent::InboundFrameDecoded { cloud_message_type, payload_size } => {
                                log::info!("[QConnect/Transport] <-- Frame decoded: cloud_type={} size={}", cloud_message_type, payload_size);
                            }
                            qconnect_transport_ws::TransportEvent::InboundPayloadBytes { cloud_message_type, payload } => {
                                log::info!("[QConnect/Transport] <-- Payload bytes: cloud_type={} len={} hex={}", cloud_message_type, payload.len(), hex_preview(payload, 64));
                            }
                            qconnect_transport_ws::TransportEvent::OutboundSent { message_type, action_uuid } => {
                                log::info!("[QConnect/Transport] --> Outbound sent: {} uuid={}", message_type, action_uuid);
                            }
                            qconnect_transport_ws::TransportEvent::TransportError { stage, message } => {
                                log::error!("[QConnect/Transport] Error: stage={} message={}", stage, message);
                            }
                            qconnect_transport_ws::TransportEvent::InboundReceived(envelope) => {
                                log::info!("[QConnect/Transport] <-- InboundReceived (JSON envelope)");
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
        if let Err(err) = bootstrap_remote_presence(&runtime_app).await {
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
        let (app, endpoint_url, last_error) = {
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

    pub async fn send_renderer_report(
        &self,
        report: RendererReport,
    ) -> Result<(), String> {
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

    /// Look up queue_item_id by track_id from the QConnect queue state.
    /// Searches queue_items first, then autoplay_items.
    /// Also updates sync_state so future lookups are fast.
    async fn lookup_queue_item_id_by_track_id(&self, track_id: u64) -> Option<u64> {
        let (app, sync_state) = {
            let guard = self.inner.lock().await;
            let runtime = guard.runtime.as_ref()?;
            (Arc::clone(&runtime.app), Arc::clone(&runtime.sync_state))
        };

        let queue = app.queue_state_snapshot().await;
        let found = queue
            .queue_items
            .iter()
            .chain(queue.autoplay_items.iter())
            .find(|item| item.track_id == track_id);

        if let Some(item) = found {
            let qid = item.queue_item_id;
            // Cache in sync_state for future reports
            let mut state = sync_state.lock().await;
            state.last_renderer_queue_item_id = Some(qid);
            state.last_renderer_track_id = Some(track_id);
            log::debug!(
                "[QConnect] Resolved queue_item_id={} for track_id={} from queue state",
                qid,
                track_id
            );
            Some(qid)
        } else {
            log::debug!(
                "[QConnect] Could not find track_id={} in queue state ({} queue_items, {} autoplay_items)",
                track_id,
                queue.queue_items.len(),
                queue.autoplay_items.len()
            );
            None
        }
    }
}

impl Default for QconnectServiceState {
    fn default() -> Self {
        Self::new()
    }
}

async fn apply_renderer_command_to_corebridge(
    core_bridge: &Arc<RwLock<Option<CoreBridge>>>,
    command: &RendererCommand,
    renderer_state: &QConnectRendererState,
) -> Result<(), String> {
    let bridge_guard = core_bridge.read().await;
    let Some(bridge) = bridge_guard.as_ref() else {
        return Err("core bridge is not initialized yet".to_string());
    };

    match command {
        RendererCommand::SetState {
            playing_state,
            current_position_ms,
            current_track,
            ..
        } => {
            let resolved_playing_state = renderer_state.playing_state.or(*playing_state);
            let resolved_current_track = renderer_state
                .current_track
                .as_ref()
                .or(current_track.as_ref());
            if let Some(current_track) = resolved_current_track {
                if let Err(err) =
                    align_corebridge_queue_cursor(bridge, current_track.track_id).await
                {
                    log::warn!("[QConnect] Failed to align CoreBridge queue cursor: {err}");
                }

                if matches!(
                    resolved_playing_state,
                    Some(PLAYING_STATE_PLAYING | PLAYING_STATE_PAUSED)
                ) {
                    if let Err(err) =
                        ensure_remote_track_loaded(bridge, current_track.track_id).await
                    {
                        log::warn!(
                            "[QConnect] Failed to load remote track {}: {err}",
                            current_track.track_id
                        );
                    }
                }
            }

            if let Some(value) = resolved_playing_state {
                match value {
                    PLAYING_STATE_PLAYING => {
                        bridge.resume()?;
                    }
                    PLAYING_STATE_PAUSED => {
                        bridge.pause()?;
                    }
                    PLAYING_STATE_STOPPED => {
                        bridge.stop()?;
                    }
                    PLAYING_STATE_UNKNOWN => {}
                    _ => {
                        log::debug!("[QConnect] Unknown playing state received: {value}");
                    }
                }
            }

            if let Some(position_ms) = renderer_state.current_position_ms.or(*current_position_ms) {
                let current_pos_secs = bridge.get_playback_state().position;
                let target_secs = position_ms / 1000;
                // Only seek if the position differs by more than 2 seconds to
                // avoid audio hiccups from redundant seeks (e.g. when the server
                // echoes back our own position in a SET_STATE after queue_load).
                if current_pos_secs.abs_diff(target_secs) > 2 {
                    log::info!(
                        "[QConnect] SetState seek: current={}s target={}s",
                        current_pos_secs, target_secs
                    );
                    bridge.seek(target_secs)?;
                }
            }
        }
        RendererCommand::SetVolume { volume, .. } => {
            if let Some(resolved) = renderer_state.volume.or(*volume) {
                bridge.set_volume(normalize_volume_to_fraction(resolved))?;
            }
        }
        RendererCommand::MuteVolume { value } => {
            if *value {
                bridge.set_volume(0.0)?;
            } else if let Some(resolved) = renderer_state.volume {
                bridge.set_volume(normalize_volume_to_fraction(resolved))?;
            }
        }
        RendererCommand::SetActive { .. }
        | RendererCommand::SetMaxAudioQuality { .. }
        | RendererCommand::SetLoopMode { .. }
        | RendererCommand::SetShuffleMode { .. } => {}
    }

    Ok(())
}

async fn materialize_remote_queue_to_corebridge(
    core_bridge: &Arc<RwLock<Option<CoreBridge>>>,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    queue_state: &QConnectQueueState,
) -> Result<(), String> {
    let (
        renderer_queue_item_id,
        renderer_track_id,
        renderer_next_queue_item_id,
        renderer_next_track_id,
        renderer_playing_state,
        should_skip,
    ) = {
        let mut state = sync_state.lock().await;
        let queue_version = (queue_state.version.major, queue_state.version.minor);
        if state.last_applied_queue_version == Some(queue_version) {
            (
                state.last_renderer_queue_item_id,
                state.last_renderer_track_id,
                state.last_renderer_next_queue_item_id,
                state.last_renderer_next_track_id,
                state.last_renderer_playing_state,
                true,
            )
        } else {
            state.last_applied_queue_version = Some(queue_version);
            (
                state.last_renderer_queue_item_id,
                state.last_renderer_track_id,
                state.last_renderer_next_queue_item_id,
                state.last_renderer_next_track_id,
                state.last_renderer_playing_state,
                false,
            )
        }
    };

    if should_skip {
        log::debug!(
            "[QConnect] materialize_remote_queue: skipped (same version {}.{})",
            queue_state.version.major,
            queue_state.version.minor
        );
        return Ok(());
    }

    log::info!(
        "[QConnect] materialize_remote_queue: version={}.{} items={} renderer_qid={:?} renderer_tid={:?} renderer_next_qid={:?} renderer_next_tid={:?} playing_state={:?}",
        queue_state.version.major,
        queue_state.version.minor,
        queue_state.queue_items.len(),
        renderer_queue_item_id,
        renderer_track_id,
        renderer_next_queue_item_id,
        renderer_next_track_id,
        renderer_playing_state
    );

    let bridge_guard = core_bridge.read().await;
    let Some(bridge) = bridge_guard.as_ref() else {
        return Err("core bridge is not initialized yet".to_string());
    };

    if queue_state.queue_items.is_empty() {
        bridge.clear_queue().await;
        bridge.set_shuffle(false).await;
        return Ok(());
    }

    let unique_track_ids = dedupe_track_ids(queue_state);
    let fetched_tracks = bridge
        .get_tracks_batch(&unique_track_ids)
        .await
        .map_err(|err| format!("fetch tracks batch for remote queue: {err}"))?;

    let mut tracks_by_id = HashMap::with_capacity(fetched_tracks.len());
    for track in fetched_tracks {
        tracks_by_id.insert(track.id, model_track_to_core_queue_track(&track));
    }

    let mut queue_tracks = Vec::with_capacity(queue_state.queue_items.len());
    for item in &queue_state.queue_items {
        if let Some(queue_track) = tracks_by_id.get(&item.track_id) {
            queue_tracks.push(queue_track.clone());
            continue;
        }

        match bridge.get_track(item.track_id).await {
            Ok(track) => {
                let mapped = model_track_to_core_queue_track(&track);
                tracks_by_id.insert(item.track_id, mapped.clone());
                queue_tracks.push(mapped);
            }
            Err(err) => {
                log::warn!(
                    "[QConnect] Unable to hydrate remote queue track {}: {}",
                    item.track_id,
                    err
                );
            }
        }
    }

    if queue_tracks.is_empty() {
        return Err("remote queue materialization produced zero playable tracks".to_string());
    }

    // Resolve start index from remote state first, then from the local playback
    // cursor only if that track is still part of the remote queue.
    let current_playback_track_id = match bridge.get_playback_state().track_id {
        0 => None,
        track_id => Some(track_id),
    };
    let mut start_index =
        resolve_remote_start_index(queue_state, renderer_queue_item_id, renderer_track_id);
    if start_index.is_none() {
        start_index = resolve_remote_start_index(queue_state, renderer_next_queue_item_id, renderer_next_track_id)
            .map(|index| index.saturating_sub(1));
    }
    if start_index.is_none() {
        start_index = current_playback_track_id.and_then(|track_id| {
            queue_state
                .queue_items
                .iter()
                .position(|item| item.track_id == track_id)
        });
    }
    if start_index.is_none() && !queue_tracks.is_empty() {
        start_index = Some(0);
    }
    log::info!(
        "[QConnect] materialize_remote_queue: setting queue with {} tracks, start_index={:?}, local_track_id={:?}",
        queue_tracks.len(),
        start_index,
        current_playback_track_id
    );
    bridge.set_queue(queue_tracks, start_index).await;
    bridge.set_shuffle(queue_state.shuffle_mode).await;

    let local_track_missing_from_remote = current_playback_track_id
        .map(|track_id| {
            !queue_state
                .queue_items
                .iter()
                .any(|item| item.track_id == track_id)
        })
        .unwrap_or(true);

    if let Some(index) = start_index {
        if local_track_missing_from_remote {
            log::info!(
                "[QConnect] materialize_remote_queue: aligning queue cursor to remote index {}",
                index
            );
            let _ = bridge.play_index(index).await;
        }
    }

    if current_playback_track_id.is_some()
        && current_playback_track_id != renderer_track_id
        && local_track_missing_from_remote
        && matches!(renderer_playing_state, Some(PLAYING_STATE_STOPPED | PLAYING_STATE_UNKNOWN))
    {
        log::info!(
            "[QConnect] materialize_remote_queue: stopping stale local playback track {:?} after remote queue replacement",
            current_playback_track_id
        );
        let _ = bridge.stop();
    }

    Ok(())
}

fn dedupe_track_ids(queue_state: &QConnectQueueState) -> Vec<u64> {
    let mut unique = Vec::with_capacity(queue_state.queue_items.len());
    for item in &queue_state.queue_items {
        if !unique.contains(&item.track_id) {
            unique.push(item.track_id);
        }
    }
    unique
}

fn resolve_remote_start_index(
    queue_state: &QConnectQueueState,
    renderer_queue_item_id: Option<u64>,
    renderer_track_id: Option<u64>,
) -> Option<usize> {
    if let Some(queue_item_id) = renderer_queue_item_id {
        if let Some(index) = queue_state
            .queue_items
            .iter()
            .position(|item| item.queue_item_id == queue_item_id)
        {
            return Some(index);
        }
    }

    if let Some(track_id) = renderer_track_id {
        if let Some(index) = queue_state
            .queue_items
            .iter()
            .position(|item| item.track_id == track_id)
        {
            return Some(index);
        }
    }

    None
}

async fn align_corebridge_queue_cursor(bridge: &CoreBridge, track_id: u64) -> Result<(), String> {
    let (tracks, current_index) = bridge.get_all_queue_tracks().await;
    log::info!(
        "[QConnect] align_corebridge_queue_cursor: track_id={track_id} queue_len={} current_index={:?}",
        tracks.len(),
        current_index
    );
    if let Some(target_index) = tracks.iter().position(|track| track.id == track_id) {
        if current_index != Some(target_index) {
            log::info!(
                "[QConnect] align_corebridge_queue_cursor: moving cursor from {:?} to {target_index}",
                current_index
            );
            let _ = bridge.play_index(target_index).await;
        }
        return Ok(());
    }

    log::info!(
        "[QConnect] align_corebridge_queue_cursor: track {track_id} not in queue, fetching and creating single-track queue"
    );
    let track = bridge
        .get_track(track_id)
        .await
        .map_err(|err| format!("fetch current remote track {track_id}: {err}"))?;
    let queue_track = model_track_to_core_queue_track(&track);
    bridge.set_queue(vec![queue_track], Some(0)).await;
    Ok(())
}

async fn ensure_remote_track_loaded(bridge: &CoreBridge, track_id: u64) -> Result<(), String> {
    if bridge.get_playback_state().track_id == track_id {
        return Ok(());
    }

    let stream_url = bridge
        .get_stream_url(track_id, Quality::UltraHiRes)
        .await
        .map_err(|err| format!("resolve stream url for remote track {track_id}: {err}"))?;
    let audio_data = download_remote_audio(&stream_url.url).await?;

    bridge
        .player()
        .play_data(audio_data, track_id)
        .map_err(|err| format!("play remote track {track_id}: {err}"))?;

    Ok(())
}

async fn download_remote_audio(url: &str) -> Result<Vec<u8>, String> {
    let response = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|err| format!("download remote audio request failed: {err}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "download remote audio failed with status {}",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("read remote audio bytes failed: {err}"))?;
    Ok(bytes.to_vec())
}

fn model_track_to_core_queue_track(track: &Track) -> QueueTrack {
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
        album_id,
        artist_id,
        streamable: track.streamable,
        source: Some(QCONNECT_REMOTE_QUEUE_SOURCE.to_string()),
        parental_warning: track.parental_warning,
    }
}

fn normalize_volume_to_fraction(volume: i32) -> f32 {
    volume.clamp(0, 100) as f32 / 100.0
}

async fn bootstrap_remote_presence(
    app: &Arc<QconnectApp<NativeWsTransport, TauriQconnectEventSink>>,
) -> Result<(), String> {
    let device_info = default_qconnect_device_info();

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
        .build_queue_command(QueueCommandType::CtrlSrvrAskForQueueState, ask_queue_payload)
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

    log::info!("[QConnect] Deferred renderer join complete for session {session_uuid}");
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

fn default_qconnect_device_info() -> QconnectDeviceInfoPayload {
    let friendly_name = std::env::var("QBZ_QCONNECT_DEVICE_NAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_QCONNECT_DEVICE_NAME.to_string());
    let brand = std::env::var("QBZ_QCONNECT_DEVICE_BRAND")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_QCONNECT_DEVICE_BRAND.to_string());
    let model = std::env::var("QBZ_QCONNECT_DEVICE_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_QCONNECT_DEVICE_MODEL.to_string());
    let software_version = std::env::var("QBZ_QCONNECT_SOFTWARE_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "{DEFAULT_QCONNECT_SOFTWARE_PREFIX}/{}",
                env!("CARGO_PKG_VERSION")
            )
        });
    let device_type = std::env::var("QBZ_QCONNECT_DEVICE_TYPE")
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .unwrap_or(DEFAULT_QCONNECT_DEVICE_TYPE);

    QconnectDeviceInfoPayload {
        device_uuid: Some(resolve_qconnect_device_uuid()),
        friendly_name: Some(friendly_name),
        brand: Some(brand),
        model: Some(model),
        serial_number: None,
        device_type: Some(device_type),
        capabilities: Some(QconnectDeviceCapabilitiesPayload {
            min_audio_quality: Some(AUDIO_QUALITY_MP3),
            max_audio_quality: Some(AUDIO_QUALITY_HIRES_LEVEL2),
            volume_remote_control: Some(VOLUME_REMOTE_CONTROL_ALLOWED),
        }),
        software_version: Some(software_version),
    }
}

fn resolve_qconnect_device_uuid() -> String {
    if let Some(explicit) = std::env::var("QBZ_QCONNECT_DEVICE_UUID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return explicit;
    }

    QCONNECT_DEVICE_UUID
        .get_or_init(|| Uuid::new_v4().to_string())
        .clone()
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
            log::info!("[QConnect] send_command_with_admission: sent uuid={uuid}");
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

    // If we still don't have a current_queue_item_id but the frontend sent
    // a track_id, try to resolve it from the QConnect queue state.
    if resolved_current_qid.is_none() {
        if let Some(track_id) = current_track_id {
            if track_id > 0 {
                if let Some(qid) = service
                    .lookup_queue_item_id_by_track_id(track_id as u64)
                    .await
                {
                    resolved_current_qid = i32::try_from(qid).ok();
                    resolution_strategy = "queue_lookup_by_track_id".to_string();
                }
            }
        }
    }

    if resolved_current_qid.is_none() && requested_current_qid.is_none() {
        resolution_strategy = "unresolved".to_string();
    }

    let queue_version = service.get_queue_version().await;

    log::debug!(
        "[QConnect/Report] Periodic state report: playing={} pos={:?} dur={:?} qid={:?} next_qid={:?} track_id={:?} qv={}.{}",
        playing_state, current_position, duration,
        resolved_current_qid, resolved_next_qid, current_track_id,
        queue_version.major, queue_version.minor
    );

    // NOTE: We intentionally OMIT current_queue_item_id and next_queue_item_id
    // from periodic state reports. The server validates these IDs against its
    // queue state and rejects reports when IDs don't match (returns "Current track
    // not found in queue nor autoplay"). This happens consistently after QBZ-initiated
    // queue loads where the server assigns non-standard queue_item_ids.
    // The server already knows the current track from SET_STATE commands and adds
    // the correct queue_item_id when forwarding to controllers.
    let report = RendererReport::new(
        RendererReportType::RndrSrvrStateUpdated,
        Uuid::new_v4().to_string(),
        queue_version,
        serde_json::json!({
            "playing_state": playing_state,
            "buffer_state": BUFFER_STATE_OK,
            "current_position": current_position,
            "duration": duration,
            "current_queue_item_id": null,
            "next_queue_item_id": null,
            "queue_version": {
                "major": queue_version.major,
                "minor": queue_version.minor
            }
        }),
    );

    if let Err(err) = service.send_renderer_report(report).await {
        log::warn!("[QConnect] Failed to report playback state: {err}");
    }

    if let Err(err) = app_handle.emit(
        "qconnect:renderer_report_debug",
        &QconnectRendererReportDebugEvent {
            requested_current_queue_item_id: requested_current_qid,
            requested_next_queue_item_id: requested_next_qid,
            resolved_current_queue_item_id: resolved_current_qid,
            resolved_next_queue_item_id: resolved_next_qid,
            sent_current_queue_item_id: None,
            sent_next_queue_item_id: None,
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

async fn resolve_transport_config(
    options: QconnectConnectOptions,
    app_state: &AppState,
) -> Result<WsTransportConfig, String> {
    let mut endpoint_url = normalize_opt_string(options.endpoint_url)
        .or_else(|| normalize_opt_string(std::env::var("QBZ_QCONNECT_WS_ENDPOINT").ok()));

    let mut jwt_qws = normalize_opt_string(options.jwt_qws)
        .or_else(|| normalize_opt_string(std::env::var("QBZ_QCONNECT_JWT_QWS").ok()))
        .or_else(|| normalize_opt_string(std::env::var("QBZ_QCONNECT_JWT").ok()));

    if endpoint_url.is_none() || jwt_qws.is_none() {
        match fetch_qconnect_transport_credentials(app_state).await {
            Ok((discovered_endpoint, discovered_jwt_qws)) => {
                endpoint_url = endpoint_url.or(discovered_endpoint);
                jwt_qws = jwt_qws.or(discovered_jwt_qws);
            }
            Err(err) if endpoint_url.is_some() => {
                log::warn!(
                    "[QConnect] qws/createToken auto-discovery failed, using provided endpoint: {err}"
                );
            }
            Err(err) => {
                return Err(format!(
                    "QConnect endpoint_url is required (arg or QBZ_QCONNECT_WS_ENDPOINT). Auto-discovery via qws/createToken failed: {err}"
                ));
            }
        }
    }

    let endpoint_url = endpoint_url.ok_or_else(|| {
        "QConnect endpoint_url is required (arg or QBZ_QCONNECT_WS_ENDPOINT)".to_string()
    })?;

    let subscribe_channels = if let Some(channels) = options.subscribe_channels_hex {
        parse_subscribe_channels(channels)?
    } else if let Ok(raw) = std::env::var("QBZ_QCONNECT_SUBSCRIBE_CHANNELS_HEX") {
        let channels: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect();
        parse_subscribe_channels(channels)?
    } else {
        // Default QConnect channels: connectionId(0x01), backend(0x02), controllers(0x03)
        vec![vec![0x01], vec![0x02], vec![0x03]]
    };

    let mut config = WsTransportConfig::default();
    config.endpoint_url = endpoint_url;
    config.jwt_qws = jwt_qws;
    config.reconnect_backoff_ms = options
        .reconnect_backoff_ms
        .unwrap_or(config.reconnect_backoff_ms);
    config.reconnect_backoff_max_ms = options
        .reconnect_backoff_max_ms
        .unwrap_or(config.reconnect_backoff_max_ms);
    config.connect_timeout_ms = options
        .connect_timeout_ms
        .unwrap_or(config.connect_timeout_ms);
    config.keepalive_interval_ms = options
        .keepalive_interval_ms
        .unwrap_or(config.keepalive_interval_ms);
    config.qcloud_proto = options.qcloud_proto.unwrap_or(config.qcloud_proto);
    config.subscribe_channels = subscribe_channels;

    Ok(config)
}

async fn fetch_qconnect_transport_credentials(
    app_state: &AppState,
) -> Result<(Option<String>, Option<String>), String> {
    let client = app_state.client.read().await.clone();
    let app_id = client
        .app_id()
        .await
        .map_err(|err| format!("qws/createToken requires initialized API client: {err}"))?;
    let user_auth_token = client
        .auth_token()
        .await
        .map_err(|err| format!("qws/createToken requires authenticated user: {err}"))?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "X-App-Id",
        reqwest::header::HeaderValue::from_str(&app_id)
            .map_err(|_| "invalid X-App-Id header value".to_string())?,
    );
    headers.insert(
        "X-User-Auth-Token",
        reqwest::header::HeaderValue::from_str(&user_auth_token)
            .map_err(|_| "invalid X-User-Auth-Token header value".to_string())?,
    );

    let url = crate::api::endpoints::build_url(QCONNECT_QWS_CREATE_TOKEN_PATH);
    let response = client
        .get_http()
        .post(&url)
        .headers(headers)
        .form(&[
            ("jwt", QCONNECT_QWS_TOKEN_KIND),
            ("user_auth_token_needed", "true"),
            ("strong_auth_needed", "true"),
        ])
        .send()
        .await
        .map_err(|err| format!("qws/createToken HTTP request failed: {err}"))?;

    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|err| format!("qws/createToken response decode failed: {err}"))?;

    if !status.is_success() {
        let preview = serde_json::to_string(&payload)
            .unwrap_or_else(|_| "<unserializable>".to_string())
            .chars()
            .take(300)
            .collect::<String>();
        return Err(format!("qws/createToken status {status}: {preview}"));
    }

    let jwt_qws_payload = payload
        .get("jwt_qws")
        .ok_or_else(|| "qws/createToken response missing jwt_qws payload".to_string())?;

    let endpoint_url = normalize_opt_string(
        jwt_qws_payload
            .get("endpoint")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    );
    let jwt_qws = normalize_opt_string(
        jwt_qws_payload
            .get("jwt")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    );

    if endpoint_url.is_none() {
        return Err("qws/createToken response missing jwt_qws.endpoint".to_string());
    }

    Ok((endpoint_url, jwt_qws))
}

fn hex_preview(data: &[u8], max_bytes: usize) -> String {
    let take = data.len().min(max_bytes);
    let hex: String = data[..take]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("");
    if data.len() > max_bytes {
        format!("{hex}...({}B total)", data.len())
    } else {
        hex
    }
}

fn normalize_opt_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn parse_subscribe_channels(items: Vec<String>) -> Result<Vec<Vec<u8>>, String> {
    items
        .into_iter()
        .map(|item| decode_hex_channel(&item))
        .collect()
}

fn decode_hex_channel(raw: &str) -> Result<Vec<u8>, String> {
    let normalized = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    if normalized.is_empty() {
        return Err("empty subscribe channel hex value".to_string());
    }

    let needs_padding = normalized.len() % 2 != 0;
    let value = if needs_padding {
        format!("0{normalized}")
    } else {
        normalized.to_string()
    };

    let mut bytes = Vec::with_capacity(value.len() / 2);
    let chars: Vec<char> = value.chars().collect();

    for idx in (0..chars.len()).step_by(2) {
        let pair = [chars[idx], chars[idx + 1]];
        let hex = pair.iter().collect::<String>();
        let byte = u8::from_str_radix(&hex, 16)
            .map_err(|_| format!("invalid subscribe channel hex byte '{hex}' in '{raw}'"))?;
        bytes.push(byte);
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{
        decode_hex_channel, normalize_volume_to_fraction, parse_subscribe_channels,
        QconnectHandoffIntent, QconnectOutboundCommandType, QconnectTrackOrigin,
    };
    use qconnect_app::{resolve_handoff_intent, QueueCommandType};

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
}
