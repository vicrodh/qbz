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

/// Check if alsa-utils is installed (aplay command available)
#[tauri::command]
pub fn check_alsa_utils_installed() -> bool {
    use std::process::Command;

    Command::new("which")
        .arg("aplay")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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
