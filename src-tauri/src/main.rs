// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
    //
    // WebKit's EGL/DMA-BUF renderer crashes on many GPU/driver/compositor
    // combinations with "Could not create default EGL display" or fatal
    // protocol errors (Error 71). Hardware acceleration is disabled by
    // default and can be opted into with QBZ_HARDWARE_ACCEL=1.
    #[cfg(target_os = "linux")]
    {
        let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland");
        let has_nvidia = is_nvidia_gpu();

        // User overrides
        let hardware_accel = std::env::var("QBZ_HARDWARE_ACCEL").as_deref() == Ok("1");
        let force_x11 = std::env::var("QBZ_FORCE_X11").as_deref() == Ok("1");

        // Diagnostic logging
        eprintln!("[QBZ] Display server: {}", if is_wayland { "Wayland" } else { "X11" });
        if has_nvidia {
            eprintln!("[QBZ] NVIDIA GPU detected");
        }

        // GDK backend selection
        if force_x11 && is_wayland {
            eprintln!("[QBZ] User override: Forcing X11 backend (QBZ_FORCE_X11=1)");
            std::env::set_var("GDK_BACKEND", "x11");
        } else if is_wayland && std::env::var_os("GDK_BACKEND").is_none() {
            std::env::set_var("GDK_BACKEND", "wayland");
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
            std::env::set_var("GTK_CSD", "1");
        }

        // Hardware acceleration control
        // Default: OFF for all GPUs (EGL crashes on too many configurations)
        // Opt-in: QBZ_HARDWARE_ACCEL=1
        if hardware_accel {
            eprintln!("[QBZ] Hardware acceleration enabled (QBZ_HARDWARE_ACCEL=1)");
            if has_nvidia && is_wayland {
                eprintln!("[QBZ] Warning: NVIDIA + Wayland may cause crashes with hardware acceleration");
            }
        } else if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            eprintln!("[QBZ] Hardware acceleration disabled (default). Set QBZ_HARDWARE_ACCEL=1 to enable");
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    qbz_nix_lib::run()
}
