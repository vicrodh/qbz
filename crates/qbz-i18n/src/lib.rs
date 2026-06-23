//! qbz-i18n — frontend-agnostic gettext-style translation catalog.
//!
//! Reads the same gettext `.po` files Slint bundles, keyed by
//! `msgid = English source string` (no `msgctxt`). Reusable by any frontend
//! (Slint / TUI / headless) — no slint or tauri dependencies (ADR-006).

pub mod plural;
pub mod po;

pub use plural::PluralRule;
pub use po::Catalog;

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// Supported language codes, indexed by the value stored in [`CURRENT`].
const LANGS: [&str; 5] = ["en", "es", "de", "fr", "pt"];

/// Embedded `.po` sources. Path is relative to this file
/// (`crates/qbz-i18n/src/lib.rs`): `../` = `qbz-i18n/`, `../../` = `crates/`.
const PO_EN: &str = include_str!("../../qbz-slint/translations/en/LC_MESSAGES/qbz-slint.po");
const PO_ES: &str = include_str!("../../qbz-slint/translations/es/LC_MESSAGES/qbz-slint.po");
const PO_DE: &str = include_str!("../../qbz-slint/translations/de/LC_MESSAGES/qbz-slint.po");
const PO_FR: &str = include_str!("../../qbz-slint/translations/fr/LC_MESSAGES/qbz-slint.po");
const PO_PT: &str = include_str!("../../qbz-slint/translations/pt/LC_MESSAGES/qbz-slint.po");

/// Current language index (0=en, 1=es, 2=de, 3=fr, 4=pt). Defaults to en.
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Lazily-parsed catalogs, one slot per language.
static CATALOGS: [OnceLock<Catalog>; 5] = [
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
];

/// Map a language code to its index, if supported.
fn lang_index(lang: &str) -> Option<u8> {
    LANGS.iter().position(|&l| l == lang).map(|i| i as u8)
}

/// Get the parsed catalog for a language index, parsing on first use.
fn catalog(idx: u8) -> &'static Catalog {
    let idx = idx as usize;
    CATALOGS[idx].get_or_init(|| {
        let src = match idx {
            0 => PO_EN,
            1 => PO_ES,
            2 => PO_DE,
            3 => PO_FR,
            4 => PO_PT,
            _ => PO_EN,
        };
        Catalog::parse(LANGS[idx], src)
    })
}

/// The catalog for the current language.
fn current_catalog() -> &'static Catalog {
    catalog(CURRENT.load(Ordering::Relaxed))
}

/// Set the active language. Accepts `"en"|"es"|"de"|"fr"|"pt"`.
/// Unknown codes leave the current language unchanged.
pub fn set_language(lang: &str) {
    if let Some(idx) = lang_index(lang) {
        CURRENT.store(idx, Ordering::Relaxed);
    }
}

/// The currently active language code.
pub fn current_language() -> &'static str {
    LANGS[CURRENT.load(Ordering::Relaxed) as usize]
}

/// Resolve the desired language from the environment: `$LC_MESSAGES` then
/// `$LANG`. The 2-letter prefix is mapped to a supported language; otherwise
/// `"en"`. This does NOT mutate [`CURRENT`] — the caller decides.
pub fn resolve_auto() -> &'static str {
    let raw = std::env::var("LC_MESSAGES")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_default();

    let prefix: String = raw
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_lowercase();

    match lang_index(&prefix) {
        Some(idx) => LANGS[idx as usize],
        None => "en",
    }
}

/// Translate `msgid` (singular) in the current language.
/// Falls back to the English `msgid` itself when untranslated.
pub fn t(msgid: &str) -> String {
    current_catalog()
        .get(msgid)
        .map(|s| s.to_string())
        .unwrap_or_else(|| msgid.to_string())
}

/// Translate a plural form for count `n` in the current language.
/// Falls back to the English `singular`/`plural` (`if n==1`) when untranslated.
pub fn tn(singular: &str, plural: &str, n: i64) -> String {
    let cat = current_catalog();
    let form = cat.plural_rule().index(n);
    if let Some(translated) = cat.get_plural(singular, form) {
        return translated.to_string();
    }
    if n == 1 {
        singular.to_string()
    } else {
        plural.to_string()
    }
}

/// [`t`] then substitute `{}` placeholders left-to-right with `args`.
pub fn t_args(msgid: &str, args: &[&str]) -> String {
    substitute(&t(msgid), args)
}

/// [`tn`] then substitute `{}` placeholders left-to-right with `args`.
pub fn tf(singular: &str, plural: &str, n: i64, args: &[&str]) -> String {
    substitute(&tn(singular, plural, n), args)
}

/// Replace each `{}` with the next arg, left-to-right. Extra `{}` or extra
/// args are left untouched / ignored respectively.
fn substitute(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut args_iter = args.iter();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'}') {
            chars.next(); // consume '}'
            match args_iter.next() {
                Some(arg) => out.push_str(arg),
                None => out.push_str("{}"),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Language state is global; serialize tests that mutate it.
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn embedded_catalogs_parse() {
        let _g = LOCK.lock().unwrap();
        // Header-only catalogs still parse and expose nplurals.
        assert_eq!(catalog(0).nplurals(), 2); // en
        assert_eq!(catalog(3).nplurals(), 2); // fr
        // fr uses (n > 1): 1 -> form 0, 2 -> form 1.
        assert_eq!(catalog(3).plural_rule().index(1), 0);
        assert_eq!(catalog(3).plural_rule().index(2), 1);
    }

    #[test]
    fn language_switch_changes_current() {
        let _g = LOCK.lock().unwrap();
        set_language("en");
        assert_eq!(current_language(), "en");
        set_language("fr");
        assert_eq!(current_language(), "fr");
        // Unknown code leaves it unchanged.
        set_language("zz");
        assert_eq!(current_language(), "fr");
        set_language("en");
    }

    #[test]
    fn t_falls_back_to_msgid() {
        let _g = LOCK.lock().unwrap();
        set_language("en");
        // Bundled en catalog has no message entries yet → identity fallback.
        assert_eq!(t("Play"), "Play");
        assert_eq!(t("Some Untranslated String"), "Some Untranslated String");
    }

    #[test]
    fn tn_english_fallback_by_count() {
        let _g = LOCK.lock().unwrap();
        set_language("en");
        assert_eq!(tn("{} track", "{} tracks", 1), "{} track");
        assert_eq!(tn("{} track", "{} tracks", 0), "{} tracks");
        assert_eq!(tn("{} track", "{} tracks", 3), "{} tracks");
    }

    #[test]
    fn t_args_substitutes_placeholders() {
        let _g = LOCK.lock().unwrap();
        set_language("en");
        assert_eq!(t_args("Hi {}", &["x"]), "Hi x");
        assert_eq!(t_args("{} of {}", &["3", "10"]), "3 of 10");
    }

    #[test]
    fn tf_substitutes_after_plural() {
        let _g = LOCK.lock().unwrap();
        set_language("en");
        assert_eq!(tf("{} track", "{} tracks", 1, &["1"]), "1 track");
        assert_eq!(tf("{} track", "{} tracks", 3, &["3"]), "3 tracks");
    }

    #[test]
    fn substitute_handles_missing_and_extra_args() {
        // No language state touched, but keep ordering deterministic anyway.
        let _g = LOCK.lock().unwrap();
        assert_eq!(substitute("a {} b {}", &["1"]), "a 1 b {}");
        assert_eq!(substitute("only {}", &["1", "2"]), "only 1");
    }

    #[test]
    fn resolve_auto_maps_prefix() {
        let _g = LOCK.lock().unwrap();
        std::env::remove_var("LC_MESSAGES");
        std::env::set_var("LANG", "fr_FR.UTF-8");
        assert_eq!(resolve_auto(), "fr");
        std::env::set_var("LANG", "xx_XX");
        assert_eq!(resolve_auto(), "en");
        std::env::set_var("LC_MESSAGES", "de_DE.UTF-8");
        assert_eq!(resolve_auto(), "de");
        std::env::remove_var("LC_MESSAGES");
        std::env::remove_var("LANG");
    }
}
