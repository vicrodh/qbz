//! Framework-agnostic graphics auto-configuration recommendation logic.
//!
//! This module detects the host graphics environment and computes the settings
//! profile QBZ should recommend. Applying that profile is intentionally left to
//! the shell adapter because it writes to persisted settings and currently
//! targets the Tauri/WebKit startup path.

use serde::Serialize;

/// Detected environment information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Environment {
    pub display_server: String,
    pub gpu_nvidia: bool,
    pub gpu_amd: bool,
    pub gpu_intel: bool,
    pub gpu_name: String,
    pub desktop: String,
    pub is_vm: bool,
}

/// Recommended configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Recommendation {
    pub hardware_acceleration: bool,
    pub force_x11: bool,
    pub gsk_renderer: Option<String>,
    pub disable_dmabuf: bool,
    pub disable_blur_background: bool,
    pub rationale: Vec<String>,
}

pub fn detect_environment() -> Environment {
    let display_server = detect_display_server();
    let gpu_nvidia = is_nvidia_gpu();
    let gpu_amd = is_amd_gpu();
    let gpu_intel = is_intel_gpu();
    let gpu_name = detect_gpu_name(gpu_nvidia, gpu_amd, gpu_intel);
    let desktop = detect_desktop();
    let is_vm = is_virtual_machine();

    Environment {
        display_server,
        gpu_nvidia,
        gpu_amd,
        gpu_intel,
        gpu_name,
        desktop,
        is_vm,
    }
}

fn detect_display_server() -> String {
    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland");

    if is_wayland {
        "Wayland".to_string()
    } else {
        "X11".to_string()
    }
}

fn is_nvidia_gpu() -> bool {
    std::path::Path::new("/proc/driver/nvidia/version").exists()
        || std::fs::read_to_string("/proc/modules")
            .map(|m| m.lines().any(|l| l.starts_with("nvidia")))
            .unwrap_or(false)
}

fn is_amd_gpu() -> bool {
    std::path::Path::new("/sys/module/amdgpu").exists()
        || std::fs::read_to_string("/proc/modules")
            .map(|m| m.lines().any(|l| l.starts_with("amdgpu")))
            .unwrap_or(false)
}

fn is_intel_gpu() -> bool {
    std::path::Path::new("/sys/module/i915").exists()
        || std::fs::read_to_string("/proc/modules")
            .map(|m| m.lines().any(|l| l.starts_with("i915")))
            .unwrap_or(false)
}

fn is_virtual_machine() -> bool {
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
    if let Ok(h) = std::fs::read_to_string("/sys/hypervisor/type") {
        if !h.trim().is_empty() {
            return true;
        }
    }
    false
}

pub fn detect_gpu_name(nvidia: bool, amd: bool, intel: bool) -> String {
    // Hybrid laptops have more than one of these set; join the names so
    // diagnostics surface the full picture instead of returning only the
    // first vendor matched.
    let mut parts: Vec<String> = Vec::new();
    if nvidia {
        parts.push(nvidia_name());
    }
    if amd {
        parts.push(amd_name());
    }
    if intel {
        parts.push(intel_name());
    }
    if parts.is_empty() {
        "Unknown / None detected".to_string()
    } else {
        parts.join(" + ")
    }
}

fn nvidia_name() -> String {
    if let Ok(version) = std::fs::read_to_string("/proc/driver/nvidia/version") {
        if let Some(line) = version.lines().next() {
            return format!("NVIDIA ({})", line.trim());
        }
    }
    "NVIDIA (driver loaded)".to_string()
}

fn amd_name() -> String {
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("card") && !name.contains('-') {
                let model_path = entry.path().join("device/product_name");
                if let Ok(model) = std::fs::read_to_string(&model_path) {
                    let model = model.trim();
                    if !model.is_empty() {
                        return format!("AMD {}", model);
                    }
                }
            }
        }
    }
    "AMD (amdgpu driver loaded)".to_string()
}

fn intel_name() -> String {
    "Intel (i915/xe driver loaded)".to_string()
}

fn detect_desktop() -> String {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("XDG_SESSION_DESKTOP").unwrap_or_default();
    let de = std::env::var("DESKTOP_SESSION").unwrap_or_default();

    if !desktop.is_empty() {
        desktop
    } else if !session.is_empty() {
        session
    } else if !de.is_empty() {
        de
    } else {
        "Unknown".to_string()
    }
}

pub fn compute_recommendation(env: &Environment) -> Recommendation {
    let mut rationale = Vec::new();

    // VM: software rendering, no blur
    if env.is_vm {
        rationale.push("Virtual machine detected: using software rendering".to_string());
        return Recommendation {
            hardware_acceleration: false,
            force_x11: false,
            gsk_renderer: Some("cairo".to_string()),
            disable_dmabuf: true,
            disable_blur_background: true,
            rationale,
        };
    }

    let is_wayland = env.display_server == "Wayland";
    let desktop_lower = env.desktop.to_lowercase();
    let is_gnome = desktop_lower.contains("gnome");
    let has_hybrid_igpu = env.gpu_nvidia && (env.gpu_intel || env.gpu_amd);

    // Hybrid laptops (NVIDIA dGPU + Intel/AMD iGPU). WebKit composes via
    // the iGPU through EGL/GLX defaults - the NVIDIA card sits idle for
    // PRIME render offload. Forcing GSK_RENDERER=gl here was hurting
    // performance because the GL renderer biases toward the dGPU stack
    // even when the iGPU is the actual paint target. Auto (None) lets
    // GTK4 pick NGL/Vulkan as appropriate for the iGPU. DMA-BUF stays
    // disabled - that one is fragile on any NVIDIA-touching setup.
    if has_hybrid_igpu {
        let igpu_label = if env.gpu_intel { "Intel" } else { "AMD" };
        rationale.push(format!(
            "NVIDIA + {} hybrid: iGPU handles WebKit compositing, leaving GSK at Auto",
            igpu_label
        ));
        return Recommendation {
            hardware_acceleration: true,
            force_x11: false,
            gsk_renderer: None,
            disable_dmabuf: true,
            disable_blur_background: false,
            rationale,
        };
    }

    // NVIDIA + Wayland + GNOME: known stutter combo
    if env.gpu_nvidia && is_wayland && is_gnome {
        rationale.push("NVIDIA + Wayland + GNOME: using GL renderer, DMA-BUF off".to_string());
        rationale.push("This combination has known compositing issues".to_string());
        return Recommendation {
            hardware_acceleration: true,
            force_x11: false,
            gsk_renderer: Some("gl".to_string()),
            disable_dmabuf: true,
            disable_blur_background: false,
            rationale,
        };
    }

    // NVIDIA + Wayland (non-GNOME)
    if env.gpu_nvidia && is_wayland {
        rationale.push("NVIDIA + Wayland: using GL renderer, DMA-BUF off".to_string());
        return Recommendation {
            hardware_acceleration: true,
            force_x11: false,
            gsk_renderer: Some("gl".to_string()),
            disable_dmabuf: true,
            disable_blur_background: false,
            rationale,
        };
    }

    // NVIDIA + X11
    if env.gpu_nvidia {
        rationale.push("NVIDIA + X11: full hardware acceleration, DMA-BUF off".to_string());
        return Recommendation {
            hardware_acceleration: true,
            force_x11: false,
            gsk_renderer: Some("gl".to_string()),
            disable_dmabuf: true,
            disable_blur_background: false,
            rationale,
        };
    }

    // AMD + Wayland
    if env.gpu_amd && is_wayland {
        rationale.push("AMD + Wayland: NGL renderer with DMA-BUF".to_string());
        return Recommendation {
            hardware_acceleration: true,
            force_x11: false,
            gsk_renderer: Some("ngl".to_string()),
            disable_dmabuf: false,
            disable_blur_background: false,
            rationale,
        };
    }

    // AMD + X11
    if env.gpu_amd {
        rationale.push("AMD + X11: full hardware acceleration".to_string());
        return Recommendation {
            hardware_acceleration: true,
            force_x11: false,
            gsk_renderer: None,
            disable_dmabuf: false,
            disable_blur_background: false,
            rationale,
        };
    }

    // Intel + Wayland
    if env.gpu_intel && is_wayland {
        rationale.push("Intel + Wayland: NGL renderer with DMA-BUF".to_string());
        return Recommendation {
            hardware_acceleration: true,
            force_x11: false,
            gsk_renderer: Some("ngl".to_string()),
            disable_dmabuf: false,
            disable_blur_background: false,
            rationale,
        };
    }

    // Intel + X11
    if env.gpu_intel {
        rationale.push("Intel + X11: full hardware acceleration".to_string());
        return Recommendation {
            hardware_acceleration: true,
            force_x11: false,
            gsk_renderer: None,
            disable_dmabuf: false,
            disable_blur_background: false,
            rationale,
        };
    }

    // Unknown GPU: safe defaults
    rationale.push("No known GPU detected: using safe defaults".to_string());
    Recommendation {
        hardware_acceleration: true,
        force_x11: false,
        gsk_renderer: None,
        disable_dmabuf: false,
        disable_blur_background: false,
        rationale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(display_server: &str) -> Environment {
        Environment {
            display_server: display_server.to_string(),
            gpu_nvidia: false,
            gpu_amd: false,
            gpu_intel: false,
            gpu_name: "test".to_string(),
            desktop: "Unknown".to_string(),
            is_vm: false,
        }
    }

    #[test]
    fn vm_uses_cpu_fallback() {
        let mut environment = env("Wayland");
        environment.is_vm = true;

        let recommendation = compute_recommendation(&environment);

        assert!(!recommendation.hardware_acceleration);
        assert_eq!(recommendation.gsk_renderer.as_deref(), Some("cairo"));
        assert!(recommendation.disable_dmabuf);
        assert!(recommendation.disable_blur_background);
    }

    #[test]
    fn nvidia_hybrid_leaves_renderer_auto_and_disables_dmabuf() {
        let mut environment = env("Wayland");
        environment.gpu_nvidia = true;
        environment.gpu_intel = true;

        let recommendation = compute_recommendation(&environment);

        assert!(recommendation.hardware_acceleration);
        assert_eq!(recommendation.gsk_renderer, None);
        assert!(recommendation.disable_dmabuf);
    }

    #[test]
    fn nvidia_wayland_gnome_uses_gl_and_disables_dmabuf() {
        let mut environment = env("Wayland");
        environment.gpu_nvidia = true;
        environment.desktop = "GNOME".to_string();

        let recommendation = compute_recommendation(&environment);

        assert_eq!(recommendation.gsk_renderer.as_deref(), Some("gl"));
        assert!(recommendation.disable_dmabuf);
    }

    #[test]
    fn nvidia_x11_uses_gl_and_disables_dmabuf() {
        let mut environment = env("X11");
        environment.gpu_nvidia = true;

        let recommendation = compute_recommendation(&environment);

        assert_eq!(recommendation.gsk_renderer.as_deref(), Some("gl"));
        assert!(recommendation.disable_dmabuf);
    }

    #[test]
    fn amd_wayland_uses_ngl_and_allows_dmabuf() {
        let mut environment = env("Wayland");
        environment.gpu_amd = true;

        let recommendation = compute_recommendation(&environment);

        assert_eq!(recommendation.gsk_renderer.as_deref(), Some("ngl"));
        assert!(!recommendation.disable_dmabuf);
    }

    #[test]
    fn intel_wayland_uses_ngl_and_allows_dmabuf() {
        let mut environment = env("Wayland");
        environment.gpu_intel = true;

        let recommendation = compute_recommendation(&environment);

        assert_eq!(recommendation.gsk_renderer.as_deref(), Some("ngl"));
        assert!(!recommendation.disable_dmabuf);
    }

    #[test]
    fn unknown_gpu_keeps_safe_defaults() {
        let environment = env("X11");

        let recommendation = compute_recommendation(&environment);

        assert!(recommendation.hardware_acceleration);
        assert!(!recommendation.force_x11);
        assert_eq!(recommendation.gsk_renderer, None);
        assert!(!recommendation.disable_dmabuf);
        assert!(!recommendation.disable_blur_background);
    }

    #[test]
    fn gpu_name_combines_hybrid_vendors() {
        let name = detect_gpu_name(true, false, true);

        assert!(name.contains("NVIDIA"));
        assert!(name.contains("Intel"));
        assert!(name.contains(" + "));
    }
}
