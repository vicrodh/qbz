//! Catppuccin Mocha theme for the TUI.
//!
//! Provides a `Theme` trait with colour slots and a `Mocha` implementation
//! with the exact Catppuccin Mocha palette.  Use `THEME` for global access.

use ratatui::style::Color;

/// All colour slots consumed by the TUI renderer.
pub trait Theme {
    // ── Surface layers ─────────────────────────────────────────────
    fn base(&self) -> Color;
    fn mantle(&self) -> Color;
    fn crust(&self) -> Color;
    fn surface0(&self) -> Color;
    fn surface1(&self) -> Color;
    fn surface2(&self) -> Color;

    // ── Text hierarchy ─────────────────────────────────────────────
    fn text(&self) -> Color;
    fn subtext1(&self) -> Color;
    fn subtext0(&self) -> Color;

    // ── Overlays ───────────────────────────────────────────────────
    fn overlay2(&self) -> Color;
    fn overlay1(&self) -> Color;
    fn overlay0(&self) -> Color;

    // ── Semantic colours ───────────────────────────────────────────
    fn primary(&self) -> Color;   // sapphire
    fn success(&self) -> Color;   // green
    fn danger(&self) -> Color;    // red
    fn accent(&self) -> Color;    // mauve
    fn selection(&self) -> Color; // yellow
    fn warning(&self) -> Color;   // peach
}

/// Catppuccin Mocha implementation.
pub struct Mocha;

/// Global theme instance.
pub static THEME: Mocha = Mocha;

impl Theme for Mocha {
    // ── Surface layers ─────────────────────────────────────────────
    fn base(&self) -> Color    { Color::Rgb(30, 30, 46) }
    fn mantle(&self) -> Color  { Color::Rgb(24, 24, 37) }
    fn crust(&self) -> Color   { Color::Rgb(17, 17, 27) }
    fn surface0(&self) -> Color { Color::Rgb(49, 50, 68) }
    fn surface1(&self) -> Color { Color::Rgb(69, 71, 90) }
    fn surface2(&self) -> Color { Color::Rgb(88, 91, 112) }

    // ── Text hierarchy ─────────────────────────────────────────────
    fn text(&self) -> Color     { Color::Rgb(205, 214, 244) }
    fn subtext1(&self) -> Color { Color::Rgb(186, 194, 222) }
    fn subtext0(&self) -> Color { Color::Rgb(166, 173, 200) }

    // ── Overlays ───────────────────────────────────────────────────
    fn overlay2(&self) -> Color { Color::Rgb(147, 153, 178) }
    fn overlay1(&self) -> Color { Color::Rgb(127, 132, 156) }
    fn overlay0(&self) -> Color { Color::Rgb(108, 112, 134) }

    // ── Semantic colours ───────────────────────────────────────────
    fn primary(&self) -> Color   { Color::Rgb(116, 199, 236) } // sapphire
    fn success(&self) -> Color   { Color::Rgb(166, 218, 149) } // green
    fn danger(&self) -> Color    { Color::Rgb(243, 139, 168) } // red
    fn accent(&self) -> Color    { Color::Rgb(203, 166, 247) } // mauve
    fn selection(&self) -> Color { Color::Rgb(249, 226, 175) } // yellow
    fn warning(&self) -> Color   { Color::Rgb(250, 179, 135) } // peach
}
