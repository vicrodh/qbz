//! V2 commands that expose the graphics auto-config recommendation to the
//! Settings UI. The detection + decision logic lives in
//! `autoconfig_graphics.rs` (shared with the `qbz --autoconfig-graphics` CLI
//! tool); this module is just the Tauri surface that the Graphics tab uses
//! to render the "Detected / Recommended" banner.

use qbz_app::graphics_autoconfig::{
    compute_recommendation, detect_environment, Environment, Recommendation,
};
use serde::Serialize;

/// Payload returned to the Settings UI. Splits environment from
/// recommendation so the frontend can show a human-readable detection
/// line and then compare the recommendation to the user's current
/// persisted settings to decide whether to surface the banner at all.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct GraphicsRecommendationPayload {
    pub environment: Environment,
    pub recommendation: Recommendation,
}

/// Compute the recommendation for the current host. Cheap — runs the
/// same detection that the CLI tool runs and synthesizes the matrix
/// decision. The Settings UI calls this on Graphics-tab mount.
#[tauri::command]
pub fn v2_get_graphics_recommendation() -> GraphicsRecommendationPayload {
    let environment = detect_environment();
    let recommendation = compute_recommendation(&environment);
    GraphicsRecommendationPayload {
        environment,
        recommendation,
    }
}

/// Payload for the rendering-GPU dropdown in Settings > Graphics.
/// `gpus` is the full list (physical PCI devices intersected with
/// available EGL vendor stacks, plus the Software fallback).
/// `auto_resolved_id` says which entry, if any, "Auto" currently pins
/// to on this system — frontend uses it to label the Auto option (e.g.
/// "Automatic (Intel Graphics)") so the user can see what default they
/// are getting without picking explicitly. `None` means Auto is a
/// no-op on this system (single GPU or no GPU detected).
#[derive(serde::Serialize)]
pub struct GpuList {
    pub gpus: Vec<crate::graphics_detection::DetectedGpu>,
    pub auto_resolved_id: Option<String>,
}

/// Enumerate detected GPUs for the rendering-GPU dropdown in
/// Settings > Graphics. Returns physical PCI devices intersected with
/// available EGL vendor stacks — entries with `is_usable = false`
/// should be hidden / disabled by the UI. Always includes a Software
/// fallback so the list is never empty.
#[tauri::command]
pub fn v2_enumerate_gpus() -> GpuList {
    let gpus = crate::graphics_detection::enumerate_gpus();
    let auto_resolved_id =
        crate::graphics_detection::auto_resolves_to(&gpus).map(|g| g.id.clone());
    GpuList {
        gpus,
        auto_resolved_id,
    }
}

/// Called by the frontend after first paint to clear the boot
/// watchdog's pending marker. If this is never called (because WebKit
/// crashed before painting), the next launch sees the marker and
/// auto-reverts the offending risky setting.
#[tauri::command]
pub fn v2_mark_boot_succeeded() {
    crate::boot_watchdog::mark_boot_succeeded();
}

/// Read the sticky crash flags so Settings > Graphics can render
/// the recovery banner. The flags say which risky setting was
/// auto-reverted in this boot, the consecutive-failure counter, and
/// whether the knob is in lockout state (2+ failures → user must
/// explicitly clear before it can be re-enabled).
#[tauri::command]
pub fn v2_get_crash_flags() -> crate::boot_watchdog::CrashFlags {
    crate::boot_watchdog::get_crash_flags()
}

/// User-driven clear of a single crash recovery flag. Called when the
/// user clicks "Try again" / "Re-enable" on the banner. Valid flag
/// names: "hardware_acceleration", "force_dmabuf", "preferred_gpu".
#[tauri::command]
pub fn v2_clear_crash_flag(flag: String) {
    crate::boot_watchdog::clear_crash_flag(&flag);
}
