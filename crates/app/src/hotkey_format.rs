//! MARK: - Display Helpers
//!
//! Windows counterpart of HotkeyManager.modifierFlagsToString / keyCodeToString
//!, plus the modifier flags of a custom hotkey.

/// Modifier flags of a custom hotkey - the values of RegisterHotKey's MOD_* constants
/// (stored as a number in settings.json).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HotkeyModifiers(pub u32);

impl HotkeyModifiers {
    pub const NONE: Self = Self(0);
    pub const ALT: Self = Self(0x1);
    pub const CONTROL: Self = Self(0x2);
    pub const SHIFT: Self = Self(0x4);
    pub const WIN: Self = Self(0x8);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0 && other.0 != 0
    }

    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for HotkeyModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Default custom hotkey: Ctrl+Alt+Space.
pub const DEFAULT_HOTKEY_KEY_CODE: u32 = VK_SPACE;
pub const DEFAULT_HOTKEY_MODIFIERS: HotkeyModifiers = HotkeyModifiers(0x2 | 0x1);

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_PAUSE: u32 = 0x13;
const VK_CAPITAL: u32 = 0x14;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_PRIOR: u32 = 0x21;
const VK_NEXT: u32 = 0x22;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_INSERT: u32 = 0x2D;
const VK_DELETE: u32 = 0x2E;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;
const VK_APPS: u32 = 0x5D;
const VK_F1: u32 = 0x70;
const VK_F24: u32 = 0x87;
const VK_LSHIFT: u32 = 0xA0;
const VK_RMENU: u32 = 0xA5;

fn key_name(virtual_key: u32) -> Option<&'static str> {
    Some(match virtual_key {
        VK_SPACE => "Space",
        VK_RETURN => "Enter",
        VK_TAB => "Tab",
        VK_BACK => "Backspace",
        VK_ESCAPE => "Esc",
        VK_INSERT => "Insert",
        VK_DELETE => "Delete",
        VK_HOME => "Home",
        VK_END => "End",
        VK_PRIOR => "PageUp",
        VK_NEXT => "PageDown",
        VK_LEFT => "Left",
        VK_RIGHT => "Right",
        VK_UP => "Up",
        VK_DOWN => "Down",
        VK_PAUSE => "Pause",
        VK_CAPITAL => "CapsLock",
        VK_APPS => "Menu",
        // OEM keys, named after the US layout like the macOS key table
        0xBA => ";",
        0xBB => "=",
        0xBC => ",",
        0xBD => "-",
        0xBE => ".",
        0xBF => "/",
        0xC0 => "`",
        0xDB => "[",
        0xDC => "\\",
        0xDD => "]",
        0xDE => "'",
        _ => return None,
    })
}

pub fn modifiers_to_string(modifiers: HotkeyModifiers) -> String {
    [
        (HotkeyModifiers::CONTROL, "Ctrl+"),
        (HotkeyModifiers::ALT, "Alt+"),
        (HotkeyModifiers::SHIFT, "Shift+"),
        (HotkeyModifiers::WIN, "Win+"),
    ]
    .into_iter()
    .filter(|(flag, _)| modifiers.contains(*flag))
    .map(|(_, name)| name)
    .collect()
}

pub fn key_to_string(virtual_key: u32) -> String {
    match virtual_key {
        0x41..=0x5A | 0x30..=0x39 => {
            char::from_u32(virtual_key).map_or_else(String::new, String::from)
        }
        VK_F1..=VK_F24 => format!("F{}", virtual_key - VK_F1 + 1),
        0x60..=0x69 => format!("Num{}", virtual_key - 0x60),
        _ => key_name(virtual_key).map_or_else(|| format!("Key{virtual_key}"), str::to_owned),
    }
}

/// e.g. "Ctrl+Alt+Space".
pub fn describe(virtual_key: u32, modifiers: HotkeyModifiers) -> String {
    modifiers_to_string(modifiers) + &key_to_string(virtual_key)
}

/// A custom hotkey needs Ctrl, Alt or Win: RegisterHotKey swallows the keystroke system-wide,
/// so Shift+letter would make capital letters impossible to type.
pub fn is_allowed(virtual_key: u32, modifiers: HotkeyModifiers) -> bool {
    modifiers.intersects(HotkeyModifiers::CONTROL | HotkeyModifiers::ALT | HotkeyModifiers::WIN)
        && !matches!(
            virtual_key,
            VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN | VK_RWIN | VK_LSHIFT..=VK_RMENU
        )
}

#[cfg(test)]
mod tests {
    //! Test suite. (hotkey part).

    use super::*;

    const CTRL: HotkeyModifiers = HotkeyModifiers::CONTROL;
    const ALT: HotkeyModifiers = HotkeyModifiers::ALT;
    const SHIFT: HotkeyModifiers = HotkeyModifiers::SHIFT;
    const WIN: HotkeyModifiers = HotkeyModifiers::WIN;

    #[test]
    fn describe_uses_windows_notation() {
        assert_eq!(describe(0x20, CTRL | ALT), "Ctrl+Alt+Space");
        assert_eq!(describe(u32::from(b'S'), ALT), "Alt+S");
        assert_eq!(describe(0x7B, CTRL | SHIFT | WIN), "Ctrl+Shift+Win+F12");
        assert_eq!(describe(0xBA, CTRL), "Ctrl+;");
        assert_eq!(describe(u32::from(b'7'), WIN), "Win+7");
        assert_eq!(describe(0x63, CTRL), "Ctrl+Num3");
        assert_eq!(describe(0xFF, CTRL), "Ctrl+Key255");
    }

    #[test]
    fn is_allowed_requires_ctrl_alt_or_win() {
        assert!(is_allowed(u32::from(b'S'), ALT));
        assert!(is_allowed(0x20, CTRL | ALT));
        assert!(!is_allowed(u32::from(b'S'), SHIFT)); // would break typing capitals
        assert!(!is_allowed(u32::from(b'S'), HotkeyModifiers::NONE));
        assert!(!is_allowed(0xA2, CTRL)); // modifier key alone
    }

    #[test]
    fn default_hotkey_is_ctrl_alt_space() {
        assert_eq!(
            describe(DEFAULT_HOTKEY_KEY_CODE, DEFAULT_HOTKEY_MODIFIERS),
            "Ctrl+Alt+Space"
        );
    }
}
