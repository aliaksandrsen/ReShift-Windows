//! Windows keyboard layout IDs (KLIDs) used by the tests.

use crate::layout::KeyboardLayout;

pub const US: &str = "00000409";
pub const US_INTERNATIONAL: &str = "00020409";
pub const BRITISH: &str = "00000809";
pub const RUSSIAN: &str = "00000419";
pub const UKRAINIAN: &str = "00000422";
pub const GERMAN: &str = "00000407";
pub const FRENCH: &str = "0000040C";
pub const SPANISH: &str = "0000040A";
pub const JAPANESE: &str = "00000411";

/// Deterministic [US, RU] pair - the macOS tests depended on the machine's layouts.
pub fn us_russian() -> Vec<KeyboardLayout> {
    vec![
        KeyboardLayout::new(US, "US", "en"),
        KeyboardLayout::new(RUSSIAN, "Russian", "ru"),
    ]
}
