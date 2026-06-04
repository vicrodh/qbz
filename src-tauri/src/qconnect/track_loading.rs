//! Remote track loading for QConnect: the protected `CoreBridge` renderer-engine
//! impl plus its private HTTP streaming feeder (stream URL probe, progressive
//! streaming into the player, full-download fallback).
//!
//! The dedup window, `should_reload`, and `ensure_remote_track_loaded`
//! orchestration moved to `qconnect_app::renderer` (slice 6, step 6); what stays
//! here is the engine-/reqwest-bound code that cannot cross the qconnect-app
//! boundary: the protected bit-perfect seams (`play_streaming_dynamic` /
//! `play_data`) and the detached HTTP feeder that owns the `BufferWriter`.

use std::time::Duration;

use crate::core_bridge::CoreBridge;

use async_trait::async_trait;
use qbz_models::{Quality, QueueTrack, RepeatMode, Track};
use qbz_player::PlaybackState;
use qconnect_app::QconnectRendererEngine;

/// Re-exported for `tests.rs` (`super::track_loading::should_reload_remote_track`);
/// the non-test orchestration that consumed it moved to `qconnect_app::renderer`.
#[cfg(test)]
pub(super) use qconnect_app::renderer::should_reload_remote_track;

struct QconnectRemoteStreamInfo {
    content_length: u64,
    sample_rate: u32,
    channels: u16,
    bit_depth: u32,
    speed_mbps: f64,
}

async fn stream_remote_track_into_player(
    bridge: &CoreBridge,
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

    let writer = bridge
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
        .use_native_tls()
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
    writer: qbz_player::BufferWriter,
    track_id: u64,
    content_length: u64,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::time::Instant;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .use_native_tls()
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

/// `CoreBridge` as the Tauri QConnect renderer engine (slice 6, step 5).
///
/// Every transport/queue/catalog method is the SAME one-line forward `CoreBridge`
/// already does to `QbzCore`, so the bytes the audio thread sees are identical to
/// the pre-trait call graph. The forwards spell out `CoreBridge::method(self, ..)`
/// (not `self.method(..)`) to call the inherent method unambiguously and never
/// recurse into this trait impl.
///
/// `start_track_stream` is the verbatim body of `load_remote_track_into_player` +
/// `stream_remote_track_into_player` (the probe-derived sample_rate/channels/
/// bit_depth flow straight into `play_streaming_dynamic` — never defaulted) with
/// the quality/duration resolution lifted to the caller. The protected seams
/// (`play_streaming_dynamic` / `play_data`) and the HTTP feeder stay impl-side;
/// `BufferWriter` never crosses the `qconnect-app` boundary.
#[async_trait]
impl QconnectRendererEngine for CoreBridge {
    // ---- transport (sync) ----
    fn resume(&self) -> Result<(), String> {
        CoreBridge::resume(self)
    }
    fn pause(&self) -> Result<(), String> {
        CoreBridge::pause(self)
    }
    fn stop(&self) -> Result<(), String> {
        CoreBridge::stop(self)
    }
    fn seek(&self, position_secs: u64) -> Result<(), String> {
        CoreBridge::seek(self, position_secs)
    }
    fn set_volume(&self, fraction: f32) -> Result<(), String> {
        CoreBridge::set_volume(self, fraction)
    }
    fn get_playback_state(&self) -> PlaybackState {
        CoreBridge::get_playback_state(self)
    }
    fn has_loaded_audio(&self) -> bool {
        self.player().has_loaded_audio()
    }

    // ---- queue / mode (async) ----
    async fn set_repeat_mode(&self, mode: RepeatMode) {
        CoreBridge::set_repeat_mode(self, mode).await
    }
    async fn set_shuffle(&self, enabled: bool) {
        CoreBridge::set_shuffle(self, enabled).await
    }
    async fn get_all_queue_tracks(&self) -> (Vec<QueueTrack>, Option<usize>) {
        CoreBridge::get_all_queue_tracks(self).await
    }
    async fn set_queue(&self, tracks: Vec<QueueTrack>, start_index: Option<usize>) {
        CoreBridge::set_queue(self, tracks, start_index).await
    }
    async fn set_queue_with_order(
        &self,
        tracks: Vec<QueueTrack>,
        start_index: Option<usize>,
        shuffle_enabled: bool,
        shuffle_order: Option<Vec<usize>>,
    ) {
        CoreBridge::set_queue_with_order(self, tracks, start_index, shuffle_enabled, shuffle_order)
            .await
    }
    async fn clear_queue(&self, keep_current: bool) {
        CoreBridge::clear_queue(self, keep_current).await
    }
    async fn play_index(&self, index: usize) -> Option<QueueTrack> {
        CoreBridge::play_index(self, index).await
    }

    // ---- catalog (async) ----
    async fn get_track(&self, track_id: u64) -> Result<Track, String> {
        CoreBridge::get_track(self, track_id).await
    }
    async fn get_tracks_batch(&self, track_ids: &[u64]) -> Result<Vec<Track>, String> {
        CoreBridge::get_tracks_batch(self, track_ids).await
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
            .get_stream_url(track_id, quality)
            .await
            .map_err(|err| format!("resolve stream url for remote track {track_id}: {err}"))?;

        match stream_remote_track_into_player(
            self,
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
                self.player()
                    .play_data(audio_data, track_id)
                    .map_err(|err| format!("play remote track {track_id}: {err}"))?;
                Ok(())
            }
        }
    }

    // ---- report-back source (read-only of the live output config) ----
    fn current_output_format(&self) -> Option<(u32, u32)> {
        let player = self.player();
        Some((
            player.state.get_sample_rate(),
            player.state.get_bit_depth(),
        ))
    }
}
