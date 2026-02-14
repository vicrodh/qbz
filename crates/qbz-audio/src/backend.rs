//! Audio backend abstraction
//!
//! Provides a unified interface for different audio backends (PipeWire, ALSA, PulseAudio)
//! allowing users to choose their preferred audio stack.

use rodio::{OutputStream, OutputStreamHandle};
use serde::{Deserialize, Serialize};

/// Supported audio backends
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioBackendType {
    /// PipeWire backend (modern, recommended)
    /// - Supports device selection without changing system default
    /// - Uses PULSE_SINK environment variable
    /// - Compatible with PulseAudio apps
    PipeWire,

    /// ALSA backend (direct hardware access)
    /// - True exclusive mode (blocks device for other apps)
    /// - Bit-perfect guaranteed
    /// - Lowest latency
    /// - Requires manual device selection (hw:X,Y)
    Alsa,

    /// PulseAudio backend (legacy compatibility)
    /// - Similar to PipeWire but older
    /// - Fallback for systems without PipeWire
    Pulse,
}

impl Default for AudioBackendType {
    fn default() -> Self {
        // PipeWire is the modern default on Linux
        AudioBackendType::PipeWire
    }
}

/// ALSA plugin type (only relevant for ALSA backend)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlsaPlugin {
    /// Direct hardware access (hw)
    /// - Bit-perfect, exclusive
    /// - No automatic format conversion
    /// - Blocks device for other apps
    Hw,

    /// Plug hardware access (plughw)
    /// - Automatic format conversion
    /// - Resampling if needed
    /// - Still relatively direct
    PlugHw,

    /// PCM device (default)
    /// - Generic ALSA device
    /// - Most compatible
    Pcm,
}

impl Default for AlsaPlugin {
    fn default() -> Self {
        // Hw is the audiophile choice
        AlsaPlugin::Hw
    }
}

/// Audio device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    /// Internal device identifier (e.g., "hw:4,0" for ALSA, sink name for PipeWire)
    pub id: String,

    /// User-friendly display name
    pub name: String,

    /// Detailed description (optional)
    pub description: Option<String>,

    /// Whether this is the system default device
    pub is_default: bool,

    /// Maximum supported sample rate (if known)
    pub max_sample_rate: Option<u32>,

    /// Supported sample rates (common audio rates that the device supports)
    /// Contains values like 44100, 48000, 88200, 96000, 176400, 192000, etc.
    pub supported_sample_rates: Option<Vec<u32>>,

    /// Device bus type (for PipeWire): "usb", "pci", "bluetooth", or None
    pub device_bus: Option<String>,

    /// Whether this is a hardware device (has HARDWARE flag in PipeWire)
    pub is_hardware: bool,
}

/// Audio backend configuration
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Backend type
    pub backend_type: AudioBackendType,

    /// Device ID (backend-specific)
    pub device_id: Option<String>,

    /// ALSA plugin (only used if backend_type == Alsa)
    pub alsa_plugin: Option<AlsaPlugin>,

    /// Sample rate (for stream creation)
    pub sample_rate: u32,

    /// Channels
    pub channels: u16,

    /// Exclusive mode flag
    pub exclusive_mode: bool,
}

/// Result type for backend operations
pub type BackendResult<T> = Result<T, String>;

/// ALSA Direct stream error classification
/// Used to determine if fallback to plughw is appropriate
#[derive(Debug, Clone)]
pub enum AlsaDirectError {
    /// PCM format not supported by hardware (can fallback to plughw)
    UnsupportedFormat(String),
    /// Device is busy/in use by another application
    DeviceBusy(String),
    /// Permission denied to access device
    PermissionDenied(String),
    /// Invalid parameters (channels, sample rate)
    InvalidParams(String),
    /// Device not found
    DeviceNotFound(String),
    /// Generic/unknown error
    Other(String),
}

impl std::fmt::Display for AlsaDirectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlsaDirectError::UnsupportedFormat(msg) => write!(f, "Unsupported format: {}", msg),
            AlsaDirectError::DeviceBusy(msg) => write!(f, "Device busy: {}", msg),
            AlsaDirectError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            AlsaDirectError::InvalidParams(msg) => write!(f, "Invalid parameters: {}", msg),
            AlsaDirectError::DeviceNotFound(msg) => write!(f, "Device not found: {}", msg),
            AlsaDirectError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl AlsaDirectError {
    /// Check if this error allows fallback to plughw
    pub fn allows_plughw_fallback(&self) -> bool {
        matches!(self, AlsaDirectError::UnsupportedFormat(_))
    }

    /// Create from raw ALSA error message
    pub fn from_alsa_error(msg: &str) -> Self {
        let msg_lower = msg.to_lowercase();

        if msg_lower.contains("no supported audio format")
            || msg_lower.contains("format")
            || msg_lower.contains("s24_3le")
            || msg_lower.contains("s24le")
            || msg_lower.contains("sample format")
        {
            AlsaDirectError::UnsupportedFormat(msg.to_string())
        } else if msg_lower.contains("busy")
            || msg_lower.contains("resource temporarily unavailable")
            || msg_lower.contains("device or resource busy")
        {
            AlsaDirectError::DeviceBusy(msg.to_string())
        } else if msg_lower.contains("permission")
            || msg_lower.contains("access denied")
            || msg_lower.contains("operation not permitted")
        {
            AlsaDirectError::PermissionDenied(msg.to_string())
        } else if msg_lower.contains("not found")
            || msg_lower.contains("no such")
            || msg_lower.contains("doesn't exist")
        {
            AlsaDirectError::DeviceNotFound(msg.to_string())
        } else if msg_lower.contains("invalid")
            || msg_lower.contains("channels")
            || msg_lower.contains("rate")
        {
            AlsaDirectError::InvalidParams(msg.to_string())
        } else {
            AlsaDirectError::Other(msg.to_string())
        }
    }
}

/// Runtime mode for bit-perfect status tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BitPerfectMode {
    /// Direct hardware access (hw:), guaranteed bit-perfect
    DirectHardware,
    /// Plugin hardware fallback (plughw:), bit-perfect with format conversion only
    PluginFallback,
    /// Not using bit-perfect path (pcm, pipewire, pulse)
    Disabled,
}

/// Audio backend trait
///
/// All audio backends must implement this trait to provide
/// a consistent interface for device enumeration and stream creation.
pub trait AudioBackend: Send + Sync {
    /// Get the backend type
    fn backend_type(&self) -> AudioBackendType;

    /// Enumerate available audio devices for this backend
    fn enumerate_devices(&self) -> BackendResult<Vec<AudioDevice>>;

    /// Create an output stream for the given configuration
    fn create_output_stream(
        &self,
        config: &BackendConfig,
    ) -> BackendResult<(OutputStream, OutputStreamHandle)>;

    /// Check if this backend is available on the current system
    fn is_available(&self) -> bool;

    /// Get a description of this backend for UI display
    fn description(&self) -> &'static str;

    /// Downcast to concrete type (for ALSA Direct stream creation)
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Backend manager - factory for creating backends
pub struct BackendManager;

impl BackendManager {
    /// Get all available backends on this system
    pub fn available_backends() -> Vec<AudioBackendType> {
        let mut backends = Vec::new();

        #[cfg(target_os = "linux")]
        {
            // PipeWire (check if running)
            if Self::is_pipewire_available() {
                backends.push(AudioBackendType::PipeWire);
            }

            // ALSA (always available on Linux)
            backends.push(AudioBackendType::Alsa);

            // PulseAudio (check if running)
            if Self::is_pulse_available() {
                backends.push(AudioBackendType::Pulse);
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            // On non-Linux, only PipeWire backend (which uses CPAL default)
            backends.push(AudioBackendType::PipeWire);
        }

        backends
    }

    /// Create a backend instance
    pub fn create_backend(backend_type: AudioBackendType) -> BackendResult<Box<dyn AudioBackend>> {
        match backend_type {
            AudioBackendType::PipeWire => {
                let backend = crate::pipewire_backend::PipeWireBackend::new()?;
                Ok(Box::new(backend))
            }
            AudioBackendType::Alsa => {
                #[cfg(target_os = "linux")]
                {
                    let backend = crate::alsa_backend::AlsaBackend::new()?;
                    Ok(Box::new(backend))
                }
                #[cfg(not(target_os = "linux"))]
                {
                    Err("ALSA backend only available on Linux".to_string())
                }
            }
            AudioBackendType::Pulse => {
                let backend = crate::pulse_backend::PulseBackend::new()?;
                Ok(Box::new(backend))
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn is_pipewire_available() -> bool {
        // Check if PipeWire/PulseAudio is available using pactl
        // PipeWire provides PulseAudio compatibility, so pactl works for both
        // This is more reliable than pw-cli, especially in sandboxed environments (Flatpak)
        std::process::Command::new("pactl")
            .arg("info")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(target_os = "linux")]
    fn is_pulse_available() -> bool {
        // Check if PulseAudio is running
        std::process::Command::new("pactl")
            .arg("info")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}
