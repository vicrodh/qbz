//! Tauri command handlers (`v2_qconnect_*` invokes registered in
//! `lib.rs::invoke_handler!`) plus the helpers that belong specifically
//! to the renderer-report path (audio-quality classification and the
//! stale-snapshot guards). Each command is a thin shim that delegates
//! to `QconnectServiceState` after a `runtime.check_requirements` gate.

use qconnect_app::{
    evaluate_remote_queue_admission, resolve_handoff_intent, QConnectQueueState,
    QConnectRendererState, QueueCommandType, RendererReport, RendererReportType,
};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::core_bridge::CoreBridgeState;
use crate::runtime::{CommandRequirement, RuntimeError, RuntimeManagerState};
use crate::AppState;

use super::session::QconnectFileAudioQualitySnapshot;
use super::transport::{
    persist_device_name, resolve_default_qconnect_device_name, resolve_system_hostname,
    resolve_transport_config,
};
use super::{
    QconnectAdmissionBlockedEvent, QconnectAdmissionResult, QconnectAskForRendererStateRequest,
    QconnectConnectOptions, QconnectConnectionStatus, QconnectJoinSessionRequest,
    QconnectMuteVolumeRequest, QconnectQueueVersionPayload, QconnectRendererReportDebugEvent,
    QconnectHandoffIntent, QconnectSendCommandRequest, QconnectSendCommandWithAdmissionRequest,
    QconnectServiceState, QconnectSessionState, QconnectSetActiveRendererRequest,
    QconnectSetLoopModeRequest, QconnectSetMaxAudioQualityRequest, QconnectSetPlayerStateRequest,
    QconnectSetVolumeRequest, QconnectTrackOrigin, AUDIO_QUALITY_CD, AUDIO_QUALITY_HIRES_LEVEL1,
    AUDIO_QUALITY_HIRES_LEVEL2, AUDIO_QUALITY_HIRES_LEVEL3, AUDIO_QUALITY_MP3,
    AUDIO_QUALITY_UNKNOWN, BUFFER_STATE_OK, DEFAULT_QCONNECT_CHANNEL_COUNT,
};

pub(super) fn classify_qconnect_audio_quality(sample_rate: u32, bit_depth: u32) -> i32 {
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

pub(super) fn build_qconnect_file_audio_quality_snapshot(
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

pub(super) async fn resolve_active_playback_audio_quality(
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

pub(super) fn should_skip_renderer_report_due_to_stale_snapshot(
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

pub(super) fn determine_queue_lookup_report_strategy(
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

pub(super) fn should_report_queue_item_ids_for_renderer_state(
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

