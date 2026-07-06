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
use qbz_theme::{CustomThemeBase, Rgba};
use slint::{Color, ComponentHandle};
use std::path::PathBuf;

/// Convert a registry `Rgba` to a Slint `Color` (straight alpha). Local mirror of
/// `crate::theme::to_color` (which is private to that module).
fn to_color(c: Rgba) -> Color {
    Color::from_argb_u8(c.a, c.r, c.g, c.b)
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

/// Seed the custom-theme editor swatches from the persisted (or freshly seeded)
/// base. Called at startup so the editor reflects the saved base when Settings
/// opens.
pub fn seed_state(window: &AppWindow) {
    let base = load_or_seed();
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
