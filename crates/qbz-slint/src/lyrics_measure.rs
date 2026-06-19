//! Per-visual-line wrap measurement for the karaoke highlight.
//!
//! # Why this exists
//!
//! A logical lyric line that word-wraps to two (or more) visual lines used to
//! light up ALL visual rows at once, because the active line was rendered as a
//! single `wrap: word-wrap` Text masked by ONE clip rectangle of full block
//! height and a single-scalar width (`LyricsLinesView.slint`). A
//! single-scalar-width rectangle of full height reveals the same left fraction
//! of *every* visual row simultaneously — Tauri instead splits the active line
//! into one element PER VISUAL LINE and fills them sequentially (row 1 to
//! 100%, then row 2). This module is the Rust analogue of Tauri's Pretext
//! layout pass: it reproduces Slint's wrap decision and reports, per visual
//! line, the segment text and its pixel ink width so the engine can partition
//! the single global progress fraction across the segments by width share.
//!
//! # How it measures
//!
//! femtovg (the Linux/Windows renderer) shapes glyphs with `swash`, so a swash
//! advance pass at the same font + ppem + weight axis + letter-spacing matches
//! what is actually rendered. The four bundled variable lyric fonts plus the
//! Inter "System"/default bold are embedded here via `include_bytes!` (the
//! same TTFs `LyricsLinesView.slint` registers via `import`) and parsed once
//! into process-global `swash::FontRef`s (held as owned byte buffers in a
//! `OnceLock`).
//!
//! The active lyric line is rendered BOLD; for the variable fonts we set the
//! `wght` variation axis to 700 so the advances match the bold raster. (Inter
//! "System"/default is measured from `Inter_18pt-Bold.ttf` directly, which is
//! already a bold static instance — no axis needed. LINE Seed JP ships only a
//! Regular file with no weight axis, so it is measured at its single instance;
//! the split is still per-visual-line and self-consistent because we render
//! the segments ourselves.)
//!
//! Letter-spacing is added as a flat `LETTER_SPACING_PX` per glyph cluster, to
//! mirror the `letter-spacing: 0.2px` on the rendered Text items.
//!
//! # Wrapping algorithm
//!
//! Greedy word wrap, matching `wrap: word-wrap`: split the text on ASCII/Unicode
//! whitespace into words; keep appending words (with their separating space) to
//! the current visual line while they fit `max_width_px`; otherwise start a new
//! visual line. A single word longer than the budget is broken per grapheme
//! cluster (CJK / no-space runs also break per character this way). Each
//! emitted segment carries its measured pixel width.

use std::sync::OnceLock;

use swash::shape::ShapeContext;
use swash::FontRef;

/// Per-glyph-cluster letter spacing, mirroring `letter-spacing: 0.2px` on the
/// rendered Text items in `LyricsLinesView.slint`.
const LETTER_SPACING_PX: f32 = 0.2;

/// The weight axis value for the bold active line (`Typography.bold` ~= 700).
const BOLD_WGHT: f32 = 700.0;

/// One bundled lyric font, parsed lazily and kept alive for the process.
struct LoadedFont {
    /// Owned font bytes — `FontRef` borrows from these, so they must outlive it.
    data: &'static [u8],
    /// Font collection index (0 for all single-face TTFs here).
    index: usize,
    /// Whether this font exposes a `wght` variation axis (set 700 when true).
    variable: bool,
}

/// Embedded copies of the EXACT TTFs `LyricsLinesView.slint` registers, plus
/// the Inter bold used for the "System"/default family. `include_bytes!` paths
/// are relative to THIS source file (`crates/qbz-slint/src/`).
static FONT_SYSTEM_INTER_BOLD: &[u8] =
    include_bytes!("../ui/assets/fonts/Inter_18pt-Bold.ttf");
static FONT_LINE_SEED_JP: &[u8] =
    include_bytes!("../../../static/fonts/LINESeedJP-Regular.ttf");
static FONT_MONTSERRAT: &[u8] =
    include_bytes!("../../../static/fonts/Montserrat-VariableFont_wght.ttf");
static FONT_NOTO_SANS: &[u8] =
    include_bytes!("../../../static/fonts/NotoSans-VariableFont_wdth,wght.ttf");
static FONT_SOURCE_SANS_3: &[u8] =
    include_bytes!("../../../static/fonts/SourceSans3-VariableFont_wght.ttf");

/// Map a `font-index` (the same enum as `LyricsState.font-index` /
/// `LyricsSidebar.slint`: 0=System→Inter, 1=LINE Seed JP, 2=Montserrat,
/// 3=Noto Sans, 4=Source Sans 3) to its embedded bytes. Unknown indices fall
/// back to the System/Inter default, matching the `.slint` `font-name` default.
fn loaded_font(font_index: i32) -> &'static LoadedFont {
    static FONTS: OnceLock<[LoadedFont; 5]> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        [
            // 0 System (window default = Inter bold static instance).
            LoadedFont { data: FONT_SYSTEM_INTER_BOLD, index: 0, variable: false },
            // 1 LINE Seed JP (Regular only — no weight axis).
            LoadedFont { data: FONT_LINE_SEED_JP, index: 0, variable: false },
            // 2 Montserrat (variable wght).
            LoadedFont { data: FONT_MONTSERRAT, index: 0, variable: true },
            // 3 Noto Sans (variable wdth,wght).
            LoadedFont { data: FONT_NOTO_SANS, index: 0, variable: true },
            // 4 Source Sans 3 (variable wght).
            LoadedFont { data: FONT_SOURCE_SANS_3, index: 0, variable: true },
        ]
    });
    let idx = match font_index {
        1..=4 => font_index as usize,
        _ => 0,
    };
    &fonts[idx]
}

/// One measured visual line of a wrapped logical lyric line.
#[derive(Debug, Clone)]
pub struct Segment {
    /// The text of this visual line (no leading/trailing wrap whitespace).
    pub text: String,
    /// The rendered pixel ink+advance width of this visual line (includes the
    /// per-cluster letter spacing, so it matches the drawn Text width).
    pub width_px: f32,
}

/// Measure the advance width (px) of `text` at `size_px` for `font`, including
/// the flat per-cluster letter spacing — the same metric Slint uses to size a
/// `no-wrap` Text. Returns the total advance.
fn measure_width(ctx: &mut ShapeContext, font: &LoadedFont, text: &str, size_px: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let Some(font_ref) = FontRef::from_index(font.data, font.index) else {
        // Parse failure: fall back to a crude estimate so callers still split.
        return text.chars().count() as f32 * size_px * 0.5;
    };
    let mut builder = ctx.builder(font_ref).size(size_px);
    if font.variable {
        builder = builder.variations(&[("wght", BOLD_WGHT)]);
    }
    let mut shaper = builder.build();
    shaper.add_str(text);
    let mut total = 0.0_f32;
    let mut clusters = 0_u32;
    shaper.shape_with(|cluster| {
        for glyph in cluster.glyphs {
            total += glyph.advance;
        }
        clusters += 1;
    });
    // Letter spacing is applied per cluster on the rendered side.
    total + clusters as f32 * LETTER_SPACING_PX
}

/// Split a single over-long word into per-grapheme-cluster pieces that each fit
/// `max_width_px` (CJK and other no-space runs land here too). Appends the
/// resulting segments to `out`.
fn break_long_word(
    ctx: &mut ShapeContext,
    font: &LoadedFont,
    word: &str,
    size_px: f32,
    max_width_px: f32,
    out: &mut Vec<Segment>,
) {
    let mut current = String::new();
    let mut current_w = 0.0_f32;
    // Break per Unicode scalar (good enough for CJK; avoids pulling in a
    // grapheme-segmentation dependency — lyric runs are short).
    for ch in word.chars() {
        let piece: String = ch.to_string();
        let piece_w = measure_width(ctx, font, &piece, size_px);
        if !current.is_empty() && current_w + piece_w > max_width_px {
            out.push(Segment { text: std::mem::take(&mut current), width_px: current_w });
            current_w = 0.0;
        }
        current.push(ch);
        current_w += piece_w;
    }
    if !current.is_empty() {
        out.push(Segment { text: current, width_px: current_w });
    }
}

/// Compute the per-visual-line segmentation of `text` as it would word-wrap
/// inside `max_width_px`, at the given `font_index` + `size_px` (bold weight,
/// 0.2px letter spacing — matching the active lyric line render).
///
/// Returns one [`Segment`] per visual line, each with the segment text and its
/// rendered pixel width. An empty / whitespace-only input yields a single empty
/// segment so the caller always has at least one row to render.
pub fn wrap_segments(text: &str, font_index: i32, size_px: f32, max_width_px: f32) -> Vec<Segment> {
    let font = loaded_font(font_index);
    let mut ctx = ShapeContext::new();
    let budget = max_width_px.max(1.0);

    // Tokenize into words, preserving nothing of the original whitespace except
    // that each inter-word gap becomes a single space when re-joined.
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return vec![Segment { text: String::new(), width_px: 0.0 }];
    }

    let space_w = measure_width(&mut ctx, font, " ", size_px);

    let mut out: Vec<Segment> = Vec::new();
    let mut line = String::new();
    let mut line_w = 0.0_f32;

    for word in words {
        let word_w = measure_width(&mut ctx, font, word, size_px);

        if line.is_empty() {
            // First word on a fresh line.
            if word_w > budget {
                // Word alone overflows — hard-break it into pieces.
                break_long_word(&mut ctx, font, word, size_px, budget, &mut out);
                // The tail piece (if any) becomes the start of the current line
                // so following words can still pack onto it.
                if let Some(last) = out.pop() {
                    line = last.text;
                    line_w = last.width_px;
                }
            } else {
                line.push_str(word);
                line_w = word_w;
            }
            continue;
        }

        // Subsequent word: does it fit with a leading space?
        let added = space_w + word_w;
        if line_w + added <= budget {
            line.push(' ');
            line.push_str(word);
            line_w += added;
        } else {
            // Flush the current line and start anew with this word.
            out.push(Segment { text: std::mem::take(&mut line), width_px: line_w });
            line_w = 0.0;
            if word_w > budget {
                break_long_word(&mut ctx, font, word, size_px, budget, &mut out);
                if let Some(last) = out.pop() {
                    line = last.text;
                    line_w = last.width_px;
                }
            } else {
                line.push_str(word);
                line_w = word_w;
            }
        }
    }

    if !line.is_empty() || out.is_empty() {
        out.push(Segment { text: line, width_px: line_w });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_short_line_one_segment() {
        let segs = wrap_segments("hello world", 0, 15.0, 10_000.0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "hello world");
        assert!(segs[0].width_px > 0.0);
    }

    #[test]
    fn empty_input_yields_one_empty_segment() {
        let segs = wrap_segments("   ", 0, 15.0, 100.0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "");
        assert_eq!(segs[0].width_px, 0.0);
    }

    #[test]
    fn narrow_budget_wraps_to_multiple_segments() {
        // A budget too small for the whole phrase must split it.
        let segs = wrap_segments("alpha beta gamma delta", 0, 15.0, 40.0);
        assert!(segs.len() >= 2, "expected a wrap, got {} segs", segs.len());
        // Every emitted segment must be non-empty (no dangling blank rows).
        for seg in &segs {
            assert!(!seg.text.is_empty());
        }
    }

    #[test]
    fn over_long_word_is_hard_broken() {
        // One unbreakable token wider than the budget breaks per character.
        let segs = wrap_segments("aaaaaaaaaaaaaaaaaaaa", 0, 15.0, 20.0);
        assert!(segs.len() >= 2, "expected hard break, got {}", segs.len());
    }
}
