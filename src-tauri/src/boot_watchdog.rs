//! Boot watchdog for graphics settings that can crash WebKit before it
//! paints a single frame (HW acceleration, DMA-BUF, preferred-GPU
//! selection that picks an unreachable stack).
//!
//! Flow:
//!   1. `before_webkit_init()` is called at startup with the risky
//!      settings of the current boot. It writes a `pending.json` marker
//!      to `<data_dir>/qbz/boot_state/` recording which settings were
//!      attempted.
//!   2. After WebKit boots and the frontend signals first-paint (via
//!      `v2_mark_boot_succeeded`), `mark_boot_succeeded()` removes the
//!      pending marker and writes `last-good.json` carrying the
//!      successful settings.
//!   3. On the NEXT boot, `before_webkit_init` first calls
//!      `check_previous_boot()`. If a pending marker exists without a
//!      matching success, the previous boot crashed during graphics
//!      init — we auto-revert the offending setting(s), record a
//!      crash recovery flag, and only THEN write the new pending
//!      marker for the current boot.
//!
//! Two consecutive crashes with the same setting promote to "lockout"
//! state: the setting stays disabled until the user explicitly clears
//! the flag from Settings. Prevents an infinite retry loop.

use qbz_app::boot_watchdog::{
    clear_crash_flag_value, mark_boot_success_flags, resolve_boot_attempt,
};
pub use qbz_app::boot_watchdog::{BootAttempt, CrashFlags, WatchdogResolution};
use std::fs;
use std::path::PathBuf;

const STATE_DIR: &str = "boot_state";
const PENDING_FILE: &str = "pending.json";
const LAST_GOOD_FILE: &str = "last-good.json";
const CRASH_FLAGS_FILE: &str = "crash-flags.json";

fn state_dir() -> Option<PathBuf> {
    let base = dirs::data_dir()?.join("qbz").join(STATE_DIR);
    fs::create_dir_all(&base).ok()?;
    Some(base)
}

fn read_pending() -> Option<BootAttempt> {
    let path = state_dir()?.join(PENDING_FILE);
    let text = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_pending(attempt: &BootAttempt) {
    let Some(dir) = state_dir() else {
        return;
    };
    if let Ok(text) = serde_json::to_string_pretty(attempt) {
        let _ = fs::write(dir.join(PENDING_FILE), text);
    }
}

fn clear_pending() {
    if let Some(dir) = state_dir() {
        let _ = fs::remove_file(dir.join(PENDING_FILE));
    }
}

fn write_last_good(attempt: &BootAttempt) {
    let Some(dir) = state_dir() else {
        return;
    };
    if let Ok(text) = serde_json::to_string_pretty(attempt) {
        let _ = fs::write(dir.join(LAST_GOOD_FILE), text);
    }
}

fn read_crash_flags() -> CrashFlags {
    let Some(dir) = state_dir() else {
        return CrashFlags::default();
    };
    let path = dir.join(CRASH_FLAGS_FILE);
    let Ok(text) = fs::read_to_string(&path) else {
        return CrashFlags::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn write_crash_flags(flags: &CrashFlags) {
    let Some(dir) = state_dir() else {
        return;
    };
    if let Ok(text) = serde_json::to_string_pretty(flags) {
        let _ = fs::write(dir.join(CRASH_FLAGS_FILE), text);
    }
}

/// Called at startup with the settings the current boot WOULD apply if
/// nothing crashed. Returns the resolved settings to actually use —
/// usually the same input, but with risky knobs reverted if the
/// previous boot left a pending marker.
///
/// Also writes the new pending marker for THIS boot so a crash before
/// the frontend signals success is detectable on the next launch.
pub fn before_webkit_init(intended: BootAttempt) -> WatchdogResolution {
    let decision = resolve_boot_attempt(intended.clone(), read_pending(), read_crash_flags());
    if decision.should_persist_reverts {
        persist_reverts_to_db(&decision.resolved_attempt, &intended);
    }

    write_crash_flags(&decision.crash_flags);
    write_pending(&decision.resolved_attempt);
    decision.resolution()
}

/// Called from a Tauri command after the frontend signals first paint.
/// Removes the pending marker so the next launch doesn't think this
/// boot crashed; writes last-good as the new baseline; clears
/// consecutive-failure counters for whatever settings were active.
pub fn mark_boot_succeeded() {
    // Read what we attempted in this boot — the pending file is the
    // source of truth for "what was active when WebKit first paint
    // happened".
    let attempt = read_pending().unwrap_or_default();
    clear_pending();
    write_last_good(&attempt);

    let flags = mark_boot_success_flags(&attempt, read_crash_flags());
    write_crash_flags(&flags);
}

/// Read-only view of the crash flags for the Settings UI.
pub fn get_crash_flags() -> CrashFlags {
    read_crash_flags()
}

/// Clear a single crash recovery flag by name. Frontend uses this when
/// the user clicks "Try again" / "I want to re-enable this" on the
/// recovery banner. Unknown names are no-ops.
pub fn clear_crash_flag(flag: &str) {
    if let Some(flags) = clear_crash_flag_value(read_crash_flags(), flag) {
        write_crash_flags(&flags);
    }
}

/// Best-effort write of reverted values back to graphics_settings.db
/// and developer_settings.db. Failure is non-fatal — the runtime
/// values for THIS boot are still the reverted ones (we pass them
/// back to main.rs via WatchdogResolution), and the next boot's read
/// will see the reverted DB rows too.
fn persist_reverts_to_db(resolved: &BootAttempt, intended: &BootAttempt) {
    if resolved.hardware_acceleration != intended.hardware_acceleration {
        if let Ok(store) = crate::config::graphics_settings::GraphicsSettingsStore::new() {
            let _ = store.set_hardware_acceleration(resolved.hardware_acceleration);
        }
    }
    if resolved.force_dmabuf != intended.force_dmabuf {
        if let Ok(store) = crate::config::developer_settings::DeveloperSettingsStore::new() {
            let _ = store.set_force_dmabuf(resolved.force_dmabuf);
        }
    }
    if resolved.preferred_gpu != intended.preferred_gpu {
        if let Ok(store) = crate::config::graphics_settings::GraphicsSettingsStore::new() {
            let _ = store.set_preferred_gpu(&resolved.preferred_gpu);
        }
    }
}
