use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use qbz_models::{Quality, QueueTrack, RepeatMode, Track};
use qconnect_app::{
    evaluate_remote_queue_admission, resolve_handoff_intent, HandoffIntent, QConnectQueueState,
    QConnectRendererState, QconnectApp, QconnectAppEvent, QconnectEventSink, QueueCommandType,
    RendererCommand, RendererReport, RendererReportType, TrackOrigin,
};
use qconnect_core::QueueItem;
use qconnect_transport_ws::{NativeWsTransport, WsTransportConfig};
use serde::{Deserialize, Serialize};
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

const PLAYING_STATE_UNKNOWN: i32 = 0;
const PLAYING_STATE_STOPPED: i32 = 1;
const PLAYING_STATE_PLAYING: i32 = 2;
const PLAYING_STATE_PAUSED: i32 = 3;
const BUFFER_STATE_OK: i32 = 2;
const QCONNECT_QWS_TOKEN_KIND: &str = "jwt_qws";
const QCONNECT_QWS_CREATE_TOKEN_PATH: &str = "/qws/createToken";
const DEFAULT_QCONNECT_DEVICE_BRAND: &str = "QBZ";
const DEFAULT_QCONNECT_DEVICE_MODEL: &str = "QBZ";
const DEFAULT_QCONNECT_DEVICE_TYPE: i32 = 5; // computer
const DEFAULT_QCONNECT_SOFTWARE_PREFIX: &str = "qbz";
const QCONNECT_REMOTE_QUEUE_SOURCE: &str = "qobuz_connect_remote";
// AudioQuality enum: 0=unknown, 1=mp3, 2=cd, 3=hires_l1, 4=hires_l2(192k), 5=hires_l3(384k)
const AUDIO_QUALITY_UNKNOWN: i32 = 0;
const AUDIO_QUALITY_MP3: i32 = 1;
const AUDIO_QUALITY_CD: i32 = 2;
const AUDIO_QUALITY_HIRES_LEVEL1: i32 = 3;
const AUDIO_QUALITY_HIRES_LEVEL2: i32 = 4;
const AUDIO_QUALITY_HIRES_LEVEL3: i32 = 5;
const DEFAULT_QCONNECT_CHANNEL_COUNT: i32 = 2;
// VolumeRemoteControl enum: 0=unknown, 1=not_allowed, 2=allowed
const VOLUME_REMOTE_CONTROL_ALLOWED: i32 = 2;
// JoinSessionReason: 0=unknown, 1=controller_request, 2=reconnection
const JOIN_SESSION_REASON_CONTROLLER_REQUEST: i32 = 1;
const QCONNECT_PLAY_TRACK_HANDOFF_WAIT_MS: u64 = 1_500;
const QCONNECT_PLAY_TRACK_HANDOFF_POLL_MS: u64 = 50;
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

#[derive(Debug, Clone, Default)]
pub struct QconnectVisibleQueueProjection {
    pub current_track: Option<QueueItem>,
    pub upcoming_tracks: Vec<QueueItem>,
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
    last_materialized_start_index: Option<usize>,
    last_materialized_core_shuffle_order: Option<Vec<usize>>,
    last_reported_file_audio_quality: Option<QconnectFileAudioQualitySnapshot>,
    last_applied_queue_state: Option<QConnectQueueState>,
    last_remote_queue_state: Option<QConnectQueueState>,
    session_loop_mode: Option<i32>,
    /// Session topology — stored from session management events (types 81-87).
    session: QconnectSessionState,
    session_renderer_states: HashMap<i32, QconnectSessionRendererState>,
}

#[derive(Debug, Clone, Default)]
struct QconnectSessionRendererState {
    active: Option<bool>,
    playing_state: Option<i32>,
    current_position_ms: Option<u64>,
    current_queue_item_id: Option<u64>,
    volume: Option<i32>,
    muted: Option<bool>,
    max_audio_quality: Option<i32>,
    loop_mode: Option<i32>,
    shuffle_mode: Option<bool>,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QconnectFileAudioQualitySnapshot {
    sampling_rate: i32,
    bit_depth: i32,
    nb_channels: i32,
    audio_quality: i32,
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

fn queue_state_needs_materialization(
    previous: Option<&QConnectQueueState>,
    next: &QConnectQueueState,
) -> bool {
    let Some(previous) = previous else {
        return true;
    };

    previous.version != next.version
        || previous.queue_items != next.queue_items
        || previous.shuffle_mode != next.shuffle_mode
        || previous.shuffle_order != next.shuffle_order
        || previous.autoplay_mode != next.autoplay_mode
        || previous.autoplay_loading != next.autoplay_loading
        || previous.autoplay_items != next.autoplay_items
}

fn qconnect_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn qconnect_repeat_mode_from_loop_mode(loop_mode: i32) -> Option<RepeatMode> {
    // QConnect protocol loop mode values:
    // 1 = off, 2 = repeat one, 3 = repeat all.
    match loop_mode {
        0 | 1 => Some(RepeatMode::Off),
        2 => Some(RepeatMode::One),
        3 => Some(RepeatMode::All),
        _ => None,
    }
}

async fn apply_remote_loop_mode_to_corebridge(
    core_bridge: &Arc<RwLock<Option<CoreBridge>>>,
    loop_mode: i32,
) -> Result<(), String> {
    let repeat_mode = qconnect_repeat_mode_from_loop_mode(loop_mode)
        .ok_or_else(|| format!("unsupported qconnect loop mode: {loop_mode}"))?;

    let bridge_guard = core_bridge.read().await;
    let Some(bridge) = bridge_guard.as_ref() else {
        return Err("core bridge is not initialized yet".to_string());
    };

    bridge.set_repeat_mode(repeat_mode).await;
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSessionState {
    pub session_uuid: Option<String>,
    pub active_renderer_id: Option<i32>,
    pub local_renderer_id: Option<i32>,
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
    report_queue_item_ids: bool,
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
    pub device_uuid: Option<String>,
    pub friendly_name: Option<String>,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub device_type: Option<i32>,
}

fn refresh_local_renderer_id(session: &mut QconnectSessionState) {
    let local_device_uuid = resolve_qconnect_device_uuid();
    if let Some(renderer_id) = session
        .renderers
        .iter()
        .find(|renderer| renderer.device_uuid.as_deref() == Some(local_device_uuid.as_str()))
        .map(|renderer| renderer.renderer_id)
    {
        session.local_renderer_id = Some(renderer_id);
        return;
    }

    let local_device_info = default_qconnect_device_info();
    let local_friendly_name = local_device_info.friendly_name.as_deref();
    let local_brand = local_device_info.brand.as_deref();
    let local_model = local_device_info.model.as_deref();
    let local_device_type = local_device_info.device_type;

    // Some server ADD_RENDERER payloads omit device_uuid for the local renderer.
    // Fall back to a unique device fingerprint so controller-side handoff logic
    // can still distinguish local vs peer renderers.
    if let Some(renderer_id) = find_unique_renderer_id(session, |renderer| {
        renderer.friendly_name.as_deref() == local_friendly_name
            && renderer.brand.as_deref() == local_brand
            && renderer.model.as_deref() == local_model
            && renderer.device_type == local_device_type
    }) {
        session.local_renderer_id = Some(renderer_id);
        return;
    }

    if let Some(renderer_id) = find_unique_renderer_id(session, |renderer| {
        renderer.friendly_name.as_deref() == local_friendly_name
            && renderer.device_type == local_device_type
    }) {
        session.local_renderer_id = Some(renderer_id);
        return;
    }

    session.local_renderer_id = None;
}

fn normalize_active_renderer_id(value: Option<i64>) -> Option<i32> {
    value
        .filter(|renderer_id| *renderer_id >= 0)
        .and_then(|renderer_id| i32::try_from(renderer_id).ok())
}

fn is_peer_renderer_active(session: &QconnectSessionState) -> bool {
    match (session.active_renderer_id, session.local_renderer_id) {
        (Some(active_renderer_id), Some(local_renderer_id)) => {
            active_renderer_id != local_renderer_id
        }
        _ => false,
    }
}

fn is_local_renderer_active(session: &QconnectSessionState) -> bool {
    match (session.active_renderer_id, session.local_renderer_id) {
        (Some(active_renderer_id), Some(local_renderer_id)) => {
            active_renderer_id == local_renderer_id
        }
        _ => false,
    }
}

fn sync_session_renderer_active_flags(state: &mut QconnectRemoteSyncState) {
    for (renderer_id, renderer_state) in &mut state.session_renderer_states {
        renderer_state.active = state
            .session
            .active_renderer_id
            .map(|active_renderer_id| active_renderer_id == *renderer_id);
    }
}

fn ensure_session_renderer_state(
    state: &mut QconnectRemoteSyncState,
    renderer_id: i32,
) -> &mut QconnectSessionRendererState {
    let active = state
        .session
        .active_renderer_id
        .map(|active_renderer_id| active_renderer_id == renderer_id);
    state
        .session_renderer_states
        .entry(renderer_id)
        .or_insert_with(|| QconnectSessionRendererState {
            active,
            ..Default::default()
        })
}

fn find_unique_renderer_id(
    session: &QconnectSessionState,
    predicate: impl Fn(&QconnectRendererInfo) -> bool,
) -> Option<i32> {
    let mut matches = session
        .renderers
        .iter()
        .filter(|renderer| predicate(renderer))
        .map(|renderer| renderer.renderer_id);

    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }

    Some(first)
}

fn queue_item_snapshot_for_cursor(
    queue: &QConnectQueueState,
    cursor: QconnectOrderedQueueCursor,
) -> Option<QueueItem> {
    match cursor {
        QconnectOrderedQueueCursor::Queue(index) => {
            queue.queue_items.get(index).cloned().map(|mut item| {
                item.queue_item_id = normalize_current_queue_item_id_from_queue_state(queue, index);
                item
            })
        }
        QconnectOrderedQueueCursor::Autoplay(index) => queue.autoplay_items.get(index).cloned(),
    }
}

fn build_session_renderer_snapshot(
    queue: &QConnectQueueState,
    renderer_state: Option<&QconnectSessionRendererState>,
    session_loop_mode: Option<i32>,
) -> QConnectRendererState {
    let renderer_state = renderer_state.cloned().unwrap_or_default();
    let cursors = ordered_queue_cursors(queue);
    let current_index =
        find_cursor_index_by_queue_item_id(&cursors, queue, renderer_state.current_queue_item_id);
    let current_track =
        current_index.and_then(|index| queue_item_snapshot_for_cursor(queue, cursors[index]));
    let next_track = current_index
        .and_then(|index| cursors.get(index + 1).copied())
        .and_then(|cursor| queue_item_snapshot_for_cursor(queue, cursor));

    QConnectRendererState {
        active: renderer_state.active,
        playing_state: renderer_state.playing_state,
        current_position_ms: renderer_state.current_position_ms,
        current_track,
        next_track,
        volume: renderer_state.volume,
        volume_delta: None,
        muted: renderer_state.muted,
        max_audio_quality: renderer_state.max_audio_quality,
        loop_mode: renderer_state.loop_mode.or(session_loop_mode),
        shuffle_mode: renderer_state.shuffle_mode,
        updated_at_ms: renderer_state.updated_at_ms,
    }
}

fn build_effective_renderer_snapshot(
    queue: &QConnectQueueState,
    base_renderer_state: &QConnectRendererState,
    session_renderer_state: Option<&QconnectSessionRendererState>,
    session_loop_mode: Option<i32>,
) -> QConnectRendererState {
    let mut renderer_snapshot = base_renderer_state.clone();

    if let Some(session_renderer_state) = session_renderer_state {
        if let Some(active) = session_renderer_state.active {
            renderer_snapshot.active = Some(active);
        }
        if let Some(playing_state) = session_renderer_state.playing_state {
            renderer_snapshot.playing_state = Some(playing_state);
        }
        if let Some(current_position_ms) = session_renderer_state.current_position_ms {
            renderer_snapshot.current_position_ms = Some(current_position_ms);
        }
        if let Some(volume) = session_renderer_state.volume {
            renderer_snapshot.volume = Some(volume);
        }
        if let Some(muted) = session_renderer_state.muted {
            renderer_snapshot.muted = Some(muted);
        }
        if let Some(max_audio_quality) = session_renderer_state.max_audio_quality {
            renderer_snapshot.max_audio_quality = Some(max_audio_quality);
        }
        if let Some(loop_mode) = session_renderer_state.loop_mode.or(session_loop_mode) {
            renderer_snapshot.loop_mode = Some(loop_mode);
        }
        if let Some(shuffle_mode) = session_renderer_state.shuffle_mode {
            renderer_snapshot.shuffle_mode = Some(shuffle_mode);
        }
        if session_renderer_state.updated_at_ms > 0 {
            renderer_snapshot.updated_at_ms = session_renderer_state.updated_at_ms;
        }

        if session_renderer_state.current_queue_item_id.is_some() {
            let session_snapshot = build_session_renderer_snapshot(
                queue,
                Some(session_renderer_state),
                session_loop_mode,
            );
            if session_snapshot.current_track.is_some() {
                renderer_snapshot.current_track = session_snapshot.current_track;
                renderer_snapshot.next_track = session_snapshot.next_track;
            }
        }
    } else if let Some(loop_mode) = session_loop_mode {
        renderer_snapshot.loop_mode = Some(loop_mode);
    }

    renderer_snapshot
}

fn build_visible_queue_projection(
    queue: &QConnectQueueState,
    renderer: &QConnectRendererState,
) -> QconnectVisibleQueueProjection {
    let cursors = ordered_queue_cursors(queue);

    let current_index = find_cursor_index_by_queue_item_id(
        &cursors,
        queue,
        renderer
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id),
    )
    .or_else(|| {
        find_cursor_index_by_track_id(
            &cursors,
            queue,
            renderer.current_track.as_ref().map(|item| item.track_id),
        )
    });

    let next_index = find_cursor_index_by_queue_item_id(
        &cursors,
        queue,
        renderer.next_track.as_ref().map(|item| item.queue_item_id),
    )
    .or_else(|| {
        find_cursor_index_by_track_id(
            &cursors,
            queue,
            renderer.next_track.as_ref().map(|item| item.track_id),
        )
    });

    let (current_track, start_index) = if let Some(index) = current_index {
        (
            queue_item_snapshot_for_cursor(queue, cursors[index]),
            index.saturating_add(1),
        )
    } else if let Some(index) = next_index {
        let inferred_current = index
            .checked_sub(1)
            .and_then(|current_index| cursors.get(current_index).copied())
            .and_then(|cursor| queue_item_snapshot_for_cursor(queue, cursor));
        (inferred_current, index)
    } else {
        (None, 0)
    };

    let upcoming_tracks = cursors
        .into_iter()
        .skip(start_index)
        .filter_map(|cursor| queue_item_snapshot_for_cursor(queue, cursor))
        .collect();

    QconnectVisibleQueueProjection {
        current_track,
        upcoming_tracks,
    }
}

fn cache_renderer_snapshot(
    sync_state: &mut QconnectRemoteSyncState,
    renderer_snapshot: &QConnectRendererState,
) {
    sync_state.last_renderer_queue_item_id = renderer_snapshot
        .current_track
        .as_ref()
        .map(|item| item.queue_item_id);
    sync_state.last_renderer_next_queue_item_id = renderer_snapshot
        .next_track
        .as_ref()
        .map(|item| item.queue_item_id);
    sync_state.last_renderer_track_id = renderer_snapshot
        .current_track
        .as_ref()
        .map(|item| item.track_id);
    sync_state.last_renderer_next_track_id = renderer_snapshot
        .next_track
        .as_ref()
        .map(|item| item.track_id);
    sync_state.last_renderer_playing_state = renderer_snapshot.playing_state;
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
                cache_renderer_snapshot(&mut sync_state, renderer_state);
            }
            QconnectAppEvent::QueueUpdated(queue_state) => {
                log::debug!(
                    "[QConnect] QueueUpdated: items={} shuffle_mode={} shuffle_order={:?} version={}.{}",
                    queue_state.queue_items.len(),
                    queue_state.shuffle_mode,
                    queue_state.shuffle_order,
                    queue_state.version.major,
                    queue_state.version.minor,
                );
                if queue_state.shuffle_mode {
                    let valid = queue_state.shuffle_order.as_ref()
                        .map(|o| is_valid_ordered_queue_shuffle_order(o, queue_state.queue_items.len()))
                        .unwrap_or(false);
                    log::debug!(
                        "[QConnect] shuffle_order valid={} items_len={} order_len={:?}",
                        valid,
                        queue_state.queue_items.len(),
                        queue_state.shuffle_order.as_ref().map(|o| o.len()),
                    );
                }
                {
                    let mut sync_state = self.sync_state.lock().await;
                    sync_state.last_remote_queue_state = Some(queue_state.clone());
                }
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
                log::info!("[QConnect] Renderer command applied: {:?}", command);
                if let Err(err) = apply_renderer_command_to_corebridge(
                    &self.core_bridge,
                    &self.sync_state,
                    command,
                    state,
                )
                .await
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
        let mut remote_projection_renderer_id: Option<i32> = None;
        let mut sync_local_playback = false;
        let mut apply_loop_mode: Option<i32> = None;
        let mut state = self.sync_state.lock().await;
        match message_type {
            "MESSAGE_TYPE_SRVR_CTRL_SESSION_STATE" => {
                if let Some(uuid) = payload.get("session_uuid").and_then(Value::as_str) {
                    state.session.session_uuid = Some(uuid.to_string());
                }
                state.session.active_renderer_id = normalize_active_renderer_id(
                    payload.get("active_renderer_id").and_then(Value::as_i64),
                );
                if let Some(loop_mode) = payload
                    .get("loop_mode")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                {
                    state.session_loop_mode = Some(loop_mode);
                    apply_loop_mode = Some(loop_mode);
                }
                if let (Some(active_renderer_id), Some(loop_mode)) =
                    (state.session.active_renderer_id, state.session_loop_mode)
                {
                    let renderer_state =
                        ensure_session_renderer_state(&mut state, active_renderer_id);
                    renderer_state.loop_mode = Some(loop_mode);
                    renderer_state.updated_at_ms = qconnect_now_ms();
                }
                sync_session_renderer_active_flags(&mut state);
                sync_local_playback = true;
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
                            device_uuid: device_info
                                .and_then(|d| d.get("device_uuid"))
                                .and_then(Value::as_str)
                                .map(String::from),
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
                        refresh_local_renderer_id(&mut state.session);
                    }
                    let _ = ensure_session_renderer_state(&mut state, renderer_id);
                    sync_session_renderer_active_flags(&mut state);
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
                        if let Some(device_uuid) = device_info
                            .and_then(|d| d.get("device_uuid"))
                            .and_then(Value::as_str)
                        {
                            existing.device_uuid = Some(device_uuid.to_string());
                        }
                        if let Some(name) = device_info
                            .and_then(|d| d.get("friendly_name"))
                            .and_then(Value::as_str)
                        {
                            existing.friendly_name = Some(name.to_string());
                        }
                        if let Some(brand) = device_info
                            .and_then(|d| d.get("brand"))
                            .and_then(Value::as_str)
                        {
                            existing.brand = Some(brand.to_string());
                        }
                        if let Some(model) = device_info
                            .and_then(|d| d.get("model"))
                            .and_then(Value::as_str)
                        {
                            existing.model = Some(model.to_string());
                        }
                        if let Some(device_type) = device_info
                            .and_then(|d| d.get("device_type"))
                            .and_then(Value::as_i64)
                        {
                            existing.device_type = Some(device_type as i32);
                        }
                        refresh_local_renderer_id(&mut state.session);
                    }
                    let _ = ensure_session_renderer_state(&mut state, renderer_id);
                    sync_session_renderer_active_flags(&mut state);
                }
            }
            "MESSAGE_TYPE_SRVR_CTRL_REMOVE_RENDERER" => {
                if let Some(renderer_id) = payload.get("renderer_id").and_then(Value::as_i64) {
                    let renderer_id = renderer_id as i32;
                    state
                        .session
                        .renderers
                        .retain(|r| r.renderer_id != renderer_id);
                    state.session_renderer_states.remove(&renderer_id);
                    refresh_local_renderer_id(&mut state.session);
                    sync_session_renderer_active_flags(&mut state);
                }
            }
            "MESSAGE_TYPE_SRVR_CTRL_ACTIVE_RENDERER_CHANGED" => {
                state.session.active_renderer_id = normalize_active_renderer_id(
                    payload.get("active_renderer_id").and_then(Value::as_i64),
                );
                if let (Some(active_renderer_id), Some(loop_mode)) =
                    (state.session.active_renderer_id, state.session_loop_mode)
                {
                    let renderer_state =
                        ensure_session_renderer_state(&mut state, active_renderer_id);
                    renderer_state.loop_mode = Some(loop_mode);
                    renderer_state.updated_at_ms = qconnect_now_ms();
                }
                apply_loop_mode = state.session_loop_mode;
                sync_session_renderer_active_flags(&mut state);
                remote_projection_renderer_id = state.session.active_renderer_id;
                sync_local_playback = true;
            }
            "MESSAGE_TYPE_SRVR_CTRL_RENDERER_STATE_UPDATED" => {
                let Some(renderer_id) = payload.get("renderer_id").and_then(Value::as_i64) else {
                    return;
                };
                let player_state = payload.get("player_state");
                let renderer_state = ensure_session_renderer_state(&mut state, renderer_id as i32);

                if let Some(playing_state) = player_state
                    .and_then(|value| value.get("playing_state"))
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                {
                    renderer_state.playing_state = Some(playing_state);
                }

                if let Some(current_position_ms) = player_state
                    .and_then(|value| value.get("current_position"))
                    .and_then(Value::as_i64)
                    .and_then(|value| u64::try_from(value).ok())
                {
                    renderer_state.current_position_ms = Some(current_position_ms);
                }

                if let Some(current_queue_item_id) = player_state
                    .and_then(|value| value.get("current_queue_item_id"))
                    .and_then(Value::as_i64)
                {
                    renderer_state.current_queue_item_id =
                        u64::try_from(current_queue_item_id).ok();
                }

                renderer_state.updated_at_ms = qconnect_now_ms();
                remote_projection_renderer_id = Some(renderer_id as i32);
                sync_local_playback = true;
            }
            "MESSAGE_TYPE_SRVR_CTRL_VOLUME_CHANGED" => {
                let Some(renderer_id) = payload.get("renderer_id").and_then(Value::as_i64) else {
                    return;
                };
                let Some(volume) = payload
                    .get("volume")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                else {
                    return;
                };

                let renderer_state = ensure_session_renderer_state(&mut state, renderer_id as i32);
                renderer_state.volume = Some(volume);
                renderer_state.updated_at_ms = qconnect_now_ms();
            }
            "MESSAGE_TYPE_SRVR_CTRL_VOLUME_MUTED" => {
                let Some(renderer_id) = payload.get("renderer_id").and_then(Value::as_i64) else {
                    return;
                };
                let Some(muted) = payload.get("value").and_then(Value::as_bool) else {
                    return;
                };

                let renderer_state = ensure_session_renderer_state(&mut state, renderer_id as i32);
                renderer_state.muted = Some(muted);
                renderer_state.updated_at_ms = qconnect_now_ms();
            }
            "MESSAGE_TYPE_SRVR_CTRL_MAX_AUDIO_QUALITY_CHANGED" => {
                let Some(renderer_id) = payload.get("renderer_id").and_then(Value::as_i64) else {
                    return;
                };
                let Some(max_audio_quality) = payload
                    .get("max_audio_quality")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                else {
                    return;
                };

                let renderer_state = ensure_session_renderer_state(&mut state, renderer_id as i32);
                renderer_state.max_audio_quality = Some(max_audio_quality);
                renderer_state.updated_at_ms = qconnect_now_ms();
            }
            "MESSAGE_TYPE_SRVR_CTRL_LOOP_MODE_SET" => {
                let Some(loop_mode) = payload
                    .get("loop_mode")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                else {
                    return;
                };
                state.session_loop_mode = Some(loop_mode);
                apply_loop_mode = Some(loop_mode);
                if let Some(active_renderer_id) = state.session.active_renderer_id {
                    let renderer_state =
                        ensure_session_renderer_state(&mut state, active_renderer_id);
                    renderer_state.loop_mode = Some(loop_mode);
                    renderer_state.updated_at_ms = qconnect_now_ms();
                }
            }
            _ => {}
        }
        drop(state);

        if let Some(loop_mode) = apply_loop_mode {
            if let Err(err) =
                apply_remote_loop_mode_to_corebridge(&self.core_bridge, loop_mode).await
            {
                log::warn!("[QConnect] Failed to apply remote loop mode to CoreBridge: {err}");
            }
        }

        if sync_local_playback {
            self.sync_local_playback_for_renderer_ownership().await;
        }

        if let Some(renderer_id) = remote_projection_renderer_id {
            self.sync_active_renderer_projection(renderer_id).await;
        }
    }

    async fn sync_local_playback_for_renderer_ownership(&self) {
        let peer_renderer_active = {
            let state = self.sync_state.lock().await;
            is_peer_renderer_active(&state.session)
        };
        if !peer_renderer_active {
            return;
        }

        let bridge_guard = self.core_bridge.read().await;
        let Some(bridge) = bridge_guard.as_ref() else {
            return;
        };

        let playback_state = bridge.get_playback_state();
        if playback_state.track_id == 0 {
            return;
        }

        log::info!(
            "[QConnect] Stopping local playback because active renderer is a peer (track_id={})",
            playback_state.track_id
        );
        if let Err(err) = bridge.stop() {
            log::warn!("[QConnect] Failed to stop local playback after renderer handoff: {err}");
        }
    }

    async fn sync_active_renderer_projection(&self, renderer_id: i32) {
        let (queue_state, renderer_state, session_loop_mode, should_align_corebridge) = {
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

        if !should_align_corebridge {
            return;
        }

        let bridge_guard = self.core_bridge.read().await;
        let Some(bridge) = bridge_guard.as_ref() else {
            return;
        };

        let Some(current_track) = renderer_snapshot.current_track.as_ref() else {
            return;
        };

        if let Err(err) = align_corebridge_queue_cursor(bridge, current_track.track_id).await {
            log::warn!("[QConnect] Failed to sync peer renderer cursor into CoreBridge: {err}");
        }
    }
}

pub struct QconnectServiceState {
    inner: Arc<Mutex<QconnectServiceInner>>,
    custom_device_name: Arc<tokio::sync::RwLock<Option<String>>>,
}

impl QconnectServiceState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(QconnectServiceInner::default())),
            custom_device_name: Arc::new(tokio::sync::RwLock::new(None)),
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
                            qconnect_transport_ws::TransportEvent::InboundReceived(_envelope) => {
                                log::info!(
                                    "[QConnect/Transport] <-- InboundReceived (JSON envelope)"
                                );
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
        let (app, session) = {
            let guard = self.inner.lock().await;
            let Some(runtime) = guard.runtime.as_ref() else {
                return Ok(false);
            };
            let session = runtime.sync_state.lock().await.session.clone();
            (Arc::clone(&runtime.app), session)
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

fn resolve_queue_item_ids_from_queue_state(
    queue: &QConnectQueueState,
    track_id: u64,
) -> (Option<u64>, Option<u64>, Option<u64>) {
    if let Some(current_index) = queue
        .queue_items
        .iter()
        .position(|item| item.track_id == track_id)
    {
        let current_qid = normalize_current_queue_item_id_from_queue_state(queue, current_index);
        let next_item = if queue.shuffle_mode {
            queue
                .shuffle_order
                .as_ref()
                .and_then(|order| {
                    order
                        .iter()
                        .position(|queue_index| *queue_index == current_index)
                        .and_then(|order_index| order.get(order_index + 1))
                        .and_then(|queue_index| queue.queue_items.get(*queue_index))
                })
                .or_else(|| queue.queue_items.get(current_index + 1))
                .or_else(|| queue.autoplay_items.first())
        } else {
            queue
                .queue_items
                .get(current_index + 1)
                .or_else(|| queue.autoplay_items.first())
        };

        return (
            Some(current_qid),
            next_item.map(|item| item.queue_item_id),
            next_item.map(|item| item.track_id),
        );
    }

    if let Some(current_index) = queue
        .autoplay_items
        .iter()
        .position(|item| item.track_id == track_id)
    {
        let current_item = &queue.autoplay_items[current_index];
        let next_item = queue.autoplay_items.get(current_index + 1);
        return (
            Some(current_item.queue_item_id),
            next_item.map(|item| item.queue_item_id),
            next_item.map(|item| item.track_id),
        );
    }

    (None, None, None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QconnectOrderedQueueCursor {
    Queue(usize),
    Autoplay(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QconnectRemoteSkipDirection {
    Next,
    Previous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QconnectControllerQueueItemResolution {
    target_queue_item_id: Option<u64>,
    strategy: &'static str,
    queue_index: Option<usize>,
    matched_track_id: Option<u64>,
    matched_queue_item_id: Option<u64>,
}

fn is_valid_ordered_queue_shuffle_order(order: &[usize], track_count: usize) -> bool {
    if order.len() != track_count {
        return false;
    }
    let mut seen = vec![false; track_count];
    for &index in order {
        if index >= track_count || seen[index] {
            return false;
        }
        seen[index] = true;
    }
    true
}

fn ordered_queue_cursors(queue: &QConnectQueueState) -> Vec<QconnectOrderedQueueCursor> {
    let mut cursors = if queue.shuffle_mode {
        queue
            .shuffle_order
            .as_ref()
            .filter(|order| is_valid_ordered_queue_shuffle_order(order, queue.queue_items.len()))
            .map(|order| {
                order
                    .iter()
                    .copied()
                    .map(QconnectOrderedQueueCursor::Queue)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                queue
                    .queue_items
                    .iter()
                    .enumerate()
                    .map(|(index, _)| QconnectOrderedQueueCursor::Queue(index))
                    .collect::<Vec<_>>()
            })
    } else {
        queue
            .queue_items
            .iter()
            .enumerate()
            .map(|(index, _)| QconnectOrderedQueueCursor::Queue(index))
            .collect::<Vec<_>>()
    };

    cursors.extend(
        queue
            .autoplay_items
            .iter()
            .enumerate()
            .map(|(index, _)| QconnectOrderedQueueCursor::Autoplay(index)),
    );
    cursors
}

fn queue_item_track_id_for_cursor(
    queue: &QConnectQueueState,
    cursor: QconnectOrderedQueueCursor,
) -> Option<u64> {
    match cursor {
        QconnectOrderedQueueCursor::Queue(index) => {
            queue.queue_items.get(index).map(|item| item.track_id)
        }
        QconnectOrderedQueueCursor::Autoplay(index) => {
            queue.autoplay_items.get(index).map(|item| item.track_id)
        }
    }
}

fn normalized_queue_item_id_for_cursor(
    queue: &QConnectQueueState,
    cursor: QconnectOrderedQueueCursor,
) -> Option<u64> {
    match cursor {
        QconnectOrderedQueueCursor::Queue(index) => Some(
            normalize_current_queue_item_id_from_queue_state(queue, index),
        ),
        QconnectOrderedQueueCursor::Autoplay(index) => queue
            .autoplay_items
            .get(index)
            .map(|item| item.queue_item_id),
    }
}

fn find_cursor_index_by_queue_item_id(
    cursors: &[QconnectOrderedQueueCursor],
    queue: &QConnectQueueState,
    queue_item_id: Option<u64>,
) -> Option<usize> {
    let queue_item_id = queue_item_id?;
    cursors.iter().position(|cursor| {
        normalized_queue_item_id_for_cursor(queue, *cursor) == Some(queue_item_id)
            || match cursor {
                QconnectOrderedQueueCursor::Queue(index) => queue
                    .queue_items
                    .get(*index)
                    .map(|item| item.queue_item_id == queue_item_id)
                    .unwrap_or(false),
                QconnectOrderedQueueCursor::Autoplay(index) => queue
                    .autoplay_items
                    .get(*index)
                    .map(|item| item.queue_item_id == queue_item_id)
                    .unwrap_or(false),
            }
    })
}

fn find_cursor_index_by_track_id(
    cursors: &[QconnectOrderedQueueCursor],
    queue: &QConnectQueueState,
    track_id: Option<u64>,
) -> Option<usize> {
    let track_id = track_id?;
    cursors
        .iter()
        .position(|cursor| queue_item_track_id_for_cursor(queue, *cursor) == Some(track_id))
}

fn find_cursor_index_by_track_id_before(
    cursors: &[QconnectOrderedQueueCursor],
    queue: &QConnectQueueState,
    track_id: Option<u64>,
    end_exclusive: usize,
) -> Option<usize> {
    let track_id = track_id?;
    if end_exclusive == 0 {
        return None;
    }

    for index in (0..end_exclusive).rev() {
        if queue_item_track_id_for_cursor(queue, cursors[index]) == Some(track_id) {
            return Some(index);
        }
    }

    None
}

fn resolve_current_cursor_index_from_snapshots(
    queue: &QConnectQueueState,
    renderer: &QConnectRendererState,
    cursors: &[QconnectOrderedQueueCursor],
) -> (Option<usize>, &'static str) {
    let current_queue_index = find_cursor_index_by_queue_item_id(
        cursors,
        queue,
        renderer
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id),
    );
    if current_queue_index.is_some() {
        return (
            current_queue_index,
            "renderer_current_queue_item_id_verified",
        );
    }

    let next_queue_index = find_cursor_index_by_queue_item_id(
        cursors,
        queue,
        renderer.next_track.as_ref().map(|item| item.queue_item_id),
    );
    let track_index_before_next = next_queue_index.and_then(|next_index| {
        find_cursor_index_by_track_id_before(
            cursors,
            queue,
            renderer.current_track.as_ref().map(|item| item.track_id),
            next_index,
        )
    });
    if track_index_before_next.is_some() {
        return (
            track_index_before_next,
            "queue_track_id_before_renderer_next",
        );
    }

    let current_track_index = find_cursor_index_by_track_id(
        cursors,
        queue,
        renderer.current_track.as_ref().map(|item| item.track_id),
    );
    if current_track_index.is_some() {
        return (current_track_index, "queue_track_id_match");
    }

    if let Some(next_index) = next_queue_index {
        if next_index > 0 {
            return (Some(next_index - 1), "queue_item_before_renderer_next");
        }
    }

    (None, "no_current_queue_item")
}

fn resolve_controller_queue_item_from_snapshots(
    queue: &QConnectQueueState,
    renderer: &QConnectRendererState,
    direction: QconnectRemoteSkipDirection,
) -> QconnectControllerQueueItemResolution {
    let cursors = ordered_queue_cursors(queue);
    if cursors.is_empty() {
        return QconnectControllerQueueItemResolution {
            target_queue_item_id: None,
            strategy: "no_queue_items",
            queue_index: None,
            matched_track_id: None,
            matched_queue_item_id: None,
        };
    }

    let (current_index, _current_strategy) =
        resolve_current_cursor_index_from_snapshots(queue, renderer, &cursors);

    let (target_index, strategy) = match direction {
        QconnectRemoteSkipDirection::Next => {
            let next_index = find_cursor_index_by_queue_item_id(
                &cursors,
                queue,
                renderer.next_track.as_ref().map(|item| item.queue_item_id),
            );
            if let Some(next_index) = next_index {
                (Some(next_index), "renderer_next_queue_item_id_verified")
            } else if let Some(current_index) = current_index {
                if current_index + 1 < cursors.len() {
                    (Some(current_index + 1), "queue_item_after_current")
                } else {
                    (None, "no_next_queue_item")
                }
            } else {
                (None, "no_next_queue_item")
            }
        }
        QconnectRemoteSkipDirection::Previous => {
            if let Some(current_index) = current_index {
                if current_index > 0 {
                    (Some(current_index - 1), "queue_item_before_current")
                } else {
                    (Some(current_index), "restart_current_queue_item")
                }
            } else {
                (None, "no_previous_queue_item")
            }
        }
    };

    let Some(target_index) = target_index else {
        return QconnectControllerQueueItemResolution {
            target_queue_item_id: None,
            strategy,
            queue_index: None,
            matched_track_id: None,
            matched_queue_item_id: None,
        };
    };

    let cursor = cursors[target_index];
    let matched_track_id = queue_item_track_id_for_cursor(queue, cursor);
    let matched_queue_item_id = normalized_queue_item_id_for_cursor(queue, cursor);

    QconnectControllerQueueItemResolution {
        target_queue_item_id: matched_queue_item_id,
        strategy,
        queue_index: Some(target_index),
        matched_track_id,
        matched_queue_item_id,
    }
}

impl Default for QconnectServiceState {
    fn default() -> Self {
        Self::new()
    }
}

async fn apply_renderer_command_to_corebridge(
    core_bridge: &Arc<RwLock<Option<CoreBridge>>>,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
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
            next_track,
            ..
        } => {
            let resolved_playing_state = renderer_state.playing_state.or(*playing_state);
            let target_position_secs = renderer_state
                .current_position_ms
                .or(*current_position_ms)
                .map(|position_ms| position_ms / 1000);
            let mut projection_renderer_state = renderer_state.clone();
            if projection_renderer_state.current_track.is_none() {
                projection_renderer_state.current_track = current_track.clone();
            }
            if projection_renderer_state.next_track.is_none() {
                projection_renderer_state.next_track = next_track.clone();
            }
            let resolved_current_track = projection_renderer_state.current_track.as_ref();
            let mut reloaded_for_restart = false;
            if let Some(current_track) = resolved_current_track {
                let queue_state = {
                    let state = sync_state.lock().await;
                    state.last_remote_queue_state.clone()
                };
                let projection_applied = if let Some(queue_state) = queue_state.as_ref() {
                    sync_corebridge_remote_shuffle_projection(
                        bridge,
                        sync_state,
                        queue_state,
                        &projection_renderer_state,
                    )
                    .await?
                } else {
                    false
                };

                if !projection_applied {
                    if let Err(err) =
                        align_corebridge_queue_cursor(bridge, current_track.track_id).await
                    {
                        log::warn!("[QConnect] Failed to align CoreBridge queue cursor: {err}");
                    }
                }

                if matches!(
                    resolved_playing_state,
                    Some(PLAYING_STATE_PLAYING | PLAYING_STATE_PAUSED)
                ) {
                    if let Some(target_position_secs) = target_position_secs {
                        let playback_state = bridge.get_playback_state();
                        if should_force_remote_track_restart(
                            &playback_state,
                            current_track.track_id,
                            target_position_secs,
                        ) {
                            log::info!(
                                "[QConnect] Restarting remote track {} from the beginning (current={}s target={}s)",
                                current_track.track_id,
                                playback_state.position,
                                target_position_secs
                            );
                            load_remote_track_into_player(bridge, current_track.track_id).await?;
                            reloaded_for_restart = true;
                        }
                    }

                    if !reloaded_for_restart {
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
                if reloaded_for_restart && target_secs <= 1 {
                    return Ok(());
                }
                // Only seek if the position differs by more than 2 seconds to
                // avoid audio hiccups from redundant seeks (e.g. when the server
                // echoes back our own position in a SET_STATE after queue_load).
                if current_pos_secs.abs_diff(target_secs) > 2 {
                    log::info!(
                        "[QConnect] SetState seek: current={}s target={}s",
                        current_pos_secs,
                        target_secs
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
        RendererCommand::SetLoopMode { loop_mode } => {
            let resolved_loop_mode = renderer_state.loop_mode.unwrap_or(*loop_mode);
            let repeat_mode = qconnect_repeat_mode_from_loop_mode(resolved_loop_mode)
                .ok_or_else(|| format!("unsupported qconnect loop mode: {resolved_loop_mode}"))?;
            bridge.set_repeat_mode(repeat_mode).await;
        }
        RendererCommand::SetActive { active } => {
            if !*active {
                bridge.stop()?;
            }
        }
        RendererCommand::SetMaxAudioQuality { .. } | RendererCommand::SetShuffleMode { .. } => {}
    }

    Ok(())
}

async fn sync_corebridge_remote_shuffle_projection(
    bridge: &CoreBridge,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    queue_state: &QConnectQueueState,
    renderer_state: &QConnectRendererState,
) -> Result<bool, String> {
    if !queue_state.shuffle_mode || queue_state.queue_items.is_empty() {
        return Ok(false);
    }

    let start_index = resolve_remote_start_index(
        queue_state,
        renderer_state
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id),
        renderer_state
            .current_track
            .as_ref()
            .map(|item| item.track_id),
    );
    let Some(start_index) = start_index else {
        return Ok(false);
    };

    let core_shuffle_order = resolve_core_shuffle_order(
        queue_state,
        renderer_state
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id),
        renderer_state
            .current_track
            .as_ref()
            .map(|item| item.track_id),
        renderer_state
            .next_track
            .as_ref()
            .map(|item| item.queue_item_id),
        renderer_state.next_track.as_ref().map(|item| item.track_id),
    );

    let should_apply = {
        let state = sync_state.lock().await;
        state.last_materialized_start_index != Some(start_index)
            || state.last_materialized_core_shuffle_order != core_shuffle_order
    };
    if !should_apply {
        return Ok(false);
    }

    let (tracks, _) = bridge.get_all_queue_tracks().await;
    if tracks.len() != queue_state.queue_items.len() || tracks.is_empty() {
        return Ok(false);
    }

    bridge
        .set_queue_with_order(
            tracks,
            Some(start_index),
            queue_state.shuffle_mode,
            core_shuffle_order.clone(),
        )
        .await;

    let mut state = sync_state.lock().await;
    state.last_materialized_start_index = Some(start_index);
    state.last_materialized_core_shuffle_order = core_shuffle_order;
    Ok(true)
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
        if !queue_state_needs_materialization(state.last_applied_queue_state.as_ref(), queue_state)
        {
            (
                state.last_renderer_queue_item_id,
                state.last_renderer_track_id,
                state.last_renderer_next_queue_item_id,
                state.last_renderer_next_track_id,
                state.last_renderer_playing_state,
                true,
            )
        } else {
            state.last_applied_queue_state = Some(queue_state.clone());
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
            "[QConnect] materialize_remote_queue: skipped (identical snapshot {}.{})",
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
        let mut state = sync_state.lock().await;
        state.last_materialized_start_index = None;
        state.last_materialized_core_shuffle_order = None;
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
        start_index = resolve_remote_start_index(
            queue_state,
            renderer_next_queue_item_id,
            renderer_next_track_id,
        )
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
    let core_shuffle_order = resolve_core_shuffle_order(
        queue_state,
        renderer_queue_item_id,
        renderer_track_id,
        renderer_next_queue_item_id,
        renderer_next_track_id,
    );
    log::info!(
        "[QConnect] materialize_remote_queue: setting queue with {} tracks, start_index={:?}, local_track_id={:?}",
        queue_tracks.len(),
        start_index,
        current_playback_track_id
    );
    bridge
        .set_queue_with_order(
            queue_tracks,
            start_index,
            queue_state.shuffle_mode,
            core_shuffle_order.clone(),
        )
        .await;

    {
        let mut state = sync_state.lock().await;
        state.last_materialized_start_index = start_index;
        state.last_materialized_core_shuffle_order = core_shuffle_order;
    }

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
        && matches!(
            renderer_playing_state,
            Some(PLAYING_STATE_STOPPED | PLAYING_STATE_UNKNOWN)
        )
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

fn resolve_core_shuffle_order(
    queue_state: &QConnectQueueState,
    renderer_queue_item_id: Option<u64>,
    renderer_track_id: Option<u64>,
    renderer_next_queue_item_id: Option<u64>,
    renderer_next_track_id: Option<u64>,
) -> Option<Vec<usize>> {
    if !queue_state.shuffle_mode {
        return None;
    }

    let raw_order = queue_state.shuffle_order.as_ref().filter(|order| {
        is_valid_ordered_queue_shuffle_order(order, queue_state.queue_items.len())
    });

    if raw_order.is_none() {
        log::debug!(
            "[QConnect] resolve_core_shuffle_order: raw_order invalid or absent, items={} order={:?}",
            queue_state.queue_items.len(),
            queue_state.shuffle_order,
        );
        return None;
    }
    let raw_order = raw_order.unwrap();

    let current_index =
        resolve_remote_start_index(queue_state, renderer_queue_item_id, renderer_track_id);
    let next_index = resolve_remote_start_index(
        queue_state,
        renderer_next_queue_item_id,
        renderer_next_track_id,
    );

    let mut ordered = Vec::with_capacity(queue_state.queue_items.len());
    if let Some(index) = current_index {
        ordered.push(index);
    }
    if let Some(index) = next_index {
        if !ordered.contains(&index) {
            ordered.push(index);
        }
    }
    for &index in raw_order {
        if !ordered.contains(&index) {
            ordered.push(index);
        }
    }
    for index in 0..queue_state.queue_items.len() {
        if !ordered.contains(&index) {
            ordered.push(index);
        }
    }

    log::debug!(
        "[QConnect] resolve_core_shuffle_order: result={:?} current={:?} next={:?}",
        ordered, current_index, next_index,
    );

    Some(ordered)
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

fn should_reload_remote_track(
    playback_state: &qbz_player::PlaybackState,
    has_loaded_audio: bool,
    track_id: u64,
) -> bool {
    playback_state.track_id != track_id || !has_loaded_audio
}

fn should_force_remote_track_restart(
    playback_state: &qbz_player::PlaybackState,
    track_id: u64,
    target_position_secs: u64,
) -> bool {
    playback_state.track_id == track_id
        && track_id != 0
        && target_position_secs <= 1
        && playback_state.position > target_position_secs.saturating_add(2)
}

async fn load_remote_track_into_player(bridge: &CoreBridge, track_id: u64) -> Result<(), String> {
    let stream_url = bridge
        .get_stream_url(track_id, Quality::UltraHiRes)
        .await
        .map_err(|err| format!("resolve stream url for remote track {track_id}: {err}"))?;
    let duration_secs = bridge
        .get_track(track_id)
        .await
        .map(|track| u64::from(track.duration))
        .unwrap_or(0);

    match stream_remote_track_into_player(bridge, track_id, duration_secs, &stream_url.url).await {
        Ok(()) => Ok(()),
        Err(stream_err) => {
            log::warn!(
                "[QConnect] Streaming handoff unavailable for track {}: {}. Falling back to full download.",
                track_id,
                stream_err
            );
            let audio_data = download_remote_audio(&stream_url.url).await?;
            bridge
                .player()
                .play_data(audio_data, track_id)
                .map_err(|err| format!("play remote track {track_id}: {err}"))?;
            Ok(())
        }
    }
}

async fn ensure_remote_track_loaded(bridge: &CoreBridge, track_id: u64) -> Result<(), String> {
    let playback_state = bridge.get_playback_state();
    if !should_reload_remote_track(&playback_state, bridge.has_loaded_audio(), track_id) {
        return Ok(());
    }

    load_remote_track_into_player(bridge, track_id).await
}

struct QconnectRemoteStreamInfo {
    content_length: u64,
    sample_rate: u32,
    channels: u16,
    bit_depth: u32,
    speed_mbps: f64,
}

async fn stream_remote_track_into_player(
    bridge: &CoreBridge,
    track_id: u64,
    duration_secs: u64,
    url: &str,
) -> Result<(), String> {
    let stream_info = probe_remote_stream_info(url).await?;
    log::info!(
        "[QConnect/STREAMING] Track {} - {:.2} MB, {}Hz, {} ch, {}-bit, {:.1} MB/s",
        track_id,
        stream_info.content_length as f64 / (1024.0 * 1024.0),
        stream_info.sample_rate,
        stream_info.channels,
        stream_info.bit_depth,
        stream_info.speed_mbps
    );

    let writer = bridge
        .player()
        .play_streaming_dynamic(
            track_id,
            stream_info.sample_rate,
            stream_info.channels,
            stream_info.bit_depth,
            stream_info.content_length,
            stream_info.speed_mbps,
            duration_secs,
        )
        .map_err(|err| format!("start streaming remote track {track_id}: {err}"))?;

    let url = url.to_string();
    let content_length = stream_info.content_length;
    tokio::spawn(async move {
        if let Err(err) =
            download_and_stream_remote_track(&url, writer, track_id, content_length).await
        {
            log::error!(
                "[QConnect/STREAMING] Track {} failed while streaming: {}",
                track_id,
                err
            );
        }
    });

    Ok(())
}

async fn probe_remote_stream_info(url: &str) -> Result<QconnectRemoteStreamInfo, String> {
    use std::time::{Duration, Instant};

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .use_native_tls()
        .build()
        .map_err(|err| format!("create stream probe client: {err}"))?;

    let head_response = client
        .head(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|err| format!("probe HEAD request failed: {err}"))?;

    if !head_response.status().is_success() {
        return Err(format!(
            "probe HEAD request failed with status {}",
            head_response.status()
        ));
    }

    let content_length = head_response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "probe missing content-length header".to_string())?;

    let start_time = Instant::now();
    let range_response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Range", "bytes=0-65535")
        .send()
        .await
        .map_err(|err| format!("probe range request failed: {err}"))?;

    if !range_response.status().is_success() {
        return Err(format!(
            "probe range request failed with status {}",
            range_response.status()
        ));
    }

    let initial_bytes = range_response
        .bytes()
        .await
        .map_err(|err| format!("read probe bytes failed: {err}"))?;

    let elapsed = start_time.elapsed();
    let speed_mbps = if elapsed.as_secs_f64() > 0.0 {
        (initial_bytes.len() as f64 / elapsed.as_secs_f64()) / (1024.0 * 1024.0)
    } else {
        10.0
    };

    let (sample_rate, channels, bit_depth) =
        if initial_bytes.len() >= 26 && initial_bytes.starts_with(b"fLaC") {
            let sample_rate = ((initial_bytes[18] as u32) << 12)
                | ((initial_bytes[19] as u32) << 4)
                | ((initial_bytes[20] as u32) >> 4);
            let channels = ((initial_bytes[20] >> 1) & 0x07) + 1;
            let bit_depth = ((initial_bytes[20] & 0x01) << 4) | ((initial_bytes[21] >> 4) & 0x0F);
            (sample_rate, channels as u16, (bit_depth + 1) as u32)
        } else {
            log::warn!("[QConnect/STREAMING] Non-FLAC probe for remote handoff, using defaults");
            (44_100, 2, 16)
        };

    Ok(QconnectRemoteStreamInfo {
        content_length,
        sample_rate,
        channels,
        bit_depth,
        speed_mbps,
    })
}

async fn download_and_stream_remote_track(
    url: &str,
    writer: qbz_player::BufferWriter,
    track_id: u64,
    content_length: u64,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::time::{Duration, Instant};

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .use_native_tls()
        .build()
        .map_err(|err| format!("create remote streaming client: {err}"))?;

    let response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|err| format!("start remote streaming request failed: {err}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "remote streaming request failed with status {}",
            response.status()
        ));
    }

    let mut bytes_received = 0u64;
    let mut stream = response.bytes_stream();
    let start_time = Instant::now();
    let mut last_log_time = Instant::now();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|err| format!("remote streaming chunk failed: {err}"))?;
        bytes_received += chunk.len() as u64;

        if let Err(err) = writer.push_chunk(&chunk) {
            log::error!(
                "[QConnect/STREAMING] Failed to push chunk for track {}: {}",
                track_id,
                err
            );
        }

        let now = Instant::now();
        if now.duration_since(last_log_time) >= Duration::from_secs(2) && content_length > 0 {
            let progress = (bytes_received as f64 / content_length as f64) * 100.0;
            let avg_speed =
                (bytes_received as f64 / start_time.elapsed().as_secs_f64()) / (1024.0 * 1024.0);
            log::info!(
                "[QConnect/STREAMING] Track {} {:.1}% ({:.2}/{:.2} MB) @ {:.2} MB/s",
                track_id,
                progress,
                bytes_received as f64 / (1024.0 * 1024.0),
                content_length as f64 / (1024.0 * 1024.0),
                avg_speed
            );
            last_log_time = now;
        }
    }

    if let Err(err) = writer.complete() {
        log::error!(
            "[QConnect/STREAMING] Failed to mark stream complete for track {}: {}",
            track_id,
            err
        );
    }

    log::info!(
        "[QConnect/STREAMING] Track {} complete: {:.2} MB in {:.1}s",
        track_id,
        bytes_received as f64 / (1024.0 * 1024.0),
        start_time.elapsed().as_secs_f64()
    );

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

fn is_cloud_placeholder_current_queue_item(
    queue: &QConnectQueueState,
    current_index: usize,
) -> bool {
    let Some(current_item) = queue.queue_items.get(current_index) else {
        return false;
    };

    current_index == 0
        && current_item.queue_item_id == current_item.track_id
        && queue
            .queue_items
            .iter()
            .skip(1)
            .any(|item| item.queue_item_id < current_item.queue_item_id)
}

fn normalize_current_queue_item_id_from_queue_state(
    queue: &QConnectQueueState,
    current_index: usize,
) -> u64 {
    if is_cloud_placeholder_current_queue_item(queue, current_index) {
        0
    } else {
        queue.queue_items[current_index].queue_item_id
    }
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

fn default_qconnect_device_info() -> QconnectDeviceInfoPayload {
    default_qconnect_device_info_with_name(None)
}

fn default_qconnect_device_info_with_name(custom_name: Option<&str>) -> QconnectDeviceInfoPayload {
    let friendly_name = custom_name
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            std::env::var("QBZ_QCONNECT_DEVICE_NAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(resolve_default_qconnect_device_name);
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
    } else {
        *guard = Some(trimmed);
    }
    Ok(())
}

#[tauri::command]
pub fn v2_get_hostname() -> Result<String, RuntimeError> {
    Ok(resolve_system_hostname())
}

fn resolve_system_hostname() -> String {
    // Try HOSTNAME env var first
    if let Ok(h) = std::env::var("HOSTNAME") {
        if !h.trim().is_empty() {
            return h.trim().to_string();
        }
    }
    // Try /etc/hostname
    if let Ok(h) = std::fs::read_to_string("/etc/hostname") {
        let trimmed = h.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    // Fallback
    "Desktop".to_string()
}

/// Returns "Qbz - {hostname}" as the default device name.
fn resolve_default_qconnect_device_name() -> String {
    let hostname = resolve_system_hostname();
    format!("Qbz - {hostname}")
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
        build_qconnect_file_audio_quality_snapshot, classify_qconnect_audio_quality,
        decode_hex_channel, determine_queue_lookup_report_strategy, find_unique_renderer_id,
        normalize_volume_to_fraction, parse_subscribe_channels, refresh_local_renderer_id,
        resolve_controller_queue_item_from_snapshots, resolve_queue_item_ids_from_queue_state,
        should_skip_renderer_report_due_to_stale_snapshot, QconnectFileAudioQualitySnapshot,
        QconnectHandoffIntent, QconnectOutboundCommandType, QconnectRemoteSkipDirection,
        QconnectRendererInfo, QconnectSessionState, QconnectTrackOrigin,
        AUDIO_QUALITY_HIRES_LEVEL1,
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
    fn forces_remote_restart_for_same_track_reset_to_zero() {
        let playback_state = qbz_player::PlaybackState {
            is_playing: false,
            position: 248,
            duration: 251,
            track_id: 72930174,
            volume: 1.0,
        };

        assert!(super::should_force_remote_track_restart(
            &playback_state,
            72930174,
            0,
        ));
        assert!(!super::should_force_remote_track_restart(
            &playback_state,
            72930174,
            120,
        ));
        assert!(!super::should_force_remote_track_restart(
            &playback_state,
            72930175,
            0,
        ));
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

        let snapshot = super::build_session_renderer_snapshot(&queue, Some(&renderer_state), None);

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

        let snapshot = super::build_session_renderer_snapshot(
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
            super::resolve_core_shuffle_order(
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
            super::resolve_core_shuffle_order(
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
    fn reloads_remote_track_when_same_track_id_has_no_loaded_audio() {
        let playback_state = qbz_player::PlaybackState {
            is_playing: false,
            position: 0,
            duration: 279,
            track_id: 193849747,
            volume: 1.0,
        };

        assert!(super::should_reload_remote_track(
            &playback_state,
            false,
            193849747,
        ));
        assert!(!super::should_reload_remote_track(
            &playback_state,
            true,
            193849747,
        ));
        assert!(super::should_reload_remote_track(
            &playback_state,
            true,
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

        assert!(super::queue_state_needs_materialization(
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

        assert!(!super::queue_state_needs_materialization(
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
            super::QconnectControllerQueueItemResolution {
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
            super::QconnectControllerQueueItemResolution {
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
            super::QconnectControllerQueueItemResolution {
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
            super::QconnectControllerQueueItemResolution {
                target_queue_item_id: Some(0),
                strategy: "queue_item_before_current",
                queue_index: Some(0),
                matched_track_id: Some(126886853),
                matched_queue_item_id: Some(0),
            }
        );
    }
}
