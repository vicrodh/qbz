//! Minimal gettext `Plural-Forms` evaluator.
//!
//! We only need to support the two expressions our locales actually use:
//!   - `nplurals=2; plural=(n != 1);`  (en, es, de, pt)
//!   - `nplurals=2; plural=(n > 1);`   (fr)
//! Anything unrecognized falls back to the English default `if n==1 {0} else {1}`.

/// The plural-selection kind extracted from a `Plural-Forms` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `plural=(n != 1)` — index 0 when n == 1, else 1.
    NotOne,
    /// `plural=(n > 1)` — index 0 when n <= 1, else 1.
    GreaterThanOne,
}

/// A parsed gettext plural rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluralRule {
    nplurals: usize,
    kind: Kind,
}

impl Default for PluralRule {
    fn default() -> Self {
        PluralRule {
            nplurals: 2,
            kind: Kind::NotOne,
        }
    }
}

impl PluralRule {
    /// Parse a `Plural-Forms` header value (the part after `Plural-Forms:`).
    ///
    /// Accepts the full header line or just the value; tolerant of whitespace.
    /// Unknown plural expressions default to `(n != 1)` with `nplurals=2`.
    pub fn parse(plural_forms_header: &str) -> PluralRule {
        // Normalize: drop spaces so `n != 1` and `n!=1` both match.
        let normalized: String = plural_forms_header
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();

        let nplurals = parse_nplurals(&normalized).unwrap_or(2);

        let kind = if normalized.contains("plural=(n>1)") || normalized.contains("plural=n>1") {
            Kind::GreaterThanOne
        } else {
            // Default and explicit `(n != 1)` both land here.
            Kind::NotOne
        };

        PluralRule { nplurals, kind }
    }

    /// Number of plural forms (`nplurals`).
    pub fn nplurals(&self) -> usize {
        self.nplurals
    }

    /// Index of the plural form to use for count `n`.
    pub fn index(&self, n: i64) -> usize {
        match self.kind {
            Kind::NotOne => {
                if n == 1 {
                    0
                } else {
                    1
                }
            }
            Kind::GreaterThanOne => {
                if n > 1 {
                    1
                } else {
                    0
                }
            }
        }
    }
}

/// Extract the integer after `nplurals=` from a whitespace-stripped header.
fn parse_nplurals(normalized: &str) -> Option<usize> {
    let idx = normalized.find("nplurals=")? + "nplurals=".len();
    let rest = &normalized[idx..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nplurals_and_en_rule_index() {
        let rule = PluralRule::parse("nplurals=2; plural=(n != 1);");
        assert_eq!(rule.nplurals(), 2);
        // English / Spanish / German / Portuguese: 1 is singular.
        assert_eq!(rule.index(0), 1);
        assert_eq!(rule.index(1), 0);
        assert_eq!(rule.index(2), 1);
        assert_eq!(rule.index(5), 1);
    }

    #[test]
    fn parses_fr_greater_than_one_rule_index() {
        let rule = PluralRule::parse("nplurals=2; plural=(n > 1);");
        assert_eq!(rule.nplurals(), 2);
        // French: 0 and 1 are singular form.
        assert_eq!(rule.index(0), 0);
        assert_eq!(rule.index(1), 0);
        assert_eq!(rule.index(2), 1);
        assert_eq!(rule.index(10), 1);
    }

    #[test]
    fn unknown_expression_falls_back_to_default() {
        let rule = PluralRule::parse("garbage header with no plural expr");
        assert_eq!(rule.nplurals(), 2);
        assert_eq!(rule.index(1), 0);
        assert_eq!(rule.index(3), 1);
    }
}
