//! MARK: - Localization
//!
//! Custom runtime localization: one key => string table per language,
//! instant language switching, fallback to English, then to the raw key. Contributors add a
//! language by copying en.rs, translating the values and adding it to `AVAILABLE_LANGUAGES`
//! and `TABLES`.

mod en;
mod ru;

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A UI language: code + native name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
}

/// Available languages, in the order of `TABLES`. English is first: it is the fallback.
pub const AVAILABLE_LANGUAGES: [Language; 2] = [
    Language {
        code: "en",
        name: "English",
    },
    Language {
        code: "ru",
        name: "Русский",
    },
];

static TABLES: LazyLock<[HashMap<&'static str, &'static str>; 2]> = LazyLock::new(|| {
    [
        en::STRINGS.iter().copied().collect(),
        ru::STRINGS.iter().copied().collect(),
    ]
});

/// Index of the current language in `AVAILABLE_LANGUAGES`.
static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn language_index(code: &str) -> Option<usize> {
    AVAILABLE_LANGUAGES
        .iter()
        .position(|language| language.code == code)
}

/// Translate a key in the given language. Falls back to English, then returns the key itself.
pub fn lookup<'a>(language: &str, key: &'a str) -> &'a str {
    let tables = &*TABLES;
    language_index(language)
        .and_then(|index| tables[index].get(key))
        .or_else(|| tables[0].get(key))
        .copied()
        .unwrap_or(key)
}

/// Translate a key in the current language.
pub fn t(key: &str) -> &str {
    lookup(current_language(), key)
}

/// Currently active language code.
pub fn current_language() -> &'static str {
    AVAILABLE_LANGUAGES
        .get(CURRENT.load(Ordering::Relaxed))
        .map_or("en", |language| language.code)
}

/// Switches the UI language. Unknown codes are ignored (returns false).
pub fn set_current_language(code: &str) -> bool {
    match language_index(code) {
        Some(index) => {
            CURRENT.store(index, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// Fills the placeholder of a localized string: `%@` for text, `%d` for numbers.
pub fn fill(template: &str, value: &str) -> String {
    template.replace("%@", value).replace("%d", value)
}

/// Initial language when nothing is saved: Russian if any of the user's preferred languages is
/// Russian (same rule as the macOS app), otherwise English.
pub fn detect_default_language<'a>(
    preferred_languages: impl IntoIterator<Item = &'a str>,
) -> &'static str {
    let is_russian = |language: &str| {
        language
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("ru"))
    };
    if preferred_languages.into_iter().any(is_russian) {
        "ru"
    } else {
        "en"
    }
}

#[cfg(test)]
mod tests {
    //! Test suite.

    use super::*;
    use std::collections::HashSet;

    fn keys(strings: &[(&'static str, &'static str)]) -> HashSet<&'static str> {
        strings.iter().map(|(key, _)| *key).collect()
    }

    #[test]
    fn english_and_russian_have_the_same_keys() {
        let en = keys(en::STRINGS);
        let ru = keys(ru::STRINGS);
        assert_eq!(en.difference(&ru).collect::<Vec<_>>(), Vec::<&&str>::new());
        assert_eq!(ru.difference(&en).collect::<Vec<_>>(), Vec::<&&str>::new());
    }

    #[test]
    fn keys_are_not_duplicated() {
        assert_eq!(keys(en::STRINGS).len(), en::STRINGS.len());
        assert_eq!(keys(ru::STRINGS).len(), ru::STRINGS.len());
    }

    #[test]
    fn every_available_language_has_a_strings_table() {
        let codes: Vec<&str> = AVAILABLE_LANGUAGES
            .iter()
            .map(|language| language.code)
            .collect();
        assert_eq!(codes, ["en", "ru"]);
        assert_eq!(TABLES.len(), AVAILABLE_LANGUAGES.len());
    }

    #[test]
    fn strings_follow_typography_rules() {
        // Plain hyphens instead of dashes, straight quotes, "=>" instead of arrows
        let forbidden = ['—', '–', '“', '”', '«', '»', '→'];
        for (key, value) in en::STRINGS.iter().chain(ru::STRINGS) {
            assert!(
                !value.contains(forbidden),
                "'{key}' contains a forbidden character: {value}"
            );
        }
    }

    #[test]
    fn lookup_falls_back_to_english_then_key() {
        assert_eq!(lookup("xx", "menu.settings"), "Settings...");
        assert_eq!(lookup("ru", "missing.key"), "missing.key");
    }

    #[test]
    fn changing_language_switches_strings_and_unknown_language_is_ignored() {
        // The only test that touches the global language
        assert!(set_current_language("ru"));
        assert_eq!(t("menu.settings"), "Настройки...");
        assert!(!set_current_language("xx"));
        assert_eq!(current_language(), "ru");
        assert!(set_current_language("en"));
        assert_eq!(t("menu.settings"), "Settings...");
    }

    #[test]
    fn default_language_is_russian_if_any_preferred_language_is_russian() {
        assert_eq!(detect_default_language(["en-US", "ru"]), "ru");
        assert_eq!(detect_default_language(["ru-RU"]), "ru");
        assert_eq!(detect_default_language(["en-US", "de-DE"]), "en");
        assert_eq!(detect_default_language([]), "en");
    }

    #[test]
    fn placeholders_are_filled() {
        assert_eq!(
            fill(lookup("en", "layouts.count"), "2"),
            "2 layouts detected"
        );
        assert_eq!(fill(lookup("ru", "about.version"), "0.1.0"), "Версия 0.1.0");
    }
}
