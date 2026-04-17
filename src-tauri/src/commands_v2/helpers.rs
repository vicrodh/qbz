// ==================== Helper Functions ====================

use std::sync::Arc;
use std::sync::Mutex;
use std::fs;
use std::io::Write;
#[cfg(target_os = "linux")]
use std::io::Cursor;
use std::path::PathBuf;

use md5::{Digest, Md5};

use qbz_models::{
    Album, Artist, Playlist, Quality, SearchResultsPage, StreamUrl, Track,
};

use crate::audio::{AlsaPlugin, AudioBackendType};
use crate::config::audio_settings::{AudioSettings, AudioSettingsState};
use crate::config::legal_settings::LegalSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::runtime::RuntimeEvent;

// -- CDN streaming coordination statics --

lazy_static::lazy_static! {
    /// Notify waiters when streaming download finishes.
    /// Prefetch waits on this if a streaming download is active.
    pub(crate) static ref CDN_STREAM_DONE: tokio::sync::Notify = tokio::sync::Notify::new();
}

/// Number of active streaming downloads (for the currently-playing track).
/// Prefetch checks this and waits if > 0 to avoid CDN rate limiting.
pub(crate) static CDN_STREAMING_ACTIVE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Backend information for UI display
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackendInfo {
    pub backend_type: AudioBackendType,
    pub name: String,
    pub description: String,
    pub is_available: bool,
}

/// ALSA plugin information for UI display
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlsaPluginInfo {
    pub plugin: AlsaPlugin,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HardwareAudioStatus {
    pub hardware_sample_rate: Option<u32>,
    pub hardware_format: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DacCapabilities {
    pub node_name: String,
    pub sample_rates: Vec<u32>,
    pub formats: Vec<String>,
    pub channels: Option<u32>,
    pub description: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "content", rename_all = "lowercase")]
pub enum V2MostPopularItem {
    Tracks(Track),
    Albums(Album),
    Artists(Artist),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct V2SearchAllResults {
    pub albums: SearchResultsPage<Album>,
    pub tracks: SearchResultsPage<Track>,
    pub artists: SearchResultsPage<Artist>,
    pub playlists: SearchResultsPage<Playlist>,
    pub most_popular: Option<V2MostPopularItem>,
}

/// Convert config AudioSettings to qbz_audio::AudioSettings.
/// Used by runtime_bootstrap (once at startup) and v2_reinit_audio_device
/// to ensure the Player has fresh settings from the database.
pub(crate) fn convert_to_qbz_audio_settings(settings: &AudioSettings) -> qbz_audio::AudioSettings {
    qbz_audio::AudioSettings {
        output_device: settings.output_device.clone(),
        exclusive_mode: settings.exclusive_mode,
        dac_passthrough: settings.dac_passthrough,
        preferred_sample_rate: settings.preferred_sample_rate,
        limit_quality_to_device: settings.limit_quality_to_device,
        device_max_sample_rate: settings.device_max_sample_rate,
        device_sample_rate_limits: settings.device_sample_rate_limits.clone(),
        backend_type: settings.backend_type.clone(),
        alsa_plugin: settings.alsa_plugin.clone(),
        alsa_hardware_volume: settings.alsa_hardware_volume,
        stream_first_track: settings.stream_first_track,
        stream_buffer_seconds: settings.stream_buffer_seconds,
        streaming_only: settings.streaming_only,
        normalization_enabled: settings.normalization_enabled,
        normalization_target_lufs: settings.normalization_target_lufs,
        gapless_enabled: settings.gapless_enabled,
        pw_force_bitperfect: settings.pw_force_bitperfect,
        skip_sink_switch: settings.skip_sink_switch,
        allow_quality_fallback: settings.allow_quality_fallback,
    }
}

/// Reload audio settings from the per-user store into the CoreBridge player.
/// This ensures the V2 Player uses the latest settings (backend_type, exclusive_mode, etc.)
/// after any audio setting change that affects routing or stream creation.
pub async fn sync_audio_settings_to_player(
    state: &AudioSettingsState,
    bridge: &CoreBridgeState,
) {
    let fresh = {
        let guard = match state.store.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match guard.as_ref().and_then(|s| s.get_settings().ok()) {
            Some(s) => s,
            None => return,
        }
    };
    if let Some(b) = bridge.try_get().await {
        let _ = b
            .player()
            .reload_settings(convert_to_qbz_audio_settings(&fresh));
        log::info!(
            "[V2] Synced audio settings to CoreBridge player: backend={:?}, exclusive={}, dac_passthrough={}",
            fresh.backend_type,
            fresh.exclusive_mode,
            fresh.dac_passthrough
        );
    }
}

/// Persist ToS acceptance and remove the backend gate for login commands.
///
/// Calling any login command IS the user's ToS acceptance (they had to check
/// the checkbox on the frontend to enable the button).  We persist the value
/// best-effort, re-initializing the store if it was torn down (e.g. after a
/// factory reset), so subsequent bootstrap auto-logins work correctly.
pub fn accept_tos_best_effort(legal_state: &LegalSettingsState) {
    use crate::config::legal_settings::LegalSettingsStore;
    if let Ok(mut guard) = legal_state.lock() {
        // Re-initialize the store if it was torn down (e.g. after factory reset).
        if guard.is_none() {
            if let Ok(new_store) = LegalSettingsStore::new() {
                *guard = Some(new_store);
            }
        }
        if let Some(store) = guard.as_ref() {
            let _ = store.set_qobuz_tos_accepted(true);
        }
    }
}

/// Rollback runtime auth state after a partial login failure.
///
/// This MUST be called when:
/// - Legacy auth succeeded but CoreBridge auth failed
/// - Legacy + CoreBridge auth succeeded but session activation failed
///
/// Ensures runtime_get_status never reports a half-authenticated state.
pub async fn rollback_auth_state(manager: &crate::runtime::RuntimeManager, app: &tauri::AppHandle) {
    log::warn!("[V2] Rolling back auth state after partial login failure");
    manager.set_legacy_auth(false, None).await;
    manager.set_corebridge_auth(false).await;
    manager.set_session_activated(false, 0).await;
    let _ = tauri::Emitter::emit(
        app,
        "runtime:event",
        RuntimeEvent::AuthChanged {
            logged_in: false,
            user_id: None,
        },
    );
}

/// Convert quality string from frontend to Quality enum
pub fn parse_quality(quality_str: Option<&str>) -> Quality {
    match quality_str {
        Some("MP3") => Quality::Mp3,
        Some("CD Quality") => Quality::Lossless,
        Some("Hi-Res") => Quality::HiRes,
        Some("Hi-Res+") => Quality::UltraHiRes,
        Some(s) => {
            // Parse "XXbit/YYYkHz" format from track metadata.
            // Map to the Quality that matches what the track actually offers,
            // avoiding unnecessary quality fallback cascades from the API.
            if let Some((bit_str, rate_str)) = s.split_once("bit/") {
                let bit_depth: u32 = bit_str.parse().unwrap_or(0);
                let sample_rate: f64 = rate_str.trim_end_matches("kHz").parse().unwrap_or(0.0);

                if bit_depth >= 24 && sample_rate > 96.0 {
                    Quality::UltraHiRes
                } else if bit_depth >= 24 {
                    Quality::HiRes
                } else {
                    Quality::Lossless
                }
            } else {
                Quality::UltraHiRes // Unknown format, try highest
            }
        }
        None => Quality::UltraHiRes,
    }
}

/// Limit quality based on device's max sample rate
pub fn limit_quality_for_device(quality: Quality, max_sample_rate: Option<u32>) -> Quality {
    let Some(max_rate) = max_sample_rate else {
        return quality;
    };

    if max_rate <= 48000 {
        match quality {
            Quality::UltraHiRes | Quality::HiRes => {
                log::info!(
                    "[V2/Quality Limit] Device max {}Hz, limiting {} to Lossless (44.1kHz)",
                    max_rate,
                    quality.label()
                );
                Quality::Lossless
            }
            _ => quality,
        }
    } else if max_rate <= 96000 {
        match quality {
            Quality::UltraHiRes => {
                log::info!(
                    "[V2/Quality Limit] Device max {}Hz, limiting Hi-Res+ to Hi-Res (96kHz)",
                    max_rate
                );
                Quality::HiRes
            }
            _ => quality,
        }
    } else {
        quality
    }
}

/// Probe sample rate and bit depth from FLAC STREAMINFO header.
/// Returns (sample_rate, bit_depth) or None for non-FLAC / truncated data.
pub fn probe_flac_format(data: &[u8]) -> Option<(u32, u32)> {
    // FLAC: "fLaC" magic + metadata blocks. First block = STREAMINFO (34 bytes).
    // Byte layout at offset 18: sample_rate (20 bits) | channels (3 bits) | bps (5 bits) | ...
    if data.len() < 22 || &data[0..4] != b"fLaC" {
        return None;
    }
    let sr = ((data[18] as u32) << 12) | ((data[19] as u32) << 4) | ((data[20] as u32) >> 4);
    let bps = ((data[20] as u32) & 0x01) << 4 | ((data[21] as u32) >> 4);
    let bit_depth = bps + 1; // FLAC stores bps-1
    if sr > 0 { Some((sr, bit_depth)) } else { None }
}

/// Convenience wrapper used by ALSA hardware check (Linux only).
#[cfg(target_os = "linux")]
pub fn probe_flac_sample_rate(data: &[u8]) -> Option<u32> {
    probe_flac_format(data).map(|(sr, _)| sr)
}

/// Check if cached audio quality is below what the user requested.
/// Returns true if the cache should be skipped in favor of a fresh download.
pub fn cached_quality_below_requested(data: &[u8], requested: Quality) -> bool {
    let (sample_rate, bit_depth) = match probe_flac_format(data) {
        Some(fmt) => fmt,
        None => return false, // Can't parse — assume compatible
    };

    let dominated = match requested {
        // Hi-Res+: expect 24-bit AND >96kHz
        Quality::UltraHiRes => bit_depth < 24 || sample_rate <= 96000,
        // Hi-Res: expect 24-bit
        Quality::HiRes => bit_depth < 24,
        // Lossless / Mp3: any FLAC is fine
        _ => false,
    };

    if dominated {
        log::info!(
            "[V2/Quality] Cached audio is {}Hz/{}bit but requested {:?} — will try re-download",
            sample_rate, bit_depth, requested
        );
    }
    dominated
}

/// Check if cached audio data has a sample rate that the current ALSA hardware
/// doesn't support. Returns true if the audio should NOT be played from cache
/// (needs re-fetch at lower quality).
#[cfg(target_os = "linux")]
pub fn cached_audio_incompatible_with_hw(
    audio_data: &[u8],
    audio_settings: &AudioSettingsState,
) -> bool {
    let sample_rate = match probe_flac_sample_rate(audio_data) {
        Some(rate) => rate,
        None => return false, // Can't determine format, assume compatible
    };

    let guard = match audio_settings.store.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let store = match guard.as_ref() {
        Some(s) => s,
        None => return false,
    };
    let settings = match store.get_settings() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let is_alsa = matches!(
        settings.backend_type,
        Some(qbz_audio::AudioBackendType::Alsa)
    );
    if !is_alsa {
        return false;
    }

    if let Some(ref device_id) = settings.output_device {
        match qbz_audio::device_supports_sample_rate(device_id, sample_rate) {
            Some(false) => {
                log::info!(
                    "[V2/Quality] Cached audio at {}Hz incompatible with hardware, will re-fetch at lower quality",
                    sample_rate
                );
                true
            }
            _ => false,
        }
    } else {
        false
    }
}

/// Download audio from URL (full download before playback)
///
/// Downloads audio data from a CDN URL.
/// Uses a connect timeout but no total timeout because Hi-Res+ tracks
/// can be 100-200MB and take several minutes on slower connections.
/// Waits for any active streaming download to finish before starting,
/// to avoid CDN rate limiting from concurrent downloads.
pub async fn download_audio(url: &str) -> Result<Vec<u8>, String> {
    use std::time::Duration;

    // Wait for streaming download to finish (CDN kills concurrent connections)
    while CDN_STREAMING_ACTIVE.load(std::sync::atomic::Ordering::Acquire) > 0 {
        log::debug!("[V2] download_audio waiting for streaming download to finish...");
        CDN_STREAM_DONE.notified().await;
    }

    // Force HTTP/1.1 — Qobuz CDN sends RST_STREAM on large downloads over HTTP/2,
    // causing "1 byte then EOF". curl (HTTP/1.1) downloads the same URLs successfully.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .use_native_tls()
        .http1_only()
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    log::info!("[V2] Downloading audio...");

    let response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch audio: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    // Log response headers for CDN diagnostics (helps debug "1 byte EOF" issues)
    {
        let headers = response.headers();
        let h_ce = headers.get("content-encoding").map(|v| v.to_str().unwrap_or("?"));
        let h_te = headers.get("transfer-encoding").map(|v| v.to_str().unwrap_or("?"));
        let h_conn = headers.get("connection").map(|v| v.to_str().unwrap_or("?"));
        let h_server = headers.get("server").map(|v| v.to_str().unwrap_or("?"));
        let h_ct = headers.get("content-type").map(|v| v.to_str().unwrap_or("?"));
        let h_via = headers.get("via").map(|v| v.to_str().unwrap_or("?"));
        log::info!(
            "[V2] CDN response: status={}, content-encoding={:?}, transfer-encoding={:?}, connection={:?}, server={:?}, content-type={:?}, via={:?}, version={:?}",
            response.status(),
            h_ce, h_te, h_conn, h_server, h_ct, h_via, response.version()
        );
    }

    let content_length = response.content_length();
    if let Some(len) = content_length {
        log::info!("[V2] Downloading audio: {} bytes expected", len);
    }

    // Stream body in chunks to handle partial reads gracefully
    let expected_len = content_length.unwrap_or(0) as usize;
    let mut all_data = Vec::with_capacity(expected_len);
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => all_data.extend_from_slice(&chunk),
            Err(e) => {
                use std::error::Error as _;
                let mut msg = format!("Failed to read audio bytes: {}", e);
                let mut source = e.source();
                while let Some(cause) = source {
                    msg.push_str(&format!(" | caused by: {}", cause));
                    source = cause.source();
                }
                // If we got some data but not all, log what we received
                if !all_data.is_empty() {
                    log::error!(
                        "[V2] Download error after {}/{} bytes: {}",
                        all_data.len(),
                        expected_len,
                        msg
                    );
                } else {
                    log::error!("[V2] Download error (0 bytes received): {}", msg);
                }
                return Err(msg);
            }
        }
    }

    if expected_len > 0 && all_data.len() != expected_len {
        log::warn!(
            "[V2] Download size mismatch: got {} bytes, expected {}",
            all_data.len(),
            expected_len
        );
    }

    log::info!("[V2] Downloaded {} bytes", all_data.len());
    Ok(all_data)
}

/// Download audio with exponential backoff and quality fallback.
///
/// Attempts up to 4 downloads:
///   1. Current quality, existing URL (immediate)
///   2. Current quality, fresh URL (1s backoff)
///   3. One quality step lower, fresh URL (2s backoff)
///   4. Lower quality again, fresh URL (4s backoff)
///
/// If already at Mp3 (lowest), attempts 3-4 retry at Mp3.
///
/// Returns `(audio_data, stream_url_that_worked)` on success, or a
/// `QualityExhausted:...` structured error string on failure.
pub async fn download_with_backoff(
    initial_url: &str,
    track_id: u64,
    initial_quality: Quality,
    bridge: &crate::core_bridge::CoreBridge,
) -> Result<(Vec<u8>, StreamUrl), String> {
    use std::time::Duration;

    struct Attempt {
        label: &'static str,
        quality: Quality,
        use_initial_url: bool,
        backoff: Duration,
    }

    let lower_quality = initial_quality.lower().unwrap_or(initial_quality);

    let attempts = [
        Attempt {
            label: "1/4 (current quality, existing URL)",
            quality: initial_quality,
            use_initial_url: true,
            backoff: Duration::ZERO,
        },
        Attempt {
            label: "2/4 (current quality, fresh URL)",
            quality: initial_quality,
            use_initial_url: false,
            backoff: Duration::from_secs(1),
        },
        Attempt {
            label: "3/4 (lower quality, fresh URL)",
            quality: lower_quality,
            use_initial_url: false,
            backoff: Duration::from_secs(2),
        },
        Attempt {
            label: "4/4 (lower quality retry, fresh URL)",
            quality: lower_quality,
            use_initial_url: false,
            backoff: Duration::from_secs(4),
        },
    ];

    let mut last_error = String::new();
    let mut saw_server_error = false;
    let mut lowest_tried = initial_quality;

    for attempt in &attempts {
        if !attempt.backoff.is_zero() {
            log::info!(
                "[V2/BACKOFF] Waiting {}s before attempt {} for track {}",
                attempt.backoff.as_secs(),
                attempt.label,
                track_id
            );
            tokio::time::sleep(attempt.backoff).await;
        }

        // Track the lowest quality we've tried
        if attempt.quality < lowest_tried {
            lowest_tried = attempt.quality;
        }

        let (url, stream_url_result) = if attempt.use_initial_url {
            (initial_url.to_string(), None)
        } else {
            log::info!(
                "[V2/BACKOFF] Fetching fresh URL for track {} at quality {} (attempt {})",
                track_id,
                attempt.quality.label(),
                attempt.label
            );
            match bridge.get_stream_url(track_id, attempt.quality).await {
                Ok(su) => {
                    let url = su.url.clone();
                    (url, Some(su))
                }
                Err(e) => {
                    log::warn!(
                        "[V2/BACKOFF] get_stream_url failed for attempt {}: {}",
                        attempt.label,
                        e
                    );
                    last_error = e;
                    continue;
                }
            }
        };

        log::info!(
            "[V2/BACKOFF] Attempt {} for track {} (quality: {})",
            attempt.label,
            track_id,
            attempt.quality.label()
        );

        match download_audio(&url).await {
            Ok(data) => {
                // Build the StreamUrl to return: either the fresh one or a
                // synthetic one from the initial URL.
                let final_url = match stream_url_result {
                    Some(su) => su,
                    None => {
                        // First attempt used the initial URL. Build a
                        // placeholder StreamUrl — the caller already has
                        // the original stream_url, but we need to return
                        // *something*. We fetch a fresh one at the same
                        // quality so caller has accurate metadata.
                        bridge
                            .get_stream_url(track_id, attempt.quality)
                            .await
                            .unwrap_or(StreamUrl {
                                url: url.clone(),
                                format_id: attempt.quality.id(),
                                mime_type: String::new(),
                                sampling_rate: 0.0,
                                bit_depth: None,
                                track_id,
                                restrictions: vec![],
                            })
                    }
                };
                log::info!(
                    "[V2/BACKOFF] Success on attempt {} for track {} ({} bytes)",
                    attempt.label,
                    track_id,
                    data.len()
                );
                return Ok((data, final_url));
            }
            Err(e) => {
                // Check for server errors (502, 503, 504)
                if e.contains("502") || e.contains("503") || e.contains("504") {
                    saw_server_error = true;
                }
                log::warn!(
                    "[V2/BACKOFF] Attempt {} failed for track {}: {}",
                    attempt.label,
                    track_id,
                    e
                );
                last_error = e;
            }
        }
    }

    Err(format!(
        "QualityExhausted:requested={},lowest_tried={},server_error={},detail={}",
        initial_quality.label(),
        lowest_tried.label(),
        saw_server_error,
        last_error
    ))
}

/// Full CMAF download for prefetch/cache — downloads ALL segments, decrypts, returns FLAC bytes.
///
/// Thin wrapper over [`qbz_qobuz::cmaf::download_full`]; the real pipeline lives
/// in the `qbz-qobuz` crate so it can be reused by offline cache and the
/// daemon without pulling Tauri as a dependency.
pub async fn try_cmaf_full_download(
    bridge: &crate::core_bridge::CoreBridge,
    track_id: u64,
    quality: Quality,
) -> Result<Vec<u8>, String> {
    let client_arc = bridge.core().client();
    let client_guard = client_arc.read().await;
    let client = client_guard
        .as_ref()
        .ok_or_else(|| "QobuzClient not initialized".to_string())?;
    qbz_qobuz::cmaf::download_full(client, track_id, quality).await
}

/// Re-export `CmafStreamingInfo` so in-tree callers can keep using
/// `crate::commands_v2::helpers::CmafStreamingInfo` unchanged.
pub use qbz_qobuz::cmaf::CmafStreamingInfo;

/// Prepare CMAF streaming: fetch init segment only, derive keys, return info.
/// Does NOT download audio segments -- the caller streams those in background.
///
/// Thin wrapper over [`qbz_qobuz::cmaf::setup_streaming`].
pub async fn try_cmaf_streaming_setup(
    bridge: &crate::core_bridge::CoreBridge,
    track_id: u64,
    quality: Quality,
) -> Result<CmafStreamingInfo, String> {
    let client_arc = bridge.core().client();
    let client_guard = client_arc.read().await;
    let client = client_guard
        .as_ref()
        .ok_or_else(|| "QobuzClient not initialized".to_string())?;
    qbz_qobuz::cmaf::setup_streaming(client, track_id, quality).await
}

/// Stream info from probing a URL (HEAD + first 64KB)
pub struct V2StreamInfo {
    pub content_length: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: u32,
    pub speed_mbps: f64,
}

/// Probe a stream URL to get content length, audio format, and download speed
pub async fn v2_get_stream_info(url: &str) -> Result<V2StreamInfo, String> {
    use std::time::{Duration, Instant};

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .use_native_tls()
        .http1_only()
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // HEAD request to get content length
    let head_response = client
        .head(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| format!("HEAD request failed: {}", e))?;

    if !head_response.status().is_success() {
        return Err(format!("HEAD request failed: {}", head_response.status()));
    }

    let content_length = head_response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| "No content-length header".to_string())?;

    // Download first 64KB to probe audio format and measure speed
    let start_time = Instant::now();
    let range_response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Range", "bytes=0-65535")
        .send()
        .await
        .map_err(|e| format!("Range request failed: {}", e))?;

    let initial_bytes = range_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read initial bytes: {}", e))?;

    let elapsed = start_time.elapsed();
    let speed_mbps = if elapsed.as_secs_f64() > 0.0 {
        (initial_bytes.len() as f64 / elapsed.as_secs_f64()) / (1024.0 * 1024.0)
    } else {
        10.0
    };

    log::info!(
        "[V2] Probe: {}KB in {:.0}ms = {:.1} MB/s",
        initial_bytes.len() / 1024,
        elapsed.as_millis(),
        speed_mbps
    );

    // Extract audio format from FLAC header
    let (sample_rate, channels, bit_depth) =
        if initial_bytes.len() >= 26 && initial_bytes.starts_with(b"fLaC") {
            let sr = ((initial_bytes[18] as u32) << 12)
                | ((initial_bytes[19] as u32) << 4)
                | ((initial_bytes[20] as u32) >> 4);
            let ch = ((initial_bytes[20] >> 1) & 0x07) + 1;
            let bps = ((initial_bytes[20] & 0x01) << 4) | ((initial_bytes[21] >> 4) & 0x0F);
            (sr, ch as u16, (bps + 1) as u32)
        } else {
            log::warn!("[V2] Non-FLAC stream, using defaults (44100Hz, 2ch, 16-bit)");
            (44100, 2, 16)
        };

    Ok(V2StreamInfo {
        content_length,
        sample_rate,
        channels,
        bit_depth,
        speed_mbps,
    })
}

/// Download audio chunks and push them to the player's streaming buffer.
/// Signals CDN_STREAMING_ACTIVE so prefetch waits until we finish,
/// preventing concurrent CDN downloads that trigger rate limiting.
pub async fn v2_download_and_stream(
    url: &str,
    writer: qbz_player::BufferWriter,
    track_id: u64,
    cache: Arc<crate::cache::AudioCache>,
    content_length: u64,
    skip_cache: bool,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::time::{Duration, Instant};

    // Signal that a streaming download is active — prefetch will wait
    CDN_STREAMING_ACTIVE.fetch_add(1, std::sync::atomic::Ordering::Release);

    // Ensure we clear the flag and notify waiters when done (even on error)
    struct StreamingGuard;
    impl Drop for StreamingGuard {
        fn drop(&mut self) {
            CDN_STREAMING_ACTIVE.fetch_sub(1, std::sync::atomic::Ordering::Release);
            CDN_STREAM_DONE.notify_waiters();
        }
    }
    let _guard = StreamingGuard;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .use_native_tls()
        .http1_only()
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    log::info!(
        "[V2/STREAMING] Starting download for track {} ({:.2} MB)",
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

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| {
            let mut msg = format!("Stream chunk error: {}", e);
            let mut source = std::error::Error::source(&e);
            while let Some(cause) = source {
                msg.push_str(&format!(" | caused by: {}", cause));
                source = std::error::Error::source(cause);
            }
            log::error!("[V2/STREAMING] Chunk error details: {}", msg);
            msg
        })?;
        bytes_received += chunk.len() as u64;

        all_data.extend_from_slice(&chunk);

        if let Err(e) = writer.push_chunk(&chunk) {
            log::error!("[V2/STREAMING] Failed to push chunk: {}", e);
        }

        // Log progress every ~2s
        let now = Instant::now();
        if now.duration_since(last_log_time) >= Duration::from_secs(2) {
            let progress = (bytes_received as f64 / content_length as f64) * 100.0;
            let avg_speed =
                (bytes_received as f64 / start_time.elapsed().as_secs_f64()) / (1024.0 * 1024.0);
            log::info!(
                "[V2/STREAMING] {:.1}% ({:.2}/{:.2} MB) @ {:.2} MB/s",
                progress,
                bytes_received as f64 / (1024.0 * 1024.0),
                content_length as f64 / (1024.0 * 1024.0),
                avg_speed
            );
            last_log_time = now;
        }
    }

    if let Err(e) = writer.complete() {
        log::error!("[V2/STREAMING] Failed to mark buffer complete: {}", e);
    }

    let total_time = start_time.elapsed();
    log::info!(
        "[V2/STREAMING] Complete: {:.2} MB in {:.1}s ({:.2} MB/s)",
        bytes_received as f64 / (1024.0 * 1024.0),
        total_time.as_secs_f64(),
        (bytes_received as f64 / total_time.as_secs_f64()) / (1024.0 * 1024.0)
    );

    if !skip_cache {
        cache.insert(track_id, all_data);
        log::info!(
            "[V2/STREAMING] Track {} cached for future playback",
            track_id
        );
    }

    Ok(())
}

/// Stream CMAF segments to the player's buffer, decrypting on the fly.
/// The FLAC header is written first by the caller, then this function
/// fetches each audio segment, decrypts it, and pushes frames to the buffer.
/// The player starts playing as soon as enough data is buffered.
///
/// Signals CDN_STREAMING_ACTIVE so prefetch waits until we finish,
/// preventing concurrent CDN downloads that trigger rate limiting.
pub async fn v2_cmaf_stream(
    url_template: &str,
    n_segments: u8,
    content_key: [u8; 16],
    flac_header: Vec<u8>,
    writer: qbz_player::BufferWriter,
    track_id: u64,
    cache: Arc<crate::cache::AudioCache>,
    skip_cache: bool,
) -> Result<(), String> {
    use std::time::Instant;

    // Signal that a streaming download is active -- prefetch will wait
    CDN_STREAMING_ACTIVE.fetch_add(1, std::sync::atomic::Ordering::Release);

    // Ensure we clear the flag and notify waiters when done (even on error)
    struct StreamingGuard;
    impl Drop for StreamingGuard {
        fn drop(&mut self) {
            CDN_STREAMING_ACTIVE.fetch_sub(1, std::sync::atomic::Ordering::Release);
            CDN_STREAM_DONE.notify_waiters();
        }
    }
    let _guard = StreamingGuard;

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .use_native_tls()
        .build()
        .map_err(|e| format!("CMAF client error: {}", e))?;

    // Write the FLAC header first so the decoder can identify the format
    if let Err(e) = writer.push_chunk(&flac_header) {
        return Err(format!("Failed to write FLAC header to buffer: {}", e));
    }

    let mut total_written: u64 = flac_header.len() as u64;
    // Collect all decrypted data for caching (header + frames)
    let mut cache_data: Vec<u8> = if skip_cache {
        Vec::new()
    } else {
        let mut v = Vec::with_capacity(flac_header.len());
        v.extend_from_slice(&flac_header);
        v
    };
    let start = Instant::now();

    for seg_idx in 1..=n_segments {
        let seg_url = url_template.replace("$SEGMENT$", &seg_idx.to_string());
        let seg_data = client
            .get(&seg_url)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
            .map_err(|e| format!("CMAF segment {} fetch: {}", seg_idx, e))?
            .bytes()
            .await
            .map_err(|e| format!("CMAF segment {} read: {}", seg_idx, e))?;

        let crypto = qbz_cmaf::parse_segment_crypto(&seg_data)
            .map_err(|e| format!("CMAF segment {} parse: {}", seg_idx, e))?;

        let mut data_pos = crypto.data_offset;
        for entry in &crypto.entries {
            let frame_end = data_pos + entry.size as usize;
            if frame_end > seg_data.len() {
                let _ = writer.error(format!("CMAF segment {} frame overflow", seg_idx));
                return Err(format!("CMAF segment {} frame overflow", seg_idx));
            }
            let mut frame = seg_data[data_pos..frame_end].to_vec();
            if entry.flags != 0 {
                qbz_cmaf::decrypt_frame(&content_key, &entry.iv, &mut frame);
            }
            // Write decrypted frame to streaming buffer
            if let Err(e) = writer.push_chunk(&frame) {
                log::error!("[V2/CMAF-STREAM] Failed to push frame: {}", e);
            }
            if !skip_cache {
                cache_data.extend_from_slice(&frame);
            }
            total_written += frame.len() as u64;
            data_pos = frame_end;
        }

        // Trailing unencrypted data after all frame entries
        if data_pos < crypto.mdat_end && crypto.mdat_end <= seg_data.len() {
            let trailing = &seg_data[data_pos..crypto.mdat_end];
            if let Err(e) = writer.push_chunk(trailing) {
                log::error!("[V2/CMAF-STREAM] Failed to push trailing data: {}", e);
            }
            if !skip_cache {
                cache_data.extend_from_slice(trailing);
            }
            total_written += trailing.len() as u64;
        }

        // Progress logging every 5 segments or on last segment
        if seg_idx % 5 == 0 || seg_idx == n_segments {
            let elapsed = start.elapsed().as_secs_f64();
            log::info!(
                "[V2/CMAF-STREAM] Segment {}/{} ({:.1} MB, {:.1} MB/s)",
                seg_idx,
                n_segments - 1,
                total_written as f64 / (1024.0 * 1024.0),
                if elapsed > 0.0 {
                    total_written as f64 / (1024.0 * 1024.0) / elapsed
                } else {
                    0.0
                }
            );
        }
    }

    // Signal end of stream
    if let Err(e) = writer.complete() {
        log::error!("[V2/CMAF-STREAM] Failed to mark buffer complete: {}", e);
    }

    log::info!(
        "[V2/CMAF-STREAM] Complete: {:.2} MB written in {:.1}s, segments fetched: 1..{} (loop ran {} iterations)",
        total_written as f64 / (1024.0 * 1024.0),
        start.elapsed().as_secs_f64(),
        n_segments - 1,
        (n_segments as u32).saturating_sub(1)
    );

    // Cache the complete FLAC (header + decrypted frames) if enabled
    if !skip_cache && !cache_data.is_empty() {
        cache.insert(track_id, cache_data);
        log::info!(
            "[V2/CMAF-STREAM] Track {} cached for future playback",
            track_id
        );
    }

    Ok(())
}

pub fn v2_teardown_type_alias_state<S>(state: &Arc<Mutex<Option<S>>>) {
    if let Ok(mut guard) = state.lock() {
        *guard = None;
    }
}

#[cfg(target_os = "linux")]
const PORTAL_NOTIFICATION_ICON_MAX_EDGE: u32 = 512;
#[cfg(target_os = "linux")]
const PORTAL_NOTIFICATION_ICON_MAX_BYTES: usize = 4 * 1024 * 1024;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn v2_get_notification_artwork_cache_dir() -> Result<PathBuf, String> {
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| "Could not find cache directory".to_string())?
        .join("qbz")
        .join("artwork");

    fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create artwork cache dir: {}", e))?;
    Ok(cache_dir)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn v2_resolve_local_artwork(url: &str) -> Option<PathBuf> {
    if let Some(path) = url.strip_prefix("file://") {
        return Some(PathBuf::from(path));
    }
    if let Some(path) = url.strip_prefix("asset://localhost/") {
        let decoded = urlencoding::decode(path).ok()?;
        return Some(PathBuf::from(decoded.into_owned()));
    }
    None
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn v2_cache_notification_artwork(url: &str) -> Result<PathBuf, String> {
    if let Some(local_path) = v2_resolve_local_artwork(url) {
        if local_path.exists() {
            return Ok(local_path);
        }
    }

    let mut hasher = Md5::new();
    hasher.update(url.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let cache_dir = v2_get_notification_artwork_cache_dir()?;
    let cache_path = cache_dir.join(format!("{}.jpg", hash));
    if cache_path.exists() {
        return Ok(cache_path);
    }

    let response = reqwest::blocking::Client::new()
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .map_err(|e| format!("Failed to download artwork: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download artwork: HTTP {} (url: {})",
            response.status(),
            url.split('?').next().unwrap_or(url)
        ));
    }

    let bytes = response
        .bytes()
        .map_err(|e| format!("Failed to read artwork bytes: {}", e))?;

    let mut file = fs::File::create(&cache_path)
        .map_err(|e| format!("Failed to create artwork cache file: {}", e))?;
    file.write_all(&bytes)
        .map_err(|e| format!("Failed to write artwork cache: {}", e))?;
    Ok(cache_path)
}

#[cfg(target_os = "linux")]
pub fn v2_prepare_notification_icon_bytes(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let source_image = image::open(path)
        .map_err(|e| format!("Failed to decode artwork image {:?}: {}", path, e))?;
    let source_width = source_image.width();
    let source_height = source_image.height();

    let square_image = if source_width == source_height {
        source_image
    } else {
        let edge = source_width.min(source_height);
        let x = (source_width - edge) / 2;
        let y = (source_height - edge) / 2;
        source_image.crop_imm(x, y, edge, edge)
    };

    let icon_image = if square_image.width() > PORTAL_NOTIFICATION_ICON_MAX_EDGE {
        square_image.resize_exact(
            PORTAL_NOTIFICATION_ICON_MAX_EDGE,
            PORTAL_NOTIFICATION_ICON_MAX_EDGE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        square_image
    };
    let icon_width = icon_image.width();
    let icon_height = icon_image.height();

    let mut png_buffer = Cursor::new(Vec::new());
    icon_image
        .write_to(&mut png_buffer, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode notification artwork PNG: {}", e))?;
    let icon_bytes = png_buffer.into_inner();

    if icon_bytes.len() > PORTAL_NOTIFICATION_ICON_MAX_BYTES {
        return Err(format!(
            "Notification icon too large after normalization: {} bytes (max {})",
            icon_bytes.len(),
            PORTAL_NOTIFICATION_ICON_MAX_BYTES
        ));
    }

    log::debug!(
        "Notification artwork normalized: {}x{} -> {}x{} ({} bytes)",
        source_width,
        source_height,
        icon_width,
        icon_height,
        icon_bytes.len()
    );

    Ok(icon_bytes)
}

pub fn v2_format_notification_quality(bit_depth: Option<u32>, sample_rate: Option<f64>) -> String {
    match (bit_depth, sample_rate) {
        (Some(bits), Some(rate)) if bits >= 24 || rate > 48.0 => {
            let rate_str = if rate.fract() == 0.0 {
                format!("{}", rate as u32)
            } else {
                format!("{}", rate)
            };
            format!("Hi-Res - {}-bit/{}kHz", bits, rate_str)
        }
        (Some(bits), Some(rate)) => {
            let rate_str = if rate.fract() == 0.0 {
                format!("{}", rate as u32)
            } else {
                format!("{}", rate)
            };
            format!("CD Quality - {}-bit/{}kHz", bits, rate_str)
        }
        _ => String::new(),
    }
}
