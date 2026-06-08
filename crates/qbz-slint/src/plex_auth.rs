//! Settings > Plex — connection + library-selection controller (Slint).
//!
//! A faithful 1:1 port of the Tauri (`SettingsView.svelte`) Plex auth/settings
//! feature. The persisted store lives in the frontend-agnostic
//! `qbz_app::settings::plex` crate (bound per-user via `crate::plex_settings`),
//! and all Plex HTTP/cache calls go through the `qbz_plex` core crate. This
//! module owns:
//!   - the caller-side URL resolver (`resolve_base_url`) + LAN gate
//!     (`is_local_address`) — the backend has none,
//!   - the 2500 ms PIN poll loop (no timeout / max attempts; abortable),
//!   - `machine_id` capture from ping,
//!   - `run_auto_setup` (ping -> sections -> sync) and
//!     `sync_selected_libraries` (cache_clear -> save_sections -> per-section
//!     fetch+save -> reload),
//!   - seeding the UI from the store + warm-loading the cache on panel open.
//!
//! Every UI write from a background task hops back via the weak-handle
//! `upgrade_in_event_loop` pattern (see `local_library_settings`). The PIN
//! poll uses a thread-local `slint::Timer` started from the event-loop thread.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use slint::{ComponentHandle, Model, Timer, TimerMode, Weak};

use crate::plex_settings;
use crate::{AppWindow, PlexSectionItem, PlexSettingsState, SettingsState};

// ----------------------------------------------------------------------------
// Status helper — Slint uses inline @tr, so we resolve the (key,args) status
// to a plain English label Rust-side (no 5-file JSON sync). `kind`: 0 none,
// 1 info, 2 connected/ok (highlight green), 3 error (highlight red).
// ----------------------------------------------------------------------------

fn set_status(weak: &Weak<AppWindow>, text: String, kind: i32) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<PlexSettingsState>();
        s.set_status_text(text.into());
        s.set_status_kind(kind);
    });
}

// ----------------------------------------------------------------------------
// Caller-side URL helpers (ported verbatim from the Svelte originals).
// ----------------------------------------------------------------------------

/// `normalizePlexServerUrl`: trim + ensure an http(s) scheme.
fn normalize_server_url(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

/// Split a normalized URL into (scheme, host-without-port, port-or-None).
/// Minimal parser sufficient for `http(s)://host[:port][/...]`.
fn parse_url(normalized: &str) -> Option<(String, String, Option<String>)> {
    let (scheme, rest) = if let Some(r) = normalized.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = normalized.strip_prefix("https://") {
        ("https", r)
    } else {
        return None;
    };
    // Authority ends at the first '/', '?' or '#'.
    let authority_end = rest
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return None;
    }
    // Strip userinfo if present.
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = if let Some(idx) = authority.rfind(':') {
        // Guard against IPv6 (we don't parse bracketed hosts here; Plex LAN
        // addresses are host names or IPv4). If the part after ':' is numeric,
        // treat it as a port.
        let maybe_port = &authority[idx + 1..];
        if !maybe_port.is_empty() && maybe_port.chars().all(|c| c.is_ascii_digit()) {
            (authority[..idx].to_string(), Some(maybe_port.to_string()))
        } else {
            (authority.to_string(), None)
        }
    } else {
        (authority.to_string(), None)
    };
    if host.is_empty() {
        return None;
    }
    Some((scheme.to_string(), host, port))
}

/// `isPrivateIpv4`.
fn is_private_ipv4(host: &str) -> bool {
    let octets: Vec<&str> = host.split('.').collect();
    if octets.len() != 4 {
        return false;
    }
    let parsed: Option<Vec<u32>> = octets
        .iter()
        .map(|o| o.parse::<u32>().ok().filter(|v| *v <= 255))
        .collect();
    let Some(o) = parsed else {
        return false;
    };
    if o[0] == 10 {
        return true;
    }
    if o[0] == 127 {
        return true;
    }
    if o[0] == 192 && o[1] == 168 {
        return true;
    }
    if o[0] == 172 && (16..=31).contains(&o[1]) {
        return true;
    }
    false
}

/// `isLocalPlexAddress`.
fn is_local_address(url_input: &str) -> bool {
    let normalized = normalize_server_url(url_input);
    if normalized.is_empty() {
        return false;
    }
    let Some((_, host, _)) = parse_url(&normalized) else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    if host == "localhost" || host == "::1" {
        return true;
    }
    if host.ends_with(".local") || host.ends_with(".lan") {
        return true;
    }
    if !host.contains('.') {
        return true;
    }
    is_private_ipv4(&host)
}

/// `resolvePlexBaseUrl`: normalize, validate scheme, default port 32400,
/// return `proto://host:port`. Empty on failure.
fn resolve_base_url(server_url: &str) -> String {
    let normalized = normalize_server_url(server_url);
    if normalized.is_empty() {
        return String::new();
    }
    let Some((scheme, host, port)) = parse_url(&normalized) else {
        return String::new();
    };
    if scheme != "http" && scheme != "https" {
        return String::new();
    }
    let port = port.unwrap_or_else(|| "32400".to_string());
    format!("{scheme}://{host}:{port}")
}

/// Host-form (`proto://host`) for seeding the input from a stored base_url.
fn host_form_from_base(base_url: &str) -> String {
    let src = if base_url.is_empty() {
        "http://127.0.0.1:32400"
    } else {
        base_url
    };
    match parse_url(src) {
        Some((scheme, host, _)) => format!("{scheme}://{host}"),
        None => "http://127.0.0.1".to_string(),
    }
}

/// `canUsePlexRequests`: enabled && local && base!="" && token!="".
fn can_use(enabled: bool, server_url: &str, token: &str) -> bool {
    enabled
        && is_local_address(server_url)
        && !resolve_base_url(server_url).is_empty()
        && !token.trim().is_empty()
}

// ----------------------------------------------------------------------------
// Persist + resolve the connection config (mirrors `persistPlexConfig`): the
// resolved base_url + the token. Returns the resolved base_url.
// ----------------------------------------------------------------------------

fn persist_config(server_url: &str, token: &str) -> String {
    let base = resolve_base_url(server_url);
    plex_settings::set_credentials(&base, token.trim());
    base
}

/// Recompute + push the derived `is-local-address`, `base-url`, `can-use`
/// gates onto the UI from the current state. Runs on the event loop.
fn refresh_gates(w: &AppWindow) {
    let s = w.global::<PlexSettingsState>();
    let server_url = s.get_server_url().to_string();
    let enabled = s.get_enabled();
    let token = s.get_token().to_string();
    let local = is_local_address(&server_url);
    let base = resolve_base_url(&server_url);
    s.set_is_local_address(local);
    s.set_base_url(base.clone().into());
    s.set_can_use(enabled && local && !base.is_empty() && !token.trim().is_empty());
}

// ----------------------------------------------------------------------------
// PIN poll lifecycle. The Timer must be started from the event-loop thread.
// ----------------------------------------------------------------------------

thread_local! {
    /// One reusable poll timer (event-loop thread). Lifecycle: started by
    /// `generate_code`, stopped on authorized / expired / disconnect.
    static PIN_POLL: Timer = Timer::default();
}

/// Bumped whenever a poll is (re)started so a stale in-flight async check
/// landing after a newer start/stop is ignored.
static PIN_GEN: AtomicU64 = AtomicU64::new(0);

/// Set once the first background auto-setup has run this session. Re-entering
/// the Local Library settings panel must NOT re-fire the heavy
/// `cache_clear` + full refetch on every section switch (the Svelte original
/// refreshes once per Settings open, not once per section). Reset by
/// `enable_toggle(true)` so re-enabling Plex forces a fresh refresh.
static BG_REFRESH_DONE: AtomicBool = AtomicBool::new(false);

fn stop_pin_poll() {
    PIN_GEN.fetch_add(1, Ordering::SeqCst);
    PIN_POLL.with(|t| t.stop());
}

// ----------------------------------------------------------------------------
// Callback handlers (bound in main.rs).
// ----------------------------------------------------------------------------

/// Panel init: seed the UI from the store, then (if enabled + usable) warm the
/// cached sections/tracks and kick a background refresh. Mirrors the Svelte
/// `loadPlexCachedState` + `refreshPlexInBackground`.
pub fn load(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let cfg = plex_settings::get();
    let server_host = host_form_from_base(&cfg.base_url);
    let selected = cfg.selected_section_keys.clone();
    // Values needed after `cfg` is moved into the seeding closure.
    let enabled = cfg.enabled;
    let token = cfg.token.clone();
    let server_url = host_form_from_base(&cfg.base_url);
    let weak2 = weak.clone();
    let server_host_for_ui = server_host.clone();
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<PlexSettingsState>();
        s.set_enabled(cfg.enabled);
        s.set_ui_collapsed(cfg.ui_collapsed);
        s.set_metadata_write_enabled(cfg.metadata_write_enabled);
        s.set_server_url(server_host_for_ui.into());
        s.set_base_url(cfg.base_url.clone().into());
        s.set_token(cfg.token.clone().into());
        s.set_manual_token_mode(cfg.manual_token_mode);
        s.set_token_input(cfg.token.clone().into());
        // Reset transient PIN/status on (re)mount.
        s.set_pin_busy(false);
        s.set_pin_code("".into());
        s.set_pin_auth_url("".into());
        s.set_busy(false);
        refresh_gates(&w);
    });

    if !enabled {
        return;
    }

    // Warm cached sections + tracks off the event loop.
    let handle_spawn = handle.clone();
    handle_spawn.spawn(async move {
        let sections = tokio::task::spawn_blocking(qbz_plex::plex_cache_get_sections)
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
        let counts = section_counts(&sections);
        let items = build_section_items(&sections, &selected, &counts);
        let cached_tracks = tokio::task::spawn_blocking(|| qbz_plex::plex_cache_get_tracks(None, Some(100_000)))
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
        let track_count = cached_tracks.len();
        let weak3 = weak2.clone();
        let _ = weak2.upgrade_in_event_loop(move |w| {
            let s = w.global::<PlexSettingsState>();
            if !items.is_empty() {
                s.set_sections(items_model(items));
            }
        });
        if track_count > 0 {
            set_status(
                &weak3,
                format!("Loaded {track_count} cached tracks"),
                1,
            );
        }

        // Background refresh if currently usable — ONCE per session (not on
        // every panel mount; the panel is conditionally mounted per settings
        // section, so an unguarded refresh would cache_clear+refetch on every
        // entry into Settings > Local Library).
        if can_use(enabled, &server_url, &token)
            && !BG_REFRESH_DONE.swap(true, Ordering::SeqCst)
        {
            run_auto_setup(weak3, handle, server_url, token).await;
        }
    });
}

pub fn enable_toggle(weak: Weak<AppWindow>, handle: tokio::runtime::Handle, enabled: bool) {
    plex_settings::set_enabled(enabled);
    let weak2 = weak.clone();
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.global::<PlexSettingsState>().set_enabled(enabled);
        refresh_gates(&w);
    });
    if enabled {
        set_status(&weak2, "Idle".to_string(), 1);
        // Re-enabling forces one fresh background refresh.
        BG_REFRESH_DONE.store(false, Ordering::SeqCst);
        // Warm + refresh like the panel init.
        load(weak2, handle);
    } else {
        stop_pin_poll();
        set_status(&weak2, "Disabled".to_string(), 0);
    }
}

pub fn collapse_toggle(collapsed: bool) {
    plex_settings::set_ui_collapsed(collapsed);
}

pub fn metadata_write_toggle(enabled: bool) {
    plex_settings::set_metadata_write_enabled(enabled);
}

/// Server-address field accepted: resolve + persist the base_url, refresh gates.
pub fn set_server_url(weak: Weak<AppWindow>, server_url: String) {
    let base = resolve_base_url(&server_url);
    plex_settings::set_base_url(&base);
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<PlexSettingsState>();
        s.set_server_url(server_url.into());
        s.set_base_url(base.into());
        refresh_gates(&w);
    });
}

/// Manual-token toggle: persist mode. Hides/show affordances via the bound flag.
pub fn manual_token_toggle(enabled: bool) {
    plex_settings::set_manual_token_mode(enabled);
}

/// Save a manually-entered token, then auto-setup if usable. Mirrors
/// `handlePlexTokenBlur` + manual mode persist.
pub fn set_token(weak: Weak<AppWindow>, handle: tokio::runtime::Handle, token: String) {
    plex_settings::set_manual_token_mode(true);
    let server_url = read_server_url(&weak);
    let base = persist_config(&server_url, &token);
    plex_settings::set_token(token.trim());
    let token_clean = token.trim().to_string();
    let weak2 = weak.clone();
    let _ = weak.upgrade_in_event_loop({
        let token_clean = token_clean.clone();
        move |w| {
            let s = w.global::<PlexSettingsState>();
            s.set_token(token_clean.into());
            s.set_manual_token_mode(true);
            refresh_gates(&w);
        }
    });
    let enabled = plex_settings::get().enabled;
    if can_use(enabled, &server_url, &token_clean) {
        let _ = base;
        let handle2 = handle.clone();
        handle.spawn(async move {
            run_auto_setup(weak2, handle2, server_url, token_clean).await;
        });
    }
}

/// Read the current server-url field synchronously (best-effort).
fn read_server_url(weak: &Weak<AppWindow>) -> String {
    weak.upgrade()
        .map(|w| w.global::<PlexSettingsState>().get_server_url().to_string())
        .unwrap_or_default()
}

/// `handlePlexConnectEasy`: PIN start + begin the 2500 ms poll.
pub fn generate_code(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let cfg = plex_settings::get();
    let server_url = read_server_url(&weak);
    if !cfg.enabled || !is_local_address(&server_url) || resolve_base_url(&server_url).is_empty() {
        return;
    }
    // Persist config (base_url) up front, like the Svelte original.
    let token_now = cfg.token.clone();
    let _ = persist_config(&server_url, &token_now);

    let _ = weak.upgrade_in_event_loop(|w| {
        let s = w.global::<PlexSettingsState>();
        s.set_pin_busy(true);
    });

    let client_id = plex_settings::get_or_create_client_id();
    let weak2 = weak.clone();
    let handle2 = handle.clone();
    // Capture the resolved server_url NOW (event-loop thread) and thread it
    // through the poll — the authorized branch runs on a tokio thread where
    // `weak.upgrade()` is None, so it must not re-read the field.
    let server_url_poll = server_url.clone();
    handle.spawn(async move {
        match qbz_plex::plex_auth_pin_start(client_id.clone()).await {
            Ok(pin) => {
                let pin_id = pin.pin_id;
                let code = pin.code.clone();
                let auth_url = pin.auth_url.clone();
                let code_for_status = code.clone();
                let _ = weak2.upgrade_in_event_loop(move |w| {
                    let s = w.global::<PlexSettingsState>();
                    s.set_pin_busy(false);
                    s.set_pin_code(code.into());
                    s.set_pin_auth_url(auth_url.into());
                });
                set_status(
                    &weak2,
                    format!("Enter code {code_for_status} at the Plex sign-in page"),
                    1,
                );
                // Start the repeating poll on the event-loop thread.
                let weak_timer = weak2.clone();
                let handle_timer = handle2.clone();
                let _ = weak2.upgrade_in_event_loop(move |_w| {
                    begin_pin_poll(
                        weak_timer,
                        handle_timer,
                        client_id,
                        pin_id,
                        code_for_status,
                        server_url_poll,
                    );
                });
            }
            Err(e) => {
                let _ = weak2.upgrade_in_event_loop(|w| {
                    w.global::<PlexSettingsState>().set_pin_busy(false);
                });
                set_status(&weak2, format!("Error: {e}"), 3);
                crate::toast::error_weak(&weak2, "Plex link failed to start");
            }
        }
    });
}

/// Begin the 2500 ms repeating poll. MUST be called from the event-loop thread.
fn begin_pin_poll(
    weak: Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    client_id: String,
    pin_id: u64,
    code: String,
    server_url: String,
) {
    let my_gen = PIN_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    PIN_POLL.with(|t| {
        t.stop();
        let weak = weak.clone();
        t.start(TimerMode::Repeated, Duration::from_millis(2500), move || {
            // Drop this tick if a newer poll superseded us.
            if PIN_GEN.load(Ordering::SeqCst) != my_gen {
                return;
            }
            // Self-terminate if the user navigated away from Settings > Local
            // Library (section 4). The panel is conditionally mounted and Slint
            // has no unmount hook, so the poll stops itself instead of leaking a
            // 2500ms background hit against plex.tv. This tick runs on the
            // event-loop thread, so `upgrade()` is valid here.
            let on_panel = weak
                .upgrade()
                .map(|w| w.global::<SettingsState>().get_section() == 4)
                .unwrap_or(false);
            if !on_panel {
                stop_pin_poll();
                return;
            }
            let weak = weak.clone();
            let handle = handle.clone();
            let client_id = client_id.clone();
            let code = code.clone();
            let server_url = server_url.clone();
            handle.clone().spawn(async move {
                if PIN_GEN.load(Ordering::SeqCst) != my_gen {
                    return;
                }
                match qbz_plex::plex_auth_pin_check(client_id.clone(), pin_id, Some(code.clone()))
                    .await
                {
                    Ok(check) => {
                        if check.authorized {
                            if let Some(token) = check.auth_token {
                                // Stop ticking immediately.
                                let _ = weak.upgrade_in_event_loop(|_w| stop_pin_poll());
                                // `server_url` was captured on the event loop at
                                // code-generation time; do NOT re-read it here
                                // (tokio thread → upgrade() None → blank base_url).
                                let _ = persist_config(&server_url, &token);
                                plex_settings::set_token(token.trim());
                                plex_settings::set_manual_token_mode(false);
                                let token_ui = token.trim().to_string();
                                let _ = weak.upgrade_in_event_loop({
                                    let token_ui = token_ui.clone();
                                    move |w| {
                                        let s = w.global::<PlexSettingsState>();
                                        s.set_token(token_ui.into());
                                        s.set_manual_token_mode(false);
                                        s.set_pin_code("".into());
                                        s.set_pin_auth_url("".into());
                                        refresh_gates(&w);
                                    }
                                });
                                set_status(&weak, "Connected to Plex".to_string(), 2);
                                run_auto_setup(weak.clone(), handle.clone(), server_url, token_ui)
                                    .await;
                            }
                        } else if check.expired {
                            let _ = weak.upgrade_in_event_loop(|w| {
                                stop_pin_poll();
                                let s = w.global::<PlexSettingsState>();
                                s.set_pin_code("".into());
                                s.set_pin_auth_url("".into());
                            });
                            set_status(&weak, "Link code expired".to_string(), 3);
                        }
                        // else: still pending — keep polling.
                    }
                    Err(e) => {
                        // Transient: swallow + keep polling (matches Svelte).
                        log::warn!("[qbz-slint] Plex auth poll error: {e}");
                    }
                }
            });
        });
    });
}

pub fn open_auth_url(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let url = weak
        .upgrade()
        .map(|w| w.global::<PlexSettingsState>().get_pin_auth_url().to_string())
        .unwrap_or_default();
    if url.is_empty() {
        return;
    }
    handle.spawn(async move {
        if let Err(e) = qbz_plex::plex_open_auth_url(url).await {
            log::warn!("[qbz-slint] open Plex auth url failed: {e}");
        }
    });
}

pub fn copy_code(weak: Weak<AppWindow>) {
    let code = weak
        .upgrade()
        .map(|w| w.global::<PlexSettingsState>().get_pin_code().to_string())
        .unwrap_or_default();
    if code.is_empty() {
        return;
    }
    crate::share::copy_to_clipboard(code);
    crate::toast::success_weak(&weak, "Code copied");
}

/// `handlePlexPing`: capture machine_id + set connected status. Returns whether
/// the ping succeeded (used by auto-setup).
async fn ping_inner(
    weak: Weak<AppWindow>,
    base_url: String,
    token: String,
) -> bool {
    let _ = weak.upgrade_in_event_loop(|w| w.global::<PlexSettingsState>().set_busy(true));
    let result = qbz_plex::plex_ping(base_url.trim().to_string(), token.trim().to_string()).await;
    let ok = match result {
        Ok(info) => {
            let machine = info.machine_identifier.clone().unwrap_or_default();
            if !machine.is_empty() {
                plex_settings::set_machine_id(&machine);
            }
            let server = info
                .friendly_name
                .clone()
                .or(info.machine_identifier.clone())
                .unwrap_or_else(|| "Plex".to_string());
            let version = info.version.clone().unwrap_or_else(|| "?".to_string());
            set_status(&weak, format!("Connected to {server} (v{version})"), 2);
            true
        }
        Err(e) => {
            set_status(&weak, format!("Error: {e}"), 3);
            false
        }
    };
    let _ = weak.upgrade_in_event_loop(|w| w.global::<PlexSettingsState>().set_busy(false));
    ok
}

pub fn ping(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let cfg = plex_settings::get();
    let server_url = read_server_url(&weak);
    if !can_use(cfg.enabled, &server_url, &cfg.token) {
        return;
    }
    let base = persist_config(&server_url, &cfg.token);
    let token = cfg.token.clone();
    handle.spawn(async move {
        ping_inner(weak, base, token).await;
    });
}

/// Build the section UI items from sections + selection + counts.
fn build_section_items(
    sections: &[qbz_plex::PlexMusicSection],
    selected: &[String],
    counts: &std::collections::HashMap<String, i32>,
) -> Vec<PlexSectionItem> {
    sections
        .iter()
        .map(|s| PlexSectionItem {
            key: s.key.clone().into(),
            title: s.title.clone().into(),
            count: counts.get(&s.key).copied().unwrap_or(0),
            selected: selected.iter().any(|k| k == &s.key),
        })
        .collect()
}

fn items_model(items: Vec<PlexSectionItem>) -> slint::ModelRc<PlexSectionItem> {
    slint::ModelRc::new(slint::VecModel::from(items))
}

/// Per-section cached track counts (blocking cache reads).
fn section_counts(
    sections: &[qbz_plex::PlexMusicSection],
) -> std::collections::HashMap<String, i32> {
    let mut counts = std::collections::HashMap::new();
    for s in sections {
        let n = qbz_plex::plex_cache_get_tracks(Some(s.key.clone()), Some(100_000))
            .map(|t| t.len() as i32)
            .unwrap_or(0);
        counts.insert(s.key.clone(), n);
    }
    counts
}

/// `handlePlexLoadSections` (autoSyncSelected: true) — fetch sections, save,
/// default-select-ALL when none persisted, then sync.
async fn load_sections_inner(
    weak: Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    base_url: String,
    token: String,
) {
    let _ = weak.upgrade_in_event_loop(|w| w.global::<PlexSettingsState>().set_busy(true));
    let sections =
        match qbz_plex::plex_get_music_sections(base_url.trim().to_string(), token.trim().to_string())
            .await
        {
            Ok(s) => s,
            Err(e) => {
                set_status(&weak, format!("Error: {e}"), 3);
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<PlexSettingsState>().set_busy(false)
                });
                return;
            }
        };

    let machine_id = plex_settings::get().machine_id;
    let server_id = (!machine_id.is_empty()).then_some(machine_id.clone());
    let sections_for_save = sections.clone();
    let _ = tokio::task::spawn_blocking(move || {
        qbz_plex::plex_cache_save_sections(server_id, sections_for_save)
    })
    .await;

    // Default-select ALL when persisted selection is empty / stale.
    let available: std::collections::HashSet<String> =
        sections.iter().map(|s| s.key.clone()).collect();
    let persisted: Vec<String> = plex_settings::get()
        .selected_section_keys
        .into_iter()
        .filter(|k| available.contains(k))
        .collect();
    let selected: Vec<String> = if persisted.is_empty() {
        sections.iter().map(|s| s.key.clone()).collect()
    } else {
        persisted
    };
    plex_settings::set_selected_section_keys(&selected);

    let count = sections.len();
    set_status(&weak, format!("{count} libraries found"), 1);

    // Push the section model (counts come from the sync reload below).
    let counts = {
        let sections = sections.clone();
        tokio::task::spawn_blocking(move || section_counts(&sections))
            .await
            .unwrap_or_default()
    };
    let items = build_section_items(&sections, &selected, &counts);
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<PlexSettingsState>();
        s.set_sections(items_model(items));
        s.set_busy(false);
    });

    sync_selected_libraries(weak, handle, base_url, token, sections).await;
}

pub fn load_sections(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let cfg = plex_settings::get();
    let server_url = read_server_url(&weak);
    if !can_use(cfg.enabled, &server_url, &cfg.token) {
        return;
    }
    let base = persist_config(&server_url, &cfg.token);
    let token = cfg.token.clone();
    let handle2 = handle.clone();
    handle.spawn(async move {
        load_sections_inner(weak, handle2, base, token).await;
    });
}

/// `syncSelectedPlexLibraries`: cache_clear -> save_sections -> per selected
/// section get_section_tracks+save_tracks -> reload get_tracks(None).
async fn sync_selected_libraries(
    weak: Weak<AppWindow>,
    _handle: tokio::runtime::Handle,
    base_url: String,
    token: String,
    sections: Vec<qbz_plex::PlexMusicSection>,
) {
    let _ = weak.upgrade_in_event_loop(|w| w.global::<PlexSettingsState>().set_busy(true));

    let machine_id = plex_settings::get().machine_id;
    let server_id = (!machine_id.is_empty()).then_some(machine_id.clone());
    let selected = plex_settings::get().selected_section_keys;

    // cache_clear + re-save sections (blocking).
    {
        let sections = sections.clone();
        let server_id = server_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = qbz_plex::plex_cache_clear();
            if !sections.is_empty() {
                let _ = qbz_plex::plex_cache_save_sections(server_id, sections);
            }
        })
        .await;
    }

    if selected.is_empty() {
        // Nothing selected → clear the track view, status 0 tracks.
        let items = build_section_items(&sections, &selected, &std::collections::HashMap::new());
        let _ = weak.upgrade_in_event_loop(move |w| {
            let s = w.global::<PlexSettingsState>();
            s.set_sections(items_model(items));
            s.set_busy(false);
        });
        set_status(&weak, "0 tracks loaded".to_string(), 1);
        return;
    }

    let mut total = 0usize;
    let mut counts: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
    for key in &selected {
        let tracks = match qbz_plex::plex_get_section_tracks(
            base_url.trim().to_string(),
            token.trim().to_string(),
            key.clone(),
            None,
        )
        .await
        {
            Ok(t) => t,
            Err(e) => {
                log::warn!("[qbz-slint] Plex get_section_tracks({key}) failed: {e}");
                continue;
            }
        };
        let n = tracks.len();
        counts.insert(key.clone(), n as i32);
        total += n;
        let server_id = server_id.clone();
        let key2 = key.clone();
        let _ = tokio::task::spawn_blocking(move || {
            qbz_plex::plex_cache_save_tracks(server_id, key2, tracks)
        })
        .await;
    }

    // Reload all so downstream browse views see fresh cache.
    let _ = tokio::task::spawn_blocking(|| qbz_plex::plex_cache_get_tracks(None, Some(100_000))).await;

    let items = build_section_items(&sections, &selected, &counts);
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<PlexSettingsState>();
        s.set_sections(items_model(items));
        s.set_busy(false);
    });
    set_status(&weak, format!("{total} tracks loaded"), 2);
}

/// `runPlexAutoSetup`: ping → (on success) sections → sync.
async fn run_auto_setup(
    weak: Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    server_url: String,
    token: String,
) {
    let base = resolve_base_url(&server_url);
    if base.is_empty() {
        return;
    }
    if !ping_inner(weak.clone(), base.clone(), token.clone()).await {
        return;
    }
    load_sections_inner(weak, handle, base, token).await;
}

/// `handlePlexLibraryToggle`: flip membership, persist (+legacy mirror), then
/// immediate full re-sync.
pub fn toggle_section(weak: Weak<AppWindow>, handle: tokio::runtime::Handle, key: String) {
    let cfg = plex_settings::get();
    // Build the new selection ordered by the current section list.
    let server_url = read_server_url(&weak);
    let current_sections = read_sections(&weak);
    let mut set: std::collections::HashSet<String> =
        cfg.selected_section_keys.iter().cloned().collect();
    if set.contains(&key) {
        set.remove(&key);
    } else {
        set.insert(key.clone());
    }
    let ordered: Vec<String> = current_sections
        .iter()
        .map(|s| s.key.to_string())
        .filter(|k| set.contains(k))
        .collect();
    plex_settings::set_selected_section_keys(&ordered);

    // Reflect selection immediately in the UI.
    let ordered_for_ui = ordered.clone();
    let _ = weak.upgrade_in_event_loop(move |w| {
        let s = w.global::<PlexSettingsState>();
        let model = s.get_sections();
        let updated: Vec<PlexSectionItem> = (0..model.row_count())
            .filter_map(|i| slint::Model::row_data(&model, i))
            .map(|mut it| {
                it.selected = ordered_for_ui.iter().any(|k| k == &it.key.to_string());
                it
            })
            .collect();
        s.set_sections(items_model(updated));
    });

    if !can_use(cfg.enabled, &server_url, &cfg.token) {
        return;
    }
    let base = resolve_base_url(&server_url);
    let token = cfg.token.clone();
    let sections: Vec<qbz_plex::PlexMusicSection> = current_sections
        .iter()
        .map(|s| qbz_plex::PlexMusicSection {
            key: s.key.to_string(),
            title: s.title.to_string(),
        })
        .collect();
    let handle2 = handle.clone();
    handle.spawn(async move {
        sync_selected_libraries(weak, handle2, base, token, sections).await;
    });
}

/// Snapshot the current section model from the UI (best-effort).
fn read_sections(weak: &Weak<AppWindow>) -> Vec<PlexSectionItem> {
    weak.upgrade()
        .map(|w| {
            let model = w.global::<PlexSettingsState>().get_sections();
            (0..model.row_count())
                .filter_map(|i| slint::Model::row_data(&model, i))
                .collect()
        })
        .unwrap_or_default()
}

/// `handlePlexDisconnect`: confirm → reset creds/sections/machine_id + clear
/// cache. Keeps enabled/client_id/metadata_write.
pub fn disconnect(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        let ok = rfd::AsyncMessageDialog::new()
            .set_title("Disconnect from Plex?")
            .set_description(
                "This signs out of Plex and clears the locally cached libraries and tracks.",
            )
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            .await;
        if ok != rfd::MessageDialogResult::Yes {
            return;
        }
        // Stop any active poll on the event loop.
        let _ = weak.upgrade_in_event_loop(|_w| stop_pin_poll());
        plex_settings::disconnect();
        let _ = tokio::task::spawn_blocking(qbz_plex::plex_cache_clear).await;
        let _ = weak.upgrade_in_event_loop(|w| {
            let s = w.global::<PlexSettingsState>();
            s.set_token("".into());
            s.set_token_input("".into());
            s.set_manual_token_mode(false);
            s.set_pin_code("".into());
            s.set_pin_auth_url("".into());
            s.set_pin_busy(false);
            s.set_busy(false);
            s.set_sections(items_model(Vec::new()));
            refresh_gates(&w);
        });
        set_status(&weak, "Disconnected".to_string(), 1);
    });
}

/// `handlePlexClearCache`: confirm → plex_cache_clear only (token kept).
pub fn clear_cache(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        let ok = rfd::AsyncMessageDialog::new()
            .set_title("Clear Plex cache?")
            .set_description("This removes cached Plex libraries and tracks. Your sign-in is kept.")
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            .await;
        if ok != rfd::MessageDialogResult::Yes {
            return;
        }
        let cleared = tokio::task::spawn_blocking(qbz_plex::plex_cache_clear)
            .await
            .map(|r| r.is_ok())
            .unwrap_or(false);
        let _ = weak.upgrade_in_event_loop(|w| {
            // Zero out the cached counts (sections kept; tracks gone).
            let s = w.global::<PlexSettingsState>();
            let model = s.get_sections();
            let cleared_items: Vec<PlexSectionItem> = (0..model.row_count())
                .filter_map(|i| slint::Model::row_data(&model, i))
                .map(|mut it| {
                    it.count = 0;
                    it
                })
                .collect();
            s.set_sections(items_model(cleared_items));
        });
        if cleared {
            set_status(&weak, "Cache cleared".to_string(), 1);
        } else {
            crate::toast::error_weak(&weak, "Couldn't clear the Plex cache");
        }
    });
}

