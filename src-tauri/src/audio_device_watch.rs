//! Audio output device availability watch.
//!
//! Pure helpers to check whether the user's currently-selected output
//! device is still present in the system's device list, plus a Tauri
//! event emitter so the frontend can surface a toast when the device
//! has gone missing (USB unplugged, KVM switched, PipeWire sink
//! removed, etc.).
//!
//! Intentionally does NOT touch pipewire_backend, init_device or
//! audio_settings — the sacred audio init path is preserved. We just
//! run a lightweight check before play/resume and emit an event on
//! mismatch. Existing init-time fallback to default still takes over
//! automatically, this just tells the user it happened.
//!
//! Level A of issue #307 (notification). The same helpers are reused
//! verbatim by a future background watchdog (Level B).

use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

use crate::config::audio_settings::AudioSettingsState;

/// Reason payload for the `audio:device-missing` Tauri event.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceMissingPayload {
    /// The device the user configured but that is no longer present.
    pub wanted: String,
    /// Snapshot of device names still available — lets the UI suggest
    /// a replacement without doing its own enumeration round-trip.
    pub available: Vec<String>,
}

/// Result of a presence check.
pub enum DevicePresence {
    /// User has no explicit output device configured — always present
    /// (we're using whatever the system default is).
    UsingDefault,
    /// Configured device was found in the enumerated list.
    Present,
    /// Configured device is NOT in the enumerated list.
    Missing {
        wanted: String,
        available: Vec<String>,
    },
    /// We failed to enumerate devices (e.g. host was shut down) —
    /// treat as inconclusive rather than missing to avoid false alarms.
    Inconclusive,
}

/// Enumerate output devices and compare against the configured name.
/// Does not lock any of the audio state — cheap to call before every
/// play/resume.
pub fn check_selected_device_presence(settings: &AudioSettingsState) -> DevicePresence {
    let wanted = {
        let guard = match settings.store.lock() {
            Ok(g) => g,
            Err(_) => return DevicePresence::Inconclusive,
        };
        let store = match guard.as_ref() {
            Some(s) => s,
            None => return DevicePresence::Inconclusive,
        };
        match store.get_settings().ok().and_then(|s| s.output_device) {
            Some(name) if !name.is_empty() => name,
            _ => return DevicePresence::UsingDefault,
        }
    };

    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let host = rodio::cpal::default_host();
    let devices = match host.output_devices() {
        Ok(d) => d,
        Err(e) => {
            log::warn!("[audio-watch] failed to enumerate devices: {}", e);
            return DevicePresence::Inconclusive;
        }
    };

    // IMPORTANT: match against the same name shape the UI persisted.
    // Settings stores what get_audio_devices() wrote, which comes from
    // DeviceTrait::description() (richer, stable identifier). The
    // legacy DeviceTrait::name() returns a different string on some
    // platforms (especially PipeWire-backed Linux) and would cause
    // every configured device to look 'missing' here.
    fn preferred_name(d: &rodio::cpal::Device) -> Option<String> {
        if let Ok(desc) = d.description() {
            return Some(desc.name().to_string());
        }
        d.name().ok()
    }

    let mut available: Vec<String> = Vec::new();
    let mut found = false;
    for device in devices {
        if let Some(name) = preferred_name(&device) {
            if name == wanted {
                found = true;
            }
            available.push(name);
        }
    }

    // Empty enumeration is treated as inconclusive, not missing.
    // cpal's device list can come back empty right after app launch
    // or while PipeWire is still populating its sink graph — that's
    // not the same as the user's device genuinely disappearing and
    // firing a "device missing" toast under those conditions produces
    // false positives (seen when pressing play on a restored session).
    if available.is_empty() {
        return DevicePresence::Inconclusive;
    }

    if found {
        DevicePresence::Present
    } else {
        DevicePresence::Missing { wanted, available }
    }
}

/// Rate-limiter for the missing-device toast. We don't want to fire
/// the event on every play/resume once the user has already been
/// notified — `true` means "this is new info, emit".
#[derive(Debug, Default)]
pub struct DeviceMissingThrottle {
    last_wanted: std::sync::Mutex<Option<String>>,
}

impl DeviceMissingThrottle {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Returns true only the first time we observe this missing name.
    /// Clears when the device reappears so the next loss re-notifies.
    pub fn should_emit(&self, presence: &DevicePresence) -> bool {
        let mut last = match self.last_wanted.lock() {
            Ok(l) => l,
            Err(_) => return false,
        };
        match presence {
            DevicePresence::Missing { wanted, .. } => {
                if last.as_deref() == Some(wanted.as_str()) {
                    false
                } else {
                    *last = Some(wanted.clone());
                    true
                }
            }
            _ => {
                *last = None;
                false
            }
        }
    }
}

/// Emit `audio:device-missing` if the user's chosen output device has
/// disappeared. Silent when the device is present / using default /
/// enumeration inconclusive. Rate-limited via the throttle.
pub fn emit_missing_if_needed(
    app: &AppHandle,
    settings: &AudioSettingsState,
    throttle: &DeviceMissingThrottle,
) {
    let presence = check_selected_device_presence(settings);
    if !throttle.should_emit(&presence) {
        return;
    }
    if let DevicePresence::Missing { wanted, available } = presence {
        log::warn!(
            "[audio-watch] configured output device '{}' is no longer present ({} devices available)",
            wanted,
            available.len()
        );
        let payload = DeviceMissingPayload { wanted, available };
        if let Err(e) = app.emit("audio:device-missing", &payload) {
            log::warn!("[audio-watch] failed to emit device-missing event: {}", e);
        }
    }
}
