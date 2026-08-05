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
//! **The wrap DECISION** (does candidate line L fit in `max_width_px`?) is
//! answered by the ACTUAL Slint window via [`crate::lyrics_sync::slint_fits_one_line`] —
//! a hidden, always-mounted `wrap: word-wrap` `Text` on `AppWindow`
//! (`LyricsWrapProbe` in state.slint / the probe elements in app.slint) laid
//! out with the exact same font/size/weight/letter-spacing/width as the real
//! segments. This is a genuine measurement by whatever backend is actually
//! active (Skia on macOS, femtovg on Linux/Windows, ...), so the wrap points
//! this module computes are — by construction — the same ones Slint itself
//! would choose. (Earlier revisions of this module re-implemented the
//! measurement independently with `swash`, calibrated for femtovg; that
//! silently drifted from Skia's shaping on macOS — issue #664 — which is why
//! this indirection exists at all.)
//!
//! **Per-segment pixel widths** (`Segment::width_px`, used only to
//! proportion the single global karaoke-fill fraction across visual rows by
//! width share) are still measured with `swash` below — those don't need to
//! match the real renderer pixel-for-pixel, only be self-consistent with each
//! other, and a probe round-trip per segment would be needless overhead for a
//! number that's discarded as soon as it's turned into a ratio.
//!
//! swash advance pass at the same font + ppem + weight axis + letter-spacing.
//! The four bundled variable lyric fonts plus the Inter "System"/default bold
//! are embedded here via `include_bytes!` (the same TTFs `LyricsLinesView.slint`
//! registers via `import`) and parsed once into process-global `swash::FontRef`s
//! (held as owned byte buffers in a `OnceLock`).
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
//! the current visual line while the LIVE PROBE reports they still fit;
//! otherwise start a new visual line. A single word the probe reports doesn't
//! fit alone is broken per grapheme cluster (CJK / no-space runs also break
//! per character this way; each candidate piece is checked against the probe
//! too). Each emitted segment carries its `swash`-measured pixel width (for
//! fill-fraction weighting only — see above).

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
    include_bytes!("../../qbz-ui/ui/assets/fonts/Inter_18pt-Bold.ttf");
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
/// on one visual row per `fits` (CJK and other no-space runs land here too).
/// Appends the resulting segments (with their `swash`-measured width, for
/// fill-fraction weighting) to `out`.
fn break_long_word(
    ctx: &mut ShapeContext,
    font: &LoadedFont,
    word: &str,
    size_px: f32,
    fits: &mut impl FnMut(&str) -> bool,
    out: &mut Vec<Segment>,
) {
    let mut current = String::new();
    // Break per Unicode scalar (good enough for CJK; avoids pulling in a
    // grapheme-segmentation dependency — lyric runs are short).
    for ch in word.chars() {
        let mut candidate = current.clone();
        candidate.push(ch);
        if !current.is_empty() && !fits(&candidate) {
            let width_px = measure_width(ctx, font, &current, size_px);
            out.push(Segment { text: std::mem::take(&mut current), width_px });
        }
        current.push(ch);
    }
    if !current.is_empty() {
        let width_px = measure_width(ctx, font, &current, size_px);
        out.push(Segment { text: current, width_px });
    }
}

/// Compute the per-visual-line segmentation of `text` as it would word-wrap
/// inside `max_width_px`, at the given `font_index` + `size_px` (bold weight,
/// 0.2px letter spacing — matching the active lyric line render).
///
/// `fits(candidate)` is the wrap oracle: true iff `candidate` renders on ONE
/// visual row at `max_width_px` in the CALLER's actual active Slint renderer
/// (see `crate::lyrics_sync::slint_fits_one_line` — the real fix for issue
/// #664's follow-on: this used to be answered by an independent swash
/// re-measurement here, which could disagree with the real renderer).
///
/// Returns one [`Segment`] per visual line, each with the segment text and its
/// `swash`-measured pixel width (fill-fraction weighting only). An empty /
/// whitespace-only input yields a single empty segment so the caller always
/// has at least one row to render.
pub fn wrap_segments(
    text: &str,
    font_index: i32,
    size_px: f32,
    max_width_px: f32,
    mut fits: impl FnMut(&str) -> bool,
) -> Vec<Segment> {
    let font = loaded_font(font_index);
    let mut ctx = ShapeContext::new();

    // Tokenize into words, preserving nothing of the original whitespace except
    // that each inter-word gap becomes a single space when re-joined.
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return vec![Segment { text: String::new(), width_px: 0.0 }];
    }

    let mut out: Vec<Segment> = Vec::new();
    let mut line = String::new();

    for word in words {
        if line.is_empty() {
            // First word on a fresh line.
            if !fits(word) {
                // Word alone overflows — hard-break it into pieces.
                break_long_word(&mut ctx, font, word, size_px, &mut fits, &mut out);
                // The tail piece (if any) becomes the start of the current line
                // so following words can still pack onto it.
                if let Some(last) = out.pop() {
                    line = last.text;
                }
            } else {
                line.push_str(word);
            }
            continue;
        }

        // Subsequent word: does it fit with a leading space?
        let mut candidate = line.clone();
        candidate.push(' ');
        candidate.push_str(word);
        if fits(&candidate) {
            line = candidate;
        } else {
            // Flush the current line and start anew with this word.
            let width_px = measure_width(&mut ctx, font, &line, size_px);
            out.push(Segment { text: std::mem::take(&mut line), width_px });
            if !fits(word) {
                break_long_word(&mut ctx, font, word, size_px, &mut fits, &mut out);
                if let Some(last) = out.pop() {
                    line = last.text;
                }
            } else {
                line.push_str(word);
            }
        }
    }

    if !line.is_empty() || out.is_empty() {
        let width_px = measure_width(&mut ctx, font, &line, size_px);
        out.push(Segment { text: line, width_px });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-only stand-in for the live Slint probe (`slint_fits_one_line`):
    /// no window exists in a unit test, so fall back to the same swash
    /// measurement `wrap_segments` used before the probe existed. This is
    /// exactly the drift-prone approximation issue #664 was about — fine
    /// here since these tests only check the ALGORITHM's shape (does it
    /// wrap, does it hard-break, ...), not real-renderer pixel accuracy.
    fn swash_oracle(font_index: i32, size_px: f32, max_width_px: f32) -> impl FnMut(&str) -> bool {
        let font = loaded_font(font_index);
        let mut ctx = ShapeContext::new();
        let budget = max_width_px.max(1.0);
        move |candidate: &str| measure_width(&mut ctx, font, candidate, size_px) <= budget
    }

    #[test]
    fn single_short_line_one_segment() {
        let segs = wrap_segments("hello world", 0, 15.0, 10_000.0, swash_oracle(0, 15.0, 10_000.0));
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "hello world");
        assert!(segs[0].width_px > 0.0);
    }

    #[test]
    fn empty_input_yields_one_empty_segment() {
        let segs = wrap_segments("   ", 0, 15.0, 100.0, swash_oracle(0, 15.0, 100.0));
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "");
        assert_eq!(segs[0].width_px, 0.0);
    }

    #[test]
    fn narrow_budget_wraps_to_multiple_segments() {
        // A budget too small for the whole phrase must split it.
        let segs = wrap_segments(
            "alpha beta gamma delta",
            0,
            15.0,
            40.0,
            swash_oracle(0, 15.0, 40.0),
        );
        assert!(segs.len() >= 2, "expected a wrap, got {} segs", segs.len());
        // Every emitted segment must be non-empty (no dangling blank rows).
        for seg in &segs {
            assert!(!seg.text.is_empty());
        }
    }

    #[test]
    fn over_long_word_is_hard_broken() {
        // One unbreakable token wider than the budget breaks per character.
        let segs = wrap_segments(
            "aaaaaaaaaaaaaaaaaaaa",
            0,
            15.0,
            20.0,
            swash_oracle(0, 15.0, 20.0),
        );
        assert!(segs.len() >= 2, "expected hard break, got {}", segs.len());
    }
}
