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

/// Push a fully-materialized registry `ThemeColors` into the running window's
/// `Theme` global. Shared by [`apply_theme`] (static themes) and the auto-theme
/// path (`crate::auto_theme`), so both go through the exact same conversion +
/// global-set sequence.
pub fn push_colors(
    window: &AppWindow,
    colors: &qbz_theme::ThemeColors,
    is_system: bool,
    is_high_contrast: bool,
) {
    let theme = window.global::<SlintTheme>();
    theme.set_c(to_slint(colors));
    theme.set_is_system(is_system);
    theme.set_is_high_contrast(is_high_contrast);
    // Relative luminance (BT.709) of the base surface -> is-dark. Drives the
    // std-widgets `Palette.color-scheme` in app.slint so native inputs follow
    // the QBZ theme; computed here (not from ThemeId) so it's correct for the
    // auto/custom themes too. System keeps following the OS (app.slint sets the
    // scheme to `unknown` when is-system, ignoring this flag).
    let s = colors.surface_main;
    let luma = 0.2126 * s.r as f64 + 0.7152 * s.g as f64 + 0.0722 * s.b as f64;
    theme.set_is_dark(luma < 128.0);
}

/// Push the palette for `id` into the running window's `Theme` global. Sets
/// `is-system` so the System theme follows the OS (the struct is still pushed as
/// a sane fallback for any non-System-overridden tokens).
pub fn apply_theme(window: &AppWindow, id: ThemeId) {
    let colors = qbz_theme::palette(id);
    push_colors(window, &colors, id == ThemeId::System, qbz_theme::is_high_contrast(id));
    log::info!("[qbz-slint] applied theme '{}'", id.slug());
}

/// Stable slug persisted for the dynamic "Auto" theme option. Distinct from the
/// registry slugs (it has no static `ThemeId`): the dropdown appends it after
/// the registry rows and `crate::auto_theme` generates the palette at runtime.
pub const AUTO_SLUG: &str = "auto";

/// Display label for the appended "Auto (dynamic)" dropdown entry. Like the
/// registry display names (`"System"`, `"Nord"`, …) this is proper-noun-style
/// UI data pushed from Rust, not a `@tr` catalog string.
pub const AUTO_LABEL: &str = "Auto (dynamic)";

/// Stable slug persisted for the user-authored "Custom" theme. Like `AUTO_SLUG`
/// it has no static `ThemeId`: the dropdown appends it after "Auto (dynamic)"
/// and `crate::custom_theme` derives the palette from `custom_theme.json`.
pub const CUSTOM_SLUG: &str = "custom";

/// Display label for the appended "Custom" dropdown entry. Proper-noun-style UI
/// data pushed from Rust, not a `@tr` catalog string (matches `AUTO_LABEL`).
pub const CUSTOM_LABEL: &str = "Custom";

/// Dropdown index of the appended "Auto (dynamic)" entry (right after every
/// registry theme; the "Custom" entry follows it).
pub fn auto_index() -> i32 {
    dropdown_themes().len() as i32
}

/// Dropdown index of the appended "Custom" entry (last position, right after
/// "Auto (dynamic)").
pub fn custom_index() -> i32 {
    auto_index() + 1
}

/// Whether a dropdown index refers to the appended "Auto (dynamic)" entry.
pub fn is_auto_index(index: i32) -> bool {
    index == auto_index()
}

/// Whether a dropdown index refers to the appended "Custom" entry.
pub fn is_custom_index(index: i32) -> bool {
    index == custom_index()
}

/// The dropdown index for a persisted theme slug, auto/custom-aware: the two
/// synthetic slugs map to their appended entries, everything else through the
/// registry.
pub fn selected_index_for_slug(slug: &str) -> i32 {
    if slug == AUTO_SLUG {
        auto_index()
    } else if slug == CUSTOM_SLUG {
        custom_index()
    } else {
        index_for_id(id_for_slug(slug))
    }
}

/// The themes shown in the Settings dropdown, in display order. P1 exposes only
/// the implemented rows. The dropdown index is just a position in THIS list.
pub fn dropdown_themes() -> Vec<ThemeId> {
    qbz_theme::implemented_theme_list()
        .into_iter()
        .map(|e| e.id)
        .collect()
}

/// Display names for the dropdown, matching [`dropdown_themes`] order, with the
/// dynamic "Auto (dynamic)" entry (index == [`auto_index`]) and the "Custom"
/// entry (index == [`custom_index`]) appended last, in that order.
pub fn dropdown_labels() -> Vec<String> {
    let mut labels: Vec<String> = dropdown_themes()
        .into_iter()
        .map(|id| id.display_name().to_string())
        .collect();
    labels.push(AUTO_LABEL.to_string());
    labels.push(CUSTOM_LABEL.to_string());
    labels
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

    #[test]
    fn auto_then_custom_are_the_last_two_entries() {
        // Auto is appended first, Custom right after it.
        assert_eq!(auto_index(), dropdown_themes().len() as i32);
        assert_eq!(custom_index(), auto_index() + 1);
        assert!(is_auto_index(auto_index()));
        assert!(is_custom_index(custom_index()));
        assert!(!is_auto_index(custom_index()));
        assert!(!is_custom_index(auto_index()));
        // The labels list is registry rows + Auto + Custom, in that order.
        let labels = dropdown_labels();
        assert_eq!(labels.len(), dropdown_themes().len() + 2);
        assert_eq!(labels[auto_index() as usize], AUTO_LABEL);
        assert_eq!(labels[custom_index() as usize], CUSTOM_LABEL);
    }

    #[test]
    fn synthetic_slugs_map_to_appended_indices() {
        assert_eq!(selected_index_for_slug(AUTO_SLUG), auto_index());
        assert_eq!(selected_index_for_slug(CUSTOM_SLUG), custom_index());
        // A real slug still resolves through the registry.
        assert_eq!(selected_index_for_slug("oled"), index_for_id(ThemeId::Oled));
    }
}
