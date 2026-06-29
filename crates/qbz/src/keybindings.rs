//! Keyboard shortcuts (hotkeys) — Rust port of the Tauri `keybindingsStore`.
//!
//! Mirrors the Tauri model 1:1: the same 26 actions, the same default
//! shortcuts, the same shortcut-string grammar, conflict detection, and user
//! overrides. The differences are mechanical:
//!
//! - Persistence is the per-machine `ui_prefs.json` (`keybindings` map) instead
//!   of `localStorage` (mirrors every other Slint appearance pref).
//! - Key events come from winit (`on_winit_window_event`) instead of a DOM
//!   `keydown` listener. The `isInputTarget` guard becomes `UiFocusState`.
//! - The two modals + the capture widget are Slint (`KeybindingsState` /
//!   `KeyboardShortcutsState`); this module owns the model + dispatch.
//!
//! Grammar (identical canonical strings to the TS `eventToShortcut`):
//! `[Ctrl+][Alt+][Shift+]Key`. `Ctrl` covers Ctrl OR Meta/Super. `Shift` is
//! only emitted for letters, digits, and named keys (Arrow*, Space, …) — for a
//! symbol the Shift is already "consumed" by producing the symbol (e.g. `?`).

use std::cell::Cell;
use std::collections::BTreeMap;

use i_slint_backend_winit::{
    winit::keyboard::{Key, NamedKey},
    EventResult,
};
use slint::ComponentHandle;

use crate::{
    AppWindow, ImmersiveActions, ImmersiveState, KeybindingsActions, KeybindingsState,
    KeyboardShortcutsState, LinkResolverState, NavState, NowPlayingState, SearchState, ShellState,
};

// ============================================================================
// Action model
// ============================================================================

/// Display/grouping category. Order here is the on-screen order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Playback,
    Navigation,
    Ui,
    Immersive,
    Mini,
}

impl Category {
    const ORDER: [Category; 5] = [
        Category::Playback,
        Category::Navigation,
        Category::Ui,
        Category::Immersive,
        Category::Mini,
    ];

    /// English source string for the localized category header.
    fn label_en(self) -> &'static str {
        match self {
            Category::Playback => "Playback",
            Category::Navigation => "Navigation",
            Category::Ui => "Interface",
            Category::Immersive => "Immersive",
            Category::Mini => "Mini Player",
        }
    }
}

/// When an action only fires in a specific surface.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Context {
    None,
    /// Only while the immersive overlay is open (seek shortcuts).
    Immersive,
    /// Only while the miniplayer window is active (surface 1-5). Dispatched on
    /// the mini window's own FocusScope, never on the main window.
    Mini,
}

pub struct ActionDef {
    pub id: &'static str,
    pub label_en: &'static str,
    pub category: Category,
    pub default: &'static str,
    pub context: Context,
}

/// The full action table — a 1:1 port of the Tauri `ACTIONS` array.
pub const ACTIONS: &[ActionDef] = &[
    // Playback
    ActionDef { id: "playback.toggle", label_en: "Play / Pause", category: Category::Playback, default: "Space", context: Context::None },
    ActionDef { id: "playback.next", label_en: "Next Track", category: Category::Playback, default: "Ctrl+ArrowRight", context: Context::None },
    ActionDef { id: "playback.prev", label_en: "Previous Track", category: Category::Playback, default: "Ctrl+ArrowLeft", context: Context::None },
    // Navigation
    ActionDef { id: "nav.back", label_en: "Go Back", category: Category::Navigation, default: "Alt+ArrowLeft", context: Context::None },
    ActionDef { id: "nav.forward", label_en: "Go Forward", category: Category::Navigation, default: "Alt+ArrowRight", context: Context::None },
    ActionDef { id: "nav.search", label_en: "Search", category: Category::Navigation, default: "Ctrl+f", context: Context::None },
    ActionDef { id: "nav.settings", label_en: "Settings", category: Category::Navigation, default: "Ctrl+,", context: Context::None },
    // Interface
    ActionDef { id: "ui.sidebar", label_en: "Toggle Sidebar", category: Category::Ui, default: "Shift+S", context: Context::None },
    ActionDef { id: "ui.focusMode", label_en: "Immersive Mode", category: Category::Ui, default: "Shift+I", context: Context::None },
    ActionDef { id: "ui.queue", label_en: "Queue", category: Category::Ui, default: "q", context: Context::None },
    ActionDef { id: "ui.escape", label_en: "Close / Dismiss", category: Category::Ui, default: "Escape", context: Context::None },
    ActionDef { id: "ui.showShortcuts", label_en: "Show Shortcuts", category: Category::Ui, default: "?", context: Context::None },
    ActionDef { id: "ui.openLink", label_en: "Open Qobuz Link", category: Category::Ui, default: "Ctrl+l", context: Context::None },
    ActionDef { id: "ui.miniPlayer", label_en: "Toggle Mini Player", category: Category::Ui, default: "Shift+M", context: Context::None },
    // Immersive (contextual)
    ActionDef { id: "focus.seekForward", label_en: "Seek Forward (5s)", category: Category::Immersive, default: "ArrowRight", context: Context::Immersive },
    ActionDef { id: "focus.seekBack", label_en: "Seek Back (5s)", category: Category::Immersive, default: "ArrowLeft", context: Context::Immersive },
    ActionDef { id: "focus.seekForwardLong", label_en: "Seek Forward (10s)", category: Category::Immersive, default: "Shift+ArrowRight", context: Context::Immersive },
    ActionDef { id: "focus.seekBackLong", label_en: "Seek Back (10s)", category: Category::Immersive, default: "Shift+ArrowLeft", context: Context::Immersive },
    // Mini Player (contextual — dispatched on the mini window)
    ActionDef { id: "mini.micro", label_en: "Micro View", category: Category::Mini, default: "1", context: Context::Mini },
    ActionDef { id: "mini.compact", label_en: "Compact View", category: Category::Mini, default: "2", context: Context::Mini },
    ActionDef { id: "mini.artwork", label_en: "Artwork View", category: Category::Mini, default: "3", context: Context::Mini },
    ActionDef { id: "mini.queue", label_en: "Queue View", category: Category::Mini, default: "4", context: Context::Mini },
    ActionDef { id: "mini.lyrics", label_en: "Lyrics View", category: Category::Mini, default: "5", context: Context::Mini },
];

fn action(id: &str) -> Option<&'static ActionDef> {
    ACTIONS.iter().find(|a| a.id == id)
}

// ============================================================================
// Modifier tracking (UI thread only)
// ============================================================================

thread_local! {
    static MODS: Cell<(bool, bool, bool)> = const { Cell::new((false, false, false)) };
}

/// Record the current modifier state from a winit `ModifiersChanged` event.
/// `ctrl` already folds in Meta/Super (mirrors the TS `ctrlKey || metaKey`).
pub fn set_mods(ctrl: bool, alt: bool, shift: bool) {
    MODS.with(|m| m.set((ctrl, alt, shift)));
}

/// Current modifier state `(ctrl, alt, shift)` as last reported by winit's
/// `ModifiersChanged`. `ctrl` already folds in Meta/Super. Read by the keyboard
/// dispatch AND by the multi-select toggle arm (to decide Shift-range vs single
/// toggle at click time — see `selection`).
pub fn mods() -> (bool, bool, bool) {
    MODS.with(|m| m.get())
}

// ============================================================================
// Shortcut-string grammar (port of eventToShortcut / formatShortcutDisplay)
// ============================================================================

/// Normalize a winit logical key to a canonical key token (the part after the
/// modifiers). Returns `None` for bare modifier presses and unrepresentable
/// keys. Letter/symbol casing is taken from winit verbatim (winit already
/// reports the shifted glyph, so Shift+s → "S", Shift+/ → "?").
pub fn token_from_key(key: &Key) -> Option<String> {
    match key {
        Key::Named(NamedKey::Space) => Some("Space".into()),
        Key::Named(NamedKey::ArrowLeft) => Some("ArrowLeft".into()),
        Key::Named(NamedKey::ArrowRight) => Some("ArrowRight".into()),
        Key::Named(NamedKey::ArrowUp) => Some("ArrowUp".into()),
        Key::Named(NamedKey::ArrowDown) => Some("ArrowDown".into()),
        Key::Named(NamedKey::Escape) => Some("Escape".into()),
        Key::Named(NamedKey::Enter) => Some("Enter".into()),
        Key::Named(NamedKey::Tab) => Some("Tab".into()),
        Key::Named(NamedKey::Backspace) => Some("Backspace".into()),
        Key::Named(NamedKey::Delete) => Some("Delete".into()),
        Key::Character(s) => {
            let t = s.as_str();
            if t.chars().count() != 1 {
                return None;
            }
            Some(t.to_string())
        }
        _ => None,
    }
}

/// Build the canonical shortcut string from modifiers + a key token.
pub fn shortcut_from_parts(ctrl: bool, alt: bool, shift: bool, token: &str) -> Option<String> {
    if token.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if ctrl {
        parts.push("Ctrl".into());
    }
    if alt {
        parts.push("Alt".into());
    }
    // Shift is emitted only for letters, digits, and named (multi-char) keys.
    let is_named = token.chars().count() > 1;
    let single = token.chars().next().unwrap();
    let is_letter = !is_named && single.is_ascii_alphabetic();
    let is_digit = !is_named && single.is_ascii_digit();
    if shift && (is_named || is_letter || is_digit) {
        parts.push("Shift".into());
    }
    parts.push(token.to_string());
    Some(parts.join("+"))
}

const KEY_DISPLAY: &[(&str, &str)] = &[
    ("ArrowLeft", "←"),
    ("ArrowRight", "→"),
    ("ArrowUp", "↑"),
    ("ArrowDown", "↓"),
    ("Space", "Space"),
    ("Escape", "Esc"),
    ("Enter", "↵"),
    ("Backspace", "⌫"),
    ("Delete", "Del"),
    ("Tab", "Tab"),
];

/// Format a shortcut string for display (port of `formatShortcutDisplay`).
/// macOS uses ⌘⌥⇧ glyphs joined by spaces; elsewhere "Ctrl + …".
pub fn format_display(shortcut: &str) -> String {
    if shortcut.is_empty() {
        return String::new();
    }
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    let mut key = "";
    for part in shortcut.split('+') {
        match part {
            "Ctrl" => ctrl = true,
            "Alt" => alt = true,
            "Shift" => shift = true,
            other => key = other,
        }
    }
    let mac = cfg!(target_os = "macos");
    let mut out: Vec<String> = Vec::new();
    if ctrl {
        out.push(if mac { "⌘" } else { "Ctrl" }.into());
    }
    if alt {
        out.push(if mac { "⌥" } else { "Alt" }.into());
    }
    if shift {
        out.push(if mac { "⇧" } else { "Shift" }.into());
    }
    let disp = KEY_DISPLAY
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| key.to_uppercase());
    out.push(disp);
    out.join(if mac { " " } else { " + " })
}

// ============================================================================
// Bindings (defaults + user overrides) + conflict detection
// ============================================================================

/// The active binding map (defaults overlaid with the user's overrides).
pub fn active_bindings() -> BTreeMap<String, String> {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for a in ACTIONS {
        map.insert(a.id.to_string(), a.default.to_string());
    }
    let prefs = crate::ui_prefs::load();
    for (id, shortcut) in prefs.keybindings {
        if map.contains_key(&id) {
            map.insert(id, shortcut);
        }
    }
    map
}

fn action_for_shortcut<'a>(
    shortcut: &str,
    bindings: &'a BTreeMap<String, String>,
) -> Option<&'static ActionDef> {
    let id = bindings
        .iter()
        .find(|(_, v)| v.as_str() == shortcut)
        .map(|(k, _)| k.clone())?;
    action(&id)
}

/// The action (other than `exclude`) that already owns `shortcut`, if any.
fn conflicting_action(
    shortcut: &str,
    exclude: &str,
    bindings: &BTreeMap<String, String>,
) -> Option<&'static ActionDef> {
    for (id, sc) in bindings {
        if sc == shortcut && id != exclude {
            return action(id);
        }
    }
    None
}

/// Persist a new binding. Returns false (and writes nothing) on a conflict.
fn set_binding(action_id: &str, shortcut: &str) -> bool {
    let bindings = active_bindings();
    if conflicting_action(shortcut, action_id, &bindings).is_some() {
        return false;
    }
    let default = action(action_id).map(|a| a.default);
    let mut prefs = crate::ui_prefs::load();
    if Some(shortcut) == default {
        // Back to default → drop the override (keeps the file minimal).
        prefs.keybindings.remove(action_id);
    } else {
        prefs.keybindings.insert(action_id.to_string(), shortcut.to_string());
    }
    crate::ui_prefs::save(&prefs);
    true
}

fn reset_one(action_id: &str) {
    let mut prefs = crate::ui_prefs::load();
    prefs.keybindings.remove(action_id);
    crate::ui_prefs::save(&prefs);
}

fn reset_all() {
    let mut prefs = crate::ui_prefs::load();
    prefs.keybindings.clear();
    crate::ui_prefs::save(&prefs);
}

// ============================================================================
// Slint model (cheatsheet + customize editor share the groups)
// ============================================================================

fn build_groups() -> slint::ModelRc<crate::KeybindingCategoryGroup> {
    use slint::{ModelRc, VecModel};
    let bindings = active_bindings();
    let mut groups: Vec<crate::KeybindingCategoryGroup> = Vec::new();
    for cat in Category::ORDER {
        let mut rows: Vec<crate::KeybindingRow> = Vec::new();
        for a in ACTIONS.iter().filter(|a| a.category == cat) {
            let shortcut = bindings.get(a.id).cloned().unwrap_or_default();
            let modified = bindings.get(a.id).map(|s| s.as_str()) != Some(a.default);
            rows.push(crate::KeybindingRow {
                id: a.id.into(),
                label: qbz_i18n::t(a.label_en).into(),
                shortcut: format_display(&shortcut).into(),
                modified,
                contextual: a.context != Context::None,
            });
        }
        groups.push(crate::KeybindingCategoryGroup {
            label: qbz_i18n::t(cat.label_en()).into(),
            rows: ModelRc::new(VecModel::from(rows)),
        });
    }
    ModelRc::new(VecModel::from(groups))
}

fn modified_count() -> i32 {
    let bindings = active_bindings();
    ACTIONS
        .iter()
        .filter(|a| bindings.get(a.id).map(|s| s.as_str()) != Some(a.default))
        .count() as i32
}

/// Repopulate the Slint state from the persisted bindings. Call at startup and
/// after any change.
pub fn refresh(window: &AppWindow) {
    let state = window.global::<KeybindingsState>();
    state.set_groups(build_groups());
    state.set_modified_count(modified_count());
}

// ============================================================================
// Callback wiring (KeybindingsActions) — called once at startup
// ============================================================================

pub fn wire(window: &AppWindow) {
    let actions = window.global::<KeybindingsActions>();

    let weak = window.as_weak();
    actions.on_start_record(move |id| {
        if let Some(w) = weak.upgrade() {
            let s = w.global::<KeybindingsState>();
            s.set_recording_id(id);
            s.set_pending_display("".into());
            s.set_conflict_label("".into());
        }
    });

    let weak = window.as_weak();
    actions.on_cancel_record(move || {
        if let Some(w) = weak.upgrade() {
            let s = w.global::<KeybindingsState>();
            s.set_recording_id("".into());
            s.set_pending_display("".into());
            s.set_conflict_label("".into());
        }
    });

    let weak = window.as_weak();
    actions.on_reset_one(move |id| {
        reset_one(id.as_str());
        if let Some(w) = weak.upgrade() {
            refresh(&w);
        }
    });

    let weak = window.as_weak();
    actions.on_reset_all(move || {
        reset_all();
        if let Some(w) = weak.upgrade() {
            refresh(&w);
        }
    });

    refresh(window);
}

// ============================================================================
// Capture (the customize editor's "press a key" widget)
// ============================================================================

/// Handle a keypress while the customize editor is recording a binding for
/// `action_id`. Always consumes the event.
pub fn handle_capture(window: &AppWindow, action_id: &str, key: &Key) -> EventResult {
    let state = window.global::<KeybindingsState>();

    // Escape cancels (does not bind — Escape stays the ui.escape default).
    if matches!(key, Key::Named(NamedKey::Escape)) {
        state.set_recording_id("".into());
        state.set_pending_display("".into());
        state.set_conflict_label("".into());
        return EventResult::PreventDefault;
    }

    let (ctrl, alt, shift) = mods();
    let Some(token) = token_from_key(key) else {
        // Bare modifier / unrepresentable — ignore, keep recording.
        return EventResult::PreventDefault;
    };
    let Some(shortcut) = shortcut_from_parts(ctrl, alt, shift, &token) else {
        return EventResult::PreventDefault;
    };

    state.set_pending_display(format_display(&shortcut).into());
    let bindings = active_bindings();
    if let Some(conflict) = conflicting_action(&shortcut, action_id, &bindings) {
        state.set_conflict_label(qbz_i18n::t(conflict.label_en).into());
        // Leave recording on so the user can pick a different combo.
    } else {
        set_binding(action_id, &shortcut);
        refresh(window);
        state.set_recording_id("".into());
        state.set_pending_display("".into());
        state.set_conflict_label("".into());
    }
    EventResult::PreventDefault
}

// ============================================================================
// Dispatch (the global hotkey handler for the MAIN window)
// ============================================================================

/// Resolve + run a hotkey for the main window. Returns `PreventDefault` when an
/// action fired, `Propagate` otherwise. The caller has already ruled out
/// recording mode, an open search dropdown, and text-input focus.
pub fn dispatch(window: &AppWindow, key: &Key) -> EventResult {
    let (ctrl, alt, shift) = mods();
    let Some(token) = token_from_key(key) else {
        return EventResult::Propagate;
    };
    let Some(shortcut) = shortcut_from_parts(ctrl, alt, shift, &token) else {
        return EventResult::Propagate;
    };
    let bindings = active_bindings();
    let Some(action) = action_for_shortcut(&shortcut, &bindings) else {
        return EventResult::Propagate;
    };
    match action.context {
        Context::Immersive => {
            if !window.global::<ImmersiveState>().get_open() {
                return EventResult::Propagate;
            }
        }
        // Mini surfaces are dispatched on the mini window's FocusScope.
        Context::Mini => return EventResult::Propagate,
        Context::None => {}
    }
    run_action(window, action.id);
    EventResult::PreventDefault
}

fn run_action(window: &AppWindow, id: &str) {
    match id {
        "playback.toggle" => window.global::<NowPlayingState>().invoke_toggle_play(),
        "playback.next" => window.global::<NowPlayingState>().invoke_next(),
        "playback.prev" => window.global::<NowPlayingState>().invoke_previous(),
        "nav.back" => window.global::<NavState>().invoke_request_back(),
        "nav.forward" => window.global::<NavState>().invoke_request_forward(),
        "nav.search" => focus_search(window),
        "nav.settings" => window.global::<NavState>().invoke_request_settings(),
        "ui.sidebar" => window.global::<ShellState>().invoke_cycle_sidebar(),
        "ui.focusMode" => toggle_immersive(window),
        "ui.queue" => {
            let shell = window.global::<ShellState>();
            let open = shell.get_queue_open();
            shell.set_queue_open(!open);
        }
        "ui.escape" => handle_escape(window),
        "ui.miniPlayer" => toggle_miniplayer(),
        "ui.openLink" => open_link_modal(window),
        "ui.showShortcuts" => {
            window.global::<KeyboardShortcutsState>().set_open(true);
            refresh(window);
        }
        "focus.seekForward" => seek_relative(window, 5),
        "focus.seekBack" => seek_relative(window, -5),
        "focus.seekForwardLong" => seek_relative(window, 10),
        "focus.seekBackLong" => seek_relative(window, -10),
        _ => {}
    }
}

fn focus_search(window: &AppWindow) {
    // Open the header cortinilla; the field grabs focus on open.
    window.global::<SearchState>().set_cortinilla_open(true);
}

fn toggle_immersive(window: &AppWindow) {
    let imm = window.global::<ImmersiveState>();
    let now = imm.get_open();
    imm.set_open(!now);
    if !now {
        window.global::<ImmersiveActions>().invoke_opened();
    }
}

fn toggle_miniplayer() {
    if crate::miniplayer::is_open() {
        crate::miniplayer::exit();
    } else {
        crate::miniplayer::enter();
    }
}

fn open_link_modal(window: &AppWindow) {
    let s = window.global::<LinkResolverState>();
    s.set_url("".into());
    s.set_platform("".into());
    s.set_error("".into());
    s.set_playlist_detected(false);
    s.set_playlist_provider("".into());
    s.set_resolving(false);
    s.set_open(true);
}

/// Seek by `delta` seconds (clamped) while immersive is open.
fn seek_relative(window: &AppWindow, delta: i32) {
    let np = window.global::<NowPlayingState>();
    let duration = np.get_duration_secs();
    if duration <= 0 {
        return;
    }
    let pos = np.get_position_secs();
    let target = (pos + delta).clamp(0, duration);
    np.invoke_seek(target as f32 / duration as f32);
}

/// Close the topmost dismissable surface. Text-input focus has already been
/// ruled out by the caller, so this only touches non-text overlays.
fn handle_escape(window: &AppWindow) {
    if window.global::<LinkResolverState>().get_open() {
        window.global::<LinkResolverState>().set_open(false);
        return;
    }
    if window.global::<KeyboardShortcutsState>().get_customize_open() {
        window.global::<KeyboardShortcutsState>().set_customize_open(false);
        return;
    }
    if window.global::<KeyboardShortcutsState>().get_open() {
        window.global::<KeyboardShortcutsState>().set_open(false);
        return;
    }
    if window.global::<SearchState>().get_cortinilla_open() {
        window.global::<SearchState>().set_cortinilla_open(false);
        return;
    }
    if window.global::<ImmersiveState>().get_open() {
        window.global::<ImmersiveState>().set_open(false);
        return;
    }
    // Leaving a multi-select session (clear + mode off) takes priority over
    // closing the queue. No-op when no surface is in select mode.
    if crate::exit_active_multi_select(window) {
        return;
    }
    let shell = window.global::<ShellState>();
    if shell.get_queue_open() {
        shell.set_queue_open(false);
    }
}

// ============================================================================
// Dispatch for the MINIPLAYER window (a separate winit window). Handles the
// contextual mini surface shortcuts (1-5), the toggle (Shift+M / Escape →
// exit), and shared transport. Respects the user's custom bindings.
// ============================================================================

pub fn dispatch_mini(window: &crate::MiniPlayerWindow, key: &Key) -> EventResult {
    let (ctrl, alt, shift) = mods();
    let Some(token) = token_from_key(key) else {
        return EventResult::Propagate;
    };
    let Some(shortcut) = shortcut_from_parts(ctrl, alt, shift, &token) else {
        return EventResult::Propagate;
    };
    let bindings = active_bindings();
    let Some(action) = action_for_shortcut(&shortcut, &bindings) else {
        return EventResult::Propagate;
    };
    match action.id {
        "mini.micro" => window.global::<crate::MiniPlayerState>().invoke_surface_change(0),
        "mini.compact" => window.global::<crate::MiniPlayerState>().invoke_surface_change(1),
        "mini.artwork" => window.global::<crate::MiniPlayerState>().invoke_surface_change(2),
        "mini.queue" => window.global::<crate::MiniPlayerState>().invoke_surface_change(3),
        "mini.lyrics" => window.global::<crate::MiniPlayerState>().invoke_surface_change(4),
        "ui.miniPlayer" | "ui.escape" => crate::miniplayer::exit(),
        "playback.toggle" => window.global::<NowPlayingState>().invoke_toggle_play(),
        "playback.next" => window.global::<NowPlayingState>().invoke_next(),
        "playback.prev" => window.global::<NowPlayingState>().invoke_previous(),
        _ => return EventResult::Propagate,
    }
    EventResult::PreventDefault
}
