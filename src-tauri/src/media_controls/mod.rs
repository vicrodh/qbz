//! MPRIS/Media controls integration
//!
//! Provides system-level media control integration:
//! - MPRIS on Linux (D-Bus based)
//! - Media key support
//! - Now playing notifications

use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::sync::{Arc, Mutex};
use std::thread;

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
}

impl MediaControlsManager {
    /// Create a new media controls manager
    pub fn new() -> Self {
        let controls = Arc::new(Mutex::new(None));
        let controls_clone = controls.clone();

        // Initialize media controls in a separate thread
        // (souvlaki requires a window handle on some platforms)
        thread::spawn(move || {
            let config = PlatformConfig {
                dbus_name: "qbz",
                display_name: "QBZ",
                hwnd: None, // Not needed on Linux
            };

            match MediaControls::new(config) {
                Ok(mut mc) => {
                    // Set up event handler for media key presses
                    // For now, just log events - we'll wire them up later
                    if let Err(e) = mc.attach(move |event: MediaControlEvent| {
                        log::info!("Media control event: {:?}", event);
                        // TODO: Wire these to player commands via Tauri events
                    }) {
                        log::error!("Failed to attach media controls handler: {}", e);
                        return;
                    }

                    log::info!("Media controls initialized successfully (MPRIS)");

                    // Store the controls
                    if let Ok(mut guard) = controls_clone.lock() {
                        *guard = Some(mc);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to initialize media controls: {}. Media keys won't work.", e);
                }
            }
        });

        Self {
            controls,
        }
    }

    /// Update the currently playing track metadata
    pub fn set_metadata(&self, track: &TrackInfo) {
        if let Ok(mut guard) = self.controls.lock() {
            if let Some(controls) = guard.as_mut() {
                let metadata = MediaMetadata {
                    title: Some(track.title.as_str()),
                    artist: Some(track.artist.as_str()),
                    album: Some(track.album.as_str()),
                    duration: track.duration_secs.map(|d| std::time::Duration::from_secs(d)),
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
                let progress = Some(souvlaki::MediaPosition(std::time::Duration::from_secs(position_secs)));
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

impl Default for MediaControlsManager {
    fn default() -> Self {
        Self::new()
    }
}
