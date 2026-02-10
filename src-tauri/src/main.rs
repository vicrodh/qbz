// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
fn is_virtual_machine() -> bool {
    // DMI product name (most reliable)
    if let Ok(product) = std::fs::read_to_string("/sys/class/dmi/id/product_name") {
        let p = product.trim().to_lowercase();
        if p.contains("virtualbox")
            || p.contains("vmware")
            || p.contains("qemu")
            || p.contains("bochs")
            || p.contains("hyper-v")
        {
            return true;
        }
    }
    // DMI system vendor
    if let Ok(vendor) = std::fs::read_to_string("/sys/class/dmi/id/sys_vendor") {
        let v = vendor.trim().to_lowercase();
        if v.contains("innotek")
            || v.contains("vmware")
            || v.contains("qemu")
            || v.contains("xen")
            || v.contains("parallels")
        {
            return true;
        }
    }
    // Hypervisor type (Xen, KVM)
    if let Ok(h) = std::fs::read_to_string("/sys/hypervisor/type") {
        let h = h.trim().to_lowercase();
        if !h.is_empty() {
            return true;
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn is_nvidia_gpu() -> bool {
    // Method 1: Check for NVIDIA driver via /proc
    if std::path::Path::new("/proc/driver/nvidia/version").exists() {
        return true;
    }

    // Method 2: Check for loaded NVIDIA kernel modules
    if let Ok(modules) = std::fs::read_to_string("/proc/modules") {
        if modules.lines().any(|line| line.starts_with("nvidia")) {
            return true;
        }
    }

    // Method 3: Check for NVIDIA devices in lspci output (requires external command)
    // Skip this for now to avoid external dependencies

    false
}

fn main() {
    // CLI flag: --reset-dmabuf — resets the developer force_dmabuf setting and exits
    if std::env::args().any(|a| a == "--reset-dmabuf") {
        match qbz_nix_lib::config::developer_settings::DeveloperSettingsStore::new() {
            Ok(store) => {
                match store.set_force_dmabuf(false) {
                    Ok(()) => {
                        eprintln!("[QBZ] Developer force_dmabuf has been reset to false.");
                        eprintln!("[QBZ] You can now start QBZ normally.");
                    }
                    Err(e) => eprintln!("[QBZ] Failed to reset force_dmabuf: {}", e),
                }
            }
            Err(e) => eprintln!("[QBZ] Failed to open developer settings: {}", e),
        }
        return;
    }

    // Set the application name/class for Linux window managers
    // This helps task managers and window switchers identify the app correctly
    #[cfg(target_os = "linux")]
    {
        // Set program name (affects WM_CLASS)
        std::env::set_var("GTK_APPLICATION_ID", "com.blitzfc.qbz");
        // GLib program name helps with process identification
        // This is set before any GTK initialization
    }

    // Use xdg-desktop-portal for file dialogs on Linux.
    // Honor explicit overrides (e.g., sandboxed environments).
    #[cfg(target_os = "linux")]
    if std::env::var_os("GTK_USE_PORTAL").is_none() {
        std::env::set_var("GTK_USE_PORTAL", "1");
    }

    // Prefer a writable TMPDIR to avoid GTK pixbuf cache crashes on some systems.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("TMPDIR").is_none() {
            if let Some(cache_dir) = dirs::cache_dir() {
                let tmp_dir = cache_dir.join("qbz/tmp");
                if std::fs::create_dir_all(&tmp_dir).is_ok() {
                    std::env::set_var("TMPDIR", tmp_dir);
                }
            }
        }
    }

    // Wayland and WebKit compatibility fixes for Linux
    // Addresses: https://github.com/vicrodh/qbz/issues/6
    //            https://github.com/vicrodh/qbz/issues/59
    //
    // The primary EGL crash ("Could not create default EGL display") in
    // AppImage is fixed at the build level by pinning WebKitGTK to 2.44.0-2
    // in the CI workflow (see tauri-apps/tauri#11994).
    //
    // Runtime workarounds below handle NVIDIA DMA-BUF issues, VM detection,
    // and provide escape hatches for edge cases.
    #[cfg(target_os = "linux")]
    {
        let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland");
        let has_nvidia = is_nvidia_gpu();
        let is_vm = is_virtual_machine();
        let force_software = std::env::var("QBZ_SOFTWARE_RENDER").as_deref() == Ok("1");

        // Graphics settings: hardware acceleration opt-in (from Settings > Appearance)
        // Default is OFF — safest for AppImage across heterogeneous hardware.
        // Env var QBZ_HARDWARE_ACCEL=1|0 ALWAYS overrides the DB value (crash recovery).
        let graphics_db = qbz_nix_lib::config::graphics_settings::GraphicsSettingsStore::new()
            .ok()
            .and_then(|store| store.get_settings().ok());
        let hw_accel_db = graphics_db.as_ref().map(|s| s.hardware_acceleration).unwrap_or(false);
        let hardware_accel = match std::env::var("QBZ_HARDWARE_ACCEL").as_deref() {
            Ok("1") => {
                qbz_nix_lib::logging::log_startup("[QBZ] Env override: QBZ_HARDWARE_ACCEL=1 (GPU rendering forced on)");
                true
            }
            Ok("0") => {
                qbz_nix_lib::logging::log_startup("[QBZ] Env override: QBZ_HARDWARE_ACCEL=0 (GPU rendering forced off)");
                false
            }
            _ => hw_accel_db,
        };
        qbz_nix_lib::logging::log_startup(&format!("[QBZ] Hardware acceleration: {}", if hardware_accel { "enabled" } else { "disabled (default)" }));

        // Developer settings: force_dmabuf override (from Settings > Developer Mode)
        let dev_force_dmabuf = qbz_nix_lib::config::developer_settings::DeveloperSettingsStore::new()
            .ok()
            .and_then(|store| store.get_settings().ok())
            .map(|s| s.force_dmabuf)
            .unwrap_or(false);
        if dev_force_dmabuf {
            std::env::set_var("QBZ_FORCE_DMABUF", "1");
            qbz_nix_lib::logging::log_startup("[QBZ] Developer override: force_dmabuf=true (from settings)");
            qbz_nix_lib::logging::log_startup("[QBZ] To reset: run `qbz --reset-dmabuf`");
        }

        // User overrides
        let force_dmabuf = std::env::var("QBZ_FORCE_DMABUF").as_deref() == Ok("1");
        let disable_dmabuf = std::env::var("QBZ_DISABLE_DMABUF").as_deref() == Ok("1");

        // Force X11: persistent setting from DB, env var overrides (crash recovery)
        let force_x11_db = graphics_db.as_ref().map(|s| s.force_x11).unwrap_or(false);
        let force_x11 = match std::env::var("QBZ_FORCE_X11").as_deref() {
            Ok("1") => {
                qbz_nix_lib::logging::log_startup("[QBZ] Env override: QBZ_FORCE_X11=1");
                true
            }
            Ok("0") => {
                qbz_nix_lib::logging::log_startup("[QBZ] Env override: QBZ_FORCE_X11=0");
                false
            }
            _ => force_x11_db,
        };

        // Diagnostic logging (display server logged after GDK backend selection below)
        if has_nvidia {
            qbz_nix_lib::logging::log_startup("[QBZ] NVIDIA GPU detected");
        }
        if is_vm {
            qbz_nix_lib::logging::log_startup("[QBZ] Virtual machine detected");
        }

        // --- Software rendering (GL layer) ---
        // LIBGL_ALWAYS_SOFTWARE=1 forces Mesa to use llvmpipe for all GL contexts.
        // Only needed in VMs or when the user explicitly requests it.
        if force_software {
            qbz_nix_lib::logging::log_startup("[QBZ] User override: forcing software rendering (QBZ_SOFTWARE_RENDER=1)");
            std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
        } else if is_vm {
            qbz_nix_lib::logging::log_startup("[QBZ] Virtual machine detected: enabling software rendering (LIBGL_ALWAYS_SOFTWARE=1)");
            std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
        }

        // --- GDK backend selection ---
        if force_x11 && is_wayland {
            qbz_nix_lib::logging::log_startup("[QBZ] User override: Forcing X11 backend (QBZ_FORCE_X11=1)");
            std::env::set_var("GDK_BACKEND", "x11");
        } else if is_wayland && std::env::var_os("GDK_BACKEND").is_none() {
            std::env::set_var("GDK_BACKEND", "wayland");
            std::env::set_var("GTK_CSD", "1");
        }

        // Log effective display server AFTER GDK backend selection so the
        // message reflects what GDK will actually use (not just the session
        // type). GDK_BACKEND=x11 on a Wayland session = XWayland.
        let effective_display = match std::env::var("GDK_BACKEND").as_deref() {
            Ok("x11") if is_wayland => "X11 (XWayland)",
            Ok("x11") => "X11",
            Ok("wayland") => "Wayland",
            _ => if is_wayland { "Wayland" } else { "X11" },
        };
        qbz_nix_lib::logging::log_startup(&format!("[QBZ] Display server: {}", effective_display));

        // --- DMA-BUF renderer control ---
        // NVIDIA GPUs have known issues with WebKit's DMA-BUF renderer on
        // Wayland, causing fatal protocol errors (Error 71).
        // When hardware_accel is off, we also disable the DMA-BUF renderer
        // as an extra safety layer for AppImage compatibility.
        if force_dmabuf {
            qbz_nix_lib::logging::log_startup("[QBZ] User override: Forcing DMA-BUF renderer enabled (QBZ_FORCE_DMABUF=1)");
            qbz_nix_lib::logging::log_startup("[QBZ] Warning: This may cause crashes on some Wayland configurations");
        } else if disable_dmabuf {
            qbz_nix_lib::logging::log_startup("[QBZ] User override: Forcing DMA-BUF renderer disabled (QBZ_DISABLE_DMABUF=1)");
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        } else if !hardware_accel {
            qbz_nix_lib::logging::log_startup("[QBZ] Hardware acceleration disabled: disabling WebKit DMA-BUF renderer");
            qbz_nix_lib::logging::log_startup("[QBZ] To enable: Settings > Appearance > Hardware Acceleration");
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        } else if has_nvidia {
            qbz_nix_lib::logging::log_startup("[QBZ] NVIDIA GPU detected: disabling WebKit DMA-BUF renderer");
            qbz_nix_lib::logging::log_startup("[QBZ] To override: set QBZ_FORCE_DMABUF=1 (not recommended)");
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        } else {
            qbz_nix_lib::logging::log_startup("[QBZ] Hardware acceleration enabled, using default WebKit renderer");
        }
    }

    qbz_nix_lib::run()
}
