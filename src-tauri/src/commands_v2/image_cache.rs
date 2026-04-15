use tauri::State;

/// Download an image via reqwest (rustls) and write to a temp file.
/// Returns a file:// URL that WebKit can load without needing system TLS.
/// Used as fallback when the image cache service is unavailable.
async fn download_image_to_temp(url: &str) -> Result<String, String> {
    let url_owned = url.to_string();
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let response = reqwest::blocking::Client::new()
            .get(&url_owned)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .map_err(|e| format!("Failed to download image: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }

        response
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read image bytes: {}", e))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    // Write to temp dir with a hash-based filename to avoid duplicates
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        hasher.finish()
    };
    let tmp_dir = std::env::temp_dir().join("qbz-img-proxy");
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let tmp_path = tmp_dir.join(format!("{:x}.img", hash));
    std::fs::write(&tmp_path, &bytes)
        .map_err(|e| format!("Failed to write temp image: {}", e))?;

    Ok(format!("file://{}", tmp_path.display()))
}

#[tauri::command]
pub async fn v2_get_cached_image(
    url: String,
    cache_state: State<'_, crate::image_cache::ImageCacheState>,
    settings_state: State<'_, crate::config::ImageCacheSettingsState>,
) -> Result<String, String> {
    // Check if caching is enabled
    let settings = {
        let lock = settings_state
            .store
            .lock()
            .map_err(|e| format!("Settings lock error: {}", e))?;
        match lock.as_ref() {
            Some(store) => store.get_settings()?,
            None => crate::config::ImageCacheSettings::default(),
        }
    };

    if !settings.enabled {
        // Cache disabled — still proxy through reqwest so WebKit never
        // needs to resolve HTTPS (fixes AppImage TLS on some distros)
        return download_image_to_temp(&url).await;
    }

    // Check cache first
    {
        let lock = cache_state
            .service
            .lock()
            .map_err(|e| format!("Cache lock error: {}", e))?;
        if let Some(service) = lock.as_ref() {
            if let Some(path) = service.get(&url) {
                return Ok(format!("file://{}", path.display()));
            }
        }
    }

    // Download the image via reqwest (uses rustls — own CA bundle)
    let url_clone = url.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let response = reqwest::blocking::Client::new()
            .get(&url_clone)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .map_err(|e| format!("Failed to download image: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }

        response
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read image bytes: {}", e))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    // Store in cache and evict if needed
    let store_result = {
        let max_bytes = (settings.max_size_mb as u64) * 1024 * 1024;
        let lock = cache_state
            .service
            .lock()
            .map_err(|e| format!("Cache lock error: {}", e))?;
        if let Some(service) = lock.as_ref() {
            let path = service.store(&url, &bytes)?;
            let _ = service.evict(max_bytes);
            Some(format!("file://{}", path.display()))
        } else {
            None
        }
    }; // lock dropped here, before any .await

    match store_result {
        Some(path) => Ok(path),
        // Service not initialized — use temp file fallback
        None => download_image_to_temp(&url).await,
    }
}

#[tauri::command]
pub async fn v2_get_image_cache_settings(
    state: State<'_, crate::config::ImageCacheSettingsState>,
) -> Result<crate::config::ImageCacheSettings, String> {
    let lock = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    match lock.as_ref() {
        Some(store) => store.get_settings(),
        None => Ok(crate::config::ImageCacheSettings::default()),
    }
}

#[tauri::command]
pub async fn v2_set_image_cache_enabled(
    enabled: bool,
    state: State<'_, crate::config::ImageCacheSettingsState>,
) -> Result<(), String> {
    let lock = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    match lock.as_ref() {
        Some(store) => store.set_enabled(enabled),
        None => Err("Image cache settings not initialized".to_string()),
    }
}

#[tauri::command]
pub async fn v2_set_image_cache_max_size(
    max_size_mb: u32,
    state: State<'_, crate::config::ImageCacheSettingsState>,
    cache_state: State<'_, crate::image_cache::ImageCacheState>,
) -> Result<(), String> {
    {
        let lock = state
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        match lock.as_ref() {
            Some(store) => store.set_max_size_mb(max_size_mb)?,
            None => return Err("Image cache settings not initialized".to_string()),
        }
    }
    // Trigger eviction with new limit
    let max_bytes = (max_size_mb as u64) * 1024 * 1024;
    let lock = cache_state
        .service
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    if let Some(service) = lock.as_ref() {
        let _ = service.evict(max_bytes);
    }
    Ok(())
}

#[tauri::command]
pub async fn v2_get_image_cache_stats(
    state: State<'_, crate::image_cache::ImageCacheState>,
) -> Result<crate::image_cache::ImageCacheStats, String> {
    let lock = state
        .service
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    match lock.as_ref() {
        Some(service) => service.stats(),
        None => Ok(crate::image_cache::ImageCacheStats {
            total_bytes: 0,
            file_count: 0,
        }),
    }
}

#[tauri::command]
pub async fn v2_clear_image_cache(
    state: State<'_, crate::image_cache::ImageCacheState>,
    reco_state: State<'_, crate::reco_store::RecoState>,
) -> Result<u64, String> {
    let freed = {
        let lock = state
            .service
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        match lock.as_ref() {
            Some(service) => service.clear()?,
            None => 0,
        }
    };

    // Also clear reco meta image URLs so they re-resolve with correct sizes
    {
        let guard__ = reco_state.db.lock().await;
        if let Some(db) = guard__.as_ref() {
            let _ = db.clear_meta_caches();
        }
    }

    Ok(freed)
}
