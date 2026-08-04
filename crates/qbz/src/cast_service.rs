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
//! `CoreBridge::fetch_for_external_stream_resolved`, requesting the user's
//! streaming-quality preference (#638; reverses the old always-UltraHiRes
//! decision D3) clamped by the manual per-renderer cap when one is stored
//! for the connected device (#638 fix 4). Caps govern only what we REQUEST —
//! bytes already in the L1/L2 cache or the offline store are served as-is,
//! never resampled. The LOCAL output device's cap never applies here: the
//! local DAC is not in a cast's signal path (precedence rule, owner
//! decision 2026-07-20).
//! Before registration the served bytes are PROBED (FLAC STREAMINFO), so the
//! picker line and the now-playing badge report the MEASURED delivered
//! quality — never the catalog metadata — regardless of which tier produced
//! the bytes, with an explicit disclosure when locally-existing bytes exceed
//! the requested tier (#638 fix 1).
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
use qbz_models::{probe_streaminfo, AssetOrigin, AudioParams, Quality, QualityLimit, QueueTrack};
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

/// How many position polls between renderer-volume refreshes. The volume is
/// authoritative on the DEVICE (its own remote/app can move it), so the slider
/// re-reads it periodically — but far less often than the position, since it is
/// an extra round trip to the renderer and volume rarely changes behind our back.
const CAST_VOLUME_REFRESH_POLLS: u32 = 5;

/// Ignore a refreshed volume within this window after a local drag: the renderer
/// may not have applied the new value yet, and pushing its pre-drag reading back
/// onto the bar would fight the user's own slider.
const CAST_VOLUME_ECHO_GUARD: std::time::Duration = std::time::Duration::from_secs(3);

/// Minimum change before a refreshed renderer volume moves the slider. Absorbs
/// the rounding of the 0..1 fraction into the device's integer scale (and back)
/// so a steady renderer doesn't jitter the bar by fractions of a percent.
const CAST_VOLUME_EPSILON: f32 = 0.02;

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

/// What `register_*` learned about the asset it just registered (#638 fix 1):
/// the MIME for the renderer, the measured STREAMINFO probe of the bytes
/// actually served (None = non-FLAC / local file), where those bytes came
/// from, and the tier the resolver was asked for (None = the source is not
/// governed by the streaming preference — local files).
struct CastAssetInfo {
    content_type: String,
    probe: Option<AudioParams>,
    origin: Option<AssetOrigin>,
    requested: Option<Quality>,
    /// Request-time cause paired with `requested` (#638 fix 4): which cap
    /// shaped the request (`Preference` / `RendererCap`), `None` when
    /// nothing constrained it or the source is not governed.
    request_cause: QualityLimit,
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
    // Stable identity of the connected renderer + the ui_prefs cap key
    // derived from it (#638 fix 4). `connected_cap_key` is None when no
    // persistable identity exists (a Chromecast without the mDNS TXT `id`
    // record — a fullname-keyed cap would silently detach on rename), and
    // the picker hides the cap row for that device.
    connected_device_id: Option<String>,
    connected_cap_key: Option<String>,
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
    // Renderer volume mirror (0..1). `cast_volume` is the last value we read
    // from or sent to the renderer — the refresh only touches the slider when
    // the device drifts away from it. `volume_set_at` stamps the last local
    // drag so an in-flight read can't snap the slider back mid-drag.
    cast_volume: Option<f32>,
    volume_set_at: Option<std::time::Instant>,
    // Poll ticks since the last renderer-volume refresh.
    volume_poll_tick: u32,
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
            // Identity + cap-key line so a "my cap doesn't apply" report is
            // diagnosable from the log alone (#638 fix 4).
            log::info!(
                "[Cast] connected to {} ({}; cap key: {})",
                inner.connected_device_name.as_deref().unwrap_or("?"),
                inner.connected_device_id.as_deref().unwrap_or("?"),
                inner.connected_cap_key.as_deref().unwrap_or("none — unstable id")
            );
        }
        self.push_connection_state().await;
        self.push_device_cap_row().await;
        // Adopt the renderer's OWN volume before the user can touch the slider.
        // Skipping this leaves the bar showing the local volume, so the first
        // drag jumps the renderer to that value — dragging down from a local
        // 100% to 70% turns a speaker sitting at 20% UP, hard.
        self.refresh_renderer_volume(true).await;
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
        // Cap key only when the id is the mDNS TXT `id` record (the Cast
        // UUID). The fullname fallback tracks the friendly name, so a cap
        // keyed on it would silently stop applying on rename — the failure
        // mode #638 exists to remove; such devices get no cap row.
        inner.connected_cap_key = device
            .id_is_stable
            .then(|| format!("chromecast:{}", device.id));
        if !device.id_is_stable {
            log::info!(
                "[Cast] {} broadcasts no mDNS TXT id — per-device quality cap unavailable",
                device.name
            );
        }
        inner.connected_device_id = Some(device.id);
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
        let udn = device.id.clone();
        let conn = DlnaConnection::connect(device)
            .await
            .map_err(|e| e.to_string())?;
        let mut inner = self.inner.lock().await;
        inner.dlna = Some(conn);
        inner.connected_device_name = Some(name);
        // The DLNA id IS the UPnP UDN — stable by construction (F14), so a
        // DLNA renderer is always cappable.
        inner.connected_cap_key = Some(format!("dlna:{udn}"));
        inner.connected_device_id = Some(udn);
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
            inner.connected_device_id = None;
            inner.connected_cap_key = None;
            // Release the served track buffers with the session (#550); the
            // server itself stays up for the next connect.
            if let Some(server) = inner.media_server.as_ref() {
                server.clear_entries();
            }
            inner.current_track_id = None;
            inner.is_playing = false;
            inner.track_end_detected = false;
            // The bar goes back to showing the LOCAL volume (the local poll
            // re-owns it within a tick), so the renderer's mirror is stale.
            inner.cast_volume = None;
            inner.volume_set_at = None;
            inner.volume_poll_tick = 0;
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
        // Clear the fix-1 measured/over-cap disclosure with the session (the
        // quality line itself is hidden by `connected == false`; the badge's
        // effective values are re-owned by the local poll on its next tick).
        // The fix-4 cap row is per-connection state — cleared with it.
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(|w| {
            use slint::ComponentHandle;
            let cs = w.global::<CastState>();
            cs.set_quality_limit_cause(0);
            cs.set_quality_over_cap(false);
            cs.set_quality_origin("".into());
            cs.set_device_cap_available(false);
            cs.set_device_cap_key("".into());
            cs.set_device_cap_index(0);
        });
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
        let info = match source {
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
        let content_type = info.content_type.clone();

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
                let result = async {
                    conn.load_media(&url, &dlna_metadata(track), &content_type)
                        .await
                        .map_err(|e| e.to_string())?;
                    conn.play().await.map_err(|e| e.to_string())?;
                    Ok::<(), String>(())
                }
                .await;
                if let Err(e) = result {
                    // Best-effort reset so the renderer doesn't sit half-loaded
                    // on a URI it already faulted (#646: a rejected stream left
                    // the renderer wedged and every retry re-routed to it).
                    let _ = conn.stop().await;
                    return Err(e);
                }
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
        // Delivered quality for the picker line (#638 fix 1): MEASURED from
        // the served bytes when the probe can read them, falling back to the
        // track's catalog metadata (non-FLAC / local files). The old
        // catalog-only label could read "24-bit / 96 kHz" while CD bytes were
        // on the wire (F20).
        let (quality_label, quality_detail) = match info.probe {
            Some(p) => (
                if p.bits_per_sample >= 24 {
                    "Hi-Res FLAC"
                } else {
                    "FLAC"
                }
                .to_string(),
                crate::quality::detail(Some(p.bits_per_sample), Some(p.sample_rate as f64)),
            ),
            None => quality_label_from_track(track),
        };
        self.push_quality(quality_label, quality_detail).await;
        // Un-stale the now-playing badge + disclose over-cap serves: the
        // local poll (which normally drives the 85e11d28 properties) is
        // skipped while casting.
        self.publish_measured_badge(&info).await;
        self.push_connection_state().await;
        Ok(())
    }

    /// qobuz: resolve via the shared core API (cache -> offline -> network),
    /// probe the served bytes' STREAMINFO, register them. Returns the asset
    /// info for the picker/badge publish.
    async fn register_qobuz(&self, track_id: u64) -> Result<CastAssetInfo, String> {
        let offline = crate::offline::get().await;
        let sink = crate::offline_cache::row_sink(self.window.clone());
        // The streaming-quality preference — clamped by this renderer's
        // manual cap when one is stored (#638 fix 4) — governs what we
        // REQUEST from Qobuz, resolved fresh per cast track so a Settings or
        // cap change applies to the very next one. This request-time min is
        // the ENTIRE enforcement: bytes that already exist locally (L1/L2
        // cache, the offline store) go out as-is at whatever tier they were
        // fetched at — no resampling, no re-fetch, ever (owner decision).
        let cap_key = self.inner.lock().await.connected_cap_key.clone();
        let (quality, request_cause) = self.effective_cast_quality(cap_key.as_deref());
        let asset = self
            .runtime
            .core()
            .fetch_for_external_stream_resolved(
                track_id,
                quality,
                offline.as_deref(),
                Some(&sink),
            )
            .await
            .ok_or_else(|| format!("Could not resolve stream for track {track_id}"))?;

        log::info!("[Cast] qobuz track {track_id} resolved from {:?}", asset.origin);
        let content_type = asset.content_type.clone();
        // Measure BEFORE register_audio moves the bytes: the probe reports the
        // truth regardless of which tier (network / L1/L2 cache / offline
        // store) served them — the same measure-the-bytes philosophy as the
        // local path's cached_quality_below_requested gate (F17/F24).
        let probe = probe_streaminfo(&asset.bytes);
        let origin = asset.origin;

        self.ensure_media_server().await?;
        {
            let mut inner = self.inner.lock().await;
            let server = inner.media_server.as_mut().ok_or("Media server gone")?;
            server.register_audio(track_id, asset.bytes, &content_type);
        }
        Ok(CastAssetInfo {
            content_type,
            probe,
            origin: Some(origin),
            requested: Some(quality),
            request_cause,
        })
    }

    /// Resolve the tier to REQUEST for a cast to the renderer keyed
    /// `cap_key`, plus the request-time cause: the user's streaming
    /// preference clamped by the manual per-renderer cap, lowest tier wins
    /// (#638 fix 4). On a tie the cause is `RendererCap` — the more
    /// specific, more surprising of the two (spec §2.2 mandate). NEVER
    /// consults the local DAC cap (`device_cap` / `local_playback_quality`):
    /// the local DAC is not in a cast's signal path (precedence rule, owner
    /// decision 2026-07-20).
    fn effective_cast_quality(&self, cap_key: Option<&str>) -> (Quality, QualityLimit) {
        let pref = crate::playback::playback_quality();
        // Second (deliberate) tiny ui_prefs read per cast track: the pref
        // stays sourced from the canonical playback_quality() — its purity
        // doc names this module as a direct caller — and the cap map is
        // read fresh beside it.
        let cap = cap_key.and_then(|k| {
            crate::ui_prefs::load()
                .cast_quality_caps
                .get(k)
                .map(|c| crate::ui_prefs::streaming_quality_for_key(&c.tier))
        });
        match cap {
            // A stored `hires_plus`/unknown tier resolves to UltraHiRes via
            // the mapper's fallback = no effective cap (and never a
            // RendererCap cause) — only real caps take these two arms.
            Some(cap) if cap < pref => {
                (Quality::min_tier(cap, pref), QualityLimit::RendererCap)
            }
            Some(cap) if cap == pref && cap < Quality::UltraHiRes => {
                (pref, QualityLimit::RendererCap)
            }
            _ => (
                pref,
                if pref < Quality::UltraHiRes {
                    QualityLimit::Preference
                } else {
                    QualityLimit::None
                },
            ),
        }
    }

    /// The tier the NEXT cast request would resolve to right now (the
    /// streaming preference clamped by the connected renderer's manual cap),
    /// or `None` when no renderer is connected — the same gate `is_casting`
    /// uses, so this agrees with `play_audible`'s routing. Lets
    /// `kick_prefetch` warm the quality-blind L1/L2 cache at the tier the
    /// cast will actually request: the cast resolve is cache-first, so bytes
    /// prefetched at any OTHER tier (notably the local-DAC-capped one) would
    /// go out to the renderer verbatim, leaking a cap from a device that is
    /// not in the cast's signal path (#638 precedence rule).
    pub async fn casting_prefetch_quality(&self) -> Option<Quality> {
        let cap_key = {
            let inner = self.inner.lock().await;
            if inner.protocol.is_none() {
                return None;
            }
            inner.connected_cap_key.clone()
        };
        Some(self.effective_cast_quality(cap_key.as_deref()).0)
    }

    /// local: stream the file from disk via register_file (no full-RAM read);
    /// the crate's rich MIME map sets the content type. No probe/origin/
    /// requested-tier: local files are not governed by the streaming
    /// preference and the picker keeps its catalog-metadata fallback.
    async fn register_local(&self, track_id: u64, path: &str) -> Result<CastAssetInfo, String> {
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
        Ok(CastAssetInfo {
            content_type,
            probe: None,
            origin: None,
            requested: None,
            request_cause: QualityLimit::None,
        })
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

    /// Seek to a 0..1 fraction of the CURRENT cast track.
    ///
    /// The seekbar cannot derive the absolute position from the local core's
    /// playback duration while casting: the local backend is stopped, so its
    /// duration reads 0 (or a stale value from the last local track), and
    /// `fraction * duration` collapses to ~0 — every drag restarts the track.
    /// Resolve the real duration from the cast track's catalog metadata (the
    /// same source the position poll uses) instead.
    pub async fn seek_fraction_if_cast(&self, fraction: f64) -> Result<bool, String> {
        if !self.is_casting().await {
            return Ok(false);
        }
        let dur = self
            .runtime
            .core()
            .current_track()
            .await
            .map(|t| t.duration_secs as f64)
            .unwrap_or(0.0);
        if dur <= 0.0 {
            // No usable duration — swallow the seek rather than jump to 0 and
            // restart the track. Reports handled (true) so the caller stops.
            return Ok(true);
        }
        let secs = (fraction.clamp(0.0, 1.0) * dur).max(0.0);
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
        // Remember what the renderer was just told, and when: the periodic
        // refresh compares against this instead of pushing every reading, and
        // stays out of the way while the drag settles.
        {
            let mut inner = self.inner.lock().await;
            inner.cast_volume = Some(v);
            inner.volume_set_at = Some(std::time::Instant::now());
            inner.volume_poll_tick = 0;
        }
        // Reflect the drag on the bar: the local set_volume (which normally
        // moves the slider) is skipped while casting.
        self.push_volume(v);
        Ok(true)
    }

    /// Read the connected renderer's REAL volume and mirror it onto the bar.
    ///
    /// The renderer owns its volume — its own remote or app can move it, and it
    /// has its own idea of what "full" means (see `VolumeRange` in qbz-cast).
    /// Whenever the bar shows something else, the next drag is a jump rather
    /// than an adjustment.
    ///
    /// `adopt` (connect) takes the reading unconditionally. The periodic refresh
    /// passes `false`: it defers to a recent local drag and ignores changes too
    /// small to be anything but scale rounding. Best-effort throughout — a
    /// renderer with no volume control, or one that faults, leaves the bar as is.
    async fn refresh_renderer_volume(&self, adopt: bool) {
        let (proto, last_known, set_at) = {
            let inner = self.inner.lock().await;
            match inner.protocol {
                Some(p) => (p, inner.cast_volume, inner.volume_set_at),
                None => return,
            }
        };
        if !adopt {
            if let Some(at) = set_at {
                if at.elapsed() < CAST_VOLUME_ECHO_GUARD {
                    return;
                }
            }
        }

        let reading = match proto {
            CastProtocol::Chromecast => {
                let status = {
                    let inner = self.inner.lock().await;
                    inner.chromecast.as_ref().map(|h| h.get_status())
                };
                match status {
                    Some(Ok(s)) => s.volume_level,
                    Some(Err(e)) => {
                        log::debug!("[Cast] volume read failed: {e}");
                        None
                    }
                    None => None,
                }
            }
            CastProtocol::Dlna => {
                let inner = self.inner.lock().await;
                let Some(conn) = inner.dlna.as_ref() else {
                    return;
                };
                match conn.get_volume().await {
                    Ok(v) => Some(v),
                    Err(e) => {
                        log::debug!("[Cast] volume read failed: {e}");
                        None
                    }
                }
            }
        };
        let Some(volume) = reading.map(|v| v.clamp(0.0, 1.0)) else {
            return;
        };

        let changed = last_known.is_none_or(|last| (volume - last).abs() >= CAST_VOLUME_EPSILON);
        if !adopt && !changed {
            return;
        }
        {
            let mut inner = self.inner.lock().await;
            // Lost the connection while the read was in flight.
            if inner.protocol != Some(proto) {
                return;
            }
            // A drag landed WHILE the read was in flight, so this reading is
            // already stale — the user's value is the newer intent, and the
            // renderer has it. Re-checked here (not only up front) because the
            // read is a full round trip to the device.
            if inner
                .volume_set_at
                .is_some_and(|at| at.elapsed() < CAST_VOLUME_ECHO_GUARD)
            {
                return;
            }
            inner.cast_volume = Some(volume);
        }
        log::info!("[Cast] renderer volume is {:.0}%", volume * 100.0);
        self.push_volume(volume);
    }

    fn push_volume(&self, volume: f32) {
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            w.global::<NowPlayingState>().set_volume(volume);
        });
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

        // Re-read the renderer's volume on a slower cadence than the position:
        // the device owns it, so a change made on the device itself (its remote,
        // its own app) has to reach the bar or the next drag jumps.
        let refresh_volume = {
            let mut inner = self.inner.lock().await;
            inner.volume_poll_tick = inner.volume_poll_tick.saturating_add(1);
            let due = inner.volume_poll_tick >= CAST_VOLUME_REFRESH_POLLS;
            if due {
                inner.volume_poll_tick = 0;
            }
            due
        };
        if refresh_volume {
            self.refresh_renderer_volume(false).await;
        }

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
        inner.connected_device_id = None;
        inner.connected_cap_key = None;
        inner.current_track_id = None;
        inner.is_playing = false;
        inner.cast_volume = None;
        inner.volume_set_at = None;
        inner.volume_poll_tick = 0;
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

    /// Publish the measured delivered-quality state the SKIPPED local poll
    /// would have published (#638 fix 1 cast half): the local player is
    /// stopped while casting, so without this the badge's downgrade
    /// arrow/tooltip (85e11d28) go stale. Also computes the over-cap
    /// disclosure for the picker line — bytes that already existed locally
    /// (cache / offline store) are served AS-IS even above the requested
    /// tier, never resampled and never re-fetched (owner decision), and the
    /// UI says so instead of hiding it. NEVER consults the local DAC cap:
    /// the local DAC is not in a cast's signal path (#638 precedence rule).
    async fn publish_measured_badge(&self, info: &CastAssetInfo) {
        let (eff_rate_hz, eff_bits) = info
            .probe
            .map(|p| (p.sample_rate, p.bits_per_sample))
            .unwrap_or((0, 0));
        // Catalog maxima seeded by refresh_now_playing_meta for this same
        // track (the cast is always the current queue track).
        let (max_rate_hz, max_bits) = crate::playback::track_catalog_max();
        let downgraded =
            crate::playback::stream_downgraded(eff_rate_hz, eff_bits, max_rate_hz, max_bits);
        let requested_id = info.requested.map(|q| q.id()).unwrap_or(0);
        // Request-time cause from the cast resolution
        // (`effective_cast_quality`): Preference or RendererCap (#638
        // fix 4). The local device cap is NEVER a cast cause.
        let request_cause = info.request_cause as i32;
        let limit_cause = crate::playback::classify_limit_cause(
            downgraded,
            requested_id,
            request_cause,
            eff_bits,
        );
        // Tier token for the badge's main line. The shared helper's
        // requested-mp3 shortcut is for the LOCAL engine path (an MP3 stream
        // decodes to 16-bit PCM, so the requested tier is the only tell);
        // here a successful STREAMINFO parse proves the served bytes are
        // FLAC, never MP3 — an over-cap cache/offline serve under an mp3
        // cap must not read "MP3" beside a measured "24-bit / 96 kHz".
        let delivered_tier = if downgraded && info.probe.is_some() {
            if eff_bits >= 24 {
                "hires"
            } else {
                "cd"
            }
        } else {
            crate::playback::delivered_tier_str(downgraded, requested_id, eff_bits)
        };
        // Measured line for the tooltip's "Output", same formatter as the
        // local poll ("16-bit / 44.1 kHz").
        let true_detail = if eff_rate_hz > 0 || eff_bits > 0 {
            crate::quality::detail(
                (eff_bits > 0).then_some(eff_bits),
                (eff_rate_hz > 0).then_some(eff_rate_hz as f64),
            )
        } else {
            String::new()
        };
        // Over-cap: locally-existing bytes ABOVE the requested tier. Measured
        // tier via the F24-style inverse of the probe (24-bit AND >96 kHz →
        // Hi-Res+; 24-bit → Hi-Res; else CD-class FLAC).
        let measured_tier = info.probe.map(|p| {
            if p.bits_per_sample >= 24 && p.sample_rate > 96_000 {
                Quality::UltraHiRes
            } else if p.bits_per_sample >= 24 {
                Quality::HiRes
            } else {
                Quality::Lossless
            }
        });
        let over_cap = matches!(info.origin, Some(o) if o != AssetOrigin::Network)
            && matches!(
                (measured_tier, info.requested),
                (Some(m), Some(r)) if m > r
            );
        let origin_str = match info.origin {
            Some(AssetOrigin::Cache) => "cache",
            Some(AssetOrigin::Offline) => "offline",
            _ => "",
        };
        if over_cap {
            log::info!(
                "[Cast] serving {origin_str} bytes above the requested tier \
                 (measured {measured_tier:?} > requested {:?}) — caps govern \
                 requests only; local bytes go out as-is, never resampled",
                info.requested
            );
        }
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            let np = w.global::<NowPlayingState>();
            np.set_effective_sample_rate_hz(eff_rate_hz as i32);
            np.set_effective_bit_depth(eff_bits as i32);
            np.set_quality_downgraded(downgraded);
            np.set_quality_true_detail(true_detail.into());
            np.set_quality_limit_cause(limit_cause);
            np.set_quality_effective_tier(delivered_tier.into());
            let cs = w.global::<CastState>();
            cs.set_quality_limit_cause(limit_cause);
            cs.set_quality_over_cap(over_cap);
            cs.set_quality_origin(origin_str.into());
        });
    }

    /// Push the per-renderer cap row state for the connected device (#638
    /// fix 4): whether a cap can be offered at all (stable identity only),
    /// the ui_prefs key the picker hands back on selection, and the current
    /// index (0 = follow the app setting — the absent-entry default).
    async fn push_device_cap_row(&self) {
        let cap_key = self.inner.lock().await.connected_cap_key.clone();
        let index = cap_key.as_deref().map(cap_index_for_key).unwrap_or(0);
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            let cs = w.global::<CastState>();
            cs.set_device_cap_available(cap_key.is_some());
            cs.set_device_cap_key(cap_key.unwrap_or_default().into());
            cs.set_device_cap_index(index);
        });
    }

    /// Persist the user's manual cap choice for the renderer keyed
    /// `cap_key` (#638 fix 4): index 0 removes the entry (follow the app
    /// setting), 1/2/3 store hires/cd/mp3. Enforcement is request-time
    /// only — the NEXT cast resolve picks the change up; nothing is
    /// cleared, re-fetched or restarted (a per-device cap must never
    /// punish the global cache or break an in-flight cast — owner
    /// decision C; an over-cap cached serve stays disclosed and self-heals
    /// on natural cache turnover).
    pub async fn set_device_cap(&self, cap_key: String, index: i32) {
        if cap_key.is_empty() {
            return;
        }
        let tier = match index {
            1 => Some("hires"),
            2 => Some("cd"),
            3 => Some("mp3"),
            _ => None,
        };
        let name = {
            let inner = self.inner.lock().await;
            inner.connected_device_name.clone().unwrap_or_default()
        };
        let mut prefs = crate::ui_prefs::load();
        match tier {
            Some(t) => {
                prefs.cast_quality_caps.insert(
                    cap_key.clone(),
                    crate::ui_prefs::CastDeviceCap {
                        tier: t.to_string(),
                        name: name.clone(),
                    },
                );
                log::info!("[Cast] quality cap for {name} ({cap_key}) -> {t}");
            }
            None => {
                prefs.cast_quality_caps.remove(&cap_key);
                log::info!(
                    "[Cast] quality cap for {name} ({cap_key}) removed — follows the app setting"
                );
            }
        }
        crate::ui_prefs::save(&prefs);
        // Reflect the persisted choice back (the dropdown binds to
        // CastState.device-cap-index, not to local widget state).
        let index = index.clamp(0, 3);
        let weak = self.window.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            use slint::ComponentHandle;
            w.global::<CastState>().set_device_cap_index(index);
        });
    }
}

// ---- Free helpers -----------------------------------------------------------

/// Dropdown index for the stored cap of `cap_key` (0 follow · 1 hires ·
/// 2 cd · 3 mp3). Unknown stored tiers read as "follow" so a hand-edited
/// value degrades to the no-cap default instead of showing a wrong cap.
fn cap_index_for_key(cap_key: &str) -> i32 {
    match crate::ui_prefs::load()
        .cast_quality_caps
        .get(cap_key)
        .map(|c| c.tier.as_str())
    {
        Some("hires") => 1,
        Some("cd") => 2,
        Some("mp3") => 3,
        _ => 0,
    }
}

/// Seed/refresh the cap dropdown option labels + the live global-quality
/// label on `CastState` (#638 fix 4). Called from `reseed_i18n_labels`
/// (startup + language change — Rust-pushed option models never
/// re-translate on their own) and from the Settings streaming-quality arm
/// (option 0 embeds the live global label, so it must track the setting).
/// UI-thread only.
pub fn push_cap_options(window: &AppWindow) {
    use slint::ComponentHandle;
    let prefs = crate::ui_prefs::load();
    let idx = crate::ui_prefs::streaming_quality_index(&prefs.streaming_quality);
    // Tier names ("Hi-Res+", "CD Quality") are product names — untranslated
    // data, same convention as the Settings streaming dropdown.
    let global_label = crate::ui_prefs::STREAMING_QUALITIES[idx].label;
    // Cap tiers use the SAME plain product names as the Settings streaming
    // dropdown (owner call, #638) — the old verbose "CD — 16-bit / 44.1 kHz"
    // form was misleading (a CD-tier track can be 48 kHz; the tier is a
    // format_id, not a fixed sample rate). Looked up by key so they can never
    // drift from `STREAMING_QUALITIES`. Index order matches `set_device_cap`:
    // 0 follow · 1 hires · 2 cd · 3 mp3. Product names are untranslated data.
    let label_for = |key: &str| {
        crate::ui_prefs::STREAMING_QUALITIES
            .iter()
            .find(|q| q.key == key)
            .map(|q| q.label)
            .unwrap_or("")
    };
    let options: Vec<slint::SharedString> = vec![
        qbz_i18n::t_args("Follow app setting ({})", &[global_label]).into(),
        label_for("hires").into(),
        label_for("cd").into(),
        label_for("mp3").into(),
    ];
    let cs = window.global::<CastState>();
    cs.set_global_quality_label(global_label.into());
    cs.set_device_cap_options(slint::ModelRc::new(slint::VecModel::from(options)));
}

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
        // DSD containers (MinimServer DLNA convention) — mirrors the served
        // MIME map in qbz-cast `content_type_for_path` (#646).
        "dsf" => "audio/x-dsf",
        "dff" => "audio/x-dff",
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

/// FALLBACK quality label + detail from the track's CATALOG metadata — used
/// only when the STREAMINFO probe cannot read the served bytes (non-FLAC /
/// local files); Qobuz FLAC casts report the MEASURED values instead (#638
/// fix 1). Returns (label, detail) e.g. ("Hi-Res FLAC", "24-bit / 96 kHz").
/// Detail order is bit-depth first to match `quality::detail` and every other
/// badge in the app (owner call, #638).
fn quality_label_from_track(track: &QueueTrack) -> (String, String) {
    // DSD tracks carry bit_depth=1 + the DSD bit rate as sample_rate — the
    // generic "{}-bit / {} kHz" format would print the nonsense
    // "1-bit / 2822.4 kHz". Use the DSD-aware label ("DSD64" …) both slots
    // report elsewhere (#646).
    if track.bit_depth == Some(1) {
        let dsd = crate::quality::dsd_multiple_label(track.sample_rate);
        return (dsd.clone(), dsd);
    }
    let detail = match (track.sample_rate, track.bit_depth) {
        (Some(khz), Some(bits)) => format!("{}-bit / {} kHz", bits, trim_khz(khz)),
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
