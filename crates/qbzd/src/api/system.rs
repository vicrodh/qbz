use std::sync::Arc;
use axum::Json;

use crate::daemon::DaemonCore;

pub async fn get_resources(daemon: Arc<DaemonCore>) -> Json<serde_json::Value> {
    let sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing()
            .with_memory(sysinfo::MemoryRefreshKind::everything())
            .with_cpu(sysinfo::CpuRefreshKind::nothing().with_cpu_usage()),
    );

    let cache_stats = daemon.audio_cache.stats();

    Json(serde_json::json!({
        "ram": {
            "total_mb": sys.total_memory() / (1024 * 1024),
            "used_mb": sys.used_memory() / (1024 * 1024),
            "available_mb": sys.available_memory() / (1024 * 1024),
        },
        "cache": {
            "memory_used_bytes": cache_stats.current_size_bytes,
            "memory_max_bytes": cache_stats.max_size_bytes,
            "memory_tracks": cache_stats.cached_tracks,
            "config_memory_mb": daemon.config.cache.memory_mb,
            "config_disk_mb": daemon.config.cache.disk_mb,
        },
    }))
}

pub async fn clear_cache(daemon: Arc<DaemonCore>) -> &'static str {
    daemon.audio_cache.clear();
    log::info!("[qbzd] Audio cache cleared via API");
    "ok"
}
