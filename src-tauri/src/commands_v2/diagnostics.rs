use tauri::State;

use crate::config::audio_settings::AudioSettingsState;
use crate::config::developer_settings::DeveloperSettingsState;
use crate::config::graphics_settings::GraphicsSettingsState;
use crate::runtime::RuntimeError;

// ==================== Utility Commands ====================

/// Fetch a remote URL as bytes (bypasses WebView CORS restrictions).
/// Used for loading PDF booklets from Qobuz CDN.
#[tauri::command]
pub async fn v2_fetch_url_bytes(url: String) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch URL: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}: {}", response.status(), url));
    }

    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("Failed to read response: {}", e))
}

// ==================== Runtime Diagnostics ====================

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostics {
    // Audio: saved settings
    pub audio_output_device: Option<String>,
    pub audio_backend_type: Option<String>,
    pub audio_exclusive_mode: bool,
    pub audio_dac_passthrough: bool,
    pub audio_preferred_sample_rate: Option<u32>,
    pub audio_alsa_plugin: Option<String>,
    pub audio_alsa_hardware_volume: bool,
    pub audio_normalization_enabled: bool,
    pub audio_normalization_target_lufs: f32,
    pub audio_gapless_enabled: bool,
    pub audio_pw_force_bitperfect: bool,
    pub audio_stream_buffer_seconds: u8,
    pub audio_streaming_only: bool,

    // Graphics: saved settings
    pub gfx_hardware_acceleration: bool,
    pub gfx_force_x11: bool,
    pub gfx_gdk_scale: Option<String>,
    pub gfx_gdk_dpi_scale: Option<String>,
    pub gfx_gsk_renderer: Option<String>,

    // Graphics: runtime (what actually applied at startup)
    pub runtime_using_fallback: bool,
    pub runtime_is_wayland: bool,
    pub runtime_has_nvidia: bool,
    pub runtime_has_amd: bool,
    pub runtime_has_intel: bool,
    pub runtime_is_vm: bool,
    pub runtime_hw_accel_enabled: bool,
    pub runtime_force_x11_active: bool,

    // Developer settings
    pub dev_force_dmabuf: bool,

    // Environment variables (what WebKit actually sees)
    pub env_webkit_disable_dmabuf: Option<String>,
    pub env_webkit_disable_compositing: Option<String>,
    pub env_gdk_backend: Option<String>,
    pub env_gsk_renderer: Option<String>,
    pub env_libgl_always_software: Option<String>,
    pub env_wayland_display: Option<String>,
    pub env_xdg_session_type: Option<String>,

    // App info
    pub app_version: String,
}

#[tauri::command]
pub fn v2_get_runtime_diagnostics(
    audio_state: State<'_, AudioSettingsState>,
    graphics_state: State<'_, GraphicsSettingsState>,
    developer_state: State<'_, DeveloperSettingsState>,
) -> Result<RuntimeDiagnostics, RuntimeError> {
    // Audio settings (may not be available before login)
    let audio = audio_state
        .store
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()));

    // Graphics settings
    let gfx = graphics_state
        .store
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()));

    // Graphics runtime status (static atomics — always available)
    let gfx_status = crate::config::graphics_settings::get_graphics_startup_status();

    // Developer settings
    let dev = developer_state
        .store
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()));

    let env_var = |name: &str| std::env::var(name).ok();

    let audio_defaults = crate::config::audio_settings::AudioSettings::default();
    let audio = audio.unwrap_or(audio_defaults);
    let gfx = gfx.unwrap_or_default();
    let dev = dev.unwrap_or_default();

    Ok(RuntimeDiagnostics {
        audio_output_device: audio.output_device,
        audio_backend_type: audio.backend_type.map(|b| format!("{:?}", b)),
        audio_exclusive_mode: audio.exclusive_mode,
        audio_dac_passthrough: audio.dac_passthrough,
        audio_preferred_sample_rate: audio.preferred_sample_rate,
        audio_alsa_plugin: audio.alsa_plugin.map(|p| format!("{:?}", p)),
        audio_alsa_hardware_volume: audio.alsa_hardware_volume,
        audio_normalization_enabled: audio.normalization_enabled,
        audio_normalization_target_lufs: audio.normalization_target_lufs,
        audio_gapless_enabled: audio.gapless_enabled,
        audio_pw_force_bitperfect: audio.pw_force_bitperfect,
        audio_stream_buffer_seconds: audio.stream_buffer_seconds,
        audio_streaming_only: audio.streaming_only,

        gfx_hardware_acceleration: gfx.hardware_acceleration,
        gfx_force_x11: gfx.force_x11,
        gfx_gdk_scale: gfx.gdk_scale,
        gfx_gdk_dpi_scale: gfx.gdk_dpi_scale,
        gfx_gsk_renderer: gfx.gsk_renderer,

        runtime_using_fallback: gfx_status.using_fallback,
        runtime_is_wayland: gfx_status.is_wayland,
        runtime_has_nvidia: gfx_status.has_nvidia,
        runtime_has_amd: gfx_status.has_amd,
        runtime_has_intel: gfx_status.has_intel,
        runtime_is_vm: gfx_status.is_vm,
        runtime_hw_accel_enabled: gfx_status.hardware_accel_enabled,
        runtime_force_x11_active: gfx_status.force_x11_active,

        dev_force_dmabuf: dev.force_dmabuf,

        env_webkit_disable_dmabuf: env_var("WEBKIT_DISABLE_DMABUF_RENDERER"),
        env_webkit_disable_compositing: env_var("WEBKIT_DISABLE_COMPOSITING_MODE"),
        env_gdk_backend: env_var("GDK_BACKEND"),
        env_gsk_renderer: env_var("GSK_RENDERER"),
        env_libgl_always_software: env_var("LIBGL_ALWAYS_SOFTWARE"),
        env_wayland_display: env_var("WAYLAND_DISPLAY"),
        env_xdg_session_type: env_var("XDG_SESSION_TYPE"),

        app_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// ==================== System Info ====================

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub kernel_version: Option<String>,
    pub distro_id: Option<String>,
    pub distro_version_id: Option<String>,
    pub distro_pretty_name: Option<String>,
    pub install_method: String,
    pub flatpak_runtime: Option<String>,
    pub flatpak_runtime_version: Option<String>,
    pub webkit2gtk_version: Option<String>,
    pub gtk_version: Option<String>,
    pub glibc_version: Option<String>,
    pub alsa_version: Option<String>,
    pub pipewire_version: Option<String>,
    pub pulseaudio_version: Option<String>,
}

/// Parse /etc/os-release (or the Flatpak host equivalent) into key/value pairs.
fn read_os_release() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    // Try Flatpak-exposed host file first, then the normal path.
    let candidates = ["/run/host/os-release", "/etc/os-release"];
    for path in candidates {
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                if let Some(idx) = line.find('=') {
                    let key = line[..idx].trim().to_string();
                    let mut value = line[idx + 1..].trim().to_string();
                    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                        value = value[1..value.len() - 1].to_string();
                    }
                    map.insert(key, value);
                }
            }
            if !map.is_empty() {
                return map;
            }
        }
    }
    map
}

fn detect_kernel_version() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|s| s.trim().to_string())
}

fn detect_install_method() -> (String, Option<String>, Option<String>) {
    // (method, flatpak_runtime, flatpak_runtime_version)
    if std::env::var("FLATPAK_ID").is_ok() || std::path::Path::new("/.flatpak-info").exists() {
        let mut runtime = None;
        let mut runtime_version = None;
        if let Ok(text) = std::fs::read_to_string("/.flatpak-info") {
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("runtime=") {
                    let v = value.trim();
                    if let Some(slash) = v.rfind('/') {
                        runtime = Some(v[..slash].to_string());
                        runtime_version = Some(v[slash + 1..].to_string());
                    } else {
                        runtime = Some(v.to_string());
                    }
                }
            }
        }
        return ("flatpak".to_string(), runtime, runtime_version);
    }
    if std::env::var("SNAP").is_ok() {
        return ("snap".to_string(), None, None);
    }
    if std::env::var("APPIMAGE").is_ok() {
        return ("appimage".to_string(), None, None);
    }
    if cfg!(debug_assertions) {
        return ("dev".to_string(), None, None);
    }
    ("native".to_string(), None, None)
}

/// Extract the best-available version string from the filename of a shared
/// library loaded by the current process. Looks for patterns like
/// `libfoo.so.0.15.7` → `0.15.7`, or `libfoo.so.2` → `2`.
/// Returns `None` if the library isn't mapped.
fn detect_loaded_lib_version(lib_name_stem: &str) -> Option<String> {
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    let mut best: Option<String> = None;
    for line in maps.lines() {
        // Last column is the path (may contain spaces, very rare).
        let path = line.splitn(6, ' ').nth(5).unwrap_or("").trim();
        if path.is_empty() {
            continue;
        }
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if !filename.starts_with(lib_name_stem) {
            continue;
        }
        // filename example: "libwebkit2gtk-4.1.so.0.15.7"
        // Strip the "lib_name_stem.so" prefix and leading dot.
        let tail = match filename.split_once(".so") {
            Some((_, rest)) => rest.trim_start_matches('.'),
            None => continue,
        };
        if tail.is_empty() {
            continue;
        }
        // Resolve symlink target if possible — often the real file carries
        // a fuller version than the SONAME alias.
        if let Ok(real) = std::fs::canonicalize(path) {
            if let Some(real_name) = real.file_name().and_then(|s| s.to_str()) {
                if let Some((_, rest)) = real_name.split_once(".so") {
                    let real_tail = rest.trim_start_matches('.');
                    if !real_tail.is_empty() {
                        best = Some(real_tail.to_string());
                        continue;
                    }
                }
            }
        }
        best.get_or_insert_with(|| tail.to_string());
    }
    best
}

#[tauri::command]
pub fn v2_get_system_info() -> Result<SystemInfo, RuntimeError> {
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let (install_method, flatpak_runtime, flatpak_runtime_version) = detect_install_method();
    let osr = read_os_release();

    Ok(SystemInfo {
        os,
        arch,
        kernel_version: detect_kernel_version(),
        distro_id: osr.get("ID").cloned(),
        distro_version_id: osr.get("VERSION_ID").cloned(),
        distro_pretty_name: osr
            .get("PRETTY_NAME")
            .cloned()
            .or_else(|| osr.get("NAME").cloned()),
        install_method,
        flatpak_runtime,
        flatpak_runtime_version,
        // Runtime shared-library versions, parsed from /proc/self/maps.
        webkit2gtk_version: detect_loaded_lib_version("libwebkit2gtk-4.1"),
        gtk_version: detect_loaded_lib_version("libgtk-3")
            .or_else(|| detect_loaded_lib_version("libgtk-4")),
        glibc_version: detect_loaded_lib_version("libc"),
        alsa_version: detect_loaded_lib_version("libasound"),
        pipewire_version: detect_loaded_lib_version("libpipewire-0.3"),
        pulseaudio_version: detect_loaded_lib_version("libpulse"),
    })
}
