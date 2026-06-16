//! HiFi Wizard (DAC setup) controller.
//!
//! Slice 6 (check step): runs the frontend-agnostic audio-stack probes
//! (`qbz_audio::health`) on open, maps them to per-distro copy-paste
//! remediations the check step renders, and recomputes them when the user
//! overrides the distro. Read-only — nothing here writes a system file or
//! opens a stream.

use std::sync::Mutex;

use qbz_audio::{AudioStackHealth, Distro, InitSystem};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::{AppWindow, DacCandidateRow, DacWizardState, RemediationRow};

// Last probe result, so a distro override recomputes commands without
// re-shelling on every dropdown change.
static LAST_HEALTH: Mutex<Option<AudioStackHealth>> = Mutex::new(None);

/// Synchronous part of opening the wizard: reset, fill the distro dropdown
/// (auto-detected, always overridable), and show a "checking…" state until the
/// async probe lands.
pub fn open_immediate(window: &AppWindow) {
    let st = window.global::<DacWizardState>();
    st.set_open(true);
    st.set_step(0);
    st.set_welcome_confirmed(false);

    let distro_opts: Vec<slint::SharedString> =
        Distro::ALL.iter().map(|d| d.label().into()).collect();
    st.set_distro_options(ModelRc::new(VecModel::from(distro_opts)));
    st.set_distro_index(qbz_audio::detect_distro().index() as i32);

    let init_opts: Vec<slint::SharedString> =
        InitSystem::ALL.iter().map(|i| i.label().into()).collect();
    st.set_init_options(ModelRc::new(VecModel::from(init_opts)));
    st.set_init_index(qbz_audio::detect_init().index() as i32);

    let sandbox = qbz_audio::detect_sandbox();
    st.set_sandboxed(sandbox != qbz_audio::Sandbox::None);
    st.set_sandbox_name(
        match sandbox {
            qbz_audio::Sandbox::Flatpak => "Flatpak",
            qbz_audio::Sandbox::Snap => "Snap",
            qbz_audio::Sandbox::None => "",
        }
        .into(),
    );

    st.set_health_ok(false);
    st.set_health_summary("Checking your audio stack…".into());
    st.set_remediations(ModelRc::new(VecModel::from(Vec::<RemediationRow>::new())));
}

/// Apply a completed health probe: cache it and re-render from the current
/// distro/init selections.
pub fn apply_health(window: &AppWindow, health: AudioStackHealth) {
    *LAST_HEALTH.lock().unwrap() = Some(health);
    recompute(window);
}

/// User overrode the distro (package manager) — recompute.
pub fn set_distro(window: &AppWindow, index: i32) {
    window.global::<DacWizardState>().set_distro_index(index);
    recompute(window);
}

/// User overrode the init system (service commands) — recompute.
pub fn set_init(window: &AppWindow, index: i32) {
    window.global::<DacWizardState>().set_init_index(index);
    recompute(window);
}

/// Rebuild the remediations from the cached probe + the current distro/init
/// dropdown selections (either of which the user can override).
fn recompute(window: &AppWindow) {
    let st = window.global::<DacWizardState>();
    let health = LAST_HEALTH
        .lock()
        .unwrap()
        .unwrap_or_else(qbz_audio::audio_stack_health);
    let distro = Distro::ALL
        .get(st.get_distro_index().max(0) as usize)
        .copied()
        .unwrap_or(Distro::Other);
    let init = InitSystem::ALL
        .get(st.get_init_index().max(0) as usize)
        .copied()
        .unwrap_or(InitSystem::Unknown);

    // In a sandbox the host probes are blind, so don't render a health verdict —
    // show reference setup commands for the chosen distro/init (Tauri-style,
    // which never probed either). The UI shows a sandbox info banner instead.
    let rows = if st.get_sandboxed() {
        st.set_health_ok(false);
        st.set_health_summary("".into());
        reference_commands(distro, init)
    } else {
        let r = remediations(health, distro, init);
        st.set_health_ok(health.is_ready());
        st.set_health_summary(if health.is_ready() {
            "Your audio stack is ready for bit-perfect playback.".into()
        } else {
            let n = r.len();
            format!(
                "{} item{} need attention before bit-perfect playback will work.",
                n,
                if n == 1 { "" } else { "s" }
            )
            .into()
        });
        r
    };
    let model: Vec<RemediationRow> = rows
        .into_iter()
        .map(|(caption, command)| RemediationRow {
            caption: caption.into(),
            command: command.into(),
        })
        .collect();
    st.set_remediations(ModelRc::new(VecModel::from(model)));
}

/// (caption, copy-paste command) per missing probe, for the given distro.
///
/// Service/restart commands are INIT-SYSTEM aware per distro (OpenRC on Gentoo,
/// runit on Void, systemd elsewhere), mirroring the Tauri DistroSelector
/// `restartCommands`. Installs and the restart are kept as separate blocks so
/// the multi-line Gentoo guidance never gets `&&`-joined.
fn remediations(h: AudioStackHealth, d: Distro, init: InitSystem) -> Vec<(String, String)> {
    // NixOS is declarative: you don't `apt/pacman install` pieces — you enable
    // the PipeWire module and rebuild. So collapse all the missing pieces into
    // one config block instead of per-package commands.
    if d == Distro::NixOS {
        if h.is_ready() {
            return Vec::new();
        }
        return vec![(
            "Enable PipeWire in your NixOS configuration".to_string(),
            NIXOS_PIPEWIRE_BLOCK.to_string(),
        )];
    }

    let mut out = Vec::new();
    let mut needs_restart = false;
    if !h.has_pw_dump {
        out.push((
            "Install the PipeWire tools (pw-dump)".to_string(),
            install(d, pkg_pw_tools(d)),
        ));
        needs_restart = true;
    }
    if !h.cpal_sees_pipewire {
        // THE Ubuntu no-list / no-playback bug: the ALSA->PipeWire bridge PCM.
        out.push((
            "Install the ALSA bridge so playback can reach PipeWire".to_string(),
            install(d, "pipewire-alsa"),
        ));
        needs_restart = true;
    }
    if !h.has_pactl {
        out.push((
            "Install the Pulse compatibility tools (optional fallback)".to_string(),
            install(d, pkg_pulse(d)),
        ));
        needs_restart = true;
    }
    if !h.any_devices {
        out.push((
            "No sinks detected — reinstall the ALSA UCM profiles, then reboot".to_string(),
            install_reinstall(d, "alsa-ucm-conf"),
        ));
    }
    // WirePlumber down, or we just installed something → (re)start the stack
    // with the ACTUAL init system running on this machine (not guessed from the
    // distro — Gentoo+systemd and Gentoo+OpenRC must differ).
    if !h.wireplumber_active || needs_restart {
        out.push((
            "(Re)start the PipeWire audio services".to_string(),
            restart_cmd(init).to_string(),
        ));
    }
    out
}

/// Init-system-aware "(re)start the audio services" command. PipeWire is a
/// user-session service, so only systemd has a first-class `--user` restart;
/// the others either use their user-service supervisor or a re-login.
fn restart_cmd(init: InitSystem) -> &'static str {
    match init {
        InitSystem::Systemd => "systemctl --user restart pipewire pipewire-pulse wireplumber",
        InitSystem::OpenRc => {
            "# OpenRC: PipeWire runs in your user session, not as an OpenRC service.\n\
             # Log out and back in to restart it."
        }
        InitSystem::Runit => {
            "sv restart pipewire wireplumber   # if set up as runit user services; otherwise log out and back in"
        }
        InitSystem::S6 => "# s6: restart via your supervision tree, or log out and back in",
        InitSystem::Dinit => "dinitctl restart pipewire wireplumber   # or log out and back in",
        InitSystem::Unknown => "# Restart PipeWire via your init system, or log out and back in",
    }
}

fn pkg_pw_tools(d: Distro) -> &'static str {
    match d {
        // Debian-family (incl. antiX) ship pw-* in pipewire-bin.
        Distro::Debian | Distro::Antix => "pipewire-bin",
        Distro::Fedora => "pipewire-utils",
        // Arch (incl. Artix) / openSUSE / Gentoo / Void ship pw-* with pipewire.
        _ => "pipewire",
    }
}

fn pkg_pulse(d: Distro) -> &'static str {
    match d {
        Distro::Debian | Distro::Antix => "pipewire-pulse pulseaudio-utils",
        Distro::Fedora => "pipewire-pulseaudio",
        _ => "pipewire-pulse",
    }
}

fn install(d: Distro, pkgs: &str) -> String {
    match d {
        // Package manager is a property of the distro family, NOT the init.
        Distro::Debian | Distro::Antix => format!("sudo apt install {pkgs}"),
        Distro::Fedora => format!("sudo dnf install {pkgs}"),
        Distro::Arch | Distro::Artix => format!("sudo pacman -S {pkgs}"),
        Distro::OpenSuse => format!("sudo zypper install {pkgs}"),
        Distro::Gentoo => format!("sudo emerge {pkgs}   # package name may differ on Gentoo"),
        Distro::Void => format!("sudo xbps-install -S {pkgs}"),
        // NixOS is special-cased in remediations(); this is an unreached fallback.
        Distro::NixOS => format!("# NixOS: add to configuration.nix (see the PipeWire block) — {pkgs}"),
        Distro::Other => format!("Install with your package manager: {pkgs}"),
    }
}

fn install_reinstall(d: Distro, pkg: &str) -> String {
    match d {
        Distro::Debian | Distro::Antix => format!("sudo apt install --reinstall {pkg}"),
        Distro::Fedora => format!("sudo dnf reinstall {pkg}"),
        _ => install(d, pkg),
    }
}

const NIXOS_PIPEWIRE_BLOCK: &str = "# /etc/nixos/configuration.nix:\n\
     services.pipewire = {\n\
     \u{20}\u{20}enable = true;\n\
     \u{20}\u{20}alsa.enable = true;\n\
     \u{20}\u{20}pulse.enable = true;\n\
     \u{20}\u{20}wireplumber.enable = true;\n\
     };\n\
     # then apply:\n\
     sudo nixos-rebuild switch";

/// Full reference setup commands for the chosen distro/init, shown when QBZ
/// can't probe the host (sandbox). Mirrors the Tauri DistroSelector, which
/// always showed per-distro install + restart commands (no probing).
fn reference_commands(d: Distro, init: InitSystem) -> Vec<(String, String)> {
    if d == Distro::NixOS {
        return vec![(
            "Enable PipeWire in your NixOS configuration".to_string(),
            NIXOS_PIPEWIRE_BLOCK.to_string(),
        )];
    }
    vec![
        (
            "Install the PipeWire audio stack".to_string(),
            install(d, full_stack_pkgs(d)),
        ),
        (
            "(Re)start the PipeWire audio services".to_string(),
            restart_cmd(init).to_string(),
        ),
    ]
}

/// The full recommended package set (incl. `pipewire-alsa`, the bit the old
/// Tauri list omitted — the cause of the Ubuntu empty-list bug).
fn full_stack_pkgs(d: Distro) -> &'static str {
    match d {
        Distro::Debian | Distro::Antix => {
            "pipewire pipewire-pulse pipewire-alsa wireplumber alsa-utils"
        }
        Distro::Fedora => "pipewire pipewire-pulseaudio pipewire-alsa wireplumber alsa-utils",
        Distro::Arch | Distro::Artix => {
            "pipewire pipewire-pulse pipewire-alsa wireplumber alsa-utils"
        }
        Distro::OpenSuse => "pipewire pipewire-pulseaudio pipewire-alsa wireplumber alsa-utils",
        Distro::Gentoo => "media-video/pipewire media-video/wireplumber media-sound/alsa-utils",
        Distro::Void => "pipewire wireplumber alsa-utils",
        Distro::NixOS => "",
        Distro::Other => "pipewire pipewire-pulse wireplumber alsa-utils",
    }
}

// ── Slice 7: select-dacs (auto-detect + manual escape hatch) ───────────────

/// Plain, `Send` candidate produced on the worker thread.
pub struct DacCandidateData {
    id: String,
    description: String,
    bus: String,
    is_default: bool,
    looks_like_dac: bool,
    rates_label: String,
}

/// Immediate UI feedback before the (blocking) enumeration runs.
pub fn begin_detect(window: &AppWindow) {
    window.global::<DacWizardState>().set_detecting(true);
}

/// Heavy work (enumerate sinks via the pw-dump-robust path + probe rates for
/// the likely DACs). Runs on a blocking thread; returns plain data.
pub fn detect_blocking() -> Vec<DacCandidateData> {
    let devices = qbz_audio::backend::BackendManager::create_backend(
        qbz_audio::backend::AudioBackendType::PipeWire,
    )
    .and_then(|b| b.enumerate_devices())
    .unwrap_or_default();

    let mut out = Vec::new();
    for d in devices {
        let bus = d.device_bus.unwrap_or_default();
        let looks_like_dac = d.is_hardware && (bus == "usb" || bus == "pci");
        // Only probe rates for likely DACs (skip virtual/monitor sinks).
        let rates_label = if looks_like_dac {
            format_rates(&qbz_audio::query_dac_capabilities(&d.id).sample_rates)
        } else {
            String::new()
        };
        let description = if d.name.is_empty() { d.id.clone() } else { d.name };
        out.push(DacCandidateData {
            id: d.id,
            description,
            bus,
            is_default: d.is_default,
            looks_like_dac,
            rates_label,
        });
    }
    out
}

/// Apply enumerated candidates to the state. Pre-selects the likely DACs; if
/// nothing enumerated, flips `has-enumeration` off so the manual escape hatch
/// shows.
pub fn apply_candidates(window: &AppWindow, data: Vec<DacCandidateData>) {
    let st = window.global::<DacWizardState>();
    let rows: Vec<DacCandidateRow> = data
        .iter()
        .map(|d| DacCandidateRow {
            id: d.id.clone().into(),
            description: d.description.clone().into(),
            bus: d.bus.clone().into(),
            is_default: d.is_default,
            looks_like_dac: d.looks_like_dac,
            checked: d.looks_like_dac,
            rates_label: d.rates_label.clone().into(),
        })
        .collect();
    let any = rows.iter().any(|r| r.checked);
    st.set_has_enumeration(!data.is_empty());
    st.set_any_dac_selected(any);
    st.set_candidates(ModelRc::new(VecModel::from(rows)));
    st.set_detecting(false);
}

/// Flip one candidate's checkbox + recompute the Next gate.
pub fn toggle_dac(window: &AppWindow, index: i32) {
    let st = window.global::<DacWizardState>();
    let model = st.get_candidates();
    if let Some(vm) = model
        .as_any()
        .downcast_ref::<VecModel<DacCandidateRow>>()
    {
        if let Some(mut row) = vm.row_data(index.max(0) as usize) {
            row.checked = !row.checked;
            vm.set_row_data(index.max(0) as usize, row);
        }
    }
    let any = (0..model.row_count()).any(|i| model.row_data(i).map(|r| r.checked).unwrap_or(false));
    st.set_any_dac_selected(any);
}

/// Validate a manually-pasted node.name (escape hatch). 1:1 with the Tauri
/// `validateNodeName` / `detectDacType`.
pub fn validate_manual(window: &AppWindow, text: &str) {
    let st = window.global::<DacWizardState>();
    st.set_manual_node_name(text.into());
    st.set_manual_valid(validate_node_name(text));
    st.set_manual_dac_type(detect_dac_type(text).into());
}

fn validate_node_name(name: &str) -> bool {
    let t = name.trim();
    !t.is_empty() && (t.contains("alsa_output") || t.contains("alsa_input"))
}

fn detect_dac_type(name: &str) -> &'static str {
    let l = name.to_lowercase();
    if l.contains("usb-") || l.contains(".usb") {
        "usb"
    } else if l.contains("pci-") || l.contains(".pci") {
        "pci"
    } else if l.contains("bluez") || l.contains("bluetooth") {
        "bluetooth"
    } else if l.contains("virtual") || l.contains("null") || l.contains("dummy") {
        "virtual"
    } else {
        "unknown"
    }
}

/// "44.1 / 96 / 192 kHz" from a rate list (kHz, .1 only when non-integer).
fn format_rates(rates: &[u32]) -> String {
    if rates.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = rates
        .iter()
        .map(|&r| {
            if r % 1000 == 0 {
                format!("{}", r / 1000)
            } else {
                format!("{:.1}", r as f64 / 1000.0)
            }
        })
        .collect();
    format!("{} kHz", parts.join(" / "))
}

#[cfg(test)]
mod slice7_tests {
    use super::*;

    #[test]
    fn validates_node_names_like_tauri() {
        assert!(validate_node_name("alsa_output.usb-Cambridge-00.analog-stereo"));
        assert!(validate_node_name("alsa_input.pci-0000_00.analog-stereo"));
        assert!(!validate_node_name(""));
        assert!(!validate_node_name("   "));
        assert!(!validate_node_name("bluez_output.AA_BB"));
    }

    #[test]
    fn detects_dac_type() {
        assert_eq!(detect_dac_type("alsa_output.usb-Cambridge-00.analog-stereo"), "usb");
        assert_eq!(detect_dac_type("alsa_output.pci-0000_00_1f.3.analog-stereo"), "pci");
        assert_eq!(detect_dac_type("bluez_output.AA"), "bluetooth");
        assert_eq!(detect_dac_type("alsa_output.virtual-dummy"), "virtual");
        assert_eq!(detect_dac_type("something.else"), "unknown");
    }

    #[test]
    fn formats_rates_khz() {
        assert_eq!(format_rates(&[44100, 96000, 192000]), "44.1 / 96 / 192 kHz");
        assert_eq!(format_rates(&[]), "");
    }
}

// ── Slice 9: self-service playback test (N6 read-back) ─────────────────────

/// One curated test track (owner-provided). Resolved by id-hint first, then by
/// "artist title" search if the id 404s (a pulled license) — never raw-id-only.
pub struct TestSeed {
    pub depth: u32,
    pub rate: f64,
    pub id_hint: u64,
    pub artist: &'static str,
    pub title: &'static str,
}

pub const TEST_SEEDS: [TestSeed; 4] = [
    TestSeed { depth: 16, rate: 44100.0, id_hint: 19301386, artist: "George Harrison", title: "My Sweet Lord" },
    TestSeed { depth: 24, rate: 44100.0, id_hint: 266725027, artist: "Billie Eilish", title: "LUNCH" },
    TestSeed { depth: 24, rate: 96000.0, id_hint: 126886854, artist: "Iron Maiden", title: "Stratego" },
    TestSeed { depth: 24, rate: 192000.0, id_hint: 52265, artist: "Toto", title: "Africa" },
];

/// The DAC node.name the test is currently watching (N6 probe target).
static TEST_NODE: Mutex<Option<String>> = Mutex::new(None);

/// True if a resolved track matches this seed's family (rate + bit depth — the
/// two 44.1 seeds only differ by depth).
pub fn track_matches_seed(track: &qbz_models::Track, seed: &TestSeed) -> bool {
    let rate_ok = track
        .maximum_sampling_rate
        .map(|r| (r * 1000.0 - seed.rate).abs() < 1.0 || (r - seed.rate).abs() < 1.0)
        .unwrap_or(false);
    let depth_ok = track.maximum_bit_depth.map(|d| d == seed.depth).unwrap_or(false);
    rate_ok && depth_ok
}

/// The node.name of the first checked candidate (the DAC being configured), or
/// the manual node.name from the escape hatch.
pub fn first_selected_node(window: &AppWindow) -> Option<String> {
    let st = window.global::<DacWizardState>();
    let model = st.get_candidates();
    for i in 0..model.row_count() {
        if let Some(r) = model.row_data(i) {
            if r.checked {
                return Some(r.id.to_string());
            }
        }
    }
    let manual = st.get_manual_node_name().to_string();
    if !manual.trim().is_empty() && st.get_manual_valid() {
        Some(manual)
    } else {
        None
    }
}

/// Start the test: stash the watched node + show the "playing" state.
pub fn begin_test(window: &AppWindow, node: Option<String>) {
    *TEST_NODE.lock().unwrap() = node;
    let st = window.global::<DacWizardState>();
    st.set_test_playing(true);
    st.set_test_rate_matched(false);
    st.set_test_requested_label("Starting…".into());
    st.set_test_negotiated_label("".into());
}

pub fn end_test(window: &AppWindow) {
    window.global::<DacWizardState>().set_test_playing(false);
}

/// The node the poll loop should probe (read on a worker thread).
pub fn test_node() -> Option<String> {
    TEST_NODE.lock().unwrap().clone()
}

/// Apply one poll: the rate QBZ requested vs the DAC's real negotiated rate (N6).
pub fn apply_poll(
    window: &AppWindow,
    requested_rate: u32,
    requested_bits: u32,
    negotiated: Option<qbz_audio::NegotiatedRate>,
) {
    let st = window.global::<DacWizardState>();
    st.set_test_requested_label(if requested_rate > 0 {
        format!(
            "QBZ requesting {} · {}-bit",
            khz(requested_rate),
            requested_bits
        )
        .into()
    } else {
        "Nothing playing".into()
    });
    match negotiated {
        Some(n) => {
            st.set_test_negotiated_label(format!("DAC running at {}", khz(n.sample_rate)).into());
            // Truth signal (N6): the DAC's real clock matches what QBZ asked for.
            st.set_test_rate_matched(requested_rate > 0 && n.sample_rate == requested_rate);
        }
        None => {
            st.set_test_negotiated_label("DAC idle — set QBZ output to this DAC".into());
            st.set_test_rate_matched(false);
        }
    }
}

fn khz(hz: u32) -> String {
    if hz % 1000 == 0 {
        format!("{} kHz", hz / 1000)
    } else {
        format!("{:.1} kHz", hz as f64 / 1000.0)
    }
}
