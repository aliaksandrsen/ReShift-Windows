//! MARK: - Terminal support (Windows only)
//!
//! Terminals have no selectable input line, so the clipboard approach of the macOS app cannot work
//! there. Instead the characters of the current line are reconstructed from the physical keys as
//! they are typed (in memory only), and the converted text is retyped.

use crate::layout_char_map::character_map;
use crate::modes::SmartConversionMode;
use crate::text_converter::TextConverter;
use crate::text_elements::{graphemes, is_letter};

// MARK: - Physical Key Map

/// The US QWERTY characters printed on the 47 character keys, unshifted and shifted.
const UNSHIFTED: &str = "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./";
const SHIFTED: &str = "~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:\"ZXCVBNM<>?";
const SPACE_SCAN_CODE: u32 = 0x39;

/// Scan codes (set 1) of the 47 character keys, in the order of the strings above.
const SCAN_CODES: [u32; 47] = [
    0x29, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, //
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x2B, //
    0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, //
    0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35,
];

/// Physical key (scan code) => the US QWERTY characters printed on it (unshifted, shifted).
pub fn qwerty_characters(scan_code: u32) -> Option<(char, char)> {
    let index = SCAN_CODES.iter().position(|code| *code == scan_code)?;
    Some((UNSHIFTED.chars().nth(index)?, SHIFTED.chars().nth(index)?))
}

/// The character a physical key produces in a layout, using the same maps as the conversion.
/// `None` for keys outside the maps or layouts without a map.
pub fn typed_character(
    layout_id: &str,
    scan_code: u32,
    shift: bool,
    caps_lock: bool,
) -> Option<char> {
    if scan_code == SPACE_SCAN_CODE {
        return Some(' ');
    }
    let map = character_map(layout_id)?;
    let (unshifted, shifted) = qwerty_characters(scan_code)?;
    let plain = map.get(unshifted)?;
    let upper = map.get(shifted)?;
    let mut use_shifted = shift;
    // Caps Lock only affects letter keys whose shifted character is the capital letter
    // (so "ß" on the German keyboard stays "ß", while "[" on the Russian one becomes "Х")
    if caps_lock && is_letter(plain) && single_uppercase(plain) == Some(upper) {
        use_shifted = !use_shifted;
    }
    Some(if use_shifted { upper } else { plain })
}

/// The uppercase form of a character if it is a single character (`char.ToUpperInvariant`).
fn single_uppercase(c: char) -> Option<char> {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => Some(u),
        _ => None,
    }
}

// MARK: - Typed Key Classifier

/// What a key press does to the reconstructed input line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypedKeyAction {
    Ignore,
    /// A character typed by a key.
    Append(char),
    /// One UTF-16 unit of text injected as VK_PACKET that is half of a surrogate pair
    /// (emoji are injected as two packets).
    AppendUnit(u16),
    Backspace,
    Reset,
}

/// A key press as seen by the keyboard hook, with the state needed to interpret it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypedKey<'a> {
    pub virtual_key: u32,
    /// Scan code; for VK_PACKET (text injected by other tools) the UTF-16 unit.
    pub scan_code: u32,
    pub is_extended: bool,
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub win: bool,
    pub caps_lock: bool,
    /// The key is a dead key in the active layout (the character depends on the next key).
    pub is_dead_key: bool,
    /// KLID of the layout active in the window, or `None` if unknown.
    pub layout_id: Option<&'a str>,
}

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_CAPITAL: u32 = 0x14;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_PRIOR: u32 = 0x21;
const VK_HELP: u32 = 0x2F;
const VK_LWIN: u32 = 0x5B;
const VK_APPS: u32 = 0x5D;
const VK_NUMPAD0: u32 = 0x60;
const VK_NUMPAD9: u32 = 0x69;
const VK_MULTIPLY: u32 = 0x6A;
const VK_ADD: u32 = 0x6B;
const VK_SUBTRACT: u32 = 0x6D;
const VK_DIVIDE: u32 = 0x6F;
const VK_F1: u32 = 0x70;
const VK_F24: u32 = 0x87;
const VK_NUMLOCK: u32 = 0x90;
const VK_SCROLL: u32 = 0x91;
const VK_LSHIFT: u32 = 0xA0;
const VK_RMENU: u32 = 0xA5;
const VK_BROWSER_BACK: u32 = 0xA6;
const VK_LAUNCH_APP2: u32 = 0xB7;
pub const VK_PACKET: u32 = 0xE7;

/// Decides how a key press changes the typed line. Anything it cannot reproduce resets the line.
pub fn classify(key: &TypedKey<'_>) -> TypedKeyAction {
    let vk = key.virtual_key;

    // Text injected as Unicode (dictation tools, other utilities)
    if vk == VK_PACKET {
        let unit = (key.scan_code & 0xFFFF) as u16;
        return char::from_u32(u32::from(unit))
            .map_or(TypedKeyAction::AppendUnit(unit), TypedKeyAction::Append);
    }
    // Modifiers, lock keys, media and browser keys do not change the line
    if matches!(
        vk,
        VK_SHIFT | VK_CONTROL | VK_MENU | VK_CAPITAL | VK_NUMLOCK | VK_SCROLL
    ) || (VK_LSHIFT..=VK_RMENU).contains(&vk)
        || (VK_LWIN..=VK_LWIN + 1).contains(&vk)
        || (VK_BROWSER_BACK..=VK_LAUNCH_APP2).contains(&vk)
    {
        return TypedKeyAction::Ignore;
    }
    // Win+Space switches the layout - keep the line
    if key.win && vk == VK_SPACE {
        return TypedKeyAction::Ignore;
    }
    // Shortcuts may edit the line or move the cursor
    if key.control || key.alt || key.win {
        return TypedKeyAction::Reset;
    }
    if vk == VK_BACK {
        return TypedKeyAction::Backspace;
    }
    // Enter, Tab, Esc, navigation, Insert/Delete, function keys
    if matches!(vk, VK_RETURN | VK_TAB | VK_ESCAPE | VK_APPS)
        || (VK_PRIOR..=VK_HELP).contains(&vk)
        || (VK_F1..=VK_F24).contains(&vk)
    {
        return TypedKeyAction::Reset;
    }
    if key.is_dead_key {
        return TypedKeyAction::Reset;
    }
    if (VK_NUMPAD0..=VK_NUMPAD9).contains(&vk) {
        return char::from_digit(vk - VK_NUMPAD0, 10)
            .map_or(TypedKeyAction::Reset, TypedKeyAction::Append);
    }
    match vk {
        VK_MULTIPLY => return TypedKeyAction::Append('*'),
        VK_ADD => return TypedKeyAction::Append('+'),
        VK_SUBTRACT => return TypedKeyAction::Append('-'),
        VK_DIVIDE => return TypedKeyAction::Append('/'),
        _ => {}
    }
    let Some(layout_id) = key.layout_id.filter(|_| !key.is_extended) else {
        return TypedKeyAction::Reset;
    };
    typed_character(layout_id, key.scan_code, key.shift, key.caps_lock)
        .map_or(TypedKeyAction::Reset, TypedKeyAction::Append)
}

// MARK: - Typed Line Buffer

/// The current input line as typed (memory only, never logged or saved). Stored as UTF-16
/// code units.
/// Not synchronized: the owner wraps it in a lock (the hook thread appends, the conversion
/// thread reads and replaces).
#[derive(Debug)]
pub struct TypedLineBuffer {
    units: Vec<u16>,
}

impl Default for TypedLineBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl TypedLineBuffer {
    /// Max length in UTF-16 units; older characters are dropped.
    pub const MAX_LENGTH: usize = 512;

    pub fn new() -> Self {
        // Reserved up front: appending on the hook thread never reallocates
        Self {
            units: Vec::with_capacity(Self::MAX_LENGTH + 2),
        }
    }

    pub fn text(&self) -> String {
        String::from_utf16_lossy(&self.units)
    }

    /// Length in UTF-16 units (for diagnostics - the content itself is never logged).
    pub fn len(&self) -> usize {
        self.units.len()
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    pub fn apply(&mut self, action: TypedKeyAction) {
        match action {
            TypedKeyAction::Ignore => {}
            TypedKeyAction::Append(c) => self.push_units(c.encode_utf16(&mut [0; 2])),
            TypedKeyAction::AppendUnit(unit) => self.push_units(&[unit]),
            TypedKeyAction::Backspace => self.remove_last_character(),
            TypedKeyAction::Reset => self.units.clear(),
        }
    }

    pub fn append(&mut self, text: &str) {
        for c in text.chars() {
            self.apply(TypedKeyAction::Append(c));
        }
    }

    pub fn reset(&mut self) {
        self.units.clear();
    }

    /// After a conversion the line holds the converted text (pressing again converts back).
    pub fn replace(&mut self, text: &str) {
        self.units.clear();
        self.units.extend(text.encode_utf16());
    }

    fn push_units(&mut self, units: &[u16]) {
        self.units.extend_from_slice(units);
        if self.units.len() > Self::MAX_LENGTH {
            let excess = self.units.len() - Self::MAX_LENGTH;
            self.units.drain(..excess);
        }
    }

    /// Removes the last grapheme cluster (a whole emoji, a letter with its accents).
    fn remove_last_character(&mut self) {
        // Lossy decoding maps every unpaired surrogate to one U+FFFD, so UTF-16 lengths match
        let text = String::from_utf16_lossy(&self.units);
        if let Some(last) = graphemes(&text).next_back() {
            let new_len = self.units.len() - last.encode_utf16().count();
            self.units.truncate(new_len);
        }
    }
}

// MARK: - Terminal Line Converter

/// Keystrokes that turn the typed line into its converted form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalEdit {
    /// Backspaces to send (characters after the unchanged prefix).
    pub erase_count: usize,
    /// Text to type after erasing.
    pub insert: String,
    /// The whole line after the edit.
    pub new_line: String,
    /// Converted part before the edit (for the notification).
    pub input: String,
    /// Converted part after the edit.
    pub output: String,
    pub target_layout_id: String,
}

/// Applies the Smart Conversion mode to the typed line - the same rules as the clipboard path:
/// greedy line or last word. `None` when there is nothing to convert.
pub fn compute_terminal_edit(
    converter: &TextConverter<'_>,
    line: &str,
    mode: SmartConversionMode,
) -> Option<TerminalEdit> {
    match mode {
        SmartConversionMode::GreedyLine => {
            let result = converter.convert_line_greedy_with_info(line)?;
            diff(
                line,
                &result.text,
                line,
                &result.text,
                result.target_layout_id,
            )
        }
        SmartConversionMode::LastWord => {
            let (prefix, word) = split_last_word(line);
            if word.trim().is_empty() || !converter.looks_like_wrong_layout(word) {
                return None;
            }
            let info = converter.convert_selected_text_with_info(word)?;
            diff(
                line,
                &format!("{prefix}{}", info.text),
                word,
                &info.text,
                info.target_layout_id,
            )
        }
        SmartConversionMode::Disabled => None,
    }
}

/// Last whitespace-separated word plus trailing spaces. Unlike Ctrl+Shift+Left this keeps
/// punctuation inside the word: "hf,jnf" is one wrong-layout word ("работа").
fn split_last_word(line: &str) -> (&str, &str) {
    let end = line.trim_end().len();
    let start = line[..end]
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(index, c)| index + c.len_utf8());
    line.split_at(start)
}

/// Minimal edit: keep the common prefix, erase the rest, type the new tail.
fn diff(
    old_line: &str,
    new_line: &str,
    input: &str,
    output: &str,
    target_layout_id: String,
) -> Option<TerminalEdit> {
    let old_elements: Vec<&str> = graphemes(old_line).collect();
    let new_elements: Vec<&str> = graphemes(new_line).collect();
    let common = old_elements
        .iter()
        .zip(&new_elements)
        .take_while(|(old, new)| old == new)
        .count();
    let erase_count = old_elements.len() - common;
    let insert = new_elements[common..].concat();
    if erase_count == 0 && insert.is_empty() {
        return None;
    }
    Some(TerminalEdit {
        erase_count,
        insert,
        new_line: new_line.to_owned(),
        input: input.to_owned(),
        output: output.to_owned(),
        target_layout_id,
    })
}

#[cfg(test)]
mod tests {
    //! Test suite.

    use super::*;
    use crate::test_layouts::*;

    // Scan codes of a few physical keys
    const SCAN_G: u32 = 0x22;
    const SCAN_Q: u32 = 0x10;
    const SCAN_LEFT_BRACKET: u32 = 0x1A;
    const SCAN_ONE: u32 = 0x02;
    const SCAN_MINUS: u32 = 0x0C;
    const SCAN_SPACE: u32 = 0x39;

    fn key(vk: u32, scan: u32) -> TypedKey<'static> {
        TypedKey {
            virtual_key: vk,
            scan_code: scan,
            layout_id: Some(US),
            ..TypedKey::default()
        }
    }

    // MARK: - PhysicalKeyMap

    #[test]
    fn typed_character_follows_physical_keys() {
        let cases = [
            (US, SCAN_G, false, false, 'g'),
            (US, SCAN_G, true, false, 'G'),
            (RUSSIAN, SCAN_G, false, false, 'п'),
            (RUSSIAN, SCAN_G, true, false, 'П'),
            (RUSSIAN, SCAN_LEFT_BRACKET, false, false, 'х'),
            (US, SCAN_LEFT_BRACKET, false, true, '['), // Caps Lock does not touch punctuation
            (RUSSIAN, SCAN_LEFT_BRACKET, false, true, 'Х'), // ... but "х" is a letter key in Russian
            (RUSSIAN, SCAN_LEFT_BRACKET, true, true, 'х'),  // Shift + Caps Lock => lower case
            (US, SCAN_ONE, true, false, '!'),
            (RUSSIAN, SCAN_ONE, true, true, '!'),
            (GERMAN, SCAN_MINUS, false, true, 'ß'), // Shift+ß is "?", Caps Lock leaves "ß" alone
            (FRENCH, SCAN_Q, false, false, 'a'),    // AZERTY
            (US, SCAN_SPACE, false, false, ' '),
        ];
        for (layout, scan, shift, caps, expected) in cases {
            assert_eq!(
                typed_character(layout, scan, shift, caps),
                Some(expected),
                "{layout} scan {scan:#x} shift={shift} caps={caps}"
            );
        }
    }

    #[test]
    fn typed_character_is_null_for_unknown_keys_and_layouts() {
        assert_eq!(typed_character(US, 0x56, false, false), None); // ISO "<>" key
        assert_eq!(typed_character(JAPANESE, SCAN_G, false, false), None);
    }

    // MARK: - TypedKeyClassifier

    #[test]
    fn character_keys_append_the_typed_character() {
        let key = TypedKey {
            layout_id: Some(RUSSIAN),
            ..key(u32::from(b'G'), SCAN_G)
        };
        assert_eq!(classify(&key), TypedKeyAction::Append('п'));
    }

    #[test]
    fn injected_unicode_is_appended() {
        assert_eq!(
            classify(&key(VK_PACKET, u32::from('ж'))),
            TypedKeyAction::Append('ж')
        );
        // Half of a surrogate pair (emoji are injected as two packets)
        assert_eq!(
            classify(&key(VK_PACKET, 0xD83D)),
            TypedKeyAction::AppendUnit(0xD83D)
        );
    }

    #[test]
    fn modifiers_and_media_keys_are_ignored() {
        // Shift, Left Ctrl, Caps Lock, Volume up
        for vk in [0x10, 0xA2, 0x14, 0xAF] {
            assert_eq!(classify(&key(vk, 0)), TypedKeyAction::Ignore, "{vk:#x}");
        }
    }

    #[test]
    fn backspace_removes_the_last_character() {
        assert_eq!(classify(&key(0x08, 0)), TypedKeyAction::Backspace);
    }

    #[test]
    fn navigation_and_editing_keys_reset() {
        // Enter, Tab, Esc, Left, Home, Delete, F5
        for vk in [0x0D, 0x09, 0x1B, 0x25, 0x24, 0x2E, 0x74] {
            assert_eq!(classify(&key(vk, 0)), TypedKeyAction::Reset, "{vk:#x}");
        }
    }

    #[test]
    fn shortcuts_reset_but_win_space_keeps_the_line() {
        let ctrl_w = TypedKey {
            control: true,
            ..key(u32::from(b'W'), 0x11)
        };
        let alt_q = TypedKey {
            alt: true,
            ..key(u32::from(b'Q'), 0x10)
        };
        let win_space = TypedKey {
            win: true,
            ..key(0x20, SCAN_SPACE)
        };
        assert_eq!(classify(&ctrl_w), TypedKeyAction::Reset);
        assert_eq!(classify(&alt_q), TypedKeyAction::Reset);
        assert_eq!(classify(&win_space), TypedKeyAction::Ignore);
    }

    #[test]
    fn dead_keys_unknown_layouts_and_extended_keys_reset() {
        let dead = TypedKey {
            layout_id: Some(GERMAN),
            is_dead_key: true,
            ..key(0xDD, 0x0D)
        };
        let unknown = TypedKey {
            layout_id: None,
            ..key(u32::from(b'G'), SCAN_G)
        };
        let unsupported = TypedKey {
            layout_id: Some(JAPANESE),
            ..key(u32::from(b'G'), SCAN_G)
        };
        let extended = TypedKey {
            is_extended: true,
            ..key(0x30, 0x35)
        };
        for typed in [dead, unknown, unsupported, extended] {
            assert_eq!(classify(&typed), TypedKeyAction::Reset, "{typed:?}");
        }
    }

    #[test]
    fn numpad_digits_and_operators_are_appended() {
        assert_eq!(classify(&key(0x67, 0)), TypedKeyAction::Append('7'));
        assert_eq!(classify(&key(0x6B, 0)), TypedKeyAction::Append('+'));
    }

    // MARK: - TypedLineBuffer

    #[test]
    fn buffer_tracks_typing_backspace_and_reset() {
        let mut buffer = TypedLineBuffer::new();
        buffer.append("ghbdtnx");
        buffer.apply(TypedKeyAction::Backspace);
        assert_eq!(buffer.text(), "ghbdtn");

        buffer.append("\u{1F44B}\u{1F3FD}");
        buffer.apply(TypedKeyAction::Backspace); // removes the whole emoji
        assert_eq!(buffer.text(), "ghbdtn");

        buffer.apply(TypedKeyAction::Reset);
        assert_eq!(buffer.text(), "");
        buffer.apply(TypedKeyAction::Backspace); // nothing to remove
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn buffer_keeps_only_the_tail() {
        let mut buffer = TypedLineBuffer::new();
        buffer.append(&"a".repeat(TypedLineBuffer::MAX_LENGTH));
        buffer.append("bc");
        assert_eq!(buffer.len(), TypedLineBuffer::MAX_LENGTH);
        assert!(buffer.text().ends_with("bc"));
    }

    #[test]
    fn buffer_joins_injected_surrogate_halves() {
        let mut buffer = TypedLineBuffer::new();
        buffer.apply(TypedKeyAction::AppendUnit(0xD83D));
        buffer.apply(TypedKeyAction::AppendUnit(0xDC4B));
        assert_eq!(buffer.text(), "\u{1F44B}");
        buffer.apply(TypedKeyAction::Backspace);
        assert!(buffer.is_empty());
    }

    // MARK: - TerminalLineConverter

    fn compute(line: &str, mode: SmartConversionMode) -> Option<TerminalEdit> {
        let layouts = us_russian();
        compute_terminal_edit(&TextConverter::new(&layouts), line, mode)
    }

    #[test]
    fn greedy_line_retypes_the_whole_wrong_layout_line() {
        let edit = compute("ghbdtn rfr ltkf", SmartConversionMode::GreedyLine).unwrap();
        assert_eq!(edit.erase_count, 15);
        assert_eq!(edit.insert, "привет как дела");
        assert_eq!(edit.new_line, "привет как дела");
        assert_eq!(edit.target_layout_id, RUSSIAN);
    }

    #[test]
    fn last_word_keeps_the_rest_of_the_line() {
        let edit = compute("hello ntrcn", SmartConversionMode::LastWord).unwrap();
        assert_eq!(edit.erase_count, 5);
        assert_eq!(edit.insert, "текст");
        assert_eq!(edit.new_line, "hello текст");
        assert_eq!(edit.input, "ntrcn");
        assert_eq!(edit.output, "текст");
    }

    #[test]
    fn last_word_keeps_punctuation_inside_the_word() {
        let edit = compute("hf,jnf ", SmartConversionMode::LastWord).unwrap();
        assert_eq!(edit.new_line, "работа ");
        assert_eq!(edit.erase_count, 7); // the unchanged trailing space is retyped too
    }

    #[test]
    fn converting_twice_restores_the_original() {
        let first = compute("ghbdtn", SmartConversionMode::GreedyLine).unwrap();
        let second = compute(&first.new_line, SmartConversionMode::GreedyLine).unwrap();
        assert_eq!(second.new_line, "ghbdtn");
    }

    #[test]
    fn nothing_to_convert_returns_null() {
        let cases = [
            ("12345", SmartConversionMode::GreedyLine),
            ("", SmartConversionMode::GreedyLine),
            ("   ", SmartConversionMode::LastWord),
            ("ghbdtn", SmartConversionMode::Disabled),
        ];
        for (line, mode) in cases {
            assert_eq!(compute(line, mode), None, "'{line}' {mode:?}");
        }
    }

    #[test]
    fn split_last_word_keeps_trailing_spaces_with_the_word() {
        assert_eq!(split_last_word("hello ntrcn  "), ("hello ", "ntrcn  "));
        assert_eq!(split_last_word("ntrcn"), ("", "ntrcn"));
        assert_eq!(split_last_word("   "), ("", "   "));
    }
}
