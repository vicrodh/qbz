//! ALSA audio backend (direct hardware access)
//!
//! Provides direct access to ALSA hardware devices for:
//! - True exclusive mode (blocks device for other apps)
//! - Bit-perfect playback (no resampling)
//! - Low-latency audio output
//!
//! Uses CPAL's ALSA host with specific device selection.

use super::backend::{AlsaPlugin, AudioBackend, AudioBackendType, AudioDevice, BackendConfig, BackendResult};
use rodio::{
    cpal::{
        traits::{DeviceTrait, HostTrait},
        BufferSize, SampleFormat, SampleRate, StreamConfig, SupportedBufferSize, SupportedStreamConfig,
    },
    OutputStream, OutputStreamHandle,
};
use std::process::Command;

/// Common audio sample rates to check for device support
const COMMON_SAMPLE_RATES: &[u32] = &[
    44100,  // CD quality
    48000,  // DVD/DAT quality
    88200,  // 2x CD
    96000,  // DVD-Audio
    176400, // 4x CD
    192000, // High-res audio
    352800, // DSD64 equivalent
    384000, // Ultra high-res
];

/// Extract supported sample rates from a CPAL device
fn get_supported_sample_rates(device: &rodio::cpal::Device) -> Option<Vec<u32>> {
    use rodio::cpal::traits::DeviceTrait;

    let configs = device.supported_output_configs().ok()?;
    let configs_vec: Vec<_> = configs.collect();

    if configs_vec.is_empty() {
        return None;
    }

    let mut supported = Vec::new();

    for rate in COMMON_SAMPLE_RATES {
        let sample_rate = rodio::cpal::SampleRate(*rate);
        // Check if any config supports this rate
        let is_supported = configs_vec.iter().any(|config| {
            sample_rate >= config.min_sample_rate() && sample_rate <= config.max_sample_rate()
        });
        if is_supported {
            supported.push(*rate);
        }
    }

    if supported.is_empty() {
        None
    } else {
        Some(supported)
    }
}

pub struct AlsaBackend {
    host: rodio::cpal::Host,
}

impl AlsaBackend {
    pub fn new() -> BackendResult<Self> {
        // Try to get ALSA host
        let available_hosts = rodio::cpal::available_hosts();

        // Check if ALSA is available
        if !available_hosts.iter().any(|h| h.name() == "ALSA") {
            return Err("ALSA host not available on this system".to_string());
        }

        // Get ALSA host
        let host = rodio::cpal::host_from_id(
            available_hosts
                .into_iter()
                .find(|h| h.name() == "ALSA")
                .ok_or("ALSA host not found".to_string())?
        ).map_err(|e| format!("Failed to create ALSA host: {}", e))?;

        log::info!("[ALSA Backend] Initialized successfully");

        Ok(Self { host })
    }

    /// Enumerate ALSA devices via CPAL with descriptions from aplay -L
    fn enumerate_via_aplay(&self) -> BackendResult<Vec<AudioDevice>> {
        // First: Get devices from CPAL (these are the device IDs that actually work)
        let cpal_devices_result = self.enumerate_via_cpal();
        if cpal_devices_result.is_err() {
            return cpal_devices_result;
        }
        let mut devices = cpal_devices_result.unwrap();

        // Second: Build description map from aplay -L
        let output = Command::new("aplay")
            .arg("-L")
            .output()
            .map_err(|e| format!("Failed to run aplay -L: {}", e))?;

        if !output.status.success() {
            log::warn!("[ALSA Backend] aplay -L failed, using CPAL names only");
            return Ok(devices);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        // Build map: device_id -> friendly description
        let mut description_map = std::collections::HashMap::new();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim_end();

            // Device ID lines don't start with whitespace
            if !line.starts_with(' ') && !line.is_empty() {
                let device_id = line.to_string();

                // Get description from next line (if it exists and is indented)
                let description = if i + 1 < lines.len() && lines[i + 1].starts_with(' ') {
                    Some(lines[i + 1].trim().to_string())
                } else {
                    None
                };

                if let Some(desc) = description {
                    description_map.insert(device_id, desc);
                }

                // Skip description lines
                i += 1;
                while i < lines.len() && lines[i].starts_with(' ') {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        // Build card number -> card name map from aplay -l
        let mut card_map = std::collections::HashMap::new();
        let aplay_l_output = Command::new("aplay")
            .arg("-l")
            .output()
            .ok()
            .and_then(|o| if o.status.success() { Some(o) } else { None });

        if let Some(output) = aplay_l_output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(card_info) = line.strip_prefix("card ") {
                    // Parse: "card 4: C20 [Cambridge Audio USB Audio 2.0], device 0: ..."
                    let parts: Vec<&str> = card_info.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        let card_num = parts[0].trim();
                        let rest = parts[1].trim();

                        // Extract description from brackets [...]
                        if let Some(start) = rest.find('[') {
                            if let Some(end) = rest.find(']') {
                                let card_desc = &rest[start + 1..end];
                                card_map.insert(card_num.to_string(), card_desc.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Third: Update device descriptions from aplay -L map
        for device in &mut devices {
            if let Some(desc) = description_map.get(&device.name) {
                device.description = Some(desc.clone());
            }
        }

        // Fourth: Add hw:X,Y and plughw:X,Y devices manually (CPAL doesn't enumerate these)
        // Parse aplay -l to create these devices
        for (card_num, card_desc) in &card_map {
            // Create hw:X,0 device (bit-perfect direct hardware access)
            let hw_device_id = format!("hw:{},0", card_num);

            // Test if CPAL can open this device
            let can_open = self.host
                .output_devices()
                .ok()
                .and_then(|mut devs| devs.find(|d| d.name().ok().as_deref() == Some(&hw_device_id)))
                .is_some();

            if !can_open {
                // Try to get supported configs to validate device exists
                // Create a test device to check if it's valid
                if let Ok(_test_devices) = self.host.output_devices() {
                    // CPAL may not enumerate hw: devices but can still open them
                    // We'll add them anyway and let the backend handle errors
                    devices.push(AudioDevice {
                        id: hw_device_id.clone(),
                        name: hw_device_id.clone(),
                        description: Some(format!("{} (Direct Hardware - Bit-perfect)", card_desc)),
                        is_default: false,
                        max_sample_rate: Some(384000), // Assume high sample rate capability
                        supported_sample_rates: None, // Can't detect without CPAL access
                        device_bus: None,
                        is_hardware: true,
                    });
                }
            } else {
                // CPAL found it, get real sample rate info
                let cpal_device = self.host
                    .output_devices()
                    .ok()
                    .and_then(|mut devs| {
                        devs.find(|d| d.name().ok().as_deref() == Some(&hw_device_id))
                    });

                let max_sample_rate = cpal_device.as_ref().and_then(|device| {
                    device
                        .supported_output_configs()
                        .ok()
                        .and_then(|mut configs| {
                            configs
                                .max_by_key(|c| c.max_sample_rate().0)
                                .map(|c| c.max_sample_rate().0)
                        })
                });

                let supported_sample_rates = cpal_device.as_ref().and_then(get_supported_sample_rates);

                devices.push(AudioDevice {
                    id: hw_device_id.clone(),
                    name: hw_device_id.clone(),
                    description: Some(format!("{} (Direct Hardware - Bit-perfect)", card_desc)),
                    is_default: false,
                    max_sample_rate,
                    supported_sample_rates,
                    device_bus: None,
                    is_hardware: true,
                });
            }

            // Also create plughw:X,0 device (plugin hardware with auto-conversion)
            let plughw_device_id = format!("plughw:{},0", card_num);
            devices.push(AudioDevice {
                id: plughw_device_id.clone(),
                name: plughw_device_id.clone(),
                description: Some(format!("{} (Plugin Hardware)", card_desc)),
                is_default: false,
                max_sample_rate: Some(384000),
                supported_sample_rates: None, // plughw can convert, so all rates are "supported"
                device_bus: None,
                is_hardware: true,
            });
        }

        log::info!("[ALSA Backend] Enumerated {} ALSA devices", devices.len());
        for (idx, dev) in devices.iter().enumerate() {
            log::info!(
                "  [{}] {} - {} (max_rate: {:?})",
                idx,
                dev.name,
                dev.description.as_deref().unwrap_or("No description"),
                dev.max_sample_rate
            );
        }

        Ok(devices)
    }

    /// Enumerate ALSA devices via CPAL (fallback - no descriptions)
    fn enumerate_via_cpal(&self) -> BackendResult<Vec<AudioDevice>> {
        let mut devices = Vec::new();

        // Get all output devices from ALSA host
        let output_devices = self.host
            .output_devices()
            .map_err(|e| format!("Failed to enumerate ALSA devices: {}", e))?;

        for (idx, device) in output_devices.enumerate() {
            let name = device.name().unwrap_or_else(|_| format!("ALSA Device {}", idx));

            // Skip non-useful devices
            // Keep hw: and plughw: devices - these are bit-perfect
            if name == "null"
                || name.starts_with("lavrate")
                || name.starts_with("samplerate")
                || name.starts_with("speexrate")
                || name == "jack"
                || name == "oss"
                || name == "speex"
                || name == "upmix"
                || name == "vdownmix"
                || name.starts_with("surround")  // Skip surround variants
                || name.starts_with("usbstream")  // Skip USB stream
                || name == "pipewire"
                || name == "pulse"
                || name == "sysdefault"  // Skip bare sysdefault
            {
                continue;
            }

            // ID is the device name
            let id = name.clone();

            // Check if this is the default device
            let is_default = self.host
                .default_output_device()
                .and_then(|d| d.name().ok())
                .map(|default_name| default_name == name)
                .unwrap_or(false);

            // Try to get max sample rate from supported configs
            let max_sample_rate = device
                .supported_output_configs()
                .ok()
                .and_then(|mut configs| {
                    configs
                        .max_by_key(|c| c.max_sample_rate().0)
                        .map(|c| c.max_sample_rate().0)
                });

            // Get supported sample rates
            let supported_sample_rates = get_supported_sample_rates(&device);

            devices.push(AudioDevice {
                id: id.clone(),
                name: name.clone(),
                description: None,  // CPAL doesn't provide descriptions
                is_default,
                max_sample_rate,
                supported_sample_rates,
                device_bus: None,
                is_hardware: true,
            });
        }

        log::info!("[ALSA Backend] Enumerated {} devices via CPAL (fallback)", devices.len());
        for (idx, dev) in devices.iter().enumerate() {
            log::info!("  [{}] {} (max_rate: {:?})", idx, dev.name, dev.max_sample_rate);
        }

        Ok(devices)
    }

    /// Enumerate ALSA devices with fallback
    fn enumerate_alsa_devices(&self) -> BackendResult<Vec<AudioDevice>> {
        // Try aplay -L first (preferred - has real hardware descriptions)
        match self.enumerate_via_aplay() {
            Ok(devices) => Ok(devices),
            Err(e) => {
                log::warn!(
                    "[ALSA Backend] aplay -L failed: {}. Falling back to CPAL enumeration (no descriptions). \
                    Install alsa-utils package for better device names.",
                    e
                );
                self.enumerate_via_cpal()
            }
        }
    }

    /// Try to create direct ALSA stream for hw: devices (bypasses CPAL)
    /// Returns None if device is not a hw: device (should use CPAL instead)
    pub fn try_create_direct_stream(
        &self,
        config: &BackendConfig,
    ) -> Option<Result<super::AlsaDirectStream, String>> {
        let device_id = config.device_id.as_ref()?;

        // Only use direct ALSA for hw: and plughw: devices
        if !super::AlsaDirectStream::is_hw_device(device_id) {
            log::info!("[ALSA Backend] Device '{}' is not hw:/plughw:, using CPAL", device_id);
            return None;
        }

        log::info!(
            "[ALSA Backend] Creating DIRECT ALSA stream for hw: device: {} ({}Hz, {}ch)",
            device_id,
            config.sample_rate,
            config.channels
        );

        // Create direct ALSA stream (bypasses rodio/CPAL)
        Some(super::AlsaDirectStream::new(
            device_id,
            config.sample_rate,
            config.channels,
        ))
    }
}

impl AudioBackend for AlsaBackend {
    fn backend_type(&self) -> AudioBackendType {
        AudioBackendType::Alsa
    }

    fn enumerate_devices(&self) -> BackendResult<Vec<AudioDevice>> {
        self.enumerate_alsa_devices()
    }

    fn create_output_stream(
        &self,
        config: &BackendConfig,
    ) -> BackendResult<(OutputStream, OutputStreamHandle)> {
        log::info!(
            "[ALSA Backend] Creating stream: {}Hz, {} channels, exclusive: {}, plugin: {:?}",
            config.sample_rate,
            config.channels,
            config.exclusive_mode,
            config.alsa_plugin
        );

        // Find the device by name/id
        let device = if let Some(device_id) = &config.device_id {
            log::info!("[ALSA Backend] Looking for device: {}", device_id);
            self.host
                .output_devices()
                .map_err(|e| format!("Failed to enumerate devices: {}", e))?
                .find(|d| {
                    d.name()
                        .ok()
                        .map(|n| n == *device_id)
                        .unwrap_or(false)
                })
                .ok_or_else(|| format!("Device '{}' not found", device_id))?
        } else {
            log::info!("[ALSA Backend] Using default device");
            self.host
                .default_output_device()
                .ok_or("No default ALSA device available")?
        };

        let device_name = device.name().unwrap_or_else(|_| "unknown".to_string());
        log::info!("[ALSA Backend] Using device: {}", device_name);

        // Create StreamConfig with requested sample rate
        let stream_config = StreamConfig {
            channels: config.channels,
            sample_rate: SampleRate(config.sample_rate),
            buffer_size: if config.exclusive_mode {
                // Smaller buffer for exclusive mode = lower latency
                BufferSize::Fixed(512)
            } else {
                BufferSize::Default
            },
        };

        // Check if device supports this configuration
        let supported_configs = device
            .supported_output_configs()
            .map_err(|e| format!("Failed to get supported configs: {}", e))?;

        let mut found_matching = false;
        for range in supported_configs {
            if range.channels() == config.channels
                && config.sample_rate >= range.min_sample_rate().0
                && config.sample_rate <= range.max_sample_rate().0
            {
                found_matching = true;
                log::info!(
                    "[ALSA Backend] Device supports {}Hz (range: {}-{}Hz)",
                    config.sample_rate,
                    range.min_sample_rate().0,
                    range.max_sample_rate().0
                );
                break;
            }
        }

        if !found_matching {
            log::warn!(
                "[ALSA Backend] Device may not support {}Hz, attempting anyway",
                config.sample_rate
            );
        }

        // Create SupportedStreamConfig
        let supported_config = SupportedStreamConfig::new(
            stream_config.channels,
            stream_config.sample_rate,
            SupportedBufferSize::Range { min: 64, max: 8192 },
            SampleFormat::F32,
        );

        // Create OutputStream with custom config
        let stream = OutputStream::try_from_device_config(&device, supported_config)
            .map_err(|e| {
                if config.exclusive_mode {
                    format!(
                        "Failed to create exclusive ALSA stream at {}Hz: {}. Device may be in use by another application.",
                        config.sample_rate, e
                    )
                } else {
                    format!("Failed to create ALSA stream at {}Hz: {}", config.sample_rate, e)
                }
            })?;

        log::info!(
            "[ALSA Backend] Output stream created successfully at {}Hz (exclusive: {})",
            config.sample_rate,
            config.exclusive_mode
        );

        Ok(stream)
    }

    fn is_available(&self) -> bool {
        // Check if we can enumerate devices (ALSA is working)
        self.host.output_devices().is_ok()
    }

    fn description(&self) -> &'static str {
        "ALSA Direct - Bit-perfect with optional exclusive hardware access"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
