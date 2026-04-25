//! DJ-mix sampling for Collections / Mixtapes.
//!
//! Pure functions used by `v2_collection_unique_track_count` and
//! `v2_collection_shuffle_tracks`. No Tauri types here — fully unit-testable.
//!
//! See spec: qbz-nix-docs/superpowers/specs/2026-04-25-track-shuffle-mix-design.md

/// Tracks whose normalized titles score at or above this Jaro/token-set
/// threshold are considered the same song (within the same normalized artist
/// bucket).
pub const SIMILARITY_THRESHOLD: f32 = 0.80;

/// No single album may contribute more than this fraction of the requested
/// sample size, after applying [`ALBUM_CAP_MIN`] as a floor.
pub const ALBUM_CAP_PCT: f32 = 0.30;

/// Floor for the per-album cap so that small requested sizes do not feel
/// artificially trimmed (e.g. requested = 20, cap = max(2, 6) = 6).
pub const ALBUM_CAP_MIN: usize = 2;

/// Lowercase, strip diacritics, drop bracketed parentheticals, drop ` - `
/// suffixes, drop `feat. X` patterns, drop punctuation, collapse whitespace.
pub fn normalize_title(s: &str) -> String {
    let lower = s.to_lowercase();
    let unaccented = strip_diacritics(&lower);
    let unbracketed = remove_brackets(&unaccented);
    let untrailed = strip_dash_suffix(&unbracketed);
    let unfeatured = strip_feat(&untrailed);
    let unpunct = strip_punctuation(&unfeatured);
    collapse_whitespace(&unpunct)
}

/// Lowercase + strip diacritics + trim. Parens are preserved (e.g. `Foo (band)`
/// must not collapse to `Foo`).
pub fn normalize_artist(s: &str) -> String {
    let lower = s.to_lowercase();
    let unaccented = strip_diacritics(&lower);
    collapse_whitespace(&unaccented)
}

/// Token-set similarity in `[0.0, 1.0]`, modeled on RapidFuzz's
/// `token_set_ratio`. Inputs are expected to be already normalized.
pub fn token_set_ratio(a: &str, b: &str) -> f32 {
    use std::collections::BTreeSet;

    let tokens_a: BTreeSet<&str> = a.split_whitespace().collect();
    let tokens_b: BTreeSet<&str> = b.split_whitespace().collect();

    if tokens_a.is_empty() && tokens_b.is_empty() {
        return 1.0;
    }
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }

    let inter: Vec<&str> = tokens_a.intersection(&tokens_b).copied().collect();
    let diff_a: Vec<&str> = tokens_a.difference(&tokens_b).copied().collect();
    let diff_b: Vec<&str> = tokens_b.difference(&tokens_a).copied().collect();

    let t1 = inter.join(" ");
    let t2 = join_with_intersection(&t1, &diff_a);
    let t3 = join_with_intersection(&t1, &diff_b);

    let r12 = strsim::normalized_levenshtein(&t1, &t2);
    let r13 = strsim::normalized_levenshtein(&t1, &t3);
    let r23 = strsim::normalized_levenshtein(&t2, &t3);

    r12.max(r13).max(r23) as f32
}

// ──────── internal helpers ────────

fn strip_diacritics(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ą' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'ī' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'ū' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            'ÿ' | 'ý' => 'y',
            'ß' => 's',
            other => other,
        })
        .collect()
}

fn remove_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ => {
                if depth == 0 {
                    out.push(c);
                }
            }
        }
    }
    out
}

fn strip_dash_suffix(s: &str) -> String {
    match s.find(" - ") {
        Some(idx) => s[..idx].to_string(),
        None => s.to_string(),
    }
}

fn strip_feat(s: &str) -> String {
    // Order matters: longest patterns first so " feat. " wins over " feat ".
    const PATTERNS: &[&str] = &[" featuring ", " feat. ", " feat ", " ft. ", " ft "];
    for p in PATTERNS {
        if let Some(idx) = s.find(p) {
            return s[..idx].to_string();
        }
    }
    s.to_string()
}

fn strip_punctuation(s: &str) -> String {
    const PUNCT: &[char] = &[
        ',', '.', '!', '?', '¿', '¡', '"', '\'', ';', ':', '/', '\\',
    ];
    s.chars().filter(|c| !PUNCT.contains(c)).collect()
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<&str>>().join(" ")
}

fn join_with_intersection(t1: &str, diff: &[&str]) -> String {
    if diff.is_empty() {
        return t1.to_string();
    }
    let diff_joined = diff.join(" ");
    if t1.is_empty() {
        diff_joined
    } else {
        format!("{} {}", t1, diff_joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ──────── normalize_title ────────

    #[test]
    fn normalize_title_lowercases() {
        assert_eq!(normalize_title("Yesterday"), "yesterday");
    }

    #[test]
    fn normalize_title_strips_parens() {
        assert_eq!(normalize_title("Yesterday (Live)"), "yesterday");
        assert_eq!(normalize_title("Song (Acoustic Version)"), "song");
    }

    #[test]
    fn normalize_title_strips_brackets_and_braces() {
        assert_eq!(normalize_title("Track [Bonus]"), "track");
        assert_eq!(normalize_title("Tune {Demo}"), "tune");
    }

    #[test]
    fn normalize_title_strips_dash_suffix() {
        assert_eq!(normalize_title("Song - 2003 Remaster"), "song");
        assert_eq!(normalize_title("Tune - Live at Wembley"), "tune");
    }

    #[test]
    fn normalize_title_strips_feat() {
        assert_eq!(normalize_title("Song feat. Artist X"), "song");
        assert_eq!(normalize_title("Tune ft. X"), "tune");
        assert_eq!(normalize_title("Anthem featuring Y"), "anthem");
    }

    #[test]
    fn normalize_title_strips_diacritics() {
        assert_eq!(normalize_title("Café"), "cafe");
        assert_eq!(normalize_title("Niño"), "nino");
        assert_eq!(normalize_title("Über"), "uber");
    }

    #[test]
    fn normalize_title_strips_punctuation() {
        assert_eq!(normalize_title("Don't Stop!"), "dont stop");
        assert_eq!(normalize_title("¿Qué Pasa?"), "que pasa");
    }

    #[test]
    fn normalize_title_collapses_whitespace() {
        assert_eq!(normalize_title("  Hello   World  "), "hello world");
    }

    #[test]
    fn normalize_title_combined() {
        assert_eq!(
            normalize_title("¡Yesterday! (Live, Wembley) - 2003 Remaster feat. Friend"),
            "yesterday"
        );
    }

    // ──────── normalize_artist ────────

    #[test]
    fn normalize_artist_lowercases_and_trims() {
        assert_eq!(normalize_artist("  The Beatles  "), "the beatles");
    }

    #[test]
    fn normalize_artist_strips_diacritics() {
        assert_eq!(normalize_artist("Mägo de Oz"), "mago de oz");
    }

    #[test]
    fn normalize_artist_keeps_parens() {
        // "Foo (band)" must NOT collapse to "Foo" — that's title behavior, not artist.
        assert_eq!(normalize_artist("Foo (band)"), "foo (band)");
    }

    // ──────── token_set_ratio ────────

    #[test]
    fn token_set_ratio_identical_returns_one() {
        assert!((token_set_ratio("yesterday", "yesterday") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn token_set_ratio_subset_returns_one() {
        // RapidFuzz behavior: when one string's tokens are a subset of the
        // other, t1 == t2 (the smaller side), so max similarity is 1.0.
        let s = token_set_ratio("yesterday", "yesterday live wembley");
        assert!(s >= 0.95, "expected >= 0.95, got {}", s);
    }

    #[test]
    fn token_set_ratio_disjoint_returns_low() {
        let s = token_set_ratio("yesterday", "tomorrow");
        assert!(s < 0.50, "expected < 0.50, got {}", s);
    }

    #[test]
    fn token_set_ratio_overlap_passes_threshold() {
        // Three of four words shared after normalization.
        let s = token_set_ratio("song of the south", "song of the north");
        assert!(s >= SIMILARITY_THRESHOLD, "expected >= 0.80, got {}", s);
    }

    #[test]
    fn token_set_ratio_unrelated_fails_threshold() {
        let s = token_set_ratio("a totally different song", "yesterday");
        assert!(s < SIMILARITY_THRESHOLD, "expected < 0.80, got {}", s);
    }

    #[test]
    fn token_set_ratio_empty_inputs_safe() {
        // Both empty → defined as 1.0 (vacuously identical).
        assert!((token_set_ratio("", "") - 1.0).abs() < 1e-6);
        // One empty → 0.0 (no overlap).
        assert!(token_set_ratio("", "yesterday") < 1e-6);
    }
}
