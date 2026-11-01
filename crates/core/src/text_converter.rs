//! MARK: - Text Converter
//!
//! High-level service that orchestrates text conversion between the enabled layouts.

use crate::layout::KeyboardLayout;
use crate::layout_mapper::{convert, detect_source_layout};
use crate::text_elements::{grapheme_indices, graphemes, is_ascii, is_letter_str, is_number_str};

/// Result of a conversion: converted text + target layout ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversionResult {
    pub text: String,
    pub target_layout_id: String,
}

/// Boundary found by the greedy algorithm: prefix to keep unchanged, suffix to convert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrongLayoutBoundary {
    pub keep: String,
    pub convert: String,
}

/// Converts text between the enabled layouts (in the user's order).
pub struct TextConverter<'a> {
    layout_ids: Vec<&'a str>,
}

impl<'a> TextConverter<'a> {
    pub fn new(enabled_layouts: &'a [KeyboardLayout]) -> Self {
        Self {
            layout_ids: enabled_layouts
                .iter()
                .map(|layout| layout.id.as_str())
                .collect(),
        }
    }

    /// Convert selected text from one layout to another. Auto-detects the source layout and
    /// converts to the "other" layout.
    pub fn convert_selected_text(&self, text: &str) -> Option<String> {
        self.convert_selected_text_with_info(text)
            .map(|result| result.text)
    }

    /// Convert selected text and return both the result text and the target layout ID.
    pub fn convert_selected_text_with_info(&self, text: &str) -> Option<ConversionResult> {
        if self.layout_ids.len() < 2 {
            return None;
        }
        // Detect which layout the text was likely typed in
        let source_id = detect_source_layout(text, self.layout_ids.iter().copied())?;
        // Find the target layout (the "other" one); fall back to the first layout
        let target_id = self
            .layout_ids
            .iter()
            .copied()
            .find(|id| *id != source_id)
            .unwrap_or(self.layout_ids[0]);
        convert(text, source_id, target_id).map(|text| ConversionResult {
            text,
            target_layout_id: target_id.to_owned(),
        })
    }

    /// Convert text explicitly between two specified layouts.
    pub fn convert_text(&self, text: &str, source_id: &str, target_id: &str) -> Option<String> {
        convert(text, source_id, target_id)
    }

    /// Check if text looks like it was typed in the wrong keyboard layout, e.g. "ghbdtn" typed
    /// on QWERTY when meaning "привет" on the Russian layout: converting it switches its script.
    pub fn looks_like_wrong_layout(&self, text: &str) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() || self.layout_ids.len() < 2 {
            return false;
        }
        let Some(source_id) = detect_source_layout(trimmed, self.layout_ids.iter().copied()) else {
            return false;
        };

        // Try converting to each other layout and see if it "makes more sense"
        for target_id in self.layout_ids.iter().filter(|id| **id != source_id) {
            let Some(converted) = convert(trimmed, source_id, target_id) else {
                continue;
            };
            let source: Vec<&str> = graphemes(trimmed).collect();
            let target: Vec<&str> = graphemes(&converted).collect();

            let is_non_latin_letter = |e: &&str| !is_ascii(e) && is_letter_str(e);
            let source_has_latin_only = !source.iter().any(is_non_latin_letter);
            let converted_has_non_latin = target.iter().any(is_non_latin_letter);
            let source_has_non_latin = source.iter().any(is_non_latin_letter);
            let converted_has_latin_only = !target.iter().any(is_non_latin_letter);

            // Case 1: "ghbdtn" (all ASCII) -> "привет" (non-ASCII) = wrong layout
            // Case 2: "руддщ" (non-ASCII) -> "hello" (all ASCII) = wrong layout
            if (source_has_latin_only && converted_has_non_latin)
                || (source_has_non_latin && converted_has_latin_only)
            {
                return true;
            }
        }
        false
    }

    // MARK: - Greedy Phrase Conversion

    /// Given a line of text (from cursor to start of line), find the boundary where the wrong
    /// layout begins and return (prefix to keep unchanged, suffix to convert). `None` if no
    /// wrong-layout portion was found.
    ///
    /// Algorithm (two-pass):
    ///
    /// Pass 1 - Whole-line check: if the entire line converts to a different script (all
    /// Latin=>Cyrillic or all Cyrillic=>Latin), convert the whole thing. This handles the common
    /// case "ghbdtn rfr ltkf lheu" where every word is wrong-layout.
    ///
    /// Pass 2 - Right-to-left boundary scan: if only some words switch script, scan from right
    /// to left using the basic script-switch check (`looks_like_wrong_layout`). Stop at the first
    /// word that does NOT switch. This handles mixed lines like "Привет ghbdtn rfr".
    ///
    /// Key insight: per-word gibberish scoring doesn't work because many wrong-layout words
    /// (like "lheu" = "друг") look like valid English. When ALL words on a line switch script,
    /// that's a strong enough signal to convert everything.
    pub fn find_wrong_layout_boundary(&self, text: &str) -> Option<WrongLayoutBoundary> {
        if self.layout_ids.len() < 2 {
            return None;
        }
        // Tokenize: split into words and separators, preserving order and whitespace
        let tokens = tokenize(text);
        let word_count = tokens
            .iter()
            .filter(|token| !is_whitespace_or_punctuation(token))
            .count();
        if word_count == 0 {
            return None;
        }

        // --- Pass 1: Check if the whole line is wrong-layout ---
        let wrong_count = tokens
            .iter()
            .filter(|token| {
                !is_whitespace_or_punctuation(token) && self.looks_like_wrong_layout(token)
            })
            .count();
        let whole_line = || WrongLayoutBoundary {
            keep: String::new(),
            convert: text.to_owned(),
        };
        // If ALL words switch script, convert the whole line
        if wrong_count == word_count {
            return Some(whole_line());
        }
        // If most words switch (>=70% and at least 3 words), also convert the whole line.
        // This handles cases where one ambiguous word doesn't trip the check.
        if word_count >= 3 && wrong_count * 10 >= word_count * 7 {
            return Some(whole_line());
        }

        // --- Pass 2: Right-to-left scan to find boundary ---
        let mut wrong_start_index = None;
        for (index, token) in tokens.iter().enumerate().rev() {
            if is_whitespace_or_punctuation(token) {
                continue;
            }
            if self.looks_like_wrong_layout(token) {
                wrong_start_index = Some(index);
            } else {
                break;
            }
        }
        let wrong_start_index = wrong_start_index?;
        let keep = tokens[..wrong_start_index].concat();
        let convert = tokens[wrong_start_index..].concat();
        if convert.trim().is_empty() {
            return None;
        }
        Some(WrongLayoutBoundary { keep, convert })
    }

    /// Convert only the wrong-layout portion of a line. Returns the full replacement text
    /// (keep + converted) or `None` if there is nothing to convert.
    pub fn convert_line_greedy(&self, text: &str) -> Option<String> {
        self.convert_line_greedy_with_info(text)
            .map(|result| result.text)
    }

    /// Convert only the wrong-layout portion of a line, returning layout info.
    pub fn convert_line_greedy_with_info(&self, text: &str) -> Option<ConversionResult> {
        let boundary = self.find_wrong_layout_boundary(text)?;
        let info = self.convert_selected_text_with_info(&boundary.convert)?;
        Some(ConversionResult {
            text: boundary.keep + &info.text,
            target_layout_id: info.target_layout_id,
        })
    }
}

// MARK: - Tokenization

/// Split text into tokens: words and whitespace/punctuation runs. Preserves the original text
/// exactly (the concatenation of the tokens is the original text).
pub(crate) fn tokenize(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut in_word = false;
    for (index, element) in grapheme_indices(text) {
        let is_word_char = is_letter_str(element) || is_number_str(element);
        if index > start && is_word_char != in_word {
            tokens.push(&text[start..index]);
            start = index;
        }
        in_word = is_word_char;
    }
    if start < text.len() {
        tokens.push(&text[start..]);
    }
    tokens
}

/// True if the token contains only whitespace, punctuation, symbols - no letters/digits.
fn is_whitespace_or_punctuation(token: &str) -> bool {
    graphemes(token).all(|element| !is_letter_str(element) && !is_number_str(element))
}

#[cfg(test)]
mod tests {
    //! Text converter test cases (35 cases).

    use super::*;
    use crate::test_layouts::*;

    fn with_converter(test: impl FnOnce(&TextConverter)) {
        let layouts = us_russian();
        test(&TextConverter::new(&layouts));
    }

    fn assert_boundary(converter: &TextConverter, text: &str, keep: &str, convert: &str) {
        let boundary = converter.find_wrong_layout_boundary(text).unwrap();
        assert_eq!(boundary.keep, keep, "keep of '{text}'");
        assert_eq!(boundary.convert, convert, "convert of '{text}'");
    }

    // MARK: - convertSelectedText

    #[test]
    fn convert_selected_text_latin_to_russian() {
        with_converter(|c| {
            assert_eq!(c.convert_selected_text("ghbdtn").as_deref(), Some("привет"))
        });
    }

    #[test]
    fn convert_selected_text_russian_to_latin() {
        with_converter(|c| assert_eq!(c.convert_selected_text("руддщ").as_deref(), Some("hello")));
    }

    #[test]
    fn convert_selected_text_multiple_words() {
        with_converter(|c| {
            assert_eq!(
                c.convert_selected_text("ghbdtn rfr ltkf").as_deref(),
                Some("привет как дела")
            );
        });
    }

    #[test]
    fn convert_selected_text_empty_string() {
        // Empty string has no characters to detect source layout => returns nil
        with_converter(|c| assert_eq!(c.convert_selected_text(""), None));
    }

    #[test]
    fn convert_selected_text_single_word() {
        with_converter(|c| assert_eq!(c.convert_selected_text("ntrcn").as_deref(), Some("текст")));
    }

    // MARK: - looksLikeWrongLayout

    #[test]
    fn looks_like_wrong_layout_latin_for_russian() {
        // "ghbdtn" is Latin text that should be Cyrillic when converted
        with_converter(|c| assert!(c.looks_like_wrong_layout("ghbdtn")));
    }

    #[test]
    fn looks_like_wrong_layout_cyrillic_for_latin() {
        // "руддщ" is Cyrillic that converts to "hello" (Latin)
        with_converter(|c| assert!(c.looks_like_wrong_layout("руддщ")));
    }

    #[test]
    fn looks_like_wrong_layout_correct_latin() {
        // Without a dictionary the algorithm can't tell "hello" from "ghbdtn": both are Latin
        // text that produces Cyrillic when converted. It relies on context (the user pressed
        // the hotkey).
        with_converter(|c| {
            assert!(
                c.looks_like_wrong_layout("hello"),
                "Any Latin text looks like wrong layout when RU is available (script switches)"
            );
        });
    }

    #[test]
    fn looks_like_wrong_layout_empty_string() {
        with_converter(|c| assert!(!c.looks_like_wrong_layout("")));
    }

    #[test]
    fn looks_like_wrong_layout_whitespace_only() {
        with_converter(|c| assert!(!c.looks_like_wrong_layout("   ")));
    }

    #[test]
    fn looks_like_wrong_layout_numbers_only() {
        // Numbers don't change script when converted
        with_converter(|c| assert!(!c.looks_like_wrong_layout("12345")));
    }

    #[test]
    fn looks_like_wrong_layout_special_chars_only() {
        with_converter(|c| assert!(!c.looks_like_wrong_layout("!@#$%")));
    }

    #[test]
    fn looks_like_wrong_layout_mixed_numbers_and_letters() {
        // "ghbdtn123" has Latin letters => converts to Cyrillic => wrong layout
        with_converter(|c| assert!(c.looks_like_wrong_layout("ghbdtn123")));
    }

    // MARK: - Tokenization (tested via findWrongLayoutBoundary)

    #[test]
    fn find_boundary_all_wrong_layout() {
        with_converter(|c| assert_boundary(c, "ghbdtn rfr ltkf", "", "ghbdtn rfr ltkf"));
    }

    #[test]
    fn find_boundary_mixed_line_cyrillic_then_latin() {
        // Both words trigger looksLikeWrongLayout (both scripts switch when converted).
        // Pass 1 sees ALL words as wrong => converts entire line.
        with_converter(|c| assert_boundary(c, "Привет ghbdtn", "", "Привет ghbdtn"));
    }

    #[test]
    fn find_boundary_mixed_line_multiple_cyrillic_then_latin() {
        with_converter(|c| {
            assert_boundary(c, "Привет мир ghbdtn rfr", "", "Привет мир ghbdtn rfr");
        });
    }

    #[test]
    fn find_boundary_empty_string() {
        with_converter(|c| assert_eq!(c.find_wrong_layout_boundary(""), None));
    }

    #[test]
    fn find_boundary_only_whitespace() {
        with_converter(|c| assert_eq!(c.find_wrong_layout_boundary("   "), None));
    }

    #[test]
    fn find_boundary_single_word_wrong() {
        with_converter(|c| assert_boundary(c, "ghbdtn", "", "ghbdtn"));
    }

    #[test]
    fn find_boundary_single_word_cyrillic_wrong() {
        // "руддщ" => "hello" - Cyrillic to Latin = wrong layout
        with_converter(|c| assert_boundary(c, "руддщ", "", "руддщ"));
    }

    // MARK: - convertLineGreedy

    #[test]
    fn convert_line_greedy_all_wrong_layout() {
        with_converter(|c| {
            assert_eq!(
                c.convert_line_greedy("ghbdtn rfr ltkf").as_deref(),
                Some("привет как дела")
            );
        });
    }

    #[test]
    fn convert_line_greedy_mixed_line() {
        // Both words look like wrong layout to the heuristic, so Pass 1 converts the entire
        // line as one unit via convertSelectedText.
        with_converter(|c| assert!(c.convert_line_greedy("Привет ghbdtn").is_some()));
    }

    #[test]
    fn convert_line_greedy_nothing_to_convert_numbers() {
        // Numbers don't look like wrong layout
        with_converter(|c| assert_eq!(c.convert_line_greedy("12345"), None));
    }

    #[test]
    fn convert_line_greedy_empty_string() {
        with_converter(|c| assert_eq!(c.convert_line_greedy(""), None));
    }

    #[test]
    fn convert_line_greedy_single_wrong_word() {
        with_converter(|c| assert_eq!(c.convert_line_greedy("ghbdtn").as_deref(), Some("привет")));
    }

    #[test]
    fn convert_line_greedy_cyrillic_wrong_to_latin() {
        with_converter(|c| assert_eq!(c.convert_line_greedy("руддщ").as_deref(), Some("hello")));
    }

    // MARK: - convertText (explicit layout pair)

    #[test]
    fn convert_text_explicit_pair() {
        with_converter(|c| {
            assert_eq!(
                c.convert_text("ghbdtn", US, RUSSIAN).as_deref(),
                Some("привет")
            );
        });
    }

    #[test]
    fn convert_text_unknown_layout() {
        with_converter(|c| assert_eq!(c.convert_text("test", JAPANESE, US), None));
    }

    // MARK: - Large dataset: looksLikeWrongLayout

    #[test]
    fn looks_like_wrong_layout_large_dataset() {
        // All these Latin strings should look like wrong layout (they convert to Cyrillic)
        let wrong_layout_strings = [
            "ghbdtn",
            "rfr ltkf",
            "lheu",
            "vbh",
            "cjkywt",
            "rjvgm.nth",
            "ghjuhfvvf",
            "ntrcn",
            "rkfdbfnehf",
            "hf,jnf",
            "ljv",
            "xfq",
            "cjj,otybt",
            "cnhjrf",
        ];
        with_converter(|c| {
            for text in wrong_layout_strings {
                assert!(
                    c.looks_like_wrong_layout(text),
                    "'{text}' should be detected as wrong layout"
                );
            }
        });
    }

    #[test]
    fn looks_like_wrong_layout_cyrillic_wrong_layout() {
        // All these Cyrillic strings should look like wrong layout (they convert to Latin)
        let wrong_layout_strings = [
            "руддщ",   // hello
            "цщкдв",   // world
            "еуыеште", // testing
            "зкщпкфь", // program
            "ьфсщы",   // macos
        ];
        with_converter(|c| {
            for text in wrong_layout_strings {
                assert!(
                    c.looks_like_wrong_layout(text),
                    "'{text}' should be detected as wrong layout"
                );
            }
        });
    }

    // MARK: - findWrongLayoutBoundary with punctuation

    #[test]
    fn find_boundary_with_trailing_punctuation() {
        // "ghbdtn" is a word token, "!" is punctuation token
        with_converter(|c| assert_boundary(c, "ghbdtn!", "", "ghbdtn!"));
    }

    #[test]
    fn find_boundary_with_leading_punctuation() {
        // "- ghbdtn" has 1 word token and punctuation/whitespace "- ".
        // 1/1 = 100% wrong => converts entire line.
        with_converter(|c| assert_boundary(c, "- ghbdtn", "", "- ghbdtn"));
    }

    // MARK: - Edge case: single character

    #[test]
    fn convert_selected_text_single_latin_char() {
        with_converter(|c| assert_eq!(c.convert_selected_text("q").as_deref(), Some("й")));
    }

    #[test]
    fn convert_selected_text_single_cyrillic_char() {
        with_converter(|c| assert_eq!(c.convert_selected_text("й").as_deref(), Some("q")));
    }

    // MARK: - Greedy with 70% threshold

    #[test]
    fn find_boundary_threshold_met() {
        // 3 Latin words + 1 numeric token: all 3 letter-words are wrong, "123" is a word token
        // that doesn't switch script => 3/4 = 75% >= 70% => convert all
        with_converter(|c| {
            assert_boundary(c, "ghbdtn rfr ltkf 123", "", "ghbdtn rfr ltkf 123");
        });
    }

    // MARK: - Windows port additions

    #[test]
    fn tokenize_keeps_original_text() {
        let text = "Привет, ghbdtn!  123 \u{1F44B}\u{1F3FD}\r\n";
        assert_eq!(tokenize(text).concat(), text);
        assert_eq!(
            tokenize("ab, cd"),
            ["ab", ", ", "cd"],
            "words and separator runs alternate"
        );
    }

    #[test]
    fn mixed_line_keeps_the_correct_prefix() {
        // Pass 2: "123 456" does not switch script, the Latin tail does
        with_converter(|c| assert_boundary(c, "123 456 ghbdtn", "123 456 ", "ghbdtn"));
    }

    #[test]
    fn fewer_than_two_layouts_convert_nothing() {
        let layouts = vec![KeyboardLayout::new(US, "US", "en")];
        let converter = TextConverter::new(&layouts);
        assert_eq!(converter.convert_selected_text("ghbdtn"), None);
        assert!(!converter.looks_like_wrong_layout("ghbdtn"));
        assert_eq!(converter.find_wrong_layout_boundary("ghbdtn"), None);
    }
}
