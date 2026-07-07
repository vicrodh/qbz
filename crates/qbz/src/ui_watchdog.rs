//! UI event-loop responsiveness watchdog (#555 diagnosis aid).
//!
//! The #555 class of lag is RENDERER-INDEPENDENT (software == GL == wgpu in
//! both field reports), which means the bottleneck sits above the renderer:
//! the event loop itself stops turning promptly. This watchdog measures
//! exactly that — the round-trip latency of a cross-thread
//! `invoke_from_event_loop` closure — every couple of seconds, from a plain
//! background thread. It never switches anything: it records, logs ONCE when
//! degradation is sustained, and feeds the Developer > Diagnostics panel so
//! field reports carry a number instead of "feels like 3fps".
//!
//! Design guards against false positives: sampling starts after a startup
//! grace (cache rebuilds, first paints), a flag needs many CONSECUTIVE bad
//! samples (a suspend/resume or one heavy IO burst resets the streak), and
//! the timeout cap keeps a wedged loop from blocking the watchdog thread.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Latency of the most recent probe, in milliseconds.
static LAST_MS: AtomicU64 = AtomicU64::new(0);
/// Worst latency seen this session, in milliseconds.
static WORST_MS: AtomicU64 = AtomicU64::new(0);
/// Set once when SUSTAINED_BAD consecutive probes exceeded BAD_MS.
static FLAGGED: AtomicBool = AtomicBool::new(false);

/// Don't sample during the startup storm.
const STARTUP_GRACE: Duration = Duration::from_secs(30);
/// Probe cadence.
const INTERVAL: Duration = Duration::from_secs(2);
/// A probe slower than this counts against the streak. ~250ms dispatch
/// latency means the loop turns at <4Hz — far past "janky", into "broken".
const BAD_MS: u64 = 250;
/// Consecutive bad probes (~20s of sustained degradation) before flagging.
const SUSTAINED_BAD: u32 = 10;
/// Cap so a fully wedged loop can't block the watchdog thread forever.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Spawn the watchdog thread. Call once, after the Slint event loop exists.
pub fn spawn() {
    let _ = std::thread::Builder::new()
        .name("qbz-ui-watchdog".into())
        .spawn(|| {
            std::thread::sleep(STARTUP_GRACE);
            let mut streak = 0u32;
            loop {
                std::thread::sleep(INTERVAL);
                let (tx, rx) = std::sync::mpsc::channel::<()>();
                let sent = Instant::now();
                if slint::invoke_from_event_loop(move || {
                    let _ = tx.send(());
                })
                .is_err()
                {
                    // Event loop is gone — the app is shutting down.
                    return;
                }
                // Clamped to the probe cap: `Instant` counts suspend time on
                // modern Linux (CLOCK_BOOTTIME), so a sleep mid-probe would
                // otherwise record a multi-hour "worst" forever.
                let ms = match rx.recv_timeout(PROBE_TIMEOUT) {
                    Ok(()) => (sent.elapsed().as_millis() as u64)
                        .min(PROBE_TIMEOUT.as_millis() as u64),
                    Err(_) => PROBE_TIMEOUT.as_millis() as u64,
                };
                LAST_MS.store(ms, Ordering::Relaxed);
                WORST_MS.fetch_max(ms, Ordering::Relaxed);
                if ms > BAD_MS {
                    streak += 1;
                } else {
                    streak = 0;
                }
                if streak == SUSTAINED_BAD && !FLAGGED.swap(true, Ordering::Relaxed) {
                    log::warn!(
                        "[ui-watchdog] UI event loop sustained >{BAD_MS}ms dispatch latency \
                         for ~{}s (last probe {ms}ms) — the interface is running degraded. \
                         This is ABOVE the renderer (all tiers affected); see issue #555.",
                        (SUSTAINED_BAD as u64 * INTERVAL.as_secs())
                    );
                }
            }
        });
}

/// Latest probe latency in ms (0 = no probe yet).
pub fn last_latency_ms() -> u64 {
    LAST_MS.load(Ordering::Relaxed)
}

/// Worst probe latency this session, in ms.
pub fn worst_latency_ms() -> u64 {
    WORST_MS.load(Ordering::Relaxed)
}

/// True once sustained degradation was flagged this session.
pub fn flagged() -> bool {
    FLAGGED.load(Ordering::Relaxed)
}
