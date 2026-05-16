//! Graphics auto-configuration shell adapter.
//!
//! Recommendation and host detection live in `qbz-app`; this module keeps the
//! Tauri/WebKit-specific surfaces that prompt the CLI user and persist the
//! selected recommendation into current settings stores.

use crate::config::graphics_settings::GraphicsSettingsStore;
use qbz_app::graphics_autoconfig::{
    compute_recommendation, detect_environment, Environment, Recommendation,
};
use std::io::{self, BufRead, Write};

/// Run the autoconfig-graphics CLI tool.
pub fn run() {
    eprintln!("[QBZ AutoConfig] Detecting environment...");
    eprintln!();

    let env = detect_environment();
    print_environment(&env);

    let rec = compute_recommendation(&env);
    print_recommendation(&rec);

    eprintln!();
    eprint!("Apply this configuration? [Y/n] ");
    io::stderr().flush().ok();

    let mut input = String::new();
    if io::stdin().lock().read_line(&mut input).is_ok() {
        let answer = input.trim().to_lowercase();
        if answer.is_empty() || answer == "y" || answer == "yes" {
            apply_recommendation(&rec);
        } else {
            eprintln!("[QBZ AutoConfig] Aborted. No changes made.");
        }
    } else {
        eprintln!("[QBZ AutoConfig] Could not read input. No changes made.");
    }
}

fn print_environment(env: &Environment) {
    eprintln!("  Display server : {}", env.display_server);
    eprintln!("  GPU            : {}", env.gpu_name);
    eprintln!("  Desktop        : {}", env.desktop);
    if env.is_vm {
        eprintln!("  Virtual machine: Yes");
    }
    eprintln!();
}

fn print_recommendation(rec: &Recommendation) {
    eprintln!("[QBZ AutoConfig] Recommended configuration:");
    eprintln!(
        "  hardware_acceleration  : {}",
        if rec.hardware_acceleration {
            "on"
        } else {
            "off"
        }
    );
    eprintln!(
        "  force_x11              : {}",
        if rec.force_x11 { "on" } else { "off" }
    );
    eprintln!(
        "  gsk_renderer           : {}",
        rec.gsk_renderer.as_deref().unwrap_or("auto")
    );
    eprintln!(
        "  disable_dmabuf         : {}",
        if rec.disable_dmabuf { "yes" } else { "no" }
    );
    eprintln!(
        "  disable_blur_background: {}",
        if rec.disable_blur_background {
            "yes"
        } else {
            "no"
        }
    );
    eprintln!();
    for reason in &rec.rationale {
        eprintln!("  Rationale: {}", reason);
    }
}

fn apply_recommendation(rec: &Recommendation) {
    match write_recommendation(rec) {
        Ok(()) => {
            if rec.disable_blur_background {
                eprintln!(
                    "[QBZ AutoConfig] Note: blur background will be disabled. You can toggle this in Settings > Appearance."
                );
            }
            eprintln!();
            eprintln!("[QBZ AutoConfig] Configuration applied successfully.");
            eprintln!("[QBZ AutoConfig] Restart QBZ to take effect.");
        }
        Err(errors) => {
            eprintln!();
            eprintln!("[QBZ AutoConfig] Some settings could not be applied:");
            for e in &errors {
                eprintln!("  - {}", e);
            }
        }
    }
}

/// Apply the recommendation to the persistence layer. Shared between the CLI
/// prompt (`apply_recommendation`) and the V2 command surface. Returns the
/// list of write errors so the caller can surface them appropriately (stderr
/// for CLI, frontend toast for the Settings UI).
///
/// DMA-BUF semantics after the 1.2.13 opt-in flip:
///   - `rec.disable_dmabuf = true`  -> force_dmabuf = false (matches default;
///     runtime keeps DMA-BUF off).
///   - `rec.disable_dmabuf = false` -> force_dmabuf = true  (user opts in via
///     this recommendation; runtime turns DMA-BUF on).
pub fn write_recommendation(rec: &Recommendation) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    match GraphicsSettingsStore::new() {
        Ok(store) => {
            if let Err(e) = store.set_hardware_acceleration(rec.hardware_acceleration) {
                errors.push(format!("hardware_acceleration: {}", e));
            }
            if let Err(e) = store.set_force_x11(rec.force_x11) {
                errors.push(format!("force_x11: {}", e));
            }
            if let Err(e) = store.set_gsk_renderer(rec.gsk_renderer.clone()) {
                errors.push(format!("gsk_renderer: {}", e));
            }
        }
        Err(e) => {
            errors.push(format!("graphics settings store: {}", e));
        }
    }

    let desired_force_dmabuf = !rec.disable_dmabuf;
    match crate::config::developer_settings::DeveloperSettingsStore::new() {
        Ok(store) => {
            if let Err(e) = store.set_force_dmabuf(desired_force_dmabuf) {
                errors.push(format!("force_dmabuf: {}", e));
            }
        }
        Err(e) => {
            errors.push(format!("developer settings store: {}", e));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
