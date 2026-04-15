//! Cache & Resources wizard section.

use crate::config::{DaemonConfig, save_default_config};

pub fn run_cache_wizard() -> Result<(), String> {
    println!("\n=== Cache & Resources ===\n");

    let sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    let total_ram_mb = sys.total_memory() / (1024 * 1024);
    let auto_cache = (total_ram_mb / 8).min(400).max(50);

    println!("System RAM: {} MB", total_ram_mb);
    println!("Recommended L1 cache: {} MB\n", auto_cache);

    // Memory cache
    println!("Memory cache (L1) in MB [0 = auto-detect, current recommended: {}]:", auto_cache);
    let memory_mb = read_number_or_default(0);

    // Disk cache
    println!("Disk cache (L2) in MB [default: 400]:");
    let disk_mb = read_number_or_default(400);

    // Prefetch
    println!("Prefetch tracks ahead [default: 2]:");
    let prefetch_count = read_number_or_default(2);

    println!("Concurrent prefetch downloads [default: 1]:");
    let prefetch_concurrent = read_number_or_default(1);

    println!("CMAF parallel segments [default: 2]:");
    let cmaf_concurrent = read_number_or_default(2);

    // Load existing config and update
    let mut config = DaemonConfig::load(None);
    config.cache.memory_mb = memory_mb;
    config.cache.disk_mb = disk_mb;
    config.cache.prefetch_count = prefetch_count;
    config.cache.prefetch_concurrent = prefetch_concurrent;
    config.cache.cmaf_concurrent_segments = cmaf_concurrent;
    config.cache.auto.enabled = memory_mb == 0;

    save_default_config(&config)?;

    println!("\nCache settings saved.");
    println!("  L1 memory: {} MB {}", memory_mb, if memory_mb == 0 { "(auto-detect)" } else { "" });
    println!("  L2 disk: {} MB", disk_mb);
    println!("  Prefetch: {} tracks, {} concurrent", prefetch_count, prefetch_concurrent);
    println!("  CMAF segments: {} parallel\n", cmaf_concurrent);
    Ok(())
}

fn read_number_or_default(default: usize) -> usize {
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap_or_default();
    let trimmed = input.trim();
    if trimmed.is_empty() {
        default
    } else {
        trimmed.parse().unwrap_or(default)
    }
}
