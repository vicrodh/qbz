//! PipeWire audio backend
//!
//! Uses PipeWire/PulseAudio for audio output with device selection.
//! - Enumerates devices using pactl (pretty names)
//! - Sets PULSE_SINK environment variable for device routing
//! - Creates stream using CPAL "pulse" or "pipewire" device
//! - Does NOT change system default (only affects QBZ)

use super::backend::{AudioBackend, AudioBackendType, AudioDevice, BackendConfig, BackendResult};
use rodio::{
    cpal::{
        traits::{DeviceTrait, HostTrait},
        BufferSize, SampleFormat, SampleRate, StreamConfig, SupportedBufferSize, SupportedStreamConfig,
    },
    DeviceSinkBuilder, MixerDeviceSink,
};
use std::process::Command;

pub struct PipeWireBackend {
    #[allow(dead_code)]
    host: rodio::cpal::Host,
}

/// Calculate the optimal PipeWire quantum for a given sample rate.
/// Returns a power-of-2 value matching common audio interface expectations.
fn quantum_for_sample_rate(sample_rate: u32) -> u32 {
    match sample_rate {
        r if r <= 48000 => 1024,
        r if r <= 96000 => 2048,
        r if r <= 192000 => 4096,
        _ => 8192,
    }
}

impl PipeWireBackend {
    pub fn new() -> BackendResult<Self> {
        Ok(Self {
            host: rodio::cpal::default_host(),
        })
    }

    /// Reset PipeWire clock.force-rate and clock.force-quantum to 0.
    /// Call this when playback stops so other apps aren't stuck at a forced rate.
    pub fn reset_pipewire_clock() {
        log::info!("[PipeWire Backend] Resetting clock.force-rate and clock.force-quantum to 0");
        let _ = Command::new("pw-metadata")
            .args(["-n", "settings", "0", "clock.force-rate", "0"])
            .output();
        let _ = Command::new("pw-metadata")
            .args(["-n", "settings", "0", "clock.force-quantum", "0"])
            .output();
    }

    /// Parse pactl output to get device list with pretty names
    fn enumerate_pipewire_sinks(&self) -> BackendResult<Vec<AudioDevice>> {
        // Get default sink
        let default_sink = Command::new("pactl")
            .args(["get-default-sink"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
                } else {
                    None
                }
            });

        // Get all sinks with details
        let output = Command::new("pactl")
            .args(["list", "sinks"])
            .output()
            .map_err(|e| format!("Failed to run pactl: {}", e))?;

        if !output.status.success() {
            return Err("pactl command failed".to_string());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut devices = Vec::new();

        // Parse pactl output
        let mut current_name: Option<String> = None;
        let mut current_description: Option<String> = None;
        let mut current_max_rate: Option<u32> = None;
        let mut current_is_hardware: bool = false;
        let mut current_device_bus: Option<String> = None;

        for line in stdout.lines() {
            let line = line.trim();

            if line.starts_with("Sink #") {
                // Save previous device if complete
                if let (Some(id), Some(name)) = (current_name.take(), current_description.take()) {
                    let is_default = default_sink.as_ref().map(|d| d == &id).unwrap_or(false);
                    devices.push(AudioDevice {
                        id: id.clone(),
                        name,
                        description: None,
                        is_default,
                        max_sample_rate: current_max_rate.take(),
                        supported_sample_rates: None, // PipeWire handles sample rate conversion
                        device_bus: current_device_bus.take(),
                        is_hardware: current_is_hardware,
                    });
                }
                current_max_rate = None;
                current_is_hardware = false;
                current_device_bus = None;
            } else if line.starts_with("Name:") {
                current_name = Some(line.trim_start_matches("Name:").trim().to_string());
            } else if line.starts_with("Description:") {
                current_description = Some(line.trim_start_matches("Description:").trim().to_string());
            } else if line.starts_with("Flags:") {
                // Check for HARDWARE flag
                current_is_hardware = line.contains("HARDWARE");
            } else if line.contains("Sample Specification:") {
                // Try to parse sample rate from lines like "Sample Specification: s32le 2ch 192000Hz"
                if let Some(hz_pos) = line.find("Hz") {
                    let before_hz = &line[..hz_pos];
                    if let Some(last_space) = before_hz.rfind(' ') {
                        if let Ok(rate) = before_hz[last_space + 1..].parse::<u32>() {
                            current_max_rate = Some(rate);
                        }
                    }
                }
            } else if line.starts_with("device.bus = ") {
                // Parse device.bus property (e.g., "usb", "pci", "bluetooth")
                let bus = line.trim_start_matches("device.bus = ").trim_matches('"').to_string();
                current_device_bus = Some(bus);
            }
        }

        // Don't forget the last device
        if let (Some(id), Some(name)) = (current_name, current_description) {
            let is_default = default_sink.as_ref().map(|d| d == &id).unwrap_or(false);
            devices.push(AudioDevice {
                id,
                name,
                description: None,
                is_default,
                max_sample_rate: current_max_rate,
                supported_sample_rates: None, // PipeWire handles sample rate conversion
                device_bus: current_device_bus,
                is_hardware: current_is_hardware,
            });
        }

        log::info!("[PipeWire Backend] Enumerated {} devices via pactl", devices.len());
        for (idx, dev) in devices.iter().enumerate() {
            log::info!("  [{}] {} (id: {}, bus: {:?}, hw: {})",
                idx, dev.name, dev.id, dev.device_bus, dev.is_hardware);
        }

        Ok(devices)
    }
}

impl AudioBackend for PipeWireBackend {
    fn backend_type(&self) -> AudioBackendType {
        AudioBackendType::PipeWire
    }

    fn enumerate_devices(&self) -> BackendResult<Vec<AudioDevice>> {
        self.enumerate_pipewire_sinks()
    }

    fn create_output_stream(
        &self,
        config: &BackendConfig,
    ) -> BackendResult<MixerDeviceSink> {
        let target_sink = config.device_id.clone();

        // Temporarily set default sink to target (if specified)
        // We DON'T restore it - let the user's system keep the selected device as default
        // This is actually the expected behavior: when you select a device, it becomes the default
        if let Some(sink_name) = &target_sink {
            log::info!("[PipeWire Backend] Setting default sink to: {}", sink_name);

            let set_result = Command::new("pactl")
                .args(["set-default-sink", sink_name])
                .output();

            match set_result {
                Ok(output) if output.status.success() => {
                    log::info!("[PipeWire Backend] Default sink set to {}", sink_name);
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    log::warn!("[PipeWire Backend] Failed to set default sink: {}", stderr);
                }
                Err(e) => {
                    log::warn!("[PipeWire Backend] Error executing pactl set-default-sink: {}", e);
                }
            }

            // Wait for PipeWire to process the default sink change
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        // Force PipeWire to use the requested sample rate (for bit-perfect playback)
        log::info!("[PipeWire Backend] Forcing sample rate to {}Hz via pw-metadata", config.sample_rate);
        let metadata_result = Command::new("pw-metadata")
            .args(["-n", "settings", "0", "clock.force-rate", &config.sample_rate.to_string()])
            .output();

        match metadata_result {
            Ok(output) if output.status.success() => {
                log::info!("[PipeWire Backend] Sample rate forced to {}Hz", config.sample_rate);
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::warn!("[PipeWire Backend] Failed to force sample rate: {}", stderr);
            }
            Err(e) => {
                log::warn!("[PipeWire Backend] Error executing pw-metadata: {}", e);
            }
        }

        // Force quantum if bit-perfect mode is enabled
        if config.pw_force_bitperfect {
            let quantum = quantum_for_sample_rate(config.sample_rate);
            log::info!("[PipeWire Backend] Forcing quantum to {} via pw-metadata (bit-perfect)", quantum);
            let quantum_result = Command::new("pw-metadata")
                .args(["-n", "settings", "0", "clock.force-quantum", &quantum.to_string()])
                .output();

            match quantum_result {
                Ok(output) if output.status.success() => {
                    log::info!("[PipeWire Backend] Quantum forced to {}", quantum);
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    log::warn!("[PipeWire Backend] Failed to force quantum: {}", stderr);
                }
                Err(e) => {
                    log::warn!("[PipeWire Backend] Error executing pw-metadata for quantum: {}", e);
                }
            }
        }

        // Wait for PipeWire to apply the sample rate change
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Create a NEW host (will use current default sink)
        log::info!("[PipeWire Backend] Creating fresh CPAL host...");
        let fresh_host = rodio::cpal::default_host();

        // Find the "pulse" or "pipewire" CPAL device
        let device = fresh_host
            .output_devices()
            .map_err(|e| format!("Failed to enumerate CPAL devices: {}", e))?
            .find(|d| {
                d.name()
                    .ok()
                    .map(|n| n == "pulse" || n == "pipewire")
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                "Could not find 'pulse' or 'pipewire' CPAL device. Is PulseAudio/PipeWire running?".to_string()
            })?;

        let device_name = device.name().unwrap_or_else(|_| "unknown".to_string());
        log::info!("[PipeWire Backend] Using CPAL device: {}", device_name);

        // Create output stream with custom sample rate configuration
        log::info!(
            "[PipeWire Backend] Creating stream: {}Hz, {} channels, exclusive: {}",
            config.sample_rate,
            config.channels,
            config.exclusive_mode
        );

        // Create StreamConfig with desired sample rate
        let stream_config = StreamConfig {
            channels: config.channels,
            sample_rate: config.sample_rate,
            buffer_size: if config.exclusive_mode {
                BufferSize::Fixed(512)  // Lower latency for exclusive mode
            } else {
                // ~100ms period for fewer CPU wakeups (matches previous vendored cpal tuning)
                BufferSize::Fixed(config.sample_rate / 10)
            },
        };

        // Check if device supports this configuration
        let supported_configs = device
            .supported_output_configs()
            .map_err(|e| format!("Failed to get supported configs: {}", e))?;

        let mut found_matching = false;
        for range in supported_configs {
            if range.channels() == config.channels
                && config.sample_rate >= range.min_sample_rate()
                && config.sample_rate <= range.max_sample_rate()
            {
                found_matching = true;
                log::info!(
                    "[PipeWire Backend] Device supports {}Hz (range: {}-{}Hz)",
                    config.sample_rate,
                    range.min_sample_rate(),
                    range.max_sample_rate()
                );
                break;
            }
        }

        if !found_matching {
            log::warn!(
                "[PipeWire Backend] Device may not support {}Hz, attempting anyway",
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

        // Create MixerDeviceSink with custom config
        let mixer_sink = DeviceSinkBuilder::from_device(device)
            .map_err(|e| format!("Failed to create device sink builder: {}", e))?
            .with_supported_config(&supported_config)
            .open_stream()
            .map_err(|e| format!("Failed to create output stream at {}Hz: {}", config.sample_rate, e))?;

        log::info!("[PipeWire Backend] Output stream created successfully at {}Hz", config.sample_rate);

        Ok(mixer_sink)
    }

    fn is_available(&self) -> bool {
        // Check if pactl is available (PipeWire/PulseAudio)
        Command::new("pactl")
            .arg("info")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn description(&self) -> &'static str {
        "PipeWire (Recommended) - Modern audio server with device sharing"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
