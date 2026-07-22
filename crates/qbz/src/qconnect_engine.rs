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

use async_trait::async_trait;
use qbz_app::shell::AppRuntime;
use qbz_core::QbzCore;
use qbz_models::{Quality, QueueTrack, RepeatMode, Track};
use qbz_player::PlaybackState;
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

    /// Last-resort load for tracks the raw-URL path cannot fetch (the CDN
    /// header flood defeats every reqwest attempt — see
    /// `remote_stream::is_header_flood_error`): the CMAF path is unaffected by
    /// the h1 header cap. `play_track_resolved` does NOT move the queue cursor
    /// (nothing on the QConnect path does — the shared driver's cursor sync
    /// only fires on a playing->playing track edge), so sync it explicitly or
    /// the now-playing truth keeps showing the PREVIOUS track while the
    /// recovered one plays.
    async fn play_via_cmaf(
        &self,
        track_id: u64,
        quality: Quality,
        start_position_secs: u64,
    ) -> Result<(), String> {
        self.core()
            .play_track_resolved(track_id, quality, None, None, start_position_secs)
            .await
            .map_err(|err| format!("CMAF fallback for remote track {track_id}: {err}"))?;
        self.core().sync_current_to_id(track_id).await;
        Ok(())
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
    fn has_loaded_audio(&self) -> bool {
        self.core().player().has_loaded_audio()
    }

    // ---- queue / mode (async) ----
    async fn set_repeat_mode(&self, mode: RepeatMode) {
        self.core().set_repeat_mode(mode).await
    }
    async fn set_shuffle(&self, enabled: bool) {
        self.core().set_shuffle(enabled).await
    }
    async fn set_shuffle_flag(&self, enabled: bool) {
        self.core().set_shuffle_with_order(enabled, None).await
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

        let player = self.core().player();
        let stream_result = crate::remote_stream::stream_remote_track_into_player(
            &player,
            track_id,
            duration_secs,
            start_position_secs,
            &stream_url.url,
            "QConnect",
        )
        .await;

        let Err(stream_err) = stream_result else {
            return Ok(());
        };

        // Akamai small-object header flood: SMALL raw-url objects come back
        // with ~106 headers, over hyper's hard-coded 100-header h1 cap, so
        // EVERY reqwest fetch of this URL fails — the full download would die
        // the same death. Skip it and go straight to the CMAF last resort.
        if crate::remote_stream::is_header_flood_error(&stream_err) {
            log::warn!(
                "[QConnect] Raw-URL streaming hit the CDN header flood for track {track_id}: {stream_err}. Skipping full download; last resort: CMAF."
            );
            return self
                .play_via_cmaf(track_id, quality, start_position_secs)
                .await;
        }

        log::warn!(
            "[QConnect] Streaming handoff unavailable for track {}: {}. Falling back to full download.",
            track_id,
            stream_err
        );
        match download_remote_audio(&stream_url.url).await {
            Ok(audio_data) => {
                self.core()
                    .player()
                    .play_data(audio_data, track_id)
                    .map_err(|err| format!("play remote track {track_id}: {err}"))?;
                Ok(())
            }
            Err(download_err) if crate::remote_stream::is_header_flood_error(&download_err) => {
                log::warn!(
                    "[QConnect] Full download hit the CDN header flood for track {track_id}: {download_err}. Last resort: CMAF."
                );
                self.play_via_cmaf(track_id, quality, start_position_secs)
                    .await
            }
            Err(download_err) => Err(download_err),
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


async fn download_remote_audio(url: &str) -> Result<Vec<u8>, String> {
    let response = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|err| {
            format!(
                "download remote audio request failed: {}",
                crate::remote_stream::describe_reqwest_error(&err)
            )
        })?;

    if !response.status().is_success() {
        return Err(format!(
            "download remote audio failed with status {}",
            response.status()
        ));
    }

    let bytes = response.bytes().await.map_err(|err| {
        format!(
            "read remote audio bytes failed: {}",
            crate::remote_stream::describe_reqwest_error(&err)
        )
    })?;
    Ok(bytes.to_vec())
}
