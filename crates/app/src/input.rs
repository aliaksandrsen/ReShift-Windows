//! MARK: - Keyboard Simulation
//!
//! SendInput-based key event simulation for ReShift on Windows.

use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MapVirtualKeyW,
    SendInput, VIRTUAL_KEY, VK_APPS, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_HOME,
    VK_INSERT, VK_LEFT, VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RCONTROL, VK_RIGHT, VK_RMENU,
    VK_RWIN, VK_SHIFT, VK_UP,
};

use crate::log::debug;

/// Tag on every injected event, so our own keyboard hook can tell our keystrokes apart from real
/// ones (and from other injectors such as Barrier or RDP, which must still work). "LSWC".
pub const EXTRA_INFO_MARKER: usize = 0x4C53_5743;

/// An unassigned key, pressed to mask the release of Alt/Win (AutoHotkey's "menu mask").
const VK_MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

const MODIFIER_KEYS: [VIRTUAL_KEY; 5] = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN];

/// Presses a chord atomically: modifiers down, key down, key up, modifiers up (reverse order).
/// One SendInput call, so real typing cannot interleave with it.
pub fn send_chord(key: VIRTUAL_KEY, modifiers: &[VIRTUAL_KEY]) {
    send(&chord_inputs(key, modifiers));
}

/// A full keystroke (key down + key up) without modifiers.
pub fn send_key(key: VIRTUAL_KEY) {
    send_chord(key, &[]);
}

/// Erases characters with Backspace and types text as Unicode (no layout involved) in one
/// SendInput batch - used for terminals, which have no selection to paste over.
pub fn send_backspaces_and_text(backspaces: usize, text: &str) {
    send(&backspaces_and_text_inputs(backspaces, text));
    debug!(
        "Posted {backspaces} backspaces and {} characters",
        text.chars().count()
    );
}

/// Injects a press of an unassigned key while Alt/Win are still held, so that their release
/// does not open the window menu or the Start menu.
pub fn send_menu_mask() {
    send_key(VK_MASK);
}

/// True if Shift, Ctrl, Alt or Win is physically held right now.
pub fn any_modifier_down() -> bool {
    MODIFIER_KEYS
        .iter()
        // SAFETY: GetAsyncKeyState has no preconditions
        .any(|key| unsafe { GetAsyncKeyState(i32::from(key.0)) } as u16 & 0x8000 != 0)
}

/// Waits until the user releases all modifiers (e.g. Ctrl+Alt of the hotkey). `sleep` is the
/// waiting primitive of the calling thread (the conversion thread keeps pumping messages).
pub fn wait_for_modifiers_released(timeout: Duration, sleep: impl Fn(Duration)) -> bool {
    let start = Instant::now();
    while any_modifier_down() {
        if start.elapsed() >= timeout {
            return false;
        }
        sleep(Duration::from_millis(10));
    }
    true
}

// MARK: - Input construction (unit-tested)

fn chord_inputs(key: VIRTUAL_KEY, modifiers: &[VIRTUAL_KEY]) -> Vec<INPUT> {
    let mut inputs = Vec::with_capacity(modifiers.len() * 2 + 2);
    inputs.extend(modifiers.iter().map(|modifier| key_input(*modifier, false)));
    inputs.push(key_input(key, false));
    inputs.push(key_input(key, true));
    inputs.extend(
        modifiers
            .iter()
            .rev()
            .map(|modifier| key_input(*modifier, true)),
    );
    inputs
}

fn backspaces_and_text_inputs(backspaces: usize, text: &str) -> Vec<INPUT> {
    let mut inputs = Vec::with_capacity(backspaces * 2 + text.len() * 2);
    for _ in 0..backspaces {
        inputs.push(key_input(VK_BACK, false));
        inputs.push(key_input(VK_BACK, true));
    }
    for unit in text.encode_utf16() {
        inputs.push(unicode_input(unit, false));
        inputs.push(unicode_input(unit, true));
    }
    inputs
}

fn key_input(key: VIRTUAL_KEY, key_up: bool) -> INPUT {
    let mut flags = if key_up {
        KEYEVENTF_KEYUP
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    if is_extended_key(key) {
        // Without this flag NumLock turns Shift+Left into Shift+Numpad4 (no selection) and
        // Insert into Numpad0
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    // SAFETY: MapVirtualKeyW has no preconditions
    let scan = unsafe { MapVirtualKeyW(u32::from(key.0), MAPVK_VK_TO_VSC) };
    keyboard_input(KEYBDINPUT {
        wVk: key,
        wScan: scan as u16,
        dwFlags: flags,
        time: 0,
        dwExtraInfo: EXTRA_INFO_MARKER,
    })
}

fn unicode_input(unit: u16, key_up: bool) -> INPUT {
    keyboard_input(KEYBDINPUT {
        wVk: VIRTUAL_KEY(0),
        wScan: unit,
        dwFlags: if key_up {
            KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
        } else {
            KEYEVENTF_UNICODE
        },
        time: 0,
        dwExtraInfo: EXTRA_INFO_MARKER,
    })
}

fn keyboard_input(ki: KEYBDINPUT) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki },
    }
}

fn is_extended_key(key: VIRTUAL_KEY) -> bool {
    matches!(
        key,
        VK_LEFT
            | VK_RIGHT
            | VK_UP
            | VK_DOWN
            | VK_HOME
            | VK_END
            | VK_PRIOR
            | VK_NEXT
            | VK_INSERT
            | VK_DELETE
            | VK_RCONTROL
            | VK_RMENU
            | VK_LWIN
            | VK_RWIN
            | VK_APPS
    )
}

fn send(inputs: &[INPUT]) {
    // SAFETY: the slice holds initialized INPUT structures of the size passed along
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        debug!("ERROR: SendInput sent {sent}/{} events", inputs.len());
    }
}

#[cfg(test)]
mod tests {
    //! Only builds the inputs - nothing is ever sent from the tests.

    use super::*;

    fn keys(inputs: &[INPUT]) -> Vec<(u16, bool, bool)> {
        inputs
            .iter()
            .map(|input| {
                // SAFETY: the tests only build keyboard inputs
                let ki = unsafe { input.Anonymous.ki };
                (
                    ki.wVk.0,
                    ki.dwFlags.contains(KEYEVENTF_KEYUP),
                    ki.dwFlags.contains(KEYEVENTF_EXTENDEDKEY),
                )
            })
            .collect()
    }

    #[test]
    fn chords_press_modifiers_around_the_key() {
        let inputs = chord_inputs(VK_LEFT, &[VK_CONTROL, VK_SHIFT]);
        assert_eq!(
            keys(&inputs),
            [
                (VK_CONTROL.0, false, false),
                (VK_SHIFT.0, false, false),
                (VK_LEFT.0, false, true), // extended: NumLock must not turn it into Numpad4
                (VK_LEFT.0, true, true),
                (VK_SHIFT.0, true, false),
                (VK_CONTROL.0, true, false),
            ]
        );
    }

    #[test]
    fn every_input_carries_our_marker() {
        let mut inputs = chord_inputs(VK_INSERT, &[VK_CONTROL]);
        inputs.extend(backspaces_and_text_inputs(1, "ж"));
        for input in &inputs {
            // SAFETY: keyboard inputs only
            assert_eq!(unsafe { input.Anonymous.ki }.dwExtraInfo, EXTRA_INFO_MARKER);
            assert_eq!(input.r#type, INPUT_KEYBOARD);
        }
    }

    #[test]
    fn text_is_typed_as_utf16_units() {
        let inputs = backspaces_and_text_inputs(2, "ж\u{1F44B}");
        // 2 backspaces + 3 UTF-16 units, each down + up
        assert_eq!(inputs.len(), 2 * 2 + 3 * 2);
        // SAFETY: keyboard inputs only
        let units: Vec<u16> = inputs[4..]
            .iter()
            .step_by(2)
            .map(|input| unsafe { input.Anonymous.ki }.wScan)
            .collect();
        assert_eq!(units, "ж\u{1F44B}".encode_utf16().collect::<Vec<_>>());
    }
}
