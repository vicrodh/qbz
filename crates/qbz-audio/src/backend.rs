//! Audio backend abstraction
//!
//! Provides a unified interface for different audio backends (PipeWire, ALSA, PulseAudio)
//! allowing users to choose their preferred audio stack.

use rodio::MixerDeviceSink;
#[cfg(not(target_os = "linux"))]
use rodio::{
    cpal::traits::{DeviceTrait, HostTrait},
    DeviceSinkBuilder,
};
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

    /// System default backend (non-Linux platforms)
    /// - Uses CPAL default host (CoreAudio on macOS, WASAPI on Windows)
    /// - Automatic device selection via OS audio system
    SystemDefault,
}

impl Default for AudioBackendType {
    fn default() -> Self {
        if cfg!(target_os = "linux") {
            AudioBackendType::PipeWire
        } else {
            AudioBackendType::SystemDefault
        }
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

    /// When true, force PipeWire clock.force-quantum for bit-perfect playback
    pub pw_force_bitperfect: bool,

    /// When true, skip `pactl set-default-sink` on stream creation.
    /// Preserves external routing (JACK, qjackctl, Reaper).
    pub skip_sink_switch: bool,
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
    fn create_output_stream(&self, config: &BackendConfig) -> BackendResult<MixerDeviceSink>;

    /// Create an output stream and optionally return a platform exclusive-mode guard.
    /// Most backends do not need a guard; macOS CoreAudio uses this to keep Hog Mode
    /// owned for the lifetime of the stream.
    fn create_output_stream_with_exclusive_guard(
        &self,
        config: &BackendConfig,
    ) -> BackendResult<(
        MixerDeviceSink,
        Option<crate::coreaudio_direct::CoreAudioExclusiveGuard>,
    )> {
        self.create_output_stream(config).map(|sink| (sink, None))
    }

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
            backends.push(AudioBackendType::SystemDefault);
        }

        backends
    }

    /// Create a backend instance
    pub fn create_backend(backend_type: AudioBackendType) -> BackendResult<Box<dyn AudioBackend>> {
        // Install the custom ALSA error handler once per process, before any
        // CPAL/ALSA enumeration fires. Idempotent via std::sync::Once.
        #[cfg(target_os = "linux")]
        crate::alsa_error_handler::install_once();

        match backend_type {
            AudioBackendType::PipeWire => {
                #[cfg(target_os = "linux")]
                {
                    let backend = crate::pipewire_backend::PipeWireBackend::new()?;
                    Ok(Box::new(backend))
                }
                #[cfg(not(target_os = "linux"))]
                {
                    log::info!(
                        "PipeWire not available on this platform, using system default audio"
                    );
                    Ok(Box::new(CpalDefaultBackend::new()?))
                }
            }
            AudioBackendType::SystemDefault => {
                #[cfg(not(target_os = "linux"))]
                {
                    Ok(Box::new(CpalDefaultBackend::new()?))
                }
                #[cfg(target_os = "linux")]
                {
                    Err("SystemDefault backend is not available on Linux; use PipeWire, ALSA, or Pulse".to_string())
                }
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
                #[cfg(target_os = "linux")]
                {
                    let backend = crate::pulse_backend::PulseBackend::new()?;
                    Ok(Box::new(backend))
                }
                #[cfg(not(target_os = "linux"))]
                {
                    Err("PulseAudio backend only available on Linux".to_string())
                }
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

/// CPAL default backend for non-Linux platforms (macOS CoreAudio, Windows WASAPI).
/// Uses the system default audio device via CPAL without any platform-specific commands.
#[cfg(not(target_os = "linux"))]
pub struct CpalDefaultBackend {
    host: rodio::cpal::Host,
}

#[cfg(not(target_os = "linux"))]
impl CpalDefaultBackend {
    pub fn new() -> BackendResult<Self> {
        Ok(Self {
            host: rodio::cpal::default_host(),
        })
    }
}

#[cfg(not(target_os = "linux"))]
impl AudioBackend for CpalDefaultBackend {
    fn backend_type(&self) -> AudioBackendType {
        AudioBackendType::SystemDefault
    }

    fn enumerate_devices(&self) -> BackendResult<Vec<AudioDevice>> {
        let default_device = self
            .host
            .default_output_device()
            .ok_or_else(|| "No default output device found".to_string())?;

        let default_name = default_device
            .description()
            .map(|desc| desc.name().to_string())
            .unwrap_or_else(|_| "Default Output".to_string());

        let mut devices = Vec::new();
        for device in self
            .host
            .output_devices()
            .map_err(|e| format!("Failed to enumerate output devices: {}", e))?
        {
            let name = device
                .description()
                .map(|desc| desc.name().to_string())
                .unwrap_or_else(|_| "Unknown Device".to_string());
            let is_default = name == default_name;

            // On macOS, probe device capabilities via CoreAudio
            #[cfg(target_os = "macos")]
            let (supported_rates, max_rate, bus_type, is_hw) = { Self::probe_macos_device(&name) };
            #[cfg(not(target_os = "macos"))]
            let (supported_rates, max_rate, bus_type, is_hw): (
                Option<Vec<u32>>,
                Option<u32>,
                Option<String>,
                bool,
            ) = (None, None, None, false);

            devices.push(AudioDevice {
                id: name.clone(),
                name,
                description: None,
                is_default,
                max_sample_rate: max_rate,
                supported_sample_rates: supported_rates,
                device_bus: bus_type,
                is_hardware: is_hw,
            });
        }

        Ok(devices)
    }

    fn create_output_stream(&self, config: &BackendConfig) -> BackendResult<MixerDeviceSink> {
        #[cfg(target_os = "macos")]
        let macos_exclusive_device_name = if config.exclusive_mode && config.device_id.is_none() {
            match crate::coreaudio_direct::resolve_output_device_name(None) {
                Ok(name) => {
                    log::info!(
                        "[CoreAudio] Resolved System Default to '{}' for exclusive stream",
                        name
                    );
                    Some(name)
                }
                Err(e) => {
                    log::warn!(
                        "[CoreAudio] Could not resolve System Default device name: {}",
                        e
                    );
                    None
                }
            }
        } else {
            None
        };

        // On macOS, only exclusive mode takes ownership of the device rate.
        // Shared mode must leave the user's current CoreAudio device rate alone.
        #[cfg(target_os = "macos")]
        {
            if config.exclusive_mode {
                if let Some(ref device_id) = config.device_id {
                    Self::switch_sample_rate_if_needed(device_id, config.sample_rate);
                } else if let Some(ref device_name) = macos_exclusive_device_name {
                    Self::switch_sample_rate_if_needed(device_name, config.sample_rate);
                } else {
                    Self::switch_default_device_rate_if_needed(config.sample_rate);
                }
            }
        }

        #[cfg(target_os = "macos")]
        let effective_device_id = config
            .device_id
            .as_ref()
            .or(macos_exclusive_device_name.as_ref());
        #[cfg(not(target_os = "macos"))]
        let effective_device_id = config.device_id.as_ref();

        let device = if let Some(device_id) = effective_device_id {
            self.host
                .output_devices()
                .map_err(|e| format!("Failed to enumerate devices: {}", e))?
                .find(|d| {
                    d.description()
                        .map(|desc| desc.name() == device_id.as_str())
                        .unwrap_or(false)
                })
                .ok_or_else(|| format!("Device '{}' not found", device_id))?
        } else {
            self.host
                .default_output_device()
                .ok_or_else(|| "No default output device found".to_string())?
        };

        #[cfg(target_os = "macos")]
        let shared_mode_override = if !config.exclusive_mode {
            Self::shared_mode_nominal_stream_config(
                &device,
                effective_device_id.map(|name| name.as_str()),
            )
        } else {
            None
        };

        let builder = DeviceSinkBuilder::from_device(device)
            .map_err(|e| format!("Failed to create device sink builder: {}", e))?;

        #[cfg(target_os = "macos")]
        let mixer_sink = if let Some(override_config) = shared_mode_override {
            let buffer_size =
                rodio::cpal::BufferSize::Fixed((override_config.sample_rate() / 20).max(64));
            builder
                .with_supported_config(&override_config)
                .with_buffer_size(buffer_size)
                .open_stream()
                .map_err(|e| format!("Failed to create output stream: {}", e))?
        } else {
            builder
                .open_stream()
                .map_err(|e| format!("Failed to create output stream: {}", e))?
        };

        #[cfg(not(target_os = "macos"))]
        let mixer_sink = builder
            .open_stream()
            .map_err(|e| format!("Failed to create output stream: {}", e))?;

        Ok(mixer_sink)
    }

    #[cfg(target_os = "macos")]
    fn create_output_stream_with_exclusive_guard(
        &self,
        config: &BackendConfig,
    ) -> BackendResult<(
        MixerDeviceSink,
        Option<crate::coreaudio_direct::CoreAudioExclusiveGuard>,
    )> {
        if !config.exclusive_mode {
            return self.create_output_stream(config).map(|sink| (sink, None));
        }

        // Resolve the target device ONCE before acquiring Hog Mode and pin its
        // name into the config we hand to `create_output_stream`. Without this,
        // the rate-switch and CPAL-stream paths inside `create_output_stream`
        // re-resolve the System Default by name — and macOS reassigns the
        // System Default the moment we hog the previous one (so other apps
        // still get audio). The result is Hog Mode held on device A while the
        // stream is created on device B, which on a multi-device machine
        // (e.g. an external DAC alongside built-in speakers) leaves the DAC
        // hogged-but-unused and audio routed to a device that may not even
        // support the requested sample rate.
        let device_id =
            crate::coreaudio_direct::resolve_output_device_id(config.device_id.as_deref())?;
        let device_name = crate::coreaudio_direct::get_device_name(device_id)?;
        let guard = crate::coreaudio_direct::CoreAudioExclusiveGuard::acquire(device_id)?;

        let mut effective_config = config.clone();
        effective_config.device_id = Some(device_name);

        self.create_output_stream(&effective_config)
            .map(|sink| (sink, Some(guard)))
    }

    fn is_available(&self) -> bool {
        self.host.default_output_device().is_some()
    }

    fn description(&self) -> &'static str {
        "System Audio - Default audio output"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(target_os = "macos")]
impl CpalDefaultBackend {
    /// In macOS shared mode, CPAL's default config can briefly report a stale
    /// sample rate after CoreAudio changes the device's nominal rate. If we
    /// trust the stale rate, playback can run at the wrong speed until the
    /// stream is recreated. Prefer opening the CPAL stream at CoreAudio's
    /// current nominal rate when the two disagree.
    fn shared_mode_nominal_stream_config(
        device: &rodio::cpal::Device,
        effective_device_name: Option<&str>,
    ) -> Option<rodio::cpal::SupportedStreamConfig> {
        use crate::coreaudio_direct;

        let device_id = match effective_device_name {
            Some(name) => coreaudio_direct::find_device_by_name(name).ok().flatten(),
            None => coreaudio_direct::get_default_output_device().ok(),
        }?;
        let nominal_rate = coreaudio_direct::get_nominal_sample_rate(device_id).ok()?;
        let default_config = device.default_output_config().ok()?;
        let default_rate = default_config.sample_rate();
        if nominal_rate == default_rate {
            return None;
        }

        let supported_configs: Vec<_> = device.supported_output_configs().ok()?.collect();
        let matching_config = supported_configs
            .iter()
            .find_map(|range| {
                if range.channels() == default_config.channels()
                    && range.sample_format() == default_config.sample_format()
                {
                    (*range).try_with_sample_rate(nominal_rate)
                } else {
                    None
                }
            })
            .or_else(|| {
                supported_configs
                    .iter()
                    .find_map(|range| (*range).try_with_sample_rate(nominal_rate))
            });

        let device_label = effective_device_name.unwrap_or("System Default");
        if matching_config.is_some() {
            log::warn!(
                "[CoreAudio] Shared-mode rate mismatch on '{}': CPAL default {}Hz vs CoreAudio nominal {}Hz. Opening stream at the nominal rate to avoid wrong-speed playback.",
                device_label,
                default_rate,
                nominal_rate
            );
        } else {
            log::warn!(
                "[CoreAudio] Shared-mode rate mismatch on '{}': CPAL default {}Hz vs CoreAudio nominal {}Hz, but no supported CPAL config matched the nominal rate.",
                device_label,
                default_rate,
                nominal_rate
            );
        }

        matching_config
    }

    /// Probe a macOS audio device for capabilities via CoreAudio APIs.
    /// Returns (supported_rates, max_rate, bus_type, is_hardware).
    fn probe_macos_device(
        device_name: &str,
    ) -> (Option<Vec<u32>>, Option<u32>, Option<String>, bool) {
        use crate::coreaudio_direct;

        let device_id = match coreaudio_direct::find_device_by_name(device_name) {
            Ok(Some(id)) => {
                log::info!("[CoreAudio] Found device '{}' with ID {}", device_name, id);
                id
            }
            Ok(None) => {
                log::debug!(
                    "[CoreAudio] Device '{}' not found via CoreAudio",
                    device_name
                );
                return (None, None, None, false);
            }
            Err(e) => {
                log::debug!("[CoreAudio] Error finding device '{}': {}", device_name, e);
                return (None, None, None, false);
            }
        };

        let supported_rates = coreaudio_direct::query_supported_sample_rates(device_id)
            .inspect(|rates| {
                log::info!(
                    "[CoreAudio] Device '{}' supported rates: {:?}",
                    device_name,
                    rates
                )
            })
            .inspect_err(|e| {
                log::warn!(
                    "[CoreAudio] Failed to query rates for '{}': {}",
                    device_name,
                    e
                )
            })
            .ok()
            .filter(|r| !r.is_empty());
        let max_rate = supported_rates
            .as_ref()
            .and_then(|rates| rates.iter().max().copied());
        let bus_type = coreaudio_direct::get_device_transport_type(device_id);
        let is_hardware = bus_type.as_deref().is_some_and(|t| {
            t == "usb" || t == "built-in" || t == "thunderbolt" || t == "firewire"
        });

        (supported_rates, max_rate, bus_type, is_hardware)
    }

    /// Switch device sample rate before stream creation (if device supports the target rate).
    fn switch_sample_rate_if_needed(device_name: &str, target_rate: u32) {
        use crate::coreaudio_direct;

        log::info!(
            "[CoreAudio] Rate switch requested: device='{}' target={}Hz",
            device_name,
            target_rate
        );

        let device_id = match coreaudio_direct::find_device_by_name(device_name) {
            Ok(Some(id)) => id,
            Ok(None) => {
                log::warn!(
                    "[CoreAudio] Cannot switch rate: device '{}' not found",
                    device_name
                );
                return;
            }
            Err(e) => {
                log::warn!("[CoreAudio] Cannot switch rate: {}", e);
                return;
            }
        };

        // Check if device supports the target rate
        if let Ok(rates) = coreaudio_direct::query_supported_sample_rates(device_id) {
            if !rates.contains(&target_rate) {
                log::debug!(
                    "[CoreAudio] Device '{}' does not support {}Hz, skipping rate switch",
                    device_name,
                    target_rate
                );
                return;
            }
        }

        if let Err(e) = coreaudio_direct::set_nominal_sample_rate(device_id, target_rate) {
            log::warn!("[CoreAudio] Failed to switch sample rate: {}", e);
        }
    }

    /// Switch the default output device's sample rate.
    fn switch_default_device_rate_if_needed(target_rate: u32) {
        use crate::coreaudio_direct;

        let device_id = match coreaudio_direct::get_default_output_device() {
            Ok(id) => id,
            Err(e) => {
                log::debug!(
                    "[CoreAudio] Could not get default device for rate switch: {}",
                    e
                );
                return;
            }
        };

        if let Ok(rates) = coreaudio_direct::query_supported_sample_rates(device_id) {
            if !rates.contains(&target_rate) {
                log::debug!(
                    "[CoreAudio] Default device does not support {}Hz, skipping rate switch",
                    target_rate
                );
                return;
            }
        }

        if let Err(e) = coreaudio_direct::set_nominal_sample_rate(device_id, target_rate) {
            log::warn!(
                "[CoreAudio] Failed to switch default device sample rate: {}",
                e
            );
        }
    }
}
