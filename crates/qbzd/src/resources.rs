use crate::config::DaemonConfig;

/// Auto-detect system RAM and adjust cache configuration.
/// Only runs if cache.auto.enabled is true.
pub fn auto_detect_cache_config(config: &mut DaemonConfig) {
    let sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()),
    );

    let total_ram_mb = sys.total_memory() / (1024 * 1024);

    log::info!("[qbzd] System RAM: {} MB", total_ram_mb);

    if config.cache.auto.enabled && config.cache.memory_mb == 0 {
        // Conservative: 1/8 of RAM, capped at 400 MB, minimum 50 MB
        let auto_memory = (total_ram_mb / 8).min(400).max(50) as usize;
        config.cache.memory_mb = auto_memory;
        log::info!("[qbzd] Auto-detected L1 cache: {} MB", auto_memory);
    }

    // Adjust prefetch for low-memory devices
    if total_ram_mb <= 1024 && config.cache.prefetch_count > 2 {
        config.cache.prefetch_count = 2;
        config.cache.prefetch_concurrent = 1;
        config.cache.cmaf_concurrent_segments = 2;
        log::info!("[qbzd] Low RAM detected, reduced prefetch to 2 tracks / 1 concurrent");
    }

    log::info!(
        "[qbzd] Cache config: L1={} MB, L2={} MB, prefetch={} tracks, concurrent={}, cmaf_segments={}",
        config.cache.memory_mb,
        config.cache.disk_mb,
        config.cache.prefetch_count,
        config.cache.prefetch_concurrent,
        config.cache.cmaf_concurrent_segments,
    );
}
