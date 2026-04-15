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
use qconnect_core::{QConnectQueueState, RendererCommand};
use qconnect_transport_ws::{NativeWsTransport, WsTransportConfig};
use qbz_models::{QueueTrack, Track};
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
    qconnect_next_track_id: Arc<std::sync::atomic::AtomicU64>,
}

impl HeadlessQconnectSink {
    pub fn new(
        event_tx: broadcast::Sender<DaemonEvent>,
        core: Arc<qbz_core::QbzCore<DaemonAdapter>>,
        qconnect_next_track_id: Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        Self { event_tx, core, qconnect_next_track_id }
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
                handle_renderer_command(&self.core, command, &self.qconnect_next_track_id).await;
            }
            QconnectAppEvent::QueueUpdated(queue_state) => {
                log::info!("[qbzd/qconnect] Queue: {} items", queue_state.queue_items.len());
                if let Err(e) = materialize_remote_queue(&self.core, queue_state).await {
                    log::warn!("[qbzd/qconnect] Failed to materialize queue: {}", e);
                }
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
    qconnect_next_track_id: &Arc<std::sync::atomic::AtomicU64>,
) {
    match command {
        RendererCommand::SetState { playing_state, current_position_ms, current_track, next_track } => {
            // Track the next_track for orchestrator's auto-advance fallback
            if let Some(nt) = next_track {
                qconnect_next_track_id.store(nt.track_id, std::sync::atomic::Ordering::Release);
                log::info!("[qbzd/qconnect] Stored next_track for auto-advance: {}", nt.track_id);
            }

            // Step 1: If a current_track is specified, ensure it's loaded
            // (matches desktop's ensure_remote_track_loaded pattern)
            if let Some(track) = current_track {
                let player = core.player();
                let loaded_track_id = player.state.current_track_id();
                if loaded_track_id != track.track_id {
                    log::info!(
                        "[qbzd/qconnect] New track from renderer: {} (was {})",
                        track.track_id, loaded_track_id
                    );
                    match download_and_play_track(core, track.track_id).await {
                        Ok(()) => log::info!("[qbzd/qconnect] Track {} loaded", track.track_id),
                        Err(e) => {
                            log::error!("[qbzd/qconnect] Failed to load track {}: {}", track.track_id, e);
                            return;
                        }
                    }
                }
            }

            // Step 2: Apply playing state (only if it actually changes)
            if let Some(state) = playing_state {
                let player = core.player();
                let already_playing = player.state.is_playing();
                match *state {
                    PLAYING_STATE_PLAYING if !already_playing => { let _ = core.resume(); }
                    PLAYING_STATE_PAUSED if already_playing => { let _ = core.pause(); }
                    PLAYING_STATE_STOPPED => { let _ = core.stop(); }
                    _ => {}
                }
            }

            // Step 3: Seek only for intentional seeks (>2s diff),
            // but ignore server echoes that want position 0 when we're already playing
            if let Some(pos_ms) = current_position_ms {
                let target_secs = *pos_ms / 1000;
                let current_secs = core.player().state.current_position();
                let is_playing = core.player().state.is_playing();
                // Skip seek-to-zero echoes from server refresh while playing
                let is_server_echo = target_secs <= 1 && is_playing && current_secs > 2;
                if current_secs.abs_diff(target_secs) > 2 && !is_server_echo {
                    let _ = core.seek(target_secs);
                }
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

/// Download a track and feed it to the player.
/// Mirrors desktop's load_remote_track_into_player → download fallback path.
async fn download_and_play_track(
    core: &qbz_core::QbzCore<DaemonAdapter>,
    track_id: u64,
) -> Result<(), String> {
    let quality = qbz_models::Quality::HiRes;
    let stream_url = core.get_stream_url(track_id, quality).await
        .map_err(|e| format!("Stream URL failed for {}: {}", track_id, e))?;

    log::info!(
        "[qbzd/qconnect] Downloading track {} ({:.0}kHz/{}bit)",
        track_id, stream_url.sampling_rate, stream_url.bit_depth.unwrap_or(0)
    );

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let data = http.get(&stream_url.url).send().await
        .map_err(|e| format!("Download failed for {}: {}", track_id, e))?
        .bytes().await
        .map_err(|e| format!("Read failed for {}: {}", track_id, e))?
        .to_vec();

    log::info!("[qbzd/qconnect] Track {} downloaded ({} bytes)", track_id, data.len());

    core.player()
        .play_data(data, track_id)
        .map_err(|e| format!("Player error for {}: {}", track_id, e))?;

    Ok(())
}

/// Materialize the QConnect remote queue into the local QbzCore queue.
/// This enables the local orchestrator's auto-advance and gapless logic
/// to work when QConnect is the playback source.
/// Mirrors desktop's materialize_remote_queue from qconnect_service.rs.
async fn materialize_remote_queue(
    core: &qbz_core::QbzCore<DaemonAdapter>,
    queue_state: &QConnectQueueState,
) -> Result<(), String> {
    if queue_state.queue_items.is_empty() {
        log::info!("[qbzd/qconnect] Empty queue, skipping materialization");
        return Ok(());
    }

    // Dedupe track_ids before batch fetch
    let mut unique_ids: Vec<u64> = Vec::with_capacity(queue_state.queue_items.len());
    for item in &queue_state.queue_items {
        if !unique_ids.contains(&item.track_id) {
            unique_ids.push(item.track_id);
        }
    }

    log::info!(
        "[qbzd/qconnect] Materializing remote queue: {} items, {} unique tracks",
        queue_state.queue_items.len(),
        unique_ids.len()
    );

    let fetched_tracks = core
        .get_tracks_batch(&unique_ids)
        .await
        .map_err(|e| format!("batch fetch failed: {}", e))?;

    let mut tracks_by_id: std::collections::HashMap<u64, QueueTrack> =
        std::collections::HashMap::with_capacity(fetched_tracks.len());
    for track in fetched_tracks {
        tracks_by_id.insert(track.id, track_to_queue_track(&track));
    }

    // Build the queue in QConnect order
    let mut queue_tracks = Vec::with_capacity(queue_state.queue_items.len());
    for item in &queue_state.queue_items {
        if let Some(qt) = tracks_by_id.get(&item.track_id) {
            queue_tracks.push(qt.clone());
        } else {
            log::warn!(
                "[qbzd/qconnect] Track {} missing from batch fetch, skipping",
                item.track_id
            );
        }
    }

    if queue_tracks.is_empty() {
        return Err("zero playable tracks after materialization".to_string());
    }

    // Determine start index from currently playing track (if any)
    let player = core.player();
    let current_track_id = player.state.current_track_id();
    let start_index = if current_track_id != 0 {
        queue_state
            .queue_items
            .iter()
            .position(|item| item.track_id == current_track_id)
    } else {
        Some(0)
    };

    log::info!(
        "[qbzd/qconnect] Setting local queue: {} tracks, start_index={:?}",
        queue_tracks.len(),
        start_index
    );

    core.set_queue(queue_tracks, start_index).await;
    Ok(())
}

/// Convert qbz_models::Track to QueueTrack.
/// Mirrors desktop's model_track_to_core_queue_track.
fn track_to_queue_track(track: &Track) -> QueueTrack {
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|album| album.image.large.clone());
    let artist = track
        .performer
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album_title = track
        .album
        .as_ref()
        .map(|a| a.title.clone())
        .unwrap_or_else(|| "Unknown Album".to_string());
    let album_id = track.album.as_ref().and_then(|a| {
        let trimmed = a.id.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    });
    let artist_id = track.performer.as_ref().map(|p| p.id);

    QueueTrack {
        id: track.id,
        title: track.title.clone(),
        artist,
        album: album_title,
        duration_secs: track.duration as u64,
        artwork_url,
        hires: track.hires,
        bit_depth: track.maximum_bit_depth,
        sample_rate: track.maximum_sampling_rate,
        is_local: false,
        album_id,
        artist_id,
        streamable: track.streamable,
        source: Some("qobuz".to_string()),
        parental_warning: track.parental_warning,
    }
}

/// Start QConnect — exact replica of desktop's QconnectServiceState::connect().
pub async fn start_qconnect(
    core: &Arc<qbz_core::QbzCore<DaemonAdapter>>,
    event_tx: broadcast::Sender<DaemonEvent>,
    device_name: &str,
    qconnect_next_track_id: Arc<std::sync::atomic::AtomicU64>,
) -> Option<Arc<App>> {
    // Step 1: Get credentials
    let client_arc = core.client();
    let client_guard = client_arc.read().await;
    let client = client_guard.as_ref()?;
    let (endpoint_url, jwt_qws) = fetch_qws_credentials(client).await?;
    drop(client_guard);

    // Step 2: Create transport + sink + app
    let transport = Arc::new(NativeWsTransport::new());
    let sink = Arc::new(HeadlessQconnectSink::new(event_tx, core.clone(), qconnect_next_track_id));
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

    // Step 7: Spawn periodic state reporter so iOS knows the renderer's state
    spawn_state_reporter(app.clone(), core.clone());

    log::info!("[qbzd/qconnect] Started as '{}'", device_name);
    Some(app)
}

/// Periodic renderer state reporter — keeps iOS in sync with daemon playback.
/// Reports every 2 seconds, and immediately on track end / state change.
fn spawn_state_reporter(app: Arc<App>, core: Arc<qbz_core::QbzCore<DaemonAdapter>>) {
    tokio::spawn(async move {
        let mut last_playing_state: i32 = 0;
        let mut last_track_id: u64 = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            let player = core.player();
            let state = &player.state;
            let track_id = state.current_track_id();
            let is_playing = state.is_playing();
            let position = state.current_position();
            let duration = state.duration();

            let playing_state = if is_playing {
                PLAYING_STATE_PLAYING
            } else if track_id != 0 {
                PLAYING_STATE_PAUSED
            } else {
                PLAYING_STATE_STOPPED
            };

            let state_changed = playing_state != last_playing_state || track_id != last_track_id;
            last_playing_state = playing_state;
            last_track_id = track_id;

            let queue_version = app.queue_state_snapshot().await.version;
            let payload = serde_json::json!({
                "playing_state": playing_state,
                "buffer_state": BUFFER_STATE_OK,
                "current_position": position * 1000, // ms
                "duration": duration * 1000, // ms
                "queue_version": {
                    "major": queue_version.major,
                    "minor": queue_version.minor
                }
            });

            let report = RendererReport::new(
                RendererReportType::RndrSrvrStateUpdated,
                Uuid::new_v4().to_string(),
                queue_version,
                payload,
            );
            if let Err(e) = app.send_renderer_report_command(report).await {
                log::debug!("[qbzd/qconnect] State report failed: {}", e);
            } else if state_changed {
                log::info!(
                    "[qbzd/qconnect] State report sent: playing_state={} track={} pos={}s",
                    playing_state, track_id, position
                );
            }
        }
    });
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

/// Build device_info matching desktop's QconnectDeviceInfoPayload exactly.
/// device_uuid is persistent per instance (generated once, reused).
fn build_device_info(device_name: &str) -> serde_json::Value {
    use std::sync::OnceLock;
    static DEVICE_UUID: OnceLock<String> = OnceLock::new();
    let uuid = DEVICE_UUID.get_or_init(|| {
        // Try to load persisted UUID, or generate a new one
        let uuid_path = dirs::data_dir()
            .unwrap_or_default()
            .join("qbz")
            .join(".qconnect-device-uuid");
        if let Ok(existing) = std::fs::read_to_string(&uuid_path) {
            let trimmed = existing.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
        let new_uuid = Uuid::new_v4().to_string();
        let _ = std::fs::create_dir_all(uuid_path.parent().unwrap_or(&uuid_path));
        let _ = std::fs::write(&uuid_path, &new_uuid);
        new_uuid
    });

    serde_json::json!({
        "device_uuid": uuid,
        "friendly_name": device_name,
        "brand": "QBZ",
        "model": "QBZ Daemon",
        "serial_number": null,
        "device_type": 5,
        "capabilities": {
            "min_audio_quality": 1,
            "max_audio_quality": 4,
            "volume_remote_control": 2
        },
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
