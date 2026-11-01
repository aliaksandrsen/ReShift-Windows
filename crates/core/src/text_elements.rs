//! MARK: - Extended Grapheme Cluster Semantics
//!
//! Iterates extended grapheme clusters and compares them with canonical Unicode equivalence.
//! Handles decomposed letters, emoji and "\r\n" correctly.

use unicode_normalization::UnicodeNormalization;
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};
use unicode_segmentation::{GraphemeIndices, Graphemes, UnicodeSegmentation};

/// Grapheme clusters.
pub(crate) fn graphemes(text: &str) -> Graphemes<'_> {
    text.graphemes(true)
}

/// Grapheme clusters with their byte offsets.
pub(crate) fn grapheme_indices(text: &str) -> GraphemeIndices<'_> {
    text.grapheme_indices(true)
}

/// The single character used as a character-map key. Decomposed sequences are normalized to
/// NFC first for canonical equality.
pub(crate) fn map_key(element: &str) -> Option<char> {
    single_bmp_char(element.chars()).or_else(|| single_bmp_char(element.nfc()))
}

/// Checks if the element starts with a Unicode letter (general category L*).
pub(crate) fn is_letter_str(element: &str) -> bool {
    element.chars().next().is_some_and(is_letter)
}

/// General category L*.
pub(crate) fn is_letter(c: char) -> bool {
    c.general_category_group() == GeneralCategoryGroup::Letter
}

/// Checks if the element starts with a Unicode number (general category N*).
pub(crate) fn is_number_str(element: &str) -> bool {
    element
        .chars()
        .next()
        .is_some_and(|c| c.general_category_group() == GeneralCategoryGroup::Number)
}

/// Checks if the element is ASCII: a single ASCII scalar, or "\r\n".
pub(crate) fn is_ascii(element: &str) -> bool {
    element == "\r\n" || (element.len() == 1 && element.is_ascii())
}

/// One character that is a single UTF-16 unit.
fn single_bmp_char(mut chars: impl Iterator<Item = char>) -> Option<char> {
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.len_utf16() == 1 => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposed_letter_has_the_composed_map_key() {
        // "и" + U+0306 (combining breve) is canonically equal to "й"
        assert_eq!(map_key("\u{438}\u{306}"), Some('й'));
        assert_eq!(map_key("й"), Some('й'));
        assert_eq!(map_key("\u{1F44B}\u{1F3FD}"), None);
    }

    #[test]
    fn crlf_is_one_ascii_element() {
        assert_eq!(graphemes("a\r\nb").collect::<Vec<_>>(), ["a", "\r\n", "b"]);
        assert!(is_ascii("\r\n"));
        assert!(!is_ascii("й"));
    }

    #[test]
    fn letters_and_numbers_follow_general_categories() {
        assert!(is_letter_str("q") && is_letter_str("й") && is_letter_str("ß"));
        assert!(!is_letter_str("1") && !is_letter_str("?") && !is_letter_str(""));
        assert!(is_number_str("7") && is_number_str("²"));
        assert!(!is_number_str("x"));
    }
}
