//! Tauri commands for audio backend management

use crate::audio::{AlsaPlugin, AudioBackendType, AudioDevice, BackendManager};
use serde::{Deserialize, Serialize};

/// Backend information for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInfo {
    pub backend_type: AudioBackendType,
    pub name: String,
    pub description: String,
    pub is_available: bool,
}

/// ALSA plugin information for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlsaPluginInfo {
    pub plugin: AlsaPlugin,
    pub name: String,
    pub description: String,
}

/// Get list of available audio backends
#[tauri::command]
pub fn get_available_backends() -> Result<Vec<BackendInfo>, String> {
    log::info!("Command: get_available_backends");

    let backends = BackendManager::available_backends();
    let backend_infos: Vec<BackendInfo> = backends
        .into_iter()
        .map(|backend_type| {
            // Create backend instance to check availability and get description
            let backend = BackendManager::create_backend(backend_type);
            let (is_available, description) = match backend {
                Ok(b) => (b.is_available(), b.description().to_string()),
                Err(_) => (false, "Not available".to_string()),
            };

            let name = match backend_type {
                AudioBackendType::PipeWire => "PipeWire",
                AudioBackendType::Alsa => "ALSA Direct",
                AudioBackendType::Pulse => "PulseAudio",
            };

            BackendInfo {
                backend_type,
                name: name.to_string(),
                description,
                is_available,
            }
        })
        .collect();

    log::info!("Found {} backends", backend_infos.len());
    for info in &backend_infos {
        log::info!("  - {} (available: {})", info.name, info.is_available);
    }

    Ok(backend_infos)
}

/// Get list of devices for a specific backend
#[tauri::command]
pub fn get_devices_for_backend(backend_type: AudioBackendType) -> Result<Vec<AudioDevice>, String> {
    log::info!("Command: get_devices_for_backend({:?})", backend_type);

    let backend = BackendManager::create_backend(backend_type)?;
    let devices = backend.enumerate_devices()?;

    log::info!("Found {} devices for {:?} backend", devices.len(), backend_type);
    for (idx, device) in devices.iter().enumerate() {
        log::info!("  [{}] {} (id: {})", idx, device.name, device.id);
    }

    Ok(devices)
}

/// Get list of available ALSA plugins
#[tauri::command]
pub fn get_alsa_plugins() -> Result<Vec<AlsaPluginInfo>, String> {
    log::info!("Command: get_alsa_plugins");

    let plugins = vec![
        AlsaPluginInfo {
            plugin: AlsaPlugin::Hw,
            name: "hw (Direct Hardware)".to_string(),
            description: "Bit-perfect, exclusive access, blocks device for other apps".to_string(),
        },
        AlsaPluginInfo {
            plugin: AlsaPlugin::PlugHw,
            name: "plughw (Plugin Hardware)".to_string(),
            description: "Automatic format conversion, still relatively direct".to_string(),
        },
        AlsaPluginInfo {
            plugin: AlsaPlugin::Pcm,
            name: "pcm (Default)".to_string(),
            description: "Generic ALSA device, most compatible".to_string(),
        },
    ];

    Ok(plugins)
}

/// Check if ALSA device enumeration is available
/// Note: As of v1.1.14, we read directly from /proc/asound instead of using aplay,
/// so alsa-utils package is no longer required. This always returns true for compatibility.
#[tauri::command]
pub fn check_alsa_utils_installed() -> bool {
    // We no longer depend on alsa-utils - device info comes from /proc/asound
    true
}

/// Get the name/description of the current default audio device for a backend
#[tauri::command]
pub fn get_default_device_name(backend_type: AudioBackendType) -> Option<String> {
    use std::process::Command;

    match backend_type {
        AudioBackendType::PipeWire | AudioBackendType::Pulse => {
            // Use pactl to get default sink description
            let output = Command::new("pactl")
                .args(["get-default-sink"])
                .output()
                .ok()?;

            if !output.status.success() {
                return None;
            }

            let sink_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if sink_name.is_empty() {
                return None;
            }

            // Now get the description for this sink
            let list_output = Command::new("pactl")
                .args(["list", "sinks", "short"])
                .output()
                .ok()?;

            // If we can't get description, return the sink name itself
            if !list_output.status.success() {
                return Some(sink_name);
            }

            // Try to get a nicer description from pactl list sinks
            let full_list = Command::new("pactl")
                .args(["list", "sinks"])
                .output()
                .ok()?;

            if full_list.status.success() {
                let stdout = String::from_utf8_lossy(&full_list.stdout);
                let mut in_target = false;

                for line in stdout.lines() {
                    if line.contains("Name:") && line.contains(&sink_name) {
                        in_target = true;
                    }
                    if in_target && line.trim().starts_with("Description:") {
                        return line.trim()
                            .strip_prefix("Description:")
                            .map(|s| s.trim().to_string());
                    }
                    if in_target && line.starts_with("Sink #") {
                        break; // Moved to next sink
                    }
                }
            }

            Some(sink_name)
        }
        AudioBackendType::Alsa => {
            // For ALSA, read from /proc/asound to find which card is default
            // The "default" ALSA device typically follows PipeWire/Pulse default
            // So we can reuse the PipeWire logic, or read ALSA config

            // Simple approach: use CPAL to get default device name
            use rodio::cpal::traits::HostTrait;

            let available_hosts = rodio::cpal::available_hosts();
            let alsa_host_id = available_hosts.into_iter().find(|h| h.name() == "ALSA")?;
            let host = rodio::cpal::host_from_id(alsa_host_id).ok()?;

            let default_device = host.default_output_device()?;
            use rodio::cpal::traits::DeviceTrait;
            let device_name = default_device.name().ok()?;

            // If it's just "default", try to resolve what it actually points to
            if device_name == "default" {
                // Fall back to PipeWire/Pulse default since ALSA often uses that
                return get_default_device_name(AudioBackendType::PipeWire);
            }

            Some(device_name)
        }
    }
}

/// Linux distribution info for install commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinuxDistroInfo {
    pub distro_id: String,
    pub distro_name: String,
    pub install_command: String,
}

/// Detect Linux distribution and return appropriate install command for alsa-utils
#[tauri::command]
pub fn get_linux_distro() -> LinuxDistroInfo {
    use std::fs;

    // Try to read /etc/os-release
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();

    let mut distro_id = String::new();
    let mut distro_name = String::new();

    for line in os_release.lines() {
        if let Some(id) = line.strip_prefix("ID=") {
            distro_id = id.trim_matches('"').to_lowercase();
        }
        if let Some(name) = line.strip_prefix("NAME=") {
            distro_name = name.trim_matches('"').to_string();
        }
    }

    // Determine install command based on distro
    let install_command = match distro_id.as_str() {
        "arch" | "manjaro" | "endeavouros" | "garuda" | "artix" =>
            "sudo pacman -S alsa-utils".to_string(),
        "debian" | "ubuntu" | "linuxmint" | "pop" | "elementary" | "zorin" | "kali" =>
            "sudo apt install alsa-utils".to_string(),
        "fedora" | "rhel" | "centos" | "rocky" | "alma" =>
            "sudo dnf install alsa-utils".to_string(),
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" =>
            "sudo zypper install alsa-utils".to_string(),
        "void" =>
            "sudo xbps-install alsa-utils".to_string(),
        "gentoo" =>
            "sudo emerge media-sound/alsa-utils".to_string(),
        "nixos" =>
            "nix-env -iA nixpkgs.alsa-utils".to_string(),
        _ => "# Install alsa-utils using your package manager".to_string(),
    };

    LinuxDistroInfo {
        distro_id,
        distro_name,
        install_command,
    }
}

/// DAC capabilities information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DacCapabilities {
    pub node_name: String,
    pub sample_rates: Vec<u32>,
    pub formats: Vec<String>,
    pub channels: Option<u32>,
    pub description: Option<String>,
    pub error: Option<String>,
}

/// Query DAC capabilities from PipeWire
#[tauri::command]
pub fn query_dac_capabilities(node_name: String) -> DacCapabilities {
    use std::process::Command;

    log::info!("Command: query_dac_capabilities({})", node_name);

    let mut caps = DacCapabilities {
        node_name: node_name.clone(),
        sample_rates: Vec::new(),
        formats: Vec::new(),
        channels: None,
        description: None,
        error: None,
    };

    // First, try to get the node ID from the node name using pw-cli
    let pw_dump = Command::new("pw-cli")
        .args(["list-objects", "Node"])
        .output();

    let node_id = match pw_dump {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Find the node ID for this node name
            // Format: id X, type PipeWire:Interface:Node/3
            //         ... node.name = "alsa_output.usb-..."
            let mut found_id: Option<String> = None;
            let mut current_id: Option<String> = None;

            for line in stdout.lines() {
                if line.contains("id ") && line.contains("type PipeWire:Interface:Node") {
                    // Extract ID from line like "id 46, type PipeWire:Interface:Node/3"
                    if let Some(id_part) = line.split("id ").nth(1) {
                        if let Some(id) = id_part.split(',').next() {
                            current_id = Some(id.trim().to_string());
                        }
                    }
                }
                if line.contains("node.name") && line.contains(&node_name) {
                    found_id = current_id.clone();
                    break;
                }
            }
            found_id
        }
        _ => None,
    };

    // If we found the node ID, query its properties
    if let Some(id) = node_id {
        log::info!("Found node ID: {} for {}", id, node_name);

        // Query node properties using pw-cli
        let inspect = Command::new("pw-cli")
            .args(["info", &id])
            .output();

        if let Ok(output) = inspect {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                parse_pw_info(&stdout, &mut caps);
            }
        }

        // Also try pactl for additional info
        let pactl = Command::new("pactl")
            .args(["list", "sinks"])
            .output();

        if let Ok(output) = pactl {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                parse_pactl_sinks(&stdout, &node_name, &mut caps);
            }
        }
    } else {
        // Try pactl directly if pw-cli didn't find it
        let pactl = Command::new("pactl")
            .args(["list", "sinks"])
            .output();

        match pactl {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !parse_pactl_sinks(&stdout, &node_name, &mut caps) {
                    caps.error = Some("Device not found. Make sure the node name is correct.".to_string());
                }
            }
            Ok(_) => {
                caps.error = Some("pactl command failed".to_string());
            }
            Err(e) => {
                caps.error = Some(format!("Failed to run pactl: {}", e));
            }
        }
    }

    // Query ALSA hardware capabilities for actual supported rates
    // This is more reliable than PipeWire/PulseAudio which only show current config
    query_alsa_hw_capabilities(&node_name, &mut caps);

    // Sort sample rates
    caps.sample_rates.sort();
    caps.sample_rates.dedup();

    // If we still don't have sample rates, add common defaults with a note
    if caps.sample_rates.is_empty() && caps.error.is_none() {
        caps.error = Some("Could not detect sample rates. Check device manually.".to_string());
    }

    log::info!("DAC capabilities: {:?}", caps);
    caps
}

/// Query ALSA hardware capabilities from /proc/asound
fn query_alsa_hw_capabilities(node_name: &str, caps: &mut DacCapabilities) {
    use std::fs;

    // Try to extract card info from node name
    // Format: alsa_output.usb-Manufacturer_Product-00.analog-stereo
    // or: alsa_output.pci-0000_00_1f.3.analog-stereo

    // First, try to find the card by listing /proc/asound/cards
    let cards_output = fs::read_to_string("/proc/asound/cards").unwrap_or_default();

    // Try to match card by description from pactl or by USB identifier in node name
    let mut card_num: Option<u32> = None;

    // Extract USB identifier from node name if present
    let usb_id = if node_name.contains("usb-") {
        node_name
            .split("usb-")
            .nth(1)
            .and_then(|s| s.split('-').next())
            .map(|s| s.to_lowercase())
    } else {
        None
    };

    // Search for matching card
    for line in cards_output.lines() {
        // Format: " 0 [Generic        ]: USB-Audio - USB Audio"
        if let Some(num_str) = line.trim().split_whitespace().next() {
            if let Ok(num) = num_str.parse::<u32>() {
                let line_lower = line.to_lowercase();

                // Match by USB identifier or description
                if let Some(ref usb) = usb_id {
                    if line_lower.contains(usb) || line_lower.contains("usb") {
                        card_num = Some(num);
                        break;
                    }
                }

                // Match by description if we have one
                if let Some(ref desc) = caps.description {
                    if line_lower.contains(&desc.to_lowercase()) {
                        card_num = Some(num);
                        break;
                    }
                }
            }
        }
    }

    // If we found a card, try to read its stream info
    if let Some(card) = card_num {
        log::info!("Found ALSA card {} for {}", card, node_name);

        // Try /proc/asound/cardX/stream0 for USB devices
        let stream_path = format!("/proc/asound/card{}/stream0", card);
        if let Ok(stream_info) = fs::read_to_string(&stream_path) {
            parse_alsa_stream_info(&stream_info, caps);
        }

        // Try /proc/asound/cardX/pcmXp/sub0/hw_params for additional info (if stream is active)
        // This provides current hardware params but may not be available if device isn't in use
        for subdev in 0..2 {
            let hw_params_path = format!("/proc/asound/card{}/pcm{}p/sub0/hw_params", card, subdev);
            if let Ok(hw_params) = fs::read_to_string(&hw_params_path) {
                if hw_params.contains("rate:") {
                    parse_proc_hw_params(&hw_params, caps);
                    break;
                }
            }
        }
    }
}

/// Parse /proc/asound/cardX/stream0 output
fn parse_alsa_stream_info(output: &str, caps: &mut DacCapabilities) {
    let mut in_playback = false;

    for line in output.lines() {
        let line = line.trim();

        // Look for Playback section
        if line.starts_with("Playback:") {
            in_playback = true;
            continue;
        }
        if line.starts_with("Capture:") {
            in_playback = false;
            continue;
        }

        if in_playback {
            // Format line contains info like: "Format: S32_LE"
            if line.starts_with("Format:") {
                if let Some(format) = line.strip_prefix("Format:") {
                    let format = format.trim().to_string();
                    if !format.is_empty() && !caps.formats.contains(&format) {
                        caps.formats.push(format);
                    }
                }
            }

            // Rates line: "Rates: 44100, 48000, 88200, 96000, 176400, 192000"
            if line.starts_with("Rates:") {
                if let Some(rates_str) = line.strip_prefix("Rates:") {
                    for rate_str in rates_str.split(',') {
                        if let Ok(rate) = rate_str.trim().parse::<u32>() {
                            if !caps.sample_rates.contains(&rate) {
                                caps.sample_rates.push(rate);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Parse /proc/asound/cardX/pcmXp/sub0/hw_params output (when device is active)
fn parse_proc_hw_params(output: &str, caps: &mut DacCapabilities) {
    for line in output.lines() {
        let line = line.trim();

        // Format: "rate: 192000 (192000/1)"
        if line.starts_with("rate:") {
            if let Some(rate_part) = line.strip_prefix("rate:") {
                // Take first number before any parenthesis or space
                let rate_str = rate_part.trim().split_whitespace().next().unwrap_or("");
                if let Ok(rate) = rate_str.parse::<u32>() {
                    if !caps.sample_rates.contains(&rate) {
                        caps.sample_rates.push(rate);
                    }
                }
            }
        }

        // Format: "format: S32_LE"
        if line.starts_with("format:") {
            if let Some(format_str) = line.strip_prefix("format:") {
                let format = format_str.trim().to_string();
                if !format.is_empty() && !caps.formats.contains(&format) {
                    caps.formats.push(format);
                }
            }
        }
    }
}

fn parse_pw_info(output: &str, caps: &mut DacCapabilities) {
    for line in output.lines() {
        let line = line.trim();

        // Look for audio.rate or similar
        if line.contains("audio.rate") || line.contains("default.clock.rate") {
            if let Some(rate_str) = line.split('=').last() {
                if let Ok(rate) = rate_str.trim().trim_matches('"').parse::<u32>() {
                    if !caps.sample_rates.contains(&rate) {
                        caps.sample_rates.push(rate);
                    }
                }
            }
        }

        // Look for allowed rates
        if line.contains("clock.allowed-rates") || line.contains("audio.allowed-rates") {
            // Format: [ 44100 48000 88200 96000 176400 192000 ]
            if let Some(rates_part) = line.split('[').last() {
                if let Some(rates_str) = rates_part.split(']').next() {
                    for rate_str in rates_str.split_whitespace() {
                        if let Ok(rate) = rate_str.parse::<u32>() {
                            if !caps.sample_rates.contains(&rate) {
                                caps.sample_rates.push(rate);
                            }
                        }
                    }
                }
            }
        }

        // Look for audio format
        if line.contains("audio.format") || line.contains("format.dsp") {
            if let Some(format) = line.split('=').last() {
                let format = format.trim().trim_matches('"').to_string();
                if !format.is_empty() && !caps.formats.contains(&format) {
                    caps.formats.push(format);
                }
            }
        }

        // Look for channels
        if line.contains("audio.channels") {
            if let Some(ch_str) = line.split('=').last() {
                if let Ok(ch) = ch_str.trim().trim_matches('"').parse::<u32>() {
                    caps.channels = Some(ch);
                }
            }
        }

        // Look for description
        if line.contains("node.description") {
            if let Some(desc) = line.split('=').last() {
                caps.description = Some(desc.trim().trim_matches('"').to_string());
            }
        }
    }
}

fn parse_pactl_sinks(output: &str, node_name: &str, caps: &mut DacCapabilities) -> bool {
    let mut in_target_sink = false;
    let mut found = false;

    for line in output.lines() {
        let line_trimmed = line.trim();

        // Check if we're entering a new sink block
        if line.starts_with("Sink #") {
            in_target_sink = false;
        }

        // Check if this is our target sink
        if line_trimmed.contains("Name:") && line_trimmed.contains(node_name) {
            in_target_sink = true;
            found = true;
        }

        if in_target_sink {
            // Get description
            if line_trimmed.starts_with("Description:") {
                if let Some(desc) = line_trimmed.strip_prefix("Description:") {
                    caps.description = Some(desc.trim().to_string());
                }
            }

            // Get sample spec (contains rate and format)
            if line_trimmed.starts_with("Sample Specification:") {
                // Format: "s32le 2ch 192000Hz"
                if let Some(spec) = line_trimmed.strip_prefix("Sample Specification:") {
                    let parts: Vec<&str> = spec.trim().split_whitespace().collect();
                    for part in parts {
                        // Format like s16le, s24le, s32le, float32le
                        if part.starts_with('s') || part.starts_with("float") {
                            let format = part.to_uppercase();
                            if !caps.formats.contains(&format) {
                                caps.formats.push(format);
                            }
                        }
                        // Channels like 2ch
                        if part.ends_with("ch") {
                            if let Ok(ch) = part.trim_end_matches("ch").parse::<u32>() {
                                caps.channels = Some(ch);
                            }
                        }
                        // Rate like 192000Hz
                        if part.ends_with("Hz") {
                            if let Ok(rate) = part.trim_end_matches("Hz").parse::<u32>() {
                                if !caps.sample_rates.contains(&rate) {
                                    caps.sample_rates.push(rate);
                                }
                            }
                        }
                    }
                }
            }

            // Look for alternate sample rates in properties
            if line_trimmed.contains("device.buffering.buffer_size") {
                // End of useful properties for this sink
            }
        }
    }

    // Sort sample rates
    caps.sample_rates.sort();

    found
}
