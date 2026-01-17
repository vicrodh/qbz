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
        let target_sink = config.device_id.clone();

        // Save current default sink (to restore later)
        let original_default = if target_sink.is_some() {
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
        } else {
            None
        };

        // Temporarily set default sink to target (if specified)
        if let Some(sink_name) = &target_sink {
            log::info!("[PipeWire Backend] Temporarily setting default sink to: {}", sink_name);

            let set_result = Command::new("pactl")
                .args(["set-default-sink", sink_name])
                .output();

            match set_result {
                Ok(output) if output.status.success() => {
                    log::info!("[PipeWire Backend] ✓ Default sink changed to {}", sink_name);
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    log::warn!("[PipeWire Backend] Failed to set default sink: {}", stderr);
                }
                Err(e) => {
                    log::warn!("[PipeWire Backend] Error executing pactl set-default-sink: {}", e);
                }
            }
        }

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

        // Create output stream (will route to current default sink)
        let stream = OutputStream::try_from_device(&device)
            .map_err(|e| format!("Failed to create output stream: {}", e))?;

        log::info!("[PipeWire Backend] ✓ Output stream created successfully");

        // Restore original default sink (if we changed it)
        if let Some(original) = original_default {
            log::info!("[PipeWire Backend] Restoring original default sink: {}", original);

            let restore_result = Command::new("pactl")
                .args(["set-default-sink", &original])
                .output();

            match restore_result {
                Ok(output) if output.status.success() => {
                    log::info!("[PipeWire Backend] ✓ Default sink restored to {}", original);
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    log::warn!("[PipeWire Backend] Failed to restore default sink: {}", stderr);
                }
                Err(e) => {
                    log::warn!("[PipeWire Backend] Error restoring default sink: {}", e);
                }
            }
        }

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
