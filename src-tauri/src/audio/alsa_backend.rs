//! ALSA audio backend (direct hardware access)
//!
//! Provides direct access to ALSA hardware devices for:
//! - True exclusive mode (blocks device for other apps)
//! - Bit-perfect playback (no resampling)
//! - Low-latency audio output
//!
//! Uses CPAL's ALSA host with specific device selection.
//! Device enumeration reads directly from /proc/asound (no alsa-utils dependency).

use super::backend::{AlsaPlugin, AudioBackend, AudioBackendType, AudioDevice, BackendConfig, BackendResult};
use rodio::{
    cpal::{
        traits::{DeviceTrait, HostTrait},
        BufferSize, SampleFormat, SampleRate, StreamConfig, SupportedBufferSize, SupportedStreamConfig,
    },
    OutputStream, OutputStreamHandle,
};
use std::collections::HashMap;
use std::fs;

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

// ============================================================================
// /proc/asound helpers - No aplay dependency
// ============================================================================

/// Information about an ALSA sound card read from /proc/asound
#[derive(Debug, Clone)]
struct ProcCardInfo {
    /// Card number (0, 1, 2, ...)
    number: String,
    /// Short name used in ALSA device IDs (e.g., "C20", "NVidia", "sofhdadsp")
    short_name: String,
    /// Long descriptive name (e.g., "Cambridge Audio USB Audio 2.0")
    long_name: String,
    /// PCM playback devices on this card
    pcm_playback_devices: Vec<ProcPcmInfo>,
}

/// Information about a PCM device
#[derive(Debug, Clone)]
struct ProcPcmInfo {
    /// Device number within the card
    device_num: String,
    /// Device name (e.g., "USB Audio", "HDMI 0")
    name: String,
}

/// Read all sound card information from /proc/asound
fn read_proc_asound_cards() -> Vec<ProcCardInfo> {
    let mut cards = Vec::new();

    // Parse /proc/asound/cards for basic card info
    // Format: " 0 [C20            ]: USB-Audio - Cambridge Audio USB Audio 2.0"
    let cards_content = match fs::read_to_string("/proc/asound/cards") {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[ALSA] Cannot read /proc/asound/cards: {}", e);
            return cards;
        }
    };

    // Parse cards file - each card has two lines
    let lines: Vec<&str> = cards_content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        // First line format: " 0 [C20            ]: USB-Audio - Cambridge Audio USB Audio 2.0"
        if let Some(card_info) = parse_proc_card_line(line) {
            // Read PCM devices for this card
            let pcm_devices = read_card_pcm_devices(&card_info.0);

            cards.push(ProcCardInfo {
                number: card_info.0,
                short_name: card_info.1,
                long_name: card_info.2,
                pcm_playback_devices: pcm_devices,
            });
        }
        i += 1;
    }

    cards
}

/// Parse a line from /proc/asound/cards
/// Returns (card_number, short_name, long_name)
fn parse_proc_card_line(line: &str) -> Option<(String, String, String)> {
    // Format: " 0 [C20            ]: USB-Audio - Cambridge Audio USB Audio 2.0"
    let line = line.trim();

    // Find card number (first number)
    let parts: Vec<&str> = line.splitn(2, '[').collect();
    if parts.len() < 2 {
        return None;
    }

    let card_num = parts[0].trim().to_string();
    if card_num.is_empty() || !card_num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    // Find short name (inside brackets)
    let rest = parts[1];
    let bracket_end = rest.find(']')?;
    let short_name = rest[..bracket_end].trim().to_string();

    // Find long name (after " - ")
    let long_name = if let Some(dash_pos) = rest.find(" - ") {
        rest[dash_pos + 3..].trim().to_string()
    } else {
        // Fallback: use everything after ]:
        rest[bracket_end + 1..]
            .trim()
            .trim_start_matches(':')
            .trim()
            .split(" - ")
            .last()
            .unwrap_or(&short_name)
            .to_string()
    };

    Some((card_num, short_name, long_name))
}

/// Read PCM playback devices for a specific card from /proc/asound
fn read_card_pcm_devices(card_num: &str) -> Vec<ProcPcmInfo> {
    let mut devices = Vec::new();
    let card_path = format!("/proc/asound/card{}", card_num);

    // Read PCM device info files
    if let Ok(entries) = fs::read_dir(&card_path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // PCM playback devices are named pcmXp (X = device number, p = playback)
            if name_str.starts_with("pcm") && name_str.ends_with('p') {
                let info_path = entry.path().join("info");
                if let Ok(content) = fs::read_to_string(&info_path) {
                    let mut pcm_name = String::new();
                    let mut device_num = String::new();

                    for line in content.lines() {
                        if let Some(val) = line.strip_prefix("name: ") {
                            pcm_name = val.trim().to_string();
                        }
                        if let Some(val) = line.strip_prefix("device: ") {
                            device_num = val.trim().to_string();
                        }
                    }

                    if !device_num.is_empty() {
                        devices.push(ProcPcmInfo {
                            device_num,
                            name: if pcm_name.is_empty() { "Unknown".to_string() } else { pcm_name },
                        });
                    }
                }
            }
        }
    }

    // Sort by device number
    devices.sort_by(|a, b| {
        a.device_num.parse::<u32>().unwrap_or(0)
            .cmp(&b.device_num.parse::<u32>().unwrap_or(0))
    });

    devices
}

/// Build a map of card_number -> (short_name, long_name) from /proc/asound
fn build_card_info_map() -> HashMap<String, (String, String)> {
    let cards = read_proc_asound_cards();
    let mut map = HashMap::new();

    for card in cards {
        map.insert(card.number.clone(), (card.short_name, card.long_name));
    }

    map
}

/// Find card number by short name (e.g., "C20" -> "0")
fn find_card_number_by_name(short_name: &str) -> Option<String> {
    let cards = read_proc_asound_cards();
    cards.iter()
        .find(|c| c.short_name == short_name)
        .map(|c| c.number.clone())
}

// ============================================================================
// ALSA Backend Implementation
// ============================================================================

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

    /// Enumerate ALSA devices using /proc/asound for descriptions
    fn enumerate_with_proc_descriptions(&self) -> BackendResult<Vec<AudioDevice>> {
        // First: Get devices from CPAL (these are the device IDs that actually work)
        let mut devices = self.enumerate_via_cpal()?;

        // Second: Read card info from /proc/asound
        let cards = read_proc_asound_cards();

        // Build description map: short_name -> long_name
        let mut card_descriptions: HashMap<String, String> = HashMap::new();
        let mut card_num_to_short_name: HashMap<String, String> = HashMap::new();

        for card in &cards {
            card_descriptions.insert(card.short_name.clone(), card.long_name.clone());
            card_num_to_short_name.insert(card.number.clone(), card.short_name.clone());
            log::debug!(
                "[ALSA Backend] Card {}: {} = {}",
                card.number, card.short_name, card.long_name
            );
        }

        // Third: Update device descriptions
        // Match by card name in device ID (e.g., "sysdefault:CARD=C20" contains "C20")
        for device in &mut devices {
            for (short_name, long_name) in &card_descriptions {
                if device.name.contains(short_name) {
                    device.description = Some(format!("{}, {}", long_name, device.name));
                    break;
                }
            }
        }

        // Fourth: Add front:CARD=X,DEV=Y devices for bit-perfect playback
        // NOTE: We intentionally do NOT add hw:X,Y and plughw:X,Y devices because:
        // - Their card numbers (X) are UNSTABLE and change between boots
        // - front:CARD=name,DEV=Y uses the card NAME which is stable
        // - Both formats are functionally equivalent for bit-perfect playback
        // This fixes issue #69 where saved device wouldn't be recognized after reboot
        // Some USB DACs don't expose hw: devices but have working front: devices
        for card in &cards {
            for pcm in &card.pcm_playback_devices {
                let front_device_id = format!("front:CARD={},DEV={}", card.short_name, pcm.device_num);

                // Check if we already have this device
                if devices.iter().any(|d| d.name == front_device_id) {
                    continue;
                }

                log::info!(
                    "[ALSA Backend] Adding front: device for bit-perfect: {} ({})",
                    front_device_id,
                    card.long_name
                );

                devices.push(AudioDevice {
                    id: front_device_id.clone(),
                    name: front_device_id.clone(),
                    description: Some(format!("{} - {} (Direct Hardware - Bit-perfect)", card.long_name, pcm.name)),
                    is_default: false,
                    max_sample_rate: Some(384000),
                    supported_sample_rates: None,
                    device_bus: None,
                    is_hardware: true,
                });
            }
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

    /// Enumerate ALSA devices via CPAL (basic enumeration, no descriptions)
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
                .and_then(|configs| {
                    configs
                        .max_by_key(|c| c.max_sample_rate().0)
                        .map(|c| c.max_sample_rate().0)
                });

            // Get supported sample rates
            let supported_sample_rates = get_supported_sample_rates(&device);

            devices.push(AudioDevice {
                id: id.clone(),
                name: name.clone(),
                description: None,  // Will be filled by enumerate_with_proc_descriptions
                is_default,
                max_sample_rate,
                supported_sample_rates,
                device_bus: None,
                is_hardware: true,
            });
        }

        log::debug!("[ALSA Backend] CPAL enumerated {} base devices", devices.len());

        Ok(devices)
    }

    /// Try to create direct ALSA stream for hw: devices (bypasses CPAL)
    /// Returns None if device is not a hw: device (should use CPAL instead)
    ///
    /// Implements controlled fallback:
    /// 1. Try direct hw access first
    /// 2. If format unsupported, try plughw (format conversion only, no resampling)
    /// 3. Abort on other errors (busy, permissions, etc.)
    pub fn try_create_direct_stream(
        &self,
        config: &BackendConfig,
    ) -> Option<Result<(super::AlsaDirectStream, super::backend::BitPerfectMode), String>> {
        let device_id = config.device_id.as_ref()?;

        // Only use direct ALSA for hw:/plughw:/front: devices
        if !super::AlsaDirectStream::is_hw_device(device_id) {
            log::info!("[ALSA Backend] Device '{}' is not hw:/plughw:/front:, using CPAL", device_id);
            return None;
        }

        // Determine the base device path for hw/plughw attempts
        // front:CARD=X,DEV=Y -> extract card name for hw attempts
        let (hw_device, plughw_device) = if device_id.starts_with("front:CARD=") {
            // front:CARD=AMP,DEV=0 -> need to find corresponding hw:X,0
            // For now, try the front: device directly as it's already hardware-direct
            (device_id.to_string(), format!("plug:{}", device_id))
        } else if device_id.starts_with("hw:") {
            (device_id.to_string(), device_id.replace("hw:", "plughw:"))
        } else if device_id.starts_with("plughw:") {
            // Already plughw, try it directly
            (device_id.replace("plughw:", "hw:"), device_id.to_string())
        } else {
            (device_id.to_string(), format!("plug:{}", device_id))
        };

        // Respect ALSA plugin selection from settings
        let try_hw_first = match config.alsa_plugin {
            Some(AlsaPlugin::Hw) => true,
            Some(AlsaPlugin::PlugHw) => false, // Skip hw, go directly to plughw
            Some(AlsaPlugin::Pcm) => {
                log::info!("[ALSA Backend] PCM mode selected, not using direct ALSA");
                return None; // Use CPAL instead
            }
            None => true, // Default: try hw first
        };

        if try_hw_first {
            log::info!(
                "[ALSA Backend] Attempting DIRECT hw stream: {} ({}Hz, {}ch)",
                hw_device,
                config.sample_rate,
                config.channels
            );

            match super::AlsaDirectStream::new(&hw_device, config.sample_rate, config.channels) {
                Ok(stream) => {
                    log::info!("[ALSA Backend] ✓ Direct hw stream created successfully");
                    return Some(Ok((stream, super::backend::BitPerfectMode::DirectHardware)));
                }
                Err(e) => {
                    let error = super::backend::AlsaDirectError::from_alsa_error(&e);
                    log::warn!("[ALSA Backend] hw attempt failed: {}", error);

                    if !error.allows_plughw_fallback() {
                        // Non-recoverable error (busy, permissions, etc.)
                        log::error!("[ALSA Backend] Cannot fallback - error type: {:?}", error);
                        return Some(Err(format!(
                            "ALSA Direct failed: {}. Device may be in use or inaccessible.",
                            error
                        )));
                    }

                    log::info!("[ALSA Backend] Format unsupported on hw, trying plughw fallback...");
                }
            }
        }

        // Try plughw fallback (format conversion only)
        log::info!(
            "[ALSA Backend] Attempting plughw stream: {} ({}Hz, {}ch)",
            plughw_device,
            config.sample_rate,
            config.channels
        );

        match super::AlsaDirectStream::new(&plughw_device, config.sample_rate, config.channels) {
            Ok(stream) => {
                log::info!("[ALSA Backend] ✓ plughw stream created (bit-perfect with format conversion)");
                Some(Ok((stream, super::backend::BitPerfectMode::PluginFallback)))
            }
            Err(e) => {
                log::error!("[ALSA Backend] plughw fallback also failed: {}", e);
                Some(Err(format!(
                    "Bit-perfect playback could not be established. hw failed, plughw failed: {}",
                    e
                )))
            }
        }
    }
}

/// Convert an unstable hw:X,0 device ID to a stable front:CARD=name,DEV=0 format.
/// This survives reboots and USB reconnections since it uses the card name, not the number.
///
/// Examples:
/// - `hw:0,0` with card "C20" -> `front:CARD=C20,DEV=0`
/// - `hw:2,0` with card "NVidia" -> `front:CARD=NVidia,DEV=0`
/// - `front:CARD=C20,DEV=0` -> unchanged (already stable)
/// - `plughw:0,0` -> unchanged (plugin devices don't benefit from this)
/// - `default` -> unchanged (not a hardware device)
pub fn normalize_device_id_to_stable(device_id: &str) -> String {
    // Already stable formats - return as-is
    if device_id.starts_with("front:CARD=")
        || device_id.starts_with("plughw:")
        || !device_id.starts_with("hw:")
    {
        return device_id.to_string();
    }

    // Parse hw:X,Y format
    let stripped = device_id.strip_prefix("hw:").unwrap_or(device_id);
    let parts: Vec<&str> = stripped.split(',').collect();
    if parts.len() < 2 {
        log::warn!("[ALSA] Could not parse hw device format: {}", device_id);
        return device_id.to_string();
    }

    let card_num = parts[0];
    let device_num = parts[1];

    // Get card info from /proc/asound
    let card_map = build_card_info_map();

    if let Some((short_name, _long_name)) = card_map.get(card_num) {
        let stable_id = format!("front:CARD={},DEV={}", short_name, device_num);
        log::info!(
            "[ALSA] Normalized device ID: {} -> {} (stable)",
            device_id,
            stable_id
        );
        return stable_id;
    }

    log::warn!(
        "[ALSA] Could not find card {} in /proc/asound, keeping original ID",
        card_num
    );
    device_id.to_string()
}

/// Get the current card number for a stable device ID.
/// Used when we need to resolve front:CARD=X to hw:N,0 for certain operations.
///
/// Returns None if the card is not currently present.
pub fn resolve_stable_to_current_hw(device_id: &str) -> Option<String> {
    // Only resolve front:CARD= format
    if !device_id.starts_with("front:CARD=") {
        return Some(device_id.to_string());
    }

    // Extract card name: front:CARD=C20,DEV=0 -> C20
    let stripped = device_id.strip_prefix("front:CARD=")?;
    let parts: Vec<&str> = stripped.split(',').collect();
    let card_name = parts.first()?;
    let dev_part = parts.get(1).and_then(|s| s.strip_prefix("DEV=")).unwrap_or("0");

    // Find current card number for this name using /proc/asound
    if let Some(card_num) = find_card_number_by_name(card_name) {
        let hw_id = format!("hw:{},{}", card_num, dev_part);
        log::debug!("[ALSA] Resolved {} -> {}", device_id, hw_id);
        return Some(hw_id);
    }

    log::warn!("[ALSA] Card '{}' not found in current enumeration", card_name);
    None
}

impl AudioBackend for AlsaBackend {
    fn backend_type(&self) -> AudioBackendType {
        AudioBackendType::Alsa
    }

    fn enumerate_devices(&self) -> BackendResult<Vec<AudioDevice>> {
        self.enumerate_with_proc_descriptions()
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
