//! Headless QConnect integration.
//!
//! Replicates the exact flow from qconnect_service.rs in the desktop app:
//! 1. Create transport + sink + app
//! 2. Connect transport
//! 3. Subscribe to transport events
//! 4. Start event loop (checks InboundQueueServerEvent for SESSION_STATE)
//! 5. Bootstrap: CtrlSrvrJoinSession + AskForQueueState
//! 6. Event loop receives SESSION_STATE → deferred renderer join

use std::sync::Arc;
use async_trait::async_trait;
use qconnect_app::{
    QconnectApp, QconnectAppEvent, QconnectEventSink,
    QueueCommandType, RendererReport, RendererReportType,
};
use qconnect_core::RendererCommand;
use qconnect_transport_ws::{NativeWsTransport, WsTransportConfig};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::adapter::{DaemonAdapter, DaemonEvent};

const PLAYING_STATE_STOPPED: i32 = 1;
const PLAYING_STATE_PLAYING: i32 = 2;
const PLAYING_STATE_PAUSED: i32 = 3;
const BUFFER_STATE_OK: i32 = 2;
const JOIN_SESSION_REASON_CONTROLLER_REQUEST: i32 = 1;
const AUDIO_QUALITY_HIRES_LEVEL2: i32 = 4;

type App = QconnectApp<NativeWsTransport, HeadlessQconnectSink>;

/// Event sink — handles renderer commands for playback.
pub struct HeadlessQconnectSink {
    event_tx: broadcast::Sender<DaemonEvent>,
    core: Arc<qbz_core::QbzCore<DaemonAdapter>>,
}

impl HeadlessQconnectSink {
    pub fn new(
        event_tx: broadcast::Sender<DaemonEvent>,
        core: Arc<qbz_core::QbzCore<DaemonAdapter>>,
    ) -> Self {
        Self { event_tx, core }
    }
}

#[async_trait]
impl QconnectEventSink for HeadlessQconnectSink {
    async fn on_event(&self, event: QconnectAppEvent) {
        match &event {
            QconnectAppEvent::TransportConnected => {
                log::info!("[qbzd/qconnect] Connected to Qobuz servers");
            }
            QconnectAppEvent::TransportDisconnected => {
                log::warn!("[qbzd/qconnect] Disconnected");
            }
            QconnectAppEvent::RendererCommandApplied { command, .. } => {
                log::info!("[qbzd/qconnect] Command: {:?}", command);
                handle_renderer_command(&self.core, command).await;
            }
            QconnectAppEvent::QueueUpdated(queue_state) => {
                log::info!("[qbzd/qconnect] Queue: {} items", queue_state.queue_items.len());
            }
            QconnectAppEvent::SessionManagementEvent { message_type, payload } => {
                log::info!("[qbzd/qconnect] Session mgmt: {}", message_type);
            }
            _ => {}
        }
    }
}

async fn handle_renderer_command(
    core: &qbz_core::QbzCore<DaemonAdapter>,
    command: &RendererCommand,
) {
    match command {
        RendererCommand::SetState { playing_state, current_position_ms, .. } => {
            if let Some(state) = playing_state {
                match *state {
                    PLAYING_STATE_PLAYING => { let _ = core.resume(); }
                    PLAYING_STATE_PAUSED => { let _ = core.pause(); }
                    PLAYING_STATE_STOPPED => { let _ = core.stop(); }
                    _ => {}
                }
            }
            if let Some(pos_ms) = current_position_ms {
                let _ = core.seek(*pos_ms / 1000);
            }
        }
        RendererCommand::SetVolume { volume, .. } => {
            if let Some(vol) = volume {
                let _ = core.set_volume((*vol as f32 / 100.0).clamp(0.0, 1.0));
            }
        }
        RendererCommand::SetShuffleMode { shuffle_mode } => {
            core.set_shuffle(*shuffle_mode).await;
        }
        RendererCommand::SetLoopMode { loop_mode } => {
            let mode = match *loop_mode {
                1 => qbz_models::RepeatMode::One,
                2 => qbz_models::RepeatMode::All,
                _ => qbz_models::RepeatMode::Off,
            };
            core.set_repeat_mode(mode).await;
        }
        RendererCommand::MuteVolume { value } => {
            if *value { let _ = core.set_volume(0.0); }
        }
        _ => {}
    }
}

/// Start QConnect — exact replica of desktop's QconnectServiceState::connect().
pub async fn start_qconnect(
    core: &Arc<qbz_core::QbzCore<DaemonAdapter>>,
    event_tx: broadcast::Sender<DaemonEvent>,
    device_name: &str,
) -> Option<Arc<App>> {
    // Step 1: Get credentials
    let client_arc = core.client();
    let client_guard = client_arc.read().await;
    let client = client_guard.as_ref()?;
    let (endpoint_url, jwt_qws) = fetch_qws_credentials(client).await?;
    drop(client_guard);

    // Step 2: Create transport + sink + app
    let transport = Arc::new(NativeWsTransport::new());
    let sink = Arc::new(HeadlessQconnectSink::new(event_tx, core.clone()));
    let app = Arc::new(QconnectApp::new(transport, sink));

    // Step 3: Connect transport
    let config = WsTransportConfig {
        endpoint_url,
        jwt_qws: Some(jwt_qws),
        ..Default::default()
    };
    if let Err(e) = app.connect(config).await {
        log::warn!("[qbzd/qconnect] Connect failed: {}", e);
        return None;
    }

    // Step 4: Subscribe to transport events BEFORE bootstrap
    // (desktop does this at line 1238, before bootstrap at line 1405)
    let mut transport_rx = app.subscribe_transport_events();

    // Step 5: Start event loop (same pattern as desktop lines 1242-1391)
    let app_for_loop = app.clone();
    let device_name_owned = device_name.to_string();
    tokio::spawn(async move {
        log::info!("[qbzd/qconnect] Event loop started");
        let mut renderer_joined = false;
        let mut has_disconnected = false;

        loop {
            match transport_rx.recv().await {
                Ok(event) => {
                    // Check for SESSION_STATE to trigger deferred renderer join
                    // (desktop line 1249-1265)
                    if !renderer_joined {
                        if let qconnect_transport_ws::TransportEvent::InboundQueueServerEvent(
                            ref evt,
                        ) = event
                        {
                            log::info!("[qbzd/qconnect] Queue server event: {}", evt.message_type());
                            if evt.message_type() == "MESSAGE_TYPE_SRVR_CTRL_SESSION_STATE" {
                                if let Some(session_uuid) =
                                    evt.payload.get("session_uuid").and_then(|v| v.as_str())
                                {
                                    renderer_joined = true;
                                    deferred_renderer_join(&app_for_loop, session_uuid, &device_name_owned).await;
                                } else {
                                    log::warn!("[qbzd/qconnect] SESSION_STATE but no session_uuid: {}", evt.payload);
                                }
                            }
                        }
                    }

                    // Handle transport state changes (desktop lines 1267-1374)
                    match &event {
                        qconnect_transport_ws::TransportEvent::Connected => {
                            log::info!("[qbzd/qconnect] WebSocket connected");
                        }
                        qconnect_transport_ws::TransportEvent::Disconnected => {
                            log::warn!("[qbzd/qconnect] WebSocket disconnected, resetting renderer_joined");
                            renderer_joined = false;
                            has_disconnected = true;
                        }
                        qconnect_transport_ws::TransportEvent::Authenticated => {
                            log::info!("[qbzd/qconnect] Authenticated with JWT");
                        }
                        qconnect_transport_ws::TransportEvent::Subscribed => {
                            log::info!("[qbzd/qconnect] Subscribed to channels");
                            if has_disconnected {
                                log::info!("[qbzd/qconnect] Re-bootstrapping after reconnect...");
                                if let Err(e) = bootstrap_remote_presence(&app_for_loop, &device_name_owned).await {
                                    log::error!("[qbzd/qconnect] Re-bootstrap failed: {}", e);
                                }
                            }
                        }
                        _ => {}
                    }

                    // Forward to QconnectApp for protocol handling
                    // (desktop line 1375)
                    if let Err(e) = app_for_loop.handle_transport_event(event).await {
                        log::error!("[qbzd/qconnect] Transport event error: {}", e);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("[qbzd/qconnect] Transport lagged {} events", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    log::info!("[qbzd/qconnect] Transport channel closed");
                    break;
                }
            }
        }
        log::info!("[qbzd/qconnect] Event loop ended");
    });

    // Step 6: Bootstrap AFTER event loop starts
    // (desktop line 1405)
    let device_name_for_bootstrap = device_name.to_string();
    if let Err(e) = bootstrap_remote_presence(&app, &device_name_for_bootstrap).await {
        log::error!("[qbzd/qconnect] Bootstrap failed: {}", e);
        // Desktop disconnects on bootstrap failure (line 1406)
        let _ = app.disconnect().await;
        return None;
    }

    log::info!("[qbzd/qconnect] Started as '{}'", device_name);
    Some(app)
}

/// Bootstrap: controller JoinSession + AskForQueueState.
/// Exact replica of desktop's bootstrap_remote_presence (lines 3801-3846).
async fn bootstrap_remote_presence(app: &Arc<App>, device_name: &str) -> Result<(), String> {
    let device_info = build_device_info(device_name);

    // 1. Controller JoinSession (works without session_uuid)
    let join_payload = serde_json::json!({
        "session_uuid": null,
        "device_info": device_info,
    });
    let join_cmd = app.build_queue_command(QueueCommandType::CtrlSrvrJoinSession, join_payload).await;
    let join_uuid = app.send_queue_command(join_cmd).await
        .map_err(|e| format!("join_session failed: {}", e))?;

    // Clear pending (desktop line 3825)
    {
        let handle = app.state_handle();
        let mut state = handle.lock().await;
        state.pending.clear();
    }

    // 2. Ask for current queue state from server
    let ask_cmd = app.build_queue_command(QueueCommandType::CtrlSrvrAskForQueueState, serde_json::json!({})).await;
    let ask_uuid = app.send_queue_command(ask_cmd).await
        .map_err(|e| format!("ask_queue_state failed: {}", e))?;

    {
        let handle = app.state_handle();
        let mut state = handle.lock().await;
        state.pending.clear();
    }

    log::info!("[qbzd/qconnect] Bootstrap: controller joined, queue requested. Renderer join deferred until SESSION_STATE.");
    Ok(())
}

/// Deferred renderer join — called when SESSION_STATE arrives with session_uuid.
/// Exact replica of desktop's deferred_renderer_join (lines 3849-3948).
async fn deferred_renderer_join(app: &Arc<App>, session_uuid: &str, device_name: &str) {
    let device_info = build_device_info(device_name);
    let queue_version = app.queue_state_snapshot().await.version;

    log::info!("[qbzd/qconnect] Renderer join with session_uuid={}", session_uuid);

    // 1. Renderer JoinSession with session_uuid
    let join_payload = serde_json::json!({
        "session_uuid": session_uuid,
        "device_info": device_info,
        "is_active": true,
        "reason": JOIN_SESSION_REASON_CONTROLLER_REQUEST,
        "initial_state": {
            "playing_state": PLAYING_STATE_STOPPED,
            "buffer_state": BUFFER_STATE_OK,
            "current_position": 0,
            "duration": 0,
            "queue_version": {
                "major": queue_version.major,
                "minor": queue_version.minor
            }
        }
    });
    let join_report = RendererReport::new(
        RendererReportType::RndrSrvrJoinSession,
        Uuid::new_v4().to_string(),
        queue_version,
        join_payload,
    );
    if let Err(e) = app.send_renderer_report_command(join_report).await {
        log::error!("[qbzd/qconnect] Renderer join failed: {}", e);
        return;
    }

    // 2. State report
    let state_payload = serde_json::json!({
        "playing_state": PLAYING_STATE_STOPPED,
        "buffer_state": BUFFER_STATE_OK,
        "current_position": 0,
        "duration": 0,
        "queue_version": { "major": queue_version.major, "minor": queue_version.minor }
    });
    let state_report = RendererReport::new(
        RendererReportType::RndrSrvrStateUpdated,
        Uuid::new_v4().to_string(),
        queue_version,
        state_payload,
    );
    let _ = app.send_renderer_report_command(state_report).await;

    // 3. Volume report
    let vol_report = RendererReport::new(
        RendererReportType::RndrSrvrVolumeChanged,
        Uuid::new_v4().to_string(),
        queue_version,
        serde_json::json!({ "volume": 100 }),
    );
    let _ = app.send_renderer_report_command(vol_report).await;

    // 4. Max quality report
    let quality_report = RendererReport::new(
        RendererReportType::RndrSrvrMaxAudioQualityChanged,
        Uuid::new_v4().to_string(),
        queue_version,
        serde_json::json!({ "max_audio_quality": AUDIO_QUALITY_HIRES_LEVEL2 }),
    );
    let _ = app.send_renderer_report_command(quality_report).await;

    // 5. Refresh session state (desktop line 3936-3947)
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let refresh_cmd = app.build_queue_command(QueueCommandType::CtrlSrvrAskForQueueState, serde_json::json!({})).await;
    let _ = app.send_queue_command(refresh_cmd).await;

    log::info!("[qbzd/qconnect] Renderer join complete — visible to other devices");
}

fn build_device_info(device_name: &str) -> serde_json::Value {
    serde_json::json!({
        "friendly_name": device_name,
        "brand": "QBZ",
        "model": "QBZ Daemon",
        "device_type": 5,
        "software_version": format!("qbzd/{}", env!("CARGO_PKG_VERSION")),
    })
}

async fn fetch_qws_credentials(client: &qbz_qobuz::QobuzClient) -> Option<(String, String)> {
    let app_id = client.app_id().await.ok()?;
    let auth_token = client.auth_token().await.ok()?;

    let http = reqwest::Client::new();
    let resp = http
        .post("https://www.qobuz.com/api.json/0.2/qws/createToken")
        .header("X-App-Id", &app_id)
        .header("X-User-Auth-Token", &auth_token)
        .form(&[
            ("jwt", "jwt_qws"),
            ("user_auth_token_needed", "true"),
            ("strong_auth_needed", "true"),
        ])
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        log::warn!("[qbzd/qconnect] qws/createToken: {}", resp.status());
        return None;
    }

    let body: serde_json::Value = resp.json().await.ok()?;
    let jwt_qws = body.get("jwt_qws")?;
    let endpoint = jwt_qws.get("endpoint")?.as_str()?.to_string();
    let jwt = jwt_qws.get("jwt")?.as_str()?.to_string();

    Some((endpoint, jwt))
}
