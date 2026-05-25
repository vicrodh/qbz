//! Release-date label formatting for album cards (#469).
//!
//! Renders a Qobuz release date as a fixed "MMM D, YYYY" structure (e.g.
//! "Nov 6, 2025") with a *localized* abbreviated month. We deliberately keep
//! the month / day / year order fixed (not the locale-reordered output of a
//! `toLocaleDateString`-style call) but localize the month token, because the
//! app ships five UI languages (en / es / de / fr / pt). The Slint UI is
//! English-only during the migration, so [`current_locale`] returns English
//! today — it is the single place to wire the real UI language once Slint
//! gets translation support, and no caller needs to change when it does.

use chrono::{Locale, NaiveDate};

/// UI language used to render date labels. Slint is English-only for now;
/// this is the one hook to thread the persisted UI language through later
/// (the app supports en / es / de / fr / pt).
pub fn current_locale() -> Locale {
    Locale::en_US
}

/// Format a Qobuz release date string ("YYYY-MM-DD", possibly with a trailing
/// time component) as "MMM D, YYYY" with a localized month. Falls back to the
/// bare 4-digit year when only a year is available or the value cannot be
/// parsed as a full date, and to an empty string when there is no date.
pub fn release_label(date: Option<&str>) -> String {
    let Some(raw) = date else {
        return String::new();
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    // Only the leading YYYY-MM-DD matters; ignore any trailing time.
    let head = raw.get(0..10).unwrap_or(raw);
    if let Ok(parsed) = NaiveDate::parse_from_str(head, "%Y-%m-%d") {
        // %b = localized abbreviated month, %-d = day without leading zero.
        return parsed
            .format_localized("%b %-d, %Y", current_locale())
            .to_string();
    }
    // Year-only fallback (e.g. the source only had "2025").
    raw.get(0..4).unwrap_or_default().to_string()
}
