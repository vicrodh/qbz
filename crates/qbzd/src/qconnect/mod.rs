// TODO(converge: qconnect-glue) — derived from crates/qbz/src/qconnect_service.rs @ 5d50158e
// (the connect/disconnect facade + startup auto-connect, UI stripped);
// do not fix bugs here without fixing the source, and vice versa.
//
//! Daemon QConnect service facade + boot-step-12 entry point.
//!
//! Composes the copied glue (engine / sink / session / report / transport /
//! remote_stream) into a headless connect flow that reproduces the desktop
//! `SlintQconnectService::connect` recipe (build transport -> one shared
//! sync-state Mutex -> sink -> `QconnectApp::new` -> `set_app` -> `connect` ->
//! subscribe transport events BEFORE the spawn -> spawn `run_session_loop` ->
//! `bootstrap_remote_presence`), minus every UI surface. Lifecycle transitions
//! latch into `DaemonShared.qconnect` so `/api/status` stays diagnosable.
//!
//! `start()` mints the daemon's OWN device identity in the daemon-root KV, reads
//! the effective startup mode (cli_override = None — never shadow the KV that
//! T11/T13 write), and, when auto-connect is on, spawns a connect-on-Ready task
//! with the bounded [2s, 5s, 15s, 30s] retry schedule. QConnect reads NOTHING
//! from qbzd.toml — only the daemon-root `qconnect_settings.db`.

pub mod engine;
pub mod remote_stream;
pub mod report;
pub mod session;
pub mod sink;
pub mod transport;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use qbz_app::shell::AppRuntime;
use qconnect_app::{
    compute_effective_startup, QconnectApp, QconnectAppEvent, QconnectEventSink,
    QconnectLifecycleState, QconnectRemoteSyncState, QconnectSessionState, SessionLoopHost,
};
use qconnect_transport_ws::{NativeWsTransport, WsTransportConfig};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::adapter::DaemonAdapter;
use crate::paths::ProfileRoots;
use crate::state::{AuthState, DaemonShared};

use self::engine::DaemonRendererEngine;
use self::session::{bootstrap_remote_presence, DaemonSessionLoopHost};
use self::sink::{DaemonEventSink, DaemonQconnectApp};

type Runtime = Arc<AppRuntime<DaemonAdapter>>;
type SharedState = Arc<std::sync::Mutex<DaemonShared>>;

/// The live QConnect runtime for one connected session (app + its config + the
/// spawned event loop + the shared sync accumulator).
pub(crate) struct DaemonQconnectRuntime {
    pub app: Arc<DaemonQconnectApp>,
    /// Re-latched by `bootstrap_after_reconnect` on a credential re-resolve;
    /// consumed by the full-reconnect path (T10) + status/endpoint reporting.
    #[allow(dead_code)]
    pub config: WsTransportConfig,
    pub event_loop: JoinHandle<()>,
    pub sync_state: Arc<Mutex<QconnectRemoteSyncState>>,
}

/// Connect-flow state, mirrored on the desktop `SlintQconnectInner`. `pub(crate)`
/// fields so `session::DaemonSessionLoopHost` can gate lifecycle + re-latch the
/// config + drop the runtime on reconnect-exhausted.
#[derive(Default)]
pub(crate) struct DaemonQconnectInner {
    pub runtime: Option<DaemonQconnectRuntime>,
    /// Latched connect/loop error; surfaced by `qbzd status` QConnect block (T11).
    #[allow(dead_code)]
    pub last_error: Option<String>,
    pub lifecycle_state: QconnectLifecycleState,
}

/// Map a lifecycle state to the `/api/status` `qconnect.state` label + the
/// session-active flag, and latch it into `DaemonShared`.
fn latch_lifecycle_into_shared(shared: &SharedState, state: QconnectLifecycleState) {
    let (label, active) = match state {
        QconnectLifecycleState::Off => ("off", false),
        QconnectLifecycleState::Connecting => ("connecting", false),
        QconnectLifecycleState::Connected => ("connected", true),
        QconnectLifecycleState::Reconnecting => ("retrying", false),
        QconnectLifecycleState::Exhausted => ("exhausted", false),
    };
    if let Ok(mut s) = shared.lock() {
        s.qconnect.state = label.to_string();
        s.qconnect.session_active = active;
        if matches!(state, QconnectLifecycleState::Reconnecting) {
            s.qconnect.last_transport_reconnect = Some(unix_seconds_string());
        }
    }
}

fn unix_seconds_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

/// Dedup + gate a lifecycle transition: only emit while a runtime is alive and
/// the state actually changes. Mirrors the desktop
/// `update_lifecycle_state_if_running`, plus the `DaemonShared` latch.
pub(crate) async fn update_lifecycle_state_if_running(
    inner: &Arc<Mutex<DaemonQconnectInner>>,
    sink: &DaemonEventSink,
    shared: &SharedState,
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
    latch_lifecycle_into_shared(shared, next);
    sink.on_event(QconnectAppEvent::LifecycleChanged { state: next })
        .await;
}

/// The headless QConnect connect service.
pub struct DaemonQconnectService {
    inner: Arc<Mutex<DaemonQconnectInner>>,
    runtime: Runtime,
    shared: SharedState,
    #[allow(dead_code)] // T11 (settings reload) re-reads the KV through this path.
    settings_db: PathBuf,
    custom_device_name: Arc<tokio::sync::RwLock<Option<String>>>,
}

impl DaemonQconnectService {
    /// Establish the QConnect session. Gated on an initialized API client (the
    /// qws/createToken discovery needs it). Idempotent: a second call while a
    /// runtime is alive (or a connect is in flight) is a no-op.
    pub async fn connect(&self) -> Result<(), String> {
        if !self.runtime.core().is_api_initialized().await {
            return Err("Qobuz API is not initialized; cannot start Qobuz Connect".to_string());
        }

        // Claim the connect slot ATOMICALLY before the transport-config await, so
        // two concurrent connect()s can't both build a runtime. A live runtime OR
        // an in-flight `Connecting` both short-circuit to a no-op.
        {
            let mut guard = self.inner.lock().await;
            if guard.runtime.is_some()
                || guard.lifecycle_state == QconnectLifecycleState::Connecting
            {
                log::info!(
                    "[QConnect] connect() called while already {:?}; no-op",
                    guard.lifecycle_state
                );
                return Ok(());
            }
            guard.lifecycle_state = QconnectLifecycleState::Connecting;
            guard.last_error = None;
        }
        latch_lifecycle_into_shared(&self.shared, QconnectLifecycleState::Connecting);

        let config = match transport::resolve_transport_config(&self.runtime).await {
            Ok(config) => config,
            Err(err) => {
                let mut guard = self.inner.lock().await;
                if guard.runtime.is_none() {
                    guard.lifecycle_state = QconnectLifecycleState::Off;
                }
                drop(guard);
                latch_lifecycle_into_shared(&self.shared, QconnectLifecycleState::Off);
                return Err(err);
            }
        };

        let transport = Arc::new(NativeWsTransport::new());
        let sync_state = Arc::new(Mutex::new(QconnectRemoteSyncState::default()));
        let engine = DaemonRendererEngine::new(Arc::clone(&self.runtime));
        let sink = Arc::new(DaemonEventSink::new(engine, Arc::clone(&sync_state)));
        let app = Arc::new(QconnectApp::new(
            Arc::clone(&transport),
            Arc::clone(&sink),
            Arc::clone(&sync_state),
        ));
        // Wire the owning app into the sink so it can emit reports + drive
        // session-apply.
        sink.set_app(&app);

        if let Err(err) = app.connect(config.clone()).await {
            let mut guard = self.inner.lock().await;
            guard.lifecycle_state = QconnectLifecycleState::Off;
            let msg = format!("qconnect transport connect failed: {err}");
            guard.last_error = Some(msg.clone());
            drop(guard);
            latch_lifecycle_into_shared(&self.shared, QconnectLifecycleState::Off);
            return Err(msg);
        }

        // Subscribe to transport events SYNCHRONOUSLY here — after connect()
        // returns and BEFORE the spawn / any further await — so the receiver is
        // live before the WS handshake emits Connected / Subscribed /
        // SessionEstablished / SESSION_STATE. tokio broadcast has no replay; a
        // receiver created inside the spawned loop would race + drop those.
        let transport_rx = app.subscribe_transport_events();
        let idle_retry_active = config.reconnect_idle_retry_ms > 0;
        let host: Arc<dyn SessionLoopHost> = Arc::new(DaemonSessionLoopHost {
            app: Arc::clone(&app),
            sync_state: Arc::clone(&sync_state),
            inner: Arc::clone(&self.inner),
            sink: Arc::clone(&sink),
            runtime: Arc::clone(&self.runtime),
            shared: Arc::clone(&self.shared),
        });
        let app_for_loop = Arc::clone(&app);
        let event_loop = tokio::spawn(async move {
            app_for_loop
                .run_session_loop(host, transport_rx, idle_retry_active)
                .await;
        });

        let runtime_app = Arc::clone(&app);
        {
            let mut guard = self.inner.lock().await;
            guard.last_error = None;
            guard.runtime = Some(DaemonQconnectRuntime {
                app,
                config,
                event_loop,
                sync_state,
            });
        }

        let custom_name = self.custom_device_name.read().await.clone();
        if let Err(err) = bootstrap_remote_presence(&runtime_app, custom_name.clone()).await {
            let _ = self.disconnect().await;
            let mut guard = self.inner.lock().await;
            guard.last_error = Some(format!("qconnect bootstrap failed: {err}"));
            return Err(format!("qconnect bootstrap failed: {err}"));
        }

        // Reflect the resolved device name in `/api/status`.
        let effective_name = transport::resolve_qconnect_friendly_name(custom_name.as_deref());
        if let Ok(mut s) = self.shared.lock() {
            s.qconnect.device_name = effective_name;
        }

        Ok(())
    }

    /// Tear the QConnect session down. Always forces Off. Aborts AND joins the
    /// event loop so its `Arc<AppRuntime>` clone drops before the daemon's
    /// shutdown releases the audio device (§8.2 / #521 ordering).
    pub async fn disconnect(&self) -> Result<(), String> {
        let runtime = {
            let mut guard = self.inner.lock().await;
            guard.lifecycle_state = QconnectLifecycleState::Off;
            guard.runtime.take()
        };

        if let Some(runtime) = runtime {
            // Disarm any in-flight liveness watchdog + clear the session topology
            // BEFORE aborting the loop, so a late event can't resurrect a stale
            // active-renderer state.
            {
                let mut state = runtime.sync_state.lock().await;
                state.watchdog_generation = state.watchdog_generation.wrapping_add(1);
                state.session = QconnectSessionState::default();
                state.session_renderer_states.clear();
            }
            if let Err(err) = runtime.app.disconnect().await {
                let mut guard = self.inner.lock().await;
                guard.last_error = Some(format!("qconnect disconnect failed: {err}"));
            }
            runtime.event_loop.abort();
            let _ = runtime.event_loop.await;
        }

        if let Ok(mut s) = self.shared.lock() {
            s.qconnect.state = "off".to_string();
            s.qconnect.session_active = false;
        }
        Ok(())
    }

    /// Wait until the daemon is Ready (logged in + API initialized), then attempt
    /// `connect()` with the bounded [2s, 5s, 15s, 30s] retry schedule. Each
    /// `connect()` re-resolves the transport config internally, so a transient
    /// credential/network failure can clear on a later attempt.
    async fn connect_on_ready(self: Arc<Self>) {
        loop {
            let logged_in = self
                .shared
                .lock()
                .map(|s| s.auth == AuthState::LoggedIn)
                .unwrap_or(false);
            if logged_in && self.runtime.core().is_api_initialized().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let schedule: [u64; 4] = [2_000, 5_000, 15_000, 30_000];
        for attempt in 0..=schedule.len() {
            match self.connect().await {
                Ok(()) => {
                    log::info!("[QConnect] auto-connect succeeded");
                    return;
                }
                Err(err) => {
                    log::warn!("[QConnect] auto-connect attempt {} failed: {err}", attempt + 1);
                }
            }
            match schedule.get(attempt) {
                Some(delay_ms) => tokio::time::sleep(Duration::from_millis(*delay_ms)).await,
                None => {
                    log::warn!(
                        "[QConnect] auto-connect gave up for this session after {} attempts",
                        attempt + 1
                    );
                    return;
                }
            }
        }
    }
}

/// Owner handle held by the daemon boot for the process lifetime. Drives runtime
/// enable/disable (T11) and the ordered shutdown (§8.2-1).
pub struct QconnectHandle {
    service: Arc<DaemonQconnectService>,
    watcher: Option<JoinHandle<()>>,
}

impl QconnectHandle {
    /// Connect on demand (T11 `qbzd qconnect enable`).
    #[allow(dead_code)]
    pub async fn connect(&self) -> Result<(), String> {
        self.service.connect().await
    }

    /// Disconnect on demand (T11 `qbzd qconnect disable`).
    #[allow(dead_code)]
    pub async fn disconnect(&self) -> Result<(), String> {
        self.service.disconnect().await
    }

    /// §8.2-1: stop the auto-connect watcher and disconnect the session BEFORE
    /// the daemon stops playback. Aborts + joins the watcher and the event loop so
    /// every `Arc<AppRuntime>` clone this handle owns drops ahead of
    /// `drop(booted)` (the #521 clock-release ordering).
    pub async fn shutdown(&mut self) {
        if let Some(watcher) = self.watcher.take() {
            watcher.abort();
            let _ = watcher.await;
        }
        let _ = self.service.disconnect().await;
    }
}

/// Boot step 12: wire QConnect. Mints the daemon's OWN device identity in the
/// daemon-root KV, decides auto-connect from the persisted startup mode
/// (`cli_override = None`, `last_known = None` — P0), latches the initial status,
/// and, when enabled, spawns the connect-on-Ready retry task.
pub fn start(runtime: Runtime, shared: SharedState, roots: &ProfileRoots) -> QconnectHandle {
    let settings_db = roots.data.join("qconnect_settings.db");
    // Re-point device identity + KV at the daemon root (NEVER the desktop global).
    transport::init_settings_db_path(settings_db.clone());

    // Effective startup decision (Ready-state only). `cli_override` stays None: a
    // `Some` would permanently shadow the KV store that `qbzd qconnect
    // enable|disable` (T11) + the TUI (T13) write, making both dead controls.
    // `last_known` is None in P0 (RememberLast resolves to off).
    let mode = transport::load_startup_mode_at(&settings_db);
    let should_auto_connect = compute_effective_startup(mode, None, None);
    let custom_name = transport::load_device_name_at(&settings_db);
    let effective_name = transport::resolve_qconnect_friendly_name(custom_name.as_deref());

    // Latch the initial status so `/api/status` reflects the config before Ready.
    if let Ok(mut s) = shared.lock() {
        s.qconnect.enabled = should_auto_connect;
        s.qconnect.device_name = effective_name;
        s.qconnect.state = "off".to_string();
        s.qconnect.session_active = false;
    }

    let service = Arc::new(DaemonQconnectService {
        inner: Arc::new(Mutex::new(DaemonQconnectInner::default())),
        runtime,
        shared,
        settings_db,
        custom_device_name: Arc::new(tokio::sync::RwLock::new(custom_name)),
    });

    let watcher = if should_auto_connect {
        log::info!(
            "[QConnect] auto-connect enabled (startup mode = {}); waiting for Ready",
            mode.as_str()
        );
        let svc = Arc::clone(&service);
        Some(tokio::spawn(async move { svc.connect_on_ready().await }))
    } else {
        log::info!(
            "[QConnect] auto-connect disabled (startup mode = {})",
            mode.as_str()
        );
        None
    };

    QconnectHandle { service, watcher }
}
