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

    // Use xdg-desktop-portal for file dialogs on Linux
    // This makes GTK apps use native file pickers (e.g., KDE's on Plasma)
    #[cfg(target_os = "linux")]
    std::env::set_var("GTK_USE_PORTAL", "1");

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
    // NVIDIA GPUs have known issues with WebKit's DMA-BUF renderer on Wayland,
    // causing fatal protocol errors (Error 71) that cannot be recovered from.
    // This must be mitigated BEFORE the WebView is initialized.
    #[cfg(target_os = "linux")]
    {
        let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland");
        let has_nvidia = is_nvidia_gpu();

        // User overrides - these ALWAYS take precedence
        let force_dmabuf = std::env::var("QBZ_FORCE_DMABUF").as_deref() == Ok("1");
        let disable_dmabuf = std::env::var("QBZ_DISABLE_DMABUF").as_deref() == Ok("1");
        let force_x11 = std::env::var("QBZ_FORCE_X11").as_deref() == Ok("1");

        // Diagnostic logging for transparency and support
        eprintln!("[QBZ] Display server: {}", if is_wayland { "Wayland" } else { "X11" });
        if has_nvidia {
            eprintln!("[QBZ] NVIDIA GPU detected");
        }

        // Handle user overrides first
        if force_x11 && is_wayland {
            eprintln!("[QBZ] User override: Forcing X11 backend (QBZ_FORCE_X11=1)");
            std::env::set_var("GDK_BACKEND", "x11");
        } else if is_wayland && std::env::var_os("GDK_BACKEND").is_none() {
            // Force Wayland backend to avoid fallback issues
            std::env::set_var("GDK_BACKEND", "wayland");

            // Disable WebKit's compositing mode which can cause protocol errors
            // with transparent windows on Wayland
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");

            // Prefer client-side decorations (we use custom titlebar anyway)
            std::env::set_var("GTK_CSD", "1");
        }

        // DMA-BUF renderer control (the critical NVIDIA fix)
        if force_dmabuf {
            eprintln!("[QBZ] User override: Forcing DMA-BUF renderer enabled (QBZ_FORCE_DMABUF=1)");
            eprintln!("[QBZ] Warning: This may cause crashes on NVIDIA + Wayland");
            // Do NOT set WEBKIT_DISABLE_DMABUF_RENDERER
        } else if disable_dmabuf {
            eprintln!("[QBZ] User override: Forcing DMA-BUF renderer disabled (QBZ_DISABLE_DMABUF=1)");
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        } else if is_wayland && has_nvidia {
            // Automatic mitigation: NVIDIA + Wayland = known issue
            eprintln!("[QBZ] Applying NVIDIA + Wayland workaround: disabling WebKit DMA-BUF renderer");
            eprintln!("[QBZ] This prevents fatal protocol errors on NVIDIA GPUs");
            eprintln!("[QBZ] To override: set QBZ_FORCE_DMABUF=1 (not recommended)");
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        } else if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            // Non-NVIDIA systems: keep default behavior unless already set
            // This ensures Intel/AMD systems maintain full hardware acceleration
            if has_nvidia {
                eprintln!("[QBZ] NVIDIA GPU on X11: applying DMA-BUF workaround for compatibility");
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            } else {
                eprintln!("[QBZ] Non-NVIDIA GPU: using default WebKit renderer (hardware accelerated)");
            }
        }
    }

    qbz_nix_lib::run()
}
