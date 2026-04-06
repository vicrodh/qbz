//! Stream fetcher for caching tracks to disk

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use super::{CacheProgress, OfflineCacheStatus};

/// Maximum number of download retry attempts
const MAX_RETRIES: u32 = 3;

/// Backoff durations for each retry attempt
const RETRY_BACKOFFS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(3),
    Duration::from_secs(5),
];

/// StreamFetcher handles fetching audio streams and caching them to disk.
///
/// Creates a fresh HTTP client per download to avoid HTTP/2 connection pool
/// poisoning: when a CDN connection breaks mid-transfer, a persistent client's
/// pool can keep reusing the dead connection, causing all subsequent downloads
/// to fail after 1 byte. Ephemeral clients guarantee a clean connection pool.
pub struct StreamFetcher;

impl StreamFetcher {
    pub fn new() -> Self {
        Self
    }

    /// Build a fresh reqwest::Client for a single download.
    ///
    /// Each download gets its own client to prevent HTTP/2 connection pool
    /// poisoning from affecting subsequent downloads.
    fn build_client() -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(300)) // 5 minute timeout for large files
            .connect_timeout(Duration::from_secs(15))
            .use_native_tls()
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))
    }

    /// Fetch a stream and cache it to disk with progress updates.
    ///
    /// Retries up to MAX_RETRIES times with exponential backoff on transient
    /// failures (connection reset, EOF, timeout). Each retry creates a fresh
    /// HTTP client to avoid reusing a poisoned connection pool.
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

        let temp_path = dest_path.with_extension("tmp");

        let mut last_error = String::new();
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = RETRY_BACKOFFS[(attempt - 1) as usize];
                log::info!(
                    "[Offline] Retry {}/{} for track {} after {}s",
                    attempt,
                    MAX_RETRIES,
                    track_id,
                    backoff.as_secs()
                );
                tokio::time::sleep(backoff).await;
            }

            // Fresh client per attempt — prevents connection pool poisoning
            let client = Self::build_client()?;

            match self
                .try_download(&client, url, &temp_path, track_id, app_handle)
                .await
            {
                Ok(size) => {
                    // Move temp file to final destination
                    std::fs::rename(&temp_path, dest_path)
                        .map_err(|e| format!("Failed to move temp file: {}", e))?;
                    log::info!("Caching complete for track {}: {} bytes", track_id, size);
                    return Ok(size);
                }
                Err(e) => {
                    last_error = e;
                    // Clean up partial temp file before retry
                    let _ = std::fs::remove_file(&temp_path);
                    if attempt < MAX_RETRIES {
                        log::warn!(
                            "[Offline] Download attempt {} failed for track {}: {}",
                            attempt + 1,
                            track_id,
                            last_error
                        );
                    }
                }
            }
        }

        Err(last_error)
    }

    /// Single download attempt: stream response body to a temp file.
    async fn try_download(
        &self,
        client: &reqwest::Client,
        url: &str,
        temp_path: &Path,
        track_id: u64,
        app_handle: Option<&AppHandle>,
    ) -> Result<u64, String> {
        let response = client
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

        let mut file = std::fs::File::create(temp_path)
            .map_err(|e| format!("Failed to create temp file: {}", e))?;

        let mut cached: u64 = 0;
        let mut last_progress: u8 = 0;
        let mut last_emit_time = Instant::now();
        const MIN_EMIT_INTERVAL: Duration = Duration::from_millis(200);

        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| {
                use std::error::Error as _;
                let mut msg = format!("Fetch error: {}", e);
                let mut source = e.source();
                while let Some(cause) = source {
                    msg.push_str(&format!(" | caused by: {}", cause));
                    source = cause.source();
                }
                log::error!(
                    "[Offline] Download error for track {} after {} bytes: {}",
                    track_id,
                    cached,
                    msg
                );
                msg
            })?;

            file.write_all(&chunk)
                .map_err(|e| format!("Failed to write chunk: {}", e))?;

            cached += chunk.len() as u64;

            // Calculate progress
            let progress = if let Some(total) = total_size {
                ((cached as f64 / total as f64) * 100.0) as u8
            } else {
                0
            };

            // Emit progress event every 2% change AND at least 200ms apart (always emit 100%)
            let elapsed = last_emit_time.elapsed();
            if progress != last_progress
                && (progress - last_progress >= 2 || progress == 100)
                && (elapsed >= MIN_EMIT_INTERVAL || progress == 100)
            {
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

        Ok(cached)
    }

    /// Fetch to memory (for smaller files or streaming)
    pub async fn fetch_to_memory(&self, url: &str) -> Result<Vec<u8>, String> {
        let client = Self::build_client()?;

        let response = client
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
