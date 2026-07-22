// TODO(converge: qconnect-glue) — copied from crates/qbz/src/qconnect_engine.rs @ c8ef2a1b;
// do not fix bugs here without fixing the source, and vice versa.
//
//! Qobuz Connect renderer engine for the qbzd daemon.
//!
//! Implements [`qconnect_app::QconnectRendererEngine`] over the daemon
//! `AppRuntime`'s `QbzCore` + `Player`, so qbzd becomes a QConnect renderer
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
//! rustls rejects, add `native-tls` to qbzd's reqwest features.)
#![allow(dead_code)]

use std::sync::Arc;

use async_trait::async_trait;
use qbz_app::shell::AppRuntime;
use qbz_core::QbzCore;
use qbz_models::{Quality, QueueTrack, RepeatMode, Track};
use qbz_player::PlaybackState;
use qconnect_app::QconnectRendererEngine;

use crate::adapter::DaemonAdapter;

// T10 (OD4, §7.4): daemon-only volume policy. The desktop has no equivalent —
// it always applies remote volume. The mode is read from the daemon-root
// `qconnect_settings.db` `volume_mode` KV key (transport::load_volume_mode_at)
// at connect time and injected into the engine + session host.
/// How the daemon treats a controller's remote volume command (01 §7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VolumeMode {
    /// OD4 DEFAULT. Remote `SetVolume` is applied to the player via the core,
    /// and the player's real volume is reported back to the controller.
    #[default]
    Software,
    /// Bit-perfect purist. The player stays at 100 % (no software attenuation);
    /// remote `SetVolume` is acknowledged-but-ignored (logged at info) and 100
    /// is reported. For DACs feeding power amps where software gain is unwanted.
    Locked,
}

impl VolumeMode {
    /// Parse the `volume_mode` KV value. Anything but the literal `"locked"`
    /// (unset, empty, unknown) falls back to `Software` — the OD4 default.
    pub fn from_kv(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("locked") => VolumeMode::Locked,
            _ => VolumeMode::Software,
        }
    }

    /// Whether a controller's remote `SetVolume` should reach the player. True
    /// only in `Software`; `Locked` acknowledges-but-ignores.
    pub fn applies_remote_volume(self) -> bool {
        matches!(self, VolumeMode::Software)
    }

    /// The volume (0-100 percent) to REPORT to the controller given the player's
    /// real 0.0-1.0 fraction. `Software` reports the real (rounded) percent;
    /// `Locked` always reports 100 regardless of the player's actual level.
    pub fn reported_volume_pct(self, real_fraction: f32) -> i32 {
        match self {
            VolumeMode::Software => (real_fraction.clamp(0.0, 1.0) * 100.0).round() as i32,
            VolumeMode::Locked => 100,
        }
    }
}

/// QConnect renderer engine backed by the daemon `AppRuntime`. Holds the shared
/// runtime and forwards every trait method through `runtime.core()`; the async
/// feeder spawns on the ambient tokio runtime (`start_track_stream` is always
/// awaited from a runtime task).
pub struct DaemonRendererEngine {
    runtime: Arc<AppRuntime<DaemonAdapter>>,
    /// T10 (OD4): resolved volume policy for this session (from the KV at connect).
    volume_mode: VolumeMode,
}

impl DaemonRendererEngine {
    pub fn new(runtime: Arc<AppRuntime<DaemonAdapter>>, volume_mode: VolumeMode) -> Self {
        Self {
            runtime,
            volume_mode,
        }
    }

    fn core(&self) -> &Arc<QbzCore<DaemonAdapter>> {
        self.runtime.core()
    }

    /// Last-resort load for tracks the raw-URL path cannot fetch (the CDN
    /// header flood defeats every reqwest attempt — see
    /// `remote_stream::is_header_flood_error`): the CMAF path is unaffected by
    /// the h1 header cap. `play_track_resolved` does NOT move the queue cursor
    /// (nothing on the QConnect path does — the shared driver's cursor sync
    /// only fires on a playing->playing track edge), so sync it explicitly or
    /// `qbzd status` / the local now-playing truth keep showing the PREVIOUS
    /// track while the recovered one plays.
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
impl QconnectRendererEngine for DaemonRendererEngine {
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
        // T10 (OD4, §7.4): volume-mode gate. In `Locked` mode the player stays
        // at 100 % and a controller's remote SetVolume is acknowledged-but-
        // ignored (logged at info), so the DAC keeps receiving full-scale,
        // bit-perfect samples. `Software` (default) applies it via the core.
        if !self.volume_mode.applies_remote_volume() {
            log::info!(
                "[QConnect] volume_mode=locked: ignoring remote SetVolume({:.3}); player stays at 100%",
                fraction
            );
            return Ok(());
        }
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
        let stream_result = super::remote_stream::stream_remote_track_into_player(
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
        if super::remote_stream::is_header_flood_error(&stream_err) {
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
            Err(download_err) if super::remote_stream::is_header_flood_error(&download_err) => {
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
                super::remote_stream::describe_reqwest_error(&err)
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
            super::remote_stream::describe_reqwest_error(&err)
        )
    })?;
    Ok(bytes.to_vec())
}

// T10 (OD4, §7.4): volume-mode policy tests. These pin the decision the engine's
// `set_volume` gate and the session host's join-time volume report consult — the
// two enforcement points of the software|locked contract.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_mode_applies_and_reports_real() {
        // remote SetVolume 0.4 -> engine.set_volume(0.4); report reads real volume.
        let mode = VolumeMode::from_kv(Some("software"));
        assert_eq!(mode, VolumeMode::Software);
        assert!(mode.applies_remote_volume());
        assert_eq!(mode.reported_volume_pct(0.4), 40);
        assert_eq!(mode.reported_volume_pct(1.0), 100);
    }

    #[test]
    fn locked_mode_ignores_and_reports_100() {
        // remote SetVolume -> acknowledged-but-ignored; player stays 1.0; 100 reported.
        let mode = VolumeMode::from_kv(Some("locked"));
        assert_eq!(mode, VolumeMode::Locked);
        assert!(!mode.applies_remote_volume());
        // 100 reported regardless of the player's actual level.
        assert_eq!(mode.reported_volume_pct(0.4), 100);
        assert_eq!(mode.reported_volume_pct(1.0), 100);
    }

    #[test]
    fn default_mode_is_software_od4() {
        // Unset / empty / unknown all resolve to the OD4 default (software).
        assert_eq!(VolumeMode::default(), VolumeMode::Software);
        assert_eq!(VolumeMode::from_kv(None), VolumeMode::Software);
        assert_eq!(VolumeMode::from_kv(Some("")), VolumeMode::Software);
        assert_eq!(VolumeMode::from_kv(Some("  ")), VolumeMode::Software);
        assert_eq!(VolumeMode::from_kv(Some("garbage")), VolumeMode::Software);
        // Whitespace around the real value is tolerated.
        assert_eq!(VolumeMode::from_kv(Some(" locked ")), VolumeMode::Locked);
    }
}
