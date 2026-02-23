//! Generate a complete set of CSS custom properties from a ThemePalette.

use std::collections::HashMap;

use super::{GeneratedTheme, PaletteColor, SystemColorScheme, ThemePalette};

/// Generate a full CSS theme from an extracted palette.
pub fn generate_theme(palette: &ThemePalette, source: &str) -> GeneratedTheme {
    let mut vars = HashMap::new();

    // Backgrounds
    vars.insert("--bg-primary".into(), palette.bg_primary.to_hex());
    vars.insert("--bg-secondary".into(), palette.bg_secondary.to_hex());
    vars.insert("--bg-tertiary".into(), palette.bg_tertiary.to_hex());
    vars.insert("--bg-hover".into(), palette.bg_hover.to_hex());

    // Text colors
    let (text_primary, text_secondary, text_muted, text_disabled) = if palette.is_dark {
        (
            PaletteColor::new(255, 255, 255),
            PaletteColor::new(204, 204, 204),
            PaletteColor::new(136, 136, 136),
            PaletteColor::new(85, 85, 85),
        )
    } else {
        (
            PaletteColor::new(15, 15, 15),
            PaletteColor::new(68, 68, 68),
            PaletteColor::new(102, 102, 102),
            PaletteColor::new(153, 153, 153),
        )
    };

    // Adjust text-primary for contrast if needed
    let text_primary = ensure_text_contrast(text_primary, &palette.bg_primary, palette.is_dark);

    vars.insert("--text-primary".into(), text_primary.to_hex());
    vars.insert("--text-secondary".into(), text_secondary.to_hex());
    vars.insert("--text-muted".into(), text_muted.to_hex());
    vars.insert("--text-disabled".into(), text_disabled.to_hex());

    // Accent
    vars.insert("--accent-primary".into(), palette.accent.to_hex());
    vars.insert(
        "--accent-hover".into(),
        palette.accent.shift_lightness(0.10).to_hex(),
    );
    vars.insert(
        "--accent-active".into(),
        palette.accent.shift_lightness(-0.10).to_hex(),
    );

    // Button text: white if accent is dark, black if light
    let btn_text = if palette.accent.luminance() < 0.5 {
        PaletteColor::new(255, 255, 255)
    } else {
        PaletteColor::new(0, 0, 0)
    };
    vars.insert("--btn-primary-text".into(), btn_text.to_hex());

    // Status colors (fixed safe values, consistent with existing themes)
    if palette.is_dark {
        vars.insert("--danger".into(), "#ef4444".into());
        vars.insert("--danger-bg".into(), "rgba(239, 68, 68, 0.1)".into());
        vars.insert("--danger-border".into(), "rgba(239, 68, 68, 0.3)".into());
        vars.insert("--danger-hover".into(), "rgba(239, 68, 68, 0.2)".into());

        vars.insert("--warning".into(), "#fbbf24".into());
        vars.insert("--warning-bg".into(), "rgba(251, 191, 36, 0.1)".into());
        vars.insert("--warning-border".into(), "rgba(251, 191, 36, 0.3)".into());
        vars.insert("--warning-hover".into(), "rgba(251, 191, 36, 0.2)".into());
    } else {
        vars.insert("--danger".into(), "#dc2626".into());
        vars.insert("--danger-bg".into(), "rgba(220, 38, 38, 0.1)".into());
        vars.insert("--danger-border".into(), "rgba(220, 38, 38, 0.3)".into());
        vars.insert("--danger-hover".into(), "rgba(220, 38, 38, 0.15)".into());

        vars.insert("--warning".into(), "#d97706".into());
        vars.insert("--warning-bg".into(), "rgba(217, 119, 6, 0.1)".into());
        vars.insert("--warning-border".into(), "rgba(217, 119, 6, 0.3)".into());
        vars.insert("--warning-hover".into(), "rgba(217, 119, 6, 0.15)".into());
    }

    // Borders: subtle shifts from bg_primary
    let border_subtle = if palette.is_dark {
        palette.bg_primary.shift_lightness(0.08)
    } else {
        palette.bg_primary.shift_lightness(-0.08)
    };
    let border_strong = if palette.is_dark {
        palette.bg_primary.shift_lightness(0.14)
    } else {
        palette.bg_primary.shift_lightness(-0.14)
    };
    vars.insert("--border-subtle".into(), border_subtle.to_hex());
    vars.insert("--border-strong".into(), border_strong.to_hex());

    // Alpha tokens: white-based for dark, black-based for light
    let alpha_base = if palette.is_dark {
        (255, 255, 255)
    } else {
        (0, 0, 0)
    };

    let alpha_levels: &[(f64, &str)] = &[
        (0.04, "--alpha-4"),
        (0.05, "--alpha-5"),
        (0.06, "--alpha-6"),
        (0.08, "--alpha-8"),
        (0.10, "--alpha-10"),
        (0.15, "--alpha-15"),
        (0.18, "--alpha-18"),
        (0.20, "--alpha-20"),
        (0.25, "--alpha-25"),
        (0.30, "--alpha-30"),
        (0.35, "--alpha-35"),
        (0.40, "--alpha-40"),
        (0.45, "--alpha-45"),
        (0.50, "--alpha-50"),
        (0.60, "--alpha-60"),
        (0.70, "--alpha-70"),
        (0.80, "--alpha-80"),
        (0.85, "--alpha-85"),
        (0.90, "--alpha-90"),
        (0.95, "--alpha-95"),
    ];

    for (alpha, name) in alpha_levels {
        vars.insert(
            name.to_string(),
            format!(
                "rgba({}, {}, {}, {})",
                alpha_base.0, alpha_base.1, alpha_base.2, alpha
            ),
        );
    }

    GeneratedTheme {
        variables: vars,
        is_dark: palette.is_dark,
        source: source.to_string(),
    }
}

/// Generate a full CSS theme directly from a DE color scheme.
///
/// Instead of k-means extraction, this maps the DE's semantic colors (window bg,
/// view bg, button bg, selection, etc.) directly to CSS variables — producing a
/// theme that matches the system look exactly.
pub fn generate_theme_from_scheme(scheme: &SystemColorScheme) -> GeneratedTheme {
    let mut vars = HashMap::new();

    // Determine dark/light from the window background
    let window_bg = scheme
        .window_bg
        .unwrap_or(PaletteColor::new(40, 40, 40));
    let is_dark = window_bg.luminance() < 0.5;

    // ── Backgrounds ──────────────────────────────────────────────────────
    vars.insert("--bg-primary".into(), window_bg.to_hex());

    let bg_secondary = scheme.view_bg.unwrap_or_else(|| {
        if is_dark {
            window_bg.shift_lightness(0.03)
        } else {
            window_bg.shift_lightness(-0.03)
        }
    });
    vars.insert("--bg-secondary".into(), bg_secondary.to_hex());

    let bg_tertiary = scheme.button_bg.unwrap_or_else(|| {
        if is_dark {
            window_bg.shift_lightness(0.10)
        } else {
            window_bg.shift_lightness(-0.10)
        }
    });
    vars.insert("--bg-tertiary".into(), bg_tertiary.to_hex());

    // Hover: midpoint between primary and secondary
    let bg_hover = scheme.window_bg_alt.unwrap_or_else(|| {
        PaletteColor::new(
            ((window_bg.r as u16 + bg_secondary.r as u16) / 2) as u8,
            ((window_bg.g as u16 + bg_secondary.g as u16) / 2) as u8,
            ((window_bg.b as u16 + bg_secondary.b as u16) / 2) as u8,
        )
    });
    vars.insert("--bg-hover".into(), bg_hover.to_hex());

    // ── Text ─────────────────────────────────────────────────────────────
    let text_primary = scheme
        .window_fg
        .unwrap_or(if is_dark {
            PaletteColor::new(223, 223, 223)
        } else {
            PaletteColor::new(36, 36, 36)
        });
    let text_primary = ensure_text_contrast(text_primary, &window_bg, is_dark);
    vars.insert("--text-primary".into(), text_primary.to_hex());

    // Secondary: slightly dimmed from primary
    let text_secondary = scheme.view_fg.unwrap_or_else(|| {
        text_primary.shift_lightness(if is_dark { -0.10 } else { 0.10 })
    });
    vars.insert("--text-secondary".into(), text_secondary.to_hex());

    let text_muted = scheme.window_fg_inactive.unwrap_or_else(|| {
        text_primary.shift_lightness(if is_dark { -0.25 } else { 0.25 })
    });
    vars.insert("--text-muted".into(), text_muted.to_hex());

    let text_disabled = text_muted.shift_lightness(if is_dark { -0.10 } else { 0.10 });
    vars.insert("--text-disabled".into(), text_disabled.to_hex());

    // ── Accent (selection) ───────────────────────────────────────────────
    let accent = scheme
        .accent
        .or(scheme.selection_bg)
        .unwrap_or(PaletteColor::new(0, 120, 215));
    vars.insert("--accent-primary".into(), accent.to_hex());

    let accent_hover = scheme
        .selection_hover
        .unwrap_or_else(|| accent.shift_lightness(0.10));
    vars.insert("--accent-hover".into(), accent_hover.to_hex());
    vars.insert(
        "--accent-active".into(),
        accent.shift_lightness(-0.10).to_hex(),
    );

    // Button text on accent
    let btn_text = scheme.selection_fg.unwrap_or(
        if accent.luminance() < 0.5 {
            PaletteColor::new(255, 255, 255)
        } else {
            PaletteColor::new(0, 0, 0)
        },
    );
    vars.insert("--btn-primary-text".into(), btn_text.to_hex());

    // ── Status colors ────────────────────────────────────────────────────
    // Use system negative/neutral if available, else fallback to safe defaults
    let danger = scheme
        .fg_negative
        .unwrap_or(if is_dark {
            PaletteColor::new(239, 68, 68)
        } else {
            PaletteColor::new(220, 38, 38)
        });
    vars.insert("--danger".into(), danger.to_hex());
    vars.insert("--danger-bg".into(), danger.to_rgba(0.1));
    vars.insert("--danger-border".into(), danger.to_rgba(0.3));
    vars.insert(
        "--danger-hover".into(),
        danger.to_rgba(if is_dark { 0.2 } else { 0.15 }),
    );

    let warning = scheme
        .fg_neutral
        .unwrap_or(if is_dark {
            PaletteColor::new(251, 191, 36)
        } else {
            PaletteColor::new(217, 119, 6)
        });
    vars.insert("--warning".into(), warning.to_hex());
    vars.insert("--warning-bg".into(), warning.to_rgba(0.1));
    vars.insert("--warning-border".into(), warning.to_rgba(0.3));
    vars.insert(
        "--warning-hover".into(),
        warning.to_rgba(if is_dark { 0.2 } else { 0.15 }),
    );

    // ── Borders ──────────────────────────────────────────────────────────
    let border_subtle = if is_dark {
        window_bg.shift_lightness(0.06)
    } else {
        window_bg.shift_lightness(-0.06)
    };
    let border_strong = if is_dark {
        window_bg.shift_lightness(0.12)
    } else {
        window_bg.shift_lightness(-0.12)
    };
    vars.insert("--border-subtle".into(), border_subtle.to_hex());
    vars.insert("--border-strong".into(), border_strong.to_hex());

    // ── Alpha tokens ─────────────────────────────────────────────────────
    let alpha_base = if is_dark {
        (255, 255, 255)
    } else {
        (0, 0, 0)
    };
    let alpha_levels: &[(f64, &str)] = &[
        (0.04, "--alpha-4"),
        (0.05, "--alpha-5"),
        (0.06, "--alpha-6"),
        (0.08, "--alpha-8"),
        (0.10, "--alpha-10"),
        (0.15, "--alpha-15"),
        (0.18, "--alpha-18"),
        (0.20, "--alpha-20"),
        (0.25, "--alpha-25"),
        (0.30, "--alpha-30"),
        (0.35, "--alpha-35"),
        (0.40, "--alpha-40"),
        (0.45, "--alpha-45"),
        (0.50, "--alpha-50"),
        (0.60, "--alpha-60"),
        (0.70, "--alpha-70"),
        (0.80, "--alpha-80"),
        (0.85, "--alpha-85"),
        (0.90, "--alpha-90"),
        (0.95, "--alpha-95"),
    ];
    for (alpha, name) in alpha_levels {
        vars.insert(
            name.to_string(),
            format!(
                "rgba({}, {}, {}, {})",
                alpha_base.0, alpha_base.1, alpha_base.2, alpha
            ),
        );
    }

    GeneratedTheme {
        variables: vars,
        is_dark,
        source: "system-colors".to_string(),
    }
}

/// Ensure text has at least WCAG AA contrast (4.5:1) against the background.
fn ensure_text_contrast(
    text: PaletteColor,
    bg: &PaletteColor,
    is_dark: bool,
) -> PaletteColor {
    if text.contrast_ratio(bg) >= 4.5 {
        return text;
    }

    // Shift text toward white (dark theme) or black (light theme) until contrast is met
    let (h, s, l) = text.to_hsl();
    let direction = if is_dark { 0.05 } else { -0.05 };
    let mut new_l = l;

    for _ in 0..20 {
        new_l = (new_l + direction).clamp(0.0, 1.0);
        let candidate = PaletteColor::from_hsl(h, s, new_l);
        if candidate.contrast_ratio(bg) >= 4.5 {
            return candidate;
        }
    }

    // Absolute fallback
    if is_dark {
        PaletteColor::new(255, 255, 255)
    } else {
        PaletteColor::new(0, 0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_theme_dark() {
        let palette = ThemePalette {
            bg_primary: PaletteColor::new(15, 15, 20),
            bg_secondary: PaletteColor::new(26, 26, 30),
            bg_tertiary: PaletteColor::new(42, 42, 48),
            bg_hover: PaletteColor::new(31, 31, 35),
            accent: PaletteColor::new(66, 133, 244),
            is_dark: true,
            all_colors: vec![],
        };

        let theme = generate_theme(&palette, "test");

        assert!(theme.is_dark);
        assert_eq!(theme.source, "test");
        assert!(theme.variables.contains_key("--bg-primary"));
        assert!(theme.variables.contains_key("--accent-primary"));
        assert!(theme.variables.contains_key("--alpha-50"));
        assert!(theme.variables.contains_key("--danger"));
        assert!(theme.variables.contains_key("--border-subtle"));

        // Dark theme should have white-based alphas
        let alpha50 = theme.variables.get("--alpha-50").unwrap();
        assert!(alpha50.starts_with("rgba(255, 255, 255"));
    }

    #[test]
    fn test_generate_theme_light() {
        let palette = ThemePalette {
            bg_primary: PaletteColor::new(245, 245, 245),
            bg_secondary: PaletteColor::new(235, 235, 235),
            bg_tertiary: PaletteColor::new(220, 220, 220),
            bg_hover: PaletteColor::new(240, 240, 240),
            accent: PaletteColor::new(26, 115, 232),
            is_dark: false,
            all_colors: vec![],
        };

        let theme = generate_theme(&palette, "test-light");

        assert!(!theme.is_dark);

        // Light theme should have black-based alphas
        let alpha50 = theme.variables.get("--alpha-50").unwrap();
        assert!(alpha50.starts_with("rgba(0, 0, 0"));

        // Text should be dark
        let text = theme.variables.get("--text-primary").unwrap();
        assert!(text.starts_with("#0"));
    }

    #[test]
    fn test_all_required_tokens_present() {
        let palette = ThemePalette {
            bg_primary: PaletteColor::new(20, 20, 25),
            bg_secondary: PaletteColor::new(30, 30, 35),
            bg_tertiary: PaletteColor::new(45, 45, 50),
            bg_hover: PaletteColor::new(35, 35, 40),
            accent: PaletteColor::new(100, 200, 100),
            is_dark: true,
            all_colors: vec![],
        };

        let theme = generate_theme(&palette, "completeness-test");

        let required = [
            "--bg-primary",
            "--bg-secondary",
            "--bg-tertiary",
            "--bg-hover",
            "--text-primary",
            "--text-secondary",
            "--text-muted",
            "--text-disabled",
            "--accent-primary",
            "--accent-hover",
            "--accent-active",
            "--btn-primary-text",
            "--danger",
            "--danger-bg",
            "--danger-border",
            "--danger-hover",
            "--warning",
            "--warning-bg",
            "--warning-border",
            "--warning-hover",
            "--border-subtle",
            "--border-strong",
            "--alpha-4",
            "--alpha-5",
            "--alpha-6",
            "--alpha-8",
            "--alpha-10",
            "--alpha-15",
            "--alpha-18",
            "--alpha-20",
            "--alpha-25",
            "--alpha-30",
            "--alpha-35",
            "--alpha-40",
            "--alpha-45",
            "--alpha-50",
            "--alpha-60",
            "--alpha-70",
            "--alpha-80",
            "--alpha-85",
            "--alpha-90",
            "--alpha-95",
        ];

        for key in &required {
            assert!(
                theme.variables.contains_key(*key),
                "Missing required token: {}",
                key
            );
        }
    }
}
