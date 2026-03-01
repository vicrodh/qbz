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
    // CLI flag: --reset-graphics — resets ALL graphics/composition settings to defaults
    if std::env::args().any(|a| a == "--reset-graphics") {
        eprintln!("[QBZ] Resetting all graphics settings to defaults...");
        let mut errors = Vec::new();

        // Reset graphics settings (force_x11, gdk_scale, gdk_dpi_scale)
        match qbz_nix_lib::config::graphics_settings::GraphicsSettingsStore::new() {
            Ok(store) => {
                if let Err(e) = store.set_force_x11(false) {
                    errors.push(format!("force_x11: {}", e));
                }
                if let Err(e) = store.set_gdk_scale(None) {
                    errors.push(format!("gdk_scale: {}", e));
                }
                if let Err(e) = store.set_gdk_dpi_scale(None) {
                    errors.push(format!("gdk_dpi_scale: {}", e));
                }
            }
            Err(e) => errors.push(format!("graphics settings store: {}", e)),
        }

        // Reset developer settings (force_dmabuf)
        match qbz_nix_lib::config::developer_settings::DeveloperSettingsStore::new() {
            Ok(store) => {
                if let Err(e) = store.set_force_dmabuf(false) {
                    errors.push(format!("force_dmabuf: {}", e));
                }
            }
            Err(e) => errors.push(format!("developer settings store: {}", e)),
        }

        if errors.is_empty() {
            eprintln!("[QBZ] All graphics settings have been reset:");
            eprintln!("[QBZ]   - force_x11: false");
            eprintln!("[QBZ]   - gdk_scale: auto");
            eprintln!("[QBZ]   - gdk_dpi_scale: auto");
            eprintln!("[QBZ]   - force_dmabuf: false");
            eprintln!("[QBZ] You can now start QBZ normally.");
        } else {
            eprintln!("[QBZ] Some settings could not be reset:");
            for e in &errors {
                eprintln!("[QBZ]   - {}", e);
            }
        }
        return;
    }

    // CLI flag: --reset-dmabuf — resets the developer force_dmabuf setting and exits
    if std::env::args().any(|a| a == "--reset-dmabuf") {
        match qbz_nix_lib::config::developer_settings::DeveloperSettingsStore::new() {
            Ok(store) => match store.set_force_dmabuf(false) {
                Ok(()) => {
                    eprintln!("[QBZ] Developer force_dmabuf has been reset to false.");
                    eprintln!("[QBZ] You can now start QBZ normally.");
                }
                Err(e) => eprintln!("[QBZ] Failed to reset force_dmabuf: {}", e),
            },
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
    //            https://github.com/vicrodh/qbz/issues/67
    //
    // v1.1.12: Reverted to v1.1.9 rendering defaults. The v1.1.10/11 approach
    // of disabling hardware acceleration by default caused severe UI lag for
    // all users (WEBKIT_DISABLE_DMABUF_RENDERER applied globally instead of
    // only to NVIDIA GPUs). Now:
    //   - X11: full hardware acceleration (nothing disabled)
    //   - Wayland: compositing disabled (prevents protocol errors), DMA-BUF
    //     disabled on Wayland for all GPUs (prevents EGL crashes on Intel Arc
    //     and NVIDIA Error 71)
    //   - NVIDIA on X11: only DMA-BUF disabled
    //   - Everything bypassable via env vars
    #[cfg(target_os = "linux")]
    {
        let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland");
        let has_nvidia = is_nvidia_gpu();
        let is_vm = is_virtual_machine();
        let force_software = std::env::var("QBZ_SOFTWARE_RENDER").as_deref() == Ok("1");

        // Graphics settings from DB (force_x11, scaling)
        // Track if we're using fallback defaults (for UI visibility)
        let mut graphics_using_fallback = false;
        let graphics_db = match qbz_nix_lib::config::graphics_settings::GraphicsSettingsStore::new()
        {
            Ok(store) => {
                match store.get_settings() {
                    Ok(settings) => Some(settings),
                    Err(e) => {
                        graphics_using_fallback = true;
                        eprintln!("[QBZ] WARNING: Graphics settings read failed: {}. Using safe defaults.", e);
                        qbz_nix_lib::logging::log_startup(&format!(
                        "[QBZ] Graphics settings unavailable ({}), using safe defaults. If experiencing lag, run: qbz --reset-graphics",
                        e
                    ));
                        None
                    }
                }
            }
            Err(e) => {
                graphics_using_fallback = true;
                eprintln!(
                    "[QBZ] WARNING: Graphics settings store unavailable: {}. Using safe defaults.",
                    e
                );
                qbz_nix_lib::logging::log_startup(&format!(
                    "[QBZ] Graphics settings store unavailable ({}), using safe defaults. If experiencing lag, run: qbz --reset-graphics",
                    e
                ));
                None
            }
        };

        // Hardware acceleration: DB value is the default, env var overrides.
        // QBZ_HARDWARE_ACCEL=0 is the nuclear opt-out that disables all GPU
        // compositing and DMA-BUF everywhere.
        let hw_accel_db = graphics_db.as_ref().map(|s| s.hardware_acceleration).unwrap_or(true);
        let hardware_accel = match std::env::var("QBZ_HARDWARE_ACCEL").as_deref() {
            Ok("0") => {
                qbz_nix_lib::logging::log_startup(
                    "[QBZ] Env override: QBZ_HARDWARE_ACCEL=0 (all GPU rendering disabled)",
                );
                false
            }
            Ok("1") => {
                qbz_nix_lib::logging::log_startup(
                    "[QBZ] Env override: QBZ_HARDWARE_ACCEL=1 (full GPU, all safety bypassed)",
                );
                true
            }
            _ => {
                if !hw_accel_db {
                    qbz_nix_lib::logging::log_startup(
                        "[QBZ] Hardware acceleration disabled via settings (DB)",
                    );
                }
                hw_accel_db
            }
        };

        // Developer settings: force_dmabuf override (from Settings > Developer Mode)
        let dev_force_dmabuf =
            match qbz_nix_lib::config::developer_settings::DeveloperSettingsStore::new() {
                Ok(store) => match store.get_settings() {
                    Ok(settings) => settings.force_dmabuf,
                    Err(e) => {
                        qbz_nix_lib::logging::log_startup(&format!(
                        "[QBZ] Developer settings read failed ({}), force_dmabuf defaulting to false",
                        e
                    ));
                        false
                    }
                },
                Err(e) => {
                    qbz_nix_lib::logging::log_startup(&format!(
                    "[QBZ] Developer settings store unavailable ({}), force_dmabuf defaulting to false",
                    e
                ));
                    false
                }
            };
        if dev_force_dmabuf {
            std::env::set_var("QBZ_FORCE_DMABUF", "1");
            qbz_nix_lib::logging::log_startup(
                "[QBZ] Developer override: force_dmabuf=true (from settings)",
            );
            qbz_nix_lib::logging::log_startup("[QBZ] To reset: run `qbz --reset-dmabuf`");
        }

        // User overrides for DMA-BUF (finer-grained than QBZ_HARDWARE_ACCEL)
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

        // Diagnostic logging
        if has_nvidia {
            qbz_nix_lib::logging::log_startup("[QBZ] NVIDIA GPU detected");
        }
        if is_vm {
            qbz_nix_lib::logging::log_startup("[QBZ] Virtual machine detected");
        }

        // Log graphics configuration summary (helps debug performance issues)
        if graphics_using_fallback {
            eprintln!("[QBZ] WARNING: Running with fallback graphics settings. Performance may be degraded.");
            eprintln!(
                "[QBZ] To fix: run 'qbz --reset-graphics' or check ~/.local/share/qbz/settings.db"
            );
        }
        qbz_nix_lib::logging::log_startup(&format!(
            "[QBZ] Graphics config: wayland={}, nvidia={}, force_x11={}, hw_accel={}, fallback={}",
            is_wayland, has_nvidia, force_x11, hardware_accel, graphics_using_fallback
        ));

        // Store startup state for frontend queries
        qbz_nix_lib::config::graphics_settings::set_startup_graphics_state(
            graphics_using_fallback,
            is_wayland,
            has_nvidia,
            is_vm,
            hardware_accel,
            force_x11,
        );

        // --- Software rendering (GL layer) ---
        // LIBGL_ALWAYS_SOFTWARE=1 forces Mesa to use llvmpipe for all GL contexts.
        // Only for VMs or explicit user request.
        if force_software {
            qbz_nix_lib::logging::log_startup(
                "[QBZ] User override: forcing software rendering (QBZ_SOFTWARE_RENDER=1)",
            );
            std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
        } else if is_vm {
            qbz_nix_lib::logging::log_startup("[QBZ] Virtual machine detected: enabling software rendering (LIBGL_ALWAYS_SOFTWARE=1)");
            std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
        }

        // --- GDK backend selection ---
        if force_x11 && is_wayland {
            qbz_nix_lib::logging::log_startup("[QBZ] Forcing X11 backend on Wayland session");
            std::env::set_var("GDK_BACKEND", "x11");

            // GDK_SCALE is integer-only and only meaningful on X11/XWayland
            if let Some(ref gdk_scale) = graphics_db.as_ref().and_then(|s| s.gdk_scale.clone()) {
                std::env::set_var("GDK_SCALE", gdk_scale);
                qbz_nix_lib::logging::log_startup(&format!("[QBZ] GDK_SCALE={}", gdk_scale));
            }
        } else if is_wayland && std::env::var_os("GDK_BACKEND").is_none() {
            std::env::set_var("GDK_BACKEND", "wayland");
            std::env::set_var("GTK_CSD", "1");
        }

        // GDK_DPI_SCALE is a float multiplier that works on ALL backends
        // (X11, XWayland, and native Wayland). Apply it unconditionally so
        // users can compensate for their DE scale without switching backends.
        if let Some(ref gdk_dpi) = graphics_db.as_ref().and_then(|s| s.gdk_dpi_scale.clone()) {
            std::env::set_var("GDK_DPI_SCALE", gdk_dpi);
            qbz_nix_lib::logging::log_startup(&format!("[QBZ] GDK_DPI_SCALE={}", gdk_dpi));
        }

        // Log effective display server AFTER GDK backend selection
        let effective_display = match std::env::var("GDK_BACKEND").as_deref() {
            Ok("x11") if is_wayland => "X11 (XWayland)",
            Ok("x11") => "X11",
            Ok("wayland") => "Wayland",
            _ => {
                if is_wayland {
                    "Wayland"
                } else {
                    "X11"
                }
            }
        };
        qbz_nix_lib::logging::log_startup(&format!("[QBZ] Display server: {}", effective_display));

        // --- WebKit renderer control ---
        //
        // QBZ_HARDWARE_ACCEL=0 is the nuclear option: disables everything.
        // QBZ_HARDWARE_ACCEL=1 (explicit) bypasses ALL safety measures.
        // Default (no env var): v1.1.9 targeted mitigations only.
        //
        // v1.1.9 defaults:
        //   - Wayland: COMPOSITING off + DMABUF off (prevents EGL/protocol errors)
        //   - X11 + NVIDIA: DMABUF off only (prevents Error 71)
        //   - X11 + non-NVIDIA: nothing disabled (full GPU acceleration)
        //
        // Override hierarchy (highest to lowest):
        //   1. QBZ_HARDWARE_ACCEL=0 → disable everything
        //   2. QBZ_HARDWARE_ACCEL=1 → enable everything (bypass all safety)
        //   3. QBZ_FORCE_DMABUF=1 / QBZ_DISABLE_DMABUF=1 → fine-grained DMA-BUF
        //   4. Auto-detection (Wayland/NVIDIA)
        if !hardware_accel {
            // Nuclear opt-out: disable all GPU compositing and DMA-BUF
            qbz_nix_lib::logging::log_startup(
                "[QBZ] Hardware acceleration disabled: all GPU rendering off",
            );
            qbz_nix_lib::logging::log_startup(
                "[QBZ] To restore: unset QBZ_HARDWARE_ACCEL or set to 1",
            );
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        } else if std::env::var("QBZ_HARDWARE_ACCEL").as_deref() == Ok("1") {
            // Explicit full GPU: skip ALL safety measures
            qbz_nix_lib::logging::log_startup("[QBZ] Full GPU mode: all WebKit safety bypassed");
        } else {
            // Default path: v1.1.9 targeted mitigations

            // Wayland compositing: disable to prevent protocol errors with
            // transparent windows. This was in v1.1.9 and worked fine.
            // Only applies to native Wayland (not force_x11/XWayland).
            if is_wayland && !force_x11 {
                std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
                qbz_nix_lib::logging::log_startup(
                    "[QBZ] Wayland: compositing mode disabled (prevents protocol errors)",
                );
            }

            // DMA-BUF renderer control
            if force_dmabuf {
                qbz_nix_lib::logging::log_startup(
                    "[QBZ] User override: DMA-BUF renderer forced ON (QBZ_FORCE_DMABUF=1)",
                );
            } else if disable_dmabuf {
                qbz_nix_lib::logging::log_startup(
                    "[QBZ] User override: DMA-BUF renderer forced OFF (QBZ_DISABLE_DMABUF=1)",
                );
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            } else if is_wayland && !force_x11 {
                // Wayland: disable DMA-BUF for ALL GPUs. Prevents:
                //   - NVIDIA Error 71 (protocol error)
                //   - Intel Arc EGL crash (Could not create default EGL display)
                // This is an improvement over v1.1.9 which only covered NVIDIA.
                qbz_nix_lib::logging::log_startup(
                    "[QBZ] Wayland: DMA-BUF renderer disabled (prevents EGL crashes)",
                );
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            } else if has_nvidia {
                // X11 + NVIDIA: disable DMA-BUF only (keeps full compositing)
                qbz_nix_lib::logging::log_startup("[QBZ] NVIDIA on X11: DMA-BUF renderer disabled");
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            } else {
                qbz_nix_lib::logging::log_startup(
                    "[QBZ] Using default WebKit renderer (full hardware acceleration)",
                );
            }
        }

        // --- GPU rendering summary ---
        let sw = std::env::var_os("LIBGL_ALWAYS_SOFTWARE").is_some();
        let comp_off = std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_some();
        let dmabuf_off = std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some();

        let gpu_status = if sw {
            "OFF (software rendering via llvmpipe)"
        } else if comp_off && dmabuf_off {
            "partial (compositing: CPU, DMA-BUF: disabled, GL: GPU)"
        } else if comp_off {
            "partial (compositing: CPU, DMA-BUF: GPU)"
        } else if dmabuf_off {
            "partial (compositing: GPU, DMA-BUF: disabled)"
        } else {
            "FULL (compositing: GPU, DMA-BUF: GPU)"
        };
        qbz_nix_lib::logging::log_startup(&format!("[QBZ] GPU rendering: {}", gpu_status));
    }

    qbz_nix_lib::run()
}
