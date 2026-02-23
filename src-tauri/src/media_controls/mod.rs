//! MPRIS/Media controls integration
//!
//! Provides system-level media control integration:
//! - MPRIS on Linux (D-Bus based)
//! - Media key support
//! - Now playing notifications

use serde::Serialize;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig, SeekDirection,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use tauri::{AppHandle, Emitter};

/// Track metadata for media controls
#[derive(Debug, Clone, Default)]
pub struct TrackInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: Option<u64>,
    pub cover_url: Option<String>,
}

/// Media controls manager
pub struct MediaControlsManager {
    controls: Arc<Mutex<Option<MediaControls>>>,
    initialized: Arc<AtomicBool>,
}

impl MediaControlsManager {
    /// Create a new media controls manager
    pub fn new() -> Self {
        let controls = Arc::new(Mutex::new(None));
        Self {
            controls,
            initialized: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn init(&self, app: AppHandle) {
        if self.initialized.swap(true, Ordering::SeqCst) {
            return;
        }

        let controls_clone = self.controls.clone();
        let app_handle = app.clone();

        // Initialize media controls in a separate thread
        // (souvlaki requires a window handle on some platforms)
        thread::spawn(move || {
            let config = PlatformConfig {
                dbus_name: "com.blitzfc.qbz",
                display_name: "QBZ",
                hwnd: None, // Not needed on Linux
            };

            match MediaControls::new(config) {
                Ok(mut mc) => {
                    if let Err(e) = mc.attach(move |event: MediaControlEvent| {
                        log::info!("Media control event: {:?}", event);
                        let payload = MediaControlPayload::from(event);

                        let _ = app_handle.emit("media:control", &payload);
                    }) {
                        log::error!("Failed to attach media controls handler: {}", e);
                        return;
                    }

                    log::info!("Media controls initialized successfully (MPRIS)");

                    if let Ok(mut guard) = controls_clone.lock() {
                        *guard = Some(mc);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to initialize media controls: {}. Media keys won't work.",
                        e
                    );
                }
            }
        });
    }

    /// Update the currently playing track metadata
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
                    log::debug!("Failed to set media metadata: {}", e);
                }
            }
        }
    }

    /// Update playback state
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

    /// Update playback state with progress
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

    /// Set stopped state (no track playing)
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

#[derive(Debug, Serialize)]
struct MediaControlPayload {
    action: String,
    direction: Option<String>,
    offset_secs: Option<i64>,
    position_secs: Option<u64>,
    volume: Option<f64>,
}

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

fn direction_to_string(direction: SeekDirection) -> String {
    match direction {
        SeekDirection::Forward => "forward".to_string(),
        SeekDirection::Backward => "backward".to_string(),
    }
}

impl Default for MediaControlsManager {
    fn default() -> Self {
        Self::new()
    }
}
