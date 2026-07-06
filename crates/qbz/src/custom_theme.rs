//! Custom-theme controller + persistence: wire the Settings "Custom" theme
//! option to `qbz_theme::custom` derivation.
//!
//! The user-authored [`CustomThemeBase`] (12 base tokens) is persisted next to
//! the other QBZ data (`<data_dir>/qbz/custom_theme.json`) and derived into a
//! full palette by `qbz_theme::theme_from_base`. Derivation is cheap (pure color
//! math, no I/O), so every token edit re-derives and re-pushes the palette live
//! on the event loop — no debounce needed.
//!
//! This module mirrors `crate::auto_theme` for its wiring style (weak-handle
//! push through `crate::theme::push_colors`, the same path static and auto
//! themes use). Persistence mirrors `crate::ui_prefs::{load, save}`.

use crate::AppWindow;
use crate::AppearanceState;
use crate::Theme as SlintTheme;
use qbz_theme::{CustomThemeBase, Rgba};
use slint::{Color, ComponentHandle};
use std::path::PathBuf;

/// Convert a registry `Rgba` to a Slint `Color` (straight alpha). Local mirror of
/// `crate::theme::to_color` (which is private to that module).
fn to_color(c: Rgba) -> Color {
    Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

/// Convert a Slint `Color` back to a registry `Rgba`.
fn rgba_of(c: Color) -> Rgba {
    Rgba::rgba(c.red(), c.green(), c.blue(), c.alpha())
}

/// Read the current editable base straight from the editor swatch properties on
/// `AppearanceState` — the in-memory source of truth while the editor is open, so
/// per-drag edits never hit the disk to reconstruct the base.
fn base_from_state(window: &AppWindow) -> CustomThemeBase {
    let st = window.global::<AppearanceState>();
    CustomThemeBase {
        is_dark: st.get_custom_is_dark(),
        surface_main: rgba_of(st.get_custom_surface_main()).to_hex(),
        surface_card: rgba_of(st.get_custom_surface_card()).to_hex(),
        surface_elevated: rgba_of(st.get_custom_surface_elevated()).to_hex(),
        text_primary: rgba_of(st.get_custom_text_primary()).to_hex(),
        text_secondary: rgba_of(st.get_custom_text_secondary()).to_hex(),
        accent: rgba_of(st.get_custom_accent()).to_hex(),
        danger: rgba_of(st.get_custom_danger()).to_hex(),
        warning: rgba_of(st.get_custom_warning()).to_hex(),
        success: rgba_of(st.get_custom_success()).to_hex(),
        border: rgba_of(st.get_custom_border()).to_hex(),
        favorite: rgba_of(st.get_custom_favorite()).to_hex(),
    }
}

/// Assign one base-token field by its stable key (the `token-key` strings the
/// Slint editor rows use). Unknown keys are ignored.
fn set_field(base: &mut CustomThemeBase, key: &str, hex: String) {
    match key {
        "surface-main" => base.surface_main = hex,
        "surface-card" => base.surface_card = hex,
        "surface-elevated" => base.surface_elevated = hex,
        "text-primary" => base.text_primary = hex,
        "text-secondary" => base.text_secondary = hex,
        "accent" => base.accent = hex,
        "danger" => base.danger = hex,
        "warning" => base.warning = hex,
        "success" => base.success = hex,
        "border" => base.border = hex,
        "favorite" => base.favorite = hex,
        other => log::debug!("[qbz-slint] custom theme: unknown token key '{other}'"),
    }
}

/// Reflect a single edited token back into its editor swatch (so the swatch
/// preview and the ColorPicker's `value` binding update), WITHOUT touching the
/// open-token state (the inline picker must stay open through the edit).
fn set_one_swatch(window: &AppWindow, key: &str, color: Color) {
    let st = window.global::<AppearanceState>();
    match key {
        "surface-main" => st.set_custom_surface_main(color),
        "surface-card" => st.set_custom_surface_card(color),
        "surface-elevated" => st.set_custom_surface_elevated(color),
        "text-primary" => st.set_custom_text_primary(color),
        "text-secondary" => st.set_custom_text_secondary(color),
        "accent" => st.set_custom_accent(color),
        "danger" => st.set_custom_danger(color),
        "warning" => st.set_custom_warning(color),
        "success" => st.set_custom_success(color),
        "border" => st.set_custom_border(color),
        "favorite" => st.set_custom_favorite(color),
        _ => {}
    }
}

/// Derive `base`, push the palette live, and persist — WITHOUT re-seeding the
/// editor swatches (the caller updates only the touched swatch). Used by the
/// per-token and polarity edits so the inline picker stays open.
fn apply_live(window: &AppWindow, base: &CustomThemeBase) {
    let colors = qbz_theme::theme_from_base(base);
    crate::theme::push_colors(window, &colors, false, false);
    save(base);
}

/// Parse an opaque `#rrggbb` base token into a Slint `Color`, falling back to
/// transparent-safe black on malformed input (the derivation applies the real
/// fallbacks; this only feeds the editor swatch preview).
fn hex_to_color(hex: &str) -> Color {
    to_color(Rgba::from_hex(hex).unwrap_or(Rgba::rgb(0, 0, 0)))
}

/// Resolve `<data_dir>/qbz/custom_theme.json` (same dir as `ui_prefs.json`).
fn custom_theme_path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("qbz").join("custom_theme.json"))
}

/// Load the persisted custom base. A missing/unreadable/corrupt file degrades to
/// the OLED-derived default rather than erroring (matches `ui_prefs::load`).
pub fn load() -> CustomThemeBase {
    let Some(path) = custom_theme_path() else {
        return CustomThemeBase::default_oled();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!("[qbz-slint] custom_theme.json parse failed, using default: {e}");
            CustomThemeBase::default_oled()
        }),
        Err(_) => CustomThemeBase::default_oled(),
    }
}

/// Persist the custom base. Best-effort — failures are logged (matches
/// `ui_prefs::save`).
pub fn save(base: &CustomThemeBase) {
    let Some(path) = custom_theme_path() else {
        log::warn!("[qbz-slint] custom_theme.json: data dir unavailable, not saving");
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::error!("[qbz-slint] custom_theme.json: create dir failed: {e}");
            return;
        }
    }
    match serde_json::to_string_pretty(base) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                log::error!("[qbz-slint] custom_theme.json: write failed: {e}");
            }
        }
        Err(e) => log::error!("[qbz-slint] custom_theme.json: serialize failed: {e}"),
    }
}

/// Load the persisted base, or seed + persist the OLED default when no file
/// exists yet (the first time the user selects the "Custom" theme).
pub fn load_or_seed() -> CustomThemeBase {
    let exists = custom_theme_path().map(|p| p.exists()).unwrap_or(false);
    if exists {
        load()
    } else {
        let base = CustomThemeBase::default_oled();
        save(&base);
        base
    }
}

/// Push a [`CustomThemeBase`] into the editor swatch properties on
/// `AppearanceState` so the swatches reflect the current base. Collapses any open
/// inline picker.
fn push_base_to_state(window: &AppWindow, base: &CustomThemeBase) {
    let st = window.global::<AppearanceState>();
    st.set_custom_surface_main(hex_to_color(&base.surface_main));
    st.set_custom_surface_card(hex_to_color(&base.surface_card));
    st.set_custom_surface_elevated(hex_to_color(&base.surface_elevated));
    st.set_custom_text_primary(hex_to_color(&base.text_primary));
    st.set_custom_text_secondary(hex_to_color(&base.text_secondary));
    st.set_custom_accent(hex_to_color(&base.accent));
    st.set_custom_danger(hex_to_color(&base.danger));
    st.set_custom_warning(hex_to_color(&base.warning));
    st.set_custom_success(hex_to_color(&base.success));
    st.set_custom_border(hex_to_color(&base.border));
    st.set_custom_favorite(hex_to_color(&base.favorite));
    st.set_custom_is_dark(base.is_dark);
    st.set_custom_open_token("".into());
}

/// Seed the custom-theme editor swatches from the persisted base (or the OLED
/// default when none exists). Runs at startup for every user, so it uses the
/// non-persisting [`load`] — the `custom_theme.json` file is only created once
/// the user actually selects/edits the Custom theme (via [`load_or_seed`]).
pub fn seed_state(window: &AppWindow) {
    let base = load();
    push_base_to_state(window, &base);
}

/// Startup apply: derive the persisted (or freshly seeded) custom base and push
/// the palette. Runs inline on the event-loop thread during window init so the
/// first paint is already the custom palette.
pub fn apply_startup(window: &AppWindow) {
    let base = load_or_seed();
    let colors = qbz_theme::theme_from_base(&base);
    crate::theme::push_colors(window, &colors, false, false);
    log::info!("[qbz-slint] applied custom theme");
}

/// Set one base token to `color` (the live ColorPicker drag path). Re-derives and
/// re-pushes the whole palette in real time, persists, and updates the token's
/// own swatch — the inline picker stays open.
pub fn set_token(window: &AppWindow, key: &str, color: Color) {
    let mut base = base_from_state(window);
    set_field(&mut base, key, rgba_of(color).to_hex());
    apply_live(window, &base);
    set_one_swatch(window, key, color);
}

/// Set one base token from a committed HEX string (`#rrggbb`). Malformed input is
/// ignored; a valid value reuses [`set_token`] (which also updates the swatch,
/// reseeding the picker's crosshair via its `value` binding).
pub fn set_token_hex(window: &AppWindow, key: &str, hex: &str) {
    match Rgba::from_hex(hex) {
        Some(c) => set_token(window, key, to_color(Rgba::rgb(c.r, c.g, c.b))),
        None => log::debug!("[qbz-slint] custom theme: ignoring malformed hex '{hex}'"),
    }
}

/// Flip the custom theme polarity (dark/light) and re-derive. The base token
/// colors are unchanged; only `is_dark` and the derived shades/overlays flip.
pub fn toggle_dark(window: &AppWindow, is_dark: bool) {
    let mut base = base_from_state(window);
    base.is_dark = is_dark;
    apply_live(window, &base);
    window.global::<AppearanceState>().set_custom_is_dark(is_dark);
}

/// "Start from current theme": snapshot the LIVE applied palette (whatever is in
/// the `Theme` global — static, auto or custom) into the editable base, then
/// derive/apply/persist and re-seed every editor swatch. `is_dark` is inferred
/// from the surface luminance; `border` prefers the opaque subtle edge, else the
/// strong one (the four legacy P1 themes store a translucent-white hairline in
/// `border_subtle` that would seed as a jarring pure-white edge).
pub fn seed_from_current(window: &AppWindow) {
    let c = window.global::<SlintTheme>().get_c();
    let surface_main = rgba_of(c.surface_main);
    let is_dark = qbz_theme::relative_luminance(Rgba::rgb(
        surface_main.r,
        surface_main.g,
        surface_main.b,
    )) < 0.5;
    let border_subtle = rgba_of(c.border_subtle);
    let border = if border_subtle.a == 255 {
        border_subtle
    } else {
        rgba_of(c.border_strong)
    };
    let base = CustomThemeBase {
        is_dark,
        surface_main: surface_main.to_hex(),
        surface_card: rgba_of(c.surface_card).to_hex(),
        surface_elevated: rgba_of(c.surface_elevated).to_hex(),
        text_primary: rgba_of(c.text_primary).to_hex(),
        text_secondary: rgba_of(c.text_secondary).to_hex(),
        accent: rgba_of(c.accent).to_hex(),
        danger: rgba_of(c.danger).to_hex(),
        warning: rgba_of(c.warning).to_hex(),
        success: rgba_of(c.success).to_hex(),
        border: border.to_hex(),
        favorite: rgba_of(c.favorite).to_hex(),
    };
    let colors = qbz_theme::theme_from_base(&base);
    crate::theme::push_colors(window, &colors, false, false);
    save(&base);
    push_base_to_state(window, &base);
}
