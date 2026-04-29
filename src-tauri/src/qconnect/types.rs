//! Wire types and request/response payloads exchanged with the frontend
//! and the QConnect cloud. Pure data — no behavior beyond enum mappings.

use qconnect_app::{HandoffIntent, QueueCommandType, TrackOrigin};
use qconnect_core::QueueItem;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// Lifecycle state surfaced to the UI so the toggle can reflect what the user
/// asked for (`running`) separately from what the transport currently has
/// (`Connecting`/`Connected`/`Reconnecting`). Without this distinction the UI
/// reads the toggle as "off" while the backend reconnect loop is alive,
/// leaving the user unable to disable QConnect (issue #358).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QconnectLifecycleState {
    /// User has not enabled QConnect (or it has been fully torn down).
    #[default]
    Off,
    /// `connect()` has been called; transport is establishing the WS but no
    /// `SessionEstablished` yet.
    Connecting,
    /// Transport saw at least one `SESSION_STATE` frame on the active WS — the
    /// session-level handshake completed.
    Connected,
    /// Transport disconnected after at least one successful connect; the
    /// reconnect loop is running.
    Reconnecting,
    /// `MaxReconnectAttemptsExceeded` fired — runtime auto-stopped, last error
    /// surfaced. User can re-enable from UI.
    Exhausted,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectConnectionStatus {
    pub running: bool,
    pub transport_connected: bool,
    pub endpoint_url: Option<String>,
    pub last_error: Option<String>,
    /// Granular lifecycle state — the toggle should base its on/off reading on
    /// this rather than on `transport_connected`, so a stuck reconnect loop is
    /// still visible as "on" and the user can disable it (issue #358).
    #[serde(default)]
    pub state: QconnectLifecycleState,
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
    pub(super) const fn to_queue_command_type(self) -> QueueCommandType {
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

    pub(super) const fn requires_remote_queue_admission(self) -> bool {
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
    pub(super) const fn into_core_origin(self) -> TrackOrigin {
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
    pub(super) const fn from_core(intent: HandoffIntent) -> Self {
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
