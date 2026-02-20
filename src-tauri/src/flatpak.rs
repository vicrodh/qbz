//! Flatpak runtime detection and utilities
//!
//! Detects when QBZ is running inside a Flatpak sandbox
//! and provides sandbox-specific utilities.

use std::fs;
use std::path::{Path, PathBuf};

/// Check if QBZ is running inside a Flatpak sandbox
pub fn is_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

/// Migrate data from old App ID to new App ID
///
/// This migrates user data from:
/// - `~/.config/com.blitzkriegfc.qbz/` → `~/.config/qbz/` (non-Flatpak)
/// - `~/.var/app/com.blitzkriegfc.qbz/config/com.blitzkriegfc.qbz/` → `~/.var/app/com.blitzfc.qbz/config/qbz/` (Flatpak)
///
/// Returns Ok(true) if migration was performed, Ok(false) if not needed
pub fn migrate_app_id_data() -> Result<bool, String> {
    let (old_config, old_data, old_cache, new_config, new_data, new_cache) = if is_flatpak() {
        // In Flatpak, we need to migrate from the OLD sandbox to the NEW sandbox
        // dirs::config_dir() returns ~/.var/app/com.blitzfc.qbz/config/ (new sandbox)
        // We need to access ~/.var/app/com.blitzkriegfc.qbz/ (old sandbox) which is outside our current sandbox

        let home = std::env::var("HOME").map_err(|_| "Could not determine HOME directory")?;
        let home_path = PathBuf::from(home);

        let old_sandbox = home_path.join(".var/app/com.blitzkriegfc.qbz");
        let new_sandbox = home_path.join(".var/app/com.blitzfc.qbz");

        (
            old_sandbox.join("config/com.blitzkriegfc.qbz"),
            old_sandbox.join("data/com.blitzkriegfc.qbz"),
            old_sandbox.join("cache/com.blitzkriegfc.qbz"),
            new_sandbox.join("config/qbz"),
            new_sandbox.join("data/qbz"),
            new_sandbox.join("cache/qbz"),
        )
    } else {
        // Non-Flatpak: migrate from ~/.config/com.blitzkriegfc.qbz to ~/.config/qbz
        (
            dirs::config_dir()
                .ok_or("Could not determine config directory")?
                .join("com.blitzkriegfc.qbz"),
            dirs::data_dir()
                .ok_or("Could not determine data directory")?
                .join("com.blitzkriegfc.qbz"),
            dirs::cache_dir()
                .ok_or("Could not determine cache directory")?
                .join("com.blitzkriegfc.qbz"),
            dirs::config_dir()
                .ok_or("Could not determine config directory")?
                .join("qbz"),
            dirs::data_dir()
                .ok_or("Could not determine data directory")?
                .join("qbz"),
            dirs::cache_dir()
                .ok_or("Could not determine cache directory")?
                .join("qbz"),
        )
    };

    let mut migrated = false;

    // Only migrate if old data exists and new data doesn't
    if old_config.exists() && !new_config.exists() {
        log::info!("Migrating config: {:?} → {:?}", old_config, new_config);
        copy_dir_all(&old_config, &new_config)
            .map_err(|e| format!("Failed to migrate config: {}", e))?;
        migrated = true;
    }

    if old_data.exists() && !new_data.exists() {
        log::info!("Migrating data: {:?} → {:?}", old_data, new_data);
        copy_dir_all(&old_data, &new_data).map_err(|e| format!("Failed to migrate data: {}", e))?;
        migrated = true;
    }

    if old_cache.exists() && !new_cache.exists() {
        log::info!("Migrating cache: {:?} → {:?}", old_cache, new_cache);
        copy_dir_all(&old_cache, &new_cache)
            .map_err(|e| format!("Failed to migrate cache: {}", e))?;
        migrated = true;
    }

    if migrated {
        log::info!("App ID migration completed successfully");
        log::info!("Old directories are preserved and can be manually deleted");
    }

    Ok(migrated)
}

/// Recursively copy a directory
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

/// Get flatpak-specific guidance for user configuration
pub fn get_flatpak_guidance() -> String {
    let app_id = "com.blitzfc.qbz";

    format!(
        r#"QBZ is running inside a Flatpak sandbox.

For offline libraries on NAS, network mounts, or external disks,
direct filesystem access is required.

Grant access using Flatseal (GUI) or by running:

flatpak override --user --filesystem=/path/to/music {app_id}

Examples:
# CIFS / Samba mount
flatpak override --user --filesystem=/mnt/nas {app_id}

# SSHFS mount
flatpak override --user --filesystem=/home/$USER/music-nas {app_id}

This setting is persistent and survives reboots and updates."#,
        app_id = app_id
    )
}

#[tauri::command]
pub fn is_running_in_flatpak() -> bool {
    is_flatpak()
}

#[tauri::command]
pub fn get_flatpak_help_text() -> String {
    get_flatpak_guidance()
}
