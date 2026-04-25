//! System capability detection.
//!
//! Probes the host environment at startup to derive a runtime profile that
//! tunes resource-heavy behaviors (prefetch depth, streaming buffer size,
//! prefetch quality cap) for memory-constrained machines like the
//! Raspberry Pi 3B (1 GB RAM, issue #331).
//!
//! Detection is one-shot, cached in a `OnceLock`, and pure once given the
//! `/proc/meminfo` contents — making it trivial to test by passing
//! synthetic input.

use std::sync::OnceLock;

/// Memory class bucket the runtime adapts behavior to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryClass {
    /// >= 2 GB RAM. Default behavior, no caps applied.
    Normal,
    /// < 2 GB RAM. Reduces prefetch and buffer footprints to keep room
    /// for the WebView and avoid swap thrash on Raspberry Pi-class
    /// devices.
    LowMemory,
}

/// Derived runtime profile applied to memory-sensitive subsystems.
#[derive(Debug, Clone, Copy)]
pub struct MemoryProfile {
    pub class: MemoryClass,
    pub mem_total_kb: u64,
    /// How many upcoming Qobuz tracks to prefetch. Hi-Res tracks are
    /// ~60 MB each held in memory, so this is the dominant source of
    /// RSS growth during normal playback.
    pub prefetch_count: usize,
    /// Maximum allowed initial streaming buffer size in bytes. Caps the
    /// dynamic-buffer growth that `from_speed_mbps` would otherwise
    /// inflate to 2 MB on slow connections — exactly the wrong direction
    /// on a memory-pressured Pi where slow downloads are themselves a
    /// symptom of swap thrash.
    pub max_initial_buffer_bytes: usize,
    /// Concurrency cap for prefetch downloads.
    pub max_concurrent_prefetch: usize,
    /// When false, prefetch downgrades from HiRes/UltraHiRes to Lossless
    /// (44.1 kHz / 16-bit FLAC) so each cached track stays under ~15 MB
    /// instead of ~60 MB.
    pub allow_hires_prefetch: bool,
}

impl MemoryProfile {
    /// Derive the profile from a total-memory figure (KB).
    fn from_total_kb(mem_total_kb: u64) -> Self {
        // Threshold: 2 GiB. Anything with at least 2 GB physical RAM is
        // assumed to have enough headroom for the WebView (~150 MB) plus
        // 5 cached HiRes tracks (~300 MB) plus typical app overhead.
        const NORMAL_FLOOR_KB: u64 = 2 * 1024 * 1024;

        if mem_total_kb >= NORMAL_FLOOR_KB {
            Self {
                class: MemoryClass::Normal,
                mem_total_kb,
                prefetch_count: 5,
                max_initial_buffer_bytes: 2 * 1024 * 1024,
                max_concurrent_prefetch: 2,
                allow_hires_prefetch: true,
            }
        } else {
            Self {
                class: MemoryClass::LowMemory,
                mem_total_kb,
                prefetch_count: 1,
                max_initial_buffer_bytes: 256 * 1024,
                max_concurrent_prefetch: 1,
                allow_hires_prefetch: false,
            }
        }
    }
}

/// Parse the `MemTotal:` line out of `/proc/meminfo` content.
/// Returns None if the field is missing or unparseable.
pub fn parse_meminfo_total_kb(content: &str) -> Option<u64> {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            // Format: "MemTotal:         938196 kB"
            let kb_str = rest.split_whitespace().next()?;
            return kb_str.parse::<u64>().ok();
        }
    }
    None
}

/// Pure detection given `/proc/meminfo` content. Falls back to Normal
/// when MemTotal is missing or unparseable so we never accidentally
/// throttle a system whose meminfo we couldn't read.
pub fn detect_profile_from_meminfo(content: &str) -> MemoryProfile {
    parse_meminfo_total_kb(content)
        .map(MemoryProfile::from_total_kb)
        .unwrap_or_else(|| MemoryProfile::from_total_kb(u64::MAX))
}

/// Read `/proc/meminfo` and derive the profile. Returns the Normal-fallback
/// profile on platforms without `/proc/meminfo` (macOS, Windows) or when
/// the file is unreadable for any reason.
fn detect_profile() -> MemoryProfile {
    match std::fs::read_to_string("/proc/meminfo") {
        Ok(content) => detect_profile_from_meminfo(&content),
        Err(_) => MemoryProfile::from_total_kb(u64::MAX),
    }
}

/// Process-wide cached profile. Detection runs once on first access.
static PROFILE: OnceLock<MemoryProfile> = OnceLock::new();

/// Return the cached memory profile, running detection on first call.
/// Logs the resolved profile at info level on the initial detection.
pub fn memory_profile() -> &'static MemoryProfile {
    PROFILE.get_or_init(|| {
        let profile = detect_profile();
        match profile.class {
            MemoryClass::LowMemory => {
                log::info!(
                    "[system] Low-memory profile active: {} MB total RAM, prefetch={}, max_initial_buffer={}KB, hires_prefetch=disabled",
                    profile.mem_total_kb / 1024,
                    profile.prefetch_count,
                    profile.max_initial_buffer_bytes / 1024,
                );
            }
            MemoryClass::Normal => {
                log::info!(
                    "[system] Normal memory profile: {} MB total RAM",
                    profile.mem_total_kb / 1024
                );
            }
        }
        profile
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_meminfo_total_kb_extracts_value() {
        let sample = "\
MemTotal:         938196 kB
MemFree:          120000 kB
Buffers:           48000 kB
";
        assert_eq!(parse_meminfo_total_kb(sample), Some(938196));
    }

    #[test]
    fn parse_meminfo_total_kb_ignores_other_fields() {
        let sample = "\
MemFree:          120000 kB
MemTotal:        4194304 kB
SwapTotal:       2097152 kB
";
        assert_eq!(parse_meminfo_total_kb(sample), Some(4194304));
    }

    #[test]
    fn parse_meminfo_total_kb_handles_missing_field() {
        let sample = "\
MemFree:          120000 kB
SwapTotal:       2097152 kB
";
        assert_eq!(parse_meminfo_total_kb(sample), None);
    }

    #[test]
    fn parse_meminfo_total_kb_handles_empty_input() {
        assert_eq!(parse_meminfo_total_kb(""), None);
    }

    #[test]
    fn pi3b_with_1gb_resolves_to_low_memory() {
        // Raspberry Pi 3B = 1 GB RAM = ~938196 kB after kernel reservations.
        let profile = MemoryProfile::from_total_kb(938196);
        assert_eq!(profile.class, MemoryClass::LowMemory);
        assert_eq!(profile.prefetch_count, 1);
        assert_eq!(profile.max_concurrent_prefetch, 1);
        assert!(!profile.allow_hires_prefetch);
        assert!(profile.max_initial_buffer_bytes <= 256 * 1024);
    }

    #[test]
    fn pi_zero_2w_512mb_resolves_to_low_memory() {
        let profile = MemoryProfile::from_total_kb(500 * 1024);
        assert_eq!(profile.class, MemoryClass::LowMemory);
    }

    #[test]
    fn machine_with_2gb_resolves_to_normal() {
        // Exactly the threshold — Normal (>= NORMAL_FLOOR_KB).
        let profile = MemoryProfile::from_total_kb(2 * 1024 * 1024);
        assert_eq!(profile.class, MemoryClass::Normal);
        assert_eq!(profile.prefetch_count, 5);
        assert!(profile.allow_hires_prefetch);
    }

    #[test]
    fn machine_with_just_under_2gb_resolves_to_low_memory() {
        let profile = MemoryProfile::from_total_kb(2 * 1024 * 1024 - 1);
        assert_eq!(profile.class, MemoryClass::LowMemory);
    }

    #[test]
    fn detect_profile_from_meminfo_falls_back_to_normal_when_unparseable() {
        let profile = detect_profile_from_meminfo("garbage\nno memtotal here\n");
        assert_eq!(profile.class, MemoryClass::Normal);
    }

    #[test]
    fn detect_profile_from_meminfo_returns_low_memory_for_pi() {
        let pi_meminfo = "\
MemTotal:         938196 kB
MemFree:          250000 kB
";
        let profile = detect_profile_from_meminfo(pi_meminfo);
        assert_eq!(profile.class, MemoryClass::LowMemory);
        assert_eq!(profile.mem_total_kb, 938196);
    }
}
