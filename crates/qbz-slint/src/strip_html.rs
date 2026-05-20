//! Convert HTML-ish strings from Qobuz (biographies, album reviews)
//! into Slint-friendly plain text. Slint's `Text` is single-style, so
//! we cannot render inline strong/em formatting — those tags are
//! stripped but their content stays inline. Paragraph and line-break
//! structure IS preserved: `<br>` collapses to `\n`, `</p>` to a
//! blank line so Text renders the paragraphs separated visually.

/// Render an HTML-ish blurb into plain text with paragraph breaks.
pub fn strip_html(input: &str) -> String {
    let normalized = normalize_breaks(input);
    let stripped = strip_remaining_tags(&normalized);
    let decoded = decode_entities(&stripped);
    collapse_blank_lines(&decoded)
}

/// Walk by char (not byte) so multi-byte UTF-8 sequences (ó, é, "—",
/// curly quotes) survive untouched. Skip recognized `<br>` and `</p>`
/// runs by replacing them with newlines; pass everything else through
/// so the second pass can strip the remaining tags.
fn normalize_breaks(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('<') {
            if let Some((replacement, consumed)) = match_break_or_paragraph(stripped) {
                out.push_str(replacement);
                rest = &stripped[consumed..];
                continue;
            }
        }
        // Advance one char (not one byte) — pushes the full UTF-8
        // sequence intact.
        let mut chars = rest.chars();
        if let Some(ch) = chars.next() {
            out.push(ch);
            rest = chars.as_str();
        } else {
            break;
        }
    }
    out
}

/// Try to match `<br>` (any case, with optional spaces and self-
/// closing slash) or `</p>` (any case). `s` starts AFTER the opening
/// `<`. Returns the replacement string + bytes consumed (after the
/// closing `>`).
fn match_break_or_paragraph(s: &str) -> Option<(&'static str, usize)> {
    let bytes = s.as_bytes();
    // </p>
    if bytes.len() >= 3
        && bytes[0] == b'/'
        && (bytes[1] == b'p' || bytes[1] == b'P')
        && bytes[2] == b'>'
    {
        return Some(("\n\n", 3));
    }
    // <br>, <br/>, <br />, etc.
    if bytes.len() >= 3 && (bytes[0] == b'b' || bytes[0] == b'B')
        && (bytes[1] == b'r' || bytes[1] == b'R')
    {
        let mut j = 2usize;
        while j < bytes.len() && bytes[j] != b'>' {
            // Only allow whitespace and a single '/' between `br` and `>`.
            if !bytes[j].is_ascii_whitespace() && bytes[j] != b'/' {
                return None;
            }
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'>' {
            return Some(("\n", j + 1));
        }
    }
    None
}

/// Drop all remaining tags but keep their text content. Char-safe.
fn strip_remaining_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while !rest.is_empty() {
        if rest.as_bytes()[0] == b'&' {
            if let Some((replacement, consumed)) = match_entity(rest) {
                out.push_str(replacement);
                rest = &rest[consumed..];
                continue;
            }
        }
        let mut chars = rest.chars();
        if let Some(ch) = chars.next() {
            out.push(ch);
            rest = chars.as_str();
        } else {
            break;
        }
    }
    out
}

fn match_entity(s: &str) -> Option<(&'static str, usize)> {
    const TABLE: &[(&str, &str)] = &[
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&apos;", "'"),
        ("&#39;", "'"),
        ("&nbsp;", " "),
        ("&copy;", "\u{00A9}"),
        ("&#169;", "\u{00A9}"),
        ("&#xa9;", "\u{00A9}"),
        ("&reg;", "\u{00AE}"),
        ("&mdash;", "\u{2014}"),
        ("&ndash;", "\u{2013}"),
        ("&hellip;", "\u{2026}"),
        ("&ldquo;", "\u{201C}"),
        ("&rdquo;", "\u{201D}"),
        ("&lsquo;", "\u{2018}"),
        ("&rsquo;", "\u{2019}"),
    ];
    for (needle, replacement) in TABLE {
        if s.starts_with(needle) {
            return Some((replacement, needle.len()));
        }
    }
    None
}

fn collapse_blank_lines(input: &str) -> String {
    let trimmed = input.trim();
    let mut out = String::with_capacity(trimmed.len());
    let mut consecutive_newlines = 0;
    for ch in trimmed.chars() {
        if ch == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines <= 2 {
                out.push(ch);
            }
        } else {
            consecutive_newlines = 0;
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_inline_formatting() {
        let html = "<p>One <strong>bold</strong> and <em>italic</em>.</p>";
        let plain = strip_html(html);
        assert_eq!(plain, "One bold and italic.");
    }

    #[test]
    fn converts_br_to_newline() {
        let html = "Line 1<br>Line 2<br />Line 3";
        assert_eq!(strip_html(html), "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn converts_paragraphs() {
        let html = "<p>First.</p><p>Second.</p>";
        assert_eq!(strip_html(html), "First.\n\nSecond.");
    }

    #[test]
    fn decodes_common_entities() {
        let html = "Rock &amp; Roll &mdash; &ldquo;the rest&rdquo;.";
        assert_eq!(strip_html(html), "Rock & Roll \u{2014} \u{201C}the rest\u{201D}.");
    }

    #[test]
    fn preserves_multibyte_characters() {
        // Mexican Spanish with accented chars, ñ, ó — the previous
        // byte-walking implementation would have shredded these into
        // their UTF-8 bytes (à+³ instead of ó).
        let html = "<p>La cantautora se estableció en Madrid, España.</p>";
        let plain = strip_html(html);
        assert_eq!(plain, "La cantautora se estableció en Madrid, España.");
    }

    #[test]
    fn collapses_excess_newlines() {
        let html = "<p>A</p><p>B</p><p>C</p>";
        let out = strip_html(html);
        assert_eq!(out, "A\n\nB\n\nC");
    }
}
