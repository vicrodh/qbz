//! Auto-theme generation from images and system wallpapers.
//!
//! Extracts dominant colors via k-means clustering, then maps them
//! to the full set of CSS custom properties expected by the QBZ frontend.

pub mod generator;
pub mod palette;
pub mod system;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single RGB color.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct PaletteColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl PaletteColor {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Relative luminance (ITU-R BT.709) in [0.0, 1.0].
    pub fn luminance(&self) -> f64 {
        fn linearize(c: u8) -> f64 {
            let s = c as f64 / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linearize(self.r) + 0.7152 * linearize(self.g) + 0.0722 * linearize(self.b)
    }

    /// HSL saturation in [0.0, 1.0].
    pub fn saturation(&self) -> f64 {
        let (r, g, b) = (self.r as f64 / 255.0, self.g as f64 / 255.0, self.b as f64 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        if delta < 1e-6 {
            return 0.0;
        }
        let l = (max + min) / 2.0;
        if l <= 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        }
    }

    /// WCAG contrast ratio against another color (range [1, 21]).
    pub fn contrast_ratio(&self, other: &PaletteColor) -> f64 {
        let l1 = self.luminance();
        let l2 = other.luminance();
        let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// Shift lightness by `amount` (-1.0 to 1.0) in HSL space. Returns a new color.
    pub fn shift_lightness(&self, amount: f64) -> PaletteColor {
        let (h, s, l) = self.to_hsl();
        let new_l = (l + amount).clamp(0.0, 1.0);
        PaletteColor::from_hsl(h, s, new_l)
    }

    /// Convert to HSL (h in [0, 360), s and l in [0, 1]).
    pub fn to_hsl(&self) -> (f64, f64, f64) {
        let (r, g, b) = (self.r as f64 / 255.0, self.g as f64 / 255.0, self.b as f64 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let delta = max - min;

        if delta < 1e-6 {
            return (0.0, 0.0, l);
        }

        let s = if l <= 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };

        let h = if (max - r).abs() < 1e-6 {
            ((g - b) / delta) % 6.0
        } else if (max - g).abs() < 1e-6 {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        let h = (h * 60.0 + 360.0) % 360.0;

        (h, s, l)
    }

    /// Construct from HSL values.
    pub fn from_hsl(h: f64, s: f64, l: f64) -> PaletteColor {
        if s < 1e-6 {
            let v = (l * 255.0).round() as u8;
            return PaletteColor::new(v, v, v);
        }

        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let h_prime = h / 60.0;
        let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
        let m = l - c / 2.0;

        let (r1, g1, b1) = match h_prime as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };

        PaletteColor::new(
            ((r1 + m) * 255.0).round() as u8,
            ((g1 + m) * 255.0).round() as u8,
            ((b1 + m) * 255.0).round() as u8,
        )
    }

    /// CSS hex string (#rrggbb).
    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// CSS rgba() string with given alpha (0.0 to 1.0).
    pub fn to_rgba(&self, alpha: f64) -> String {
        format!("rgba({}, {}, {}, {})", self.r, self.g, self.b, alpha)
    }

    /// Euclidean distance in RGB space.
    pub fn distance(&self, other: &PaletteColor) -> f64 {
        let dr = self.r as f64 - other.r as f64;
        let dg = self.g as f64 - other.g as f64;
        let db = self.b as f64 - other.b as f64;
        (dr * dr + dg * dg + db * db).sqrt()
    }
}

/// Extracted palette from an image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemePalette {
    pub bg_primary: PaletteColor,
    pub bg_secondary: PaletteColor,
    pub bg_tertiary: PaletteColor,
    pub bg_hover: PaletteColor,
    pub accent: PaletteColor,
    pub is_dark: bool,
    pub all_colors: Vec<PaletteColor>,
}

/// A fully generated theme ready for CSS injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedTheme {
    /// CSS variable name (e.g. "--bg-primary") to value (e.g. "#0f0f0f").
    pub variables: HashMap<String, String>,
    pub is_dark: bool,
    pub source: String,
}

/// Full color scheme read from the desktop environment.
///
/// Maps directly to the semantic color roles in KDE kdeglobals, GNOME dconf, etc.
/// Each field is optional because not all DEs expose all colors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemColorScheme {
    // Backgrounds
    pub window_bg: Option<PaletteColor>,
    pub window_bg_alt: Option<PaletteColor>,
    pub view_bg: Option<PaletteColor>,
    pub button_bg: Option<PaletteColor>,
    pub header_bg: Option<PaletteColor>,
    pub header_bg_inactive: Option<PaletteColor>,
    pub tooltip_bg: Option<PaletteColor>,

    // Foregrounds (text)
    pub window_fg: Option<PaletteColor>,
    pub window_fg_inactive: Option<PaletteColor>,
    pub view_fg: Option<PaletteColor>,
    pub button_fg: Option<PaletteColor>,

    // Selection / accent
    pub selection_bg: Option<PaletteColor>,
    pub selection_fg: Option<PaletteColor>,
    pub selection_hover: Option<PaletteColor>,
    pub accent: Option<PaletteColor>,

    // Semantic
    pub fg_link: Option<PaletteColor>,
    pub fg_negative: Option<PaletteColor>,
    pub fg_neutral: Option<PaletteColor>,
    pub fg_positive: Option<PaletteColor>,

    // Window manager
    pub wm_active_bg: Option<PaletteColor>,
    pub wm_active_fg: Option<PaletteColor>,
    pub wm_inactive_bg: Option<PaletteColor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_luminance_black() {
        let black = PaletteColor::new(0, 0, 0);
        assert!((black.luminance() - 0.0).abs() < 1e-4);
    }

    #[test]
    fn test_luminance_white() {
        let white = PaletteColor::new(255, 255, 255);
        assert!((white.luminance() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_contrast_ratio_bw() {
        let black = PaletteColor::new(0, 0, 0);
        let white = PaletteColor::new(255, 255, 255);
        let ratio = black.contrast_ratio(&white);
        assert!((ratio - 21.0).abs() < 0.1);
    }

    #[test]
    fn test_hsl_roundtrip() {
        let c = PaletteColor::new(66, 133, 244); // Google Blue
        let (h, s, l) = c.to_hsl();
        let back = PaletteColor::from_hsl(h, s, l);
        assert!((c.r as i16 - back.r as i16).unsigned_abs() <= 1);
        assert!((c.g as i16 - back.g as i16).unsigned_abs() <= 1);
        assert!((c.b as i16 - back.b as i16).unsigned_abs() <= 1);
    }

    #[test]
    fn test_saturation_gray() {
        let gray = PaletteColor::new(128, 128, 128);
        assert!(gray.saturation() < 0.01);
    }

    #[test]
    fn test_hex_format() {
        let c = PaletteColor::new(15, 15, 15);
        assert_eq!(c.to_hex(), "#0f0f0f");
    }
}
