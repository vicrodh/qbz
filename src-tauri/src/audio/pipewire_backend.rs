//! PipeWire audio backend
//!
//! Uses PipeWire/PulseAudio for audio output with device selection.
//! - Enumerates devices using pactl (pretty names)
//! - Sets PULSE_SINK environment variable for device routing
//! - Creates stream using CPAL "pulse" or "pipewire" device
//! - Does NOT change system default (only affects QBZ)

use super::backend::{AlsaPlugin, AudioBackend, AudioBackendType, AudioDevice, BackendConfig, BackendResult};
use rodio::{cpal::traits::{DeviceTrait, HostTrait}, OutputStream, OutputStreamHandle};
use std::process::Command;

pub struct PipeWireBackend {
    host: rodio::cpal::Host,
}

impl PipeWireBackend {
    pub fn new() -> BackendResult<Self> {
        Ok(Self {
            host: rodio::cpal::default_host(),
        })
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
                    });
                }
                current_max_rate = None;
            } else if line.starts_with("Name:") {
                current_name = Some(line.trim_start_matches("Name:").trim().to_string());
            } else if line.starts_with("Description:") {
                current_description = Some(line.trim_start_matches("Description:").trim().to_string());
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
            });
        }

        log::info!("[PipeWire Backend] Enumerated {} devices via pactl", devices.len());
        for (idx, dev) in devices.iter().enumerate() {
            log::info!("  [{}] {} (id: {}, max_rate: {:?})",
                idx, dev.name, dev.id, dev.max_sample_rate);
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
    ) -> BackendResult<(OutputStream, OutputStreamHandle)> {
        // CRITICAL: Set PULSE_SINK environment variable BEFORE creating new host
        // This tells PulseAudio/PipeWire which sink to use for THIS process only
        if let Some(device_id) = &config.device_id {
            log::info!("[PipeWire Backend] Setting PULSE_SINK={}", device_id);
            std::env::set_var("PULSE_SINK", device_id);

            // Verify it was set correctly
            match std::env::var("PULSE_SINK") {
                Ok(val) => log::info!("[PipeWire Backend] ✓ PULSE_SINK confirmed: {}", val),
                Err(e) => log::error!("[PipeWire Backend] ✗ PULSE_SINK verification failed: {:?}", e),
            }
        } else {
            // Clear PULSE_SINK to use system default
            log::info!("[PipeWire Backend] Clearing PULSE_SINK (using system default)");
            std::env::remove_var("PULSE_SINK");
        }

        // CRITICAL: Create a NEW host AFTER setting PULSE_SINK
        // The host connects to PulseAudio/PipeWire when created and reads PULSE_SINK at that time
        log::info!("[PipeWire Backend] Creating fresh CPAL host to pick up PULSE_SINK...");
        let fresh_host = rodio::cpal::default_host();

        // Find the "pulse" or "pipewire" CPAL device from the fresh host
        // These are ALSA PCM devices that route to PulseAudio/PipeWire
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

        // Create output stream
        // CPAL will route to the sink specified in PULSE_SINK
        let stream = OutputStream::try_from_device(&device)
            .map_err(|e| format!("Failed to create output stream: {}", e))?;

        log::info!("[PipeWire Backend] ✓ Output stream created successfully");

        Ok(stream)
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
}
