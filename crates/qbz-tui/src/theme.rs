use ratatui::style::Color;

// ── Accent colours ──────────────────────────────────────────────
pub const ACCENT: Color = Color::Cyan;
pub const ACCENT_ALT: Color = Color::Green;

// ── Backgrounds ─────────────────────────────────────────────────
pub const BG_PRIMARY: Color = Color::Rgb(15, 20, 25); // dark blue-gray
pub const BG_SECONDARY: Color = Color::Rgb(20, 25, 32);
pub const BG_SELECTED: Color = Color::Rgb(30, 45, 55); // teal tint for selection

// ── Text hierarchy ──────────────────────────────────────────────
pub const TEXT_PRIMARY: Color = Color::White;
pub const TEXT_SECONDARY: Color = Color::Rgb(160, 170, 180);
pub const TEXT_MUTED: Color = Color::Rgb(100, 110, 120);
pub const TEXT_DIM: Color = Color::Rgb(60, 70, 80);

// ── Semantic colours ────────────────────────────────────────────
pub const SUCCESS: Color = Color::Green;
pub const DANGER: Color = Color::Red;
pub const HIRES_BADGE: Color = Color::Rgb(80, 200, 120); // green for hi-res
