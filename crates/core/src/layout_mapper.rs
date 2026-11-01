//! MARK: - Layout Mapper
//!
//! Maps characters from one keyboard layout to another based on physical key positions.

use crate::layout_char_map::character_map;
use crate::text_elements::{graphemes, is_letter, is_letter_str, map_key};

/// Convert text typed in the source layout to what it would be in the target layout by mapping
/// each character through its physical key position. `None` if a layout has no character map.
pub fn convert(text: &str, source_layout_id: &str, target_layout_id: &str) -> Option<String> {
    let source_map = character_map(source_layout_id)?;
    let target_map = character_map(target_layout_id)?;

    // For each character in input:
    // 1. Find which physical key produced it in source layout (reverse lookup)
    // 2. Map that physical key to target layout character
    //
    // Punctuation preservation rule:
    // If the source char is NOT a letter and the converted result is also NOT a letter,
    // keep the original character. This handles cases like "?" -> "," where the user
    // intentionally typed "?" and it should stay "?", not become ",".
    // But ";" -> "ж" still converts because the target is a letter.
    let mut result = String::with_capacity(text.len() * 2);
    for element in graphemes(text) {
        let target_char = map_key(element)
            .and_then(|key| source_map.physical_key(key))
            .and_then(|physical_key| target_map.get(physical_key));
        match target_char {
            Some(target_char) if is_letter_str(element) || is_letter(target_char) => {
                result.push(target_char);
            }
            // Punctuation kept, or a character not in the mapping (e.g. space, emoji)
            _ => result.push_str(element),
        }
    }
    Some(result)
}

/// Try to detect which layout a text was likely typed in: the candidate whose character set
/// contains most of the characters. Ties go to the earlier candidate; `None` if nothing matches.
pub fn detect_source_layout<'a>(
    text: &str,
    candidate_layouts: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut best_match = None;
    let mut best_score = 0;
    for layout_id in candidate_layouts {
        let Some(map) = character_map(layout_id) else {
            continue;
        };
        let score = graphemes(text)
            .filter(|element| map_key(element).is_some_and(|key| map.types(key)))
            .count();
        if score > best_score {
            best_score = score;
            best_match = Some(layout_id);
        }
    }
    best_match
}

#[cfg(test)]
mod tests {
    //! Layout mapper test cases (58 cases) with Windows KLIDs.

    use super::*;
    use crate::test_layouts::*;

    fn convert_us_ru(text: &str) -> Option<String> {
        convert(text, US, RUSSIAN)
    }

    // MARK: - LayoutCharacterMap.characterMap(for:)

    #[test]
    fn character_map_matches_us_layout() {
        assert!(character_map(US).is_some());
    }

    #[test]
    fn character_map_matches_russian_layout() {
        assert!(character_map(RUSSIAN).is_some());
    }

    #[test]
    fn character_map_matches_ukrainian_layout() {
        assert!(character_map(UKRAINIAN).is_some());
    }

    #[test]
    fn character_map_matches_german_layout() {
        assert!(character_map(GERMAN).is_some());
    }

    #[test]
    fn character_map_matches_french_layout() {
        assert!(character_map(FRENCH).is_some());
    }

    #[test]
    fn character_map_matches_spanish_layout() {
        assert!(character_map(SPANISH).is_some());
    }

    #[test]
    fn character_map_matches_british_layout() {
        assert!(character_map(BRITISH).is_some());
    }

    // macOS: "ABC" layout. Windows counterpart: another QWERTY variant.
    #[test]
    fn character_map_matches_us_international_layout() {
        assert!(character_map(US_INTERNATIONAL).is_some());
    }

    #[test]
    fn character_map_returns_nil_for_unknown_layout() {
        assert!(character_map(JAPANESE).is_none());
    }

    // macOS: "russian" contains "us" - the pattern order was critical. KLIDs are matched exactly,
    // but keep the same assertions: Russian maps 'q' to 'й', US maps 'q' to itself.
    #[test]
    fn russian_does_not_match_us_map() {
        assert_eq!(character_map(RUSSIAN).unwrap().get('q'), Some('й'));
        assert_eq!(character_map(US).unwrap().get('q'), Some('q'));
    }

    // MARK: - LayoutMapper.convert() - EN => RU

    #[test]
    fn convert_en_to_ru_privet() {
        assert_eq!(convert_us_ru("ghbdtn").as_deref(), Some("привет"));
    }

    #[test]
    fn convert_en_to_ru_hello() {
        assert_eq!(convert_us_ru("hello").as_deref(), Some("руддщ"));
    }

    #[test]
    fn convert_en_to_ru_kak_dela() {
        assert_eq!(convert_us_ru("rfr ltkf").as_deref(), Some("как дела"));
    }

    #[test]
    fn convert_en_to_ru_drug() {
        assert_eq!(convert_us_ru("lheu").as_deref(), Some("друг"));
    }

    #[test]
    fn convert_en_to_ru_long_phrase() {
        assert_eq!(
            convert_us_ru("ghbdtn rfr ltkf lheu").as_deref(),
            Some("привет как дела друг")
        );
    }

    #[test]
    fn convert_en_to_ru_upper_case() {
        assert_eq!(convert_us_ru("GHBDTN").as_deref(), Some("ПРИВЕТ"));
    }

    #[test]
    fn convert_en_to_ru_mixed_case() {
        assert_eq!(convert_us_ru("Ghbdtn").as_deref(), Some("Привет"));
    }

    #[test]
    fn convert_en_to_ru_with_punctuation() {
        assert!(convert_us_ru("ghbdtn!").unwrap().starts_with("привет"));
    }

    #[test]
    fn convert_en_to_ru_numbers() {
        assert_eq!(convert_us_ru("123").as_deref(), Some("123"));
    }

    #[test]
    fn convert_en_to_ru_spaces_preserved() {
        assert_eq!(convert_us_ru("  ghbdtn  ").as_deref(), Some("  привет  "));
    }

    // MARK: - LayoutMapper.convert() - RU => EN

    #[test]
    fn convert_ru_to_en_rullo() {
        assert_eq!(convert("руддщ", RUSSIAN, US).as_deref(), Some("hello"));
    }

    #[test]
    fn convert_ru_to_en_privet() {
        assert_eq!(convert("привет", RUSSIAN, US).as_deref(), Some("ghbdtn"));
    }

    #[test]
    fn convert_ru_to_en_upper_case() {
        assert_eq!(convert("РУДДЩ", RUSSIAN, US).as_deref(), Some("HELLO"));
    }

    // MARK: - Round-trip tests (EN => RU => EN should return original)

    #[test]
    fn round_trip_en_to_ru_and_back() {
        let original = "ghbdtn rfr ltkf";
        let to_ru = convert(original, US, RUSSIAN).unwrap();
        assert_eq!(convert(&to_ru, RUSSIAN, US).as_deref(), Some(original));
    }

    #[test]
    fn round_trip_ru_to_en_and_back() {
        let original = "привет мир";
        let to_en = convert(original, RUSSIAN, US).unwrap();
        assert_eq!(convert(&to_en, US, RUSSIAN).as_deref(), Some(original));
    }

    // MARK: - Edge Cases

    #[test]
    fn convert_empty_string() {
        assert_eq!(convert_us_ru("").as_deref(), Some(""));
    }

    #[test]
    fn convert_single_character() {
        assert_eq!(convert_us_ru("q").as_deref(), Some("й"));
    }

    #[test]
    fn convert_only_spaces() {
        assert_eq!(convert_us_ru("   ").as_deref(), Some("   "));
    }

    #[test]
    fn convert_nil_for_unknown_source_layout() {
        assert_eq!(convert("test", JAPANESE, US), None);
    }

    #[test]
    fn convert_nil_for_unknown_target_layout() {
        assert_eq!(convert("test", US, JAPANESE), None);
    }

    // MARK: - German (QWERTZ) conversions

    #[test]
    fn convert_en_to_de_yz() {
        assert_eq!(convert("y", US, GERMAN).as_deref(), Some("z"));
    }

    #[test]
    fn convert_en_to_de_zy() {
        assert_eq!(convert("z", US, GERMAN).as_deref(), Some("y"));
    }

    // MARK: - French (AZERTY) conversions

    #[test]
    fn convert_en_to_fr_qwerty_to_azerty() {
        assert_eq!(convert("q", US, FRENCH).as_deref(), Some("a"));
    }

    #[test]
    fn convert_en_to_fr_a_to_q() {
        assert_eq!(convert("a", US, FRENCH).as_deref(), Some("q"));
    }

    // MARK: - Ukrainian conversions

    #[test]
    fn convert_en_to_uk_privet() {
        // In Ukrainian: g=>п, h=>р, b=>и, d=>в, s=>і, n=>т
        assert_eq!(convert("ghbdsn", US, UKRAINIAN).as_deref(), Some("привіт"));
    }

    // MARK: - detectSourceLayout

    #[test]
    fn detect_source_layout_cyrillic_text() {
        assert_eq!(detect_source_layout("привет", [US, RUSSIAN]), Some(RUSSIAN));
    }

    #[test]
    fn detect_source_layout_latin_text() {
        assert_eq!(detect_source_layout("hello", [US, RUSSIAN]), Some(US));
    }

    #[test]
    fn detect_source_layout_wrong_layout_latin() {
        assert_eq!(detect_source_layout("ghbdtn", [US, RUSSIAN]), Some(US));
    }

    #[test]
    fn detect_source_layout_wrong_layout_cyrillic() {
        assert_eq!(detect_source_layout("руддщ", [US, RUSSIAN]), Some(RUSSIAN));
    }

    #[test]
    fn detect_source_layout_empty_text() {
        // Implementation returns nil when bestScore is 0
        assert_eq!(detect_source_layout("", [US, RUSSIAN]), None);
    }

    #[test]
    fn detect_source_layout_single_candidate() {
        assert_eq!(detect_source_layout("hello", [US]), Some(US));
    }

    #[test]
    fn detect_source_layout_no_candidates() {
        assert_eq!(detect_source_layout("hello", []), None);
    }

    #[test]
    fn detect_source_layout_only_numbers() {
        // Numbers exist in both layouts - should pick one with higher score
        assert!(detect_source_layout("12345", [US, RUSSIAN]).is_some());
    }

    // MARK: - Large Dataset: EN => RU word pairs

    #[test]
    fn convert_en_to_ru_large_dataset() {
        let pairs = [
            ("ghbdtn", "привет"),
            ("rfr ltkf", "как дела"),
            ("lheu", "друг"),
            ("vbh", "мир"),
            ("cjkywt", "солнце"),
            ("rjvgm.nth", "компьютер"),
            ("ghjuhfvvf", "программа"),
            ("ntrcn", "текст"),
            ("rkfdbfnehf", "клавиатура"),
            ("hf,jnf", "работа"),
            ("ljv", "дом"),
            ("xfq", "чай"),
            ("rjit", "коше"), // коше through mapping
            ("cjj,otybt", "сообщение"),
            ("cnhjrf", "строка"),
            (",erdf", "буква"),
            ("zyfrjvsq", "янакомый"), // 'z' maps to 'я' on Russian keyboard, not 'з'
            ("cghfdjxybr", "справочник"),
        ];
        for (input, expected) in pairs {
            assert_eq!(convert_us_ru(input).as_deref(), Some(expected), "{input}");
        }
    }

    // MARK: - Large Dataset: RU => EN word pairs

    #[test]
    fn convert_ru_to_en_large_dataset() {
        let pairs = [
            ("руддщ", "hello"),
            ("цщкдв", "world"),
            ("еуыештп", "testing"), // 'п' (at 'g' key) not 'г'
            ("зкщпкфь", "program"),
            ("ьфсщы", "macos"),
            ("ызусшфд", "special"),
            ("лунищфкв", "keyboard"),
            ("дфнщге", "layout"),
            ("ыцшеср", "switch"),
            ("сщтмуке", "convert"),
        ];
        for (input, expected) in pairs {
            assert_eq!(
                convert(input, RUSSIAN, US).as_deref(),
                Some(expected),
                "{input}"
            );
        }
    }

    // MARK: - Special characters and symbols

    #[test]
    fn convert_special_chars_brackets() {
        // '[' in US QWERTY => 'х' in Russian
        assert_eq!(convert_us_ru("[").as_deref(), Some("х"));
    }

    #[test]
    fn convert_special_chars_semicolon() {
        // ';' in US QWERTY => 'ж' in Russian
        assert_eq!(convert_us_ru(";").as_deref(), Some("ж"));
    }

    #[test]
    fn convert_special_chars_apostrophe() {
        // '\'' in US QWERTY => 'э' in Russian
        assert_eq!(convert_us_ru("'").as_deref(), Some("э"));
    }

    #[test]
    fn convert_special_chars_backtick() {
        // '`' in US QWERTY => 'ё' in Russian (non-letter => letter = converts)
        assert_eq!(convert_us_ru("`").as_deref(), Some("ё"));
    }

    // MARK: - Punctuation preservation
    // Non-letter chars that map to non-letter chars are preserved as-is.
    // This prevents "?" becoming "," or "/" becoming "." during conversion.

    #[test]
    fn punctuation_preserved_question_mark() {
        assert_eq!(convert_us_ru("ghbdtn?").as_deref(), Some("привет?"));
    }

    #[test]
    fn punctuation_preserved_question_mark_alone() {
        assert_eq!(convert_us_ru("?").as_deref(), Some("?"));
    }

    #[test]
    fn punctuation_preserved_exclamation_mark() {
        assert_eq!(convert_us_ru("ghbdtn!").as_deref(), Some("привет!"));
    }

    #[test]
    fn punctuation_preserved_slash() {
        // '/' in QWERTY maps to '.' in Russian - both non-letter => preserve '/'
        assert_eq!(convert_us_ru("/").as_deref(), Some("/"));
    }

    #[test]
    fn punctuation_still_converts_semicolon_to_letter() {
        assert_eq!(convert_us_ru(";").as_deref(), Some("ж"));
    }

    #[test]
    fn punctuation_still_converts_comma_to_letter() {
        assert_eq!(convert_us_ru(",").as_deref(), Some("б"));
    }

    #[test]
    fn punctuation_still_converts_dot_to_letter() {
        assert_eq!(convert_us_ru(".").as_deref(), Some("ю"));
    }

    #[test]
    fn punctuation_preserved_full_phrase() {
        // "ghbdtn rfr ltkf?" should become "привет как дела?" (not "привет как дела,")
        assert_eq!(
            convert_us_ru("ghbdtn rfr ltkf?").as_deref(),
            Some("привет как дела?")
        );
    }

    #[test]
    fn punctuation_preserved_numbers_unchanged() {
        assert_eq!(convert_us_ru("123").as_deref(), Some("123"));
    }

    // MARK: - Unicode Character semantics

    #[test]
    fn decomposed_letter_is_mapped_with_canonical_equivalence() {
        // "и" + U+0306 (combining breve) is canonically equal to "й"
        assert_eq!(convert("\u{438}\u{306}", RUSSIAN, US).as_deref(), Some("q"));
    }

    #[test]
    fn emoji_passes_through_unchanged() {
        assert_eq!(
            convert_us_ru("ghbdtn \u{1F44B}\u{1F3FD}").as_deref(),
            Some("привет \u{1F44B}\u{1F3FD}")
        );
    }

    #[test]
    fn crlf_is_kept_as_one_character() {
        assert_eq!(
            convert_us_ru("ghbdtn\r\nvbh").as_deref(),
            Some("привет\r\nмир")
        );
    }
}
