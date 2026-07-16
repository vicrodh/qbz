//! System media controls wiring (MPRIS on Linux — with the app icon via the
//! `DesktopEntry` property — SMTC/MediaRemote on macOS/Windows).
//!
//! The backend lives in the frontend-agnostic `qbz-media-controls` crate; this
//! module owns the process-global handle and bridges inbound control events
//! (media keys, the GNOME/KDE media widget, macOS Now Playing) to the player.
//! Playback metadata/state is pushed from `playback.rs` (mirroring the tray).

use std::sync::{Arc, OnceLock};

use qbz_media_controls::{MediaEvent, MediaIntegration};

use crate::adapter::SlintAdapter;
use crate::AppWindow;
use qbz_app::shell::AppRuntime;

type Runtime = Arc<AppRuntime<SlintAdapter>>;

static CONTROLS: OnceLock<Box<dyn MediaIntegration>> = OnceLock::new();

/// The live integration, if it started. Playback pushes metadata/state through
/// it (`set_metadata`/`set_playback`).
pub fn handle() -> Option<&'static dyn MediaIntegration> {
    CONTROLS.get().map(|b| b.as_ref())
}

/// Start the OS media-controls integration once. No-op if already started or
/// unavailable on this platform.
pub fn init(runtime: Runtime, weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    if CONTROLS.get().is_some() {
        return;
    }
    let on_event = move |ev: MediaEvent| {
        dispatch(ev, runtime.clone(), weak.clone(), handle.clone());
    };
    match qbz_media_controls::spawn(on_event) {
        Some(c) => {
            let _ = CONTROLS.set(c);
            log::info!("[media-controls] integration started");
        }
        None => log::info!("[media-controls] no integration on this platform"),
    }
}

fn dispatch(ev: MediaEvent, rt: Runtime, weak: slint::Weak<AppWindow>, h: tokio::runtime::Handle) {
    match ev {
        // The OS only sends Play when paused and Pause when playing (it reads
        // PlaybackStatus), so routing all three through the toggle is correct
        // and keeps the QConnect-aware path (shared with the tray).
        MediaEvent::Play | MediaEvent::Pause | MediaEvent::Toggle => {
            crate::tray::dispatch_play_pause(rt, weak, h)
        }
        MediaEvent::Next => crate::tray::dispatch_next(rt, weak, h),
        MediaEvent::Previous => crate::tray::dispatch_previous(rt, weak, h),
        MediaEvent::Stop => {
            h.spawn(async move {
                if let Err(e) = rt.core().stop() {
                    log::warn!("[media-controls] stop failed: {e}");
                }
            });
        }
        // Present, not show_window: with the miniplayer open a forced main-
        // window show reads as a duplicate instance (#559) — raise the mini.
        MediaEvent::Raise => crate::tray::present(&weak),
        MediaEvent::Quit => crate::tray::quit(),
        MediaEvent::SetVolume(v) => crate::playback::set_volume(rt, weak, h, v as f32),
        MediaEvent::SetPosition(micros) => seek_to_micros(rt, h, micros),
        MediaEvent::SeekBy(delta_micros) => seek_by_micros(rt, h, delta_micros),
    }
}

fn seek_to_micros(rt: Runtime, h: tokio::runtime::Handle, micros: i64) {
    let spawn_h = h.clone();
    h.spawn(async move {
        let dur = rt.core().get_playback_state().duration; // seconds
        if dur == 0 {
            return;
        }
        let fraction = (micros as f64 / 1_000_000.0 / dur as f64).clamp(0.0, 1.0) as f32;
        do_seek(rt, spawn_h, fraction).await;
    });
}

fn seek_by_micros(rt: Runtime, h: tokio::runtime::Handle, delta_micros: i64) {
    let spawn_h = h.clone();
    h.spawn(async move {
        let st = rt.core().get_playback_state();
        if st.duration == 0 {
            return;
        }
        let target = ((st.position as i64) * 1_000_000 + delta_micros).max(0);
        let fraction = (target as f64 / 1_000_000.0 / st.duration as f64).clamp(0.0, 1.0) as f32;
        do_seek(rt, spawn_h, fraction).await;
    });
}

/// QConnect-aware seek (mirrors the now-playing bar's `on_seek`).
async fn do_seek(rt: Runtime, h: tokio::runtime::Handle, fraction: f32) {
    if let Some(svc) = crate::qconnect_service::service() {
        let position_ms =
            (fraction as f64 * rt.core().get_playback_state().duration as f64 * 1000.0).round() as i64;
        match svc.set_position_if_remote(position_ms).await {
            Ok(true) => return,
            Ok(false) => {}
            Err(e) => {
                log::warn!("[media-controls] seek handoff: {e}");
                return;
            }
        }
    }
    crate::playback::seek(rt, h, fraction);
}
