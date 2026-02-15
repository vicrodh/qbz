//! Playback-related Tauri commands

use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

use crate::api::client::QobuzClient;
use crate::api::models::Quality;
use crate::cache::AudioCache;
use crate::config::audio_settings::AudioSettingsState;
use crate::offline_cache::OfflineCacheState;
use crate::player::PlaybackState;
use crate::queue::QueueManager;
use crate::AppState;

/// Convert quality string from frontend to Quality enum
fn parse_quality(quality_str: Option<&str>) -> Quality {
    match quality_str {
        Some("MP3") => Quality::Mp3,
        Some("CD Quality") => Quality::Lossless,
        Some("Hi-Res") => Quality::HiRes,
        Some("Hi-Res+") => Quality::UltraHiRes,
        _ => Quality::UltraHiRes, // Default to highest
    }
}

/// Limit quality based on device's max sample rate
/// This ensures bit-perfect playback by not requesting tracks that exceed device capabilities
fn limit_quality_for_device(quality: Quality, max_sample_rate: Option<u32>) -> Quality {
    let Some(max_rate) = max_sample_rate else {
        return quality; // No limit if device max rate unknown
    };

    // Quality mapping:
    // - UltraHiRes (27): up to 192kHz - requires max_rate > 96000
    // - HiRes (7): up to 96kHz - requires max_rate > 48000
    // - Lossless (6): 44.1kHz - works with any device
    // - Mp3 (5): compressed - works with any device

    if max_rate <= 48000 {
        // Device only supports up to 48kHz, limit to CD quality (44.1kHz)
        match quality {
            Quality::UltraHiRes | Quality::HiRes => {
                log::info!(
                    "[Quality Limit] Device max {}Hz, limiting {} to Lossless (44.1kHz)",
                    max_rate, quality.label()
                );
                Quality::Lossless
            }
            _ => quality,
        }
    } else if max_rate <= 96000 {
        // Device supports up to 96kHz, limit to HiRes
        match quality {
            Quality::UltraHiRes => {
                log::info!(
                    "[Quality Limit] Device max {}Hz, limiting Hi-Res+ to Hi-Res (96kHz)",
                    max_rate
                );
                Quality::HiRes
            }
            _ => quality,
        }
    } else {
        // Device supports > 96kHz, allow all qualities
        quality
    }
}

/// Result from play_track command with format info
#[derive(serde::Serialize)]
pub struct PlayTrackResult {
    /// The actual format_id returned by Qobuz (5=MP3, 6=FLAC 16-bit, 7=24-bit, 27=Hi-Res)
    /// None when playing from cache (format unknown)
    pub format_id: Option<u32>,
}

/// Play a track by ID (with caching support)
#[tauri::command]
pub async fn play_track(
    track_id: u64,
    duration_secs: Option<u64>,
    quality: Option<String>,
    state: State<'_, AppState>,
    offline_cache: State<'_, OfflineCacheState>,
    audio_settings: State<'_, AudioSettingsState>,
) -> Result<PlayTrackResult, String> {
    let preferred_quality = parse_quality(quality.as_deref());

    // Apply per-device sample rate limit if enabled
    let final_quality = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        if let Some(store) = guard.as_ref() {
            if let Ok(settings) = store.get_settings() {
                if settings.limit_quality_to_device {
                    // Get the device ID (use current output device or "default")
                    let device_id = settings.output_device.as_deref().unwrap_or("default");
                    // Get per-device limit, falling back to global limit
                    let max_rate = settings
                        .device_sample_rate_limits
                        .get(device_id)
                        .copied()
                        .or(settings.device_max_sample_rate);
                    limit_quality_for_device(preferred_quality, max_rate)
                } else {
                    preferred_quality
                }
            } else {
                preferred_quality
            }
        } else {
            preferred_quality
        }
    };

    log::info!(
        "Command: play_track {} (duration: {:?}s, quality_str={:?}, parsed={:?}, final={:?}, format_id={})",
        track_id, duration_secs, quality, preferred_quality, final_quality, final_quality.id()
    );

    // First check offline cache (persistent disk cache)
    {
        let cached_path = {
            let db_opt__ = offline_cache.db.lock().await;
            if let Some(db) = db_opt__.as_ref() {
                if let Ok(Some(file_path)) = db.get_file_path(track_id) {
                    let _ = db.touch(track_id);
                    Some(file_path)
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(file_path) = cached_path {
            let path = std::path::Path::new(&file_path);
            if path.exists() {
                log::info!("[CACHE HIT] Track {} from OFFLINE cache: {:?}", track_id, path);

                // Read file and play
                let audio_data = std::fs::read(path)
                    .map_err(|e| format!("Failed to read cached file: {}", e))?;

                state.player.play_data(audio_data, track_id)?;

                // Check if prefetch should be skipped (streaming_only mode)
                let skip_prefetch = {
                    let guard = audio_settings.store.lock().map_err(|e| format!("Lock error: {}", e))?;
                    guard.as_ref().and_then(|s| s.get_settings().ok()).map(|s| s.streaming_only).unwrap_or(false)
                };

                // Prefetch next track in background
                spawn_prefetch(
                    state.client.clone(),
                    state.audio_cache.clone(),
                    &state.queue,
                    final_quality,
                    skip_prefetch,
                );

                return Ok(PlayTrackResult { format_id: None });
            }
        }
    }

    let cache = state.audio_cache.clone();

    // Check if track is in memory cache (L1)
    if let Some(cached) = cache.get(track_id) {
        log::info!("[CACHE HIT] Track {} from MEMORY cache ({} bytes) - instant playback", track_id, cached.size_bytes);
        state.player.play_data(cached.data, track_id)?;

        // Check if prefetch should be skipped (streaming_only mode)
        let skip_prefetch = {
            let guard = audio_settings.store.lock().map_err(|e| format!("Lock error: {}", e))?;
            guard.as_ref().and_then(|s| s.get_settings().ok()).map(|s| s.streaming_only).unwrap_or(false)
        };

        // Prefetch next track in background
        spawn_prefetch(
            state.client.clone(),
            state.audio_cache.clone(),
            &state.queue,
            final_quality,
            skip_prefetch,
        );

        return Ok(PlayTrackResult { format_id: None });
    }

    // Check if track is in playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!("[CACHE HIT] Track {} from DISK cache ({} bytes) - instant playback", track_id, audio_data.len());

            // Promote back to memory cache
            cache.insert(track_id, audio_data.clone());

            state.player.play_data(audio_data, track_id)?;

            // Check if prefetch should be skipped (streaming_only mode)
            let skip_prefetch = {
                let guard = audio_settings.store.lock().map_err(|e| format!("Lock error: {}", e))?;
                guard.as_ref().and_then(|s| s.get_settings().ok()).map(|s| s.streaming_only).unwrap_or(false)
            };

            // Prefetch next track in background
            spawn_prefetch(
                state.client.clone(),
                state.audio_cache.clone(),
                &state.queue,
                final_quality,
                skip_prefetch,
            );

            return Ok(PlayTrackResult { format_id: None });
        }
    }

    // Not in any cache - check if streaming is enabled
    log::info!("Track {} not in any cache, fetching from network...", track_id);

    // Check streaming settings
    let (stream_first_enabled, buffer_seconds, streaming_only) = {
        let guard = audio_settings.store.lock().map_err(|e| format!("Lock error: {}", e))?;
        match guard.as_ref().and_then(|s| s.get_settings().ok()) {
            Some(settings) => (settings.stream_first_track, settings.stream_buffer_seconds, settings.streaming_only),
            None => {
                log::warn!("Failed to get audio settings, using defaults");
                (false, 3, false)
            }
        }
    };

    log::info!(
        "[Playback Settings] stream_first: {}, streaming_only: {}, buffer: {}s",
        stream_first_enabled, streaming_only, buffer_seconds
    );

    let client = state.client.read().await;

    // Get the stream URL with final quality (after per-device limiting)
    let stream_url = client
        .get_stream_url_with_fallback(track_id, final_quality)
        .await
        .map_err(|e| format!("Failed to get stream URL: {}", e))?;

    log::info!("Got stream URL for track {}", track_id);

    if stream_first_enabled {
        // Use streaming playback - start playing before full download
        log::info!("[STREAMING] Track {} - streaming from network (cache_after: {})", track_id, !streaming_only);

        // Get content length, audio info, and measured speed via HEAD request
        let stream_info = get_stream_info(&stream_url.url).await?;

        log::info!(
            "Stream info: {:.2} MB, {}Hz, {} channels, {}-bit, {:.1} MB/s",
            stream_info.content_length as f64 / (1024.0 * 1024.0),
            stream_info.sample_rate,
            stream_info.channels,
            stream_info.bit_depth,
            stream_info.speed_mbps
        );

        // Start streaming playback with dynamic buffer based on measured speed
        // The player will use from_speed_mbps internally
        let buffer_writer = state.player.play_streaming_dynamic(
            track_id,
            stream_info.sample_rate,
            stream_info.channels,
            stream_info.bit_depth,
            stream_info.content_length,
            stream_info.speed_mbps,
            duration_secs.unwrap_or(0), // Use 0 if not provided
        )?;

        // Release client lock before spawning background download
        drop(client);

        // Spawn background task to download and push data to buffer
        let url = stream_url.url.clone();
        let cache_clone = cache.clone();
        let content_len = stream_info.content_length;
        let skip_cache = streaming_only;
        tokio::spawn(async move {
            match download_and_stream(&url, buffer_writer, track_id, cache_clone, content_len, skip_cache).await {
                Ok(()) => {
                    if skip_cache {
                        log::info!("[STREAMING COMPLETE] Track {} - NOT cached (streaming_only mode)", track_id);
                    } else {
                        log::info!("[STREAMING COMPLETE] Track {} - cached for instant replay", track_id);
                    }
                },
                Err(e) => log::error!("[STREAMING ERROR] Track {}: {}", track_id, e),
            }
        });

        // Capture format_id before returning
        let actual_format_id = stream_url.format_id;

        // Prefetch next track in background
        spawn_prefetch(
            state.client.clone(),
            state.audio_cache.clone(),
            &state.queue,
            final_quality,
            streaming_only,
        );

        return Ok(PlayTrackResult { format_id: Some(actual_format_id) });
    }

    // Standard download path (streaming disabled)
    log::info!("[DOWNLOAD] Track {} - full download before playback (cache_after: {})", track_id, !streaming_only);

    // Download the audio
    let audio_data = download_audio(&stream_url.url).await?;
    let data_size = audio_data.len();

    // Cache it (unless streaming_only mode)
    if !streaming_only {
        cache.insert(track_id, audio_data.clone());
        log::info!("[CACHED] Track {} stored in memory cache", track_id);
    } else {
        log::info!("[NOT CACHED] Track {} - streaming_only mode active", track_id);
    }

    // Play it
    state.player.play_data(audio_data, track_id)?;

    log::info!("Playing track {} ({} bytes)", track_id, data_size);

    // Release client lock before prefetching
    drop(client);

    // Prefetch next track in background
    spawn_prefetch(
        state.client.clone(),
        state.audio_cache.clone(),
        &state.queue,
        final_quality,
        streaming_only,
    );

    Ok(PlayTrackResult { format_id: Some(stream_url.format_id) })
}

/// Prefetch a track into the in-memory cache without starting playback
#[tauri::command]
pub async fn prefetch_track(
    track_id: u64,
    quality: Option<String>,
    state: State<'_, AppState>,
    offline_cache: State<'_, OfflineCacheState>,
    audio_settings: State<'_, AudioSettingsState>,
) -> Result<(), String> {
    let preferred_quality = parse_quality(quality.as_deref());

    // Apply per-device sample rate limit if enabled
    let final_quality = {
        let guard = audio_settings
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        if let Some(store) = guard.as_ref() {
            if let Ok(settings) = store.get_settings() {
                if settings.limit_quality_to_device {
                    let device_id = settings.output_device.as_deref().unwrap_or("default");
                    let max_rate = settings
                        .device_sample_rate_limits
                        .get(device_id)
                        .copied()
                        .or(settings.device_max_sample_rate);
                    limit_quality_for_device(preferred_quality, max_rate)
                } else {
                    preferred_quality
                }
            } else {
                preferred_quality
            }
        } else {
            preferred_quality
        }
    };

    log::info!(
        "Command: prefetch_track {} (quality_str={:?}, parsed={:?}, final={:?}, format_id={})",
        track_id, quality, preferred_quality, final_quality, final_quality.id()
    );

    let cache = state.audio_cache.clone();

    if cache.contains(track_id) {
        log::info!("Track {} already in memory cache", track_id);
        return Ok(());
    }

    if cache.is_fetching(track_id) {
        log::info!("Track {} already being fetched", track_id);
        return Ok(());
    }

    cache.mark_fetching(track_id);
    let result = async {
        // Check persistent offline cache first
        {
            let cached_path = {
                let db_opt__ = offline_cache.db.lock().await;
                if let Some(db) = db_opt__.as_ref() {
                    db.get_file_path(track_id).ok().flatten()
                } else {
                    None
                }
            };
            if let Some(file_path) = cached_path {
                let path = std::path::Path::new(&file_path);
                if path.exists() {
                    log::info!("Prefetching track {} from offline cache", track_id);
                    let audio_data = std::fs::read(path)
                        .map_err(|e| format!("Failed to read cached file: {}", e))?;
                    cache.insert(track_id, audio_data);
                    return Ok(());
                }
            }
        }

        let client = state.client.read().await;
        let stream_url = client
            .get_stream_url_with_fallback(track_id, final_quality)
            .await
            .map_err(|e| format!("Failed to get stream URL: {}", e))?;
        drop(client);

        let audio_data = download_audio(&stream_url.url).await?;
        cache.insert(track_id, audio_data);
        Ok(())
    }
    .await;

    cache.unmark_fetching(track_id);
    result
}

/// Download audio from URL
async fn download_audio(url: &str) -> Result<Vec<u8>, String> {
    use std::time::Duration;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    log::info!("Caching audio...");

    let response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch audio: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read audio bytes: {}", e))?;

    log::info!("Cached {} bytes", bytes.len());
    Ok(bytes.to_vec())
}

/// Stream info including measured download speed
pub struct StreamInfo {
    pub content_length: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: u32,
    pub speed_mbps: f64,
}

/// Get stream info (content length, sample rate, channels, speed) via HEAD request and initial bytes
async fn get_stream_info(url: &str) -> Result<StreamInfo, String> {
    use std::time::{Duration, Instant};

    lazy_static::lazy_static! {
        // Reuse a static client to avoid intermittent builder errors from creating too many clients
        static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");
    }

    let client = &*HTTP_CLIENT;

    // Retry HEAD request up to 3 times with small delay to handle transient failures
    let mut head_response = None;
    let mut last_error = String::new();
    for attempt in 0..3 {
        match client
            .head(url)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
        {
            Ok(resp) => {
                head_response = Some(resp);
                break;
            }
            Err(e) => {
                last_error = e.to_string();
                if attempt < 2 {
                    log::warn!("HEAD request attempt {} failed: {}, retrying...", attempt + 1, e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    let head_response = head_response.ok_or_else(|| format!("Failed HEAD request after 3 attempts: {}", last_error))?;

    if !head_response.status().is_success() {
        return Err(format!("HEAD request failed: {}", head_response.status()));
    }

    let content_length = head_response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| "No content-length header".to_string())?;

    // Download first ~64KB to probe audio format and measure speed
    // This is enough for FLAC/M4A headers
    let start_time = Instant::now();
    
    let range_response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Range", "bytes=0-65535")
        .send()
        .await
        .map_err(|e| format!("Failed range request: {}", e))?;

    if !range_response.status().is_success() && range_response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("Range request failed: {}", range_response.status()));
    }

    let initial_bytes = range_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read initial bytes: {}", e))?;

    let elapsed = start_time.elapsed();
    let bytes_downloaded = initial_bytes.len() as f64;
    let speed_mbps = if elapsed.as_secs_f64() > 0.0 {
        (bytes_downloaded / elapsed.as_secs_f64()) / (1024.0 * 1024.0)
    } else {
        10.0 // Assume fast if instant
    };

    log::info!(
        "Probe: {}KB in {:.0}ms = {:.1} MB/s",
        initial_bytes.len() / 1024,
        elapsed.as_millis(),
        speed_mbps
    );

    // Try to extract audio format from initial bytes
    let (sample_rate, channels, bit_depth) = extract_audio_format_from_header(&initial_bytes)?;

    Ok(StreamInfo {
        content_length,
        sample_rate,
        channels,
        bit_depth,
        speed_mbps,
    })
}

/// Extract sample rate, channels, and bit depth from audio file header
fn extract_audio_format_from_header(data: &[u8]) -> Result<(u32, u16, u32), String> {
    use std::io::{BufReader, Cursor};
    use rodio::Decoder;

    // Check if this is FLAC
    if data.len() >= 4 && &data[0..4] == b"fLaC" {
        // Parse FLAC STREAMINFO block
        // FLAC format: "fLaC" + METADATA_BLOCK_HEADER (4 bytes) + STREAMINFO
        if data.len() >= 26 {
            // STREAMINFO starts at byte 8
            // Bytes 18-20: sample rate (20 bits) + channels (3 bits) + bits per sample (5 bits)
            let sr_high = ((data[18] as u32) << 12) | ((data[19] as u32) << 4) | ((data[20] as u32) >> 4);
            let sample_rate = sr_high;
            let channels = ((data[20] >> 1) & 0x07) + 1;
            // Bits per sample: 5 bits starting at bit 4 of byte 20
            let bits_per_sample = ((data[20] & 0x01) << 4) | ((data[21] >> 4) & 0x0F);
            let bit_depth = (bits_per_sample + 1) as u32; // FLAC stores (bits - 1)
            return Ok((sample_rate, channels as u16, bit_depth));
        }
    }

    // Check if this is M4A/MP4 (ftyp box)
    if data.len() >= 12 && &data[4..8] == b"ftyp" {
        // M4A uses AAC or ALAC, try symphonia probe
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;
        use symphonia::default::get_probe;

        let _cursor = Box::new(Cursor::new(data.to_vec())) as Box<dyn std::io::Read + Send + Sync>;
        // For probing, we need to create a MediaSource
        // Use a simple wrapper
        struct ProbeSource {
            inner: Cursor<Vec<u8>>,
            len: u64,
        }
        impl std::io::Read for ProbeSource {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.inner.read(buf)
            }
        }
        impl std::io::Seek for ProbeSource {
            fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
                self.inner.seek(pos)
            }
        }
        impl symphonia::core::io::MediaSource for ProbeSource {
            fn is_seekable(&self) -> bool { true }
            fn byte_len(&self) -> Option<u64> { Some(self.len) }
        }

        let len = data.len() as u64;
        let source = Box::new(ProbeSource { inner: Cursor::new(data.to_vec()), len });
        let mss = MediaSourceStream::new(source, Default::default());

        let mut hint = Hint::new();
        hint.with_extension("m4a");

        let format_opts = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };

        if let Ok(probed) = get_probe().format(&hint, mss, &format_opts, &MetadataOptions::default()) {
            if let Some(track) = probed.format.default_track() {
                let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
                let channels = track.codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);
                let bit_depth = track.codec_params.bits_per_sample.unwrap_or(16);
                return Ok((sample_rate, channels, bit_depth));
            }
        }

        // Default to common values for M4A (typically 16-bit for AAC)
        return Ok((44100, 2, 16));
    }

    // Try rodio decoder for other formats (rodio doesn't expose bit depth)
    match Decoder::new(BufReader::new(Cursor::new(data.to_vec()))) {
        Ok(decoder) => {
            use rodio::Source;
            // Assume 16-bit for rodio-decoded formats (MP3, etc.)
            Ok((decoder.sample_rate(), decoder.channels(), 16))
        }
        Err(_) => {
            // Default fallback
            log::warn!("Could not determine audio format, using defaults (44100Hz, stereo, 16-bit)");
            Ok((44100, 2, 16))
        }
    }
}

/// Download audio chunks and stream them to the buffer writer
/// Also caches the complete data when download finishes (unless skip_cache is true)
async fn download_and_stream(
    url: &str,
    writer: crate::player::BufferWriter,
    track_id: u64,
    cache: Arc<AudioCache>,
    content_length: u64,
    skip_cache: bool,
) -> Result<(), String> {
    use std::time::{Duration, Instant};
    use futures_util::StreamExt;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300)) // Longer timeout for streaming
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    log::info!("Starting streaming cache for track {} ({:.2} MB total)", 
        track_id, 
        content_length as f64 / (1024.0 * 1024.0)
    );

    let response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| format!("Failed to start stream: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Stream request failed: {}", response.status()));
    }

    let mut all_data = Vec::with_capacity(content_length as usize);
    let mut stream = response.bytes_stream();
    let mut bytes_received = 0u64;
    let start_time = Instant::now();
    let mut last_log_time = Instant::now();
    let mut last_log_bytes = 0u64;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream chunk error: {}", e))?;
        bytes_received += chunk.len() as u64;

        // Accumulate for caching
        all_data.extend_from_slice(&chunk);

        // Push to streaming buffer
        if let Err(e) = writer.push_chunk(&chunk) {
            log::error!("Failed to push chunk to buffer: {}", e);
        }

        // Log progress every ~500ms with speed info
        let now = Instant::now();
        if now.duration_since(last_log_time) >= Duration::from_millis(500) {
            let elapsed_total = start_time.elapsed().as_secs_f64();
            let elapsed_interval = now.duration_since(last_log_time).as_secs_f64();
            let bytes_this_interval = bytes_received - last_log_bytes;
            
            // Current speed (MB/s)
            let current_speed = (bytes_this_interval as f64 / elapsed_interval) / (1024.0 * 1024.0);
            // Average speed
            let avg_speed = (bytes_received as f64 / elapsed_total) / (1024.0 * 1024.0);
            // Progress percentage
            let progress = (bytes_received as f64 / content_length as f64) * 100.0;
            // ETA
            let remaining_bytes = content_length.saturating_sub(bytes_received);
            let eta_secs = if avg_speed > 0.0 {
                remaining_bytes as f64 / (avg_speed * 1024.0 * 1024.0)
            } else {
                0.0
            };

            log::info!(
                "Download: {:.1}% ({:.2}/{:.2} MB) | Speed: {:.2} MB/s (avg: {:.2}) | ETA: {:.1}s",
                progress,
                bytes_received as f64 / (1024.0 * 1024.0),
                content_length as f64 / (1024.0 * 1024.0),
                current_speed,
                avg_speed,
                eta_secs
            );

            last_log_time = now;
            last_log_bytes = bytes_received;
        }
    }

    // Mark stream as complete
    if let Err(e) = writer.complete() {
        log::error!("Failed to mark buffer complete: {}", e);
    }

    let total_time = start_time.elapsed();
    let avg_speed = (bytes_received as f64 / total_time.as_secs_f64()) / (1024.0 * 1024.0);
    
    log::info!(
        "Streaming cache complete: {:.2} MB in {:.1}s ({:.2} MB/s avg)",
        bytes_received as f64 / (1024.0 * 1024.0),
        total_time.as_secs_f64(),
        avg_speed
    );

    // Cache the complete file for future plays (unless streaming-only mode)
    if skip_cache {
        log::info!("Streaming-only mode: skipping cache for track {}", track_id);
    } else {
        log::info!("Caching track {} for future playback", track_id);
        cache.insert(track_id, all_data);
    }

    Ok(())
}

/// Number of Qobuz tracks to prefetch (not total tracks, just Qobuz)
const QOBUZ_PREFETCH_COUNT: usize = 2;

/// How far ahead to look for tracks to prefetch (to handle mixed playlists)
const PREFETCH_LOOKAHEAD: usize = 10;

/// Maximum concurrent prefetch downloads (reduced to prevent potential race conditions
/// in native audio libraries that can cause memory corruption)
const MAX_CONCURRENT_PREFETCH: usize = 1;

lazy_static::lazy_static! {
    /// Semaphore to limit concurrent prefetch operations
    /// This helps prevent race conditions in native audio code (CPAL/PipeWire/ALSA)
    /// that can cause memory corruption when multiple operations run simultaneously
    static ref PREFETCH_SEMAPHORE: tokio::sync::Semaphore =
        tokio::sync::Semaphore::new(MAX_CONCURRENT_PREFETCH);
}

/// Spawn background tasks to prefetch upcoming Qobuz tracks
/// For mixed playlists, we look further ahead to find Qobuz tracks past local ones
fn spawn_prefetch(
    client: Arc<RwLock<QobuzClient>>,
    cache: Arc<AudioCache>,
    queue: &QueueManager,
    quality: Quality,
    streaming_only: bool,
) {
    // Skip prefetch entirely in streaming_only mode
    if streaming_only {
        log::debug!("[PREFETCH] Skipped - streaming_only mode active");
        return;
    }

    // Look further ahead to find Qobuz tracks in mixed playlists
    let upcoming_tracks = queue.peek_upcoming(PREFETCH_LOOKAHEAD);

    if upcoming_tracks.is_empty() {
        log::debug!("No upcoming tracks to prefetch");
        return;
    }

    let mut qobuz_prefetched = 0;

    for track in upcoming_tracks {
        // Stop once we've prefetched enough Qobuz tracks
        if qobuz_prefetched >= QOBUZ_PREFETCH_COUNT {
            break;
        }

        let track_id = track.id;
        let track_title = track.title.clone();

        // Skip local tracks - they don't need prefetching from Qobuz
        if track.is_local {
            log::debug!("Skipping prefetch for local track: {} - {}", track_id, track_title);
            continue;
        }

        // Check if already cached or being fetched
        if cache.contains(track_id) {
            log::debug!("Track {} already cached", track_id);
            qobuz_prefetched += 1; // Count as "handled"
            continue;
        }

        if cache.is_fetching(track_id) {
            log::debug!("Track {} already being fetched", track_id);
            qobuz_prefetched += 1; // Count as "handled"
            continue;
        }

        // Mark as fetching
        cache.mark_fetching(track_id);
        qobuz_prefetched += 1;

        let client_clone = client.clone();
        let cache_clone = cache.clone();

        log::info!("Prefetching track: {} - {}", track_id, track_title);

        // Spawn background task for each track (with semaphore to limit concurrency)
        tokio::spawn(async move {
            // Acquire semaphore permit to limit concurrent prefetches
            // This prevents potential race conditions in native audio code
            let _permit = match PREFETCH_SEMAPHORE.acquire().await {
                Ok(permit) => permit,
                Err(_) => {
                    log::warn!("Prefetch semaphore closed, skipping track {}", track_id);
                    cache_clone.unmark_fetching(track_id);
                    return;
                }
            };

            let result = async {
                let client_guard = client_clone.read().await;
                let stream_url = client_guard
                    .get_stream_url_with_fallback(track_id, quality)
                    .await
                    .map_err(|e| format!("Failed to get stream URL: {}", e))?;
                drop(client_guard);

                let data = download_audio(&stream_url.url).await?;
                Ok::<Vec<u8>, String>(data)
            }
            .await;

            match result {
                Ok(data) => {
                    // Small delay before cache insertion to avoid potential race with audio thread
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    cache_clone.insert(track_id, data);
                    log::info!("Prefetch complete for track {}", track_id);
                }
                Err(e) => {
                    log::warn!("Prefetch failed for track {}: {}", track_id, e);
                }
            }

            cache_clone.unmark_fetching(track_id);
            // Permit is automatically released when _permit goes out of scope
        });
    }
}

/// Pause playback
#[tauri::command]
pub fn pause_playback(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: pause_playback");
    state.media_controls.set_playback(false);
    state.player.pause()
}

/// Resume playback
#[tauri::command]
pub fn resume_playback(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: resume_playback");
    state.media_controls.set_playback(true);
    state.player.resume()
}

/// Stop playback
#[tauri::command]
pub fn stop_playback(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: stop_playback");
    state.media_controls.set_stopped();
    state.player.stop()
}

/// Queue next track for gapless playback (cache-only, no download)
/// Returns true if gapless was queued, false if track not cached or ineligible
#[tauri::command]
pub async fn play_next_gapless(
    track_id: u64,
    state: State<'_, AppState>,
    offline_cache: State<'_, OfflineCacheState>,
) -> Result<bool, String> {
    log::info!("Command: play_next_gapless for track {}", track_id);

    // Check offline cache (persistent disk cache)
    {
        let cached_path = {
            let db_opt = offline_cache.db.lock().await;
            if let Some(db) = db_opt.as_ref() {
                if let Ok(Some(file_path)) = db.get_file_path(track_id) {
                    Some(file_path)
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(file_path) = cached_path {
            let path = std::path::Path::new(&file_path);
            if path.exists() {
                log::info!("[GAPLESS] Track {} from OFFLINE cache", track_id);
                let audio_data = std::fs::read(path)
                    .map_err(|e| format!("Failed to read cached file: {}", e))?;
                state.player.play_next(audio_data, track_id)?;
                return Ok(true);
            }
        }
    }

    // Check memory cache (L1)
    let cache = state.audio_cache.clone();
    if let Some(cached) = cache.get(track_id) {
        log::info!("[GAPLESS] Track {} from MEMORY cache ({} bytes)", track_id, cached.size_bytes);
        state.player.play_next(cached.data, track_id)?;
        return Ok(true);
    }

    // Check playback cache (L2 - disk)
    if let Some(playback_cache) = cache.get_playback_cache() {
        if let Some(audio_data) = playback_cache.get(track_id) {
            log::info!("[GAPLESS] Track {} from DISK cache ({} bytes)", track_id, audio_data.len());
            state.player.play_next(audio_data, track_id)?;
            return Ok(true);
        }
    }

    log::info!("[GAPLESS] Track {} not in any cache, gapless not possible", track_id);
    Ok(false)
}

/// Set media controls metadata (for MPRIS integration)
#[tauri::command]
pub fn set_media_metadata(
    title: String,
    artist: String,
    album: String,
    duration_secs: Option<u64>,
    cover_url: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: set_media_metadata - {} by {}", title, artist);
    crate::update_media_controls_metadata(
        &state.media_controls,
        &title,
        &artist,
        &album,
        duration_secs,
        cover_url,
    );
    state.media_controls.set_playback_with_progress(true, 0);
    Ok(())
}

/// Set volume (0.0 - 1.0)
#[tauri::command]
pub fn set_volume(volume: f32, state: State<'_, AppState>) -> Result<(), String> {
    // Skip logging if volume is the same (reduces log spam from MPRIS polling)
    let current = state.player.state.volume();
    if (volume - current).abs() >= 0.001 {
        log::info!("Command: set_volume {}", volume);
    }
    state.player.set_volume(volume)
}

/// Seek to position in seconds
#[tauri::command]
pub fn seek(position: u64, state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: seek {}", position);
    let result = state.player.seek(position);

    // Update MPRIS with new position
    let playback_state = state.player.get_state().unwrap_or_default();
    state.media_controls.set_playback_with_progress(
        playback_state.is_playing,
        position,
    );

    result
}

/// Get current playback state (also updates MPRIS progress)
#[tauri::command]
pub fn get_playback_state(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    let playback_state = state.player.get_state()?;

    // Update MPRIS with current progress (called every ~500ms from frontend)
    state.media_controls.set_playback_with_progress(
        playback_state.is_playing,
        playback_state.position,
    );

    Ok(playback_state)
}

/// Audio device information
#[derive(serde::Serialize)]
pub struct AudioDevice {
    pub name: String,           // Technical name (internal use)
    pub display_name: String,   // Friendly name for UI
    pub is_default: bool,
    pub device_type: Option<String>,  // "USB", "PCI", "HDMI", etc.
}

/// Get friendly device names from PipeWire/PulseAudio
#[cfg(target_os = "linux")]
fn get_pipewire_device_descriptions() -> std::collections::HashMap<String, String> {
    use std::process::Command;

    let mut map = std::collections::HashMap::new();

    // Try pactl (PipeWire/PulseAudio)
    if let Ok(output) = Command::new("pactl").args(&["list", "sinks"]).output() {
        if let Ok(text) = String::from_utf8(output.stdout) {
            let mut current_name: Option<String> = None;

            for line in text.lines() {
                let line = line.trim();

                // Extract Name: field
                if let Some(name) = line.strip_prefix("Name: ") {
                    current_name = Some(name.to_string());
                }

                // Extract Description: field
                if let Some(desc) = line.strip_prefix("Description: ") {
                    if let Some(name) = &current_name {
                        map.insert(name.clone(), desc.to_string());
                        log::debug!("Device mapping: {} -> {}", name, desc);
                    }
                }
            }
        }
    }

    log::info!("Found {} device descriptions from PipeWire/PulseAudio", map.len());
    map
}

#[cfg(not(target_os = "linux"))]
fn get_pipewire_device_descriptions() -> std::collections::HashMap<String, String> {
    std::collections::HashMap::new()
}

/// Get available audio output devices
#[tauri::command]
pub fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    log::info!("Command: get_audio_devices");

    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    let host = rodio::cpal::default_host();

    let default_device_name = host
        .default_output_device()
        .and_then(|d| d.name().ok());

    // Get friendly names from PipeWire/PulseAudio
    let friendly_names = get_pipewire_device_descriptions();

    let devices: Vec<AudioDevice> = host
        .output_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?
        .filter_map(|device| {
            device.name().ok().map(|name| {
                let is_default = default_device_name.as_ref().map(|d| d == &name).unwrap_or(false);

                // Try to get friendly name from PipeWire/PulseAudio
                let display_name = friendly_names
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| {
                        // Fallback: clean up technical name slightly
                        name.replace("alsa_output.", "")
                            .replace("_", " ")
                            .replace("pro-output", "")
                            .trim()
                            .to_string()
                    });

                // Detect device type from technical name
                let device_type = if name.contains("usb") || name.contains("USB") {
                    Some("USB".to_string())
                } else if name.contains("hdmi") || name.contains("HDMI") {
                    Some("HDMI".to_string())
                } else if name.contains("pci") || name.contains("PCI") {
                    Some("PCI".to_string())
                } else if name.contains("bluetooth") || name.contains("bluez") {
                    Some("Bluetooth".to_string())
                } else {
                    None
                };

                log::debug!(
                    "Device: {} -> {} (type: {:?}, default: {})",
                    name,
                    display_name,
                    device_type,
                    is_default
                );

                AudioDevice {
                    name,
                    display_name,
                    is_default,
                    device_type,
                }
            })
        })
        .collect();

    log::info!("Found {} audio output devices", devices.len());
    Ok(devices)
}

/// Current audio output status
#[derive(serde::Serialize)]
pub struct AudioOutputStatus {
    pub device_name: Option<String>,
    pub is_playing: bool,
}

/// Get current audio output status (what device is actually being used)
#[tauri::command]
pub fn get_audio_output_status(
    state: tauri::State<'_, crate::AppState>,
) -> Result<AudioOutputStatus, String> {
    let device_name = state.player.state.current_device();
    let is_playing = state.player.state.is_playing();

    Ok(AudioOutputStatus {
        device_name,
        is_playing,
    })
}

/// Reinitialize audio device (releases and re-acquires)
/// Call this when changing audio settings like exclusive mode or output device
#[deprecated(since = "1.2.0", note = "Use v2_reinit_audio_device instead")]
#[tauri::command]
pub fn reinit_audio_device(
    device: Option<String>,
    state: tauri::State<'_, crate::AppState>,
    audio_settings_state: tauri::State<'_, crate::config::audio_settings::AudioSettingsState>,
) -> Result<(), String> {
    log::info!("Command: reinit_audio_device {:?}", device);

    // Reload settings from database to ensure Player has latest config (including backend_type)
    if let Ok(guard) = audio_settings_state.store.lock() {
        if let Some(store) = guard.as_ref() {
            if let Ok(fresh_settings) = store.get_settings() {
                log::info!("Reloading audio settings before reinit (backend_type: {:?})", fresh_settings.backend_type);
                let _ = state.player.reload_settings(fresh_settings);
            }
        }
    }

    state.player.reinit_device(device)
}

/// PipeWire/PulseAudio sink information (cross-platform compatible struct)
#[derive(serde::Serialize, Clone)]
pub struct PipewireSink {
    /// Internal name (e.g., "alsa_output.usb-XXX" on Linux, device name on Mac)
    pub name: String,
    /// User-friendly description (e.g., "AB13X Headset Adapter Analog Stereo")
    pub description: String,
    /// Current volume percentage (0-100) - only available on Linux
    pub volume: Option<u32>,
    /// Whether this is the default sink
    pub is_default: bool,
}

/// Get audio output devices using CPAL (works on all platforms including Linux/PipeWire)
/// CRITICAL: Uses CPAL device names so they match what the player can open
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn get_pipewire_sinks() -> Result<Vec<PipewireSink>, String> {
    log::info!("Command: get_pipewire_sinks (Linux, using CPAL)");

    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    let host = rodio::cpal::default_host();

    // Get default device name from CPAL
    let default_device_name = host
        .default_output_device()
        .and_then(|d| d.name().ok());

    log::info!("CPAL default device: {:?}", default_device_name);

    // Enumerate all output devices using CPAL
    let sinks: Vec<PipewireSink> = host
        .output_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?
        .enumerate()
        .filter_map(|(idx, device)| {
            match device.name() {
                Ok(name) => {
                    let is_default = default_device_name.as_ref().map(|d| d == &name).unwrap_or(false);

                    // Get detailed device info for logging
                    let configs_info = device.supported_output_configs()
                        .ok()
                        .map(|configs| {
                            let config_strs: Vec<String> = configs
                                .take(3) // Just first 3 configs for brevity
                                .map(|c| format!("{}ch/{}Hz", c.channels(), c.max_sample_rate().0))
                                .collect();
                            config_strs.join(", ")
                        })
                        .unwrap_or_else(|| "no configs".to_string());

                    log::info!("  [{}] Device: '{}' (default: {}) - Configs: {}",
                        idx, name, is_default, configs_info);

                    // CRITICAL: Use CPAL device name as both name and description
                    // This ensures the name we save is the exact name CPAL can find later
                    Some(PipewireSink {
                        name: name.clone(),
                        description: name, // CPAL names are already user-friendly in PipeWire
                        volume: None,      // Volume not available via CPAL
                        is_default,
                    })
                }
                Err(e) => {
                    log::warn!("  [{}] Failed to get device name: {}", idx, e);
                    None
                }
            }
        })
        .collect();

    log::info!("Found {} audio output devices via CPAL", sinks.len());

    Ok(sinks)
}

/// Get audio output devices using cpal (macOS/Windows fallback)
/// Returns devices in the same format as Linux for UI compatibility
#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub fn get_pipewire_sinks() -> Result<Vec<PipewireSink>, String> {
    log::info!("Command: get_pipewire_sinks (non-Linux, using cpal)");

    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    let host = rodio::cpal::default_host();

    let default_device_name = host
        .default_output_device()
        .and_then(|d| d.name().ok());

    let sinks: Vec<PipewireSink> = host
        .output_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?
        .filter_map(|device| {
            device.name().ok().map(|name| {
                let is_default = default_device_name.as_ref().map(|d| d == &name).unwrap_or(false);
                PipewireSink {
                    name: name.clone(),
                    description: name, // On Mac/Windows, name is usually descriptive enough
                    volume: None,      // Volume not available via cpal
                    is_default,
                }
            })
        })
        .collect();

    log::info!("Found {} audio output devices", sinks.len());
    Ok(sinks)
}

/// Set the default PipeWire/PulseAudio sink
/// This is used when the user selects an audio device in settings
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn set_pipewire_default_sink(sink_name: String) -> Result<(), String> {
    use std::process::Command;

    log::info!("Command: set_pipewire_default_sink {}", sink_name);

    let output = Command::new("pactl")
        .args(["set-default-sink", &sink_name])
        .output()
        .map_err(|e| format!("Failed to run pactl: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("pactl set-default-sink failed: {}", stderr));
    }

    log::info!("Set default sink to: {}", sink_name);
    Ok(())
}

/// Set the default sink (no-op on non-Linux)
#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub fn set_pipewire_default_sink(_sink_name: String) -> Result<(), String> {
    log::info!("Command: set_pipewire_default_sink (no-op on non-Linux)");
    Ok(())
}

/// DEBUG: Get CPAL device names for comparison with PipeWire sinks
#[tauri::command]
pub fn debug_get_cpal_devices() -> Result<Vec<String>, String> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    
    log::info!("Command: debug_get_cpal_devices");
    
    let host = rodio::cpal::default_host();
    
    let devices: Vec<String> = host
        .output_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?
        .filter_map(|device| device.name().ok())
        .collect();
    
    log::info!("CPAL devices: {:?}", devices);
    Ok(devices)
}
