//! Audio backend & device wizard section.

use qbz_audio::{AudioBackendType, BackendManager};

pub fn run_audio_wizard() -> Result<(), String> {
    println!("\n=== Audio Backend & Device ===\n");

    // List available backends
    let backends = BackendManager::available_backends();
    println!("Available backends:");
    for (i, bt) in backends.iter().enumerate() {
        let backend = BackendManager::create_backend(*bt);
        let (available, desc) = match backend {
            Ok(b) => (b.is_available(), b.description().to_string()),
            Err(_) => (false, "Not available".to_string()),
        };
        let name = match bt {
            AudioBackendType::PipeWire => "PipeWire",
            AudioBackendType::Alsa => "ALSA Direct",
            AudioBackendType::Pulse => "PulseAudio",
            AudioBackendType::SystemDefault => "System Default",
        };
        let status = if available { "available" } else { "not found" };
        println!("  {}. {} - {} ({})", i + 1, name, desc, status);
    }

    println!("\nSelect backend (1-{}):", backends.len());
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).map_err(|e| e.to_string())?;
    let choice: usize = input.trim().parse().unwrap_or(1);

    if choice == 0 || choice > backends.len() {
        return Err("Invalid choice".to_string());
    }

    let selected_backend = backends[choice - 1];
    println!("\nSelected: {:?}", selected_backend);

    // List devices for selected backend
    let backend = BackendManager::create_backend(selected_backend)
        .map_err(|e| format!("Backend error: {}", e))?;
    let devices = backend.enumerate_devices().map_err(|e| format!("Device error: {}", e))?;

    if devices.is_empty() {
        println!("No devices found for this backend.");
    } else {
        println!("\nAvailable devices:");
        for (i, device) in devices.iter().enumerate() {
            let default_mark = if device.is_default { " (default)" } else { "" };
            println!("  {}. {}{}", i + 1, device.name, default_mark);
        }

        println!("\nSelect device (1-{}, or Enter for default):", devices.len());
        let mut dev_input = String::new();
        std::io::stdin().read_line(&mut dev_input).map_err(|e| e.to_string())?;
        let dev_choice: usize = dev_input.trim().parse().unwrap_or(0);

        if dev_choice > 0 && dev_choice <= devices.len() {
            println!("Selected device: {}", devices[dev_choice - 1].name);
        } else {
            println!("Using default device");
        }
    }

    // Save to audio settings
    if let Ok(store) = qbz_audio::settings::AudioSettingsStore::new() {
        let _ = store.set_backend_type(Some(selected_backend));
        println!("\nAudio settings saved.");
    } else {
        println!("\nNote: Could not save settings (no active session). Settings will apply when qbzd starts.");
    }

    println!();
    Ok(())
}
