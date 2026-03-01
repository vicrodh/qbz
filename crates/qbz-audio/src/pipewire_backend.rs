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
        BufferSize, SampleFormat, StreamConfig, SupportedBufferSize, SupportedStreamConfig,
    },
    DeviceSinkBuilder, MixerDeviceSink,
};
use std::process::Command;

pub struct PipeWireBackend {
    #[allow(dead_code)]
    host: rodio::cpal::Host,
}

impl PipeWireBackend {
    pub fn new() -> BackendResult<Self> {
        Ok(Self {
            host: rodio::cpal::default_host(),
        })
    }

    /// Reset PipeWire clock.force-rate and clock.force-quantum to 0.
    /// Call this when playback stops so other apps aren't stuck at a forced rate.
    /// Quantum reset is kept for safety even though we no longer force it.
    pub fn reset_pipewire_clock() {
        log::info!("[PipeWire Backend] Resetting clock.force-rate and clock.force-quantum to 0");
        let _ = Command::new("pw-metadata")
            .args(["-n", "settings", "0", "clock.force-rate", "0"])
            .output();
        let _ = Command::new("pw-metadata")
            .args(["-n", "settings", "0", "clock.force-quantum", "0"])
            .output();
    }

    /// Get the ALSA card number for a PipeWire/PulseAudio sink name.
    /// Parses `pactl list sinks` to find the `alsa.card` property.
    fn get_alsa_card_for_sink(sink_name: &str) -> Option<String> {
        let output = Command::new("pactl")
            .args(["list", "sinks"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut in_target_sink = false;

        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Sink #") {
                if in_target_sink {
                    return None; // Passed target sink without finding alsa.card
                }
            } else if trimmed.starts_with("Name:") {
                let name = trimmed.trim_start_matches("Name:").trim();
                in_target_sink = name == sink_name;
            } else if in_target_sink && trimmed.starts_with("alsa.card = ") {
                let card = trimmed
                    .trim_start_matches("alsa.card = ")
                    .trim_matches('"')
                    .to_string();
                return Some(card);
            }
        }

        None
    }

    /// Query the DAC's supported sample rates from /proc/asound/cardN/stream0.
    /// Returns None if rates can't be determined (non-USB device, continuous range, etc.)
    fn get_sink_supported_rates(sink_name: &str) -> Option<Vec<u32>> {
        let alsa_card = Self::get_alsa_card_for_sink(sink_name)?;

        let stream_path = format!("/proc/asound/card{}/stream0", alsa_card);
        let content = std::fs::read_to_string(&stream_path).ok()?;

        // Collect all rates from Playback Rates: lines (handles multiple alt settings)
        let mut in_playback = false;
        let mut all_rates = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "Playback:" {
                in_playback = true;
            } else if trimmed == "Capture:" {
                in_playback = false;
            }
            if in_playback && trimmed.starts_with("Rates:") {
                let rates_str = trimmed.trim_start_matches("Rates:").trim();
                if rates_str.contains("continuous") {
                    return None; // Any rate in range is supported
                }
                for rate_str in rates_str.split(',') {
                    if let Ok(rate) = rate_str.trim().parse::<u32>() {
                        if !all_rates.contains(&rate) {
                            all_rates.push(rate);
                        }
                    }
                }
            }
        }

        if all_rates.is_empty() {
            None
        } else {
            all_rates.sort();
            Some(all_rates)
        }
    }

    /// Find the best fallback sample rate in the same family.
    /// 44.1kHz family: 44100, 88200, 176400, 352800
    /// 48kHz family: 48000, 96000, 192000, 384000
    fn find_best_fallback_rate(requested: u32, supported: &[u32]) -> u32 {
        let is_441_family = requested % 44100 == 0;

        // Find highest supported rate in the same family that's <= requested
        let mut candidates: Vec<u32> = supported
            .iter()
            .filter(|&&r| {
                if is_441_family {
                    r % 44100 == 0
                } else {
                    r % 48000 == 0
                }
            })
            .filter(|&&r| r <= requested)
            .copied()
            .collect();
        candidates.sort();

        if let Some(&best) = candidates.last() {
            return best;
        }

        // No rate in the same family — use highest supported rate overall
        supported.iter().copied().max().unwrap_or(48000)
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

        // Check if the DAC supports the requested sample rate.
        // Query via /proc/asound/ (USB DACs list discrete supported rates).
        // If unsupported, fall back to the nearest rate in the same family
        // (e.g., 176.4kHz → 88.2kHz). Rodio resamples from track rate to
        // stream rate automatically.
        let effective_sink = target_sink.clone().or_else(|| {
            Command::new("pactl")
                .args(["get-default-sink"])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
                    } else {
                        None
                    }
                })
        });

        let effective_rate = if let Some(ref sink_name) = effective_sink {
            match Self::get_sink_supported_rates(sink_name) {
                Some(rates) if rates.contains(&config.sample_rate) => {
                    log::info!(
                        "[PipeWire Backend] DAC supports {}Hz (available: {:?})",
                        config.sample_rate, rates
                    );
                    config.sample_rate
                }
                Some(rates) => {
                    let fallback = Self::find_best_fallback_rate(config.sample_rate, &rates);
                    log::warn!(
                        "[PipeWire Backend] DAC doesn't support {}Hz. Supported: {:?}. Falling back to {}Hz (resampled by rodio)",
                        config.sample_rate, rates, fallback
                    );
                    fallback
                }
                None => {
                    log::info!(
                        "[PipeWire Backend] Could not determine DAC supported rates, using {}Hz",
                        config.sample_rate
                    );
                    config.sample_rate
                }
            }
        } else {
            config.sample_rate
        };

        // Force PipeWire to use the effective sample rate (for bit-perfect playback)
        log::info!("[PipeWire Backend] Forcing sample rate to {}Hz via pw-metadata", effective_rate);
        let metadata_result = Command::new("pw-metadata")
            .args(["-n", "settings", "0", "clock.force-rate", &effective_rate.to_string()])
            .output();

        match metadata_result {
            Ok(output) if output.status.success() => {
                log::info!("[PipeWire Backend] Sample rate forced to {}Hz", effective_rate);
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::warn!("[PipeWire Backend] Failed to force sample rate: {}", stderr);
            }
            Err(e) => {
                log::warn!("[PipeWire Backend] Error executing pw-metadata: {}", e);
            }
        }

        // Note: clock.force-quantum is intentionally NOT set.
        // rodio 0.22's MixerDeviceSink has its own internal mixer thread that
        // cannot synchronize with PipeWire's forced quantum, causing massive
        // buffer underruns at sample rates >= 88.2kHz. clock.force-rate alone
        // is sufficient for bit-perfect sample rate switching.

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
                d.description()
                    .ok()
                    .map(|desc| {
                        let device_name = desc.name();
                        device_name == "pulse" || device_name == "pipewire"
                    })
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                "Could not find 'pulse' or 'pipewire' CPAL device. Is PulseAudio/PipeWire running?".to_string()
            })?;

        let device_name = device
            .description()
            .map(|desc| desc.name().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        log::info!("[PipeWire Backend] Using CPAL device: {}", device_name);

        // Create output stream with custom sample rate configuration
        log::info!(
            "[PipeWire Backend] Creating stream: {}Hz (track: {}Hz), {} channels, exclusive: {}",
            effective_rate,
            config.sample_rate,
            config.channels,
            config.exclusive_mode
        );

        // Create StreamConfig with effective sample rate
        // Note: buffer_size here is unused — with_supported_config() resets it.
        // The actual buffer size is set via with_buffer_size() below.
        let stream_config = StreamConfig {
            channels: config.channels,
            sample_rate: effective_rate,
            buffer_size: BufferSize::Default,
        };

        // Check if CPAL device supports this configuration
        let supported_configs = device
            .supported_output_configs()
            .map_err(|e| format!("Failed to get supported configs: {}", e))?;

        let mut found_matching = false;
        for range in supported_configs {
            if range.channels() == config.channels
                && effective_rate >= range.min_sample_rate()
                && effective_rate <= range.max_sample_rate()
            {
                found_matching = true;
                log::info!(
                    "[PipeWire Backend] CPAL device supports {}Hz (range: {}-{}Hz)",
                    effective_rate,
                    range.min_sample_rate(),
                    range.max_sample_rate()
                );
                break;
            }
        }

        if !found_matching {
            log::warn!(
                "[PipeWire Backend] CPAL device may not support {}Hz, attempting anyway",
                effective_rate
            );
        }

        // Create SupportedStreamConfig
        let supported_config = SupportedStreamConfig::new(
            stream_config.channels,
            stream_config.sample_rate,
            SupportedBufferSize::Range { min: 64, max: 8192 },
            SampleFormat::F32,
        );

        // Compute buffer size — must be applied AFTER with_supported_config()
        // because that method resets buffer_size to Default via ..Default::default().
        // MixerDeviceSink has zero internal buffering, so CPAL's buffer is the
        // ONLY buffer between the mixer and audio hardware.
        let cpal_buffer_size = if config.exclusive_mode {
            BufferSize::Fixed(512)  // Low latency for exclusive mode
        } else {
            // ~100ms buffer, matching old vendored cpal period size.
            // Prevents underruns at high sample rates (192kHz = 19200 frames).
            BufferSize::Fixed(effective_rate / 10)
        };
        log::info!("[PipeWire Backend] Buffer size: {:?}", cpal_buffer_size);

        // Create MixerDeviceSink with custom config
        let mixer_sink = DeviceSinkBuilder::from_device(device)
            .map_err(|e| format!("Failed to create device sink builder: {}", e))?
            .with_supported_config(&supported_config)
            .with_buffer_size(cpal_buffer_size)
            .open_stream()
            .map_err(|e| format!("Failed to create output stream at {}Hz: {}", effective_rate, e))?;

        log::info!("[PipeWire Backend] Output stream created successfully at {}Hz", effective_rate);

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
