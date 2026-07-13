//! Linux sleep/idle inhibitor via `org.freedesktop.login1` (#522).
//!
//! While playback is Playing, QBZ holds a logind inhibitor lock
//! (`Inhibit("sleep:idle", ..., "block")`) so the system does not suspend or
//! go idle mid-track. The lock is fd-based: logind releases it the moment the
//! returned file descriptor is closed, so pause/stop (or process exit, even a
//! crash) releases it automatically — no explicit "uninhibit" call exists or
//! is needed.
//!
//! Uses the zbus re-exported by `mpris-server` (same graph-wide zbus the rest
//! of this crate rides) with a raw `call_method` — no proxy macro, matching
//! the crate's dependency-light style. login1 lives on the **system** bus,
//! unlike MPRIS (session bus), so this keeps its own lazy connection.

use mpris_server::zbus::{self, zvariant::OwnedFd};

const LOGIN1_DEST: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const LOGIN1_IFACE: &str = "org.freedesktop.login1.Manager";

/// Holds the inhibitor fd while playing. Owned by the MPRIS update loop
/// (single-threaded); acquire/release are driven by playback-state updates.
pub struct SleepInhibitor {
    /// Lazy system-bus connection; `None` until first acquire (or if the
    /// system bus is unreachable — then we retry on the next transition).
    conn: Option<zbus::Connection>,
    /// The inhibitor lock. `Some` while Playing; dropping it closes the fd,
    /// which is how logind releases the lock.
    fd: Option<OwnedFd>,
}

impl SleepInhibitor {
    pub fn new() -> Self {
        Self { conn: None, fd: None }
    }

    /// Drive the inhibitor from a playback transition: acquire on Playing,
    /// release on Paused/Stopped. Idempotent — repeated Playing updates keep
    /// the existing lock. Failures are logged and non-fatal (playback must
    /// never depend on logind being present, e.g. non-systemd distros).
    pub async fn set_playing(&mut self, playing: bool) {
        if playing {
            self.acquire().await;
        } else {
            self.release();
        }
    }

    async fn acquire(&mut self) {
        if self.fd.is_some() {
            return;
        }
        let conn = match &self.conn {
            Some(c) => c.clone(),
            None => match zbus::Connection::system().await {
                Ok(c) => {
                    self.conn = Some(c.clone());
                    c
                }
                Err(e) => {
                    log::warn!("[inhibit] system bus unavailable, cannot inhibit sleep: {e}");
                    return;
                }
            },
        };
        let reply = conn
            .call_method(
                Some(LOGIN1_DEST),
                LOGIN1_PATH,
                Some(LOGIN1_IFACE),
                "Inhibit",
                &("sleep:idle", "QBZ", "Music playback in progress", "block"),
            )
            .await;
        match reply {
            Ok(msg) => match msg.body().deserialize::<OwnedFd>() {
                Ok(fd) => {
                    log::info!("[inhibit] acquired login1 sleep:idle inhibitor");
                    self.fd = Some(fd);
                }
                Err(e) => log::warn!("[inhibit] bad Inhibit reply (expected fd): {e}"),
            },
            Err(e) => log::warn!("[inhibit] login1 Inhibit call failed: {e}"),
        }
    }

    fn release(&mut self) {
        if self.fd.take().is_some() {
            // Dropping the OwnedFd closes it; logind releases the lock.
            log::info!("[inhibit] released login1 sleep:idle inhibitor");
        }
    }
}
