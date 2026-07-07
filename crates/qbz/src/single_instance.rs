//! Single-instance guard (issues #544/#559 — Tauri parity: the old app
//! shipped tauri-plugin-single-instance; the Slint rebuild lost it, so
//! every click on a pinned taskbar shortcut / launcher entry spawned
//! another full player — reported on both Hyprland and KDE).
//!
//! The first instance takes ownership of the well-known session-bus name
//! `com.blitzfc.qbz` (Flatpak auto-grants owning the app-id name — no
//! finish-args change needed). A second launch sees the name taken, asks
//! the owner to bring its window up through the ALREADY-EXPORTED MPRIS
//! `Raise` method (routes to `tray::show_window` in the running instance),
//! and exits. Any D-Bus problem — no session bus, weird sandbox — falls
//! through as "we are primary": the guard must never block startup.
//!
//! Blocking zbus API on purpose: this runs once on the main thread before
//! the UI exists, and the async-io executor self-drives the connection
//! from any context (the zbus 5 "tokio" feature is FORBIDDEN graph-wide —
//! see the rfd/ksni comments in Cargo.toml).
#![cfg(target_os = "linux")]

use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::Connection;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::names::WellKnownName;

const BUS_NAME: &str = "com.blitzfc.qbz";

/// Keeps the acquired name owned for the process lifetime (releasing it
/// would let a second launch believe it is primary).
static CONN: std::sync::OnceLock<Connection> = std::sync::OnceLock::new();

/// True = we are the primary instance (name acquired, or D-Bus unusable).
/// False = another instance owns the name; it has been asked to raise its
/// window and the caller should exit.
pub fn acquire_or_raise() -> bool {
    match probe() {
        Ok(primary) => primary,
        Err(e) => {
            log::warn!(
                "[qbz-slint] single-instance: D-Bus probe failed ({e}); continuing as primary"
            );
            true
        }
    }
}

fn probe() -> zbus::Result<bool> {
    let conn = Connection::session()?;
    let proxy = DBusProxy::new(&conn)?;
    let name: WellKnownName<'_> = BUS_NAME.try_into().map_err(zbus::Error::from)?;
    match proxy.request_name(name, RequestNameFlags::DoNotQueue.into())? {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => {
            let _ = CONN.set(conn);
            Ok(true)
        }
        // Exists (or the DO_NOT_QUEUE-unreachable InQueue): another instance
        // runs. Best-effort raise — MPRIS may not be registered yet (login
        // screen) or may be disabled; the duplicate still must not start.
        RequestNameReply::Exists | RequestNameReply::InQueue => {
            // Full MPRIS name = "org.mpris.MediaPlayer2." + BUS_SUFFIX, and
            // qbz-media-controls registers with BUS_SUFFIX = the app id
            // (linux.rs), NOT "qbz".
            let _ = conn.call_method(
                Some("org.mpris.MediaPlayer2.com.blitzfc.qbz"),
                "/org/mpris/MediaPlayer2",
                Some("org.mpris.MediaPlayer2"),
                "Raise",
                &(),
            );
            Ok(false)
        }
    }
}
