//! QConnect (Qobuz Connect) integration for the TUI.
//!
//! Provides a service that connects to the Qobuz Connect WebSocket protocol,
//! making this TUI instance discoverable and controllable by other Qobuz devices.

use std::sync::Arc;

use async_trait::async_trait;
use qconnect_app::{QconnectApp, QconnectAppEvent, QconnectEventSink};
use qconnect_transport_ws::{NativeWsTransport, TransportEvent, WsTransportConfig};
use serde_json::Value;
use tokio::sync::mpsc;

use qbz_core::QbzCore;
use crate::adapter::TuiAdapter;

const QCONNECT_QWS_TOKEN_KIND: &str = "jwt_qws";
const QCONNECT_QWS_CREATE_TOKEN_PATH: &str = "/qws/createToken";

/// Events emitted by the QConnect service to the TUI event loop.
#[derive(Debug, Clone)]
pub enum QconnectTuiEvent {
    /// Transport connected to the QConnect WebSocket.
    Connected,
    /// Transport disconnected.
    Disconnected,
    /// An error occurred.
    Error(String),
    /// Session management event (device joined, left, etc.).
    SessionEvent { message_type: String },
}

/// Event sink that forwards QConnect events to the TUI via an mpsc channel.
pub struct TuiQconnectEventSink {
    tx: mpsc::UnboundedSender<QconnectTuiEvent>,
}

impl TuiQconnectEventSink {
    pub fn new(tx: mpsc::UnboundedSender<QconnectTuiEvent>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl QconnectEventSink for TuiQconnectEventSink {
    async fn on_event(&self, event: QconnectAppEvent) {
        let tui_event = match event {
            QconnectAppEvent::TransportConnected => QconnectTuiEvent::Connected,
            QconnectAppEvent::TransportDisconnected => QconnectTuiEvent::Disconnected,
            QconnectAppEvent::SessionManagementEvent { message_type, .. } => {
                QconnectTuiEvent::SessionEvent { message_type }
            }
            _ => return, // Other events logged but not forwarded to UI
        };
        let _ = self.tx.send(tui_event);
    }
}

/// Runtime handle for a running QConnect service.
pub struct QconnectServiceHandle {
    app: Arc<QconnectApp<NativeWsTransport, TuiQconnectEventSink>>,
    event_loop_handle: tokio::task::JoinHandle<()>,
}

/// Manages the QConnect service lifecycle for the TUI.
pub struct QconnectService {
    handle: Option<QconnectServiceHandle>,
}

impl QconnectService {
    pub fn new() -> Self {
        Self { handle: None }
    }

    /// Whether the service is currently running.
    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    /// Start the QConnect service. Fetches credentials from the Qobuz client
    /// and connects to the QConnect WebSocket.
    pub async fn start(
        &mut self,
        core: &Arc<QbzCore<TuiAdapter>>,
        event_tx: mpsc::UnboundedSender<QconnectTuiEvent>,
    ) -> Result<(), String> {
        if self.handle.is_some() {
            return Err("QConnect service is already running".into());
        }

        // Fetch transport credentials from the authenticated Qobuz client
        let config = fetch_transport_config(core).await?;

        let transport = Arc::new(NativeWsTransport::new());
        let sink = Arc::new(TuiQconnectEventSink::new(event_tx.clone()));
        let app = Arc::new(QconnectApp::new(transport, sink));

        // Connect the transport
        app.connect(config)
            .await
            .map_err(|err| format!("QConnect transport connect failed: {err}"))?;

        // Spawn event loop to process transport events
        let mut transport_rx = app.subscribe_transport_events();
        let app_for_loop = Arc::clone(&app);
        let event_tx_for_loop = event_tx;

        let event_loop_handle = tokio::spawn(async move {
            log::info!("[QConnect/TUI] Event loop started");
            let mut renderer_joined = false;
            loop {
                match transport_rx.recv().await {
                    Ok(event) => {
                        // Check for SESSION_STATE to trigger deferred renderer join
                        if !renderer_joined {
                            if let TransportEvent::InboundQueueServerEvent(ref evt) = event {
                                if evt.message_type() == "MESSAGE_TYPE_SRVR_CTRL_SESSION_STATE" {
                                    if let Some(session_uuid) =
                                        evt.payload.get("session_uuid").and_then(|v| v.as_str())
                                    {
                                        renderer_joined = true;
                                        log::info!(
                                            "[QConnect/TUI] Joining session as renderer: {}",
                                            session_uuid
                                        );
                                    }
                                }
                            }
                        }
                        match &event {
                            TransportEvent::Connected => {
                                log::info!("[QConnect/TUI] WebSocket connected");
                            }
                            TransportEvent::Disconnected => {
                                log::warn!("[QConnect/TUI] WebSocket disconnected");
                                renderer_joined = false;
                            }
                            TransportEvent::ReconnectScheduled {
                                attempt,
                                backoff_ms,
                                reason,
                            } => {
                                log::warn!(
                                    "[QConnect/TUI] Reconnect scheduled: attempt={} backoff={}ms reason={}",
                                    attempt, backoff_ms, reason
                                );
                            }
                            TransportEvent::TransportError { stage, message } => {
                                log::error!(
                                    "[QConnect/TUI] Transport error: stage={} message={}",
                                    stage, message
                                );
                                let _ = event_tx_for_loop.send(QconnectTuiEvent::Error(
                                    format!("{}: {}", stage, message),
                                ));
                            }
                            _ => {
                                log::debug!("[QConnect/TUI] Transport event: {:?}", event);
                            }
                        }

                        // Dispatch to app for state processing
                        if let Err(err) = app_for_loop.handle_transport_event(event).await {
                            log::warn!("[QConnect/TUI] Failed to handle transport event: {err}");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!("[QConnect/TUI] Event loop lagged by {n} events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        log::info!("[QConnect/TUI] Transport channel closed, exiting event loop");
                        break;
                    }
                }
            }
        });

        self.handle = Some(QconnectServiceHandle {
            app,
            event_loop_handle,
        });

        log::info!("[QConnect/TUI] Service started");
        Ok(())
    }

    /// Stop the QConnect service.
    pub async fn stop(&mut self) -> Result<(), String> {
        if let Some(handle) = self.handle.take() {
            handle
                .app
                .disconnect()
                .await
                .map_err(|err| format!("QConnect disconnect failed: {err}"))?;
            handle.event_loop_handle.abort();
            log::info!("[QConnect/TUI] Service stopped");
        }
        Ok(())
    }
}

/// Fetch QConnect transport credentials from the Qobuz API client.
async fn fetch_transport_config(
    core: &Arc<QbzCore<TuiAdapter>>,
) -> Result<WsTransportConfig, String> {
    // Check environment variables first
    let env_endpoint = std::env::var("QBZ_QCONNECT_WS_ENDPOINT").ok();
    let env_jwt = std::env::var("QBZ_QCONNECT_JWT_QWS")
        .or_else(|_| std::env::var("QBZ_QCONNECT_JWT"))
        .ok();

    let (endpoint_url, jwt_qws) = if env_endpoint.is_some() && env_jwt.is_some() {
        (env_endpoint.unwrap(), env_jwt)
    } else {
        // Auto-discover from Qobuz API
        let client_lock = core.client();
        let client_guard = client_lock.read().await;
        let client = client_guard
            .as_ref()
            .ok_or("QConnect requires an authenticated Qobuz session")?;

        let app_id = client
            .app_id()
            .await
            .map_err(|err| format!("QConnect requires initialized API client: {err}"))?;
        let user_auth_token = client
            .auth_token()
            .await
            .map_err(|err| format!("QConnect requires authenticated user: {err}"))?;

        let http = client.get_http();
        let url = qbz_qobuz::endpoints::build_url(QCONNECT_QWS_CREATE_TOKEN_PATH);

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "X-App-Id",
            reqwest::header::HeaderValue::from_str(&app_id)
                .map_err(|_| "invalid X-App-Id header value")?,
        );
        headers.insert(
            "X-User-Auth-Token",
            reqwest::header::HeaderValue::from_str(&user_auth_token)
                .map_err(|_| "invalid X-User-Auth-Token header value")?,
        );

        let response = http
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
            return Err(format!(
                "qws/createToken failed (HTTP {}): {}",
                status,
                serde_json::to_string(&payload).unwrap_or_default()
            ));
        }

        let discovered_endpoint = payload
            .get("endpoint")
            .and_then(|v| v.as_str())
            .map(String::from);
        let discovered_jwt = payload
            .get("token")
            .and_then(|v| v.as_str())
            .map(String::from);

        let final_endpoint = env_endpoint
            .or(discovered_endpoint)
            .ok_or("QConnect endpoint not found in qws/createToken response")?;
        let final_jwt = env_jwt.or(discovered_jwt);

        (final_endpoint, final_jwt)
    };

    Ok(WsTransportConfig {
        endpoint_url,
        jwt_qws,
        subscribe_channels: vec![vec![0x01], vec![0x02], vec![0x03]],
        ..WsTransportConfig::default()
    })
}
