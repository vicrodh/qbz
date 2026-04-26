#[derive(Debug, Clone)]
pub struct WsTransportConfig {
    pub endpoint_url: String,
    pub jwt_qws: Option<String>,
    pub reconnect_backoff_ms: u64,
    pub reconnect_backoff_max_ms: u64,
    /// Maximum number of consecutive reconnect attempts before the transport
    /// gives up and shuts down. The counter resets only when a session-level
    /// join is confirmed (cloud emits MESSAGE_TYPE_SRVR_CTRL_SESSION_STATE),
    /// not when the WS / TCP connection succeeds — Qobuz cloud accepts the WS
    /// connection before rejecting the session join, so a TCP-level reset
    /// would mask the failure mode behind issue #358.
    ///
    /// `None` means unlimited (legacy behavior, retained for tests).
    pub reconnect_max_attempts: Option<u32>,
    pub connect_timeout_ms: u64,
    pub keepalive_interval_ms: u64,
    pub auto_subscribe: bool,
    pub subscribe_channels: Vec<Vec<u8>>,
    pub qcloud_proto: u32,
}

impl Default for WsTransportConfig {
    fn default() -> Self {
        Self {
            endpoint_url: String::new(),
            jwt_qws: None,
            reconnect_backoff_ms: 2_000,
            reconnect_backoff_max_ms: 30_000,
            reconnect_max_attempts: Some(10),
            connect_timeout_ms: 10_000,
            keepalive_interval_ms: 30_000,
            auto_subscribe: true,
            subscribe_channels: Vec::new(),
            qcloud_proto: 1,
        }
    }
}
