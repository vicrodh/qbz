//! Framework-agnostic boot watchdog decision logic.
//!
//! The Tauri shell owns marker-file IO, settings persistence, and startup env
//! mutation. This module owns the pure crash-recovery state transition that can
//! be reused by any future native shell.

use serde::{Deserialize, Serialize};

/// Snapshot of the risky settings attempted in a boot. Compared against the
/// previous boot's snapshot to decide which setting "caused" a crash.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootAttempt {
    pub hardware_acceleration: bool,
    pub force_dmabuf: bool,
    pub preferred_gpu: String,
    pub force_x11: bool,
}

/// Sticky flags written when a setting was auto-reverted after a crash.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrashFlags {
    /// Last boot crashed with hardware_acceleration ON; we reverted it.
    pub hardware_acceleration_disabled: bool,
    pub hardware_acceleration_consecutive_failures: u8,
    /// Same for force_dmabuf.
    pub force_dmabuf_disabled: bool,
    pub force_dmabuf_consecutive_failures: u8,
    /// Same for preferred_gpu (reset to "auto").
    pub preferred_gpu_disabled: bool,
    pub preferred_gpu_consecutive_failures: u8,
    /// Lockout - set when consecutive_failures >= 2 for a given knob.
    pub hardware_acceleration_locked: bool,
    pub force_dmabuf_locked: bool,
    pub preferred_gpu_locked: bool,
}

/// Resolved settings the host shell should actually use for this boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchdogResolution {
    pub hardware_acceleration: bool,
    pub force_dmabuf: bool,
    pub preferred_gpu: String,
    /// Reverted-in-this-boot messages so the UI knows which setting was
    /// just rolled back. Persisted in CrashFlags as well.
    pub recovery_messages: Vec<String>,
}

/// Pure output for a boot watchdog decision. The host adapter decides how to
/// persist marker files and settings writes from this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootWatchdogDecision {
    pub resolved_attempt: BootAttempt,
    pub crash_flags: CrashFlags,
    pub recovery_messages: Vec<String>,
    pub should_persist_reverts: bool,
}

impl BootWatchdogDecision {
    pub fn resolution(&self) -> WatchdogResolution {
        WatchdogResolution {
            hardware_acceleration: self.resolved_attempt.hardware_acceleration,
            force_dmabuf: self.resolved_attempt.force_dmabuf,
            preferred_gpu: self.resolved_attempt.preferred_gpu.clone(),
            recovery_messages: self.recovery_messages.clone(),
        }
    }
}

pub fn resolve_boot_attempt(
    intended: BootAttempt,
    pending: Option<BootAttempt>,
    mut flags: CrashFlags,
) -> BootWatchdogDecision {
    let mut resolved = intended.clone();
    let mut messages: Vec<String> = Vec::new();
    let mut any_revert = false;

    if let Some(prev) = pending {
        if prev.hardware_acceleration && resolved.hardware_acceleration {
            flags.hardware_acceleration_consecutive_failures = flags
                .hardware_acceleration_consecutive_failures
                .saturating_add(1);
            resolved.hardware_acceleration = false;
            flags.hardware_acceleration_disabled = true;
            if flags.hardware_acceleration_consecutive_failures >= 2 {
                flags.hardware_acceleration_locked = true;
            }
            messages.push(
                "Hardware acceleration was disabled because the previous launch crashed during \
                 graphics init."
                    .to_string(),
            );
            any_revert = true;
        }

        if prev.force_dmabuf && resolved.force_dmabuf {
            flags.force_dmabuf_consecutive_failures =
                flags.force_dmabuf_consecutive_failures.saturating_add(1);
            resolved.force_dmabuf = false;
            flags.force_dmabuf_disabled = true;
            if flags.force_dmabuf_consecutive_failures >= 2 {
                flags.force_dmabuf_locked = true;
            }
            messages.push(
                "DMA-BUF renderer was disabled because the previous launch crashed during \
                 graphics init."
                    .to_string(),
            );
            any_revert = true;
        }

        if prev.preferred_gpu == resolved.preferred_gpu && resolved.preferred_gpu != "auto" {
            flags.preferred_gpu_consecutive_failures =
                flags.preferred_gpu_consecutive_failures.saturating_add(1);
            resolved.preferred_gpu = "auto".to_string();
            flags.preferred_gpu_disabled = true;
            if flags.preferred_gpu_consecutive_failures >= 2 {
                flags.preferred_gpu_locked = true;
            }
            messages.push(format!(
                "Rendering GPU preference was reset to Auto because pinning to {:?} crashed the \
                 previous launch.",
                prev.preferred_gpu
            ));
            any_revert = true;
        }

        if any_revert {
            if resolved.hardware_acceleration {
                resolved.hardware_acceleration = false;
                flags.hardware_acceleration_disabled = true;
                messages.push(
                    "Hardware acceleration was also disabled as a safety fallback to land in \
                     CPU mode."
                        .to_string(),
                );
            }
            if resolved.force_dmabuf {
                resolved.force_dmabuf = false;
                flags.force_dmabuf_disabled = true;
            }
            if resolved.preferred_gpu != "auto" {
                resolved.preferred_gpu = "auto".to_string();
                flags.preferred_gpu_disabled = true;
            }
        }
    } else {
        if !flags.hardware_acceleration_locked {
            flags.hardware_acceleration_disabled = false;
            flags.hardware_acceleration_consecutive_failures = 0;
        }
        if !flags.force_dmabuf_locked {
            flags.force_dmabuf_disabled = false;
            flags.force_dmabuf_consecutive_failures = 0;
        }
        if !flags.preferred_gpu_locked {
            flags.preferred_gpu_disabled = false;
            flags.preferred_gpu_consecutive_failures = 0;
        }
    }

    let should_persist_reverts = resolved != intended;
    BootWatchdogDecision {
        resolved_attempt: resolved,
        crash_flags: flags,
        recovery_messages: messages,
        should_persist_reverts,
    }
}

pub fn mark_boot_success_flags(attempt: &BootAttempt, mut flags: CrashFlags) -> CrashFlags {
    if attempt.hardware_acceleration {
        flags.hardware_acceleration_consecutive_failures = 0;
        flags.hardware_acceleration_disabled = false;
        flags.hardware_acceleration_locked = false;
    }
    if attempt.force_dmabuf {
        flags.force_dmabuf_consecutive_failures = 0;
        flags.force_dmabuf_disabled = false;
        flags.force_dmabuf_locked = false;
    }
    if attempt.preferred_gpu != "auto" {
        flags.preferred_gpu_consecutive_failures = 0;
        flags.preferred_gpu_disabled = false;
        flags.preferred_gpu_locked = false;
    }
    flags
}

pub fn clear_crash_flag_value(mut flags: CrashFlags, flag: &str) -> Option<CrashFlags> {
    match flag {
        "hardware_acceleration" => {
            flags.hardware_acceleration_disabled = false;
            flags.hardware_acceleration_consecutive_failures = 0;
            flags.hardware_acceleration_locked = false;
        }
        "force_dmabuf" => {
            flags.force_dmabuf_disabled = false;
            flags.force_dmabuf_consecutive_failures = 0;
            flags.force_dmabuf_locked = false;
        }
        "preferred_gpu" => {
            flags.preferred_gpu_disabled = false;
            flags.preferred_gpu_consecutive_failures = 0;
            flags.preferred_gpu_locked = false;
        }
        _ => return None,
    }
    Some(flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt() -> BootAttempt {
        BootAttempt {
            hardware_acceleration: true,
            force_dmabuf: true,
            preferred_gpu: "auto".to_string(),
            force_x11: false,
        }
    }

    #[test]
    fn no_pending_returns_intended_and_clears_non_locked_flags() {
        let flags = CrashFlags {
            hardware_acceleration_disabled: true,
            hardware_acceleration_consecutive_failures: 1,
            force_dmabuf_disabled: true,
            force_dmabuf_consecutive_failures: 1,
            preferred_gpu_disabled: true,
            preferred_gpu_consecutive_failures: 1,
            ..CrashFlags::default()
        };

        let decision = resolve_boot_attempt(attempt(), None, flags);

        assert_eq!(decision.resolved_attempt, attempt());
        assert!(!decision.should_persist_reverts);
        assert_eq!(decision.crash_flags, CrashFlags::default());
    }

    #[test]
    fn no_pending_preserves_locked_flags() {
        let flags = CrashFlags {
            hardware_acceleration_disabled: true,
            hardware_acceleration_consecutive_failures: 2,
            hardware_acceleration_locked: true,
            ..CrashFlags::default()
        };

        let decision = resolve_boot_attempt(attempt(), None, flags);

        assert!(decision.crash_flags.hardware_acceleration_disabled);
        assert_eq!(
            decision
                .crash_flags
                .hardware_acceleration_consecutive_failures,
            2
        );
        assert!(decision.crash_flags.hardware_acceleration_locked);
    }

    #[test]
    fn pending_hardware_acceleration_crash_disables_hardware_acceleration() {
        let decision = resolve_boot_attempt(attempt(), Some(attempt()), CrashFlags::default());

        assert!(!decision.resolved_attempt.hardware_acceleration);
        assert!(!decision.resolved_attempt.force_dmabuf);
        assert_eq!(decision.resolved_attempt.preferred_gpu, "auto");
        assert!(decision.crash_flags.hardware_acceleration_disabled);
        assert_eq!(
            decision
                .crash_flags
                .hardware_acceleration_consecutive_failures,
            1
        );
        assert!(decision.should_persist_reverts);
    }

    #[test]
    fn pending_dmabuf_crash_disables_dmabuf() {
        let mut intended = attempt();
        intended.hardware_acceleration = false;
        let pending = intended.clone();

        let decision = resolve_boot_attempt(intended, Some(pending), CrashFlags::default());

        assert!(!decision.resolved_attempt.force_dmabuf);
        assert!(decision.crash_flags.force_dmabuf_disabled);
        assert_eq!(decision.crash_flags.force_dmabuf_consecutive_failures, 1);
        assert!(decision.should_persist_reverts);
    }

    #[test]
    fn pending_same_preferred_gpu_resets_to_auto() {
        let mut intended = attempt();
        intended.hardware_acceleration = false;
        intended.force_dmabuf = false;
        intended.preferred_gpu = "0000:01:00.0".to_string();
        let pending = intended.clone();

        let decision = resolve_boot_attempt(intended, Some(pending), CrashFlags::default());

        assert_eq!(decision.resolved_attempt.preferred_gpu, "auto");
        assert!(decision.crash_flags.preferred_gpu_disabled);
        assert_eq!(decision.crash_flags.preferred_gpu_consecutive_failures, 1);
        assert!(decision.should_persist_reverts);
    }

    #[test]
    fn pending_different_preferred_gpu_does_not_blame_gpu_choice() {
        let mut intended = attempt();
        intended.hardware_acceleration = false;
        intended.force_dmabuf = false;
        intended.preferred_gpu = "0000:02:00.0".to_string();
        let mut pending = intended.clone();
        pending.preferred_gpu = "0000:01:00.0".to_string();

        let decision = resolve_boot_attempt(intended.clone(), Some(pending), CrashFlags::default());

        assert_eq!(decision.resolved_attempt, intended);
        assert!(!decision.crash_flags.preferred_gpu_disabled);
        assert!(!decision.should_persist_reverts);
    }

    #[test]
    fn second_failure_locks_setting() {
        let flags = CrashFlags {
            hardware_acceleration_consecutive_failures: 1,
            ..CrashFlags::default()
        };

        let decision = resolve_boot_attempt(attempt(), Some(attempt()), flags);

        assert_eq!(
            decision
                .crash_flags
                .hardware_acceleration_consecutive_failures,
            2
        );
        assert!(decision.crash_flags.hardware_acceleration_locked);
    }

    #[test]
    fn success_with_enabled_knobs_clears_matching_flags() {
        let flags = CrashFlags {
            hardware_acceleration_disabled: true,
            hardware_acceleration_consecutive_failures: 2,
            hardware_acceleration_locked: true,
            force_dmabuf_disabled: true,
            force_dmabuf_consecutive_failures: 2,
            force_dmabuf_locked: true,
            preferred_gpu_disabled: true,
            preferred_gpu_consecutive_failures: 2,
            preferred_gpu_locked: true,
        };
        let mut boot = attempt();
        boot.preferred_gpu = "0000:01:00.0".to_string();

        let updated = mark_boot_success_flags(&boot, flags);

        assert_eq!(updated, CrashFlags::default());
    }

    #[test]
    fn success_with_cpu_fallback_does_not_clear_disabled_flags() {
        let flags = CrashFlags {
            hardware_acceleration_disabled: true,
            hardware_acceleration_consecutive_failures: 1,
            force_dmabuf_disabled: true,
            force_dmabuf_consecutive_failures: 1,
            preferred_gpu_disabled: true,
            preferred_gpu_consecutive_failures: 1,
            ..CrashFlags::default()
        };
        let boot = BootAttempt {
            hardware_acceleration: false,
            force_dmabuf: false,
            preferred_gpu: "auto".to_string(),
            force_x11: false,
        };

        let updated = mark_boot_success_flags(&boot, flags.clone());

        assert_eq!(updated, flags);
    }

    #[test]
    fn clear_known_flag_resets_disabled_counter_and_lock() {
        let flags = CrashFlags {
            force_dmabuf_disabled: true,
            force_dmabuf_consecutive_failures: 2,
            force_dmabuf_locked: true,
            ..CrashFlags::default()
        };

        let updated = clear_crash_flag_value(flags, "force_dmabuf").expect("known flag");

        assert!(!updated.force_dmabuf_disabled);
        assert_eq!(updated.force_dmabuf_consecutive_failures, 0);
        assert!(!updated.force_dmabuf_locked);
    }

    #[test]
    fn clear_unknown_flag_is_noop() {
        assert_eq!(
            clear_crash_flag_value(CrashFlags::default(), "force_x11"),
            None
        );
    }
}
