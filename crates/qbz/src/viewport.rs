//! Shared scroll-viewport plumbing for windowed surfaces (LL albums grid
//! today; the Phase-2 rollout surfaces reuse it). This side owns the band
//! dispatcher that turns raw `window-changed` reports from a .slint grid
//! into throttled artwork dispatches. Element windowing (card vs
//! placeholder) and the contract hysteresis on the band live .slint-side
//! (`AlbumGrid` in AlbumCollectionView.slint). Generation guarding is the
//! caller's job: capture the surface's gen when scheduling and check it
//! inside the action.

use slint::{Timer, TimerMode};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Throttle with leading + trailing edge for viewport band reports.
/// UI-thread only (holds a `slint::Timer`) — wrap instances in
/// `thread_local!`. The first report after a quiet period runs immediately,
/// so a slow scroll gets artwork with no perceptible delay; while reports
/// keep arriving (a fling crosses a row boundary every ~270px) they coalesce
/// into at most one run per `interval`, always ending with a trailing run
/// for the final band.
pub struct BandDispatcher {
    timer: Timer,
    interval: Duration,
    last_run: Rc<Cell<Option<Instant>>>,
    pending: Rc<RefCell<Option<Box<dyn FnOnce()>>>>,
}

impl BandDispatcher {
    pub fn new(interval_ms: u64) -> Self {
        Self {
            timer: Timer::default(),
            interval: Duration::from_millis(interval_ms),
            last_run: Rc::new(Cell::new(None)),
            pending: Rc::new(RefCell::new(None)),
        }
    }

    /// Report a band change. `action` either runs now (leading edge) or
    /// replaces the pending trailing run — only the newest report survives
    /// coalescing, which is sound because every action re-reads the live
    /// band state when it runs.
    pub fn report(&self, action: Box<dyn FnOnce()>) {
        let now = Instant::now();
        let quiet = self
            .last_run
            .get()
            .map_or(true, |t| now.duration_since(t) >= self.interval);
        if quiet && !self.timer.running() {
            self.last_run.set(Some(now));
            action();
            return;
        }
        *self.pending.borrow_mut() = Some(action);
        if !self.timer.running() {
            let elapsed = self
                .last_run
                .get()
                .map_or(Duration::ZERO, |t| now.duration_since(t));
            let delay = self
                .interval
                .saturating_sub(elapsed)
                .max(Duration::from_millis(1));
            let pending = self.pending.clone();
            let last_run = self.last_run.clone();
            self.timer.start(TimerMode::SingleShot, delay, move || {
                last_run.set(Some(Instant::now()));
                // Release the RefCell borrow BEFORE running the action — it
                // may synchronously re-enter `report()` via model updates.
                let action = pending.borrow_mut().take();
                if let Some(f) = action {
                    f();
                }
            });
        }
    }
}
