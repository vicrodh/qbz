//! Settings controller — Audio and Playback preferences.
//!
//! Owns the two persistence stores (`AudioSettingsStore` from `qbz-audio`,
//! `PlaybackPreferencesStore` from `qbz-app`) plus the JSON `ui_prefs`
//! store (Streaming Quality), and bridges them to the `SettingsState`
//! Slint global.
//!
//! Audio changes are persisted and then applied to the live `Player`:
//! routing-critical changes (backend, output device, max sample rate,
//! exclusive mode, DAC passthrough, ALSA plugin) trigger a device
//! re-init; the rest only reload the settings struct. Playback-preference
//! changes (autoplay, show-context, persist, resume) just persist.
//!
//! Neither domain store is exposed by `AppRuntime`, so this module opens
//! them directly at the shared global path — the same path
//! `AppRuntime::new` reads to seed the `Player`, so the two stay
//! consistent.

use std::sync::{Arc, Mutex};

use qbz_app::settings::bundle::{self, ExportOptions, ExportSource};
use qbz_app::settings::playback::{
    AutoplayMode, PlaybackPreferencesState, PlaybackPreferencesStore,
};
use qbz_app::shell::AppRuntime;
use qbz_audio::backend::{AlsaPlugin, AudioBackendType, BackendManager};
use qbz_audio::settings::{AudioSettingsState, AudioSettingsStore};
use qconnect_app::QconnectStartupMode;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::adapter::SlintAdapter;
use crate::ui_prefs::{self, STREAMING_QUALITIES};
use crate::{AppWindow, NowPlayingState, SettingsExportState, SettingsState};

/// Maximum-sample-rate dropdown options. Index 0 is "No limit" (`None`).
/// Backs `device_max_sample_rate`.
// Labels in these tables are `mark`ed so the extractor registers the English
// literals; they are translated once with `t(l)` where the snapshot is built.
const MAX_SAMPLE_RATES: &[(&str, Option<u32>)] = &[
    (qbz_i18n::mark("No limit"), None),
    (qbz_i18n::mark("44.1 kHz"), Some(44_100)),
    (qbz_i18n::mark("48 kHz"), Some(48_000)),
    (qbz_i18n::mark("88.2 kHz"), Some(88_200)),
    (qbz_i18n::mark("96 kHz"), Some(96_000)),
    (qbz_i18n::mark("176.4 kHz"), Some(176_400)),
    (qbz_i18n::mark("192 kHz"), Some(192_000)),
    (qbz_i18n::mark("352.8 kHz"), Some(352_800)),
    (qbz_i18n::mark("384 kHz"), Some(384_000)),
];

/// ALSA-plugin dropdown options.
/// DSD delivery modes (DSD plan Phases 2-3). Value strings are the
/// AudioSettings.dsd_mode contract ("convert" | "dop" | "native").
const DSD_MODES: &[(&str, &str)] = &[
    (qbz_i18n::mark("Convert to PCM (works everywhere)"), "convert"),
    (qbz_i18n::mark("DoP — DSD over PCM (bit-perfect)"), "dop"),
    (qbz_i18n::mark("Native DSD (kernel support required)"), "native"),
];

const ALSA_PLUGINS: &[(&str, AlsaPlugin)] = &[
    (qbz_i18n::mark("hw (Direct Hardware)"), AlsaPlugin::Hw),
    (qbz_i18n::mark("plughw (Auto-convert)"), AlsaPlugin::PlugHw),
    (qbz_i18n::mark("pcm (Most compatible)"), AlsaPlugin::Pcm),
];

/// "When quality retries fail" dropdown options. The value is the
/// `quality_fallback_behavior` DB string.
const RETRY_BEHAVIORS: &[(&str, &str)] = &[
    (qbz_i18n::mark("Ask me"), "ask"),
    (qbz_i18n::mark("Always try lowest quality"), "always_fallback"),
    (qbz_i18n::mark("Always skip track"), "always_skip"),
];

/// "Auto-connect Qobuz Connect on startup" dropdown options. The value is the
/// persisted QConnect startup mode — same DB key + values as the Tauri app
/// (`startup_mode` in `qconnect_settings.db`), persisted via
/// `crate::qconnect_transport`, NOT the audio/playback stores.
const QCONNECT_STARTUP_MODES: &[(&str, QconnectStartupMode)] = &[
    (
        qbz_i18n::mark("Remember state"),
        QconnectStartupMode::RememberLast,
    ),
    (qbz_i18n::mark("On by default"), QconnectStartupMode::On),
    (qbz_i18n::mark("Off by default"), QconnectStartupMode::Off),
];

/// What a persisted audio change requires of the live `Player`.
enum Apply {
    /// Not a player-applied setting — nothing to apply.
    None,
    /// Settings struct refresh only (gapless, fallback, stream-*, ...).
    Reload,
    /// Routing-critical — also re-init the output device.
    Reinit,
}

/// Index -> value maps the dropdown callbacks resolve against. The label
/// lists live in `SettingsState`; these are the parallel value lists.
#[derive(Default)]
struct SettingsMaps {
    backends: Vec<AudioBackendType>,
    /// Device ids, parallel to `SettingsState.devices` labels. An empty
    /// id is the "System default" entry (`output_device = None`).
    devices: Vec<String>,
}

/// Owns the settings stores and the dropdown index maps.
pub struct SettingsCtx {
    audio: AudioSettingsState,
    playback: PlaybackPreferencesState,
    maps: Mutex<SettingsMaps>,
}

impl SettingsCtx {
    /// Open both domain stores at the shared global path. A store that
    /// fails to open degrades to an empty (no-op) handle rather than
    /// aborting.
    pub fn open() -> Arc<Self> {
        let audio = AudioSettingsState::new().unwrap_or_else(|e| {
            log::warn!("[qbz-slint] audio settings store unavailable: {e}");
            AudioSettingsState::new_empty()
        });
        let playback = PlaybackPreferencesState::new().unwrap_or_else(|e| {
            log::warn!("[qbz-slint] playback preferences store unavailable: {e}");
            PlaybackPreferencesState::new_empty()
        });
        Arc::new(Self {
            audio,
            playback,
            maps: Mutex::new(SettingsMaps::default()),
        })
    }

    /// A handle to the playback-preferences store sharing the same
    /// underlying SQLite connection. The Queue controller uses it to read
    /// and toggle the autoplay (infinite-play) mode so the sidebar's
    /// infinite-play button stays in step with the Playback settings.
    pub fn playback_prefs(&self) -> PlaybackPreferencesState {
        PlaybackPreferencesState {
            store: std::sync::Arc::clone(&self.playback.store),
        }
    }
}

/// Plain, `Send` settings data built off the UI thread.
pub struct SettingsSnapshot {
    // Audio — dropdowns.
    streaming_qualities: Vec<String>,
    streaming_quality_index: i32,
    sample_rates: Vec<String>,
    sample_rate_index: i32,
    backends: Vec<String>,
    backend_index: i32,
    devices: Vec<String>,
    device_bp: Vec<bool>,
    device_groups: Vec<String>,
    device_index: i32,
    alsa_plugins: Vec<String>,
    alsa_plugin_index: i32,
    // Audio — toggles.
    limit_quality_to_device: bool,
    alsa_hardware_volume: bool,
    dsd_modes: Vec<String>,
    dsd_mode_index: i32,
    exclusive_mode: bool,
    reserve_dac: bool,
    dac_passthrough: bool,
    pw_force_bitperfect: bool,
    allow_quality_fallback: bool,
    sync_audio_on_startup: bool,
    skip_sink_switch: bool,
    // Audio — conditional flags.
    backend_is_alsa: bool,
    backend_is_pipewire: bool,
    backend_is_jack: bool,
    alsa_plugin_is_hw: bool,
    // Playback.
    continue_playback: bool,
    show_context_icon: bool,
    persist_session: bool,
    resume_position: bool,
    gapless: bool,
    stream_uncached: bool,
    streaming_only: bool,
    normalization: bool,
    buffer_seconds: i32,
    retry_behaviors: Vec<String>,
    retry_behavior_index: i32,
    qconnect_startup_modes: Vec<String>,
    qconnect_startup_index: i32,
    // QConnect device name — persisted custom override ("" = unset) + the
    // effective default used as the input's placeholder.
    qconnect_device_name: String,
    qconnect_device_name_default: String,
    // Now-playing output indicators (backend + effective bit-perfect mode).
    output_backend_label: String,
    output_mode_label: String,
    output_backend_active: bool,
    output_mode_active: bool,
}

/// Compute the two now-playing output leds from the real audio constraints:
/// led 1 = the active backend, led 2 = the effective output mode for THAT
/// backend (bit-perfect / exclusive / routed / shared). `*_active` is true
/// when a deliberate, non-shared mode is engaged (colours the led).
fn output_labels(audio: &qbz_audio::settings::AudioSettings) -> (String, String, bool, bool) {
    let (backend_label, backend_active) = match audio.backend_type {
        Some(AudioBackendType::PipeWire) => ("PIPEWIRE", true),
        Some(AudioBackendType::Alsa) => ("ALSA", true),
        Some(AudioBackendType::Jack) => ("JACK", true),
        Some(AudioBackendType::Pulse) => ("PULS", true),
        Some(AudioBackendType::SystemDefault) => ("SYST", false),
        None => ("AUTO", false),
    };
    let (mode_label, mode_active) = match audio.backend_type {
        Some(AudioBackendType::PipeWire) => {
            if audio.dac_passthrough {
                ("DACPASS", true)
            } else if audio.pw_force_bitperfect {
                ("BITPERF", true)
            } else {
                ("SHARED", false)
            }
        }
        Some(AudioBackendType::Alsa) => match audio.alsa_plugin {
            Some(AlsaPlugin::Hw) => {
                if audio.exclusive_mode {
                    ("EXCL", true)
                } else {
                    ("DIRECT", true)
                }
            }
            _ => ("SHARED", false),
        },
        Some(AudioBackendType::Jack) => {
            if audio.reserve_dac_while_running {
                ("LOCKED", true)
            } else {
                ("ROUTED", false)
            }
        }
        Some(AudioBackendType::Pulse) => ("SHARED", false),
        Some(AudioBackendType::SystemDefault) | None => ("DEFAULT", false),
    };
    (
        backend_label.to_string(),
        mode_label.to_string(),
        backend_active,
        mode_active,
    )
}

/// Devices enumerated for one backend: parallel label / id / bit-perfect
/// / section-header lists. `bp[i]` flags a device able to deliver
/// bit-perfect output; `groups[i]` is the section-header label shown
/// above row `i` (empty = no header — the row continues the previous
/// section). The four lists stay index-aligned with each other and with
/// `SettingsMaps.devices`, so `device_index` keeps resolving correctly
/// after the rows are regrouped.
struct DeviceList {
    labels: Vec<String>,
    ids: Vec<String>,
    bp: Vec<bool>,
    groups: Vec<String>,
}

/// Which ALSA dropdown section a device belongs to. Mirrors the Tauri
/// `DeviceDropdown.svelte` ALSA grouping (`Defaults`, `Bit-perfect
/// (Hardware / Digital)`, `Plugin Hardware`, `Other Outputs`), in that
/// display order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum AlsaSection {
    Defaults,
    BitPerfect,
    PluginHw,
    Other,
}

/// Classify an ALSA device into its dropdown section, matching the Tauri
/// `DeviceDropdown.svelte` ALSA branch:
///  - the "System default" entry (empty id) and `default` / `is_default`
///    devices → Defaults;
///  - `hw:`, `iec958:`, `front:CARD=` ids, or any label containing
///    "bit-perfect" → Bit-perfect (Hardware / Digital);
///  - `plughw:` ids → Plugin Hardware;
///  - everything else (`sysdefault:`, `hdmi:`, ...) → Other Outputs.
fn alsa_section(id: &str, is_default: bool, label: &str) -> AlsaSection {
    let id_l = id.to_ascii_lowercase();
    if id.is_empty() || id_l == "default" || is_default {
        AlsaSection::Defaults
    } else if id_l.starts_with("hw:")
        || id_l.starts_with("iec958:")
        || id_l.starts_with("front:card=")
        || label.to_ascii_lowercase().contains("bit-perfect")
    {
        AlsaSection::BitPerfect
    } else if id_l.starts_with("plughw:") {
        AlsaSection::PluginHw
    } else {
        AlsaSection::Other
    }
}

/// The display label for an ALSA section header.
fn alsa_section_label(section: AlsaSection) -> &'static str {
    // Marked at the definition so the extractor registers the English literals;
    // the single `t(...)` happens at the `group_alsa_devices` call site.
    match section {
        AlsaSection::Defaults => qbz_i18n::mark("Defaults"),
        AlsaSection::BitPerfect => qbz_i18n::mark("Bit-perfect (Hardware / Digital)"),
        AlsaSection::PluginHw => qbz_i18n::mark("Plugin Hardware"),
        AlsaSection::Other => qbz_i18n::mark("Other Outputs"),
    }
}

/// Whether a device can deliver bit-perfect playback on `backend` — the
/// rule that drives the "BP" badge. On ALSA this is exactly the
/// Bit-perfect section of the dropdown (Tauri shows the badge on that
/// group only): direct-hardware `hw:` / `front:CARD=` PCMs and the
/// digital `iec958:` outputs. `sysdefault:`, `hdmi:`, `plughw:` and the
/// system default route through converting plugins / mixers and never
/// qualify. PipeWire reports a hardware flag per node; PulseAudio always
/// mixes, so never capable.
fn device_is_bit_perfect(backend: AudioBackendType, device: &qbz_audio::AudioDevice) -> bool {
    match backend {
        AudioBackendType::Alsa => {
            let label = device.description.as_deref().unwrap_or(&device.name);
            alsa_section(&device.id, device.is_default, label) == AlsaSection::BitPerfect
        }
        AudioBackendType::PipeWire => device.is_hardware,
        // JACK never bit-perfect (resampled to the graph rate); no per-device concept.
        AudioBackendType::Pulse | AudioBackendType::SystemDefault | AudioBackendType::Jack => false,
    }
}

fn backend_label(t: AudioBackendType) -> String {
    match t {
        // Brand/product names stay literal; only the prose entry is translated.
        AudioBackendType::PipeWire => "PipeWire".to_string(),
        AudioBackendType::Alsa => "ALSA".to_string(),
        AudioBackendType::Pulse => "PulseAudio".to_string(),
        AudioBackendType::SystemDefault => qbz_i18n::t("System default"),
        AudioBackendType::Jack => "JACK".to_string(),
    }
}

/// One enumerated output device before grouping.
struct DeviceRow {
    label: String,
    id: String,
    bp: bool,
}

/// Enumerate output devices for a backend. Always leads with a "System
/// default" entry (empty id). Blocking — call off the UI thread.
///
/// For the ALSA backend the rows are regrouped into the Tauri dropdown
/// sections (Defaults / Bit-perfect / Plugin Hardware / Other Outputs)
/// and a parallel `groups` list carries the section header shown above
/// each section's first row. Other backends keep a flat list with no
/// headers (`groups` all empty).
fn enumerate_devices(backend: AudioBackendType) -> DeviceList {
    // The synthetic "System default" entry (empty id) always leads.
    let mut rows = vec![DeviceRow {
        label: qbz_i18n::t("System default"),
        id: String::new(),
        bp: false,
    }];
    match BackendManager::create_backend(backend).and_then(|b| b.enumerate_devices()) {
        Ok(devices) => {
            for d in devices {
                let label = match d.description.as_deref() {
                    Some(desc) if !desc.is_empty() => desc.to_string(),
                    _ => d.name.clone(),
                };
                let bp = device_is_bit_perfect(backend, &d);
                rows.push(DeviceRow {
                    label,
                    id: d.id,
                    bp,
                });
            }
        }
        Err(e) => log::warn!("[qbz-slint] device enumeration failed: {e}"),
    }

    if backend == AudioBackendType::Alsa {
        group_alsa_devices(rows)
    } else {
        // Non-ALSA backends: flat list, no section headers.
        let len = rows.len();
        let mut list = DeviceList {
            labels: Vec::with_capacity(len),
            ids: Vec::with_capacity(len),
            bp: Vec::with_capacity(len),
            groups: vec![String::new(); len],
        };
        for r in rows {
            list.labels.push(r.label);
            list.ids.push(r.id);
            list.bp.push(r.bp);
        }
        list
    }
}

/// Regroup ALSA device rows into the Tauri dropdown sections and build
/// the parallel `groups` header list. A row's `groups` entry is the
/// section label only when it is the first row of its section; the rest
/// are empty. Rows keep their relative order within a section, so the
/// resulting `ids` stay a faithful index map for `device_index`.
fn group_alsa_devices(rows: Vec<DeviceRow>) -> DeviceList {
    // Stable sort by section keeps within-section enumeration order.
    let mut indexed: Vec<(AlsaSection, DeviceRow)> = rows
        .into_iter()
        .map(|r| (alsa_section(&r.id, false, &r.label), r))
        .collect();
    indexed.sort_by_key(|(section, _)| *section);

    let len = indexed.len();
    let mut list = DeviceList {
        labels: Vec::with_capacity(len),
        ids: Vec::with_capacity(len),
        bp: Vec::with_capacity(len),
        groups: Vec::with_capacity(len),
    };
    let mut prev_section: Option<AlsaSection> = None;
    for (section, row) in indexed {
        let header = if prev_section != Some(section) {
            prev_section = Some(section);
            qbz_i18n::t(alsa_section_label(section))
        } else {
            String::new()
        };
        list.labels.push(row.label);
        list.ids.push(row.id);
        list.bp.push(row.bp);
        list.groups.push(header);
    }
    list
}

fn with_audio<T>(
    audio: &AudioSettingsState,
    f: impl FnOnce(&AudioSettingsStore) -> Result<T, String>,
) -> Result<T, String> {
    let guard = audio
        .store
        .lock()
        .map_err(|_| "audio settings lock poisoned".to_string())?;
    let store = guard
        .as_ref()
        .ok_or_else(|| "audio settings store not open".to_string())?;
    f(store)
}

fn with_playback<T>(
    playback: &PlaybackPreferencesState,
    f: impl FnOnce(&PlaybackPreferencesStore) -> Result<T, String>,
) -> Result<T, String> {
    let guard = playback
        .store
        .lock()
        .map_err(|_| "playback preferences lock poisoned".to_string())?;
    let store = guard
        .as_ref()
        .ok_or_else(|| "playback preferences store not open".to_string())?;
    f(store)
}

/// Build a snapshot from already-read settings. Splitting this out lets
/// `load_snapshot` and a post-reset rebuild share the device-enumeration
/// and index-mapping logic.
fn build_snapshot(
    ctx: &SettingsCtx,
    audio: qbz_audio::settings::AudioSettings,
    prefs: qbz_app::settings::playback::PlaybackPreferences,
    streaming_quality_key: &str,
) -> SettingsSnapshot {
    // Keep the session-persistence gates in step with the live playback prefs
    // whenever a settings snapshot is built (startup load + post-reset rebuild).
    crate::session_persist::set_gates(prefs.persist_session, prefs.resume_playback_position);
    let backend_types = BackendManager::available_backends();
    let current_backend = audio.backend_type.unwrap_or_default();
    let backend_index = backend_types
        .iter()
        .position(|t| *t == current_backend)
        .unwrap_or(0);
    let active_backend = backend_types
        .get(backend_index)
        .copied()
        .unwrap_or_default();

    let device_list = enumerate_devices(active_backend);
    let device_index = match &audio.output_device {
        None => 0,
        Some(id) => device_list.ids.iter().position(|d| d == id).unwrap_or(0),
    };

    let sample_rate_index = MAX_SAMPLE_RATES
        .iter()
        .position(|(_, r)| *r == audio.device_max_sample_rate)
        .unwrap_or(0);
    let alsa_plugin = audio.alsa_plugin.unwrap_or(AlsaPlugin::Hw);
    let alsa_plugin_index = ALSA_PLUGINS
        .iter()
        .position(|(_, p)| *p == alsa_plugin)
        .unwrap_or(0);
    let retry_behavior_index = RETRY_BEHAVIORS
        .iter()
        .position(|(_, v)| *v == audio.quality_fallback_behavior)
        .unwrap_or(0);
    // QConnect startup mode — read from the QConnect settings DB (blocking
    // SQLite, fine here: build_snapshot always runs inside spawn_blocking).
    let qconnect_startup_mode = crate::qconnect_transport::load_startup_mode();
    let qconnect_startup_index = QCONNECT_STARTUP_MODES
        .iter()
        .position(|(_, m)| *m == qconnect_startup_mode)
        .unwrap_or(QCONNECT_STARTUP_MODES.len() - 1); // last entry = Off (default)
    // QConnect device name — same DB (and same blocking-SQLite caveat) as the
    // startup mode above. Empty = no custom override; the placeholder shows
    // the name that will actually be announced (env var -> "Qbz - {hostname}").
    let qconnect_device_name =
        crate::qconnect_transport::load_persisted_device_name().unwrap_or_default();
    let qconnect_device_name_default =
        crate::qconnect_transport::resolve_qconnect_friendly_name(None);

    let backend_is_alsa = active_backend == AudioBackendType::Alsa;
    let backend_is_pipewire = active_backend == AudioBackendType::PipeWire;
    let backend_is_jack = active_backend == AudioBackendType::Jack;
    let alsa_plugin_is_hw = alsa_plugin == AlsaPlugin::Hw;
    let (out_backend_label, out_mode_label, out_backend_active, out_mode_active) =
        output_labels(&audio);
    let continue_playback = prefs.autoplay_mode == AutoplayMode::ContinueWithinSource;

    {
        let mut maps = ctx.maps.lock().unwrap_or_else(|e| e.into_inner());
        maps.backends = backend_types.clone();
        maps.devices = device_list.ids.clone();
    }

    SettingsSnapshot {
        streaming_qualities: STREAMING_QUALITIES
            .iter()
            .map(|q| q.label.to_string())
            .collect(),
        streaming_quality_index: ui_prefs::streaming_quality_index(streaming_quality_key) as i32,
        sample_rates: MAX_SAMPLE_RATES.iter().map(|(l, _)| qbz_i18n::t(l)).collect(),
        sample_rate_index: sample_rate_index as i32,
        // Index 0 is "Auto" (a resolve-and-set action, #470); the concrete
        // backends follow. backend_type is always persisted concrete, so the
        // current selection is its position shifted by 1 past the Auto entry —
        // the dropdown never rests on Auto.
        backends: std::iter::once(qbz_i18n::t("Auto"))
            .chain(backend_types.iter().map(|t| backend_label(*t)))
            .collect(),
        backend_index: backend_index as i32 + 1,
        devices: device_list.labels,
        device_bp: device_list.bp,
        device_groups: device_list.groups,
        device_index: device_index as i32,
        alsa_plugins: ALSA_PLUGINS.iter().map(|(l, _)| qbz_i18n::t(l)).collect(),
        alsa_plugin_index: alsa_plugin_index as i32,
        limit_quality_to_device: audio.limit_quality_to_device,
        alsa_hardware_volume: audio.alsa_hardware_volume,
        dsd_modes: DSD_MODES.iter().map(|(l, _)| qbz_i18n::t(l)).collect(),
        dsd_mode_index: DSD_MODES
            .iter()
            .position(|(_, v)| *v == audio.dsd_mode)
            .unwrap_or(0) as i32,
        exclusive_mode: audio.exclusive_mode,
        reserve_dac: audio.reserve_dac_while_running,
        dac_passthrough: audio.dac_passthrough,
        pw_force_bitperfect: audio.pw_force_bitperfect,
        allow_quality_fallback: audio.allow_quality_fallback,
        sync_audio_on_startup: audio.sync_audio_on_startup,
        skip_sink_switch: audio.skip_sink_switch,
        backend_is_alsa,
        backend_is_pipewire,
        backend_is_jack,
        alsa_plugin_is_hw,
        continue_playback,
        show_context_icon: prefs.show_context_icon,
        persist_session: prefs.persist_session,
        resume_position: prefs.resume_playback_position,
        gapless: audio.gapless_enabled,
        stream_uncached: audio.stream_first_track,
        streaming_only: audio.streaming_only,
        normalization: audio.normalization_enabled,
        buffer_seconds: audio.stream_buffer_seconds as i32,
        retry_behaviors: RETRY_BEHAVIORS.iter().map(|(l, _)| qbz_i18n::t(l)).collect(),
        retry_behavior_index: retry_behavior_index as i32,
        qconnect_startup_modes: QCONNECT_STARTUP_MODES
            .iter()
            .map(|(l, _)| qbz_i18n::t(l))
            .collect(),
        qconnect_startup_index: qconnect_startup_index as i32,
        qconnect_device_name,
        qconnect_device_name_default,
        output_backend_label: out_backend_label,
        output_mode_label: out_mode_label,
        output_backend_active: out_backend_active,
        output_mode_active: out_mode_active,
    }
}

/// Read both domain stores, the JSON UI prefs, and enumerate audio
/// devices. Blocking (SQLite + device enumeration) — run inside
/// `spawn_blocking`. Also fills the index maps.
pub fn load_snapshot(ctx: &SettingsCtx) -> SettingsSnapshot {
    let audio = with_audio(&ctx.audio, |s| s.get_settings()).unwrap_or_default();
    let prefs = with_playback(&ctx.playback, |s| s.get_preferences()).unwrap_or_default();
    let ui = ui_prefs::load();
    build_snapshot(ctx, audio, prefs, &ui.streaming_quality)
}

fn string_model(items: Vec<String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    ))
}

fn bool_model(items: Vec<bool>) -> ModelRc<bool> {
    ModelRc::new(VecModel::from(items))
}

/// Push a snapshot onto the `SettingsState` global. Runs on the UI thread.
pub fn apply_snapshot(window: &AppWindow, snap: SettingsSnapshot) {
    let st = window.global::<SettingsState>();
    // Audio — dropdowns.
    st.set_streaming_qualities(string_model(snap.streaming_qualities));
    st.set_streaming_quality_index(snap.streaming_quality_index);
    st.set_sample_rates(string_model(snap.sample_rates));
    st.set_sample_rate_index(snap.sample_rate_index);
    st.set_backends(string_model(snap.backends));
    st.set_backend_index(snap.backend_index);
    st.set_devices(string_model(snap.devices));
    st.set_device_bp(bool_model(snap.device_bp));
    st.set_device_groups(string_model(snap.device_groups));
    st.set_device_index(snap.device_index);
    st.set_alsa_plugins(string_model(snap.alsa_plugins));
    st.set_alsa_plugin_index(snap.alsa_plugin_index);
    // Audio — toggles.
    st.set_limit_quality_to_device(snap.limit_quality_to_device);
    st.set_alsa_hardware_volume(snap.alsa_hardware_volume);
    st.set_dsd_modes(string_model(snap.dsd_modes));
    st.set_dsd_mode_index(snap.dsd_mode_index);
    st.set_exclusive_mode(snap.exclusive_mode);
    st.set_reserve_dac(snap.reserve_dac);
    st.set_dac_passthrough(snap.dac_passthrough);
    st.set_pw_force_bitperfect(snap.pw_force_bitperfect);
    st.set_allow_quality_fallback(snap.allow_quality_fallback);
    st.set_sync_audio_on_startup(snap.sync_audio_on_startup);
    st.set_skip_sink_switch(snap.skip_sink_switch);
    // Audio — conditional flags.
    st.set_backend_is_alsa(snap.backend_is_alsa);
    st.set_backend_is_pipewire(snap.backend_is_pipewire);
    st.set_backend_is_jack(snap.backend_is_jack);
    st.set_alsa_plugin_is_hw(snap.alsa_plugin_is_hw);
    // Playback.
    st.set_continue_playback(snap.continue_playback);
    st.set_show_context_icon(snap.show_context_icon);
    st.set_persist_session(snap.persist_session);
    st.set_resume_position(snap.resume_position);
    st.set_gapless(snap.gapless);
    st.set_stream_uncached(snap.stream_uncached);
    st.set_streaming_only(snap.streaming_only);
    st.set_normalization(snap.normalization);
    // Mirror the four output LEDs onto NowPlayingState too, so the Mode C
    // "Small" now-playing bar has a single source for the song card + the
    // DAC/EXC cluster. Cloned because the SettingsState setters below consume
    // the snapshot's String fields via `.into()`.
    let np = window.global::<NowPlayingState>();
    np.set_output_backend_label(snap.output_backend_label.clone().into());
    np.set_output_mode_label(snap.output_mode_label.clone().into());
    np.set_output_backend_active(snap.output_backend_active);
    np.set_output_mode_active(snap.output_mode_active);
    st.set_output_backend_label(snap.output_backend_label.into());
    st.set_output_mode_label(snap.output_mode_label.into());
    st.set_output_backend_active(snap.output_backend_active);
    st.set_output_mode_active(snap.output_mode_active);
    st.set_buffer_seconds(snap.buffer_seconds);
    st.set_retry_behaviors(string_model(snap.retry_behaviors));
    st.set_retry_behavior_index(snap.retry_behavior_index);
    st.set_qconnect_startup_modes(string_model(snap.qconnect_startup_modes));
    st.set_qconnect_startup_index(snap.qconnect_startup_index);
    st.set_qconnect_device_name(snap.qconnect_device_name.into());
    st.set_qconnect_device_name_default(snap.qconnect_device_name_default.into());
    st.set_loading(false);
}

/// Re-read the persisted audio settings and apply them to the live player.
fn apply_audio(ctx: &SettingsCtx, runtime: &AppRuntime<SlintAdapter>, apply: Apply) {
    let reinit = match apply {
        Apply::None => return,
        Apply::Reload => false,
        Apply::Reinit => true,
    };
    let fresh = match with_audio(&ctx.audio, |s| s.get_settings()) {
        Ok(s) => s,
        Err(e) => {
            log::error!("[qbz-slint] re-read audio settings failed: {e}");
            return;
        }
    };
    let player = runtime.core().player();
    if let Err(e) = player.reload_settings(fresh.clone()) {
        log::error!("[qbz-slint] player.reload_settings failed: {e}");
    }
    if reinit {
        if let Err(e) = player.reinit_device(fresh.output_device.clone()) {
            log::error!("[qbz-slint] player.reinit_device failed: {e}");
        }
    }
    log::info!("[qbz-slint] audio settings applied to player (reinit={reinit})");
}

/// Force the local player volume to 100% in bit-perfect, mirroring Tauri's
/// `playerSetVolume(100)`. Bit-perfect (ALSA backend + `hw` plugin) requires the
/// software volume out of the path, so the player runs at unity gain and the
/// hardware/DAC controls level. Gated on NOT controlling a peer — while a peer
/// renderer owns playback the local lock is lifted and the user adjusts the
/// remote renderer, so forcing local 100 there would be wrong.
///
/// `core().set_volume` is the safe seam (it does NOT touch the protected
/// device-init). Pushes `NowPlayingState.volume = 1.0` so the bar reflects it.
async fn maybe_force_bitperfect_volume(
    ctx: &SettingsCtx,
    runtime: &AppRuntime<SlintAdapter>,
    weak: &slint::Weak<AppWindow>,
) {
    let audio = match with_audio(&ctx.audio, |s| s.get_settings()) {
        Ok(s) => s,
        Err(e) => {
            log::error!("[qbz-slint] re-read audio for force-100 failed: {e}");
            return;
        }
    };
    let is_alsa_direct_hw = audio.backend_type.unwrap_or_default() == AudioBackendType::Alsa
        && audio.alsa_plugin.unwrap_or(AlsaPlugin::Hw) == AlsaPlugin::Hw;
    if !is_alsa_direct_hw {
        return;
    }
    // Skip while controlling a peer — the bit-perfect lock is lifted then.
    let controlling_peer = match crate::qconnect_service::service() {
        Some(svc) => svc.is_peer_active().await,
        None => false,
    };
    if controlling_peer {
        return;
    }
    if let Err(e) = runtime.core().set_volume(1.0) {
        log::error!("[qbz-slint] force bit-perfect volume to 100 failed: {e}");
        return;
    }
    log::info!("[qbz-slint] bit-perfect: forced local volume to 100%");
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<NowPlayingState>().set_volume(1.0);
    });
}

/// Public entry for the startup audio-settings load: apply the bit-perfect
/// force-100 once the player is seeded, so the bar reflects unity gain before
/// the user ever opens Settings. No-op unless ALSA-direct-hw and not controlling.
pub async fn apply_startup_bitperfect_volume(
    ctx: &SettingsCtx,
    runtime: &AppRuntime<SlintAdapter>,
    weak: &slint::Weak<AppWindow>,
) {
    maybe_force_bitperfect_volume(ctx, runtime, weak).await;
}

/// Recompute the backend/ALSA conditional flags from the current audio
/// settings and push them onto `SettingsState`. Called after a backend or
/// ALSA-plugin change so the `.slint` panels re-gate the conditional rows.
fn push_conditional_flags(ctx: &SettingsCtx, weak: &slint::Weak<AppWindow>) {
    let audio = match with_audio(&ctx.audio, |s| s.get_settings()) {
        Ok(s) => s,
        Err(e) => {
            log::error!("[qbz-slint] re-read audio settings for flags failed: {e}");
            return;
        }
    };
    let backend = audio.backend_type.unwrap_or_default();
    let plugin = audio.alsa_plugin.unwrap_or(AlsaPlugin::Hw);
    let is_alsa = backend == AudioBackendType::Alsa;
    let is_pipewire = backend == AudioBackendType::PipeWire;
    let is_jack = backend == AudioBackendType::Jack;
    let plugin_is_hw = plugin == AlsaPlugin::Hw;
    let plugin_index = ALSA_PLUGINS
        .iter()
        .position(|(_, p)| *p == plugin)
        .unwrap_or(0) as i32;
    let _ = weak.upgrade_in_event_loop(move |w| {
        let st = w.global::<SettingsState>();
        st.set_backend_is_alsa(is_alsa);
        st.set_backend_is_pipewire(is_pipewire);
        st.set_backend_is_jack(is_jack);
        st.set_alsa_plugin_is_hw(plugin_is_hw);
        st.set_alsa_plugin_index(plugin_index);
    });
}

/// Rebuild the full snapshot off the UI thread and push it onto
/// `SettingsState`. Used after a cross-setting cascade so the UI reflects
/// every forced change (and the conditional flags) in one shot.
async fn rebuild_and_push(ctx: Arc<SettingsCtx>, weak: slint::Weak<AppWindow>) {
    let snap = match tokio::task::spawn_blocking(move || load_snapshot(&ctx)).await {
        Ok(s) => s,
        Err(e) => {
            log::error!("[qbz-slint] settings cascade rebuild task failed: {e}");
            return;
        }
    };
    let _ = weak.upgrade_in_event_loop(move |w| {
        apply_snapshot(&w, snap);
    });
}

/// Handle a toggle change: persist it, apply any cross-setting cascade,
/// then apply audio settings to the player.
///
/// Cross-setting cascades (matching the Tauri app):
///  - DAC passthrough ON  → force `skip_sink_switch` off (mutually exclusive).
///  - DAC passthrough OFF → force `pw_force_bitperfect` off.
///  - Streaming-only  ON  → force `gapless_enabled` off.
///
/// When a cascade fires, the forced changes are persisted too and the
/// whole snapshot is rebuilt and re-pushed to `SettingsState` so the UI
/// (toggle states, conditional rows, disabled states) stays consistent.
pub async fn handle_bool(
    ctx: Arc<SettingsCtx>,
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    key: String,
    value: bool,
) {
    let key = key.as_str();
    // The offline-MODE toggle persists through the shared engine's per-user
    // store, not the audio/playback stores — routed apart from the Apply
    // machinery below.
    if key == "offline-mode-enabled" {
        set_offline_mode(ctx, runtime, weak, value).await;
        return;
    }
    // Cross-setting cascades — force dependent settings off and persist
    // those forced changes. `cascaded` flags whether a full snapshot
    // re-push is needed afterwards.
    let mut cascaded = false;
    match key {
        "dac-passthrough" if value => {
            if let Err(e) = with_audio(&ctx.audio, |s| s.set_skip_sink_switch(false)) {
                log::error!("[qbz-slint] cascade skip-sink-switch off failed: {e}");
            } else {
                cascaded = true;
            }
        }
        "dac-passthrough" => {
            if let Err(e) = with_audio(&ctx.audio, |s| s.set_pw_force_bitperfect(false)) {
                log::error!("[qbz-slint] cascade pw-force-bitperfect off failed: {e}");
            } else {
                cascaded = true;
            }
        }
        "streaming-only" if value => {
            if let Err(e) = with_audio(&ctx.audio, |s| s.set_gapless_enabled(false)) {
                log::error!("[qbz-slint] cascade gapless off failed: {e}");
            } else {
                cascaded = true;
            }
        }
        _ => {}
    }

    let outcome: Result<Apply, String> = match key {
        // --- Audio toggles -------------------------------------------------
        "limit-quality-to-device" => {
            with_audio(&ctx.audio, |s| s.set_limit_quality_to_device(value))
                .map(|_| Apply::Reload)
        }
        "alsa-hardware-volume" => {
            with_audio(&ctx.audio, |s| s.set_alsa_hardware_volume(value)).map(|_| Apply::Reinit)
        }
        "exclusive-mode" => {
            with_audio(&ctx.audio, |s| s.set_exclusive_mode(value)).map(|_| Apply::Reinit)
        }
        "reserve-dac" => {
            with_audio(&ctx.audio, |s| s.set_reserve_dac_while_running(value))
                .map(|_| Apply::Reload)
        }
        "dac-passthrough" => {
            with_audio(&ctx.audio, |s| s.set_dac_passthrough(value)).map(|_| Apply::Reinit)
        }
        "pw-force-bitperfect" => {
            with_audio(&ctx.audio, |s| s.set_pw_force_bitperfect(value)).map(|_| Apply::Reload)
        }
        "allow-quality-fallback" => {
            with_audio(&ctx.audio, |s| s.set_allow_quality_fallback(value))
                .map(|_| Apply::Reload)
        }
        "sync-audio-on-startup" => {
            with_audio(&ctx.audio, |s| s.set_sync_audio_on_startup(value)).map(|_| Apply::Reload)
        }
        "skip-sink-switch" => {
            with_audio(&ctx.audio, |s| s.set_skip_sink_switch(value)).map(|_| Apply::Reinit)
        }
        // --- Playback toggles backed by AudioSettings ----------------------
        "gapless" => {
            with_audio(&ctx.audio, |s| s.set_gapless_enabled(value)).map(|_| Apply::Reload)
        }
        "normalization" => {
            // Loudness leveling; the shared player applies/bypasses it (and
            // skips it entirely under bit-perfect). Reload (not Reinit) — the
            // audio thread re-reads the settings struct, no device re-init.
            with_audio(&ctx.audio, |s| s.set_normalization_enabled(value)).map(|_| Apply::Reload)
        }
        "stream-uncached" => {
            with_audio(&ctx.audio, |s| s.set_stream_first_track(value)).map(|_| Apply::Reload)
        }
        "streaming-only" => {
            with_audio(&ctx.audio, |s| s.set_streaming_only(value)).map(|_| Apply::Reload)
        }
        // --- Playback toggles backed by PlaybackPreferences ----------------
        "continue-playback" => {
            // On = ContinueWithinSource, off = PlayTrackOnly.
            let mode = if value {
                AutoplayMode::ContinueWithinSource
            } else {
                AutoplayMode::PlayTrackOnly
            };
            with_playback(&ctx.playback, |s| s.set_autoplay_mode(mode)).map(|_| Apply::None)
        }
        "show-context-icon" => {
            with_playback(&ctx.playback, |s| s.set_show_context_icon(value)).map(|_| Apply::None)
        }
        "show-recommendations" => {
            crate::discover_prefs::set_show_recommendations(value);
            Ok(Apply::None)
        }
        "musicbrainz" => {
            // Opt-out toggle (default ON). Persist to ui_prefs (Option B,
            // mirrors system_notifications) and drive the core client's
            // in-memory enabled flag so the artist Network/Scene sidebar and
            // playlist Suggested-Songs gate immediately.
            let mut prefs = crate::ui_prefs::load();
            prefs.musicbrainz_enabled = value;
            crate::ui_prefs::save(&prefs);
            runtime.core().musicbrainz_set_enabled(value).await;
            Ok(Apply::None)
        }
        "persist-session" => {
            let r = with_playback(&ctx.playback, |s| s.set_persist_session(value))
                .map(|_| Apply::None);
            if let Ok(p) = with_playback(&ctx.playback, |s| s.get_preferences()) {
                crate::session_persist::set_gates(p.persist_session, p.resume_playback_position);
            }
            r
        }
        "resume-position" => {
            let r = with_playback(&ctx.playback, |s| s.set_resume_playback_position(value))
                .map(|_| Apply::None);
            if let Ok(p) = with_playback(&ctx.playback, |s| s.get_preferences()) {
                crate::session_persist::set_gates(p.persist_session, p.resume_playback_position);
            }
            r
        }
        other => {
            log::warn!("[qbz-slint] unknown settings bool key: {other}");
            return;
        }
    };
    match outcome {
        Ok(apply) => {
            // A cascade forced extra changes — always re-init the device
            // (cascade targets are routing-critical) regardless of what the
            // triggering toggle alone required.
            let apply = if cascaded { Apply::Reinit } else { apply };
            apply_audio(&ctx, &runtime, apply);
            // Reflect the persisted value back onto SettingsState so toggles
            // that are purely driven by `checked: SettingsState.x` (e.g. the
            // now-playing bar audio-menu QbzToggles for Normalization/Gapless)
            // actually flip. The Settings panel's own toggles already reflect
            // their click optimistically, but the bar flyout's do NOT self-flip
            // — they need this push. Skipped when cascaded, since the full
            // snapshot re-push below already carries the new value.
            if !cascaded {
                let key = key.to_string();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let st = w.global::<SettingsState>();
                    match key.as_str() {
                        "normalization" => st.set_normalization(value),
                        "gapless" => st.set_gapless(value),
                        _ => {}
                    }
                });
            }
        }
        Err(e) => log::error!("[qbz-slint] failed to persist '{key}': {e}"),
    }
    // After a cascade, rebuild + re-push the full snapshot so the forced
    // changes and disabled states reach the UI.
    if cascaded {
        rebuild_and_push(ctx, weak).await;
    }
}

/// Settings > Offline — the Enable Offline Mode toggle (induced offline).
///
/// The shared engine persists the flag, handles the #279 stream-first
/// snapshot/restore against the real audio store, and broadcasts the
/// status change (the offline_mode UI forwarder updates `OfflineState`).
/// Exit is ALWAYS allowed — no confirm dialog, no network-probe gate
/// (deliberate improvement over Tauri's exit trap, spec §3.1).
async fn set_offline_mode(
    ctx: Arc<SettingsCtx>,
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    value: bool,
) {
    let engine = crate::offline_mode::engine();
    // Best-effort #279: pass the real audio store when it's open (the
    // outer Ok-wrap keeps store-unavailability distinct from engine
    // failures); flip the mode without the snapshot when it isn't.
    let result = match with_audio(&ctx.audio, |store| {
        Ok(engine.set_induced(value, Some(store)))
    }) {
        Ok(inner) => inner,
        Err(e) => {
            log::warn!(
                "[qbz-slint] offline toggle: audio store unavailable for the #279 snapshot ({e}); flipping without it"
            );
            engine.set_induced(value, None)
        }
    };
    match result {
        Ok(status) => {
            log::info!(
                "[qbz-slint] offline mode toggled: induced={value} (mode={:?})",
                status.mode
            );
            // The #279 snapshot may have mutated stream_first_track behind
            // the Playback panel's back: reload the live player settings and
            // re-push the full snapshot so the "Stream uncached tracks"
            // toggle stays honest (Tauri's audio-settings-changed re-sync).
            apply_audio(&ctx, &runtime, Apply::Reload);
            rebuild_and_push(ctx, weak).await;
        }
        Err(e) => {
            log::error!("[qbz-slint] offline mode toggle failed: {e}");
            // Revert the optimistic toggle to the engine's actual state.
            let actual = engine
                .settings()
                .map(|s| s.manual_offline_mode)
                .unwrap_or_else(|_| {
                    engine.status().mode == qbz_app::offline_mode::OfflineMode::InducedOffline
                });
            let _ = weak.upgrade_in_event_loop(move |w| {
                w.global::<SettingsState>().set_offline_mode_enabled(actual);
            });
        }
    }
}

/// Handle a slider change: persist it and reload the player settings.
/// Currently only the Initial Buffer Size slider exists.
pub fn handle_slider(
    ctx: &SettingsCtx,
    runtime: &AppRuntime<SlintAdapter>,
    key: &str,
    value: i32,
) {
    match key {
        "buffer-seconds" => {
            let seconds = value.clamp(1, 10) as u8;
            match with_audio(&ctx.audio, |s| s.set_stream_buffer_seconds(seconds)) {
                Ok(()) => apply_audio(ctx, runtime, Apply::Reload),
                Err(e) => log::error!("[qbz-slint] persist buffer seconds failed: {e}"),
            }
        }
        other => log::warn!("[qbz-slint] unknown settings slider key: {other}"),
    }
}

/// Handle a text-input commit (Enter or focus loss). Currently only the
/// QConnect device name — mirrors the Tauri `v2_qconnect_set_device_name`:
/// trim; empty clears the override so the announced name falls back to the
/// default ("Qbz - {hostname}"). Persisted in the QConnect settings DB and
/// pushed into the live service's cache; the name is only announced during
/// `connect()`, so a rename takes effect on the next connection.
pub async fn handle_string(weak: slint::Weak<AppWindow>, key: String, value: String) {
    match key.as_str() {
        "qconnect-device-name" => {
            let trimmed = value.trim().to_string();
            let stored = (!trimmed.is_empty()).then(|| trimmed.clone());
            // Persist (blocking SQLite) off the async runtime.
            let to_persist = stored.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || {
                crate::qconnect_transport::persist_device_name(to_persist.as_deref())
            })
            .await
            {
                log::error!("[qbz-slint] persist qconnect device name failed: {e}");
            }
            // Update the live service cache so the next connect announces the
            // new name without an app restart (it loads the DB only once, at
            // construction).
            if let Some(svc) = crate::qconnect_service::service() {
                svc.set_custom_device_name(stored).await;
            }
            // Push the trimmed value back so a whitespace-only entry visibly
            // resets the input to the placeholder/default state.
            let _ = weak.upgrade_in_event_loop(move |w| {
                w.global::<SettingsState>()
                    .set_qconnect_device_name(trimmed.into());
            });
        }
        other => log::warn!("[qbz-slint] unknown settings string key: {other}"),
    }
}

/// Handle a dropdown change: persist it, apply audio ones to the player,
/// and — for a backend switch — re-enumerate devices and recompute the
/// conditional flags into `SettingsState`.
pub async fn handle_select(
    ctx: Arc<SettingsCtx>,
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    key: String,
    index: usize,
) {
    match key.as_str() {
        "streaming-quality" => {
            // UI-only preference, persisted to ui_prefs.json.
            let Some(quality) = STREAMING_QUALITIES.get(index) else {
                return;
            };
            let mut prefs = ui_prefs::load();
            if prefs.streaming_quality != quality.key {
                prefs.streaming_quality = quality.key.to_string();
                ui_prefs::save(&prefs);
                // The L1/L2 audio cache is keyed by track id alone (no quality
                // dimension), so bytes fetched at the old tier would keep
                // serving plays and casts until they aged out. Clear it so the
                // new tier applies from the next fetch — fire-and-forget even
                // mid-playback: an armed gapless handoff may drop (one possible
                // audible gap on this explicit, rare action), which beats
                // intermittently serving the old tier. Logged so a reported
                // gap is attributable.
                log::info!(
                    "[qbz-slint] streaming quality changed -> clearing audio cache (L1+L2)"
                );
                runtime.core().player().clear_audio_cache();
            }
        }
        "sample-rate" => {
            let rate = MAX_SAMPLE_RATES.get(index).map(|(_, r)| *r).unwrap_or(None);
            if let Err(e) = with_audio(&ctx.audio, |s| s.set_device_max_sample_rate(rate)) {
                log::error!("[qbz-slint] persist max sample rate failed: {e}");
                return;
            }
            apply_audio(&ctx, &runtime, Apply::Reinit);
        }
        "backend" => {
            // Dropdown index 0 is "Auto" — a resolve-and-set action (#470), not a
            // persisted mode. Pick the best available backend (PipeWire if present,
            // else System), persist it concrete, and let the rebuilt snapshot move
            // the dropdown onto that backend; backend_type is never left null/Auto.
            // Indices >= 1 map to the concrete `maps.backends` list (no Auto entry).
            let backend = if index == 0 {
                let types = ctx
                    .maps
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .backends
                    .clone();
                if types.iter().any(|t| *t == AudioBackendType::PipeWire) {
                    AudioBackendType::PipeWire
                } else {
                    AudioBackendType::SystemDefault
                }
            } else {
                let resolved = ctx
                    .maps
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .backends
                    .get(index - 1)
                    .copied();
                let Some(resolved) = resolved else {
                    return;
                };
                resolved
            };
            if let Err(e) = with_audio(&ctx.audio, |s| s.set_backend_type(Some(backend))) {
                log::error!("[qbz-slint] persist backend failed: {e}");
                return;
            }
            // Cross-setting cascades — force settings unsupported by the
            // new backend off, matching the Tauri app.
            if backend != AudioBackendType::PipeWire {
                if let Err(e) = with_audio(&ctx.audio, |s| s.set_dac_passthrough(false)) {
                    log::error!("[qbz-slint] cascade dac-passthrough off failed: {e}");
                }
                if let Err(e) = with_audio(&ctx.audio, |s| s.set_pw_force_bitperfect(false)) {
                    log::error!("[qbz-slint] cascade pw-force-bitperfect off failed: {e}");
                }
            }
            if backend != AudioBackendType::Alsa {
                if let Err(e) = with_audio(&ctx.audio, |s| s.set_exclusive_mode(false)) {
                    log::error!("[qbz-slint] cascade exclusive-mode off failed: {e}");
                }
            }
            if backend == AudioBackendType::Alsa {
                if let Err(e) = with_audio(&ctx.audio, |s| s.set_gapless_enabled(false)) {
                    log::error!("[qbz-slint] cascade gapless off failed: {e}");
                }
            }
            // A backend switch invalidates the device list; reset routing
            // to the system default.
            if let Err(e) = with_audio(&ctx.audio, |s| s.set_output_device(None)) {
                log::error!("[qbz-slint] reset output device failed: {e}");
            }
            // Apply to the player first, then rebuild + re-push the full
            // snapshot. `load_snapshot` re-enumerates the new backend's
            // devices and refills the index maps, so the new device list,
            // the reset device index, the forced cascade changes
            // (dac-passthrough / pw-force-bitperfect / exclusive-mode /
            // gapless) and the conditional flags all reach the UI in one
            // consistent push.
            apply_audio(&ctx, &runtime, Apply::Reinit);
            // Bit-perfect (ALSA + hw) forces local volume to 100%; lifted while
            // controlling a peer. Mirrors Tauri's playerSetVolume(100).
            maybe_force_bitperfect_volume(&ctx, &runtime, &weak).await;
            rebuild_and_push(ctx, weak).await;
        }
        "device" => {
            let id = ctx
                .maps
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .devices
                .get(index)
                .cloned();
            let Some(id) = id else {
                return;
            };
            let device_opt = if id.is_empty() { None } else { Some(id.as_str()) };
            if let Err(e) = with_audio(&ctx.audio, |s| s.set_output_device(device_opt)) {
                log::error!("[qbz-slint] persist device failed: {e}");
                return;
            }
            apply_audio(&ctx, &runtime, Apply::Reinit);
        }
        "dsd-mode" => {
            let Some((_, mode)) = DSD_MODES.get(index) else {
                return;
            };
            if let Err(e) = with_audio(&ctx.audio, |s| s.set_dsd_mode(mode)) {
                log::error!("[qbz-slint] persist DSD mode failed: {e}");
                return;
            }
            apply_audio(&ctx, &runtime, Apply::Reinit);
        }
        "alsa-plugin" => {
            let plugin = ALSA_PLUGINS.get(index).map(|(_, p)| *p);
            let Some(plugin) = plugin else {
                return;
            };
            if let Err(e) = with_audio(&ctx.audio, |s| s.set_alsa_plugin(Some(plugin))) {
                log::error!("[qbz-slint] persist ALSA plugin failed: {e}");
                return;
            }
            // ALSA plugin gates the Hardware Volume Control row.
            push_conditional_flags(&ctx, &weak);
            apply_audio(&ctx, &runtime, Apply::Reinit);
            // Switching to/from the `hw` plugin changes bit-perfect status;
            // re-apply the force-100 (no-op when not ALSA-direct-hw).
            maybe_force_bitperfect_volume(&ctx, &runtime, &weak).await;
        }
        "retry-behavior" => {
            let behavior = RETRY_BEHAVIORS.get(index).map(|(_, v)| *v).unwrap_or("ask");
            if let Err(e) = with_audio(&ctx.audio, |s| s.set_quality_fallback_behavior(behavior)) {
                log::error!("[qbz-slint] persist retry behavior failed: {e}");
                return;
            }
            apply_audio(&ctx, &runtime, Apply::Reload);
        }
        "qconnect-startup" => {
            // Persisted in the QConnect settings DB (same key/values as the
            // Tauri app) — nothing to apply to the live player: the mode is
            // only consulted at startup (and by the toggle's write-through).
            let Some((_, mode)) = QCONNECT_STARTUP_MODES.get(index) else {
                return;
            };
            let mode = *mode;
            if let Err(e) =
                tokio::task::spawn_blocking(move || crate::qconnect_transport::save_startup_mode(mode))
                    .await
            {
                log::error!("[qbz-slint] persist qconnect startup mode failed: {e}");
            }
        }
        other => log::warn!("[qbz-slint] unknown settings select key: {other}"),
    }
}

/// Reset all Audio + Playback settings to defaults, rebuild the snapshot,
/// push it onto `SettingsState`, and re-apply the audio settings to the
/// player. Streaming Quality (a UI-only pref) is intentionally left
/// untouched — it is not part of either domain store.
pub async fn handle_reset(
    ctx: Arc<SettingsCtx>,
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
) {
    if let Err(e) = with_audio(&ctx.audio, |s| s.reset_all()) {
        log::error!("[qbz-slint] audio reset_all failed: {e}");
    }
    if let Err(e) = with_playback(&ctx.playback, |s| s.reset_all()) {
        log::error!("[qbz-slint] playback reset_all failed: {e}");
    }
    // Rebuild the snapshot off the UI thread (device enumeration blocks).
    let snap = {
        let ctx = ctx.clone();
        match tokio::task::spawn_blocking(move || load_snapshot(&ctx)).await {
            Ok(s) => s,
            Err(e) => {
                log::error!("[qbz-slint] settings reset rebuild task failed: {e}");
                return;
            }
        }
    };
    let _ = weak.upgrade_in_event_loop(move |w| {
        apply_snapshot(&w, snap);
    });
    // Routing-critical defaults changed — re-init the device.
    apply_audio(&ctx, &runtime, Apply::Reinit);
}

/// Release the held output device, then re-enumerate. Frees a device QBZ is
/// holding exclusively (ALSA Direct, which leaves the DAC invisible to
/// PipeWire/other apps) and rebuilds the snapshot so a freed or hot-plugged
/// DAC shows up in the list without restarting the app — the Tauri
/// "refresh" affordance plus an explicit release, in one action.
pub async fn handle_release_device(
    ctx: Arc<SettingsCtx>,
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
) {
    if let Err(e) = runtime.core().player().release_device() {
        log::error!("[qbz-slint] player.release_device failed: {e}");
    }
    // Let PipeWire/WirePlumber reclaim the just-freed device before we list.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let snap = {
        let ctx = ctx.clone();
        match tokio::task::spawn_blocking(move || load_snapshot(&ctx)).await {
            Ok(s) => s,
            Err(e) => {
                log::error!("[qbz-slint] release-device rebuild task failed: {e}");
                return;
            }
        }
    };
    let _ = weak.upgrade_in_event_loop(move |w| {
        apply_snapshot(&w, snap);
    });
}

/// Export the desktop's settings to a user-chosen `.qbzb` bundle (Settings >
/// Developer > "Export settings…", 04 §4.2). The modal collects the single
/// `--include-auth` gate; this glue reads it, closes the modal, then off the
/// event loop: builds the bundle via the shared `qbz_app::settings::bundle`
/// engine (no new export logic), opens a native save dialog seeded with
/// `qbz-settings-YYYYMMDD.qbzb`, writes the file 0600 (the engine enforces the
/// mode), and toasts the import command. Wired to `SettingsExportActions
/// .confirm()` in `main.rs`. Any failure surfaces an error toast; a cancelled
/// dialog is a silent no-op.
pub fn export_settings(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    // We are on the Slint event loop (invoked from the modal callback): read
    // the auth gate and close the modal here, before going async.
    let include_auth = weak
        .upgrade()
        .map(|w| {
            let st = w.global::<SettingsExportState>();
            let v = st.get_include_auth();
            st.set_open(false);
            v
        })
        .unwrap_or(false);

    handle.spawn(async move {
        // Build the bundle off the event loop (blocking SQLite reads).
        let built = tokio::task::spawn_blocking(move || {
            bundle::export(ExportSource::Desktop, &ExportOptions { include_auth })
        })
        .await;
        let doc = match built {
            Ok(Ok(b)) => b,
            Ok(Err(e)) => {
                crate::toast::error_weak(
                    &weak,
                    format!("{}: {e}", qbz_i18n::t("Could not export settings")),
                );
                return;
            }
            Err(e) => {
                crate::toast::error_weak(
                    &weak,
                    format!("{}: {e}", qbz_i18n::t("Could not export settings")),
                );
                return;
            }
        };

        // Native "save as" dialog seeded with the suggested filename; cancel =
        // silent no-op (matches booklet::download_booklet).
        let default_name = bundle::default_filename();
        let Some(dest) = rfd::AsyncFileDialog::new()
            .set_title(&qbz_i18n::t("Export settings"))
            .set_file_name(&default_name)
            .add_filter("QBZ settings bundle", &["qbzb"])
            .save_file()
            .await
        else {
            return;
        };
        let path = dest.path().to_path_buf();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| default_name.clone());

        // Write 0600 off the event loop.
        let write_path = path.clone();
        let written =
            tokio::task::spawn_blocking(move || bundle::write_bundle_file(&write_path, &doc)).await;
        match written {
            Ok(Ok(())) => {
                // Teach the import step (04 §4.2 toast copy). Only the lead-in is
                // localized; the command stays verbatim so it copies correctly.
                crate::toast::success_weak(
                    &weak,
                    format!(
                        "{} qbzd settings import {file_name}",
                        qbz_i18n::t("Bundle saved. On the daemon box:")
                    ),
                );
            }
            Ok(Err(e)) => crate::toast::error_weak(
                &weak,
                format!("{}: {e}", qbz_i18n::t("Could not export settings")),
            ),
            Err(e) => crate::toast::error_weak(
                &weak,
                format!("{}: {e}", qbz_i18n::t("Could not export settings")),
            ),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_sample_rate_table_starts_with_no_limit() {
        assert_eq!(MAX_SAMPLE_RATES[0].1, None);
        assert_eq!(MAX_SAMPLE_RATES.last().unwrap().1, Some(384_000));
        assert_eq!(MAX_SAMPLE_RATES.len(), 9);
    }

    #[test]
    fn alsa_plugin_table_first_is_hw() {
        assert_eq!(ALSA_PLUGINS[0].1, AlsaPlugin::Hw);
        assert_eq!(ALSA_PLUGINS.len(), 3);
    }

    #[test]
    fn retry_behavior_table_first_is_ask() {
        assert_eq!(RETRY_BEHAVIORS[0].1, "ask");
        assert_eq!(RETRY_BEHAVIORS[1].1, "always_fallback");
        assert_eq!(RETRY_BEHAVIORS[2].1, "always_skip");
    }

    #[test]
    fn alsa_section_classification_matches_tauri() {
        // Empty id = synthetic "System default" -> Defaults.
        assert_eq!(alsa_section("", false, "System default"), AlsaSection::Defaults);
        // The qbz-audio `default` device -> Defaults.
        assert_eq!(alsa_section("default", true, "default"), AlsaSection::Defaults);
        // Direct hardware / digital PCMs -> Bit-perfect.
        assert_eq!(
            alsa_section("front:CARD=C20,DEV=0", false, "Cambridge"),
            AlsaSection::BitPerfect
        );
        assert_eq!(
            alsa_section("iec958:CARD=PCH,DEV=0", false, "S/PDIF"),
            AlsaSection::BitPerfect
        );
        assert_eq!(alsa_section("hw:0,0", false, "raw"), AlsaSection::BitPerfect);
        // Plugin hardware -> Plugin Hardware.
        assert_eq!(
            alsa_section("plughw:0,0", false, "converted"),
            AlsaSection::PluginHw
        );
        // sysdefault: and hdmi: route through plugins / are not in the
        // Tauri ALSA bit-perfect rule -> Other Outputs.
        assert_eq!(
            alsa_section("sysdefault:CARD=PCH", false, "onboard"),
            AlsaSection::Other
        );
        assert_eq!(
            alsa_section("hdmi:CARD=HDMI,DEV=0", false, "HDMI"),
            AlsaSection::Other
        );
    }

    #[test]
    fn group_alsa_devices_orders_sections_and_places_headers() {
        // Deliberately scrambled input order.
        let rows = vec![
            DeviceRow {
                label: "HDMI out".into(),
                id: "hdmi:CARD=HDMI,DEV=0".into(),
                bp: false,
            },
            DeviceRow {
                label: "System default".into(),
                id: String::new(),
                bp: false,
            },
            DeviceRow {
                label: "Cambridge S/PDIF".into(),
                id: "iec958:CARD=C20,DEV=0".into(),
                bp: true,
            },
            DeviceRow {
                label: "Onboard".into(),
                id: "sysdefault:CARD=PCH".into(),
                bp: false,
            },
            DeviceRow {
                label: "Cambridge front".into(),
                id: "front:CARD=C20,DEV=0".into(),
                bp: true,
            },
        ];
        let list = group_alsa_devices(rows);
        // Section order: Defaults, Bit-perfect, Other.
        assert_eq!(
            list.ids,
            vec![
                "",
                "iec958:CARD=C20,DEV=0",
                "front:CARD=C20,DEV=0",
                "hdmi:CARD=HDMI,DEV=0",
                "sysdefault:CARD=PCH",
            ]
        );
        // Header appears on the first row of each section, empty otherwise.
        assert_eq!(
            list.groups,
            vec![
                "Defaults".to_string(),
                "Bit-perfect (Hardware / Digital)".to_string(),
                String::new(),
                "Other Outputs".to_string(),
                String::new(),
            ]
        );
        // BP badge only on the bit-perfect section.
        assert_eq!(list.bp, vec![false, true, true, false, false]);
        // All parallel lists stay index-aligned.
        assert_eq!(list.labels.len(), list.ids.len());
        assert_eq!(list.ids.len(), list.bp.len());
        assert_eq!(list.bp.len(), list.groups.len());
    }

    #[test]
    fn backend_labels_are_distinct() {
        let labels: Vec<_> = [
            AudioBackendType::PipeWire,
            AudioBackendType::Alsa,
            AudioBackendType::Pulse,
            AudioBackendType::SystemDefault,
        ]
        .iter()
        .map(|t| backend_label(*t))
        .collect();
        let mut deduped = labels.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(labels.len(), deduped.len());
    }
}
