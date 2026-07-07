//! Diagnostics panel controller (Settings > Developer).
//!
//! Wires the `DiagnosticsState` Slint global. On `refresh()` it reads the three
//! settings stores (audio/graphics/developer) + computes the graphics runtime,
//! builds the frontend-agnostic `RuntimeDiagnostics` + `SystemInfo` snapshots
//! (`qbz_app::diagnostics`), snapshots the core player for the Playback rows, and
//! reads the LIVE Qobuz Connect session for the QConnect rows — then pushes all
//! seven per-section `[DiagRow]` models in one event-loop hop. Cast is the only
//! on-demand section: `cast-scan()` reuses the existing `CastService` discovery
//! and reads the populated `CastState`. Export serializes the cached snapshot
//! (camelCase, matching the Tauri DiagnosticsPanel export) to the clipboard.
//!
//! 1:1 port of `src/lib/components/DiagnosticsPanel.svelte` (the row builders),
//! over the shared backend extracted to `qbz_app::diagnostics`.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::{AppWindow, CastState, DiagRow, DiagnosticsState};

type Runtime = Arc<qbz_app::shell::AppRuntime<SlintAdapter>>;

/// The Diagnostics controller. Cloned into each `DiagnosticsState` callback.
#[derive(Clone)]
struct DiagController {
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    /// Cached export base built on each `refresh()` — a JSON object with the
    /// runtime-diagnostics fields flattened + `systemInfo` + `playback` +
    /// `qconnect`. `castScan` + `exportedAt` are merged in at export time.
    export: Arc<Mutex<Option<Value>>>,
    /// Last Cast scan result (camelCase), merged into the export as `castScan`.
    last_cast: Arc<Mutex<Option<Value>>>,
}

/// Wire every `DiagnosticsState` callback. Call once at shell setup.
pub fn install(window: &AppWindow, runtime: Runtime, handle: tokio::runtime::Handle) {
    let ctrl = DiagController {
        runtime,
        weak: window.as_weak(),
        handle,
        export: Arc::new(Mutex::new(None)),
        last_cast: Arc::new(Mutex::new(None)),
    };

    let state = window.global::<DiagnosticsState>();
    {
        let c = ctrl.clone();
        state.on_refresh(move || c.refresh());
    }
    {
        let c = ctrl.clone();
        state.on_export_clipboard(move || c.export_clipboard());
    }
    {
        let c = ctrl.clone();
        state.on_cast_scan(move || c.cast_scan());
    }
}

impl DiagController {
    /// Build the diagnostics snapshot and push the seven models. Called on the UI
    /// thread (Slint callback); flips `loading` immediately, then spawns the work.
    fn refresh(&self) {
        if let Some(w) = self.weak.upgrade() {
            w.global::<DiagnosticsState>().set_loading(true);
        }
        let this = self.clone();
        self.handle.spawn(async move {
            this.refresh_async().await;
        });
    }

    async fn refresh_async(&self) {
        // (a) blocking: the three settings stores + /proc + /sys reads.
        let collected = tokio::task::spawn_blocking(|| {
            let audio = qbz_audio::settings::AudioSettingsStore::new()
                .and_then(|s| s.get_settings())
                .unwrap_or_default();
            let (graphics, gfx_failed) =
                match qbz_app::settings::graphics::GraphicsSettingsStore::new()
                    .and_then(|s| s.get_settings())
                {
                    Ok(g) => (g, false),
                    Err(_) => (Default::default(), true),
                };
            let developer = qbz_app::settings::developer::DeveloperSettingsStore::new()
                .and_then(|s| s.get_settings())
                .unwrap_or_default();
            let gfx = qbz_app::diagnostics::detect_graphics_runtime(&graphics, gfx_failed);
            let runtime_diag =
                qbz_app::diagnostics::runtime_diagnostics(&qbz_app::diagnostics::DiagnosticsInputs {
                    audio: &audio,
                    graphics: &graphics,
                    developer: &developer,
                    gfx,
                    app_version: env!("CARGO_PKG_VERSION"),
                });
            let sys = qbz_app::diagnostics::system_info();
            // Live output sinks (BLOCKING CPAL enumeration — stays inside this
            // spawn_blocking, never on the async path).
            let (active_output, available_outputs, active_fmt) = collect_output_sinks();
            (runtime_diag, sys, active_output, available_outputs, active_fmt)
        })
        .await;

        let (runtime_diag, sys, active_output, available_outputs, active_fmt) = match collected {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[qbz-slint] diagnostics: settings read panicked: {e}");
                let weak = self.weak.clone();
                let _ = weak.upgrade_in_event_loop(|w| {
                    let d = w.global::<DiagnosticsState>();
                    d.set_loading(false);
                    d.set_error("Failed to read diagnostics".into());
                });
                return;
            }
        };

        // (b) async core snapshot for the Playback section.
        let pb = self.runtime.core().get_playback_state();
        let track = self.runtime.core().current_track().await;

        // (c) LIVE QConnect snapshot (no discovery; default when not running).
        let qc = match crate::qconnect_service::service() {
            Some(s) => s.diagnostics_snapshot().await,
            None => Default::default(),
        };

        // (d) build the seven row vectors (1:1 with the Tauri row builders).
        let system_rows = build_system_rows(&sys);
        let playback_rows = build_playback_rows(&pb, track.as_ref());
        let qconnect_rows = build_qconnect_rows(&qc);
        let audio_rows = build_audio_rows(
            &runtime_diag,
            active_output.as_deref(),
            &available_outputs,
            active_fmt
                .as_ref()
                .map(|(r, _)| r.as_str())
                .filter(|s| !s.is_empty()),
            active_fmt
                .as_ref()
                .map(|(_, f)| f.as_str())
                .filter(|s| !s.is_empty()),
        );
        let graphics_rows = build_graphics_rows(&runtime_diag);
        let env_rows = build_env_rows(&runtime_diag);

        // (e) cache the export base (runtimeDiag flattened + systemInfo +
        //     playback + qconnect). castScan + exportedAt are added at export.
        let playback_json = build_playback_json(&pb, track.as_ref());
        let qconnect_json = build_qconnect_json(&qc);
        let mut map = serde_json::Map::new();
        if let Ok(Value::Object(rd)) = serde_json::to_value(&runtime_diag) {
            for (k, v) in rd {
                map.insert(k, v);
            }
        }
        map.insert(
            "systemInfo".to_string(),
            serde_json::to_value(&sys).unwrap_or(Value::Null),
        );
        map.insert("playback".to_string(), playback_json);
        map.insert("qconnect".to_string(), qconnect_json);
        if let Ok(mut g) = self.export.lock() {
            *g = Some(Value::Object(map));
        }

        let app_version = runtime_diag.app_version.clone();

        // (f) one event-loop hop: push all seven models + version + flags.
        let weak = self.weak.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            let d = w.global::<DiagnosticsState>();
            d.set_system_rows(ModelRc::new(VecModel::from(system_rows)));
            d.set_playback_rows(ModelRc::new(VecModel::from(playback_rows)));
            d.set_qconnect_rows(ModelRc::new(VecModel::from(qconnect_rows)));
            d.set_audio_rows(ModelRc::new(VecModel::from(audio_rows)));
            d.set_graphics_rows(ModelRc::new(VecModel::from(graphics_rows)));
            d.set_env_rows(ModelRc::new(VecModel::from(env_rows)));
            d.set_app_version(app_version.into());
            d.set_loaded(true);
            d.set_loading(false);
            d.set_error("".into());
        });
    }

    /// Serialize the cached snapshot (+ last Cast scan + exportedAt) to the
    /// clipboard. Flips `copied` for 1.5s.
    fn export_clipboard(&self) {
        let base = self.export.lock().ok().and_then(|g| g.clone());
        let Some(mut value) = base else {
            return;
        };
        if let Some(map) = value.as_object_mut() {
            let cast = self.last_cast.lock().ok().and_then(|g| g.clone());
            map.insert("castScan".to_string(), cast.unwrap_or(Value::Null));
            map.insert(
                "exportedAt".to_string(),
                Value::String(chrono::Utc::now().to_rfc3339()),
            );
        }
        let json = serde_json::to_string_pretty(&value).unwrap_or_default();
        crate::share::copy_to_clipboard(json);

        if let Some(w) = self.weak.upgrade() {
            w.global::<DiagnosticsState>().set_copied(true);
        }
        let weak = self.weak.clone();
        self.handle.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<DiagnosticsState>().set_copied(false);
            });
        });
    }

    /// On-demand Cast discovery scan: reuse the existing `CastService`, wait 10s,
    /// then read the populated `CastState` to build the cast rows. Guarded against
    /// re-entrancy; stops discovery afterwards only if the picker isn't using it.
    fn cast_scan(&self) {
        if let Some(w) = self.weak.upgrade() {
            let st = w.global::<DiagnosticsState>();
            if st.get_cast_scanning() {
                return;
            }
            st.set_cast_scanning(true);
        }

        let this = self.clone();
        self.handle.spawn(async move {
            let Some(svc) = crate::cast_service::service() else {
                let _ = this.weak.upgrade_in_event_loop(|w| {
                    w.global::<DiagnosticsState>().set_cast_scanning(false);
                });
                return;
            };

            svc.start_discovery().await;
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            svc.refresh_devices().await;

            // Read the live CastState on the UI thread, build the rows, and report
            // the picker-open flag back so we can gate the discovery stop.
            let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
            let last_cast = this.last_cast.clone();
            let _ = this.weak.upgrade_in_event_loop(move |w| {
                let cs = w.global::<CastState>();
                let cc = cs.get_chromecast_count();
                let dl = cs.get_dlna_count();
                let devices = cs.get_devices();

                let mut rows: Vec<DiagRow> = Vec::new();
                rows.push(row("Chromecast devices", "—", &cc.to_string(), 0));
                rows.push(row("DLNA devices", "—", &dl.to_string(), 0));
                let mut device_json: Vec<Value> = Vec::new();
                for i in 0..devices.row_count() {
                    if let Some(dev) = devices.row_data(i) {
                        let protocol = dev.protocol.to_string();
                        let name = dev.name.to_string();
                        rows.push(row(&format!("• {protocol}"), "—", &name, 0));
                        device_json.push(json!({ "name": name, "protocol": protocol }));
                    }
                }

                if let Ok(mut g) = last_cast.lock() {
                    *g = Some(json!({
                        "chromecastCount": cc,
                        "dlnaCount": dl,
                        "devices": device_json,
                    }));
                }

                let d = w.global::<DiagnosticsState>();
                d.set_cast_rows(ModelRc::new(VecModel::from(rows)));
                d.set_cast_scanning(false);

                let _ = tx.send(cs.get_picker_open());
            });

            // Stop discovery only when the picker isn't relying on it (otherwise
            // we'd kill the picker's live device list).
            if let Ok(false) = rx.await {
                svc.stop_discovery().await;
            }
        });
    }
}

// ---- Full markdown report (uploaded diagnostics paste) ----------------------

/// Append a `- **key:** value` markdown bullet (one self-contained line, so it
/// renders correctly without relying on trailing-whitespace hard breaks).
fn md_line(out: &mut String, key: &str, value: &str) {
    out.push_str("- **");
    out.push_str(key);
    out.push_str(":** ");
    out.push_str(value);
    out.push('\n');
}

/// Build a COMPLETE, human-readable markdown diagnostics report — the same data
/// `refresh` gathers, formatted for the uploaded paste. The caller appends logs
/// separately, so this is the non-log body. Runs in an async tokio context, so
/// `tokio::task::spawn_blocking` works without an explicit handle.
pub async fn build_full_report(runtime: &Runtime) -> String {
    // (a) blocking: the three settings stores + /proc + /sys + CPAL sinks.
    let collected = tokio::task::spawn_blocking(|| {
        let audio = qbz_audio::settings::AudioSettingsStore::new()
            .and_then(|s| s.get_settings())
            .unwrap_or_default();
        let (graphics, gfx_failed) =
            match qbz_app::settings::graphics::GraphicsSettingsStore::new()
                .and_then(|s| s.get_settings())
            {
                Ok(g) => (g, false),
                Err(_) => (Default::default(), true),
            };
        let developer = qbz_app::settings::developer::DeveloperSettingsStore::new()
            .and_then(|s| s.get_settings())
            .unwrap_or_default();
        let gfx = qbz_app::diagnostics::detect_graphics_runtime(&graphics, gfx_failed);
        let runtime_diag =
            qbz_app::diagnostics::runtime_diagnostics(&qbz_app::diagnostics::DiagnosticsInputs {
                audio: &audio,
                graphics: &graphics,
                developer: &developer,
                gfx,
                app_version: env!("CARGO_PKG_VERSION"),
            });
        let sys = qbz_app::diagnostics::system_info();
        let (active_output, available_outputs, active_fmt) = collect_output_sinks();
        (runtime_diag, sys, active_output, available_outputs, active_fmt)
    })
    .await;

    let (d, sys, active_output, available_outputs, active_fmt) = match collected {
        Ok(v) => v,
        Err(e) => {
            return format!(
                "# qbz diagnostics\n\n**Version:** {}\n\nFailed to gather diagnostics: {e}\n",
                env!("CARGO_PKG_VERSION")
            );
        }
    };

    // (b) async core snapshot for the Playback section.
    let pb = runtime.core().get_playback_state();
    let track = runtime.core().current_track().await;

    // (c) LIVE QConnect snapshot (no discovery; default when not running).
    let qc = match crate::qconnect_service::service() {
        Some(s) => s.diagnostics_snapshot().await,
        None => Default::default(),
    };

    let mut out = String::new();
    out.push_str("# qbz diagnostics\n\n");
    md_line(&mut out, "Version", env!("CARGO_PKG_VERSION"));
    md_line(
        &mut out,
        "Generated",
        &chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    );

    // ## System
    out.push_str("\n## System\n\n");
    md_line(&mut out, "OS", &sys.os);
    md_line(&mut out, "Arch", &sys.arch);
    md_line(&mut out, "Kernel", &opt(&sys.kernel_version));
    md_line(&mut out, "Distro", &opt(&sys.distro_pretty_name));
    md_line(&mut out, "Distro ID", &opt(&sys.distro_id));
    md_line(&mut out, "Distro Version", &opt(&sys.distro_version_id));
    md_line(&mut out, "Install Method", &sys.install_method);
    if let Some(rt) = &sys.flatpak_runtime {
        md_line(
            &mut out,
            "Flatpak Runtime",
            &format!("{} {}", rt, opt(&sys.flatpak_runtime_version)),
        );
    }
    md_line(&mut out, "WebKit2GTK", &opt(&sys.webkit2gtk_version));
    md_line(&mut out, "GTK", &opt(&sys.gtk_version));
    md_line(&mut out, "glibc", &opt(&sys.glibc_version));
    md_line(&mut out, "ALSA", &opt(&sys.alsa_version));
    md_line(&mut out, "PipeWire", &opt(&sys.pipewire_version));
    md_line(&mut out, "PulseAudio", &opt(&sys.pulseaudio_version));

    // ## Audio
    out.push_str("\n## Audio\n\n");
    let sample_rate = match d.audio_preferred_sample_rate {
        Some(hz) => format!("{hz} Hz"),
        None => "Auto".to_string(),
    };
    let available_str = if available_outputs.is_empty() {
        "—".to_string()
    } else {
        available_outputs.join(", ")
    };
    md_line(&mut out, "Output Device (saved)", &opt(&d.audio_output_device));
    md_line(
        &mut out,
        "Active Output (runtime)",
        active_output.as_deref().unwrap_or("—"),
    );
    if let Some((rate, fmt)) = &active_fmt {
        if !rate.is_empty() {
            md_line(&mut out, "Active Rate (runtime)", rate);
        }
        if !fmt.is_empty() {
            md_line(&mut out, "Active Format (runtime)", fmt);
        }
    }
    md_line(&mut out, "Available Outputs", &available_str);
    md_line(&mut out, "Backend", &opt(&d.audio_backend_type));
    md_line(&mut out, "Exclusive Mode", yn(d.audio_exclusive_mode));
    md_line(&mut out, "DAC Passthrough", yn(d.audio_dac_passthrough));
    md_line(&mut out, "Preferred Sample Rate", &sample_rate);
    md_line(&mut out, "ALSA Plugin", &opt(&d.audio_alsa_plugin));
    md_line(&mut out, "ALSA HW Volume", yn(d.audio_alsa_hardware_volume));
    md_line(&mut out, "Normalization", yn(d.audio_normalization_enabled));
    md_line(
        &mut out,
        "Normalization Target",
        &format!("{} LUFS", d.audio_normalization_target_lufs),
    );
    md_line(&mut out, "Gapless", yn(d.audio_gapless_enabled));
    md_line(
        &mut out,
        "PW Force Bitperfect",
        yn(d.audio_pw_force_bitperfect),
    );
    md_line(
        &mut out,
        "Stream Buffer",
        &format!("{}s", d.audio_stream_buffer_seconds),
    );
    md_line(&mut out, "Streaming Only", yn(d.audio_streaming_only));

    // ## Graphics (saved + runtime)
    out.push_str("\n## Graphics\n\n");
    let compositing = if d.env_webkit_disable_compositing.as_deref() == Some("1") {
        "DISABLED"
    } else {
        "ENABLED"
    };
    let dmabuf = if d.env_webkit_disable_dmabuf.as_deref() == Some("1") {
        "DISABLED"
    } else {
        "ENABLED"
    };
    md_line(
        &mut out,
        "Hardware Acceleration",
        &format!(
            "saved {} / runtime {}",
            yn(d.gfx_hardware_acceleration),
            yn(d.runtime_hw_accel_enabled)
        ),
    );
    md_line(
        &mut out,
        "Force DMA-BUF",
        &format!("saved {} / runtime {}", yn(d.dev_force_dmabuf), dmabuf),
    );
    md_line(
        &mut out,
        "Force X11",
        &format!(
            "saved {} / runtime {}",
            yn(d.gfx_force_x11),
            yn(d.runtime_force_x11_active)
        ),
    );
    md_line(
        &mut out,
        "GSK Renderer",
        &format!(
            "saved {} / runtime {}",
            opt(&d.gfx_gsk_renderer),
            opt(&d.env_gsk_renderer)
        ),
    );
    {
        let (renderer_runtime, renderer_adapters) = crate::renderer_decision_summary();
        md_line(
            &mut out,
            "Renderer (Slint)",
            &format!(
                "saved {} / runtime {}",
                crate::ui_prefs::load().renderer,
                renderer_runtime
            ),
        );
        md_line(&mut out, "GPU Adapters", &renderer_adapters);
        md_line(
            &mut out,
            "UI Loop Latency",
            &format!(
                "{} ms (worst {} ms{})",
                crate::ui_watchdog::last_latency_ms(),
                crate::ui_watchdog::worst_latency_ms(),
                if crate::ui_watchdog::flagged() {
                    ", sustained degradation flagged"
                } else {
                    ""
                }
            ),
        );
    }
    md_line(&mut out, "GDK Scale", &opt(&d.gfx_gdk_scale));
    md_line(&mut out, "GDK DPI Scale", &opt(&d.gfx_gdk_dpi_scale));
    md_line(&mut out, "Compositing Mode", compositing);
    md_line(
        &mut out,
        "GPU",
        if d.runtime_gpu_name.is_empty() {
            "Unknown"
        } else {
            &d.runtime_gpu_name
        },
    );
    md_line(
        &mut out,
        "GPU: NVIDIA",
        if d.runtime_has_nvidia { "Detected" } else { "No" },
    );
    md_line(
        &mut out,
        "GPU: Intel",
        if d.runtime_has_intel { "Detected" } else { "No" },
    );
    md_line(
        &mut out,
        "GPU: AMD",
        if d.runtime_has_amd { "Detected" } else { "No" },
    );
    md_line(
        &mut out,
        "Desktop Environment",
        if d.runtime_desktop_environment.is_empty() {
            "Unknown"
        } else {
            &d.runtime_desktop_environment
        },
    );
    md_line(
        &mut out,
        "Wayland",
        if d.runtime_is_wayland {
            "Yes"
        } else {
            "No (X11)"
        },
    );
    md_line(&mut out, "VM", if d.runtime_is_vm { "Yes" } else { "No" });
    md_line(&mut out, "Using Fallback", yn(d.runtime_using_fallback));

    // ## Environment
    out.push_str("\n## Environment\n\n");
    md_line(
        &mut out,
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        &opt(&d.env_webkit_disable_dmabuf),
    );
    md_line(
        &mut out,
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        &opt(&d.env_webkit_disable_compositing),
    );
    md_line(&mut out, "GDK_BACKEND", &opt(&d.env_gdk_backend));
    md_line(&mut out, "GSK_RENDERER", &opt(&d.env_gsk_renderer));
    md_line(
        &mut out,
        "LIBGL_ALWAYS_SOFTWARE",
        &opt(&d.env_libgl_always_software),
    );
    md_line(&mut out, "WAYLAND_DISPLAY", &opt(&d.env_wayland_display));
    md_line(&mut out, "XDG_SESSION_TYPE", &opt(&d.env_xdg_session_type));

    // ## Playback
    out.push_str("\n## Playback\n\n");
    let volume_percent = (pb.volume * 100.0).round() as i64;
    let title = track.as_ref().map(|t| t.title.clone());
    let artist = track.as_ref().map(|t| t.artist.clone());
    let album = track.as_ref().map(|t| t.album.clone());
    let source = track.as_ref().and_then(|t| t.source.clone());
    let bit_depth = track
        .as_ref()
        .and_then(|t| t.bit_depth)
        .map(|b| format!("{b}-bit"))
        .unwrap_or_else(|| "—".to_string());
    let track_sample_rate = track
        .as_ref()
        .and_then(|t| t.sample_rate)
        .map(|r| format!("{} kHz", trim_khz(r)))
        .unwrap_or_else(|| "—".to_string());
    let is_local = match track.as_ref() {
        Some(t) => yn(t.is_local).to_string(),
        None => "—".to_string(),
    };
    md_line(&mut out, "Playing", yn(pb.is_playing));
    md_line(&mut out, "Volume", &format!("{volume_percent}%"));
    md_line(
        &mut out,
        "Position / Duration",
        &format!("{}s / {}s", pb.position, pb.duration),
    );
    md_line(&mut out, "Has Track", yn(track.is_some()));
    md_line(&mut out, "Track Title", &opt(&title));
    md_line(&mut out, "Track Artist", &opt(&artist));
    md_line(&mut out, "Track Album", &opt(&album));
    md_line(&mut out, "Track Source", &opt(&source));
    md_line(&mut out, "Track Is Local", &is_local);
    md_line(&mut out, "Track Quality", "—");
    md_line(&mut out, "Track Format", "—");
    md_line(&mut out, "Track Bit Depth", &bit_depth);
    md_line(&mut out, "Track Sample Rate", &track_sample_rate);

    // ## Qobuz Connect
    out.push_str("\n## Qobuz Connect\n\n");
    let role = if qc.role.is_empty() { "none" } else { qc.role };
    let last_error = qc
        .last_error
        .as_deref()
        .map(redact_id_like)
        .unwrap_or_else(|| "—".to_string());
    md_line(&mut out, "Running", yn(qc.running));
    md_line(
        &mut out,
        "Transport Connected",
        yn(qc.transport_connected),
    );
    md_line(&mut out, "Has Endpoint", yn(qc.has_endpoint));
    md_line(&mut out, "Role", role);
    md_line(&mut out, "Active Renderer", &opt(&qc.active_name));
    md_line(&mut out, "Renderer Brand", &opt(&qc.active_brand));
    md_line(&mut out, "Renderer Model", &opt(&qc.active_model));
    md_line(&mut out, "Visible Renderers", &qc.renderer_count.to_string());
    md_line(&mut out, "Last Error", &last_error);

    out
}

// ---- Row builders (1:1 with DiagnosticsPanel.svelte) ------------------------

/// One diagnostics row. `status`: 0 info | 1 match | 2 mismatch.
fn row(label: &str, saved: &str, runtime: &str, status: i32) -> DiagRow {
    DiagRow {
        label: label.into(),
        saved: saved.into(),
        runtime: runtime.into(),
        status,
    }
}

/// `ON`/`OFF`, mirroring the Tauri `bool()` helper.
fn yn(value: bool) -> &'static str {
    if value {
        "ON"
    } else {
        "OFF"
    }
}

/// `Some -> value`, `None -> "—"`, mirroring the Tauri `str()` helper.
fn opt(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "—".to_string())
}

/// Match status for the saved-vs-runtime comparison (Audio/Graphics).
fn match_status(saved: &str, runtime: &str) -> i32 {
    if saved == "—" || runtime == "—" {
        0
    } else if saved == runtime {
        1
    } else {
        2
    }
}

/// Format a kHz value without a trailing ".0" (96.0 -> "96", 44.1 -> "44.1").
fn trim_khz(khz: f64) -> String {
    if khz.fract().abs() < f64::EPSILON {
        format!("{}", khz as i64)
    } else {
        format!("{khz:.1}")
    }
}

/// Query the live output sinks (BLOCKING — CPAL enumeration). Must be called
/// inside a `spawn_blocking`. Returns `(active_output, available_outputs)`:
/// `active_output` is the description (fallback name) of the default sink (the
/// ACTIVE output), and `available_outputs` is the description of every sink.
/// On `Err`, both are empty (treated as no sinks).
fn collect_output_sinks() -> (Option<String>, Vec<String>, Option<(String, String)>) {
    let label = |s: &qbz_audio::output_sinks::OutputSinkInfo| -> String {
        if s.description.is_empty() {
            s.name.clone()
        } else {
            s.description.clone()
        }
    };
    let fmt = active_sink_format();
    match qbz_audio::output_sinks::list_output_sinks() {
        Ok(sinks) => {
            let active = sinks.iter().find(|s| s.is_default).map(&label);
            let available = sinks.iter().map(&label).collect();
            (active, available, fmt)
        }
        Err(e) => {
            log::warn!("[qbz-slint] diagnostics: list_output_sinks failed: {e}");
            (None, Vec::new(), fmt)
        }
    }
}

/// Best-effort LIVE sample format of the active (default) output sink, parsed
/// from `pactl list sinks short`. Returns `(rate, format)` like
/// `("44100 Hz", "s32le · 2ch")` — the rate is what the device is ACTUALLY
/// running at right now (vs the saved "Preferred Sample Rate"). Linux/PipeWire/
/// Pulse only (via pactl); `None` if pactl is unavailable or the sink can't be
/// determined. READ-ONLY — never touches the protected audio backend.
fn active_sink_format() -> Option<(String, String)> {
    use std::process::Command;
    let default = Command::new("pactl")
        .arg("get-default-sink")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let out = Command::new("pactl")
        .args(["list", "sinks", "short"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;

    // sample-spec token like "s32le 2ch 44100Hz" -> ("44100 Hz", "s32le · 2ch").
    let parse_spec = |spec: &str| -> (String, String) {
        let (mut rate, mut chans, mut fmt) = (String::new(), String::new(), String::new());
        for tok in spec.split_whitespace() {
            if let Some(hz) = tok.strip_suffix("Hz") {
                rate = format!("{hz} Hz");
            } else if tok.ends_with("ch") {
                chans = tok.to_string();
            } else {
                fmt = tok.to_string();
            }
        }
        let format = match (fmt.is_empty(), chans.is_empty()) {
            (false, false) => format!("{fmt} · {chans}"),
            (false, true) => fmt,
            (true, false) => chans,
            (true, true) => spec.trim().to_string(),
        };
        (rate, format)
    };

    // Prefer the default sink; fall back to the first RUNNING sink.
    let mut running: Option<(String, String)> = None;
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 5 {
            continue;
        }
        let (name, spec, state) = (cols[1], cols[3], cols[4]);
        if let Some(d) = &default {
            if name == d {
                return Some(parse_spec(spec));
            }
        }
        if state.eq_ignore_ascii_case("RUNNING") && running.is_none() {
            running = Some(parse_spec(spec));
        }
    }
    running
}

fn build_audio_rows(
    d: &qbz_app::diagnostics::RuntimeDiagnostics,
    active_output: Option<&str>,
    available_outputs: &[String],
    active_rate: Option<&str>,
    active_fmt: Option<&str>,
) -> Vec<DiagRow> {
    let sample_rate = match d.audio_preferred_sample_rate {
        Some(hz) => format!("{hz} Hz"),
        None => "Auto".to_string(),
    };

    // Output Device: saved id (may be a stale/unplugged DAC) vs the live active
    // output. Match (1) when the active equals OR is contained/suffixed by the
    // saved value; mismatch (2) when an active device exists but differs (so the
    // stale-saved-vs-live discrepancy is visible); info (0) when no live device.
    let saved_output = opt(&d.audio_output_device);
    let output_runtime = active_output.unwrap_or("—");
    let output_status = match active_output {
        Some(active) => {
            if saved_output == active
                || saved_output.contains(active)
                || saved_output.ends_with(active)
            {
                1
            } else {
                2
            }
        }
        None => 0,
    };
    let available_runtime = if available_outputs.is_empty() {
        "—".to_string()
    } else {
        available_outputs.join(", ")
    };

    vec![
        row("Output Device", &saved_output, output_runtime, output_status),
        row("Backend", &opt(&d.audio_backend_type), "—", 0),
        row("Exclusive Mode", yn(d.audio_exclusive_mode), "—", 0),
        row("DAC Passthrough", yn(d.audio_dac_passthrough), "—", 0),
        row("Preferred Sample Rate", &sample_rate, active_rate.unwrap_or("—"), 0),
        row("Active Format", "—", active_fmt.unwrap_or("—"), 0),
        row("ALSA Plugin", &opt(&d.audio_alsa_plugin), "—", 0),
        row("ALSA HW Volume", yn(d.audio_alsa_hardware_volume), "—", 0),
        row("Normalization", yn(d.audio_normalization_enabled), "—", 0),
        row(
            "Normalization Target",
            &format!("{} LUFS", d.audio_normalization_target_lufs),
            "—",
            0,
        ),
        row("Gapless", yn(d.audio_gapless_enabled), "—", 0),
        row("PW Force Bitperfect", yn(d.audio_pw_force_bitperfect), "—", 0),
        row(
            "Stream Buffer",
            &format!("{}s", d.audio_stream_buffer_seconds),
            "—",
            0,
        ),
        row("Streaming Only", yn(d.audio_streaming_only), "—", 0),
        row("Available Outputs", "—", &available_runtime, 0),
    ]
}

fn build_graphics_rows(d: &qbz_app::diagnostics::RuntimeDiagnostics) -> Vec<DiagRow> {
    // Active Slint renderer decision (wgpu / GL / software + why), recorded at
    // startup by select_slint_backend. Saved side = the Settings>Appearance
    // preference; runtime side = what actually got selected.
    let (renderer_runtime, renderer_adapters) = crate::renderer_decision_summary();
    let renderer_saved = crate::ui_prefs::load().renderer;
    let hw_saved = yn(d.gfx_hardware_acceleration);
    let hw_runtime = yn(d.runtime_hw_accel_enabled);
    let x11_saved = yn(d.gfx_force_x11);
    let x11_runtime = yn(d.runtime_force_x11_active);
    let compositing = if d.env_webkit_disable_compositing.as_deref() == Some("1") {
        "DISABLED"
    } else {
        "ENABLED"
    };
    let dmabuf = if d.env_webkit_disable_dmabuf.as_deref() == Some("1") {
        "DISABLED"
    } else {
        "ENABLED"
    };
    let gsk_saved = opt(&d.gfx_gsk_renderer);
    let gsk_runtime = opt(&d.env_gsk_renderer);
    let dmabuf_status = if d.dev_force_dmabuf == (dmabuf == "ENABLED") {
        1
    } else {
        2
    };

    // Event-loop responsiveness probe (#555): dispatch latency of a
    // cross-thread closure — renderer-independent, so a bad number here with
    // a healthy GPU points ABOVE the renderer. status 2 = sustained
    // degradation was flagged this session.
    let ui_latency = {
        let last = crate::ui_watchdog::last_latency_ms();
        let worst = crate::ui_watchdog::worst_latency_ms();
        if last == 0 && worst == 0 {
            "not sampled yet".to_string()
        } else {
            format!("{last} ms (worst {worst} ms)")
        }
    };
    let ui_latency_status = if crate::ui_watchdog::flagged() { 2 } else { 0 };

    vec![
        row("Renderer (Slint)", &renderer_saved, &renderer_runtime, 0),
        row("GPU Adapters", "—", &renderer_adapters, 0),
        row("UI Loop Latency", "—", &ui_latency, ui_latency_status),
        row(
            "Hardware Acceleration",
            hw_saved,
            hw_runtime,
            match_status(hw_saved, hw_runtime),
        ),
        row(
            "Force DMA-BUF",
            yn(d.dev_force_dmabuf),
            dmabuf,
            dmabuf_status,
        ),
        row(
            "Force X11",
            x11_saved,
            x11_runtime,
            match_status(x11_saved, x11_runtime),
        ),
        row(
            "GSK Renderer",
            &gsk_saved,
            &gsk_runtime,
            match_status(&gsk_saved, &gsk_runtime),
        ),
        row("GDK Scale", &opt(&d.gfx_gdk_scale), "—", 0),
        row("GDK DPI Scale", &opt(&d.gfx_gdk_dpi_scale), "—", 0),
        row("Compositing Mode", "—", compositing, 0),
        row(
            "GPU",
            "—",
            if d.runtime_gpu_name.is_empty() {
                "Unknown"
            } else {
                &d.runtime_gpu_name
            },
            0,
        ),
        row(
            "GPU: NVIDIA",
            "—",
            if d.runtime_has_nvidia { "Detected" } else { "No" },
            0,
        ),
        row(
            "GPU: Intel",
            "—",
            if d.runtime_has_intel { "Detected" } else { "No" },
            0,
        ),
        row(
            "GPU: AMD",
            "—",
            if d.runtime_has_amd { "Detected" } else { "No" },
            0,
        ),
        row(
            "Desktop Environment",
            "—",
            if d.runtime_desktop_environment.is_empty() {
                "Unknown"
            } else {
                &d.runtime_desktop_environment
            },
            0,
        ),
        row(
            "Wayland",
            "—",
            if d.runtime_is_wayland {
                "Yes"
            } else {
                "No (X11)"
            },
            0,
        ),
        row("VM", "—", if d.runtime_is_vm { "Yes" } else { "No" }, 0),
        row(
            "Using Fallback",
            "—",
            yn(d.runtime_using_fallback),
            if d.runtime_using_fallback { 2 } else { 0 },
        ),
    ]
}

fn build_env_rows(d: &qbz_app::diagnostics::RuntimeDiagnostics) -> Vec<DiagRow> {
    vec![
        row(
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "—",
            &opt(&d.env_webkit_disable_dmabuf),
            0,
        ),
        row(
            "WEBKIT_DISABLE_COMPOSITING_MODE",
            "—",
            &opt(&d.env_webkit_disable_compositing),
            0,
        ),
        row("GDK_BACKEND", "—", &opt(&d.env_gdk_backend), 0),
        row("GSK_RENDERER", "—", &opt(&d.env_gsk_renderer), 0),
        row(
            "LIBGL_ALWAYS_SOFTWARE",
            "—",
            &opt(&d.env_libgl_always_software),
            0,
        ),
        row("WAYLAND_DISPLAY", "—", &opt(&d.env_wayland_display), 0),
        row("XDG_SESSION_TYPE", "—", &opt(&d.env_xdg_session_type), 0),
    ]
}

fn build_system_rows(s: &qbz_app::diagnostics::SystemInfo) -> Vec<DiagRow> {
    let mut rows = vec![
        row("OS", "—", &s.os, 0),
        row("Arch", "—", &s.arch, 0),
        row("Kernel", "—", &opt(&s.kernel_version), 0),
        row("Distro", "—", &opt(&s.distro_pretty_name), 0),
        row("Distro ID", "—", &opt(&s.distro_id), 0),
        row("Distro Version", "—", &opt(&s.distro_version_id), 0),
        row("Install Method", "—", &s.install_method, 0),
    ];
    if let Some(runtime) = &s.flatpak_runtime {
        rows.push(row(
            "Flatpak Runtime",
            "—",
            &format!("{} {}", runtime, opt(&s.flatpak_runtime_version)),
            0,
        ));
    }
    rows.push(row("WebKit2GTK", "—", &opt(&s.webkit2gtk_version), 0));
    rows.push(row("GTK", "—", &opt(&s.gtk_version), 0));
    rows.push(row("glibc", "—", &opt(&s.glibc_version), 0));
    rows.push(row("ALSA", "—", &opt(&s.alsa_version), 0));
    rows.push(row("PipeWire", "—", &opt(&s.pipewire_version), 0));
    rows.push(row("PulseAudio", "—", &opt(&s.pulseaudio_version), 0));
    rows
}

fn build_playback_rows(
    pb: &qbz_player::PlaybackState,
    track: Option<&qbz_models::QueueTrack>,
) -> Vec<DiagRow> {
    let volume_percent = (pb.volume * 100.0).round() as i64;
    let has_track = track.is_some();
    let title = track.map(|t| t.title.clone());
    let artist = track.map(|t| t.artist.clone());
    let album = track.map(|t| t.album.clone());
    let source = track.and_then(|t| t.source.clone());
    let bit_depth = track
        .and_then(|t| t.bit_depth)
        .map(|d| format!("{d}-bit"))
        .unwrap_or_else(|| "—".to_string());
    let sample_rate = track
        .and_then(|t| t.sample_rate)
        .map(|r| format!("{} kHz", trim_khz(r)))
        .unwrap_or_else(|| "—".to_string());
    let is_local = match track {
        Some(t) => yn(t.is_local).to_string(),
        None => "—".to_string(),
    };

    vec![
        row("Playing", "—", yn(pb.is_playing), 0),
        row("Volume", "—", &format!("{volume_percent}%"), 0),
        row(
            "Position / Duration",
            "—",
            &format!("{}s / {}s", pb.position, pb.duration),
            0,
        ),
        row("Has Track", "—", yn(has_track), 0),
        row("Track Title", "—", &opt(&title), 0),
        row("Track Artist", "—", &opt(&artist), 0),
        row("Track Album", "—", &opt(&album), 0),
        row("Track Source", "—", &opt(&source), 0),
        row("Track Is Local", "—", &is_local, 0),
        // No quality/format field on QueueTrack — emit "—" (faithful to data).
        row("Track Quality", "—", "—", 0),
        row("Track Format", "—", "—", 0),
        row("Track Bit Depth", "—", &bit_depth, 0),
        row("Track Sample Rate", "—", &sample_rate, 0),
    ]
}

fn build_qconnect_rows(q: &crate::qconnect_service::QconnectDiagSnapshot) -> Vec<DiagRow> {
    let role = if q.role.is_empty() { "none" } else { q.role };
    let last_error = q
        .last_error
        .as_deref()
        .map(redact_id_like)
        .unwrap_or_else(|| "—".to_string());
    vec![
        row("Running", "—", yn(q.running), 0),
        row("Transport Connected", "—", yn(q.transport_connected), 0),
        row("Has Endpoint", "—", yn(q.has_endpoint), 0),
        row("Role", "—", role, 0),
        row("Active Renderer", "—", &opt(&q.active_name), 0),
        row("Renderer Brand", "—", &opt(&q.active_brand), 0),
        row("Renderer Model", "—", &opt(&q.active_model), 0),
        row("Visible Renderers", "—", &q.renderer_count.to_string(), 0),
        row("Last Error", "—", &last_error, 0),
    ]
}

// ---- Export JSON (camelCase, matching the Tauri export shape) ---------------

fn build_playback_json(
    pb: &qbz_player::PlaybackState,
    track: Option<&qbz_models::QueueTrack>,
) -> Value {
    json!({
        "isPlaying": pb.is_playing,
        "volumePercent": (pb.volume * 100.0).round() as i64,
        "positionSecs": pb.position,
        "durationSecs": pb.duration,
        "hasTrack": track.is_some(),
        "trackTitle": track.map(|t| t.title.clone()),
        "trackArtist": track.map(|t| t.artist.clone()),
        "trackAlbum": track.map(|t| t.album.clone()),
        "trackQuality": Value::Null,
        "trackFormat": Value::Null,
        "trackBitDepth": track.and_then(|t| t.bit_depth),
        "trackSamplingRate": track.and_then(|t| t.sample_rate),
        "trackIsLocal": track.map(|t| t.is_local),
        "trackSource": track.and_then(|t| t.source.clone()),
    })
}

fn build_qconnect_json(q: &crate::qconnect_service::QconnectDiagSnapshot) -> Value {
    let role = if q.role.is_empty() { "none" } else { q.role };
    json!({
        "running": q.running,
        "transport_connected": q.transport_connected,
        "hasEndpoint": q.has_endpoint,
        "lastError": q.last_error.as_deref().map(redact_id_like),
        "role": role,
        "activeRendererName": q.active_name,
        "activeRendererBrand": q.active_brand,
        "activeRendererModel": q.active_model,
        "rendererCount": q.renderer_count,
    })
}

/// Redact UUID + long-hex substrings (ported from DiagnosticsPanel.svelte's
/// `redactIdLike`, JS regex `/[0-9a-f]{8}-…/` + `/\b[0-9a-f]{32,}\b/`). Keeps
/// pasted diagnostics free of anything a secret scanner might flag. Operates on
/// chars so it is UTF-8 safe.
fn redact_id_like(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len();
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < n {
        // UUID shape (8-4-4-4-12 hex), word-boundary delimited.
        if uuid_at(&chars, i)
            && (i == 0 || !is_word(chars[i - 1]))
            && (i + 36 >= n || !is_word(chars[i + 36]))
        {
            out.push_str("<uuid>");
            i += 36;
            continue;
        }
        // A maximal word token that is entirely hex and >= 32 chars long.
        if chars[i].is_ascii_hexdigit() && (i == 0 || !is_word(chars[i - 1])) {
            let mut j = i;
            while j < n && is_word(chars[j]) {
                j += 1;
            }
            if j - i >= 32 && chars[i..j].iter().all(|c| c.is_ascii_hexdigit()) {
                out.push_str("<hex>");
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Whether a 36-char `8-4-4-4-12` hex UUID starts at `i`.
fn uuid_at(chars: &[char], i: usize) -> bool {
    let groups = [8usize, 4, 4, 4, 12];
    let mut p = i;
    for (gi, &len) in groups.iter().enumerate() {
        for _ in 0..len {
            if p >= chars.len() || !chars[p].is_ascii_hexdigit() {
                return false;
            }
            p += 1;
        }
        if gi < 4 {
            if p >= chars.len() || chars[p] != '-' {
                return false;
            }
            p += 1;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_uuid_and_long_hex() {
        let s = "session 550e8400-e29b-41d4-a716-446655440000 token \
                 0123456789abcdef0123456789abcdef ok";
        let out = redact_id_like(s);
        assert!(out.contains("<uuid>"), "{out}");
        assert!(out.contains("<hex>"), "{out}");
        assert!(out.contains("session"));
        assert!(out.contains("ok"));
    }

    #[test]
    fn leaves_short_hex_alone() {
        // 8 hex chars (a SONAME-ish short id) is below the 32-char threshold.
        assert_eq!(redact_id_like("abc123 deadbeef end"), "abc123 deadbeef end");
    }

    #[test]
    fn match_status_rules() {
        assert_eq!(match_status("ON", "ON"), 1);
        assert_eq!(match_status("ON", "OFF"), 2);
        assert_eq!(match_status("—", "ON"), 0);
        assert_eq!(match_status("ON", "—"), 0);
    }
}
