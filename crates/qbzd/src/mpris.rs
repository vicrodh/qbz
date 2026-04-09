//! Headless MPRIS media controls via D-Bus (Linux).
//!
//! Uses souvlaki with hwnd: None — works without a window on Linux.
//! Receives media key events (play/pause/next/prev) and routes them
//! to QbzCore.

use std::sync::Arc;
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};

use crate::adapter::DaemonAdapter;

/// Start MPRIS media controls in a background thread.
/// Returns a handle to update metadata/playback state.
pub fn start_mpris(
    core: Arc<qbz_core::QbzCore<DaemonAdapter>>,
) -> Option<Arc<std::sync::Mutex<MediaControls>>> {
    let config = PlatformConfig {
        dbus_name: "com.blitzfc.qbzd",
        display_name: "QBZ Daemon",
        hwnd: None,
    };

    let mut controls = match MediaControls::new(config) {
        Ok(mc) => mc,
        Err(e) => {
            log::warn!("[qbzd/mpris] Failed to create media controls: {:?}", e);
            return None;
        }
    };

    let core_for_handler = core.clone();
    if let Err(e) = controls.attach(move |event: MediaControlEvent| {
        let core = core_for_handler.clone();
        match event {
            MediaControlEvent::Play => { let _ = core.resume(); }
            MediaControlEvent::Pause => { let _ = core.pause(); }
            MediaControlEvent::Toggle => {
                if core.player().state.is_playing() {
                    let _ = core.pause();
                } else {
                    let _ = core.resume();
                }
            }
            MediaControlEvent::Next => {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        core.next_track().await;
                    });
                });
            }
            MediaControlEvent::Previous => {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        core.previous_track().await;
                    });
                });
            }
            MediaControlEvent::Stop => { let _ = core.stop(); }
            _ => {}
        }
    }) {
        log::warn!("[qbzd/mpris] Failed to attach handler: {:?}", e);
        return None;
    }

    // Set initial state
    let _ = controls.set_playback(MediaPlayback::Stopped);
    log::info!("[qbzd/mpris] MPRIS media controls initialized (D-Bus)");

    Some(Arc::new(std::sync::Mutex::new(controls)))
}

/// Update MPRIS metadata from playback state.
pub fn update_mpris_metadata(
    controls: &Arc<std::sync::Mutex<MediaControls>>,
    title: &str,
    artist: &str,
    album: &str,
    duration_secs: u64,
) {
    if let Ok(mut mc) = controls.lock() {
        let _ = mc.set_metadata(MediaMetadata {
            title: Some(title),
            artist: Some(artist),
            album: Some(album),
            duration: Some(std::time::Duration::from_secs(duration_secs)),
            ..Default::default()
        });
    }
}

/// Update MPRIS playback state.
pub fn update_mpris_playback(
    controls: &Arc<std::sync::Mutex<MediaControls>>,
    is_playing: bool,
    position_secs: u64,
) {
    if let Ok(mut mc) = controls.lock() {
        let playback = if is_playing {
            MediaPlayback::Playing {
                progress: Some(souvlaki::MediaPosition(std::time::Duration::from_secs(position_secs))),
            }
        } else {
            MediaPlayback::Paused {
                progress: Some(souvlaki::MediaPosition(std::time::Duration::from_secs(position_secs))),
            }
        };
        let _ = mc.set_playback(playback);
    }
}
