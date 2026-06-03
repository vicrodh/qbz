//! Qobuz Connect renderer engine for the Slint frontend (slice 6, Phase S).
//!
//! Implements [`qconnect_app::QconnectRendererEngine`] over the Slint
//! `AppRuntime`'s `QbzCore` + `Player`, so qbz-Slint becomes a QConnect renderer
//! that inherits the shared echo/cursor/materialize/shuffle orchestration in
//! `qconnect_app::renderer` instead of re-deriving it.
//!
//! The protected bit-perfect seams (`play_streaming_dynamic` / `play_data`) and
//! the HTTP feeder live here, impl-side, exactly as the Tauri `CoreBridge` impl
//! does; the probe-derived sample_rate/channels/bit_depth flow STRAIGHT into
//! `play_streaming_dynamic` (never defaulted, or hi-res remote playback silently
//! resamples). The feeder body is a near-verbatim port of the Tauri
//! `track_loading.rs` feeder, with `bridge.player()` -> `self.core().player()`;
//! the only deviation is the TLS backend — the crates workspace `reqwest` ships
//! `rustls-tls` (not `native-tls`), so the `.use_native_tls()` calls are dropped.
//! TLS is transport encryption only; the decoded audio bytes are identical, so
//! bit-perfect is unaffected. (If the Qobuz streaming CDN ever presents a cert
//! rustls rejects, add `native-tls` to qbz-slint's reqwest features.)
//!
//! Wired by the Slint QConnect service + event sink (next Phase S step); until
//! that lands the constructor is unused.
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use qbz_app::shell::AppRuntime;
use qbz_core::QbzCore;
use qbz_models::{Quality, QueueTrack, RepeatMode, Track};
use qbz_player::{BufferWriter, PlaybackState};
use qconnect_app::QconnectRendererEngine;

use crate::adapter::SlintAdapter;

/// QConnect renderer engine backed by the Slint `AppRuntime`. Holds the shared
/// runtime and forwards every trait method through `runtime.core()`; the async
/// feeder spawns on the ambient tokio runtime (`start_track_stream` is always
/// awaited from a runtime task).
pub struct SlintRendererEngine {
    runtime: Arc<AppRuntime<SlintAdapter>>,
}

impl SlintRendererEngine {
    pub fn new(runtime: Arc<AppRuntime<SlintAdapter>>) -> Self {
        Self { runtime }
    }

    fn core(&self) -> &Arc<QbzCore<SlintAdapter>> {
        self.runtime.core()
    }
}

#[async_trait]
impl QconnectRendererEngine for SlintRendererEngine {
    // ---- transport (sync) ----
    fn resume(&self) -> Result<(), String> {
        self.core().resume().map_err(|err| err.to_string())
    }
    fn pause(&self) -> Result<(), String> {
        self.core().pause().map_err(|err| err.to_string())
    }
    fn stop(&self) -> Result<(), String> {
        self.core().stop().map_err(|err| err.to_string())
    }
    fn seek(&self, position_secs: u64) -> Result<(), String> {
        self.core().seek(position_secs).map_err(|err| err.to_string())
    }
    fn set_volume(&self, fraction: f32) -> Result<(), String> {
        self.core().set_volume(fraction).map_err(|err| err.to_string())
    }
    fn get_playback_state(&self) -> PlaybackState {
        self.core().get_playback_state()
    }

    // ---- queue / mode (async) ----
    async fn set_repeat_mode(&self, mode: RepeatMode) {
        self.core().set_repeat_mode(mode).await
    }
    async fn set_shuffle(&self, enabled: bool) {
        self.core().set_shuffle(enabled).await
    }
    async fn get_all_queue_tracks(&self) -> (Vec<QueueTrack>, Option<usize>) {
        self.core().get_all_queue_tracks().await
    }
    async fn set_queue(&self, tracks: Vec<QueueTrack>, start_index: Option<usize>) {
        self.core().set_queue(tracks, start_index).await
    }
    async fn set_queue_with_order(
        &self,
        tracks: Vec<QueueTrack>,
        start_index: Option<usize>,
        shuffle_enabled: bool,
        shuffle_order: Option<Vec<usize>>,
    ) {
        self.core()
            .set_queue_with_order(tracks, start_index, shuffle_enabled, shuffle_order)
            .await
    }
    async fn clear_queue(&self, keep_current: bool) {
        self.core().clear_queue(keep_current).await
    }
    async fn play_index(&self, index: usize) -> Option<QueueTrack> {
        self.core().play_index(index).await
    }

    // ---- catalog (async) ----
    async fn get_track(&self, track_id: u64) -> Result<Track, String> {
        self.core()
            .get_track(track_id)
            .await
            .map_err(|err| err.to_string())
    }
    async fn get_tracks_batch(&self, track_ids: &[u64]) -> Result<Vec<Track>, String> {
        self.core()
            .get_tracks_batch(track_ids)
            .await
            .map_err(|err| err.to_string())
    }

    // ---- protected audio seam (the only protected touch) ----
    async fn start_track_stream(
        &self,
        track_id: u64,
        quality: Quality,
        duration_secs: u64,
        start_position_secs: u64,
    ) -> Result<(), String> {
        let stream_url = self
            .core()
            .get_stream_url(track_id, quality)
            .await
            .map_err(|err| format!("resolve stream url for remote track {track_id}: {err}"))?;

        match stream_remote_track_into_player(
            self.core(),
            track_id,
            duration_secs,
            start_position_secs,
            &stream_url.url,
        )
        .await
        {
            Ok(()) => Ok(()),
            Err(stream_err) => {
                log::warn!(
                    "[QConnect] Streaming handoff unavailable for track {}: {}. Falling back to full download.",
                    track_id,
                    stream_err
                );
                let audio_data = download_remote_audio(&stream_url.url).await?;
                self.core()
                    .player()
                    .play_data(audio_data, track_id)
                    .map_err(|err| format!("play remote track {track_id}: {err}"))?;
                Ok(())
            }
        }
    }

    fn current_output_format(&self) -> Option<(u32, u32)> {
        let player = self.core().player();
        Some((
            player.state.get_sample_rate(),
            player.state.get_bit_depth(),
        ))
    }
}

// ===================== HTTP streaming feeder =====================
//
// reqwest + BufferWriter bound, so it stays frontend-side and never crosses the
// qconnect-app boundary. Verbatim port of the Tauri `track_loading.rs` feeder.

struct QconnectRemoteStreamInfo {
    content_length: u64,
    sample_rate: u32,
    channels: u16,
    bit_depth: u32,
    speed_mbps: f64,
}

async fn stream_remote_track_into_player(
    core: &QbzCore<SlintAdapter>,
    track_id: u64,
    duration_secs: u64,
    start_position_secs: u64,
    url: &str,
) -> Result<(), String> {
    let stream_info = probe_remote_stream_info(url).await?;
    log::info!(
        "[QConnect/STREAMING] Track {} - {:.2} MB, {}Hz, {} ch, {}-bit, {:.1} MB/s",
        track_id,
        stream_info.content_length as f64 / (1024.0 * 1024.0),
        stream_info.sample_rate,
        stream_info.channels,
        stream_info.bit_depth,
        stream_info.speed_mbps
    );

    let writer = core
        .player()
        .play_streaming_dynamic(
            track_id,
            stream_info.sample_rate,
            stream_info.channels,
            stream_info.bit_depth,
            stream_info.content_length,
            stream_info.speed_mbps,
            duration_secs,
            start_position_secs, // QConnect callers pass 0; resume is local-only
        )
        .map_err(|err| format!("start streaming remote track {track_id}: {err}"))?;

    let url = url.to_string();
    let content_length = stream_info.content_length;
    tokio::spawn(async move {
        if let Err(err) =
            download_and_stream_remote_track(&url, writer, track_id, content_length).await
        {
            log::error!(
                "[QConnect/STREAMING] Track {} failed while streaming: {}",
                track_id,
                err
            );
        }
    });

    Ok(())
}

async fn probe_remote_stream_info(url: &str) -> Result<QconnectRemoteStreamInfo, String> {
    use std::time::Instant;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|err| format!("create stream probe client: {err}"))?;

    let head_response = client
        .head(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|err| format!("probe HEAD request failed: {err}"))?;

    if !head_response.status().is_success() {
        return Err(format!(
            "probe HEAD request failed with status {}",
            head_response.status()
        ));
    }

    let content_length = head_response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "probe missing content-length header".to_string())?;

    let start_time = Instant::now();
    let range_response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Range", "bytes=0-65535")
        .send()
        .await
        .map_err(|err| format!("probe range request failed: {err}"))?;

    if !range_response.status().is_success() {
        return Err(format!(
            "probe range request failed with status {}",
            range_response.status()
        ));
    }

    let initial_bytes = range_response
        .bytes()
        .await
        .map_err(|err| format!("read probe bytes failed: {err}"))?;

    let elapsed = start_time.elapsed();
    let speed_mbps = if elapsed.as_secs_f64() > 0.0 {
        (initial_bytes.len() as f64 / elapsed.as_secs_f64()) / (1024.0 * 1024.0)
    } else {
        10.0
    };

    let (sample_rate, channels, bit_depth) =
        if initial_bytes.len() >= 26 && initial_bytes.starts_with(b"fLaC") {
            let sample_rate = ((initial_bytes[18] as u32) << 12)
                | ((initial_bytes[19] as u32) << 4)
                | ((initial_bytes[20] as u32) >> 4);
            let channels = ((initial_bytes[20] >> 1) & 0x07) + 1;
            let bit_depth = ((initial_bytes[20] & 0x01) << 4) | ((initial_bytes[21] >> 4) & 0x0F);
            (sample_rate, channels as u16, (bit_depth + 1) as u32)
        } else {
            log::warn!("[QConnect/STREAMING] Non-FLAC probe for remote handoff, using defaults");
            (44_100, 2, 16)
        };

    Ok(QconnectRemoteStreamInfo {
        content_length,
        sample_rate,
        channels,
        bit_depth,
        speed_mbps,
    })
}

async fn download_and_stream_remote_track(
    url: &str,
    writer: BufferWriter,
    track_id: u64,
    content_length: u64,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::time::Instant;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|err| format!("create remote streaming client: {err}"))?;

    let response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|err| format!("start remote streaming request failed: {err}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "remote streaming request failed with status {}",
            response.status()
        ));
    }

    let mut bytes_received = 0u64;
    let mut stream = response.bytes_stream();
    let start_time = Instant::now();
    let mut last_log_time = Instant::now();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|err| format!("remote streaming chunk failed: {err}"))?;
        bytes_received += chunk.len() as u64;

        if let Err(err) = writer.push_chunk(&chunk) {
            log::error!(
                "[QConnect/STREAMING] Failed to push chunk for track {}: {}",
                track_id,
                err
            );
        }

        let now = Instant::now();
        if now.duration_since(last_log_time) >= Duration::from_secs(2) && content_length > 0 {
            let progress = (bytes_received as f64 / content_length as f64) * 100.0;
            let avg_speed =
                (bytes_received as f64 / start_time.elapsed().as_secs_f64()) / (1024.0 * 1024.0);
            log::info!(
                "[QConnect/STREAMING] Track {} {:.1}% ({:.2}/{:.2} MB) @ {:.2} MB/s",
                track_id,
                progress,
                bytes_received as f64 / (1024.0 * 1024.0),
                content_length as f64 / (1024.0 * 1024.0),
                avg_speed
            );
            last_log_time = now;
        }
    }

    if let Err(err) = writer.complete() {
        log::error!(
            "[QConnect/STREAMING] Failed to mark stream complete for track {}: {}",
            track_id,
            err
        );
    }

    log::info!(
        "[QConnect/STREAMING] Track {} complete: {:.2} MB in {:.1}s",
        track_id,
        bytes_received as f64 / (1024.0 * 1024.0),
        start_time.elapsed().as_secs_f64()
    );

    Ok(())
}

async fn download_remote_audio(url: &str) -> Result<Vec<u8>, String> {
    let response = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|err| format!("download remote audio request failed: {err}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "download remote audio failed with status {}",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("read remote audio bytes failed: {err}"))?;
    Ok(bytes.to_vec())
}
