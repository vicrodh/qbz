// crates/qbzd/src/cli/settings.rs — `qbzd settings show|set`,
// `qbzd qconnect enable|disable|name`, `qbzd config path|show`
// (02-cli-and-api.md §2.2). ALL ⬇ daemon-down capable (§2.4): every verb here
// reads/writes the daemon's REAL stores directly at the daemon roots
// (`AudioSettingsStore::new_at`, `PlaybackPreferencesStore::new_at`,
// `daemon_prefs`, the qconnect KV `_at` helpers — T9) and best-effort nudges a
// running daemon via `POST /api/settings/reload` afterwards
// (`login::nudge_reload` — the same ping-then-reload pattern `login`/`logout`
// already use), never the other way around (§1.1: the CLI holds no daemon
// state of its own). Export/import land in T12.
//
// The canonical dotted-key table below is this CLI's own copy of the desktop
// Apply ladder (`qbz/src/settings.rs:87-94`, per-key classification
// `:877-967,1134-1290`; 03-setup-tui.md §4.3's 9-field Reinit list). Per
// 03-setup-tui.md §4.3 (normative): the server's reload response carries NO
// reinit/reload narrative of its own — that classification is composed
// CLIENT-side, which is exactly what [`ApplyClass`] is for.
//
// Value encoding (deliberate, P0 scope): every value round-trips as a plain
// string — `settings show --json`'s values are exactly what `settings set`
// accepts back, key for key. No per-type JSON (bool/number) encoding; that
// would need a second parser on top of the one `set` already has, for a
// convenience no shipped P0/P1 script needs (the CLI is the machine
// interface here, not `/api/settings/reload`'s response body).

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use qbz_app::settings::bundle::{
    self, Bundle, DeviceChoice, ExportOptions, ExportSource, ImportOptions, ImportPlan, LiveSystem,
    ProfilePaths,
};
use qbz_app::settings::daemon_prefs;
use qbz_app::settings::playback::{AutoplayMode, PlaybackPreferencesStore};
use qbz_audio::settings::AudioSettingsStore;
use qbz_audio::{AlsaPlugin, AudioBackendType, BackendManager};

use crate::paths::ProfileRoots;
use crate::qconnect::transport as qconnect_kv;

/// Whether a key's write is Reinit-class (closes/reopens the output device),
/// Reload-class (struct refresh only, no audible gap), or affects nothing the
/// live `Player`/QConnect service reads immediately (playback prefs, qconnect
/// KV — applies next play / next connect respectively). Purely this CLI's own
/// bookkeeping for the success-line hint; the daemon decides for itself via
/// `daemon::audio_routing_changed` — the two are independent copies of the
/// same table, per 03-setup-tui.md §4.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyClass {
    Reinit,
    Reload,
    None,
}

/// The canonical dotted-key table (NORMATIVE for this build): `settings show`
/// lists exactly these, in this order; `settings set` accepts exactly these
/// keys and nothing else. Domains: `audio.*` (`AudioSettingsStore`),
/// `playback.*` (daemon_prefs.streaming_quality + `PlaybackPreferencesStore`),
/// `qconnect.*` (the daemon-root `qconnect_settings.db` KV, T9).
const KEY_TABLE: &[(&str, ApplyClass)] = &[
    // --- audio (Reinit — 03-setup-tui.md §4.3's 9-field list) -------------
    ("audio.backend", ApplyClass::Reinit),
    ("audio.device", ApplyClass::Reinit),
    ("audio.alsa_plugin", ApplyClass::Reinit),
    ("audio.alsa_hardware_volume", ApplyClass::Reinit),
    ("audio.exclusive_mode", ApplyClass::Reinit),
    ("audio.dac_passthrough", ApplyClass::Reinit),
    ("audio.skip_sink_switch", ApplyClass::Reinit),
    ("audio.dsd_mode", ApplyClass::Reinit),
    ("audio.device_max_sample_rate", ApplyClass::Reinit),
    // --- audio (Reload) -----------------------------------------------------
    ("audio.stream_first_track", ApplyClass::Reload),
    ("audio.stream_buffer_seconds", ApplyClass::Reload),
    ("audio.streaming_only", ApplyClass::Reload),
    ("audio.limit_quality_to_device", ApplyClass::Reload),
    ("audio.allow_quality_fallback", ApplyClass::Reload),
    ("audio.quality_fallback_behavior", ApplyClass::Reload),
    ("audio.gapless_enabled", ApplyClass::Reload),
    ("audio.normalization_enabled", ApplyClass::Reload),
    ("audio.normalization_target_lufs", ApplyClass::Reload),
    ("audio.pw_force_bitperfect", ApplyClass::Reload),
    ("audio.reserve_dac_while_running", ApplyClass::Reload),
    ("audio.sync_audio_on_startup", ApplyClass::Reload),
    // --- playback (daemon_prefs + PlaybackPreferencesStore) ----------------
    ("playback.quality", ApplyClass::None),
    ("playback.autoplay", ApplyClass::None),
    ("playback.persist_session", ApplyClass::None),
    ("playback.resume_playback_position", ApplyClass::None),
    ("playback.show_context_icon", ApplyClass::None),
    ("playback.mpris", ApplyClass::None),
    // --- qconnect (daemon-root qconnect_settings.db KV, T9) ----------------
    ("qconnect.device_name", ApplyClass::None),
    ("qconnect.startup_mode", ApplyClass::None),
    ("qconnect.volume_mode", ApplyClass::None),
];

fn classify(key: &str) -> Option<ApplyClass> {
    KEY_TABLE.iter().find(|(k, _)| *k == key).map(|(_, c)| *c)
}

/// The fault + fix for an unknown key (02 §1.4 error voice: name the fault,
/// then the fix). No "error:" prefix — `set()` adds it uniformly at the print
/// site, matching every other value-parse error in this file.
fn unknown_key_error(key: &str) -> String {
    let mut out = format!("unknown setting key '{key}'\n  → valid keys:\n");
    for (k, _) in KEY_TABLE {
        out.push_str(&format!("      {k}\n"));
    }
    out
}

fn qconnect_db(roots: &ProfileRoots) -> PathBuf {
    roots.data.join("qconnect_settings.db")
}

// ============================ value parsing ============================

fn parse_bool(v: &str) -> Result<bool, String> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "on" | "1" | "yes" => Ok(true),
        "false" | "off" | "0" | "no" => Ok(false),
        other => Err(format!("invalid value '{other}' — expected true or false")),
    }
}
fn render_bool(v: bool) -> String {
    v.to_string()
}

fn parse_backend(v: &str) -> Result<Option<AudioBackendType>, String> {
    match v.to_ascii_lowercase().as_str() {
        "system" | "systemdefault" | "system_default" => Ok(Some(AudioBackendType::SystemDefault)),
        "pipewire" | "pw" => Ok(Some(AudioBackendType::PipeWire)),
        "alsa" => Ok(Some(AudioBackendType::Alsa)),
        "pulse" | "pulseaudio" => Ok(Some(AudioBackendType::Pulse)),
        "jack" => Ok(Some(AudioBackendType::Jack)),
        other => Err(format!(
            "invalid backend '{other}' — expected one of: system, pipewire, alsa, pulse, jack"
        )),
    }
}
fn render_backend(v: Option<AudioBackendType>) -> String {
    match v {
        Some(AudioBackendType::SystemDefault) => "system".to_string(),
        Some(AudioBackendType::PipeWire) => "pipewire".to_string(),
        Some(AudioBackendType::Alsa) => "alsa".to_string(),
        Some(AudioBackendType::Pulse) => "pulse".to_string(),
        Some(AudioBackendType::Jack) => "jack".to_string(),
        None => "auto".to_string(),
    }
}

fn parse_alsa_plugin(v: &str) -> Result<Option<AlsaPlugin>, String> {
    match v.to_ascii_lowercase().as_str() {
        "hw" => Ok(Some(AlsaPlugin::Hw)),
        "plughw" => Ok(Some(AlsaPlugin::PlugHw)),
        "pcm" => Ok(Some(AlsaPlugin::Pcm)),
        // Not a documented TUI option (03-setup-tui.md §3.2.1 lists only the 3
        // concrete plugins, default Hw) — accepted here only so `settings
        // show`'s value on a never-migrated-to-a-plugin row (the seed INSERT
        // leaves this column NULL, unlike `backend_type`) round-trips back
        // into `set` unchanged instead of erroring on its own read value.
        "auto" => Ok(None),
        other => Err(format!(
            "invalid ALSA plugin '{other}' — expected one of: hw, plughw, pcm"
        )),
    }
}
fn render_alsa_plugin(v: Option<AlsaPlugin>) -> String {
    match v {
        Some(AlsaPlugin::Hw) => "hw".to_string(),
        Some(AlsaPlugin::PlugHw) => "plughw".to_string(),
        Some(AlsaPlugin::Pcm) => "pcm".to_string(),
        None => "auto".to_string(),
    }
}

/// `audio.device`: empty / "system" / "default" clears to `None` (system
/// default); anything else is the device id verbatim (`hw:CARD=D30,DEV=0`,
/// a PipeWire node name, ...) — free text, not validated against a live
/// enumeration here (that is the TUI's job, T13; a headless `settings set`
/// must work with no device attached to check against).
fn parse_output_device(v: &str) -> Option<String> {
    let trimmed = v.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("system") || trimmed.eq_ignore_ascii_case("default")
    {
        None
    } else {
        Some(trimmed.to_string())
    }
}
fn render_opt_string(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "system".to_string())
}

/// `audio.device_max_sample_rate`: "none"/"" clears the limit (Hz, matching
/// the stored unit directly — e.g. `192000`, not `192` kHz).
fn parse_opt_u32(v: &str) -> Result<Option<u32>, String> {
    let trimmed = v.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    trimmed
        .parse::<u32>()
        .map(Some)
        .map_err(|_| format!("invalid sample rate '{trimmed}' — expected a Hz integer (e.g. 192000) or none"))
}
fn render_opt_u32(v: Option<u32>) -> String {
    v.map(|r| r.to_string()).unwrap_or_else(|| "none".to_string())
}

fn parse_dsd_mode(v: &str) -> Result<String, String> {
    match v.to_ascii_lowercase().as_str() {
        "convert" | "dop" | "native" => Ok(v.to_ascii_lowercase()),
        other => Err(format!(
            "invalid DSD mode '{other}' — expected one of: convert, dop, native"
        )),
    }
}

/// The daemon has no one to ask (03-setup-tui.md §3.3.2) — `settings set`
/// never writes `"ask"`, even though a legacy/imported store may still hold
/// it (readable via `settings show`, just not settable back to it).
fn parse_quality_fallback_behavior(v: &str) -> Result<String, String> {
    match v.to_ascii_lowercase().as_str() {
        "always_fallback" | "always_skip" => Ok(v.to_ascii_lowercase()),
        "ask" => Err(
            "'ask' needs a UI the daemon doesn't have — use always_fallback or always_skip"
                .to_string(),
        ),
        other => Err(format!(
            "invalid value '{other}' — expected one of: always_fallback, always_skip"
        )),
    }
}

fn parse_f32(v: &str) -> Result<f32, String> {
    v.trim()
        .parse::<f32>()
        .map_err(|_| format!("invalid number '{v}'"))
}

fn parse_stream_buffer_seconds(v: &str) -> Result<u8, String> {
    let n: u8 = v
        .trim()
        .parse()
        .map_err(|_| format!("invalid buffer size '{v}' — expected 1-10"))?;
    if !(1..=10).contains(&n) {
        return Err(format!("invalid buffer size '{n}' — expected 1-10"));
    }
    Ok(n)
}

fn parse_streaming_quality(v: &str) -> Result<String, String> {
    match v.to_ascii_lowercase().as_str() {
        "mp3" | "cd" | "hires" | "hires_plus" => Ok(v.to_ascii_lowercase()),
        other => Err(format!(
            "invalid quality '{other}' — expected one of: mp3, cd, hires, hires_plus"
        )),
    }
}

fn parse_autoplay(v: &str) -> Result<AutoplayMode, String> {
    serde_json::from_value(serde_json::Value::String(v.to_string())).map_err(|_| {
        format!("invalid autoplay mode '{v}' — expected one of: continue, track_only, infinite")
    })
}
fn render_autoplay(mode: AutoplayMode) -> String {
    serde_json::to_value(mode)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "continue".to_string())
}

fn parse_volume_mode(v: &str) -> Result<String, String> {
    match v.to_ascii_lowercase().as_str() {
        "software" | "locked" => Ok(v.to_ascii_lowercase()),
        other => Err(format!(
            "invalid volume mode '{other}' — expected one of: software, locked"
        )),
    }
}

// ============================ store IO ============================

fn open_audio(roots: &ProfileRoots) -> Result<AudioSettingsStore, String> {
    AudioSettingsStore::new_at(&roots.data)
}
fn open_playback(roots: &ProfileRoots) -> Result<PlaybackPreferencesStore, String> {
    PlaybackPreferencesStore::new_at(&roots.data)
}

/// Read every canonical key's current value, in [`KEY_TABLE`] order — the
/// backing for `settings show`. Opens each store once (not once per key).
fn read_all(roots: &ProfileRoots) -> Result<Vec<(&'static str, String)>, String> {
    let audio = open_audio(roots)?.get_settings()?;
    let playback = open_playback(roots)?.get_preferences()?;
    let prefs = daemon_prefs::load_at(&roots.data);
    let db = qconnect_db(roots);

    let mut out = Vec::with_capacity(KEY_TABLE.len());
    for (key, _) in KEY_TABLE {
        let value = match *key {
            "audio.backend" => render_backend(audio.backend_type),
            "audio.device" => render_opt_string(&audio.output_device),
            "audio.alsa_plugin" => render_alsa_plugin(audio.alsa_plugin),
            "audio.alsa_hardware_volume" => render_bool(audio.alsa_hardware_volume),
            "audio.exclusive_mode" => render_bool(audio.exclusive_mode),
            "audio.dac_passthrough" => render_bool(audio.dac_passthrough),
            "audio.skip_sink_switch" => render_bool(audio.skip_sink_switch),
            "audio.dsd_mode" => audio.dsd_mode.clone(),
            "audio.device_max_sample_rate" => render_opt_u32(audio.device_max_sample_rate),
            "audio.stream_first_track" => render_bool(audio.stream_first_track),
            "audio.stream_buffer_seconds" => audio.stream_buffer_seconds.to_string(),
            "audio.streaming_only" => render_bool(audio.streaming_only),
            "audio.limit_quality_to_device" => render_bool(audio.limit_quality_to_device),
            "audio.allow_quality_fallback" => render_bool(audio.allow_quality_fallback),
            "audio.quality_fallback_behavior" => audio.quality_fallback_behavior.clone(),
            "audio.gapless_enabled" => render_bool(audio.gapless_enabled),
            "audio.normalization_enabled" => render_bool(audio.normalization_enabled),
            "audio.normalization_target_lufs" => audio.normalization_target_lufs.to_string(),
            "audio.pw_force_bitperfect" => render_bool(audio.pw_force_bitperfect),
            "audio.reserve_dac_while_running" => render_bool(audio.reserve_dac_while_running),
            "audio.sync_audio_on_startup" => render_bool(audio.sync_audio_on_startup),
            "playback.quality" => prefs.streaming_quality.clone(),
            "playback.autoplay" => render_autoplay(playback.autoplay_mode),
            "playback.persist_session" => render_bool(playback.persist_session),
            "playback.resume_playback_position" => render_bool(playback.resume_playback_position),
            "playback.show_context_icon" => render_bool(playback.show_context_icon),
            "playback.mpris" => render_bool(prefs.mpris_enabled),
            "qconnect.device_name" => render_opt_string(&qconnect_kv::load_device_name_at(&db)),
            "qconnect.startup_mode" => qconnect_kv::load_startup_mode_at(&db).as_str().to_string(),
            "qconnect.volume_mode" => {
                qconnect_kv::load_volume_mode_at(&db).unwrap_or_else(|| "software".to_string())
            }
            other => unreachable!("KEY_TABLE/read_all drifted apart on key: {other}"),
        };
        out.push((*key, value));
    }
    Ok(out)
}

/// The two exit-code classes a [`write_one`] failure can fall into (02 §1.3,
/// the frozen exit-code table: 2 is reserved for USAGE mistakes only). An
/// unknown key or an invalid value for a known key never touches a store —
/// that is `Usage` (exit 2). A key that classified and parsed fine but whose
/// backing store then failed to open or write — a disk-full/permissions/
/// corrupt-file problem — is not a usage mistake: that is `Io` (exit 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetError {
    Usage(String),
    Io(String),
}

impl SetError {
    fn message(&self) -> &str {
        match self {
            SetError::Usage(m) | SetError::Io(m) => m,
        }
    }
}

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// Validate + write ONE canonical key. Returns its [`ApplyClass`] on success
/// (the CLI's own success-line hint — see module doc). `pub(crate)` so the T13
/// setup TUI persists every screen through this SAME validated writer (03 §6 —
/// the TUI adds no persistence of its own). Every arm parses (`Usage` on
/// failure) BEFORE it opens/writes a store (`Io` on failure) — see [`SetError`].
pub(crate) fn write_one(roots: &ProfileRoots, key: &str, raw: &str) -> Result<ApplyClass, SetError> {
    let Some(class) = classify(key) else {
        return Err(SetError::Usage(unknown_key_error(key)));
    };
    match key {
        "audio.backend" => {
            let v = parse_backend(raw).map_err(SetError::Usage)?;
            open_audio(roots).map_err(SetError::Io)?.set_backend_type(v).map_err(SetError::Io)?
        }
        "audio.device" => {
            let v = parse_output_device(raw);
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_output_device(v.as_deref())
                .map_err(SetError::Io)?
        }
        "audio.alsa_plugin" => {
            let v = parse_alsa_plugin(raw).map_err(SetError::Usage)?;
            open_audio(roots).map_err(SetError::Io)?.set_alsa_plugin(v).map_err(SetError::Io)?
        }
        "audio.alsa_hardware_volume" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_alsa_hardware_volume(v)
                .map_err(SetError::Io)?
        }
        "audio.exclusive_mode" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots).map_err(SetError::Io)?.set_exclusive_mode(v).map_err(SetError::Io)?
        }
        "audio.dac_passthrough" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots).map_err(SetError::Io)?.set_dac_passthrough(v).map_err(SetError::Io)?
        }
        "audio.skip_sink_switch" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_skip_sink_switch(v)
                .map_err(SetError::Io)?
        }
        "audio.dsd_mode" => {
            let v = parse_dsd_mode(raw).map_err(SetError::Usage)?;
            open_audio(roots).map_err(SetError::Io)?.set_dsd_mode(&v).map_err(SetError::Io)?
        }
        "audio.device_max_sample_rate" => {
            let v = parse_opt_u32(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_device_max_sample_rate(v)
                .map_err(SetError::Io)?
        }
        "audio.stream_first_track" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_stream_first_track(v)
                .map_err(SetError::Io)?
        }
        "audio.stream_buffer_seconds" => {
            let v = parse_stream_buffer_seconds(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_stream_buffer_seconds(v)
                .map_err(SetError::Io)?
        }
        "audio.streaming_only" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots).map_err(SetError::Io)?.set_streaming_only(v).map_err(SetError::Io)?
        }
        "audio.limit_quality_to_device" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_limit_quality_to_device(v)
                .map_err(SetError::Io)?
        }
        "audio.allow_quality_fallback" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_allow_quality_fallback(v)
                .map_err(SetError::Io)?
        }
        "audio.quality_fallback_behavior" => {
            let v = parse_quality_fallback_behavior(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_quality_fallback_behavior(&v)
                .map_err(SetError::Io)?
        }
        "audio.gapless_enabled" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots).map_err(SetError::Io)?.set_gapless_enabled(v).map_err(SetError::Io)?
        }
        "audio.normalization_enabled" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_normalization_enabled(v)
                .map_err(SetError::Io)?
        }
        "audio.normalization_target_lufs" => {
            let v = parse_f32(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_normalization_target_lufs(v)
                .map_err(SetError::Io)?
        }
        "audio.pw_force_bitperfect" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_pw_force_bitperfect(v)
                .map_err(SetError::Io)?
        }
        "audio.reserve_dac_while_running" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_reserve_dac_while_running(v)
                .map_err(SetError::Io)?
        }
        "audio.sync_audio_on_startup" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_audio(roots)
                .map_err(SetError::Io)?
                .set_sync_audio_on_startup(v)
                .map_err(SetError::Io)?
        }
        "playback.quality" => {
            let v = parse_streaming_quality(raw).map_err(SetError::Usage)?;
            let mut prefs = daemon_prefs::load_at(&roots.data);
            prefs.streaming_quality = v;
            daemon_prefs::save_at(&prefs, &roots.data).map_err(SetError::Io)?;
        }
        "playback.mpris" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            let mut prefs = daemon_prefs::load_at(&roots.data);
            prefs.mpris_enabled = v;
            daemon_prefs::save_at(&prefs, &roots.data).map_err(SetError::Io)?;
        }
        "playback.autoplay" => {
            let v = parse_autoplay(raw).map_err(SetError::Usage)?;
            open_playback(roots).map_err(SetError::Io)?.set_autoplay_mode(v).map_err(SetError::Io)?
        }
        "playback.persist_session" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_playback(roots)
                .map_err(SetError::Io)?
                .set_persist_session(v)
                .map_err(SetError::Io)?
        }
        "playback.resume_playback_position" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_playback(roots)
                .map_err(SetError::Io)?
                .set_resume_playback_position(v)
                .map_err(SetError::Io)?
        }
        "playback.show_context_icon" => {
            let v = parse_bool(raw).map_err(SetError::Usage)?;
            open_playback(roots)
                .map_err(SetError::Io)?
                .set_show_context_icon(v)
                .map_err(SetError::Io)?
        }
        "qconnect.device_name" => {
            qconnect_kv::persist_device_name_at(&qconnect_db(roots), parse_output_device(raw).as_deref())
        }
        "qconnect.startup_mode" => {
            let mode = match raw.to_ascii_lowercase().as_str() {
                "on" => qconnect_app::QconnectStartupMode::On,
                "off" => qconnect_app::QconnectStartupMode::Off,
                other => {
                    return Err(SetError::Usage(format!(
                        "invalid startup mode '{other}' — expected one of: on, off (use: qbzd qconnect enable|disable)"
                    )))
                }
            };
            qconnect_kv::save_startup_mode_at(&qconnect_db(roots), mode)
        }
        "qconnect.volume_mode" => {
            let v = parse_volume_mode(raw).map_err(SetError::Usage)?;
            qconnect_kv::save_volume_mode_at(&qconnect_db(roots), &v)
        }
        other => unreachable!("KEY_TABLE/write_one drifted apart on key: {other}"),
    }
    Ok(class)
}

/// Best-effort nudge of a LOCAL running daemon (§1.1/§1.5: these verbs always
/// target the daemon whose stores they just wrote, never `--host`/
/// `QBZD_HOST` — same reasoning as `login`/`logout`). Reads the opt-in
/// `[server] token` the same way `cli/client.rs::resolve_token` does for the
/// local target, so a token-protected daemon still gets nudged.
fn nudge(roots: &ProfileRoots) -> bool {
    let host = crate::login::nudge_host(roots);
    let token = local_token(roots);
    crate::login::nudge_reload(&host, token.as_deref())
}

/// Three-state variant of [`nudge`] for `settings import`, which must
/// distinguish daemon-down from reload-refused (04 §5.3 step 7).
fn nudge_outcome(roots: &ProfileRoots) -> crate::login::NudgeOutcome {
    let host = crate::login::nudge_host(roots);
    let token = local_token(roots);
    crate::login::nudge_reload_outcome(&host, token.as_deref())
}

fn local_token(roots: &ProfileRoots) -> Option<String> {
    if let Ok(t) = std::env::var("QBZD_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    crate::config::QbzdConfig::load(&roots.config.join("qbzd.toml"))
        .ok()
        .and_then(|(c, _)| c.server.token)
        .filter(|t| !t.trim().is_empty())
}

// ============================ CLI entry points ============================

/// `qbzd settings show [--json]` (⬇). `--json`: `{"audio.backend": "alsa", ...}`
/// — every value the plain string `settings set` would accept back (module
/// doc). Exit: 0 · 1 (a store failed to open/read).
pub fn show(json: bool, roots: &ProfileRoots) -> i32 {
    let values = match read_all(roots) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    if json {
        let mut map = serde_json::Map::with_capacity(values.len());
        for (k, v) in &values {
            map.insert((*k).to_string(), serde_json::Value::String(v.clone()));
        }
        println!("{}", serde_json::to_string(&serde_json::Value::Object(map)).unwrap_or_default());
    } else {
        let width = values.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (k, v) in &values {
            println!("{k:width$} = {v}");
        }
    }
    0
}

/// `qbzd settings set <KEY> <VALUE>` (⬇). Unknown key → exit 2 listing valid
/// keys; invalid value for a known key → exit 2 naming the valid values;
/// the key/value classified and parsed fine but the backing store failed to
/// open or write (disk/permissions) → exit 1 (02 §1.3: 2 is USAGE-only —
/// see [`SetError`]). Writes always succeed locally before any daemon
/// contact is attempted (§2.4 daemon-down capable); daemon-down prints
/// `changes apply when the daemon starts` (this task's brief, verbatim)
/// instead of failing.
pub fn set(roots: &ProfileRoots, key: &str, value: &str) -> i32 {
    let class = match write_one(roots, key, value) {
        Ok(c) => c,
        Err(SetError::Usage(e)) => {
            eprintln!("error: {}", e.trim_end());
            return 2;
        }
        Err(SetError::Io(e)) => {
            eprintln!("error: {}", e.trim_end());
            return 1;
        }
    };
    if nudge(roots) {
        let hint = match class {
            ApplyClass::Reinit => " (daemon reinitialized the output device)",
            ApplyClass::Reload | ApplyClass::None => "",
        };
        println!("{key} = {value}{hint}");
    } else {
        println!("{key} = {value}");
        println!("changes apply when the daemon starts");
    }
    0
}

/// `qbzd qconnect enable` (⬇). Writes `startup_mode = on`, nudges reload.
/// Human line verbatim per 02 §2.2 (device name interpolated).
pub fn qconnect_enable(roots: &ProfileRoots) -> i32 {
    let db = qconnect_db(roots);
    qconnect_kv::save_startup_mode_at(&db, qconnect_app::QconnectStartupMode::On);
    nudge(roots);
    let name = qconnect_kv::resolve_qconnect_friendly_name(qconnect_kv::load_device_name_at(&db).as_deref());
    println!("qconnect enabled — device \"{name}\" will appear in the Qobuz app once logged in");
    0
}

/// `qbzd qconnect disable` (⬇). Writes `startup_mode = off`, nudges reload
/// (disconnects the live session, per `daemon::reload_qconnect`).
pub fn qconnect_disable(roots: &ProfileRoots) -> i32 {
    let db = qconnect_db(roots);
    qconnect_kv::save_startup_mode_at(&db, qconnect_app::QconnectStartupMode::Off);
    nudge(roots);
    println!("qconnect disabled");
    0
}

/// `qbzd qconnect name "Living Room"` (⬇). Empty clears the override back to
/// the hostname default (desktop write semantics preserved, 03-setup-tui.md
/// §3.4). Applies on the NEXT connection, never forces a reconnect
/// (`daemon::reload_qconnect` / `QconnectControl::refresh_device_name`).
pub fn qconnect_name(roots: &ProfileRoots, name: &str) -> i32 {
    let db = qconnect_db(roots);
    qconnect_kv::persist_device_name_at(&db, parse_output_device(name).as_deref());
    nudge(roots);
    let effective = qconnect_kv::resolve_qconnect_friendly_name(parse_output_device(name).as_deref());
    println!("qconnect device name set to \"{effective}\" — applies on the next connection");
    0
}

/// `qbzd config path` (⬇). Process roots + the credential file + the
/// (conventional, not necessarily yet installed — T14) systemd user unit
/// path. No `--json` (02 §2.2 gives none for this subverb).
pub fn config_path(roots: &ProfileRoots) -> i32 {
    println!("config : {}", roots.config.display());
    println!("data   : {}", roots.data.display());
    println!("cache  : {}", roots.cache.display());
    println!("cred   : {}", roots.config.join(".qbz-oauth-token").display());
    println!("unit   : {}", unit_path().display());
    0
}

fn unit_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("systemd/user/qbzd.service")
}

/// `qbzd config show [--json]` (⬇) — effective `qbzd.toml`, process concerns
/// only (01-architecture.md §10.1; engine settings live in `settings show`).
/// Keys the file doesn't set are annotated `(default)` in human mode.
pub fn config_show(json: bool, roots: &ProfileRoots) -> i32 {
    let path = roots.config.join("qbzd.toml");
    let (cfg, _warns) = match crate::config::QbzdConfig::load(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    if json {
        println!("{}", serde_json::to_string(&cfg).unwrap_or_default());
        return 0;
    }
    let present = present_keys(&path);
    let line = |label: &str, dotted: &str, value: String| {
        let marker = if present.contains(dotted) { "" } else { " (default)" };
        println!("{label:<24}= {value}{marker}");
    };
    line("config_version", "config_version", cfg.config_version.to_string());
    line(
        "data_root",
        "data_root",
        cfg.data_root.clone().unwrap_or_else(|| "(auto)".to_string()),
    );
    line("server.bind", "server.bind", cfg.server.bind.clone());
    line("server.port", "server.port", cfg.server.port.to_string());
    line(
        "server.token",
        "server.token",
        match &cfg.server.token {
            Some(t) if !t.trim().is_empty() => "(set)".to_string(),
            _ => "(empty = open)".to_string(),
        },
    );
    line("log.level", "log.level", cfg.log.level.clone());
    line("mpris.enabled", "mpris.enabled", cfg.mpris.enabled.to_string());
    0
}

/// Which dotted config keys the on-disk `qbzd.toml` actually sets (vs.
/// defaulted) — a missing/unreadable file means every key is `(default)`.
fn present_keys(path: &Path) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
    };
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return out;
    };
    if let toml::Value::Table(top) = &value {
        for (k, v) in top {
            if let toml::Value::Table(inner) = v {
                for ik in inner.keys() {
                    out.insert(format!("{k}.{ik}"));
                }
            } else {
                out.insert(k.clone());
            }
        }
    }
    out
}

// ============================ settings export / import (T12) ============================

/// `qbzd settings export [FILE] [--from daemon|desktop] [--include-auth]` (⬇,
/// 04-settings-portability.md §4.1). Reads the daemon (default) or the desktop's
/// GLOBAL stores, writes ONE versioned JSON bundle at 0600. Exit: 0 · 1 · 2.
pub fn export(roots: &ProfileRoots, file: Option<String>, from: &str, include_auth: bool) -> i32 {
    let source = match from {
        "daemon" => ExportSource::Daemon(ProfilePaths {
            config_root: roots.config.clone(),
            data_root: roots.data.clone(),
        }),
        "desktop" => ExportSource::Desktop,
        other => {
            eprintln!("error: invalid --from '{other}' — expected 'daemon' or 'desktop'");
            return 2;
        }
    };

    let bundle = match bundle::export(source, &ExportOptions { include_auth }) {
        Ok(b) => b,
        Err(bundle::BundleError::NoDesktopProfile) => {
            eprintln!("{}", crate::cli::copy::bundle_no_desktop_profile());
            return 1;
        }
        Err(bundle::BundleError::TokenDecryptFailed) => {
            eprintln!("{}", crate::cli::copy::bundle_token_decrypt_failed());
            return 1;
        }
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    let path = file.unwrap_or_else(bundle::default_filename);
    if let Err(e) = bundle::write_bundle_file(&PathBuf::from(&path), &bundle) {
        eprintln!("error: {e}");
        return 1;
    }

    // The §3 warning prints whenever ANY secret actually made it into the file:
    // the auth token OR a non-blank scrobbler secret (--include-auth exports
    // scrobbler tokens even when the Qobuz token itself is absent).
    if bundle.contains_secrets() {
        println!("{}", crate::cli::copy::bundle_secret_warning(&path));
    } else {
        println!("{}", crate::cli::copy::bundle_export_success(&path));
    }
    0
}

/// `qbzd settings import FILE [--include-auth] [--trust-dsd] [--remap OLD=NEW]...
/// [--dry-run]` (⬇, 04 §5.3). read → version-gate → plan → (TTY device re-pick /
/// non-tty safe defaults) → validate secrets BEFORE any write → apply →
/// reload-nudge → three-bucket summary. Exit: 0 · 1 · 2 · 4.
pub async fn import(
    roots: &ProfileRoots,
    file: &str,
    include_auth: bool,
    trust_dsd: bool,
    remap_raw: &[String],
    dry_run: bool,
) -> i32 {
    // Step 1: read + parse.
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: cannot read bundle: {e}");
            return 1;
        }
    };
    let bundle = match Bundle::parse(&text) {
        Ok(b) => b,
        Err(bundle::BundleError::VersionMalformed) => {
            eprintln!("error: cannot read bundle: missing or non-integer schema_version");
            return 1;
        }
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    // --remap OLD=NEW (parsed + validated even though the P0 daemon skips
    // library_folders, so scripts written for P1 do not break — 04 §5.2).
    let mut remap = Vec::new();
    for r in remap_raw {
        match r.split_once('=') {
            Some((old, new)) => remap.push((old.to_string(), new.to_string())),
            None => {
                eprintln!("error: invalid --remap '{r}' — expected OLD=NEW");
                return 2;
            }
        }
    }

    let target = ProfilePaths {
        config_root: roots.config.clone(),
        data_root: roots.data.clone(),
    };
    let non_tty = !std::io::stdin().is_terminal();
    let opts = ImportOptions {
        include_auth,
        trust_dsd,
        remap,
        non_tty,
    };
    let live = build_live_system(&bundle);

    // Steps 2–4: plan.
    let mut plan = match bundle::plan(&bundle, &target, &opts, &live) {
        Ok(p) => p,
        Err(bundle::BundleError::VersionTooNew { bundle: b, supported }) => {
            eprintln!("{}", crate::cli::copy::bundle_version_too_new(b, supported));
            return 1;
        }
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    print_summary_header(&bundle);

    // Step 4: interactive device re-pick (TTY only; non-tty already fell back).
    if let Some(pick) = plan.device_pick.clone() {
        if !non_tty && !dry_run {
            let choice = prompt_device(&pick);
            plan = match bundle::replan_with_device(&bundle, &target, &opts, &live, choice) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
        }
    }

    // Step 5: validate secrets BEFORE any write (rejected → exit 4, nothing done).
    let mut validated_uid: Option<u64> = None;
    let mut auth_note: Option<String> = None;
    if let Some(token) = plan.auth_token.clone() {
        match crate::login::validate_token(&token).await {
            Ok(session) => {
                validated_uid = Some(session.user_id);
                let mut note =
                    format!("Qobuz token validated — logged in as user {}", session.user_id);
                if let Some(bid) = plan.bundle_user_id {
                    if bid != session.user_id {
                        note.push_str(&format!(
                            "\n  note: bundle user_id {bid} differs from the validated login {}",
                            session.user_id
                        ));
                    }
                }
                auth_note = Some(note);
            }
            Err(_) => {
                eprintln!("{}", crate::cli::copy::bundle_token_rejected());
                return 4;
            }
        }
    }

    // Dry-run stops after step 5 (04 §5.1): same summary, writes nothing.
    if dry_run {
        print_buckets(&plan, &bundle, auth_note.as_deref(), None);
        println!("\ndry-run: no changes written");
        return 0;
    }

    // Step 6: apply (validate-all-then-apply; re-run is idempotent).
    if let Err(e) = bundle::apply(&plan, &target, validated_uid) {
        print_buckets(&plan, &bundle, auth_note.as_deref(), None);
        eprintln!(
            "\nerror: settings only partially applied: {e}\n  → fix the disk/permissions, then re-run (import is idempotent)"
        );
        return 1;
    }

    // Step 7: reload-nudge a running daemon. Three states (04 §5.3 step 7):
    // reloaded / not running (fine) / up-but-refused (exit 1, restart hint).
    let outcome = nudge_outcome(roots);
    let (done_line, stderr_msg, exit) =
        reload_disposition(outcome, plan.routing_critical_changed);

    print_buckets(&plan, &bundle, auth_note.as_deref(), Some(&done_line));
    if let Some(msg) = stderr_msg {
        eprintln!("\n{msg}");
    }
    exit
}

/// Pure mapping of the nudge outcome (+ whether a routing-critical field
/// changed) to the `done:` reload phrase, an optional stderr error, and the
/// exit code. Split from IO so the §5.3-step-7 contract is unit-testable.
fn reload_disposition(
    outcome: crate::login::NudgeOutcome,
    routing_critical: bool,
) -> (String, Option<String>, i32) {
    use crate::login::NudgeOutcome::*;
    match outcome {
        Reloaded => {
            // §5.3 step 7 honesty rule: a routing-critical change re-inits the
            // output device on the spot — say so instead of hiding it.
            let line = if routing_critical {
                "daemon reloaded (was running; output device reinitialized — an in-flight track may gap)"
            } else {
                "daemon reloaded (was running)"
            };
            (line.to_string(), None, 0)
        }
        DaemonDown => (
            "daemon not running (changes apply on next start)".to_string(),
            None,
            0,
        ),
        ReloadRefused => (
            "daemon answered ping but refused the reload".to_string(),
            Some(
                "error: settings saved but the daemon did not reload — restart it: systemctl --user restart qbzd"
                    .to_string(),
            ),
            1,
        ),
    }
}

/// Enumerate the local audio system for [`bundle::plan`]: the available backends
/// + the devices of the bundle's intended backend (the picker's candidate list).
fn build_live_system(bundle: &Bundle) -> LiveSystem {
    let backends: Vec<String> = BackendManager::available_backends()
        .into_iter()
        .filter_map(|b| serde_json::to_value(b).ok().and_then(|v| v.as_str().map(str::to_string)))
        .collect();

    let wanted: Option<AudioBackendType> = bundle
        .domains
        .get("audio")
        .and_then(|a| a.get("backend_type"))
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let backend = wanted.unwrap_or(AudioBackendType::SystemDefault);
    let devices = BackendManager::create_backend(backend)
        .and_then(|b| b.enumerate_devices())
        .map(|list| list.into_iter().map(|d| (d.id, d.name)).collect())
        .unwrap_or_default();

    LiveSystem { backends, devices }
}

/// Interactive device picker (04 §5.4). Numbered device list; the last entry is
/// always "system default"; an unparseable answer falls to system default.
fn prompt_device(pick: &bundle::DevicePick) -> DeviceChoice {
    println!(
        "audio device \"{}\" not found on this machine. Available on {}:",
        pick.wanted, pick.backend
    );
    for (i, (id, label)) in pick.options.iter().enumerate() {
        println!("  [{}] {}  {}", i + 1, id, label);
    }
    let sys_idx = pick.options.len() + 1;
    println!("  [{sys_idx}] system default");
    print!("pick a device [1-{sys_idx}]: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    match line.trim().parse::<usize>() {
        Ok(n) if n >= 1 && n <= pick.options.len() => {
            let (id, label) = pick.options[n - 1].clone();
            println!();
            DeviceChoice::Device { id, label }
        }
        _ => {
            println!();
            DeviceChoice::SystemDefault
        }
    }
}

fn print_summary_header(bundle: &Bundle) {
    let date = bundle
        .created_at
        .split('T')
        .next()
        .unwrap_or(&bundle.created_at);
    println!(
        "bundle: schema v{} — exported {} from \"{}\" (qbz {}, {} profile)",
        bundle.schema_version,
        date,
        bundle.source.hostname,
        bundle.source.app_version,
        bundle.source.profile
    );
    println!();
}

/// The three-bucket summary EXACTLY in the 04 §5.4 format: applied (`= value`),
/// adapted (`old -> new (why)`), skipped (`+why`), the desktop shared-name
/// advisory, the auth footer, and the `done:` line.
fn print_buckets(
    plan: &ImportPlan,
    bundle: &Bundle,
    auth_note: Option<&str>,
    reload_line: Option<&str>,
) {
    let width = plan
        .applied
        .iter()
        .chain(&plan.adapted)
        .chain(&plan.skipped)
        .map(|l| l.key.len())
        .max()
        .unwrap_or(0)
        .min(44);

    println!("applied ({})", plan.applied.len());
    for l in &plan.applied {
        let note = if l.why.is_empty() {
            String::new()
        } else {
            format!(" ({})", l.why)
        };
        println!("  {:width$} = {}{}", l.key, l.new, note);
    }

    println!("\nadapted ({})", plan.adapted.len());
    for l in &plan.adapted {
        println!(
            "  {:width$} {} -> {} ({})",
            l.key,
            l.old.as_deref().unwrap_or(""),
            l.new,
            l.why
        );
    }

    println!("\nskipped ({})", plan.skipped.len());
    for l in &plan.skipped {
        println!("  {:width$} {}", l.key, l.why);
    }

    // Shared-device-name advisory: only for a desktop-sourced bundle (§2.4/§5.4).
    if bundle.source.profile == "desktop" {
        if let Some(dn) = plan.applied.iter().find(|l| l.key == "qconnect.device_name") {
            println!("\nadvisory");
            println!(
                "  qconnect.device_name \"{}\" is also the exporting desktop's Connect name — two nodes with",
                dn.new
            );
            println!(
                "  one name are hard to tell apart in the Qobuz app; rename with: qbzd qconnect name <name>"
            );
        }
    }

    if let Some(note) = auth_note {
        println!("\nauth");
        println!("  {note}");
    }

    if let Some(reload) = reload_line {
        println!(
            "\ndone: {} applied, {} adapted, {} skipped — {}",
            plan.applied.len(),
            plan.adapted.len(),
            plan.skipped.len(),
            reload
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_roots(name: &str) -> ProfileRoots {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "qbzd-cli-settings-{name}-{}-{nonce}",
            std::process::id()
        ));
        ProfileRoots {
            config: base.join("config"),
            data: base.join("data"),
            cache: base.join("cache"),
        }
    }

    fn cleanup(roots: &ProfileRoots) {
        let _ = std::fs::remove_dir_all(roots.data.parent().unwrap_or(&roots.data));
    }

    // ------------------------- key table integrity -------------------------

    #[test]
    fn unknown_key_is_rejected_and_lists_every_valid_key() {
        let roots = scratch_roots("unknown-key");
        let err = write_one(&roots, "audio.bogus", "x").unwrap_err();
        // 02 §1.3: an unknown key is a USAGE mistake (exit 2), never Io.
        assert!(matches!(err, SetError::Usage(_)), "{err:?}");
        assert!(err.message().contains("unknown setting key 'audio.bogus'"), "{err}");
        for (k, _) in KEY_TABLE {
            assert!(err.message().contains(k), "missing '{k}' from the listed keys:\n{err}");
        }
        cleanup(&roots);
    }

    #[test]
    fn invalid_value_for_a_known_key_is_a_usage_error_not_io() {
        // 02 §1.3: a bad value for a KNOWN key is still exit 2 (usage), same
        // class as an unknown key — never Io, which is reserved for a store
        // that failed to open/write after the value parsed fine.
        let roots = scratch_roots("bad-value");
        let err = write_one(&roots, "audio.exclusive_mode", "maybe").unwrap_err();
        assert!(matches!(err, SetError::Usage(_)), "{err:?}");
        cleanup(&roots);
    }

    #[test]
    fn store_open_failure_is_an_io_error_not_usage() {
        // 02 §1.3: the key classifies and the value parses fine (exclusive_mode
        // is a plain bool) — a store that then fails to even OPEN (here: the
        // data root is blocked by a plain file, so `create_dir_all` fails) is
        // exit 1 (Io), never the usage exit 2.
        let roots = scratch_roots("store-open-blocked");
        std::fs::create_dir_all(roots.data.parent().unwrap()).unwrap();
        std::fs::write(&roots.data, b"not a directory").unwrap();
        let err = write_one(&roots, "audio.exclusive_mode", "true").unwrap_err();
        assert!(matches!(err, SetError::Io(_)), "{err:?}");
        cleanup(&roots);
    }

    #[test]
    fn key_table_has_no_duplicate_keys() {
        let mut seen = std::collections::HashSet::new();
        for (k, _) in KEY_TABLE {
            assert!(seen.insert(*k), "duplicate canonical key: {k}");
        }
    }

    #[test]
    fn show_json_round_trips_into_set_for_every_canonical_key() {
        // The brief's Step 1 property: `settings show --json` includes every
        // key `settings set` accepts, AND the value it reports for a key is
        // itself a valid `set` input for that same key (a real functional
        // round-trip against temp on-disk stores, not just "same key names").
        let roots = scratch_roots("roundtrip");
        let values = read_all(&roots).expect("read_all opens fresh stores with defaults");
        assert_eq!(values.len(), KEY_TABLE.len(), "read_all must cover every canonical key");
        for (key, value) in &values {
            // The one documented exception (03-setup-tui.md §3.3.2): a fresh
            // (or desktop-imported) store's `quality_fallback_behavior`
            // column defaults to `"ask"`, which `set` correctly REJECTS —
            // "the daemon has no one to ask... the TUI never writes ask".
            // `show` must still be able to READ it; the round-trip property
            // is deliberately one-way for this single value.
            if *key == "audio.quality_fallback_behavior" && value == "ask" {
                continue;
            }
            write_one(&roots, key, value)
                .unwrap_or_else(|e| panic!("show's own value for '{key}' ('{value}') was rejected by set: {e}"));
        }
        cleanup(&roots);
    }

    #[test]
    fn set_then_show_persists_across_a_fresh_store_open() {
        let roots = scratch_roots("persist");
        write_one(&roots, "audio.backend", "alsa").expect("set backend");
        write_one(&roots, "audio.exclusive_mode", "true").expect("set exclusive");
        write_one(&roots, "playback.quality", "cd").expect("set quality");
        write_one(&roots, "playback.autoplay", "track_only").expect("set autoplay");
        write_one(&roots, "qconnect.device_name", "Kitchen").expect("set device name");
        write_one(&roots, "qconnect.startup_mode", "on").expect("set startup mode");

        let values: std::collections::HashMap<_, _> = read_all(&roots).unwrap().into_iter().collect();
        assert_eq!(values["audio.backend"], "alsa");
        assert_eq!(values["audio.exclusive_mode"], "true");
        assert_eq!(values["playback.quality"], "cd");
        assert_eq!(values["playback.autoplay"], "track_only");
        assert_eq!(values["qconnect.device_name"], "Kitchen");
        assert_eq!(values["qconnect.startup_mode"], "on");
        cleanup(&roots);
    }

    #[test]
    fn qconnect_device_name_empty_clears_to_default() {
        let roots = scratch_roots("qc-clear");
        write_one(&roots, "qconnect.device_name", "Studio").expect("set name");
        write_one(&roots, "qconnect.device_name", "").expect("clear name");
        let values: std::collections::HashMap<_, _> = read_all(&roots).unwrap().into_iter().collect();
        assert_eq!(values["qconnect.device_name"], "system");
        cleanup(&roots);
    }

    // ------------------------------ value parsing ------------------------------

    #[test]
    fn parse_bool_accepts_common_spellings_and_rejects_garbage() {
        assert_eq!(parse_bool("true"), Ok(true));
        assert_eq!(parse_bool("On"), Ok(true));
        assert_eq!(parse_bool("1"), Ok(true));
        assert_eq!(parse_bool("false"), Ok(false));
        assert_eq!(parse_bool("Off"), Ok(false));
        assert!(parse_bool("maybe").is_err());
    }

    #[test]
    fn parse_backend_accepts_the_five_concrete_backends_only() {
        assert_eq!(parse_backend("alsa"), Ok(Some(AudioBackendType::Alsa)));
        assert_eq!(parse_backend("PipeWire"), Ok(Some(AudioBackendType::PipeWire)));
        assert_eq!(parse_backend("system"), Ok(Some(AudioBackendType::SystemDefault)));
        assert!(parse_backend("auto").is_err(), "Auto is omitted in v1 (03-setup-tui.md §3.2.1)");
        assert!(parse_backend("bogus").is_err());
    }

    #[test]
    fn parse_quality_fallback_behavior_rejects_ask() {
        assert_eq!(parse_quality_fallback_behavior("always_fallback"), Ok("always_fallback".into()));
        assert_eq!(parse_quality_fallback_behavior("always_skip"), Ok("always_skip".into()));
        let err = parse_quality_fallback_behavior("ask").unwrap_err();
        assert!(err.contains("needs a UI"), "{err}");
    }

    #[test]
    fn parse_streaming_quality_matches_the_four_canonical_keys() {
        for ok in ["mp3", "cd", "hires", "hires_plus"] {
            assert!(parse_streaming_quality(ok).is_ok(), "{ok}");
        }
        assert!(parse_streaming_quality("hires192").is_err(), "not a real key — see report");
    }

    #[test]
    fn parse_autoplay_matches_the_playback_preferences_wire_values() {
        assert_eq!(parse_autoplay("continue").unwrap(), AutoplayMode::ContinueWithinSource);
        assert_eq!(parse_autoplay("track_only").unwrap(), AutoplayMode::PlayTrackOnly);
        assert_eq!(parse_autoplay("infinite").unwrap(), AutoplayMode::InfiniteRadio);
        assert!(parse_autoplay("bogus").is_err());
    }

    #[test]
    fn parse_opt_u32_clears_on_none_or_empty() {
        assert_eq!(parse_opt_u32(""), Ok(None));
        assert_eq!(parse_opt_u32("none"), Ok(None));
        assert_eq!(parse_opt_u32("192000"), Ok(Some(192_000)));
        assert!(parse_opt_u32("loud").is_err());
    }

    #[test]
    fn parse_dsd_mode_rejects_unknown_modes() {
        for ok in ["convert", "dop", "native"] {
            assert!(parse_dsd_mode(ok).is_ok());
        }
        assert!(parse_dsd_mode("bogus").is_err());
    }

    #[test]
    fn parse_stream_buffer_seconds_enforces_1_to_10() {
        assert_eq!(parse_stream_buffer_seconds("2"), Ok(2));
        assert_eq!(parse_stream_buffer_seconds("10"), Ok(10));
        assert!(parse_stream_buffer_seconds("0").is_err());
        assert!(parse_stream_buffer_seconds("11").is_err());
    }

    #[test]
    fn present_keys_empty_for_a_missing_file() {
        let roots = scratch_roots("config-missing");
        let keys = present_keys(&roots.config.join("qbzd.toml"));
        assert!(keys.is_empty());
        cleanup(&roots);
    }

    #[test]
    fn reload_disposition_maps_the_three_outcomes() {
        use crate::login::NudgeOutcome::*;

        let (line, err, code) = reload_disposition(Reloaded, false);
        assert_eq!(line, "daemon reloaded (was running)");
        assert!(err.is_none());
        assert_eq!(code, 0);

        // §5.3 step 7 honesty rule: routing-critical + reloaded names the gap.
        let (line, err, code) = reload_disposition(Reloaded, true);
        assert!(line.contains("output device reinitialized"), "{line}");
        assert!(line.contains("gap"), "{line}");
        assert!(err.is_none());
        assert_eq!(code, 0);

        // Daemon simply not running is NOT an error, routing-critical or not.
        let (line, err, code) = reload_disposition(DaemonDown, true);
        assert_eq!(line, "daemon not running (changes apply on next start)");
        assert!(err.is_none());
        assert_eq!(code, 0);

        // Up-but-refused → exit 1 with the verbatim restart hint.
        let (_, err, code) = reload_disposition(ReloadRefused, false);
        let msg = err.expect("refused must carry the stderr copy");
        assert_eq!(
            msg,
            "error: settings saved but the daemon did not reload — restart it: systemctl --user restart qbzd"
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn present_keys_reports_nested_and_top_level_keys() {
        let roots = scratch_roots("config-present");
        std::fs::create_dir_all(&roots.config).unwrap();
        std::fs::write(
            roots.config.join("qbzd.toml"),
            "config_version = 1\n[server]\nport = 9000\n",
        )
        .unwrap();
        let keys = present_keys(&roots.config.join("qbzd.toml"));
        assert!(keys.contains("config_version"));
        assert!(keys.contains("server.port"));
        assert!(!keys.contains("server.bind"));
        cleanup(&roots);
    }
}
