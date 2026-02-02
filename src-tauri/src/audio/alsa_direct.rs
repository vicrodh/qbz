//! Direct ALSA access using alsa-rs
//!
//! Provides bit-perfect playback for hw:X,Y devices that CPAL cannot open.
//! This module bypasses rodio/CPAL completely for direct hardware access.

#[cfg(target_os = "linux")]
use alsa::pcm::{Access, Format, HwParams, PCM};
#[cfg(target_os = "linux")]
use alsa::{Direction, ValueOr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// Direct ALSA PCM stream for hw: devices
pub struct AlsaDirectStream {
    pcm: Arc<Mutex<PCM>>,
    is_playing: Arc<AtomicBool>,
    sample_rate: u32,
    channels: u16,
    format: Format,
    device_id: String,
}

impl AlsaDirectStream {
    /// Create new ALSA direct stream
    #[cfg(target_os = "linux")]
    pub fn new(device_id: &str, sample_rate: u32, channels: u16) -> Result<Self, String> {
        log::info!(
            "[ALSA Direct] Opening device: {} ({}Hz, {}ch)",
            device_id,
            sample_rate,
            channels
        );

        // Open PCM device
        let pcm = PCM::new(device_id, Direction::Playback, false)
            .map_err(|e| format!("Failed to open ALSA device '{}': {}", device_id, e))?;

        // Set hardware parameters and auto-detect best format
        let selected_format = {
            let hwp = HwParams::any(&pcm)
                .map_err(|e| format!("Failed to get hardware params: {}", e))?;

            // Set access type (interleaved)
            hwp.set_access(Access::RWInterleaved)
                .map_err(|e| format!("Failed to set access: {}", e))?;

            // Try formats in order of preference for bit-perfect playback
            // S24_3LE first: required by SMSL-class USB DACs (TAS1020B chip)
            // Then descending bit-depth for quality
            let format_priority = [
                (Format::S243LE, "S24_3LE"),  // 24-bit packed (SMSL, Topping, Fosi DACs)
                (Format::S32LE, "S32LE"),      // 32-bit
                (Format::S24LE, "S24LE"),      // 24-bit in 32-bit container
                (Format::S16LE, "S16LE"),      // 16-bit
                (Format::FloatLE, "Float32LE"), // Float (compatibility)
            ];

            let mut selected_format = None;
            for (format, name) in &format_priority {
                if hwp.set_format(*format).is_ok() {
                    log::info!("[ALSA Direct] Selected format: {}", name);
                    selected_format = Some(*format);
                    break;
                }
            }

            let format = selected_format
                .ok_or_else(|| "No supported audio format found (tried S24_3LE, S32LE, S24LE, S16LE, FloatLE)".to_string())?;

            // Set channels
            hwp.set_channels(channels as u32)
                .map_err(|e| format!("Failed to set channels: {}", e))?;

            // Set sample rate (exact match - bit-perfect!)
            hwp.set_rate(sample_rate, ValueOr::Nearest)
                .map_err(|e| format!("Failed to set sample rate: {}", e))?;

            // Set buffer size (larger buffer for high-res audio)
            let buffer_size = if sample_rate >= 192000 {
                // 500ms buffer for 192kHz+ (like MPD config)
                (sample_rate / 2) as i64
            } else if sample_rate >= 96000 {
                // 250ms buffer for 96kHz
                (sample_rate / 4) as i64
            } else {
                // 125ms buffer for lower rates
                (sample_rate / 8) as i64
            };

            hwp.set_buffer_size_near(buffer_size)
                .map_err(|e| format!("Failed to set buffer size: {}", e))?;

            // Set period size (1/10 of buffer)
            hwp.set_period_size_near(buffer_size / 10, ValueOr::Nearest)
                .map_err(|e| format!("Failed to set period size: {}", e))?;

            // Apply hardware parameters
            pcm.hw_params(&hwp)
                .map_err(|e| format!("Failed to apply hardware params: {}", e))?;

            log::info!("[ALSA Direct] Hardware configured: {}Hz, {}ch, buffer: {} frames, format: {:?}",
                sample_rate, channels, buffer_size, format);

            format
        };

        // Prepare device for playback
        pcm.prepare()
            .map_err(|e| format!("Failed to prepare PCM: {}", e))?;

        Ok(Self {
            pcm: Arc::new(Mutex::new(pcm)),
            is_playing: Arc::new(AtomicBool::new(false)),
            sample_rate,
            channels,
            format: selected_format,
            device_id: device_id.to_string(),
        })
    }

    /// Write audio samples to ALSA (auto-converts from i16 based on detected format)
    #[cfg(target_os = "linux")]
    pub fn write(&self, samples_i16: &[i16]) -> Result<(), String> {
        let mut pcm = self.pcm.lock().unwrap();
        let frames = samples_i16.len() / self.channels as usize;

        match self.format {
            Format::FloatLE => {
                // Convert i16 to f32
                let samples_f32: Vec<f32> = samples_i16
                    .iter()
                    .map(|&s| s as f32 / 32768.0)
                    .collect();

                let io = pcm.io_f32()
                    .map_err(|e| format!("Failed to get PCM I/O: {}", e))?;

                match io.writei(&samples_f32) {
                    Ok(written) => {
                        if written != frames {
                            log::warn!("[ALSA Direct] Partial write: {} / {} frames", written, frames);
                        }
                        Ok(())
                    }
                    Err(e) => {
                        if let Err(recover_err) = pcm.recover(e.errno() as i32, false) {
                            Err(format!("Failed to recover from error: {}", recover_err))
                        } else {
                            log::warn!("[ALSA Direct] Recovered from PCM error");
                            Ok(())
                        }
                    }
                }
            }
            Format::S32LE => {
                // Convert i16 to i32 (bit-perfect: shift left 16 bits)
                let samples_i32: Vec<i32> = samples_i16
                    .iter()
                    .map(|&s| (s as i32) << 16)
                    .collect();

                let io = pcm.io_i32()
                    .map_err(|e| format!("Failed to get PCM I/O: {}", e))?;

                match io.writei(&samples_i32) {
                    Ok(written) => {
                        if written != frames {
                            log::warn!("[ALSA Direct] Partial write: {} / {} frames", written, frames);
                        }
                        Ok(())
                    }
                    Err(e) => {
                        if let Err(recover_err) = pcm.recover(e.errno() as i32, false) {
                            Err(format!("Failed to recover from error: {}", recover_err))
                        } else {
                            log::warn!("[ALSA Direct] Recovered from PCM error");
                            Ok(())
                        }
                    }
                }
            }
            Format::S16LE => {
                // Direct write (no conversion needed)
                let io = pcm.io_i16()
                    .map_err(|e| format!("Failed to get PCM I/O: {}", e))?;

                match io.writei(samples_i16) {
                    Ok(written) => {
                        if written != frames {
                            log::warn!("[ALSA Direct] Partial write: {} / {} frames", written, frames);
                        }
                        Ok(())
                    }
                    Err(e) => {
                        if let Err(recover_err) = pcm.recover(e.errno() as i32, false) {
                            Err(format!("Failed to recover from error: {}", recover_err))
                        } else {
                            log::warn!("[ALSA Direct] Recovered from PCM error");
                            Ok(())
                        }
                    }
                }
            }
            Format::S243LE => {
                // S24_3LE: 24-bit packed in 3 bytes, little-endian
                // Required by SMSL-class USB DACs (TAS1020B chip)
                // Convert i16 → i24: shift left 8 bits, then pack into 3 bytes
                let mut bytes: Vec<u8> = Vec::with_capacity(samples_i16.len() * 3);

                for &sample in samples_i16 {
                    // Convert i16 to i24 (lossless: zeros in lower 8 bits)
                    let s24 = (sample as i32) << 8;
                    // Pack as 3 bytes in little-endian order
                    bytes.push((s24 & 0xFF) as u8);         // LSB
                    bytes.push(((s24 >> 8) & 0xFF) as u8);  // Middle
                    bytes.push(((s24 >> 16) & 0xFF) as u8); // MSB (sign-extended)
                }

                // Use raw byte I/O for 3-byte packed format
                let io = unsafe { pcm.io_bytes() };

                match io.writei(&bytes) {
                    Ok(written) => {
                        if written != frames {
                            log::warn!("[ALSA Direct] Partial write: {} / {} frames (S24_3LE)", written, frames);
                        }
                        Ok(())
                    }
                    Err(e) => {
                        if let Err(recover_err) = pcm.recover(e.errno() as i32, false) {
                            Err(format!("Failed to recover from error: {}", recover_err))
                        } else {
                            log::warn!("[ALSA Direct] Recovered from PCM error (S24_3LE)");
                            Ok(())
                        }
                    }
                }
            }
            Format::S24LE => {
                // S24LE: 24-bit in 32-bit container (padded)
                // Convert i16 → i32, shift left 16 bits (same as S32LE for i16 source)
                let samples_i32: Vec<i32> = samples_i16
                    .iter()
                    .map(|&s| (s as i32) << 16)
                    .collect();

                let io = pcm.io_i32()
                    .map_err(|e| format!("Failed to get PCM I/O: {}", e))?;

                match io.writei(&samples_i32) {
                    Ok(written) => {
                        if written != frames {
                            log::warn!("[ALSA Direct] Partial write: {} / {} frames", written, frames);
                        }
                        Ok(())
                    }
                    Err(e) => {
                        if let Err(recover_err) = pcm.recover(e.errno() as i32, false) {
                            Err(format!("Failed to recover from error: {}", recover_err))
                        } else {
                            log::warn!("[ALSA Direct] Recovered from PCM error");
                            Ok(())
                        }
                    }
                }
            }
            _ => {
                Err(format!("Unsupported format: {:?}", self.format))
            }
        }
    }

    /// Drain and stop playback
    #[cfg(target_os = "linux")]
    pub fn drain(&self) -> Result<(), String> {
        log::info!("[ALSA Direct] Draining PCM");
        let pcm = self.pcm.lock().unwrap();
        pcm.drain()
            .map_err(|e| format!("Failed to drain PCM: {}", e))
    }

    /// Stop PCM immediately (prepare for next playback)
    #[cfg(target_os = "linux")]
    pub fn stop(&self) -> Result<(), String> {
        log::info!("[ALSA Direct] Stopping PCM");
        let pcm = self.pcm.lock().unwrap();
        // PCM::drop() is called automatically when pcm goes out of scope
        // For now, just prepare for next playback
        pcm.prepare()
            .map_err(|e| format!("Failed to prepare PCM after stop: {}", e))
    }

    /// Get sample rate
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get channels
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Get device ID
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Try to set hardware volume via ALSA mixer
    ///
    /// Returns error if:
    /// - DAC doesn't have mixer controls (common for USB DACs)
    /// - Mixer API fails
    ///
    /// NOTE: Failure doesn't break playback, just means volume can't be controlled.
    #[cfg(target_os = "linux")]
    pub fn set_hardware_volume(&self, volume: f32) -> Result<(), String> {
        use alsa::mixer::{Mixer, SelemId};
        use alsa::mixer::SelemChannelId::*;

        // Open mixer for device
        let mixer = Mixer::new(&self.device_id, false)
            .map_err(|e| format!("Failed to open mixer for {}: {}", self.device_id, e))?;

        // Try to find a volume control element
        // Common names: "Master", "PCM", "Speaker", "Headphone"
        let control_names = ["Master", "PCM", "Speaker", "Headphone", "Digital"];

        for name in &control_names {
            let selem_id = SelemId::new(name, 0);

            if let Some(selem) = mixer.find_selem(&selem_id) {
                // Check if this element has playback volume control
                if selem.has_playback_volume() {
                    let (min, max) = selem.get_playback_volume_range();
                    let target = min + ((max - min) as f32 * volume) as i64;

                    log::info!("[ALSA Direct] Setting hardware volume via '{}': {:.0}% (raw: {}/{})",
                        name, volume * 100.0, target, max);

                    // Set volume on all channels
                    for channel in &[FrontLeft, FrontRight, FrontCenter, RearLeft, RearRight] {
                        let _ = selem.set_playback_volume(*channel, target);
                    }

                    return Ok(());
                }
            }
        }

        Err(format!("No volume control found for {}. DAC may not support hardware mixer.", self.device_id))
    }

    /// Check if device is a bit-perfect hardware device
    /// Includes: hw:X,Y, plughw:X,Y, and front:CARD=X,DEV=Y
    pub fn is_hw_device(device_id: &str) -> bool {
        device_id.starts_with("hw:")
            || device_id.starts_with("plughw:")
            || device_id.starts_with("front:CARD=")
    }
}

#[cfg(not(target_os = "linux"))]
impl AlsaDirectStream {
    pub fn new(_device_id: &str, _sample_rate: u32, _channels: u16) -> Result<Self, String> {
        Err("ALSA Direct is only available on Linux".to_string())
    }

    pub fn write(&self, _samples: &[f32]) -> Result<(), String> {
        Err("ALSA Direct is only available on Linux".to_string())
    }

    pub fn drain(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn sample_rate(&self) -> u32 {
        44100
    }

    pub fn channels(&self) -> u16 {
        2
    }

    /// Check if device is a bit-perfect hardware device (always false on non-Linux)
    pub fn is_hw_device(_device_id: &str) -> bool {
        false
    }
}
