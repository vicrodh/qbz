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
    // WebKitGTK's DMA-BUF renderer can crash on Wayland with
    // "Could not create default EGL display: EGL_BAD_PARAMETER. Aborting..."
    // Known triggers: NVIDIA GPUs, AppImage builds (WebKitGTK version mismatch
    // between build and host), and virtual machines.
    // All env-var mitigations must be set BEFORE the WebView is initialized.
    #[cfg(target_os = "linux")]
    {
        let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland");
        let has_nvidia = is_nvidia_gpu();
        let is_vm = is_virtual_machine();
        let force_software = std::env::var("QBZ_SOFTWARE_RENDER").as_deref() == Ok("1");

        // Developer settings: force_dmabuf override (from Settings > Developer Mode)
        // This sets the env var BEFORE the check below, so it integrates seamlessly
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

        // User overrides - these ALWAYS take precedence
        let force_dmabuf = std::env::var("QBZ_FORCE_DMABUF").as_deref() == Ok("1");
        let disable_dmabuf = std::env::var("QBZ_DISABLE_DMABUF").as_deref() == Ok("1");
        let force_x11 = std::env::var("QBZ_FORCE_X11").as_deref() == Ok("1");

        // Diagnostic logging for transparency and support
        qbz_nix_lib::logging::log_startup(&format!("[QBZ] Display server: {}", if is_wayland { "Wayland" } else { "X11" }));
        if has_nvidia {
            qbz_nix_lib::logging::log_startup("[QBZ] NVIDIA GPU detected");
        }
        if is_vm {
            qbz_nix_lib::logging::log_startup("[QBZ] Virtual machine detected");
        }

        // AppImage detection (APPIMAGE/APPDIR set by the AppImage runtime)
        let is_appimage = std::env::var_os("APPIMAGE").is_some()
            || std::env::var_os("APPDIR").is_some();
        if is_appimage {
            qbz_nix_lib::logging::log_startup("[QBZ] Running as AppImage");
        }

        // --- Software rendering (GL layer) ---
        // LIBGL_ALWAYS_SOFTWARE=1 forces Mesa to use llvmpipe for all GL
        // contexts.  Only needed in VMs or when the user explicitly requests it.
        // Does NOT affect WebKit's DMA-BUF renderer decision (that is below).
        if force_software {
            qbz_nix_lib::logging::log_startup("[QBZ] User override: forcing software rendering (QBZ_SOFTWARE_RENDER=1)");
            std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
        } else if is_vm {
            qbz_nix_lib::logging::log_startup("[QBZ] Virtual machine detected: enabling software rendering (LIBGL_ALWAYS_SOFTWARE=1)");
            std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
        }

        // --- Opt-in: disable WebKit compositing mode ---
        // WEBKIT_DISABLE_COMPOSITING_MODE=1 was previously set automatically
        // on Wayland, but it causes a "ghost app" (process alive, tray/MPRIS OK,
        // UI never renders) on Fedora + Wayland.  Now opt-in only.
        if std::env::var("QBZ_WEBKIT_DISABLE_COMPOSITING").as_deref() == Ok("1") {
            qbz_nix_lib::logging::log_startup("[QBZ] User override: disabling WebKit compositing mode (QBZ_WEBKIT_DISABLE_COMPOSITING=1)");
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }

        // --- AppImage + Wayland: prevent ALL EGL usage (unconditional) ---
        // AppImage builds bundle an incomplete/incompatible libEGL.so.
        // On Wayland hosts, even with GDK_BACKEND=x11, WebKitGTK still
        // initializes EGL for GPU compositing.  The bundled libEGL sees
        // WAYLAND_DISPLAY, tries the Wayland EGL platform, and aborts
        // with "Could not create default EGL display: EGL_BAD_PARAMETER."
        //
        // There is NO runtime fallback once EGL is attempted.
        // Three env vars are needed to fully prevent EGL initialization:
        //   GDK_BACKEND=x11           → GTK uses X11/XWayland (not Wayland)
        //   WEBKIT_DISABLE_COMPOSITING_MODE=1 → WebKitGTK skips GPU compositing
        //                                        (this is what actually prevents EGL init)
        //   WEBKIT_DISABLE_DMABUF_RENDERER=1  → extra safety: no DMA-BUF EGL path
        //
        // WEBKIT_DISABLE_COMPOSITING_MODE caused "ghost app" on native Wayland,
        // but is safe on X11 (which we force here).
        //
        // Does NOT affect RPM, DEB, Flatpak, or native builds.
        // Users who want to test native Wayland can set QBZ_FORCE_WAYLAND=1.
        let appimage_forced_x11 = if is_appimage && is_wayland {
            let user_wants_wayland = std::env::var("QBZ_FORCE_WAYLAND").as_deref() == Ok("1");
            if user_wants_wayland {
                qbz_nix_lib::logging::log_startup("[QBZ] AppImage on Wayland: user override QBZ_FORCE_WAYLAND=1, keeping native Wayland");
                qbz_nix_lib::logging::log_startup("[QBZ] Warning: This may crash with EGL_BAD_PARAMETER on some systems");
                false
            } else {
                qbz_nix_lib::logging::log_startup("[QBZ] AppImage on Wayland: forcing X11 + disabling GPU compositing to prevent EGL crash");
                qbz_nix_lib::logging::log_startup("[QBZ] To test native Wayland: set QBZ_FORCE_WAYLAND=1");
                std::env::set_var("GDK_BACKEND", "x11");
                std::env::set_var("QT_QPA_PLATFORM", "xcb");
                std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
                true
            }
        } else {
            false
        };

        // --- GDK backend selection (non-AppImage paths) ---
        if !appimage_forced_x11 {
            if force_x11 && is_wayland {
                qbz_nix_lib::logging::log_startup("[QBZ] User override: Forcing X11 backend (QBZ_FORCE_X11=1)");
                std::env::set_var("GDK_BACKEND", "x11");
            } else if is_wayland && std::env::var_os("GDK_BACKEND").is_none() {
                // Non-AppImage Wayland: use native Wayland backend
                std::env::set_var("GDK_BACKEND", "wayland");

                // Prefer client-side decorations (we use custom titlebar anyway)
                std::env::set_var("GTK_CSD", "1");
            }
        }

        // --- DMA-BUF renderer control (non-AppImage paths) ---
        // WebKitGTK's DMA-BUF renderer (default since 2.42) calls
        // eglGetPlatformDisplay() during init.  On some Wayland configurations
        // this fails with EGL_BAD_PARAMETER and aborts the process.
        // Known triggers: NVIDIA GPUs, virtual GPUs in VMs.
        //
        // Disabling the DMA-BUF renderer makes WebKit fall back to a simpler
        // rendering path that does not require EGL platform display creation.
        //
        // Note: AppImage + Wayland already set WEBKIT_DISABLE_DMABUF_RENDERER
        // above, so this block only runs for non-AppImage builds.
        if !appimage_forced_x11 {
            if force_dmabuf {
                qbz_nix_lib::logging::log_startup("[QBZ] User override: Forcing DMA-BUF renderer enabled (QBZ_FORCE_DMABUF=1)");
                qbz_nix_lib::logging::log_startup("[QBZ] Warning: This may cause crashes on some Wayland configurations");
                // Do NOT set WEBKIT_DISABLE_DMABUF_RENDERER
            } else if disable_dmabuf {
                qbz_nix_lib::logging::log_startup("[QBZ] User override: Forcing DMA-BUF renderer disabled (QBZ_DISABLE_DMABUF=1)");
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            } else if has_nvidia {
                qbz_nix_lib::logging::log_startup("[QBZ] NVIDIA GPU detected: disabling WebKit DMA-BUF renderer");
                qbz_nix_lib::logging::log_startup("[QBZ] To override: set QBZ_FORCE_DMABUF=1 (not recommended)");
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            } else if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
                qbz_nix_lib::logging::log_startup("[QBZ] Non-NVIDIA GPU: using default WebKit renderer (hardware accelerated)");
            }
        }
    }

    qbz_nix_lib::run()
}
