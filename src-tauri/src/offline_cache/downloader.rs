//! Stream fetcher for caching tracks to disk

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use super::{CacheProgress, OfflineCacheStatus};

/// StreamFetcher handles fetching audio streams and caching them to disk
pub struct StreamFetcher {
    client: reqwest::Client,
}

impl StreamFetcher {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300)) // 5 minute timeout for large files
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
    }

    /// Fetch a stream and cache it to disk with progress updates
    pub async fn fetch_to_file(
        &self,
        url: &str,
        dest_path: &Path,
        track_id: u64,
        app_handle: Option<&AppHandle>,
    ) -> Result<u64, String> {
        log::info!("Caching track {} to {:?}", track_id, dest_path);

        // Create parent directories if needed
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        // Start fetching
        let response = self
            .client
            .get(url)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
            .map_err(|e| format!("Failed to start fetch: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let total_size = response.content_length();
        log::info!(
            "Caching started for track {}, total size: {:?} bytes",
            track_id,
            total_size
        );

        // Create temp file for caching
        let temp_path = dest_path.with_extension("tmp");
        let mut file = std::fs::File::create(&temp_path)
            .map_err(|e| format!("Failed to create temp file: {}", e))?;

        let mut cached: u64 = 0;
        let mut last_progress: u8 = 0;
        let mut last_emit_time = Instant::now();
        const MIN_EMIT_INTERVAL: Duration = Duration::from_millis(200);

        // Stream the response body
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("Fetch error: {}", e))?;

            file.write_all(&chunk)
                .map_err(|e| format!("Failed to write chunk: {}", e))?;

            cached += chunk.len() as u64;

            // Calculate progress
            let progress = if let Some(total) = total_size {
                ((cached as f64 / total as f64) * 100.0) as u8
            } else {
                // If we don't know total size, report bytes cached
                0
            };

            // Emit progress event every 2% change AND at least 200ms apart (always emit 100%)
            let elapsed = last_emit_time.elapsed();
            if progress != last_progress && (progress - last_progress >= 2 || progress == 100) && (elapsed >= MIN_EMIT_INTERVAL || progress == 100) {
                last_progress = progress;
                last_emit_time = Instant::now();

                if let Some(app) = app_handle {
                    let _ = app.emit(
                        "offline:caching_progress",
                        CacheProgress {
                            track_id,
                            progress_percent: progress,
                            bytes_downloaded: cached,
                            total_bytes: total_size,
                            status: OfflineCacheStatus::Downloading,
                        },
                    );
                }

                log::debug!(
                    "Caching progress for track {}: {}% ({}/{:?} bytes)",
                    track_id,
                    progress,
                    cached,
                    total_size
                );
            }
        }

        // Ensure all data is written
        file.flush()
            .map_err(|e| format!("Failed to flush file: {}", e))?;
        drop(file);

        // Move temp file to final destination
        std::fs::rename(&temp_path, dest_path)
            .map_err(|e| format!("Failed to move temp file: {}", e))?;

        log::info!(
            "Caching complete for track {}: {} bytes",
            track_id,
            cached
        );

        Ok(cached)
    }

    /// Fetch to memory (for smaller files or streaming)
    pub async fn fetch_to_memory(&self, url: &str) -> Result<Vec<u8>, String> {
        let response = self
            .client
            .get(url)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read bytes: {}", e))?;

        Ok(bytes.to_vec())
    }
}

impl Default for StreamFetcher {
    fn default() -> Self {
        Self::new()
    }
}

