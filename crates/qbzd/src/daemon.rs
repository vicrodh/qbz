// crates/qbzd/src/daemon.rs — the `qbzd run` boot sequence (01-architecture.md
// §8.1, NORMATIVE order), the NeedsAuth-stays-up state machine (§6.2) and the
// graceful shutdown (§8.2). Later tasks splice into the numbered steps: the
// playback driver (T4) at step 10, the HTTP server (T6) at step 11, QConnect
// (T9/T10) at step 12. Until they land the daemon boots a playable core and
// parks on signals — API-less but fully diagnosable in-process.
use std::sync::{Arc, Mutex};

use qbz_app::settings::daemon_prefs;
use qbz_app::shell::AppRuntime;
use qbz_app::playback_driver::{self, DriverDeps};
use qbz_core::CoreError;
use qbz_models::{CoreEvent, UserSession};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::adapter::DaemonAdapter;
use crate::config::QbzdConfig;
use crate::lock::{InstanceLock, LockError};
use crate::paths::ProfileRoots;
use crate::state::{AuthState, DaemonShared, LatchedErrors, QconnectStatus};

/// The composed runtime handoff produced by [`boot`] and consumed by later
/// tasks: T4 spawns the playback driver on `runtime` + `shared`, T6 serves
/// `bus` over HTTP/SSE, T9/T10 wire QConnect. Held alive by [`run`] through the
/// signal park so the core stays up.
#[allow(dead_code)] // fields are the seam later tasks (T6/T9/T10) read.
pub struct BootedRuntime {
    pub runtime: Arc<AppRuntime<DaemonAdapter>>,
    pub shared: Arc<Mutex<DaemonShared>>,
    pub bus: broadcast::Sender<CoreEvent>,
    /// Background session-restore retry (network-class boot failure only). Held
    /// so shutdown can abort+join it BEFORE releasing the audio device: it holds
    /// an `Arc<AppRuntime>` clone, so leaving it running would keep the Player
    /// alive past `drop(booted)` and break the #521 clock-release ordering (§8.2).
    pub auth_retry: Option<JoinHandle<()>>,
}

/// `qbzd run` — boot the daemon in the foreground, park on signals, shut down
/// gracefully. Returns the process exit code (0 = clean shutdown). `warns` are
/// the unknown-key warnings surfaced by [`QbzdConfig::load`] in `main`.
pub async fn run(roots: ProfileRoots, cfg: QbzdConfig, warns: Vec<String>) -> Result<i32, String> {
    // 1. argv parse happened in main(). 2. logging:
    qbz_log::install(&cfg.log.level);
    // 3. config: surface unknown-key warnings (they never abort — D14).
    for w in &warns {
        log::warn!("[config] unknown key: {w}");
    }
    // 4. instance lock on the DATA ROOT, taken BEFORE any port bind (§8.3): it,
    //    not the port, protects the single-device_uuid / single-session.db
    //    invariants. A second daemon on the same root is diagnosed → exit 3.
    let _lock = InstanceLock::acquire(&roots.data).map_err(diagnose_lock)?;
    // 5. port bind + foreign-occupant diagnosis — STATELESS, so it runs BEFORE
    //    stores (6) and runtime composition (7) per the §8.1 order. On a bind
    //    conflict the occupant is probed with GET /api/ping: a qbzd answer means
    //    a stale foreign root (the lock said this root was free), anything else
    //    the §2.2 "another process" copy. The socket is bound here but not served
    //    until step 11 — connections queue in the listen backlog through boot.
    let bind_addr = resolve_bind_addr(&cfg)?;
    let bound = match crate::api::bind(bind_addr) {
        Ok(b) => b,
        Err(crate::api::BindError::AddrInUse(addr)) => return Err(diagnose_port_conflict(addr)),
        Err(crate::api::BindError::Other(msg)) => {
            return Err(format!(
                "error: could not bind the control API on {bind_addr}: {msg}\n  → check [server] bind/port in ~/.config/qbzd/qbzd.toml"
            ));
        }
    };
    if !bind_addr.ip().is_loopback() {
        // §6.3 verbatim — reachable-from-LAN warning (also shown by the TUI).
        eprintln!("{}", crate::cli::copy::lan_bind_warning(&bind_addr.to_string()));
        log::warn!("control API bound to non-loopback {bind_addr}");
    }

    // 6.-9. compose stores + runtime + restore credentials + restore session.
    let mut booted = boot(&roots, &cfg, warns.len()).await?;

    // 10. playback driver (T4). Spawn the 450 ms headless orchestrator on the
    //     booted runtime + shared state. It runs safely regardless of auth: with
    //     no session the queue is empty and each tick is a near-no-op. The
    //     streaming quality is resolved from daemon_prefs through the SAME key
    //     contract the desktop uses (playback_quality(), playback.rs:170-172),
    //     so hi-res never silently downgrades. 11. HTTP serve (T6) · 12. QConnect
    //     (T9/T10) splice after this, reading `booted`.
    let prefs = daemon_prefs::load_at(&roots.data);
    let quality = playback_driver::quality_from_key(&prefs.streaming_quality);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let deps = build_driver_deps(quality, booted.shared.clone());
    let driver = tokio::spawn(playback_driver::run_driver(
        booted.runtime.clone(),
        deps,
        shutdown_rx,
    ));

    // 11. HTTP serve (02 §3) on the already-bound socket. `ApiState` carries a
    //     second read-only audio-store connection (WAL) for the status audio
    //     block, the tokio handle for the async queue read, and the opt-in
    //     [server] token (None = open). 12. QConnect (T9/T10) splices after this.
    let api_audio = qbz_audio::settings::AudioSettingsStore::new_at(&roots.data)
        .map_err(|e| format!("error: could not open the audio settings store for the API: {e}"))?;
    let api = crate::api::serve(
        bound,
        crate::api::ApiState {
            runtime: booted.runtime.clone(),
            shared: booted.shared.clone(),
            roots: roots.clone(),
            token: cfg.server.token.filter(|t| !t.trim().is_empty()),
            bind: bind_addr.to_string(),
            rt: tokio::runtime::Handle::current(),
            audio: api_audio,
            devices: std::sync::Mutex::new(crate::api::DeviceCache::default()),
        },
    );
    log::info!("control API listening on {bind_addr}");

    // 12. QConnect (T9): mint the daemon's OWN device identity in the daemon-root
    //     KV, decide auto-connect from the persisted startup mode (cli_override =
    //     None so the KV that `qbzd qconnect enable|disable` writes is never
    //     shadowed), and — when enabled — connect-on-Ready with the bounded retry
    //     schedule. Reads NOTHING from qbzd.toml. Held to shut the session down
    //     ahead of playback (§8.2-1); it also clones `Arc<AppRuntime>`, so it must
    //     drop before `drop(booted)` (the #521 ordering).
    let mut qconnect = crate::qconnect::start(booted.runtime.clone(), booted.shared.clone(), &roots);

    // 13. park on SIGTERM/SIGINT. NO startup audio "hygiene": both candidate
    //     fns are verified no-ops from a fresh process and re-adding them is the
    //     documented skeptic-correction #1 trap (§8.1).
    wait_for_signal().await;

    // ── Shutdown (§8.2, ordered). Step 1: disconnect the QConnect session (and
    //    stop its auto-connect watcher) BEFORE playback is stopped, then drop the
    //    handle so its Arc<AppRuntime> clone is released ahead of `drop(booted)`.
    qconnect.shutdown().await;
    drop(qconnect);
    // Step 2: stop the playback driver. It holds an Arc<AppRuntime> clone, so its
    //    task must finish (dropping that Arc) before `drop(booted)` can release
    //    the audio device ahead of the #521 pair. Signal, then join.
    let _ = shutdown_tx.send(true);
    if let Err(e) = driver.await {
        log::warn!("driver task join failed: {e:?}");
    }
    // Final full session save (queue + position) now that playback is quiesced.
    playback_driver::save_session_now(booted.runtime.as_ref()).await;
    // The background auth-retry task also holds an Arc<AppRuntime> clone — abort
    // AND join it so its Arc is dropped before `drop(booted)`; otherwise the
    // ordering claim below (drop releases the device) breaks once playback has
    // engaged a real device.
    if let Some(retry) = booted.auth_retry.take() {
        retry.abort();
        let _ = retry.await;
    }
    // Stop the API thread and JOIN it: its `ApiState` holds an `Arc<AppRuntime>`
    // clone, which must drop before `drop(booted)` releases the audio device
    // ahead of the #521 pair — the same ordering constraint as the driver and
    // auth-retry tasks (§8.2).
    api.shutdown();
    // Release the audio device by dropping the runtime (its Player) BEFORE the
    // #521 pair (§8.2 step 3 precedes step 4).
    drop(booted);
    //    THE #521 PAIR runs unconditionally on Linux — exactly the desktop quit
    //    choke-point (crates/qbz/src/main.rs:20393): a forced PipeWire clock left
    //    set would pin the whole system's sample rate after the process dies.
    //    Both calls self-gate to no-ops when QBZ forced nothing.
    #[cfg(target_os = "linux")]
    {
        qbz_audio::alsa_backend::resume_suspended_sink();
        qbz_audio::pipewire_backend::PipeWireBackend::reset_pipewire_clock();
    }

    Ok(0) // instance lock released on drop of `_lock`
}

/// Steps 6-9 of §8.1: open the daemon-root stores, compose the runtime with the
/// two NORMATIVE substitutions (`with_audio_settings` + `activate_at`), and
/// restore the saved session per the §6.2 clearing taxonomy.
async fn boot(roots: &ProfileRoots, cfg: &QbzdConfig, warn_count: usize) -> Result<BootedRuntime, String> {
    // 6.+7. stores + runtime composition. The two substitutions (01 §2.2):
    //   - with_audio_settings, NOT AppRuntime::new (which hardcodes the
    //     desktop-global AudioSettingsStore — shell.rs:87-101);
    //   - activate_at (below), NOT activate (which resolves desktop
    //     UserDataPaths — shell.rs:195-203).
    // Everything routes through the T2 daemon roots.
    let store = qbz_audio::settings::AudioSettingsStore::new_at(&roots.data)?; // settings.rs:263
    let settings = store.get_settings()?;
    let (adapter, _rx) = DaemonAdapter::new();
    let bus = adapter.sender();
    let runtime = Arc::new(AppRuntime::with_audio_settings(
        adapter,
        settings.output_device.clone(),
        settings,
        None,
    )); // shell.rs:64

    // Offline-tolerant (§8.1-8): a network failure here still leaves a locally
    // usable core; a missing DAC is likewise non-fatal (Player starts deviceless
    // and retries with backoff — never the spotifyd #1097 crash-exit).
    if let Err(e) = runtime.init().await {
        log::warn!("core init did not complete (continuing offline-tolerant): {e}");
    }

    let shared = new_shared(cfg);
    if let Ok(mut s) = shared.lock() {
        s.startup_warnings = warn_count as u32;
    }

    // 8. credential restore per the §6.2 taxonomy (mirrors qbz/src/auth.rs:
    //    215-230): clear the token ONLY on explicit auth rejection; KEEP it on
    //    every network-class failure (clearing on transient errors is the
    //    documented boot-token-loss bug class).
    let auth_retry = match qbz_credentials::load_oauth_token_at(&roots.config)? {
        None => {
            set_needs_auth(&shared, None);
            None
        }
        Some(token) => {
            // Register before the token can reach any log line (§6.3).
            qbz_log::register_secret(token.clone());
            match runtime.core().login_with_token(&token).await {
                Ok(session) => {
                    restore_activate(&runtime, &shared, roots, session).await?;
                    // 9½. session restore (queue/position) PAUSED: the daemon's
                    //     session store IS its queue persistence, so a restart
                    //     comes back with the queue armed but not auto-playing.
                    playback_driver::restore_session_paused(runtime.as_ref()).await;
                    None
                }
                Err(e) if is_auth_rejection(&e) => {
                    qbz_credentials::clear_oauth_token_at(&roots.config)?;
                    latch_auth_error(&shared, &e);
                    set_needs_auth(&shared, Some(e));
                    None
                }
                Err(e) => {
                    // network-class: KEEP token, stay Restoring, retry w/ backoff.
                    log::warn!("session restore deferred (network-class): {e}");
                    Some(spawn_auth_retry(
                        runtime.clone(),
                        shared.clone(),
                        roots.clone(),
                    ))
                }
            }
        }
    };

    Ok(BootedRuntime {
        runtime,
        shared,
        bus,
        auth_retry,
    })
}

/// Assemble the driver's host side channels: the streaming-quality resolver and
/// the daemon-shared latching / tick-timestamping hooks. `on_edge` is a no-op
/// until T10 wires the outbound QConnect renderer report.
fn build_driver_deps(quality: qbz_models::Quality, shared: Arc<Mutex<DaemonShared>>) -> DriverDeps {
    let latch_shared = shared.clone();
    let tick_shared = shared;
    DriverDeps {
        quality: Arc::new(move || quality),
        on_edge: Arc::new(|| {}),
        on_latch: Arc::new(move |category, message| {
            if let Ok(mut s) = latch_shared.lock() {
                match category {
                    "stream" => s.last_errors.stream = Some(message),
                    "transport" => s.last_errors.transport = Some(message),
                    "auth" => s.last_errors.auth = Some(message),
                    _ => {}
                }
            }
        }),
        on_tick: Arc::new(move || {
            if let Ok(mut s) = tick_shared.lock() {
                s.driver_last_tick = Some(std::time::Instant::now());
            }
        }),
    }
}

/// Activate the per-user session against DAEMON paths (§8.1-9): inject the
/// session into the core, then `activate_at` the runtime with per-user daemon
/// data/cache directories — never the desktop `UserDataPaths`.
async fn restore_activate(
    runtime: &Arc<AppRuntime<DaemonAdapter>>,
    shared: &Arc<Mutex<DaemonShared>>,
    roots: &ProfileRoots,
    session: UserSession,
) -> Result<(), String> {
    runtime
        .core()
        .set_session(session.clone())
        .await
        .map_err(|e| e.to_string())?;
    runtime
        .activate_at(
            session.user_id,
            &roots.data.join(format!("users/{}", session.user_id)),
            &roots.cache.join(format!("users/{}", session.user_id)),
        )
        .await?;
    set_logged_in(shared, &session);
    Ok(())
}

/// Fresh shared state. Starts in `Restoring` — credential restore drives the
/// terminal transition to `LoggedIn` or `NeedsAuth` (§6.2 diagram).
fn new_shared(cfg: &QbzdConfig) -> Arc<Mutex<DaemonShared>> {
    let _ = cfg; // reserved: premute/mpris defaults wire in with later tasks.
    Arc::new(Mutex::new(DaemonShared {
        auth: AuthState::Restoring,
        user_id: None,
        subscription: None,
        last_errors: LatchedErrors::default(),
        driver_last_tick: None,
        muted: false,
        premute_volume: 1.0,
        started_at: std::time::Instant::now(),
        startup_warnings: 0,
        qconnect: QconnectStatus::default(),
    }))
}

/// Enter NeedsAuth. `err = None` = no saved credentials at all (the common
/// first-run case); `Some(e)` = an explicit auth rejection just cleared the
/// token. Either way the daemon STAYS UP (§6.2) and names the fix.
fn set_needs_auth(shared: &Arc<Mutex<DaemonShared>>, err: Option<CoreError>) {
    if let Ok(mut s) = shared.lock() {
        s.auth = AuthState::NeedsAuth;
        s.user_id = None;
        s.subscription = None;
    }
    match err {
        None => log::info!("Not logged in — run 'qbzd setup' (or 'qbzd login')"),
        Some(e) => {
            log::warn!("Qobuz rejected the saved session ({e}) — run 'qbzd login' to re-authenticate")
        }
    }
}

/// Enter LoggedIn (Ready). Records the user id + subscription label for
/// `/api/status` (T6). The auth token itself is never stored here — it is a
/// registered secret and lives only in the credential file.
fn set_logged_in(shared: &Arc<Mutex<DaemonShared>>, session: &UserSession) {
    if let Ok(mut s) = shared.lock() {
        s.auth = AuthState::LoggedIn;
        s.user_id = Some(session.user_id);
        s.subscription = Some(session.subscription_label.clone());
    }
    log::info!(
        "Logged in (user {}, subscription '{}')",
        session.user_id,
        session.subscription_label
    );
}

/// Latch an auth error so a `status` call remains diagnosable after the fact
/// (§9.4 — drain-once channels alone cannot answer "why did the music stop?").
fn latch_auth_error(shared: &Arc<Mutex<DaemonShared>>, e: &CoreError) {
    if let Ok(mut s) = shared.lock() {
        s.last_errors.auth = Some(format!("token rejected by Qobuz — cleared ({e})"));
    }
}

/// True ONLY for an explicit auth rejection from Qobuz — a 401 on the token
/// login (`AuthenticationError`) or an ineligible-account verdict. Network
/// failures, offline gate, 5xx, rate limiting and parse errors all return false
/// so the saved token is KEPT (mirrors crates/qbz/src/auth.rs:215-230; the
/// taxonomy — not the variant list — is the normative part).
fn is_auth_rejection(error: &CoreError) -> bool {
    matches!(
        error,
        CoreError::Api(
            qbz_qobuz::ApiError::AuthenticationError(_) | qbz_qobuz::ApiError::IneligibleUser
        )
    )
}

/// Background retry for a network-class restore failure (§6.2: stay in the
/// authenticating state, KEEP the token, retry with backoff). On success the
/// session activates; on a now-explicit auth rejection the token is cleared and
/// the daemon drops to NeedsAuth; if the whole schedule sees only network-class
/// failures the token is KEPT and the daemon surfaces NeedsAuth so it stays
/// diagnosable and a later `qbzd login` / settings reload can retry.
fn spawn_auth_retry(
    runtime: Arc<AppRuntime<DaemonAdapter>>,
    shared: Arc<Mutex<DaemonShared>>,
    roots: ProfileRoots,
) -> JoinHandle<()> {
    const SCHEDULE_SECS: [u64; 4] = [2, 5, 15, 30];
    tokio::spawn(async move {
        let token = match qbz_credentials::load_oauth_token_at(&roots.config) {
            Ok(Some(t)) => t,
            _ => return, // token vanished (concurrent logout) — nothing to retry.
        };
        for (i, delay) in SCHEDULE_SECS.iter().enumerate() {
            tokio::time::sleep(std::time::Duration::from_secs(*delay)).await;
            log::info!("session restore retry {}/{}", i + 1, SCHEDULE_SECS.len());
            match runtime.core().login_with_token(&token).await {
                Ok(session) => {
                    if let Err(e) = restore_activate(&runtime, &shared, &roots, session).await {
                        log::warn!("session activation after retry failed: {e}");
                    }
                    return;
                }
                Err(e) if is_auth_rejection(&e) => {
                    let _ = qbz_credentials::clear_oauth_token_at(&roots.config);
                    latch_auth_error(&shared, &e);
                    set_needs_auth(&shared, Some(e));
                    return;
                }
                Err(e) => log::warn!("session restore retry {} failed (network-class): {e}", i + 1),
            }
        }
        // Schedule exhausted with only network-class failures: KEEP the token,
        // surface NeedsAuth, latch the reason for `qbzd status`.
        if let Ok(mut s) = shared.lock() {
            s.auth = AuthState::NeedsAuth;
            s.last_errors.auth = Some(
                "could not reach Qobuz to restore the saved session — token kept, retry with 'qbzd login' or 'qbzd settings reload'".into(),
            );
        }
        log::warn!(
            "session restore gave up after {} network-class attempts — token KEPT",
            SCHEDULE_SECS.len()
        );
    })
}

/// Resolve `[server] bind:port` to a `SocketAddr` (01 §10.1). A malformed value
/// is a fatal boot error that names the fix (exit 1).
fn resolve_bind_addr(cfg: &QbzdConfig) -> Result<std::net::SocketAddr, String> {
    use std::net::ToSocketAddrs;
    let hostport = format!("{}:{}", cfg.server.bind, cfg.server.port);
    hostport
        .to_socket_addrs()
        .map_err(|e| {
            format!("error: invalid [server] bind/port '{hostport}': {e}\n  → set a valid ip and port in ~/.config/qbzd/qbzd.toml")
        })?
        .next()
        .ok_or_else(|| {
            format!("error: [server] '{hostport}' resolved to no address\n  → set a valid ip and port in ~/.config/qbzd/qbzd.toml")
        })
}

/// The step-5 bind-conflict diagnosis (02 §8.1-5 / §2.2): a qbzd occupant on a
/// different data root vs. an unrelated process on the port.
fn diagnose_port_conflict(addr: std::net::SocketAddr) -> String {
    if crate::api::probe_is_qbzd(addr) {
        crate::cli::copy::foreign_qbzd(&addr.to_string())
    } else {
        crate::cli::copy::port_in_use(addr.port())
    }
}

/// Render an [`InstanceLock`] failure. For the already-running case this prints
/// the frozen exit-3 error voice (02 §1.3/§1.4) and exits 3 directly — the new
/// process must never clobber the running one. An I/O failure returns a String
/// that propagates to a generic exit 1.
fn diagnose_lock(e: LockError) -> String {
    match e {
        LockError::AlreadyRunning(pid) => {
            let who = pid
                .map(|p| format!("(pid {p})"))
                .unwrap_or_else(|| "(pid unknown)".to_string());
            eprintln!("error: qbzd is already running {who}");
            eprintln!("  → stop it first:  systemctl --user stop qbzd");
            eprintln!("  → or inspect it:  systemctl --user status qbzd");
            std::process::exit(3);
        }
        LockError::Io(msg) => {
            format!("error: could not take the instance lock: {msg}\n  → check permissions on the data root")
        }
    }
}

/// Park until SIGTERM or SIGINT. A second signal after this returns lets the
/// default handler take over → immediate exit (§8.2).
async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) {
            (Ok(mut term), Ok(mut int)) => {
                tokio::select! {
                    _ = term.recv() => log::info!("SIGTERM received — shutting down"),
                    _ = int.recv()  => log::info!("SIGINT received — shutting down"),
                }
            }
            _ => {
                // Fall back to Ctrl-C if the SIGTERM handler could not install.
                let _ = tokio::signal::ctrl_c().await;
                log::info!("Ctrl-C received — shutting down");
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        log::info!("Ctrl-C received — shutting down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_auth_rejection_matches_only_explicit_rejections() {
        // Explicit rejections → clear the token.
        assert!(is_auth_rejection(&CoreError::Api(
            qbz_qobuz::ApiError::AuthenticationError("401".into())
        )));
        assert!(is_auth_rejection(&CoreError::Api(
            qbz_qobuz::ApiError::IneligibleUser
        )));
        // Network-class / other → KEEP the token (the boot-token-loss guard).
        assert!(!is_auth_rejection(&CoreError::Api(
            qbz_qobuz::ApiError::ServerError(503)
        )));
        assert!(!is_auth_rejection(&CoreError::Api(
            qbz_qobuz::ApiError::RateLimited(30)
        )));
        assert!(!is_auth_rejection(&CoreError::NotInitialized));
    }

    #[test]
    fn no_credentials_enters_needs_auth() {
        let shared = new_shared(&QbzdConfig::default());
        set_needs_auth(&shared, None);
        let s = shared.lock().unwrap();
        assert_eq!(s.auth, AuthState::NeedsAuth);
        assert!(s.user_id.is_none());
        assert!(s.last_errors.auth.is_none());
    }

    #[test]
    fn explicit_rejection_latches_and_needs_auth() {
        let shared = new_shared(&QbzdConfig::default());
        let err = CoreError::Api(qbz_qobuz::ApiError::AuthenticationError("401".into()));
        latch_auth_error(&shared, &err);
        set_needs_auth(&shared, Some(err));
        let s = shared.lock().unwrap();
        assert_eq!(s.auth, AuthState::NeedsAuth);
        assert!(s.last_errors.auth.is_some());
    }

    #[test]
    fn logged_in_records_user_and_subscription() {
        let shared = new_shared(&QbzdConfig::default());
        let session = UserSession {
            user_auth_token: "secret".into(),
            user_id: 1234567,
            email: "a@b.c".into(),
            display_name: "Tester".into(),
            subscription_label: "studio".into(),
            subscription_valid_until: None,
        };
        set_logged_in(&shared, &session);
        let s = shared.lock().unwrap();
        assert_eq!(s.auth, AuthState::LoggedIn);
        assert_eq!(s.user_id, Some(1234567));
        assert_eq!(s.subscription.as_deref(), Some("studio"));
    }
}
