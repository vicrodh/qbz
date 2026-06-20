//! Theme application: bridge the frontend-agnostic `qbz-theme` registry to the
//! Slint `Theme` global.
//!
//! `qbz-theme` owns all palette data (ADR-006). This module is the only place
//! that knows about both `qbz_theme::Rgba` and the generated Slint `ThemeColors`
//! struct: it converts one to the other and pushes it into `Theme.c`, plus sets
//! the `Theme.is-system` flag so the System theme keeps following the OS.
//!
//! The dropdown shows a filtered/ordered list of themes; the stable slug is the
//! source of truth (persisted in `ui_prefs.theme`), and the dropdown index is
//! DERIVED from it — never the reverse.

use crate::{AppWindow, Theme as SlintTheme, ThemeColors as SlintThemeColors};
use qbz_theme::{Rgba, ThemeId};
use slint::{Color, ComponentHandle};

/// Convert a registry `Rgba` to a Slint `Color` (straight alpha).
fn to_color(c: Rgba) -> Color {
    Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

/// Build the generated Slint `ThemeColors` from a registry `ThemeColors`.
fn to_slint(c: &qbz_theme::ThemeColors) -> SlintThemeColors {
    SlintThemeColors {
        surface_main: to_color(c.surface_main),
        surface_card: to_color(c.surface_card),
        surface_elevated: to_color(c.surface_elevated),
        surface_hover: to_color(c.surface_hover),
        bg_hover: to_color(c.bg_hover),

        text_primary: to_color(c.text_primary),
        text_secondary: to_color(c.text_secondary),
        text_muted: to_color(c.text_muted),
        text_disabled: to_color(c.text_disabled),

        accent: to_color(c.accent),
        accent_hover: to_color(c.accent_hover),
        accent_pressed: to_color(c.accent_pressed),
        accent_text: to_color(c.accent_text),

        danger: to_color(c.danger),
        danger_bg: to_color(c.danger_bg),
        danger_border: to_color(c.danger_border),
        danger_hover: to_color(c.danger_hover),

        warning: to_color(c.warning),
        warning_bg: to_color(c.warning_bg),
        warning_border: to_color(c.warning_border),
        warning_hover: to_color(c.warning_hover),

        success: to_color(c.success),
        success_bg: to_color(c.success_bg),
        success_border: to_color(c.success_border),
        success_hover: to_color(c.success_hover),

        border_subtle: to_color(c.border_subtle),
        border_muted: to_color(c.border_muted),
        border_strong: to_color(c.border_strong),

        focus_ring: to_color(c.focus_ring),

        favorite: to_color(c.favorite),
        card_shadow: to_color(c.card_shadow),

        alpha_4: to_color(c.alpha_pct(4)),
        alpha_5: to_color(c.alpha_pct(5)),
        alpha_6: to_color(c.alpha_pct(6)),
        alpha_8: to_color(c.alpha_pct(8)),
        alpha_10: to_color(c.alpha_pct(10)),
        alpha_12: to_color(c.alpha_pct(12)),
        alpha_15: to_color(c.alpha_pct(15)),
        alpha_18: to_color(c.alpha_pct(18)),
        alpha_20: to_color(c.alpha_pct(20)),
        alpha_25: to_color(c.alpha_pct(25)),
        alpha_30: to_color(c.alpha_pct(30)),
        alpha_35: to_color(c.alpha_pct(35)),
        alpha_40: to_color(c.alpha_pct(40)),
        alpha_45: to_color(c.alpha_pct(45)),
        alpha_50: to_color(c.alpha_pct(50)),
        alpha_55: to_color(c.alpha_pct(55)),
        alpha_60: to_color(c.alpha_pct(60)),
        alpha_65: to_color(c.alpha_pct(65)),
        alpha_70: to_color(c.alpha_pct(70)),
        alpha_75: to_color(c.alpha_pct(75)),
        alpha_80: to_color(c.alpha_pct(80)),
        alpha_85: to_color(c.alpha_pct(85)),
        alpha_90: to_color(c.alpha_pct(90)),
        alpha_95: to_color(c.alpha_pct(95)),
    }
}

/// Push the palette for `id` into the running window's `Theme` global. Sets
/// `is-system` so the System theme follows the OS (the struct is still pushed as
/// a sane fallback for any non-System-overridden tokens).
pub fn apply_theme(window: &AppWindow, id: ThemeId) {
    let colors = qbz_theme::palette(id);
    let theme = window.global::<SlintTheme>();
    theme.set_c(to_slint(&colors));
    theme.set_is_system(id == ThemeId::System);
    theme.set_is_high_contrast(qbz_theme::is_high_contrast(id));
    log::info!("[qbz-slint] applied theme '{}'", id.slug());
}

/// The themes shown in the Settings dropdown, in display order. P1 exposes only
/// the implemented rows. The dropdown index is just a position in THIS list.
pub fn dropdown_themes() -> Vec<ThemeId> {
    qbz_theme::implemented_theme_list()
        .into_iter()
        .map(|e| e.id)
        .collect()
}

/// Display names for the dropdown, matching [`dropdown_themes`] order.
pub fn dropdown_labels() -> Vec<String> {
    dropdown_themes()
        .into_iter()
        .map(|id| id.display_name().to_string())
        .collect()
}

/// Map a persisted slug to a `ThemeId`, falling back to the default (OLED) when
/// the slug is unknown or absent.
pub fn id_for_slug(slug: &str) -> ThemeId {
    ThemeId::from_slug(slug).unwrap_or_else(qbz_theme::default_theme_id)
}

/// Map a dropdown index to a `ThemeId`. Out-of-range indices fall back to the
/// default theme.
pub fn id_for_index(index: i32) -> ThemeId {
    let list = dropdown_themes();
    list.get(index as usize)
        .copied()
        .unwrap_or_else(qbz_theme::default_theme_id)
}

/// Derive the dropdown index for a `ThemeId` (position in [`dropdown_themes`]).
/// Returns `0` if the id is not in the dropdown list (e.g. a P2/P3 theme not yet
/// exposed) — the caller should treat that as "no explicit selection".
pub fn index_for_id(id: ThemeId) -> i32 {
    dropdown_themes()
        .iter()
        .position(|&t| t == id)
        .map(|p| p as i32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_index_roundtrip_for_p1() {
        for (i, id) in dropdown_themes().into_iter().enumerate() {
            assert_eq!(index_for_id(id), i as i32);
            assert_eq!(id_for_index(i as i32), id);
            assert_eq!(id_for_slug(id.slug()), id);
        }
    }

    #[test]
    fn unknown_slug_falls_back_to_oled() {
        assert_eq!(id_for_slug("nope"), ThemeId::Oled);
        assert_eq!(id_for_slug(""), ThemeId::Oled);
    }

    #[test]
    fn out_of_range_index_falls_back_to_default() {
        assert_eq!(id_for_index(9999), qbz_theme::default_theme_id());
        assert_eq!(id_for_index(-1), qbz_theme::default_theme_id());
    }
}
