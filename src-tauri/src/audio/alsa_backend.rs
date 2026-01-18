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

    /// Enumerate ALSA devices via aplay -L (preferred - has real descriptions)
    fn enumerate_via_aplay(&self) -> BackendResult<Vec<AudioDevice>> {
        let mut devices = Vec::new();

        // Run aplay -L to get device list with descriptions
        let output = Command::new("aplay")
            .arg("-L")
            .output()
            .map_err(|e| format!("Failed to run aplay -L: {}", e))?;

        if !output.status.success() {
            return Err("aplay -L command failed".to_string());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim_end();

            // Device ID lines don't start with whitespace
            if !line.starts_with(' ') && !line.is_empty() {
                let device_id = line.to_string();

                // Skip blacklisted devices
                // Only keep useful devices for audiophile playback:
                // - default, sysdefault:CARD= (system defaults)
                // - front:CARD= (stereo/front output - most common for DACs)
                // - hdmi:CARD= (HDMI outputs)
                // - iec958:CARD= (S/PDIF digital)
                // Skip surround variants, usbstream, plugins, etc.
                let is_blacklisted = device_id == "null"
                    || device_id.starts_with("lavrate")
                    || device_id.starts_with("samplerate")
                    || device_id.starts_with("speexrate")
                    || device_id == "jack"
                    || device_id == "oss"
                    || device_id == "speex"
                    || device_id == "upmix"
                    || device_id == "vdownmix"
                    || device_id.starts_with("surround")  // Skip surround21, surround40, surround51, etc.
                    || device_id.starts_with("usbstream:")  // Skip USB stream devices
                    || device_id == "pipewire"  // Skip - use PipeWire backend instead
                    || device_id == "pulse"  // Skip - use PulseAudio backend instead
                    || device_id == "sysdefault";  // Skip bare sysdefault (keep sysdefault:CARD= only)

                if is_blacklisted {
                    // Skip this device and its description lines
                    i += 1;
                    while i < lines.len() && lines[i].starts_with(' ') {
                        i += 1;
                    }
                    continue;
                }

                // Get description from next line (if it exists and is indented)
                let description = if i + 1 < lines.len() && lines[i + 1].starts_with(' ') {
                    Some(lines[i + 1].trim().to_string())
                } else {
                    None
                };

                // Check if this is the default device
                let is_default = device_id == "default" || device_id.starts_with("default:");

                // Try to get max sample rate by testing with CPAL
                let max_sample_rate = self.host
                    .output_devices()
                    .ok()
                    .and_then(|mut devs| {
                        devs.find(|d| d.name().ok().as_deref() == Some(&device_id))
                    })
                    .and_then(|device| {
                        device
                            .supported_output_configs()
                            .ok()
                            .and_then(|mut configs| {
                                configs
                                    .max_by_key(|c| c.max_sample_rate().0)
                                    .map(|c| c.max_sample_rate().0)
                            })
                    });

                devices.push(AudioDevice {
                    id: device_id.clone(),
                    name: device_id.clone(),
                    description,
                    is_default,
                    max_sample_rate,
                });

                // Skip description lines
                i += 1;
                while i < lines.len() && lines[i].starts_with(' ') {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        // Add hw: devices for bit-perfect playback (not listed by aplay -L)
        // Extract unique CARD names from devices we found
        let mut cards: Vec<String> = Vec::new();
        for device in &devices {
            if let Some(card_match) = device.name.strip_prefix("front:CARD=") {
                if let Some(card_name) = card_match.split(',').next() {
                    if !cards.contains(&card_name.to_string()) {
                        cards.push(card_name.to_string());
                    }
                }
            } else if let Some(card_match) = device.name.strip_prefix("sysdefault:CARD=") {
                if !cards.contains(&card_match.to_string()) {
                    cards.push(card_match.to_string());
                }
            }
        }

        // For each card, add hw:CARD=XXX,DEV=0 device (bit-perfect, no plugins)
        for card in &cards {
            let hw_device_id = format!("hw:CARD={},DEV=0", card);

            // Get max sample rate from CPAL
            let max_sample_rate = self.host
                .output_devices()
                .ok()
                .and_then(|mut devs| {
                    devs.find(|d| d.name().ok().as_deref() == Some(&hw_device_id))
                })
                .and_then(|device| {
                    device
                        .supported_output_configs()
                        .ok()
                        .and_then(|mut configs| {
                            configs
                                .max_by_key(|c| c.max_sample_rate().0)
                                .map(|c| c.max_sample_rate().0)
                        })
                });

            // Find the friendly description from front: or sysdefault: device
            let base_description = devices.iter()
                .find(|d| d.name.starts_with(&format!("front:CARD={}", card)) ||
                         d.name == format!("sysdefault:CARD={}", card))
                .and_then(|d| d.description.as_ref())
                .map(|desc| {
                    // Extract just the card name (first line before comma)
                    desc.split(',').next().unwrap_or(desc).trim().to_string()
                })
                .unwrap_or_else(|| card.clone());

            devices.push(AudioDevice {
                id: hw_device_id.clone(),
                name: hw_device_id.clone(),
                description: Some(format!("{} (Direct Hardware - Bit-perfect)", base_description)),
                is_default: false,
                max_sample_rate,
            });
        }

        log::info!("[ALSA Backend] Enumerated {} devices via aplay -L (+ {} hw: devices)", devices.len() - cards.len(), cards.len());
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
            if name == "null"
                || name.starts_with("lavrate")
                || name.starts_with("samplerate")
                || name.starts_with("speexrate")
                || name == "jack"
                || name == "oss"
                || name == "speex"
                || name == "upmix"
                || name == "vdownmix"
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

            devices.push(AudioDevice {
                id: id.clone(),
                name: name.clone(),
                description: None,  // CPAL doesn't provide descriptions
                is_default,
                max_sample_rate,
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
            "[ALSA Backend] ✓ Output stream created successfully at {}Hz (exclusive: {})",
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
}
