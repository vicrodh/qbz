//! CoreAudio direct access for macOS
//!
//! Provides device capability probing and sample rate switching on macOS
//! using the coreaudio-rs safe wrappers.
//!
//! Phase 1: Device probing + nominal sample rate switching (shared mode)
//! Phase 2 (future): Hog mode + integer mode + IO proc for bit-perfect playback

#[cfg(target_os = "macos")]
use coreaudio::audio_unit::macos_helpers;
#[cfg(target_os = "macos")]
use coreaudio::audio_unit::Scope;

/// CoreAudio device ID (re-exported so callers don't need objc2_core_audio)
#[cfg(target_os = "macos")]
pub type AudioDeviceID = u32;

// CoreAudio transport type constants (FourCC values from AudioHardware.h)
#[cfg(target_os = "macos")]
mod transport_types {
    pub const BUILT_IN: u32 = 0x626c746e;    // 'bltn'
    pub const USB: u32 = 0x75736220;          // 'usb '
    pub const BLUETOOTH: u32 = 0x626c7565;    // 'blue'
    pub const BLUETOOTH_LE: u32 = 0x626c6561; // 'blea'
    pub const HDMI: u32 = 0x68646d69;         // 'hdmi'
    pub const DISPLAY_PORT: u32 = 0x64707274; // 'dprt'
    pub const THUNDERBOLT: u32 = 0x7468756e;  // 'thun'
    pub const FIREWIRE: u32 = 0x31333934;     // '1394'
    pub const VIRTUAL: u32 = 0x76697274;      // 'virt'
    pub const AGGREGATE: u32 = 0x67727570;    // 'grup'
}

/// Common audio sample rates to check against device capabilities
#[cfg(target_os = "macos")]
const COMMON_SAMPLE_RATES: &[u32] = &[
    44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000, 705600, 768000,
];

/// Query supported sample rates for a CoreAudio device.
/// Returns discrete rates from the device's available nominal sample rate ranges.
#[cfg(target_os = "macos")]
pub fn query_supported_sample_rates(device_id: AudioDeviceID) -> Result<Vec<u32>, String> {
    let ranges = macos_helpers::get_available_sample_rates(device_id)
        .map_err(|e| format!("Failed to get sample rate ranges: {:?}", e))?;

    let mut rates = Vec::new();
    for range in &ranges {
        if (range.mMinimum - range.mMaximum).abs() < 0.5 {
            // Point value (min == max)
            rates.push(range.mMinimum as u32);
        } else {
            // Continuous range — check which common rates fall within it
            for &rate in COMMON_SAMPLE_RATES {
                let rate_f = rate as f64;
                if rate_f >= range.mMinimum && rate_f <= range.mMaximum {
                    rates.push(rate);
                }
            }
        }
    }

    rates.sort_unstable();
    rates.dedup();
    Ok(rates)
}

/// Set the nominal sample rate of a device.
/// Delegates to coreaudio-rs which handles async confirmation with a 2-second timeout.
#[cfg(target_os = "macos")]
pub fn set_nominal_sample_rate(device_id: AudioDeviceID, target_rate: u32) -> Result<(), String> {
    log::info!(
        "[CoreAudio] Switching sample rate to {}Hz on device {}",
        target_rate,
        device_id
    );

    macos_helpers::set_device_sample_rate(device_id, target_rate as f64)
        .map_err(|e| format!("Failed to set sample rate to {}Hz: {:?}", target_rate, e))?;

    log::info!("[CoreAudio] Sample rate switched to {}Hz", target_rate);
    Ok(())
}

/// Get the default output device ID.
#[cfg(target_os = "macos")]
pub fn get_default_output_device() -> Result<AudioDeviceID, String> {
    macos_helpers::get_default_device_id(false)
        .ok_or_else(|| "No default output device found".to_string())
}

/// Get all output device IDs.
#[cfg(target_os = "macos")]
pub fn get_output_device_ids() -> Result<Vec<AudioDeviceID>, String> {
    macos_helpers::get_audio_device_ids_for_scope(Scope::Output)
        .map_err(|e| format!("Failed to enumerate output devices: {:?}", e))
}

/// Get the name of a CoreAudio device.
#[cfg(target_os = "macos")]
pub fn get_device_name(device_id: AudioDeviceID) -> Result<String, String> {
    macos_helpers::get_device_name(device_id)
        .map_err(|e| format!("Failed to get device name: {:?}", e))
}

/// Find a CoreAudio output device ID by its name.
#[cfg(target_os = "macos")]
pub fn find_device_by_name(name: &str) -> Result<Option<AudioDeviceID>, String> {
    // get_device_id_from_name: input=false means output device
    Ok(macos_helpers::get_device_id_from_name(name, false))
}

/// Get the transport type of a device (USB, built-in, Bluetooth, etc.)
#[cfg(target_os = "macos")]
pub fn get_device_transport_type(device_id: AudioDeviceID) -> Option<String> {
    let transport = macos_helpers::get_device_transport_type(device_id).ok()?;

    let transport_str = if transport == transport_types::BUILT_IN {
        "built-in"
    } else if transport == transport_types::USB {
        "usb"
    } else if transport == transport_types::BLUETOOTH
        || transport == transport_types::BLUETOOTH_LE
    {
        "bluetooth"
    } else if transport == transport_types::HDMI
        || transport == transport_types::DISPLAY_PORT
    {
        "hdmi"
    } else if transport == transport_types::THUNDERBOLT {
        "thunderbolt"
    } else if transport == transport_types::FIREWIRE {
        "firewire"
    } else if transport == transport_types::VIRTUAL {
        "virtual"
    } else if transport == transport_types::AGGREGATE {
        "aggregate"
    } else {
        "unknown"
    };

    Some(transport_str.to_string())
}

// ---- Non-macOS stubs ----

/// Query supported sample rates (stub for non-macOS)
#[cfg(not(target_os = "macos"))]
pub fn query_supported_sample_rates(_device_name: &str) -> Result<Vec<u32>, String> {
    Ok(Vec::new())
}

/// Get the current nominal sample rate (stub for non-macOS)
#[cfg(not(target_os = "macos"))]
pub fn get_nominal_sample_rate_by_name(_device_name: &str) -> Result<u32, String> {
    Err("CoreAudio is only available on macOS".to_string())
}

/// Set the nominal sample rate (stub for non-macOS)
#[cfg(not(target_os = "macos"))]
pub fn set_nominal_sample_rate_by_name(
    _device_name: &str,
    _target_rate: u32,
) -> Result<(), String> {
    Err("CoreAudio is only available on macOS".to_string())
}
