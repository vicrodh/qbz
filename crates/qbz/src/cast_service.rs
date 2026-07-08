//! Cast (Chromecast / DLNA) service for the Slint frontend.
//!
//! Mirrors the Tauri cast integration (the `castStore.ts` behavior + the
//! `commands_v2/library.rs` cast commands) on top of the shared, Tauri-agnostic
//! `qbz-cast` crate. Modeled on `qconnect_service.rs`: a process-wide singleton
//! holding the discovery handles, the active connection, ONE lazy shared
//! `MediaServer`, and a 1s position-poll task. Cast bypasses the local audio
//! backend entirely (the renderer's own DAC decodes the bytes we serve).
//!
//! Bytes + MIME + delivered quality are resolved through the shared core API
//! `CoreBridge::fetch_for_external_stream_resolved` (Phase 0), which attempts
//! UltraHiRes first and reports the real delivered tier (decision D3).
//!
//! Source routing (decision: route by `QueueTrack.source`, never QConnect
//! admission): qobuz -> shared resolver; local -> `register_file` (streams from
//! disk, rich MIME); plex -> TODO (needs the Plex bytes resolver, tracked).

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_cast::{
    CastPositionInfo, ChromecastHandle, DeviceDiscovery, DiscoveredDevice, DiscoveredDlnaDevice,
    DlnaConnection, DlnaDiscovery, DlnaMetadata, DlnaPositionInfo, MediaMetadata, MediaServer,
};
use qbz_models::{QueueTrack, Quality};
use tokio::sync::Mutex;

use crate::adapter::SlintAdapter;
use crate::{AppWindow, CastState, NowPlayingState};

type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Cast position poll cadence (mirrors Tauri's `POSITION_POLL_INTERVAL_MS`).
const POSITION_POLL_INTERVAL_MS: u64 = 1000;

/// How close (in seconds) the renderer must get to a track's end before a DLNA
/// `STOPPED`/`NO_MEDIA_PRESENT` counts as genuine end-of-track. A stop while the
/// max observed position is further than this from the end is treated as a
/// renderer hiccup (logged, no auto-advance) rather than a track end.
const CAST_END_GUARD_SECS: f64 = 5.0;

/// Below this observed max position the renderer's RelTime is considered
/// unreliable (plenty of renderers never implement GetPositionInfo and report
/// 0 forever, while `duration` still resolves from the catalog fallback) and
/// the near-end guard is skipped — otherwise those renderers would never
/// auto-advance again.
const CAST_POSITION_SIGNAL_MIN_SECS: f64 = 1.0;

/// A guard must never wedge the queue: after this many consecutive polls in
/// STOPPED that the near-end guard classified as premature, the stop is
/// honored anyway (~4 s late advance beats a queue stuck forever on a
/// renderer that under-reports position or trims trailing silence).
const CAST_PREMATURE_STOP_POLLS_MAX: u32 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CastProtocol {
    Chromecast,
    Dlna,
}

impl CastProtocol {
    fn as_str(self) -> &'static str {
        match self {
            CastProtocol::Chromecast => "chromecast",
            CastProtocol::Dlna => "dlna",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "chromecast" => Some(CastProtocol::Chromecast),
            "dlna" => Some(CastProtocol::Dlna),
            _ => None,
        }
    }
}

// ---- Module singleton -------------------------------------------------------

static SERVICE: std::sync::OnceLock<Arc<SlintCastService>> = std::sync::OnceLock::new();

/// Initialize the Cast service singleton (idempotent).
pub fn init_service(runtime: Runtime, window: slint::Weak<AppWindow>) -> Arc<SlintCastService> {
    SERVICE
        .get_or_init(|| Arc::new(SlintCastService::new(runtime, window)))
        .clone()
}

/// The initialized Cast service, if shell setup has run.
pub fn service() -> Option<Arc<SlintCastService>> {
    SERVICE.get().cloned()
}

// ---- State ------------------------------------------------------------------

#[derive(Default)]
struct CastInner {
    // Discovery (started while the picker is open).
    chromecast_discovery: Option<DeviceDiscovery>,
    dlna_discovery: Option<DlnaDiscovery>,
    // Active connection (exactly one protocol at a time).
    chromecast: Option<ChromecastHandle>,
    dlna: Option<DlnaConnection>,
    protocol: Option<CastProtocol>,
    connected_device_ip: Option<String>,
    connected_device_name: Option<String>,
    // ONE shared lazy media server for both protocols.
    media_server: Option<MediaServer>,
    // Playback mirror.
    current_track_id: Option<u64>,
    is_playing: bool,
    // Track-end one-shot latch (reset on PLAYING).
    track_end_detected: bool,
    // DLNA track-end guard: whether we've seen PLAYING for the current track,
    // and the max position observed while playing. Many renderers reset RelTime
    // to 0 on STOP, so the instantaneous position at the STOPPED poll is
    // unreliable — track the max instead. All three reset per new track.
    cast_saw_playing: bool,
    cast_max_position: f64,
    // Consecutive STOPPED polls the guard called premature (anti-wedge latch).
    cast_premature_stop_polls: u32,
    // QConnect coexistence: remember whether QConnect was on before casting.
    qconnect_was_on_before_cast: bool,
    // Position-poll task; aborted on disconnect.
    poll_task: Option<tokio::task::JoinHandle<()>>,
    // Device-refresh task (2s loop while the picker is open).
    discovery_task: Option<tokio::task::JoinHandle<()>>,
}

/// Picker device-list poll cadence + scan-spinner window (mirror Tauri).
const DEVICE_POLL_INTERVAL_MS: u64 = 2000;
const SCAN_DURATION_MS: u64 = 15000;

pub struct SlintCastService {
    inner: Arc<Mutex<CastInner>>,
    runtime: Runtime,
    window: slint::Weak<AppWindow>,
}

impl SlintCastService {
    pub fn new(runtime: Runtime, window: slint::Weak<AppWindow>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CastInner::default())),
            runtime,
            window,
        }
    }

    /// True while a renderer is connected and owns transport.
    pub async fn is_casting(&self) -> bool {
        self.inner.lock().await.protocol.is_some()
    }

    // ---- Discovery ----------------------------------------------------------

    /// Start mDNS (Chromecast) + SSDP (DLNA) discovery, the 2s device-refresh
    /// loop, and the 15s scan-spinner window. Picker-owned.
    pub async fn start_discovery(self: &Arc<Self>) {
        {
            let mut inner = self.inner.lock().await;
            if inner.chromecast_discovery.is_none() {
                let mut disco = DeviceDiscovery::new();
                if let Err(e) = disco.start_discovery() {
                    log::warn!("[Cast] chromecast discovery start failed: {e}");
                }
                inner.chromecast_discovery = Some(disco);
            }
            if inner.dlna_discovery.is_none() {
                let mut disco = DlnaDiscovery::new();
                if let Err(e) = disco.start_discovery().await {
                    log::warn!("[Cast] dlna discovery start failed: {e}");
                }
                inner.dlna_discovery = Some(disco);
            }
        }

        // Arm the scan-spinner window.
        self.set_scanning(true);
        let svc_scan = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(SCAN_DURATION_MS)).await;
            svc_scan.set_scanning(false);
        });

        // 2s device-refresh loop (replaces any prior).
        let svc = self.clone();
        let task = tokio::spawn(async move {
            loop {
                svc.refresh_devices().await;
                tokio::time::sleep(std::time::Duration::from_millis(DEVICE_POLL_INTERVAL_MS)).await;
            }
        });
        let mut inner = self.inner.lock().await;
        if let Some(old) = inner.discovery_task.replace(task) {
            old.abort();
        }
    }

    /// Stop both discoveries + the refresh loop (picker closed). The active
    /// connection is untouched.
    pub async fn stop_discovery(&self) {
        let mut inner = self.inner.lock().await;
        if let Some(task) = inner.discovery_task.take() {
            task.abort();
        }
        if let Some(mut disco) = inner.chromecast_discovery.take() {
            let _ = disco.stop_discovery();
        }
        if let Some(mut disco) = inner.dlna_discovery.take() {
            let _ = disco.stop_discovery();
        }
    }

    fn set_scanning(&self, scanning: bool) {
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            w.global::<CastState>().set_scanning(scanning);
        });
    }

    /// Snapshot both device lists and push them to `CastState` for the picker.
    pub async fn refresh_devices(&self) {
        let (chromecast, dlna) = {
            let inner = self.inner.lock().await;
            let cc = inner
                .chromecast_discovery
                .as_ref()
                .map(|d| d.get_discovered_devices())
                .unwrap_or_default();
            let dl = inner
                .dlna_discovery
                .as_ref()
                .map(|d| d.get_discovered_devices())
                .unwrap_or_default();
            (cc, dl)
        };
        self.push_devices(chromecast, dlna);
    }

    // ---- Connect / disconnect ----------------------------------------------

    /// Connect to a device, halting local playback and suspending QConnect, then
    /// re-cast the current track if one was playing. Mirrors
    /// `castStore.connectToDevice`.
    pub async fn connect(self: &Arc<Self>, device_id: String, protocol: String) -> Result<(), String> {
        let proto = CastProtocol::from_str(&protocol)
            .ok_or_else(|| format!("Unknown cast protocol: {protocol}"))?;

        // Snapshot local playback BEFORE we tear it down.
        let snapshot_track = self.runtime.core().current_track().await;
        let pb = self.runtime.core().get_playback_state();
        let was_playing = pb.is_playing;
        let resume_pos = pb.position;

        // Halt the local audio backend (no double audio).
        let _ = self.runtime.core().stop();

        // Suspend QConnect if it was on (best-effort; never block casting).
        self.suspend_qconnect_if_on().await;

        // Connect to the renderer.
        let device_ip = match proto {
            CastProtocol::Chromecast => self.connect_chromecast(&device_id).await?,
            CastProtocol::Dlna => self.connect_dlna(&device_id).await?,
        };

        {
            let mut inner = self.inner.lock().await;
            inner.protocol = Some(proto);
            inner.connected_device_ip = Some(device_ip);
            inner.track_end_detected = false;
            inner.cast_saw_playing = false;
            inner.cast_max_position = 0.0;
            inner.cast_premature_stop_polls = 0;
        }
        self.push_connection_state().await;
        self.start_position_poll();

        // Re-cast the current track at its position, passing the REAL source
        // (fixes the Tauri resume-source bug where Plex re-cast as Qobuz).
        if was_playing {
            if let Some(track) = snapshot_track {
                if let Err(e) = self.cast_track(&track).await {
                    log::warn!("[Cast] resume re-cast failed: {e}");
                } else if resume_pos > 5 {
                    // Deferred seek to the prior position (renderer needs the
                    // media loaded first).
                    let svc = self.clone();
                    let pos = resume_pos as f64;
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        let _ = svc.seek_secs(pos).await;
                    });
                }
            }
        }
        Ok(())
    }

    async fn connect_chromecast(&self, device_id: &str) -> Result<String, String> {
        let device: DiscoveredDevice = {
            let inner = self.inner.lock().await;
            inner
                .chromecast_discovery
                .as_ref()
                .and_then(|d| d.get_device(device_id))
                .ok_or_else(|| format!("Chromecast device not found: {device_id}"))?
        };
        let handle = ChromecastHandle::new();
        handle
            .connect(device.ip.clone(), device.port)
            .map_err(|e| e.to_string())?;
        let mut inner = self.inner.lock().await;
        inner.chromecast = Some(handle);
        inner.connected_device_name = Some(device.name.clone());
        Ok(device.ip)
    }

    async fn connect_dlna(&self, device_id: &str) -> Result<String, String> {
        // `DlnaConnection::connect` consumes the discovered device by value.
        let device: DiscoveredDlnaDevice = {
            let inner = self.inner.lock().await;
            inner
                .dlna_discovery
                .as_ref()
                .and_then(|d| d.get_device(device_id))
                .ok_or_else(|| format!("DLNA device not found: {device_id}"))?
        };
        let ip = device.ip.clone();
        let name = device.name.clone();
        let conn = DlnaConnection::connect(device)
            .await
            .map_err(|e| e.to_string())?;
        let mut inner = self.inner.lock().await;
        inner.dlna = Some(conn);
        inner.connected_device_name = Some(name);
        Ok(ip)
    }

    /// Disconnect: stop the renderer, drop the connection, restore QConnect,
    /// reset state. Mirrors `castStore.disconnect`.
    pub async fn disconnect(&self) {
        // Stop the renderer first (disconnect alone leaves it playing).
        let _ = self.stop_renderer().await;

        let (poll, was_on) = {
            let mut inner = self.inner.lock().await;
            if let Some(h) = inner.chromecast.take() {
                let _ = h.disconnect();
            }
            if let Some(mut c) = inner.dlna.take() {
                let _ = c.disconnect();
            }
            inner.protocol = None;
            inner.connected_device_ip = None;
            inner.connected_device_name = None;
            // Release the served track buffers with the session (#550); the
            // server itself stays up for the next connect.
            if let Some(server) = inner.media_server.as_ref() {
                server.clear_entries();
            }
            inner.current_track_id = None;
            inner.is_playing = false;
            inner.track_end_detected = false;
            (inner.poll_task.take(), inner.qconnect_was_on_before_cast)
        };
        if let Some(task) = poll {
            task.abort();
        }
        if was_on {
            self.restore_qconnect().await;
            self.inner.lock().await.qconnect_was_on_before_cast = false;
        }
        // Hand the lyrics position source back to the local player.
        crate::lyrics_sync::clear_remote_anchor();
        self.push_connection_state().await;
    }

    // ---- Casting a track ----------------------------------------------------

    /// Resolve a track's bytes + MIME, register them with the shared media
    /// server, and hand the URL to the active renderer. Routes by source.
    pub async fn cast_track(self: &Arc<Self>, track: &QueueTrack) -> Result<(), String> {
        let proto = {
            let inner = self.inner.lock().await;
            inner.protocol.ok_or_else(|| "Not connected".to_string())?
        };

        let source = if track.is_local {
            "local"
        } else {
            track.source.as_deref().unwrap_or("qobuz")
        };

        // Resolve the content type + register the bytes per source. The fetch
        // (cache / offline / network) happens OUTSIDE the inner lock.
        let content_type = match source {
            "local" | "ephemeral" => {
                let path = resolve_local_path(track.id)
                    .ok_or_else(|| format!("Local file not found for track {}", track.id))?;
                self.register_local(track.id, &path).await?
            }
            "qobuz" | "qobuz_download" => {
                // Cache-first + offline tier (see fetch_for_external_stream_resolved):
                // a prefetched / replayed / downloaded track resolves with no
                // network; only a cold online track downloads. An offline track
                // not in the cache will simply fail to resolve below.
                self.register_qobuz(track.id).await?
            }
            "plex" => {
                // TODO(cast-plex): Plex casting needs the Plex bytes resolver
                // (baseUrl/token/ratingKey -> proxied bytes). Not yet wired in
                // the Slint frontend; tracked for a follow-up slice.
                return Err("Plex casting is not yet supported".to_string());
            }
            other => return Err(format!("Unsupported cast source: {other}")),
        };

        // Build the per-device URL and hand it to the renderer.
        let url = {
            let inner = self.inner.lock().await;
            let ip = inner.connected_device_ip.clone();
            let server = inner
                .media_server
                .as_ref()
                .ok_or_else(|| "Media server not initialized".to_string())?;
            match ip.as_deref() {
                Some(ip) => server.get_audio_url_for_target(track.id, ip),
                None => server.get_audio_url(track.id),
            }
            .ok_or_else(|| "Failed to build media URL".to_string())?
        };

        match proto {
            CastProtocol::Chromecast => {
                let inner = self.inner.lock().await;
                let handle = inner.chromecast.as_ref().ok_or("Chromecast not connected")?;
                // load_media auto-plays on the Default Media Receiver.
                handle
                    .load_media(url, content_type, media_metadata(track))
                    .map_err(|e| e.to_string())?;
            }
            CastProtocol::Dlna => {
                let mut inner = self.inner.lock().await;
                let conn = inner.dlna.as_mut().ok_or("DLNA not connected")?;
                // DLNA is a TWO-step load -> play.
                conn.load_media(&url, &dlna_metadata(track), &content_type)
                    .await
                    .map_err(|e| e.to_string())?;
                conn.play().await.map_err(|e| e.to_string())?;
            }
        }

        {
            let mut inner = self.inner.lock().await;
            inner.current_track_id = Some(track.id);
            inner.is_playing = true;
            inner.track_end_detected = false;
            inner.cast_saw_playing = false;
            inner.cast_max_position = 0.0;
            inner.cast_premature_stop_polls = 0;
        }
        // Quality label comes from the track's catalog metadata (always present,
        // no network call) so it's the same whether bytes came from cache,
        // offline, or the network.
        let (quality_label, quality_detail) = quality_label_from_track(track);
        self.push_quality(quality_label, quality_detail).await;
        self.push_connection_state().await;
        Ok(())
    }

    /// qobuz: resolve via the shared core API (cache -> offline -> network),
    /// register the bytes. Returns the content type.
    async fn register_qobuz(&self, track_id: u64) -> Result<String, String> {
        let offline = crate::offline::get().await;
        let sink = crate::offline_cache::row_sink(self.window.clone());
        let asset = self
            .runtime
            .core()
            .fetch_for_external_stream_resolved(
                track_id,
                Quality::UltraHiRes,
                offline.as_deref(),
                Some(&sink),
            )
            .await
            .ok_or_else(|| format!("Could not resolve stream for track {track_id}"))?;

        log::info!("[Cast] qobuz track {track_id} resolved from {:?}", asset.origin);
        let content_type = asset.content_type.clone();

        self.ensure_media_server().await?;
        {
            let mut inner = self.inner.lock().await;
            let server = inner.media_server.as_mut().ok_or("Media server gone")?;
            server.register_audio(track_id, asset.bytes, &content_type);
        }
        Ok(content_type)
    }

    /// local: stream the file from disk via register_file (no full-RAM read);
    /// the crate's rich MIME map sets the content type.
    async fn register_local(&self, track_id: u64, path: &str) -> Result<String, String> {
        self.ensure_media_server().await?;
        let content_type = {
            let mut inner = self.inner.lock().await;
            let server = inner.media_server.as_mut().ok_or("Media server gone")?;
            server
                .register_file(track_id, path)
                .map_err(|e| e.to_string())?;
            // Recompute the content type from the path for the UI (cheap, matches
            // the crate's own register_file map).
            content_type_for_local(path)
        };
        Ok(content_type)
    }

    async fn ensure_media_server(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        if inner.media_server.is_none() {
            let server = MediaServer::start().map_err(|e| e.to_string())?;
            inner.media_server = Some(server);
        }
        Ok(())
    }

    // ---- Transport (cast-first gating; mirrors qconnect *_if_remote) --------

    /// Toggle play/pause on the renderer. Ok(false) = not casting (fall through
    /// to local). Ok(true) = handled.
    pub async fn toggle_play_if_cast(&self) -> Result<bool, String> {
        let (proto, playing) = {
            let inner = self.inner.lock().await;
            match inner.protocol {
                Some(p) => (p, inner.is_playing),
                None => return Ok(false),
            }
        };
        if playing {
            self.pause_renderer(proto).await?;
        } else {
            self.play_renderer(proto).await?;
        }
        self.inner.lock().await.is_playing = !playing;
        self.push_connection_state().await;
        Ok(true)
    }

    pub async fn seek_if_cast(&self, secs: f64) -> Result<bool, String> {
        if !self.is_casting().await {
            return Ok(false);
        }
        self.seek_secs(secs).await?;
        Ok(true)
    }

    pub async fn set_volume_if_cast(&self, volume: f32) -> Result<bool, String> {
        let proto = {
            let inner = self.inner.lock().await;
            match inner.protocol {
                Some(p) => p,
                None => return Ok(false),
            }
        };
        let v = volume.clamp(0.0, 1.0);
        match proto {
            CastProtocol::Chromecast => {
                let inner = self.inner.lock().await;
                if let Some(h) = inner.chromecast.as_ref() {
                    h.set_volume(v).map_err(|e| e.to_string())?;
                }
            }
            CastProtocol::Dlna => {
                let mut inner = self.inner.lock().await;
                if let Some(c) = inner.dlna.as_mut() {
                    c.set_volume(v).await.map_err(|e| e.to_string())?;
                }
            }
        }
        // Reflect the drag on the bar: the local set_volume (which normally
        // moves the slider) is skipped while casting, and the cast poll doesn't
        // push volume, so update NowPlayingState.volume here.
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            w.global::<NowPlayingState>().set_volume(v);
        });
        Ok(true)
    }

    // No transport "stop" button in the bar today; kept for parity + a future
    // stop control.
    #[allow(dead_code)]
    pub async fn stop_if_cast(&self) -> Result<bool, String> {
        if !self.is_casting().await {
            return Ok(false);
        }
        self.stop_renderer().await?;
        {
            let mut inner = self.inner.lock().await;
            inner.is_playing = false;
            // Stop = the user is done with this track; release its bytes
            // instead of holding them until the next cast/app exit (#550).
            if let Some(server) = inner.media_server.as_ref() {
                server.clear_entries();
            }
        }
        self.push_connection_state().await;
        Ok(true)
    }

    // NOTE: next/previous are intentionally NOT handled here. While casting, the
    // local playback::next/previous flow runs (it moves the core cursor +
    // refreshes the card/queue), and play_audible casts the new current track.
    // A cast-only advance would desync the UI cursor from the renderer.

    async fn seek_secs(&self, secs: f64) -> Result<(), String> {
        let proto = {
            let inner = self.inner.lock().await;
            match inner.protocol {
                Some(p) => p,
                None => return Ok(()),
            }
        };
        match proto {
            CastProtocol::Chromecast => {
                let inner = self.inner.lock().await;
                if let Some(h) = inner.chromecast.as_ref() {
                    h.seek(secs).map_err(|e| e.to_string())?;
                }
            }
            CastProtocol::Dlna => {
                let mut inner = self.inner.lock().await;
                if let Some(c) = inner.dlna.as_mut() {
                    c.seek(secs.max(0.0) as u64)
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }

    async fn play_renderer(&self, proto: CastProtocol) -> Result<(), String> {
        match proto {
            CastProtocol::Chromecast => {
                let inner = self.inner.lock().await;
                if let Some(h) = inner.chromecast.as_ref() {
                    h.play().map_err(|e| e.to_string())?;
                }
            }
            CastProtocol::Dlna => {
                let mut inner = self.inner.lock().await;
                if let Some(c) = inner.dlna.as_mut() {
                    c.play().await.map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }

    async fn pause_renderer(&self, proto: CastProtocol) -> Result<(), String> {
        match proto {
            CastProtocol::Chromecast => {
                let inner = self.inner.lock().await;
                if let Some(h) = inner.chromecast.as_ref() {
                    h.pause().map_err(|e| e.to_string())?;
                }
            }
            CastProtocol::Dlna => {
                let mut inner = self.inner.lock().await;
                if let Some(c) = inner.dlna.as_mut() {
                    c.pause().await.map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }

    async fn stop_renderer(&self) -> Result<(), String> {
        let proto = {
            let inner = self.inner.lock().await;
            match inner.protocol {
                Some(p) => p,
                None => return Ok(()),
            }
        };
        match proto {
            CastProtocol::Chromecast => {
                let inner = self.inner.lock().await;
                if let Some(h) = inner.chromecast.as_ref() {
                    h.stop().map_err(|e| e.to_string())?;
                }
            }
            CastProtocol::Dlna => {
                let mut inner = self.inner.lock().await;
                if let Some(c) = inner.dlna.as_mut() {
                    c.stop().await.map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }

    // ---- Position poll + ended detection ------------------------------------

    fn start_position_poll(self: &Arc<Self>) {
        let svc = self.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(POSITION_POLL_INTERVAL_MS))
                    .await;
                if !svc.is_casting().await {
                    break;
                }
                svc.poll_once().await;
            }
        });
        // Replace any prior task.
        let svc2 = self.clone();
        tokio::spawn(async move {
            let mut inner = svc2.inner.lock().await;
            if let Some(old) = inner.poll_task.replace(task) {
                old.abort();
            }
        });
    }

    async fn poll_once(self: &Arc<Self>) {
        let proto = {
            let inner = self.inner.lock().await;
            match inner.protocol {
                Some(p) => p,
                None => return,
            }
        };

        // Read position/state from the active renderer.
        let (position, duration, state, playing) = match proto {
            CastProtocol::Chromecast => {
                let info: Option<CastPositionInfo> = {
                    let inner = self.inner.lock().await;
                    inner.chromecast.as_ref().and_then(|h| h.get_media_position().ok())
                };
                match info {
                    Some(i) => {
                        let st = i.player_state.to_uppercase();
                        let playing = st == "PLAYING";
                        (i.position_secs, i.duration_secs, st, playing)
                    }
                    None => return,
                }
            }
            CastProtocol::Dlna => {
                let info: Option<DlnaPositionInfo> = {
                    let inner = self.inner.lock().await;
                    match inner.dlna.as_ref() {
                        Some(c) => c.get_position_info().await.ok(),
                        None => None,
                    }
                };
                match info {
                    Some(i) => {
                        let st = i.transport_state.to_uppercase();
                        let playing = st == "PLAYING";
                        (i.position_secs as f64, i.duration_secs as f64, st, playing)
                    }
                    None => return,
                }
            }
        };

        // Many DLNA renderers report TrackDuration as 0 / NOT_IMPLEMENTED in
        // GetPositionInfo, which left the seekbar permanently full (position/0 ->
        // clamped to 1.0) with no total time. Fall back to the track's catalog
        // duration (the position from the renderer is still authoritative).
        let duration = if duration > 0.0 {
            duration
        } else {
            self.runtime
                .core()
                .current_track()
                .await
                .map(|t| t.duration_secs as f64)
                .unwrap_or(0.0)
        };

        // Track-end detection (mirrors castStore): Chromecast {PLAYING,BUFFERING}
        // -> IDLE; DLNA PLAYING -> {STOPPED, NO_MEDIA_PRESENT}. One-shot latch,
        // reset on PLAYING.
        //
        // For DLNA a bare STOPPED is ambiguous: a strict renderer that hiccups
        // mid-track also reports STOPPED. We only treat it as end-of-track when
        // the track actually reached (near) its end — guarded by the max
        // position observed while PLAYING. A premature STOPPED is logged and
        // NOT advanced, and is not latched so a resume to PLAYING recovers.
        let max_position;
        let ended = {
            let mut inner = self.inner.lock().await;
            inner.is_playing = playing;
            if state == "PLAYING" {
                inner.cast_saw_playing = true;
                inner.cast_max_position = inner.cast_max_position.max(position);
            }
            max_position = inner.cast_max_position;
            let ended = match proto {
                CastProtocol::Chromecast => state == "IDLE" && !inner.track_end_detected,
                CastProtocol::Dlna => {
                    let stopped =
                        matches!(state.as_str(), "STOPPED" | "NO_MEDIA_PRESENT");
                    // The guard only makes sense when the position signal is
                    // usable. `duration` almost always resolves (catalog
                    // fallback), so the real escape hatches are: renderers
                    // whose RelTime never moves (max stays ~0 — honor STOPPED
                    // like pre-guard behavior) and the anti-wedge latch below.
                    let position_reliable =
                        inner.cast_max_position > CAST_POSITION_SIGNAL_MIN_SECS;
                    let near_end = duration <= 0.0
                        || !position_reliable
                        || max_position >= duration - CAST_END_GUARD_SECS;
                    if stopped && inner.cast_saw_playing && !near_end {
                        inner.cast_premature_stop_polls += 1;
                        log::warn!(
                            "[Cast] premature STOPPED {}/{} — not advancing yet \
                             (state={state}, max_position={max_position:.1}, \
                             duration={duration:.1})",
                            inner.cast_premature_stop_polls,
                            CAST_PREMATURE_STOP_POLLS_MAX
                        );
                    } else if !stopped {
                        inner.cast_premature_stop_polls = 0;
                    }
                    // A guard must never wedge the queue: a STOPPED that
                    // persists across the latch window is honored even when
                    // the position math says "premature".
                    let persistent_stop =
                        inner.cast_premature_stop_polls >= CAST_PREMATURE_STOP_POLLS_MAX;
                    stopped
                        && inner.cast_saw_playing
                        && (near_end || persistent_stop)
                        && !inner.track_end_detected
                }
            };
            if state == "PLAYING" {
                inner.track_end_detected = false;
            } else if ended {
                inner.track_end_detected = true;
            }
            ended
        };

        log::debug!(
            "[Cast] poll: state={state} position={position:.1} duration={duration:.1} \
             max_position={max_position:.1}"
        );

        // Feed the lyrics engine our position so it auto-follows while casting
        // (the local poll is skipped, so it can't drive lyrics). The 30Hz sync
        // engine extrapolates between these 1s ticks, same as the QConnect path.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        crate::lyrics_sync::publish_remote_anchor(
            (position.max(0.0) * 1000.0) as u64,
            now_ms,
            playing,
        );

        // Push position to CastState + the now-playing bar (the local poll is
        // stopped while casting, so the cast poll drives the bar).
        // Mirror the flag onto the visualizer tap too: the local poll (which
        // normally owns it) is skipped while casting, and a paused renderer
        // must park the FFT producer like a local pause does.
        if let Some(tap) = self.runtime.visualizer_tap() {
            tap.set_paused(!playing);
        }
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            let cs = w.global::<CastState>();
            cs.set_position_secs(position as f32);
            cs.set_duration_secs(duration as f32);
            cs.set_is_playing(playing);
            let np = w.global::<NowPlayingState>();
            np.set_progress((position / duration.max(1.0)).clamp(0.0, 1.0) as f32);
            np.set_position_secs(position as i32);
            np.set_duration_secs(duration as i32);
            np.set_seekable_max(1.0);
            np.set_elapsed(format_mmss(position).into());
            np.set_remaining(format_mmss((duration - position).max(0.0)).into());
            np.set_playing(playing);
        });

        if ended {
            log::info!(
                "[Cast] track ended (state={state}, position={position:.1}, \
                 duration={duration:.1}, max_position={max_position:.1}); auto-advancing"
            );
            // Run the SAME local advance the next button uses: it moves the
            // core cursor, refreshes the card + queue, and play_audible casts
            // the new current track. Keeps the UI and renderer in sync.
            crate::playback::next(
                self.runtime.clone(),
                self.window.clone(),
                tokio::runtime::Handle::current(),
            );
        }
    }

    // ---- QConnect coexistence ----------------------------------------------

    async fn suspend_qconnect_if_on(&self) {
        let Some(qc) = crate::qconnect_service::service() else {
            return;
        };
        if qc.is_running().await {
            self.inner.lock().await.qconnect_was_on_before_cast = true;
            if let Err(e) = qc.disconnect().await {
                log::warn!("[Cast] QConnect suspend failed (continuing): {e}");
            }
        }
    }

    async fn restore_qconnect(&self) {
        let Some(qc) = crate::qconnect_service::service() else {
            return;
        };
        if let Err(e) = qc.connect().await {
            log::warn!("[Cast] QConnect restore failed: {e}");
        }
    }

    // ---- Shutdown (logout / app exit) --------------------------------------

    /// Tear down everything: stop the renderer, abort the poll, drop discovery
    /// and the media server. Fixes the Tauri logout/exit leaks (#32/#33).
    pub async fn shutdown(&self) {
        let _ = self.stop_renderer().await;
        let mut inner = self.inner.lock().await;
        if let Some(task) = inner.poll_task.take() {
            task.abort();
        }
        if let Some(h) = inner.chromecast.take() {
            let _ = h.disconnect();
        }
        if let Some(mut c) = inner.dlna.take() {
            let _ = c.disconnect();
        }
        if let Some(mut disco) = inner.chromecast_discovery.take() {
            let _ = disco.stop_discovery();
        }
        if let Some(mut disco) = inner.dlna_discovery.take() {
            let _ = disco.stop_discovery();
        }
        if let Some(mut server) = inner.media_server.take() {
            server.stop();
        }
        inner.protocol = None;
        inner.connected_device_ip = None;
        inner.connected_device_name = None;
        inner.current_track_id = None;
        inner.is_playing = false;
    }

    // ---- State push to the UI ----------------------------------------------

    fn push_devices(&self, chromecast: Vec<DiscoveredDevice>, dlna: Vec<DiscoveredDlnaDevice>) {
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            let cc_count = chromecast.len() as i32;
            let dl_count = dlna.len() as i32;
            let mut rows: Vec<crate::CastDevice> = Vec::with_capacity(chromecast.len() + dlna.len());
            for d in chromecast {
                rows.push(crate::CastDevice {
                    id: d.id.into(),
                    name: d.name.into(),
                    ip: d.ip.into(),
                    protocol: "chromecast".into(),
                    model: d.model.into(),
                    can_play: true,
                    can_set_volume: true,
                });
            }
            for d in dlna {
                rows.push(crate::CastDevice {
                    id: d.id.into(),
                    name: d.name.into(),
                    ip: d.ip.into(),
                    protocol: "dlna".into(),
                    model: d.model.into(),
                    can_play: d.has_av_transport,
                    can_set_volume: d.has_rendering_control,
                });
            }
            let model = std::rc::Rc::new(slint::VecModel::from(rows));
            let cs = w.global::<CastState>();
            cs.set_devices(model.into());
            cs.set_chromecast_count(cc_count);
            cs.set_dlna_count(dl_count);
        });
    }

    async fn push_connection_state(&self) {
        let (connected, protocol, name, playing) = {
            let inner = self.inner.lock().await;
            (
                inner.protocol.is_some(),
                inner.protocol.map(|p| p.as_str().to_string()).unwrap_or_default(),
                inner.connected_device_name.clone().unwrap_or_default(),
                inner.is_playing,
            )
        };
        // Same tap mirror as poll_once: while connected this owns the bar's
        // playing flag, so it owns the producer gate too.
        if connected {
            if let Some(tap) = self.runtime.visualizer_tap() {
                tap.set_paused(!playing);
            }
        }
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            let cs = w.global::<CastState>();
            cs.set_connected(connected);
            cs.set_protocol(protocol.clone().into());
            cs.set_device_name(name.into());
            cs.set_is_playing(playing);
            let np = w.global::<NowPlayingState>();
            np.set_cast_active(connected);
            np.set_cast_protocol(protocol.into());
            // Keep the bar's play/pause icon in sync immediately (the local poll
            // is skipped while casting).
            if connected {
                np.set_playing(playing);
            }
        });
    }

    async fn push_quality(&self, label: String, detail: String) {
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            let cs = w.global::<CastState>();
            cs.set_quality_label(label.into());
            cs.set_quality_detail(detail.into());
        });
    }
}

// ---- Free helpers -----------------------------------------------------------

/// Resolve the on-disk path for a local/ephemeral track id (mirrors
/// `playback::local_track_file_exists` lookup).
fn resolve_local_path(track_id: u64) -> Option<String> {
    if crate::ephemeral::is_ephemeral_id(track_id as i64) {
        crate::ephemeral::get_track(track_id as i64).map(|row| row.file_path)
    } else {
        crate::library_db::with_db(|db| db.get_track(track_id as i64))
            .flatten()
            .map(|row| row.file_path)
    }
}

/// Content type for a local file by extension (for the UI label; the served
/// MIME is set by the crate's own map in register_file).
fn content_type_for_local(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "m4a" | "alac" | "mp4" => "audio/mp4",
        "aiff" | "aif" => "audio/aiff",
        "ape" => "audio/x-ape",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "aac" => "audio/aac",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn media_metadata(track: &QueueTrack) -> MediaMetadata {
    MediaMetadata {
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        artwork_url: track.artwork_url.clone(),
        duration_secs: Some(track.duration_secs),
    }
}

fn dlna_metadata(track: &QueueTrack) -> DlnaMetadata {
    DlnaMetadata {
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        artwork_url: track.artwork_url.clone(),
        duration_secs: Some(track.duration_secs),
    }
}

/// Quality label + detail from the track's CATALOG metadata (always present,
/// no network call) — used regardless of whether the bytes came from cache,
/// offline, or the network. Returns (label, detail) e.g. ("Hi-Res FLAC",
/// "96 kHz / 24-bit").
fn quality_label_from_track(track: &QueueTrack) -> (String, String) {
    let detail = match (track.sample_rate, track.bit_depth) {
        (Some(khz), Some(bits)) => format!("{} kHz / {}-bit", trim_khz(khz), bits),
        (Some(khz), None) => format!("{} kHz", trim_khz(khz)),
        (None, Some(bits)) => format!("{}-bit", bits),
        (None, None) => String::new(),
    };
    let label = if track.hires { "Hi-Res FLAC" } else { "FLAC" }.to_string();
    (label, detail)
}

/// Format seconds as "m:ss" for the now-playing bar elapsed label.
fn format_mmss(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Format a kHz value without a trailing ".0" (96.0 -> "96", 44.1 -> "44.1").
fn trim_khz(khz: f64) -> String {
    if (khz.fract()).abs() < f64::EPSILON {
        format!("{}", khz as i64)
    } else {
        format!("{:.1}", khz)
    }
}
