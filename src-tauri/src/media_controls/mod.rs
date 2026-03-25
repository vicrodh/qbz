//! MPRIS/Media controls integration
//!
//! Provides system-level media control integration:
//! - MPRIS on Linux (D-Bus based)
//! - Media key support
//! - Now playing notifications
//!
//! On FreeBSD (and other non-Linux/macOS/Windows platforms) souvlaki's
//! platform-specific fields (e.g. `dbus_name`) are unavailable, so the
//! manager degrades to a no-op that compiles cleanly and logs a warning.

#[cfg(target_os = "linux")]
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[cfg(target_os = "linux")]
use std::sync::Mutex;
#[cfg(target_os = "linux")]
use std::thread;
use tauri::AppHandle;
#[cfg(target_os = "linux")]
use tauri::Emitter;

#[cfg(target_os = "linux")]
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig, SeekDirection,
};

/// Track metadata for media controls
#[derive(Debug, Clone, Default)]
pub struct TrackInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: Option<u64>,
    pub cover_url: Option<String>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Linux implementation (MPRIS via souvlaki)
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub struct MediaControlsManager {
    controls: Arc<Mutex<Option<MediaControls>>>,
    initialized: Arc<AtomicBool>,
}

#[cfg(target_os = "linux")]
impl MediaControlsManager {
    pub fn new() -> Self {
        Self {
            controls: Arc::new(Mutex::new(None)),
            initialized: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn init(&self, app: AppHandle) {
        if self.initialized.swap(true, Ordering::SeqCst) {
            return;
        }

        let controls_clone = self.controls.clone();
        let app_handle = app.clone();

        thread::spawn(move || {
            let config = PlatformConfig {
                dbus_name: "com.blitzfc.qbz",
                display_name: "QBZ",
                hwnd: None,
            };

            match MediaControls::new(config) {
                Ok(mut mc) => {
                    if let Err(e) = mc.attach(move |event: MediaControlEvent| {
                        log::info!("Media control event: {:?}", event);
                        let payload = MediaControlPayload::from(event);
                        let _ = app_handle.emit("media:control", &payload);
                    }) {
                        log::error!("Failed to attach media controls handler: {:?}", e);
                        return;
                    }

                    log::info!("Media controls initialized successfully (MPRIS)");

                    if let Ok(mut guard) = controls_clone.lock() {
                        *guard = Some(mc);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to initialize media controls: {:?}. Media keys won't work.",
                        e
                    );
                }
            }
        });
    }

    pub fn set_metadata(&self, track: &TrackInfo) {
        if let Ok(mut guard) = self.controls.lock() {
            if let Some(controls) = guard.as_mut() {
                let metadata = MediaMetadata {
                    title: Some(track.title.as_str()),
                    artist: Some(track.artist.as_str()),
                    album: Some(track.album.as_str()),
                    duration: track
                        .duration_secs
                        .map(|d| std::time::Duration::from_secs(d)),
                    cover_url: track.cover_url.as_deref(),
                };

                if let Err(e) = controls.set_metadata(metadata) {
                    log::debug!("Failed to set media metadata: {:?}", e);
                }
            }
        }
    }

    pub fn set_playback(&self, playing: bool) {
        if let Ok(mut guard) = self.controls.lock() {
            if let Some(controls) = guard.as_mut() {
                let playback = if playing {
                    MediaPlayback::Playing { progress: None }
                } else {
                    MediaPlayback::Paused { progress: None }
                };

                if let Err(e) = controls.set_playback(playback) {
                    log::debug!("Failed to set playback state: {}", e);
                }
            }
        }
    }

    pub fn set_playback_with_progress(&self, playing: bool, position_secs: u64) {
        if let Ok(mut guard) = self.controls.lock() {
            if let Some(controls) = guard.as_mut() {
                let progress = Some(souvlaki::MediaPosition(std::time::Duration::from_secs(
                    position_secs,
                )));
                let playback = if playing {
                    MediaPlayback::Playing { progress }
                } else {
                    MediaPlayback::Paused { progress }
                };

                if let Err(e) = controls.set_playback(playback) {
                    log::debug!("Failed to set playback state: {}", e);
                }
            }
        }
    }

    pub fn set_stopped(&self) {
        if let Ok(mut guard) = self.controls.lock() {
            if let Some(controls) = guard.as_mut() {
                if let Err(e) = controls.set_playback(MediaPlayback::Stopped) {
                    log::debug!("Failed to set stopped state: {}", e);
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl Default for MediaControlsManager {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// No-op stub for non-Linux platforms (FreeBSD, macOS, Windows)
// souvlaki's PlatformConfig has Linux-specific fields (dbus_name) so we can't
// use it directly.  The stub compiles cleanly and does nothing.
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(not(target_os = "linux"))]
pub struct MediaControlsManager {
    initialized: Arc<AtomicBool>,
}

#[cfg(not(target_os = "linux"))]
impl MediaControlsManager {
    pub fn new() -> Self {
        Self {
            initialized: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn init(&self, _app: AppHandle) {
        if self.initialized.swap(true, Ordering::SeqCst) {
            return;
        }
        log::info!("Media controls: no-op on this platform (MPRIS not available)");
    }

    pub fn set_metadata(&self, _track: &TrackInfo) {}
    pub fn set_playback(&self, _playing: bool) {}
    pub fn set_playback_with_progress(&self, _playing: bool, _position_secs: u64) {}
    pub fn set_stopped(&self) {}
}

#[cfg(not(target_os = "linux"))]
impl Default for MediaControlsManager {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Types used only by the Linux implementation (kept Linux-gated to avoid
// pulling in souvlaki on other platforms)
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
#[derive(Debug, Serialize)]
struct MediaControlPayload {
    action: String,
    direction: Option<String>,
    offset_secs: Option<i64>,
    position_secs: Option<u64>,
    volume: Option<f64>,
}

#[cfg(target_os = "linux")]
impl From<MediaControlEvent> for MediaControlPayload {
    fn from(event: MediaControlEvent) -> Self {
        match event {
            MediaControlEvent::Play => Self::action_only("play"),
            MediaControlEvent::Pause => Self::action_only("pause"),
            MediaControlEvent::Toggle => Self::action_only("toggle"),
            MediaControlEvent::Next => Self::action_only("next"),
            MediaControlEvent::Previous => Self::action_only("previous"),
            MediaControlEvent::Stop => Self::action_only("stop"),
            MediaControlEvent::Seek(direction) => Self {
                action: "seek".to_string(),
                direction: Some(direction_to_string(direction)),
                offset_secs: None,
                position_secs: None,
                volume: None,
            },
            MediaControlEvent::SeekBy(direction, duration) => {
                let offset = duration.as_secs() as i64;
                let signed_offset = match direction {
                    SeekDirection::Forward => offset,
                    SeekDirection::Backward => -offset,
                };
                Self {
                    action: "seek_by".to_string(),
                    direction: Some(direction_to_string(direction)),
                    offset_secs: Some(signed_offset),
                    position_secs: None,
                    volume: None,
                }
            }
            MediaControlEvent::SetPosition(position) => Self {
                action: "set_position".to_string(),
                direction: None,
                offset_secs: None,
                position_secs: Some(position.0.as_secs()),
                volume: None,
            },
            MediaControlEvent::SetVolume(volume) => Self {
                action: "set_volume".to_string(),
                direction: None,
                offset_secs: None,
                position_secs: None,
                volume: Some(volume),
            },
            MediaControlEvent::OpenUri(_) => Self::action_only("open_uri"),
            MediaControlEvent::Raise => Self::action_only("raise"),
            MediaControlEvent::Quit => Self::action_only("quit"),
        }
    }
}

#[cfg(target_os = "linux")]
impl MediaControlPayload {
    fn action_only(action: &str) -> Self {
        Self {
            action: action.to_string(),
            direction: None,
            offset_secs: None,
            position_secs: None,
            volume: None,
        }
    }
}

#[cfg(target_os = "linux")]
fn direction_to_string(direction: SeekDirection) -> String {
    match direction {
        SeekDirection::Forward => "forward".to_string(),
        SeekDirection::Backward => "backward".to_string(),
    }
}
