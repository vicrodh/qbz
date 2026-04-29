//! Remote track loading for QConnect: stream URL resolution, FLAC probe,
//! progressive HTTP streaming into the player engine, full-download
//! fallback, and the dedup window that prevents echo SetState frames
//! from re-triggering an in-progress load.

use std::sync::Arc;
use std::time::Duration;

use qbz_models::Quality;
use tokio::sync::Mutex;

use crate::core_bridge::CoreBridge;

use super::QconnectRemoteSyncState;

const LOAD_ATTEMPT_DEDUP_WINDOW: Duration = Duration::from_secs(5);

pub(super) fn should_reload_remote_track(
    playback_state: &qbz_player::PlaybackState,
    track_id: u64,
) -> bool {
    // Only reload when the track ID actually changed. The previous
    // !has_loaded_audio gate fired during the buffering window of an
    // initial load (qbz already started fetching but the audio engine
    // hasn't reported the track as loaded yet) — when the cloud echo
    // SetState arrived for the same track, this caused a redundant
    // load_remote_track_into_player that interrupted the in-progress
    // load. That was the residual first-track hiccup.
    playback_state.track_id != track_id
}

async fn load_remote_track_into_player(
    bridge: &CoreBridge,
    track_id: u64,
) -> Result<(), String> {
    let stream_url = bridge
        .get_stream_url(track_id, Quality::UltraHiRes)
        .await
        .map_err(|err| format!("resolve stream url for remote track {track_id}: {err}"))?;
    let duration_secs = bridge
        .get_track(track_id)
        .await
        .map(|track| u64::from(track.duration))
        .unwrap_or(0);

    match stream_remote_track_into_player(bridge, track_id, duration_secs, &stream_url.url).await {
        Ok(()) => Ok(()),
        Err(stream_err) => {
            log::warn!(
                "[QConnect] Streaming handoff unavailable for track {}: {}. Falling back to full download.",
                track_id,
                stream_err
            );
            let audio_data = download_remote_audio(&stream_url.url).await?;
            bridge
                .player()
                .play_data(audio_data, track_id)
                .map_err(|err| format!("play remote track {track_id}: {err}"))?;
            Ok(())
        }
    }
}

/// Returns true if a load attempt for `track_id` was registered within the
/// dedup window. The audio thread updates `playback_state.track_id` only
/// after `engine.append(source)` succeeds — the buffer/decode window
/// before that creates a gap during which an echoed SetState would
/// otherwise re-trigger the same load and interrupt the in-progress one.
fn is_recent_load_attempt(state: &QconnectRemoteSyncState, track_id: u64) -> bool {
    match state.last_load_attempt {
        Some((tid, ts)) => tid == track_id && ts.elapsed() < LOAD_ATTEMPT_DEDUP_WINDOW,
        None => false,
    }
}

pub(super) async fn ensure_remote_track_loaded(
    bridge: &CoreBridge,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    track_id: u64,
) -> Result<(), String> {
    {
        let state = sync_state.lock().await;
        if is_recent_load_attempt(&state, track_id) {
            return Ok(());
        }
    }
    let playback_state = bridge.get_playback_state();
    if !should_reload_remote_track(&playback_state, track_id) {
        return Ok(());
    }

    {
        let mut state = sync_state.lock().await;
        state.last_load_attempt = Some((track_id, std::time::Instant::now()));
    }
    load_remote_track_into_player(bridge, track_id).await
}

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
