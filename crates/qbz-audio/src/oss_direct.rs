//! Direct OSS audio access for FreeBSD
//!
//! Provides bit-perfect playback by writing directly to /dev/dspX without
//! any mixing layer. Equivalent to the ALSA Direct path on Linux.

use std::fs::{File, OpenOptions};
use std::os::unix::io::AsRawFd;

use crate::backend::DirectAudioStream;

// ---------------------------------------------------------------------------
// FreeBSD OSS ioctl constants  (from /usr/include/sys/soundcard.h)
//
// _IO('P', n)  → IOC_VOID | ('P' << 8) | n  = 0x20000000 | 0x5000 | n
// _IOWR('P', n, int) → IOC_INOUT | (sizeof(int) << 16) | ('P' << 8) | n
//                    = 0xC0000000 | 0x00040000 | 0x00005000 | n
// ---------------------------------------------------------------------------
const SNDCTL_DSP_RESET: libc::c_ulong = 0x20005000; // _IO('P', 0)
const SNDCTL_DSP_SYNC: libc::c_ulong = 0x20005001;  // _IO('P', 1)
const SNDCTL_DSP_SPEED: libc::c_ulong = 0xC004_5002; // _IOWR('P', 2, int)
const SNDCTL_DSP_SETFMT: libc::c_ulong = 0xC004_5005; // _IOWR('P', 5, int)
const SNDCTL_DSP_CHANNELS: libc::c_ulong = 0xC004_5006; // _IOWR('P', 6, int)

// Audio sample format constants (OSS4, same on FreeBSD and Linux)
const AFMT_S16_LE: libc::c_int = 0x0000_0010;
const AFMT_S24_LE: libc::c_int = 0x0000_0400; // 24-bit in 32-bit container
const AFMT_S32_LE: libc::c_int = 0x0000_1000;

#[derive(Debug, Clone, Copy, PartialEq)]
enum OssFormat {
    S32LE,
    /// 24-bit signed integer stored right-aligned in a 32-bit LE word.
    S24LE,
    S16LE,
}

/// Direct OSS PCM stream for /dev/dspX devices.
///
/// Opens the device exclusively, negotiates the best available sample format,
/// and writes f32 audio data converted to the negotiated format.
pub struct OssDirectStream {
    file: File,
    sample_rate: u32,
    channels: u16,
    format: OssFormat,
    device_path: String,
}

impl OssDirectStream {
    /// Open an OSS device and configure it for bit-perfect playback.
    pub fn new(device_path: &str, sample_rate: u32, channels: u16) -> Result<Self, String> {
        log::info!(
            "[OSS Direct] Opening device: {} ({}Hz, {}ch)",
            device_path,
            sample_rate,
            channels
        );

        let file = OpenOptions::new()
            .write(true)
            .open(device_path)
            .map_err(|e| format!("Failed to open OSS device '{}': {}", device_path, e))?;

        let fd = file.as_raw_fd();

        // Negotiate the best available PCM format
        let format = Self::negotiate_format(fd)?;
        log::info!("[OSS Direct] Selected format: {:?}", format);

        // Set channel count (ioctl reads back the actual value)
        let mut ch = channels as libc::c_int;
        let ret = unsafe { libc::ioctl(fd, SNDCTL_DSP_CHANNELS, &mut ch) };
        if ret < 0 || ch != channels as libc::c_int {
            return Err(format!(
                "[OSS Direct] Failed to set {} channels (device returned {}): {}",
                channels,
                ch,
                std::io::Error::last_os_error()
            ));
        }

        // Set sample rate (device may choose a nearby supported rate)
        let mut rate = sample_rate as libc::c_int;
        let ret = unsafe { libc::ioctl(fd, SNDCTL_DSP_SPEED, &mut rate) };
        if ret < 0 {
            return Err(format!(
                "[OSS Direct] Failed to set sample rate {}Hz: {}",
                sample_rate,
                std::io::Error::last_os_error()
            ));
        }
        if (rate as u32) != sample_rate {
            log::warn!(
                "[OSS Direct] Requested {}Hz but device settled on {}Hz",
                sample_rate,
                rate
            );
        }

        log::info!(
            "[OSS Direct] Configured: {}Hz, {}ch, {:?}",
            rate,
            ch,
            format
        );

        Ok(Self {
            file,
            sample_rate: rate as u32,
            channels: ch as u16,
            format,
            device_path: device_path.to_string(),
        })
    }

    /// Try formats from highest to lowest bit depth.
    fn negotiate_format(fd: libc::c_int) -> Result<OssFormat, String> {
        let candidates: &[(libc::c_int, OssFormat, &str)] = &[
            (AFMT_S32_LE, OssFormat::S32LE, "S32LE"),
            (AFMT_S24_LE, OssFormat::S24LE, "S24LE (24-bit in 32-bit)"),
            (AFMT_S16_LE, OssFormat::S16LE, "S16LE"),
        ];

        for &(afmt, fmt, label) in candidates {
            let mut req = afmt;
            let ret = unsafe { libc::ioctl(fd, SNDCTL_DSP_SETFMT, &mut req) };
            if ret >= 0 && req == afmt {
                log::info!("[OSS Direct] Format negotiated: {}", label);
                return Ok(fmt);
            }
        }

        Err("No supported OSS audio format found (tried S32LE, S24LE, S16LE)".to_string())
    }

    /// Write a raw byte buffer to the device fd, looping until all bytes sent.
    fn write_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        let fd = self.file.as_raw_fd();
        let mut offset = 0usize;
        while offset < bytes.len() {
            let ret = unsafe {
                libc::write(
                    fd,
                    bytes[offset..].as_ptr() as *const libc::c_void,
                    bytes.len() - offset,
                )
            };
            if ret < 0 {
                return Err(format!(
                    "[OSS Direct] Write failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            offset += ret as usize;
        }
        Ok(())
    }

    /// Check if a device identifier looks like an OSS device path.
    pub fn is_oss_device(device_id: &str) -> bool {
        device_id.starts_with("/dev/dsp")
    }
}

impl DirectAudioStream for OssDirectStream {
    fn write_f32(&self, samples: &[f32]) -> Result<(), String> {
        match self.format {
            OssFormat::S32LE => {
                // f32 [-1, 1] → i32 full range, 4 bytes/sample
                let out: Vec<i32> = samples
                    .iter()
                    .map(|&s| (s.clamp(-1.0, 1.0) * 2_147_483_647.0) as i32)
                    .collect();
                let bytes = unsafe {
                    std::slice::from_raw_parts(out.as_ptr() as *const u8, out.len() * 4)
                };
                self.write_bytes(bytes)
            }
            OssFormat::S24LE => {
                // f32 → 24-bit signed integer, right-aligned in 32-bit LE word
                // (AFMT_S24_LE: value occupies bits 0-23, sign-extended to 32 bits)
                let out: Vec<i32> = samples
                    .iter()
                    .map(|&s| (s.clamp(-1.0, 1.0) * 8_388_607.0) as i32)
                    .collect();
                let bytes = unsafe {
                    std::slice::from_raw_parts(out.as_ptr() as *const u8, out.len() * 4)
                };
                self.write_bytes(bytes)
            }
            OssFormat::S16LE => {
                // f32 → i16, 2 bytes/sample
                let out: Vec<i16> = samples
                    .iter()
                    .map(|&s| (s.clamp(-1.0, 1.0) * 32_767.0) as i16)
                    .collect();
                let bytes = unsafe {
                    std::slice::from_raw_parts(out.as_ptr() as *const u8, out.len() * 2)
                };
                self.write_bytes(bytes)
            }
        }
    }

    fn drain(&self) -> Result<(), String> {
        // SNDCTL_DSP_SYNC blocks until the hardware has played all buffered data
        let ret = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                SNDCTL_DSP_SYNC,
                std::ptr::null_mut::<libc::c_int>(),
            )
        };
        if ret < 0 {
            Err(format!(
                "[OSS Direct] Drain (SNDCTL_DSP_SYNC) failed: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(())
        }
    }

    fn stop(&self) -> Result<(), String> {
        // SNDCTL_DSP_RESET immediately clears kernel and hardware buffers
        let ret = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                SNDCTL_DSP_RESET,
                std::ptr::null_mut::<libc::c_int>(),
            )
        };
        if ret < 0 {
            log::warn!(
                "[OSS Direct] Reset (SNDCTL_DSP_RESET) failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn device_id(&self) -> &str {
        &self.device_path
    }
    // set_hardware_volume: uses trait default (returns error) — OSS USB DACs
    // typically have no software mixer control accessible via ioctl.
}

/// Enumerate available OSS audio devices by scanning /dev/dsp* and /dev/sndstat.
pub fn enumerate_oss_devices() -> Vec<crate::backend::AudioDevice> {
    let mut devices = Vec::new();

    // Try to parse /dev/sndstat for device names
    let sndstat = std::fs::read_to_string("/dev/sndstat").ok();
    let device_names: std::collections::HashMap<u32, String> = sndstat
        .as_deref()
        .map(|content| {
            content
                .lines()
                .filter_map(|line| {
                    // Format: "pcm0: <device name> ..."
                    if line.starts_with("pcm") {
                        let rest = line.strip_prefix("pcm")?;
                        let (num_str, remainder) = rest.split_once(':')?;
                        let num = num_str.parse::<u32>().ok()?;
                        let name = remainder
                            .trim()
                            .strip_prefix('<')
                            .and_then(|s| s.split_once('>'))
                            .map(|(name, _)| name.to_string())
                            .unwrap_or_else(|| remainder.trim().to_string());
                        Some((num, name))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Scan /dev/dsp* devices
    if let Ok(entries) = std::fs::read_dir("/dev") {
        let mut dsp_paths: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("dsp") && name != "dsp" {
                    Some(format!("/dev/{}", name))
                } else {
                    None
                }
            })
            .collect();
        dsp_paths.sort();

        // Always include /dev/dsp0 even if not explicitly listed
        if dsp_paths.is_empty() && std::path::Path::new("/dev/dsp0").exists() {
            dsp_paths.push("/dev/dsp0".to_string());
        }

        for path in dsp_paths {
            let num = path
                .strip_prefix("/dev/dsp")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            let name = device_names
                .get(&num)
                .cloned()
                .unwrap_or_else(|| format!("OSS Device {}", num));

            devices.push(crate::backend::AudioDevice {
                id: path.clone(),
                name: format!("{} ({})", name, path),
                is_default: num == 0,
            });
        }
    }

    // If nothing found, add a default entry
    if devices.is_empty() {
        devices.push(crate::backend::AudioDevice {
            id: "/dev/dsp0".to_string(),
            name: "Default OSS Device (/dev/dsp0)".to_string(),
            is_default: true,
        });
    }

    devices
}
