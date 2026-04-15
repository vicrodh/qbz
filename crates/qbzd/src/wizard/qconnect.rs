//! QConnect wizard section.

use crate::config::{DaemonConfig, save_default_config};

pub fn run_qconnect_wizard() -> Result<(), String> {
    println!("\n=== QConnect (Qobuz Connect) ===\n");

    let mut config = DaemonConfig::load(None);

    // Enable/disable
    println!("Enable QConnect? [Y/n] (current: {}):", if config.qconnect.enabled { "enabled" } else { "disabled" });
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap_or_default();
    let trimmed = input.trim();
    if !trimmed.is_empty() {
        config.qconnect.enabled = !trimmed.eq_ignore_ascii_case("n");
    }

    if config.qconnect.enabled {
        // Device name
        let current_name = if config.qconnect.device_name.is_empty() {
            hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "qbzd".to_string())
        } else {
            config.qconnect.device_name.clone()
        };

        println!("Device name [current: {}]:", current_name);
        let mut name_input = String::new();
        std::io::stdin().read_line(&mut name_input).unwrap_or_default();
        let name_trimmed = name_input.trim();
        if !name_trimmed.is_empty() {
            config.qconnect.device_name = name_trimmed.to_string();
        }
    }

    save_default_config(&config)?;

    println!("\nQConnect settings saved.");
    println!("  Enabled: {}", config.qconnect.enabled);
    if config.qconnect.enabled {
        let name = if config.qconnect.device_name.is_empty() { "hostname" } else { &config.qconnect.device_name };
        println!("  Device name: {}", name);
    }
    println!();
    Ok(())
}
