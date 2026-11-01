//! MARK: - Layout Character Maps
//!
//! Standard QWERTY <-> various layout mappings for physical key positions. The layout strings
//! match the PC layouts that ship with Windows.

use std::collections::HashMap;
use std::sync::LazyLock;

/// The 47 character keys of a US QWERTY keyboard: unshifted characters, then shifted ones.
pub const QWERTY: &str = "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:\"ZXCVBNM<>?";

/// Standard US QWERTY keyboard.
pub static QWERTY_US: LazyLock<CharacterMap> = LazyLock::new(|| CharacterMap::build(QWERTY));

/// Russian (standard) keyboard - mapped to the same physical keys as US QWERTY.
pub static RUSSIAN: LazyLock<CharacterMap> = LazyLock::new(|| {
    CharacterMap::build(
        "ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю.Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,",
    )
});

/// German QWERTZ.
pub static GERMAN: LazyLock<CharacterMap> = LazyLock::new(|| {
    CharacterMap::build(
        "^1234567890ß´qwertzuiopü+#asdfghjklöäyxcvbnm,.-°!\"§$%&/()=?`QWERTZUIOPÜ*'ASDFGHJKLÖÄYXCVBNM;:_",
    )
});

/// French AZERTY.
pub static FRENCH: LazyLock<CharacterMap> = LazyLock::new(|| {
    CharacterMap::build(
        "²&é\"'(-è_çà)=azertyuiop^$*qsdfghjklmùwxcvbn,;:!³1234567890°+AZERTYUIOP¨£µQSDFGHJKLM%WXCVBN?./§",
    )
});

/// Ukrainian.
pub static UKRAINIAN: LazyLock<CharacterMap> = LazyLock::new(|| {
    CharacterMap::build(
        "'1234567890-=йцукенгшщзхї\\фівапролджєячсмитьбю.₴!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЇ/ФІВАПРОЛДЖЄЯЧСМИТЬБЮ,",
    )
});

/// Spanish.
pub static SPANISH: LazyLock<CharacterMap> = LazyLock::new(|| {
    CharacterMap::build(
        "º1234567890'¡qwertyuiop`+çasdfghjklñ´zxcvbnm,.-ª!\"·$%&/()=?¿QWERTYUIOP^*ÇASDFGHJKLÑ¨ZXCVBNM;:_",
    )
});

/// Windows KLID => character map. Replaces the substring patterns of the macOS app
/// ("russian", "us", ...): a KLID identifies the physical layout exactly, so no ordering tricks
/// are needed.
static MAPS_BY_LAYOUT_ID: [(&str, &LazyLock<CharacterMap>); 14] = [
    ("00000409", &QWERTY_US), // US
    ("00020409", &QWERTY_US), // United States-International
    ("00000809", &QWERTY_US), // United Kingdom - close enough for conversion
    ("00000452", &QWERTY_US), // United Kingdom Extended
    ("00000419", &RUSSIAN),   // Russian
    ("00000422", &UKRAINIAN), // Ukrainian
    ("00020422", &UKRAINIAN), // Ukrainian (Enhanced)
    ("00000407", &GERMAN),    // German
    ("00010407", &GERMAN),    // German (IBM)
    ("00000807", &GERMAN),    // Swiss German
    ("0000040C", &FRENCH),    // French (Legacy, AZERTY)
    ("0001040C", &FRENCH),    // French (Standard, AZERTY)
    ("0000040A", &SPANISH),   // Spanish
    ("0001040A", &SPANISH),   // Spanish Variation
];

/// Character map for a layout identifier (KLID, case-insensitive), or `None` if the layout
/// is not supported.
pub fn character_map(layout_id: &str) -> Option<&'static CharacterMap> {
    MAPS_BY_LAYOUT_ID
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(layout_id))
        .map(|(_, map)| LazyLock::force(map))
}

/// All KLIDs that have a character map.
pub fn supported_layout_ids() -> impl Iterator<Item = &'static str> {
    MAPS_BY_LAYOUT_ID.iter().map(|(id, _)| *id)
}

/// Physical key (its US QWERTY character) => character of a layout, plus the reverse direction.
#[derive(Debug)]
pub struct CharacterMap {
    forward: HashMap<char, char>,
    /// Layout character => physical key. First wins on duplicates, in QWERTY order
    /// (no map has duplicate values - see the tests).
    reverse: HashMap<char, char>,
}

impl CharacterMap {
    fn build(layout: &str) -> Self {
        let mut forward = HashMap::with_capacity(QWERTY.len());
        let mut reverse = HashMap::with_capacity(QWERTY.len());
        for (physical_key, layout_char) in QWERTY.chars().zip(layout.chars()) {
            forward.insert(physical_key, layout_char);
            reverse.entry(layout_char).or_insert(physical_key);
        }
        Self { forward, reverse }
    }

    /// The character the physical key (given by its QWERTY character) types in this layout.
    pub fn get(&self, physical_key: char) -> Option<char> {
        self.forward.get(&physical_key).copied()
    }

    /// The physical key (its QWERTY character) that types `layout_char` in this layout.
    pub fn physical_key(&self, layout_char: char) -> Option<char> {
        self.reverse.get(&layout_char).copied()
    }

    /// True if some key of this layout types `layout_char`.
    pub fn types(&self, layout_char: char) -> bool {
        self.reverse.contains_key(&layout_char)
    }

    /// The characters of this layout.
    pub fn values(&self) -> impl Iterator<Item = char> + '_ {
        self.forward.values().copied()
    }

    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }
}

#[cfg(test)]
mod tests {
    //! Windows-specific guarantees around the character maps.

    use super::*;
    use std::collections::HashSet;

    // The macOS reverse map picks "first" in Dictionary iteration order, which is random per
    // process. That is only safe while no map has duplicate values - keep it that way.
    #[test]
    fn maps_have_no_duplicate_values() {
        for layout_id in supported_layout_ids() {
            let map = character_map(layout_id).unwrap();
            let unique: HashSet<char> = map.values().collect();
            assert_eq!(unique.len(), map.len(), "{layout_id} has duplicate values");
        }
    }

    #[test]
    fn maps_cover_all_physical_keys() {
        for layout_id in supported_layout_ids() {
            // 47 keys x (unshifted + shifted)
            assert_eq!(character_map(layout_id).unwrap().len(), 94, "{layout_id}");
        }
    }

    #[test]
    fn layout_ids_are_case_insensitive() {
        assert!(std::ptr::eq(character_map("0000040c").unwrap(), &*FRENCH));
    }

    #[test]
    fn german_variants_use_german_map() {
        for layout_id in ["00010407", "00000807"] {
            // German (IBM), Swiss German
            assert!(
                std::ptr::eq(character_map(layout_id).unwrap(), &*GERMAN),
                "{layout_id}"
            );
        }
    }

    #[test]
    fn non_matching_layouts_are_unsupported() {
        // US-Dvorak (not QWERTY, must not be treated as US), Russian - Mnemonic (different
        // letter positions), Chinese
        for layout_id in ["00010409", "00020419", "00000804"] {
            assert!(character_map(layout_id).is_none(), "{layout_id}");
        }
    }
}
