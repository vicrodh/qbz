use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use qconnect_core::{
    apply_event, apply_renderer_command, telemetry, PendingCorrelation, PendingQueueAction,
    QConnectQueueState, QConnectRendererState, QueueEvent, QueueItem, RendererCommand,
};
use qconnect_protocol::{
    build_qconnect_outbound_envelope, build_qconnect_renderer_outbound_envelope,
    parse_inbound_event, InboundEnvelope, QueueCommand, QueueCommandType, QueueEventType,
    QueueServerEvent, RendererCommandType, RendererReport, RendererReportType,
    RendererServerCommand,
};
use qconnect_transport_ws::{TransportEvent, WsTransport, WsTransportConfig};
use serde_json::Value;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{QconnectAppError, QconnectAppEvent, QconnectEventSink, QconnectRuntimeState};

pub struct QconnectApp<TTransport, TSink>
where
    TTransport: WsTransport,
    TSink: QconnectEventSink,
{
    transport: Arc<TTransport>,
    sink: Arc<TSink>,
    state: Arc<Mutex<QconnectRuntimeState>>,
}

impl<TTransport, TSink> Clone for QconnectApp<TTransport, TSink>
where
    TTransport: WsTransport,
    TSink: QconnectEventSink,
{
    fn clone(&self) -> Self {
        Self {
            transport: Arc::clone(&self.transport),
            sink: Arc::clone(&self.sink),
            state: Arc::clone(&self.state),
        }
    }
}

impl<TTransport, TSink> QconnectApp<TTransport, TSink>
where
    TTransport: WsTransport + 'static,
    TSink: QconnectEventSink + 'static,
{
    const PENDING_ACTION_TIMEOUT_MS: u64 = 10_000;

    pub fn new(transport: Arc<TTransport>, sink: Arc<TSink>) -> Self {
        Self {
            transport,
            sink,
            state: Arc::new(Mutex::new(QconnectRuntimeState::default())),
        }
    }

    pub fn state_handle(&self) -> Arc<Mutex<QconnectRuntimeState>> {
        Arc::clone(&self.state)
    }

    pub fn subscribe_transport_events(&self) -> tokio::sync::broadcast::Receiver<TransportEvent> {
        self.transport.subscribe()
    }

    pub async fn connect(&self, config: WsTransportConfig) -> Result<(), QconnectAppError> {
        self.transport.connect(config).await?;
        {
            let mut state = self.state.lock().await;
            state.transport_connected = true;
        }
        self.sink
            .on_event(QconnectAppEvent::TransportConnected)
            .await;
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<(), QconnectAppError> {
        self.transport.disconnect().await?;
        {
            let mut state = self.state.lock().await;
            state.transport_connected = false;
            state.pending.clear();
        }
        self.sink
            .on_event(QconnectAppEvent::TransportDisconnected)
            .await;
        Ok(())
    }

    pub async fn queue_state_snapshot(&self) -> QConnectQueueState {
        self.state.lock().await.queue.clone()
    }

    pub async fn renderer_state_snapshot(&self) -> QConnectRendererState {
        self.state.lock().await.renderer.clone()
    }

    pub async fn build_queue_command(
        &self,
        command_type: QueueCommandType,
        payload: Value,
    ) -> QueueCommand {
        let version_ref = self.state.lock().await.queue.version;
        QueueCommand::new(command_type, self.next_action_uuid(), version_ref, payload)
    }

    pub async fn send_queue_command(
        &self,
        command: QueueCommand,
    ) -> Result<String, QconnectAppError> {
        let action_uuid = command.action_uuid.clone();
        let is_set_active_renderer_action = matches!(
            command.command_type,
            QueueCommandType::CtrlSrvrSetActiveRenderer
        );
        let pending = PendingQueueAction {
            uuid: action_uuid.clone(),
            queue_version_ref: command.queue_version_ref,
            emit_result_event: true,
            is_ask_for_state_action: matches!(
                command.command_type,
                QueueCommandType::CtrlSrvrAskForQueueState
            ),
            is_transport_control_action: matches!(
                command.command_type,
                QueueCommandType::CtrlSrvrSetPlayerState
            ),
            is_set_loop_mode_action: matches!(
                command.command_type,
                QueueCommandType::CtrlSrvrSetLoopMode
            ),
            is_set_active_renderer_action,
            expected_active_renderer_id: if is_set_active_renderer_action {
                pending_active_renderer_id_from_payload(&command.payload)
            } else {
                None
            },
            concurrency_error: false,
            sent_at_ms: now_ms(),
        };

        {
            let mut state = self.state.lock().await;
            state.pending.start(pending)?;
        }

        let envelope = build_qconnect_outbound_envelope(command)?;
        if let Err(err) = self.transport.send(envelope).await {
            let mut state = self.state.lock().await;
            state.pending.clear();
            return Err(err.into());
        }

        self.sink
            .on_event(QconnectAppEvent::PendingActionStarted {
                uuid: action_uuid.clone(),
            })
            .await;
        self.spawn_pending_timeout_watch(action_uuid.clone());
        Ok(action_uuid)
    }

    pub async fn send_renderer_report_command(
        &self,
        report: RendererReport,
    ) -> Result<(), QconnectAppError> {
        self.send_renderer_report(report).await
    }

    /// Update the renderer state's position from the frontend's playback position.
    /// This keeps the internal state in sync with the actual audio playback, so that
    /// renderer reports triggered by server commands (pause/resume) include the real position.
    pub async fn update_renderer_position(&self, position_ms: u64) {
        let mut state = self.state.lock().await;
        state.renderer.current_position_ms = Some(position_ms);
    }

    pub async fn handle_transport_event(
        &self,
        event: TransportEvent,
    ) -> Result<(), QconnectAppError> {
        match event {
            TransportEvent::Connected => {
                {
                    let mut state = self.state.lock().await;
                    state.transport_connected = true;
                }
                self.sink
                    .on_event(QconnectAppEvent::TransportConnected)
                    .await;
            }
            TransportEvent::Disconnected => {
                {
                    let mut state = self.state.lock().await;
                    state.transport_connected = false;
                    state.pending.clear();
                }
                self.sink
                    .on_event(QconnectAppEvent::TransportDisconnected)
                    .await;
            }
            TransportEvent::InboundReceived(inbound) => {
                self.handle_inbound_envelope(inbound).await?;
            }
            TransportEvent::InboundQueueServerEvent(event) => {
                self.apply_server_event(event).await?;
            }
            TransportEvent::InboundRendererServerCommand(command) => {
                self.apply_renderer_server_command(command).await?;
            }
            TransportEvent::Authenticated
            | TransportEvent::Subscribed
            | TransportEvent::SessionEstablished
            | TransportEvent::MaxReconnectAttemptsExceeded { .. }
            | TransportEvent::ReconnectScheduled { .. }
            | TransportEvent::KeepalivePingSent
            | TransportEvent::KeepalivePongReceived
            | TransportEvent::TransportError { .. }
            | TransportEvent::InboundFrameDecoded { .. }
            | TransportEvent::InboundPayloadBytes { .. }
            | TransportEvent::OutboundSent { .. } => {}
        }
        Ok(())
    }

    pub async fn handle_inbound_envelope(
        &self,
        inbound: InboundEnvelope,
    ) -> Result<(), QconnectAppError> {
        let event = parse_inbound_event(inbound)?;
        self.apply_server_event(event).await
    }

    async fn apply_server_event(&self, event: QueueServerEvent) -> Result<(), QconnectAppError> {
        // Session management events bypass the queue reducer entirely.
        // They provide session topology info (renderers, active renderer, etc.)
        if event.event_type.is_session_management() {
            let completed_uuid = {
                let mut state = self.state.lock().await;
                let matched_by_uuid = matches!(
                    state.pending.correlate(event.action_uuid.as_deref()),
                    PendingCorrelation::Matched
                );
                let matched_by_session_effect = state
                    .pending
                    .current()
                    .map(|pending| {
                        session_management_event_completes_pending_action(
                            pending,
                            &event.event_type,
                            &event.payload,
                        )
                    })
                    .unwrap_or(false);
                if matched_by_uuid || matched_by_session_effect {
                    state.pending.clear().map(|pending| pending.uuid)
                } else {
                    None
                }
            };

            if let Some(uuid) = completed_uuid {
                self.sink
                    .on_event(QconnectAppEvent::PendingActionCompleted { uuid })
                    .await;
            }

            self.sink
                .on_event(QconnectAppEvent::SessionManagementEvent {
                    message_type: event.message_type().to_string(),
                    payload: event.payload.clone(),
                })
                .await;
            return Ok(());
        }

        let mut completed_uuid: Option<String> = None;
        let mut canceled_uuid: Option<String> = None;
        let mut ignored_queue_error_uuid: Option<String> = None;
        let mut should_trigger_resync = false;
        let mut should_emit_queue_update = false;
        let remote_action_uuid = event.action_uuid.clone().unwrap_or_default();
        let snapshot: QConnectQueueState;

        {
            let mut state = self.state.lock().await;
            match state.pending.correlate(event.action_uuid.as_deref()) {
                PendingCorrelation::Matched => {
                    completed_uuid = state.pending.clear().map(|pending| pending.uuid);
                }
                PendingCorrelation::Concurrent => {
                    state.pending.mark_concurrency_error();
                    canceled_uuid = state.pending.clear().map(|pending| pending.uuid);
                    state.concurrency_canceled_action_uuid = canceled_uuid.clone();
                    should_trigger_resync = canceled_uuid.is_some();
                }
                PendingCorrelation::NoPending | PendingCorrelation::EventWithoutActionUuid => {}
            }

            let ignore_queue_error =
                matches!(event.event_type, QueueEventType::SrvrCtrlQueueErrorMessage)
                    && event.action_uuid.is_some()
                    && state.concurrency_canceled_action_uuid.as_deref()
                        == event.action_uuid.as_deref();

            if ignore_queue_error {
                ignored_queue_error_uuid = event.action_uuid.clone();
                state.concurrency_canceled_action_uuid = None;
            } else {
                let queue_event = map_server_event(&event, &state.queue);
                let reducer_outcome = apply_event(&mut state.queue, &queue_event, now_ms());
                let _metric_name = telemetry::queue_reducer_event_name(reducer_outcome.event_name);
                should_emit_queue_update = true;
                if matches!(
                    event.event_type,
                    QueueEventType::SrvrCtrlShuffleModeSet
                        | QueueEventType::SrvrCtrlQueueTracksReordered
                        | QueueEventType::SrvrCtrlQueueTracksRemoved
                ) {
                    should_trigger_resync = true;
                }
            }
            snapshot = state.queue.clone();
        }

        if let Some(uuid) = completed_uuid {
            self.sink
                .on_event(QconnectAppEvent::PendingActionCompleted { uuid })
                .await;
        }

        if let Some(pending_uuid) = canceled_uuid {
            self.sink
                .on_event(
                    QconnectAppEvent::PendingActionCanceledByConcurrentRemoteEvent {
                        pending_uuid,
                        remote_action_uuid,
                    },
                )
                .await;
        }

        if let Some(action_uuid) = ignored_queue_error_uuid {
            self.sink
                .on_event(QconnectAppEvent::QueueErrorIgnoredByConcurrency { action_uuid })
                .await;
        }

        if should_emit_queue_update {
            self.sink
                .on_event(QconnectAppEvent::QueueUpdated(snapshot))
                .await;
        }

        if should_trigger_resync {
            self.trigger_queue_state_resync().await;
        }
        Ok(())
    }

    async fn apply_renderer_server_command(
        &self,
        command: RendererServerCommand,
    ) -> Result<(), QconnectAppError> {
        let Some(renderer_command) = map_renderer_server_command(&command) else {
            return Ok(());
        };

        // Detect echo SET_STATE commands: the server echoes every state report
        // as a SET_STATE with only next_track (playing_state=None, current_track=None).
        // These echoes must NOT trigger CoreBridge actions (align cursor, load track,
        // resume/pause) or state reports, otherwise they destroy the local queue
        // and cause feedback loops.
        let is_echo = matches!(
            &renderer_command,
            RendererCommand::SetState {
                playing_state,
                current_track,
                ..
            } if playing_state.is_none() && current_track.is_none()
        );

        let (snapshot, queue_version) = {
            let mut state = self.state.lock().await;
            apply_renderer_command(&mut state.renderer, &renderer_command, now_ms());
            (state.renderer.clone(), state.queue.version)
        };

        // Always update renderer state (for tracking next_track etc.)
        self.sink
            .on_event(QconnectAppEvent::RendererUpdated(snapshot.clone()))
            .await;

        if is_echo {
            log::debug!("[QConnect] Skipping echo SET_STATE (no playing_state or current_track)");
            return Ok(());
        }

        self.sink
            .on_event(QconnectAppEvent::RendererCommandApplied {
                command: renderer_command.clone(),
                state: snapshot.clone(),
            })
            .await;

        self.send_renderer_reports(&renderer_command, &snapshot, queue_version)
            .await?;
        Ok(())
    }

    fn spawn_pending_timeout_watch(&self, action_uuid: String) {
        let app = self.clone();
        tokio::spawn(async move {
            app.watch_pending_action_timeout(action_uuid).await;
        });
    }

    async fn watch_pending_action_timeout(&self, action_uuid: String) {
        tokio::time::sleep(Duration::from_millis(Self::PENDING_ACTION_TIMEOUT_MS)).await;

        let (timed_out, timed_out_ask_for_state) = {
            let mut state = self.state.lock().await;
            let (is_same_pending, is_ask_for_state_action) = state
                .pending
                .current()
                .map(|pending| {
                    (
                        pending.uuid.as_str() == action_uuid,
                        pending.is_ask_for_state_action,
                    )
                })
                .unwrap_or((false, false));
            if is_same_pending {
                state.pending.clear();
                (true, is_ask_for_state_action)
            } else {
                (false, false)
            }
        };

        if !timed_out {
            return;
        }

        self.sink
            .on_event(QconnectAppEvent::PendingActionTimedOut {
                uuid: action_uuid,
                timeout_ms: Self::PENDING_ACTION_TIMEOUT_MS,
            })
            .await;

        if !timed_out_ask_for_state {
            self.trigger_queue_state_resync().await;
        }
    }

    async fn trigger_queue_state_resync(&self) {
        let queue_version_ref = {
            let state = self.state.lock().await;
            if state.pending.current().is_some() {
                return;
            }
            state.queue.version
        };

        let command = QueueCommand::new(
            QueueCommandType::CtrlSrvrAskForQueueState,
            self.next_action_uuid(),
            queue_version_ref,
            Value::Object(Default::default()),
        );

        if self.send_queue_command(command).await.is_ok() {
            self.sink
                .on_event(QconnectAppEvent::QueueResyncTriggered)
                .await;
        }
    }

    fn next_action_uuid(&self) -> String {
        Uuid::new_v4().to_string()
    }

    async fn send_renderer_reports(
        &self,
        command: &RendererCommand,
        renderer: &QConnectRendererState,
        queue_version_ref: qconnect_core::QueueVersion,
    ) -> Result<(), QconnectAppError> {
        match command {
            RendererCommand::SetState {
                playing_state,
                current_track,
                ..
            } => {
                // Only send a state report when the SET_STATE carries a meaningful
                // change (playing_state or current_track). The server echoes every
                // state report as a SET_STATE with only next_track updated, which
                // would create an infinite feedback loop if we replied to it.
                let is_substantive = playing_state.is_some() || current_track.is_some();
                if is_substantive {
                    log::info!(
                        "[QConnect/Report] SetState report: playing={:?} pos={:?} track={:?} next={:?} qv={}.{}",
                        renderer.playing_state,
                        renderer.current_position_ms,
                        renderer.current_track.as_ref().map(|t| (t.track_id, t.queue_item_id)),
                        renderer.next_track.as_ref().map(|t| (t.track_id, t.queue_item_id)),
                        queue_version_ref.major,
                        queue_version_ref.minor
                    );
                    let report = RendererReport::new(
                        RendererReportType::RndrSrvrStateUpdated,
                        self.next_action_uuid(),
                        queue_version_ref,
                        serde_json::json!({
                            "playing_state": renderer.playing_state,
                            "buffer_state": infer_buffer_state(renderer.playing_state),
                            "current_position": renderer.current_position_ms,
                            "duration": Option::<u64>::None,
                            "queue_version": {
                                "major": queue_version_ref.major,
                                "minor": queue_version_ref.minor
                            },
                            "current_queue_item_id": Option::<i32>::None,
                            "next_queue_item_id": Option::<i32>::None
                        }),
                    );
                    self.send_renderer_report(report).await?;
                } else {
                    log::debug!(
                        "[QConnect/Report] Skipping echo SET_STATE report (no playing_state or current_track change)"
                    );
                }
            }
            RendererCommand::SetVolume { volume, .. } => {
                let resolved_volume = renderer.volume.or(*volume);
                if let Some(resolved_volume) = resolved_volume {
                    let report = RendererReport::new(
                        RendererReportType::RndrSrvrVolumeChanged,
                        self.next_action_uuid(),
                        queue_version_ref,
                        serde_json::json!({
                            "volume": resolved_volume
                        }),
                    );
                    self.send_renderer_report(report).await?;
                }
            }
            RendererCommand::MuteVolume { value } => {
                let report = RendererReport::new(
                    RendererReportType::RndrSrvrVolumeMuted,
                    self.next_action_uuid(),
                    queue_version_ref,
                    serde_json::json!({
                        "value": value
                    }),
                );
                self.send_renderer_report(report).await?;
            }
            RendererCommand::SetMaxAudioQuality { max_audio_quality } => {
                let report = RendererReport::new(
                    RendererReportType::RndrSrvrMaxAudioQualityChanged,
                    self.next_action_uuid(),
                    queue_version_ref,
                    serde_json::json!({
                        "max_audio_quality": max_audio_quality
                    }),
                );
                self.send_renderer_report(report).await?;
            }
            RendererCommand::SetActive { .. }
            | RendererCommand::SetLoopMode { .. }
            | RendererCommand::SetShuffleMode { .. } => {}
        }

        Ok(())
    }

    async fn send_renderer_report(&self, report: RendererReport) -> Result<(), QconnectAppError> {
        let envelope = build_qconnect_renderer_outbound_envelope(report)?;
        self.transport.send(envelope).await?;
        Ok(())
    }
}

fn infer_buffer_state(playing_state: Option<i32>) -> Option<i32> {
    match playing_state {
        Some(2) | Some(3) => Some(2),
        Some(1) => Some(1),
        Some(value) => Some(value),
        None => None,
    }
}

fn map_server_event(event: &QueueServerEvent, current: &QConnectQueueState) -> QueueEvent {
    let version = event
        .queue_version
        .unwrap_or_else(|| current.version.next_minor());
    match event.event_type {
        QueueEventType::SrvrCtrlQueueState => {
            let mut next = current.clone();
            next.version = version;
            next.queue_items = parse_queue_items(&event.payload, "tracks");
            next.shuffle_mode = parse_bool(&event.payload, "shuffle_mode", current.shuffle_mode);
            next.autoplay_mode = parse_bool(&event.payload, "autoplay_mode", current.autoplay_mode);
            next.autoplay_loading =
                parse_bool(&event.payload, "autoplay_loading", current.autoplay_loading);
            next.autoplay_items = parse_queue_items(&event.payload, "autoplay_tracks");

            if next.shuffle_mode {
                let parsed_shuffle = parse_usize_list(&event.payload, "shuffled_track_indexes");
                next.shuffle_order = if !parsed_shuffle.is_empty() {
                    Some(parsed_shuffle)
                } else {
                    current.shuffle_order.clone()
                };
            } else {
                next.shuffle_order = None;
            }

            QueueEvent::QueueStateReplaced {
                action_uuid: event.action_uuid.clone(),
                state: next,
            }
        }
        QueueEventType::SrvrCtrlQueueTracksAdded => QueueEvent::TracksAdded {
            action_uuid: event.action_uuid.clone(),
            version,
            tracks: parse_queue_items(&event.payload, "tracks"),
            shuffle_seed: parse_u64(&event.payload, "shuffle_seed"),
            autoplay_reset: parse_bool(&event.payload, "autoplay_reset", false),
            autoplay_loading: parse_bool(&event.payload, "autoplay_loading", false),
        },
        QueueEventType::SrvrCtrlQueueTracksLoaded => QueueEvent::TracksLoaded {
            action_uuid: event.action_uuid.clone(),
            version,
            tracks: parse_queue_items(&event.payload, "tracks"),
            queue_position: parse_u64(&event.payload, "queue_position"),
            shuffle_mode: parse_optional_bool(&event.payload, "shuffle_mode"),
            shuffle_seed: parse_u64(&event.payload, "shuffle_seed"),
            shuffle_pivot_queue_item_id: parse_u64(&event.payload, "shuffle_pivot_queue_item_id"),
            autoplay_reset: parse_bool(&event.payload, "autoplay_reset", false),
            autoplay_loading: parse_bool(&event.payload, "autoplay_loading", false),
        },
        QueueEventType::SrvrCtrlQueueTracksInserted => QueueEvent::TracksInserted {
            action_uuid: event.action_uuid.clone(),
            version,
            tracks: parse_queue_items(&event.payload, "tracks"),
            insert_after: parse_u64(&event.payload, "insert_after"),
            shuffle_seed: parse_u64(&event.payload, "shuffle_seed"),
            autoplay_reset: parse_bool(&event.payload, "autoplay_reset", false),
            autoplay_loading: parse_bool(&event.payload, "autoplay_loading", false),
        },
        QueueEventType::SrvrCtrlQueueTracksRemoved => QueueEvent::TracksRemoved {
            action_uuid: event.action_uuid.clone(),
            version,
            queue_item_ids: parse_queue_item_ids(&event.payload),
            autoplay_reset: parse_bool(&event.payload, "autoplay_reset", false),
            autoplay_loading: parse_bool(&event.payload, "autoplay_loading", false),
        },
        QueueEventType::SrvrCtrlQueueTracksReordered => QueueEvent::TracksReordered {
            action_uuid: event.action_uuid.clone(),
            version,
            queue_item_ids: parse_queue_item_ids(&event.payload),
            insert_after: parse_u64(&event.payload, "insert_after"),
            autoplay_reset: parse_bool(&event.payload, "autoplay_reset", false),
            autoplay_loading: parse_bool(&event.payload, "autoplay_loading", false),
        },
        QueueEventType::SrvrCtrlQueueCleared => QueueEvent::QueueCleared {
            action_uuid: event.action_uuid.clone(),
            version,
        },
        QueueEventType::SrvrCtrlShuffleModeSet => QueueEvent::ShuffleModeSet {
            action_uuid: event.action_uuid.clone(),
            version,
            shuffle_mode: parse_bool(&event.payload, "shuffle_mode", false),
            shuffle_seed: parse_u64(&event.payload, "shuffle_seed"),
            shuffle_pivot_queue_item_id: parse_u64(&event.payload, "shuffle_pivot_queue_item_id"),
            autoplay_reset: parse_bool(&event.payload, "autoplay_reset", false),
            autoplay_loading: parse_bool(&event.payload, "autoplay_loading", false),
        },
        QueueEventType::SrvrCtrlAutoplayModeSet => QueueEvent::AutoplayModeSet {
            action_uuid: event.action_uuid.clone(),
            version,
            autoplay_mode: parse_bool(&event.payload, "autoplay_mode", false),
            autoplay_reset: parse_bool(&event.payload, "autoplay_reset", false),
            autoplay_loading: parse_bool(&event.payload, "autoplay_loading", false),
        },
        QueueEventType::SrvrCtrlAutoplayTracksLoaded => QueueEvent::AutoplayTracksLoaded {
            action_uuid: event.action_uuid.clone(),
            version,
            tracks: parse_queue_items(&event.payload, "tracks"),
        },
        QueueEventType::SrvrCtrlAutoplayTracksRemoved => QueueEvent::AutoplayTracksRemoved {
            action_uuid: event.action_uuid.clone(),
            version,
            queue_item_ids: parse_queue_item_ids(&event.payload),
        },
        QueueEventType::SrvrCtrlQueueTracksAddedFromAutoplay => QueueEvent::TracksAdded {
            action_uuid: event.action_uuid.clone(),
            version,
            tracks: Vec::new(),
            shuffle_seed: None,
            autoplay_reset: false,
            autoplay_loading: false,
        },
        QueueEventType::SrvrCtrlQueueErrorMessage => QueueEvent::QueueError {
            action_uuid: event.action_uuid.clone(),
            version: Some(version),
            code: event
                .payload
                .get("error_code")
                .map(value_to_string)
                .unwrap_or_else(|| "remote_error".to_string()),
            message: event
                .payload
                .get("error_message")
                .map(value_to_string)
                .unwrap_or_else(|| "queue_error_message".to_string()),
        },
        // Session management events are intercepted in apply_server_event()
        // and should never reach map_server_event(). Treat as no-op if they do.
        _ => QueueEvent::QueueCleared {
            action_uuid: None,
            version: current.version,
        },
    }
}

fn map_renderer_server_command(command: &RendererServerCommand) -> Option<RendererCommand> {
    match command.command_type {
        RendererCommandType::SrvrRndrSetState => Some(RendererCommand::SetState {
            playing_state: parse_i32(&command.payload, "playing_state"),
            current_position_ms: parse_u64(&command.payload, "current_position"),
            current_track: parse_renderer_track(&command.payload, "current_track"),
            next_track: parse_renderer_track(&command.payload, "next_track"),
        }),
        RendererCommandType::SrvrRndrSetVolume => Some(RendererCommand::SetVolume {
            volume: parse_i32(&command.payload, "volume"),
            volume_delta: parse_i32(&command.payload, "volume_delta"),
        }),
        RendererCommandType::SrvrRndrSetActive => Some(RendererCommand::SetActive {
            active: parse_bool(&command.payload, "active", false),
        }),
        RendererCommandType::SrvrRndrSetMaxAudioQuality => {
            parse_i32(&command.payload, "max_audio_quality")
                .map(|max_audio_quality| RendererCommand::SetMaxAudioQuality { max_audio_quality })
        }
        RendererCommandType::SrvrRndrSetLoopMode => parse_i32(&command.payload, "loop_mode")
            .map(|loop_mode| RendererCommand::SetLoopMode { loop_mode }),
        RendererCommandType::SrvrRndrSetShuffleMode => Some(RendererCommand::SetShuffleMode {
            shuffle_mode: parse_bool(&command.payload, "shuffle_mode", false),
        }),
        RendererCommandType::SrvrRndrMuteVolume => Some(RendererCommand::MuteVolume {
            value: parse_bool(&command.payload, "value", false),
        }),
    }
}

fn parse_renderer_track(payload: &Value, field: &str) -> Option<QueueItem> {
    payload.get(field).and_then(parse_queue_item)
}

fn parse_queue_items(payload: &Value, field: &str) -> Vec<QueueItem> {
    payload
        .get(field)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_queue_item).collect())
        .unwrap_or_default()
}

fn parse_queue_item(value: &Value) -> Option<QueueItem> {
    let track_context_uuid = value
        .get("track_context_uuid")
        .or_else(|| value.get("context_uuid"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let track_id = value
        .get("track_id")
        .or_else(|| value.get("trackId"))
        .and_then(value_as_u64)?;

    let queue_item_id = value
        .get("queue_item_id")
        .or_else(|| value.get("queueItemId"))
        .and_then(value_as_u64)
        .unwrap_or(track_id);

    Some(QueueItem {
        track_context_uuid,
        track_id,
        queue_item_id,
    })
}

fn parse_queue_item_ids(payload: &Value) -> Vec<u64> {
    payload
        .get("queue_item_ids")
        .or_else(|| payload.get("queueItemsIds"))
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(value_as_u64).collect())
        .unwrap_or_default()
}

fn parse_u64(payload: &Value, field: &str) -> Option<u64> {
    payload.get(field).and_then(value_as_u64)
}

fn parse_bool(payload: &Value, field: &str, default: bool) -> bool {
    payload
        .get(field)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn parse_optional_bool(payload: &Value, field: &str) -> Option<bool> {
    payload.get(field).and_then(Value::as_bool)
}

fn parse_i32(payload: &Value, field: &str) -> Option<i32> {
    payload.get(field).and_then(value_as_i32)
}

fn parse_usize_list(payload: &Value, field: &str) -> Vec<usize> {
    payload
        .get(field)
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(value_as_u64)
                .filter_map(|value| usize::try_from(value).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn value_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|entry| u64::try_from(entry).ok()))
}

fn value_as_i32(value: &Value) -> Option<i32> {
    value
        .as_i64()
        .and_then(|entry| i32::try_from(entry).ok())
        .or_else(|| value.as_u64().and_then(|entry| i32::try_from(entry).ok()))
}

fn value_to_string(value: &Value) -> String {
    value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn pending_active_renderer_id_from_payload(payload: &Value) -> Option<i32> {
    payload
        .get("renderer_id")
        .or_else(|| payload.get("active_renderer_id"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn session_management_event_completes_pending_action(
    pending: &PendingQueueAction,
    event_type: &QueueEventType,
    payload: &Value,
) -> bool {
    if pending.is_transport_control_action
        && matches!(event_type, QueueEventType::SrvrCtrlRendererStateUpdated)
    {
        // Transport control SET_PLAYER_STATE commands do not get a dedicated
        // action_uuid ack. The first renderer state update is the practical
        // completion signal; otherwise rapid next/previous presses stay
        // blocked behind a stale pending slot.
        return true;
    }

    if pending.is_set_loop_mode_action && matches!(event_type, QueueEventType::SrvrCtrlLoopModeSet)
    {
        // Loop mode changes come back as session-management events without a
        // stable action_uuid ack. Treat the first loop-mode-set echo as the
        // completion signal so repeat toggles are not blocked behind the
        // generic 10s pending timeout.
        return true;
    }

    if !pending.is_set_active_renderer_action {
        return false;
    }

    let Some(expected_renderer_id) = pending.expected_active_renderer_id else {
        return false;
    };

    match event_type {
        QueueEventType::SrvrCtrlActiveRendererChanged | QueueEventType::SrvrCtrlSessionState => {
            payload
                .get("active_renderer_id")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                == Some(expected_renderer_id)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use qconnect_core::QueueVersion;
    use qconnect_transport_ws::InMemoryWsTransport;
    use serde_json::json;
    use tokio::sync::Mutex;

    use crate::{QconnectAppEvent, QconnectEventSink};

    use super::QconnectApp;
    use qconnect_protocol::{
        QueueCommandType, QueueEventType, QueueServerEvent, RendererCommandType,
        RendererServerCommand,
    };
    use qconnect_transport_ws::WsTransportConfig;

    #[derive(Debug, Default, Clone)]
    struct TestSink {
        events: Arc<Mutex<Vec<QconnectAppEvent>>>,
    }

    impl TestSink {
        async fn snapshot(&self) -> Vec<QconnectAppEvent> {
            self.events.lock().await.clone()
        }
    }

    #[async_trait]
    impl QconnectEventSink for TestSink {
        async fn on_event(&self, event: QconnectAppEvent) {
            self.events.lock().await.push(event);
        }
    }

    fn test_config() -> WsTransportConfig {
        WsTransportConfig {
            endpoint_url: "wss://example.invalid/ws".to_string(),
            subscribe_channels: vec![vec![1, 2, 3]],
            ..Default::default()
        }
    }

    async fn build_connected_app() -> (
        QconnectApp<InMemoryWsTransport, TestSink>,
        TestSink,
        Arc<InMemoryWsTransport>,
        tokio::sync::broadcast::Receiver<qconnect_transport_ws::TransportEvent>,
    ) {
        let transport = Arc::new(InMemoryWsTransport::new());
        let sink = TestSink::default();
        let app = QconnectApp::new(Arc::clone(&transport), Arc::new(sink.clone()));
        let events_rx = app.subscribe_transport_events();
        app.connect(test_config()).await.expect("connect");
        (app, sink, transport, events_rx)
    }

    #[tokio::test(start_paused = true)]
    async fn pending_timeout_triggers_resync() {
        let (app, sink, transport, _events_rx) = build_connected_app().await;
        let command = app
            .build_queue_command(
                QueueCommandType::CtrlSrvrQueueAddTracks,
                json!({"track_ids":[101]}),
            )
            .await;
        app.send_queue_command(command)
            .await
            .expect("send queue command");

        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_millis(
            QconnectApp::<InMemoryWsTransport, TestSink>::PENDING_ACTION_TIMEOUT_MS + 10,
        ))
        .await;
        tokio::task::yield_now().await;

        let events = sink.snapshot().await;
        assert!(events
            .iter()
            .any(|event| { matches!(event, QconnectAppEvent::PendingActionTimedOut { .. }) }));
        assert!(events
            .iter()
            .any(|event| matches!(event, QconnectAppEvent::QueueResyncTriggered)));

        let sent = transport.sent_messages().await;
        assert!(
            sent.len() >= 2,
            "expected original command plus ask-for-state resync"
        );
    }

    #[tokio::test]
    async fn concurrent_remote_event_cancels_pending_and_requests_resync() {
        let (app, sink, transport, _events_rx) = build_connected_app().await;
        let command = app
            .build_queue_command(
                QueueCommandType::CtrlSrvrQueueAddTracks,
                json!({"track_ids":[101]}),
            )
            .await;
        let pending_uuid = app
            .send_queue_command(command)
            .await
            .expect("send queue command");

        let remote_event = QueueServerEvent {
            event_type: QueueEventType::SrvrCtrlQueueTracksAdded,
            action_uuid: Some("0f892e1a-a2f4-4d18-82c6-31e8daf2ea0f".to_string()),
            queue_version: Some(QueueVersion::new(1, 1)),
            payload: json!({"tracks":[]}),
        };

        app.apply_server_event(remote_event)
            .await
            .expect("apply concurrent event");

        let events = sink.snapshot().await;
        assert!(events.iter().any(|event| {
            matches!(
                event,
                QconnectAppEvent::PendingActionCanceledByConcurrentRemoteEvent { pending_uuid: id, .. } if id == &pending_uuid
            )
        }));
        assert!(events
            .iter()
            .any(|event| matches!(event, QconnectAppEvent::QueueResyncTriggered)));

        let sent = transport.sent_messages().await;
        assert!(
            sent.len() >= 2,
            "expected original command plus ask-for-state resync"
        );
    }

    #[tokio::test]
    async fn shuffle_mode_set_requests_authoritative_queue_state() {
        let (app, sink, transport, _events_rx) = build_connected_app().await;

        app.apply_server_event(QueueServerEvent {
            event_type: QueueEventType::SrvrCtrlShuffleModeSet,
            action_uuid: Some("6f9f8d84-cd82-486f-a423-cd467117f39d".to_string()),
            queue_version: Some(QueueVersion::new(1, 2)),
            payload: json!({
                "shuffle_mode": true,
                "shuffle_seed": 123,
                "shuffle_pivot_queue_item_id": 0,
                "autoplay_reset": false,
                "autoplay_loading": false
            }),
        })
        .await
        .expect("apply shuffle-mode-set event");

        let events = sink.snapshot().await;
        assert!(events
            .iter()
            .any(|event| matches!(event, QconnectAppEvent::QueueUpdated(_))));
        assert!(events
            .iter()
            .any(|event| matches!(event, QconnectAppEvent::QueueResyncTriggered)));

        let sent = transport.sent_messages().await;
        assert!(
            !sent.is_empty(),
            "expected ask-for-state resync after shuffle"
        );
    }

    #[tokio::test]
    async fn reorder_event_requests_authoritative_queue_state() {
        let (app, sink, transport, _events_rx) = build_connected_app().await;

        app.apply_server_event(QueueServerEvent {
            event_type: QueueEventType::SrvrCtrlQueueTracksReordered,
            action_uuid: Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string()),
            queue_version: Some(QueueVersion::new(2, 1)),
            payload: json!({
                "queue_item_ids": [3],
                "insert_after": 1,
                "autoplay_reset": false,
                "autoplay_loading": false
            }),
        })
        .await
        .expect("apply reorder event");

        let events = sink.snapshot().await;
        assert!(events
            .iter()
            .any(|event| matches!(event, QconnectAppEvent::QueueResyncTriggered)));

        let sent = transport.sent_messages().await;
        assert!(
            !sent.is_empty(),
            "expected ask-for-state resync after reorder"
        );
    }

    #[tokio::test]
    async fn remove_event_requests_authoritative_queue_state() {
        let (app, sink, transport, _events_rx) = build_connected_app().await;

        app.apply_server_event(QueueServerEvent {
            event_type: QueueEventType::SrvrCtrlQueueTracksRemoved,
            action_uuid: Some("b2c3d4e5-f6a7-8901-bcde-f12345678901".to_string()),
            queue_version: Some(QueueVersion::new(3, 1)),
            payload: json!({
                "queue_item_ids": [2],
                "autoplay_reset": false,
                "autoplay_loading": false
            }),
        })
        .await
        .expect("apply remove event");

        let events = sink.snapshot().await;
        assert!(events
            .iter()
            .any(|event| matches!(event, QconnectAppEvent::QueueResyncTriggered)));

        let sent = transport.sent_messages().await;
        assert!(
            !sent.is_empty(),
            "expected ask-for-state resync after remove"
        );
    }

    #[tokio::test]
    async fn late_queue_error_for_concurrency_canceled_action_is_ignored() {
        let (app, sink, _, _events_rx) = build_connected_app().await;
        {
            let state = app.state_handle();
            let mut guard = state.lock().await;
            guard.concurrency_canceled_action_uuid =
                Some("85fa0dd6-7bd6-4b3c-8f43-b8ee22e65d5e".to_string());
        }

        let late_error = QueueServerEvent {
            event_type: QueueEventType::SrvrCtrlQueueErrorMessage,
            action_uuid: Some("85fa0dd6-7bd6-4b3c-8f43-b8ee22e65d5e".to_string()),
            queue_version: Some(QueueVersion::new(2, 1)),
            payload: json!({
                "error_code": "409",
                "error_message": "stale_queue_version"
            }),
        };

        app.apply_server_event(late_error)
            .await
            .expect("apply late queue error");

        let events = sink.snapshot().await;
        assert!(events.iter().any(|event| {
            matches!(
                event,
                QconnectAppEvent::QueueErrorIgnoredByConcurrency { action_uuid }
                if action_uuid == "85fa0dd6-7bd6-4b3c-8f43-b8ee22e65d5e"
            )
        }));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, QconnectAppEvent::QueueUpdated(_))),
            "ignored late queue error should not mutate queue"
        );
    }

    #[tokio::test]
    async fn matching_session_management_event_completes_pending_action() {
        let (app, sink, _transport, _events_rx) = build_connected_app().await;
        let command = app
            .build_queue_command(
                QueueCommandType::CtrlSrvrSetActiveRenderer,
                json!({ "active_renderer_id": 1 }),
            )
            .await;
        let pending_uuid = app
            .send_queue_command(command)
            .await
            .expect("send set-active-renderer command");

        let matching_event = QueueServerEvent {
            event_type: QueueEventType::SrvrCtrlActiveRendererChanged,
            action_uuid: Some(pending_uuid.clone()),
            queue_version: Some(QueueVersion::new(1, 1)),
            payload: json!({ "active_renderer_id": 1 }),
        };

        app.apply_server_event(matching_event)
            .await
            .expect("apply active-renderer-changed event");

        let second_command = app
            .build_queue_command(
                QueueCommandType::CtrlSrvrQueueLoadTracks,
                json!({ "track_ids": [101, 102] }),
            )
            .await;

        app.send_queue_command(second_command)
            .await
            .expect("pending action should be cleared by matching session event");

        let events = sink.snapshot().await;
        assert!(events.iter().any(|event| {
            matches!(
                event,
                QconnectAppEvent::PendingActionCompleted { uuid } if uuid == &pending_uuid
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                QconnectAppEvent::SessionManagementEvent { message_type, .. }
                if message_type == "MESSAGE_TYPE_SRVR_CTRL_ACTIVE_RENDERER_CHANGED"
            )
        }));
    }

    #[tokio::test]
    async fn active_renderer_changed_without_action_uuid_completes_matching_pending_action() {
        let (app, sink, _transport, _events_rx) = build_connected_app().await;
        let command = app
            .build_queue_command(
                QueueCommandType::CtrlSrvrSetActiveRenderer,
                json!({ "renderer_id": 16 }),
            )
            .await;
        let pending_uuid = app
            .send_queue_command(command)
            .await
            .expect("send set-active-renderer command");

        app.apply_server_event(QueueServerEvent {
            event_type: QueueEventType::SrvrCtrlActiveRendererChanged,
            action_uuid: None,
            queue_version: Some(QueueVersion::new(1, 1)),
            payload: json!({ "active_renderer_id": 16 }),
        })
        .await
        .expect("apply active-renderer-changed event without action uuid");

        let second_command = app
            .build_queue_command(
                QueueCommandType::CtrlSrvrQueueLoadTracks,
                json!({ "track_ids": [201, 202] }),
            )
            .await;

        app.send_queue_command(second_command)
            .await
            .expect("matching active-renderer change should clear pending action");

        let events = sink.snapshot().await;
        assert!(events.iter().any(|event| {
            matches!(
                event,
                QconnectAppEvent::PendingActionCompleted { uuid } if uuid == &pending_uuid
            )
        }));
    }

    #[tokio::test]
    async fn renderer_state_update_completes_transport_control_pending_without_action_uuid() {
        let (app, sink, _transport, _events_rx) = build_connected_app().await;
        let command = app
            .build_queue_command(
                QueueCommandType::CtrlSrvrSetPlayerState,
                json!({
                    "playing_state": 2,
                    "current_position": 0,
                    "current_queue_item": {
                        "queue_version": { "major": 1, "minor": 1 },
                        "id": 1
                    }
                }),
            )
            .await;
        let pending_uuid = app
            .send_queue_command(command)
            .await
            .expect("send set-player-state command");

        app.apply_server_event(QueueServerEvent {
            event_type: QueueEventType::SrvrCtrlRendererStateUpdated,
            action_uuid: None,
            queue_version: Some(QueueVersion::new(1, 1)),
            payload: json!({
                "renderer_id": 1,
                "status": 1,
                "player_state": {
                    "playing_state": 2,
                    "current_position": 1234,
                    "current_queue_item_id": 1
                }
            }),
        })
        .await
        .expect("apply renderer-state-updated event");

        let second_command = app
            .build_queue_command(
                QueueCommandType::CtrlSrvrSetPlayerState,
                json!({
                    "playing_state": 2,
                    "current_position": 0,
                    "current_queue_item": {
                        "queue_version": { "major": 1, "minor": 1 },
                        "id": 2
                    }
                }),
            )
            .await;

        app.send_queue_command(second_command)
            .await
            .expect("transport control pending should clear after renderer-state update");

        let events = sink.snapshot().await;
        assert!(events.iter().any(|event| {
            matches!(
                event,
                QconnectAppEvent::PendingActionCompleted { uuid } if uuid == &pending_uuid
            )
        }));
    }

    #[tokio::test]
    async fn loop_mode_set_without_action_uuid_completes_matching_pending_action() {
        let (app, sink, _transport, _events_rx) = build_connected_app().await;
        let command = app
            .build_queue_command(
                QueueCommandType::CtrlSrvrSetLoopMode,
                json!({ "loop_mode": 3 }),
            )
            .await;
        let pending_uuid = app
            .send_queue_command(command)
            .await
            .expect("send set-loop-mode command");

        app.apply_server_event(QueueServerEvent {
            event_type: QueueEventType::SrvrCtrlLoopModeSet,
            action_uuid: None,
            queue_version: Some(QueueVersion::new(1, 1)),
            payload: json!({ "loop_mode": 3 }),
        })
        .await
        .expect("apply loop-mode-set event without action uuid");

        let second_command = app
            .build_queue_command(
                QueueCommandType::CtrlSrvrSetLoopMode,
                json!({ "loop_mode": 2 }),
            )
            .await;

        app.send_queue_command(second_command)
            .await
            .expect("loop-mode-set event should clear pending action");

        let events = sink.snapshot().await;
        assert!(events.iter().any(|event| {
            matches!(
                event,
                QconnectAppEvent::PendingActionCompleted { uuid } if uuid == &pending_uuid
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                QconnectAppEvent::SessionManagementEvent { message_type, .. }
                if message_type == "MESSAGE_TYPE_SRVR_CTRL_LOOP_MODE_SET"
            )
        }));
    }

    #[tokio::test]
    async fn inbound_renderer_command_updates_renderer_state() {
        let (app, sink, transport, _events_rx) = build_connected_app().await;

        app.apply_renderer_server_command(RendererServerCommand {
            command_type: RendererCommandType::SrvrRndrSetState,
            payload: json!({
                "playing_state": 2,
                "current_position": 65321,
                "current_track": {
                    "track_context_uuid": "ctx-remote",
                    "track_id": 777001,
                    "queue_item_id": 991
                }
            }),
        })
        .await
        .expect("apply set-state command");

        app.apply_renderer_server_command(RendererServerCommand {
            command_type: RendererCommandType::SrvrRndrSetVolume,
            payload: json!({
                "volume": 52,
                "volume_delta": 3
            }),
        })
        .await
        .expect("apply set-volume command");

        let renderer = app.renderer_state_snapshot().await;
        assert_eq!(renderer.playing_state, Some(2));
        assert_eq!(renderer.current_position_ms, Some(65_321));
        assert_eq!(renderer.volume, Some(55));
        assert_eq!(renderer.volume_delta, Some(3));
        assert_eq!(
            renderer
                .current_track
                .as_ref()
                .map(|item| item.queue_item_id),
            Some(991)
        );

        let events = sink.snapshot().await;
        assert!(
            events
                .iter()
                .filter(|event| matches!(event, QconnectAppEvent::RendererUpdated(_)))
                .count()
                >= 2
        );
        assert!(events
            .iter()
            .any(|event| matches!(event, QconnectAppEvent::RendererCommandApplied { .. })));

        let sent = transport.sent_messages().await;
        assert!(sent
            .iter()
            .any(|msg| msg.message_type == "MESSAGE_TYPE_RNDR_SRVR_STATE_UPDATED"));
        assert!(sent
            .iter()
            .any(|msg| msg.message_type == "MESSAGE_TYPE_RNDR_SRVR_VOLUME_CHANGED"));

        let state_update = sent
            .iter()
            .find(|msg| msg.message_type == "MESSAGE_TYPE_RNDR_SRVR_STATE_UPDATED")
            .expect("state update report");
        assert!(state_update
            .payload
            .get("current_queue_item_id")
            .expect("current_queue_item_id field")
            .is_null());
        assert!(state_update
            .payload
            .get("next_queue_item_id")
            .expect("next_queue_item_id field")
            .is_null());
    }
}
