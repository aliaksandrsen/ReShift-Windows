//! MARK: - Double Shift Detector
//!
//! Port of HotkeyManager.handleFlagsChanged from the macOS app. A pure
//! state machine: the Windows low-level keyboard hook feeds it virtual-key events, tests feed it
//! directly. No allocations - it runs inside the hook callback.

/// Max time between the two Shift releases (macOS: doubleTapInterval = 0.4 s).
pub const DOUBLE_TAP_INTERVAL_MS: u64 = 400;

/// Device-independent modifier flags, like `NSEvent.ModifierFlags`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModifierFlags(u8);

impl ModifierFlags {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1);
    pub const CONTROL: Self = Self(2);
    pub const ALT: Self = Self(4);
    pub const WIN: Self = Self(8);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for ModifierFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for ModifierFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

// Windows virtual-key codes of the modifier keys (generic and left/right variants)
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;

#[derive(Debug, Default)]
pub struct DoubleShiftDetector {
    /// Modifier keys currently held, as a bit set of virtual-key codes.
    pressed_modifiers: [u64; 4],
    // Double-tap detection state
    last_shift_press_time: Option<u64>,
    shift_was_alone_down: bool,
}

impl DoubleShiftDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one key event. Returns true when a double Shift has just been detected.
    pub fn process_key(&mut self, virtual_key: u32, is_key_down: bool, timestamp_ms: u64) -> bool {
        if modifier_for(virtual_key) == ModifierFlags::NONE {
            // Any other key pressed resets shift-alone tracking (macOS keyDown monitor).
            // The timer is intentionally NOT reset - same as the macOS app.
            if is_key_down {
                self.shift_was_alone_down = false;
            }
            return false;
        }

        // Only real modifier state changes count, like NSEvent.flagsChanged.
        // Windows auto-repeats a held modifier key; macOS does not.
        let changed = self.set_pressed(virtual_key, is_key_down);
        changed && self.handle_flags_changed(self.current_flags(), timestamp_ms)
    }

    /// Exact port of `handleFlagsChanged(_:)`.
    pub fn handle_flags_changed(&mut self, flags: ModifierFlags, timestamp_ms: u64) -> bool {
        let shift_down = flags.contains(ModifierFlags::SHIFT);
        // Only Shift pressed, no other modifiers
        let only_shift = shift_down
            && !flags.intersects(ModifierFlags::CONTROL | ModifierFlags::ALT | ModifierFlags::WIN);

        if only_shift && !self.shift_was_alone_down {
            // Shift just pressed down (alone)
            self.shift_was_alone_down = true;
        } else if !shift_down && self.shift_was_alone_down {
            // Shift just released, and it was a "clean" press (no other keys pressed during)
            self.shift_was_alone_down = false;
            if let Some(last) = self.last_shift_press_time
                && timestamp_ms.saturating_sub(last) < DOUBLE_TAP_INTERVAL_MS
            {
                // Double Shift detected!
                self.last_shift_press_time = None;
                return true;
            }
            self.last_shift_press_time = Some(timestamp_ms);
        } else if !only_shift {
            // Some other modifier involved - reset
            self.shift_was_alone_down = false;
        }
        false
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn is_shift_key(virtual_key: u32) -> bool {
        modifier_for(virtual_key) == ModifierFlags::SHIFT
    }

    /// Marks a modifier key as pressed or released; true if its state changed.
    fn set_pressed(&mut self, virtual_key: u32, pressed: bool) -> bool {
        let (word, bit) = ((virtual_key as usize >> 6) & 3, 1u64 << (virtual_key & 63));
        let was_pressed = self.pressed_modifiers[word] & bit != 0;
        if pressed {
            self.pressed_modifiers[word] |= bit;
        } else {
            self.pressed_modifiers[word] &= !bit;
        }
        was_pressed != pressed
    }

    fn current_flags(&self) -> ModifierFlags {
        let mut flags = ModifierFlags::NONE;
        for (word, bits) in self.pressed_modifiers.iter().enumerate() {
            for bit in 0..64 {
                if bits & (1 << bit) != 0 {
                    flags |= modifier_for((word * 64 + bit) as u32);
                }
            }
        }
        flags
    }
}

fn modifier_for(virtual_key: u32) -> ModifierFlags {
    match virtual_key {
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => ModifierFlags::SHIFT,
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => ModifierFlags::CONTROL,
        VK_MENU | VK_LMENU | VK_RMENU => ModifierFlags::ALT,
        VK_LWIN | VK_RWIN => ModifierFlags::WIN,
        _ => ModifierFlags::NONE,
    }
}

#[cfg(test)]
mod tests {
    //! Test suite.

    use super::*;

    const L_SHIFT: u32 = 0xA0;
    const R_SHIFT: u32 = 0xA1;
    const L_CONTROL: u32 = 0xA2;
    const KEY_H: u32 = 0x48;
    const KEY_A: u32 = 0x41;

    /// Press and release a key; returns true if the release (or press) fired.
    fn tap(detector: &mut DoubleShiftDetector, vk: u32, down_at: u64, up_at: u64) -> bool {
        let down = detector.process_key(vk, true, down_at);
        let up = detector.process_key(vk, false, up_at);
        down | up
    }

    #[test]
    fn two_quick_taps_fire() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, L_SHIFT, 1000, 1050));
        assert!(tap(&mut d, L_SHIFT, 1200, 1250));
    }

    #[test]
    fn taps_too_far_apart_do_not_fire_but_second_tap_starts_new_sequence() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, L_SHIFT, 1000, 1050));
        assert!(!tap(&mut d, L_SHIFT, 1500, 1600)); // 550 ms after the first release
        assert!(tap(&mut d, L_SHIFT, 1700, 1750)); // 150 ms after the second release
    }

    #[test]
    fn interval_is_measured_between_releases() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, L_SHIFT, 1000, 1050));
        assert!(!tap(&mut d, L_SHIFT, 1100, 1450)); // released exactly 400 ms later - not "< 400"
    }

    #[test]
    fn typing_capital_letters_does_not_fire() {
        // Shift held while typing "H" is not a clean Shift press
        let mut d = DoubleShiftDetector::new();
        d.process_key(L_SHIFT, true, 1000);
        d.process_key(KEY_H, true, 1020);
        d.process_key(KEY_H, false, 1040);
        assert!(!d.process_key(L_SHIFT, false, 1060));

        d.process_key(L_SHIFT, true, 1100);
        d.process_key(KEY_H, true, 1120);
        d.process_key(KEY_H, false, 1140);
        assert!(!d.process_key(L_SHIFT, false, 1160));
    }

    #[test]
    fn ctrl_shift_does_not_fire() {
        let mut d = DoubleShiftDetector::new();
        for t in [1000, 1200] {
            d.process_key(L_CONTROL, true, t);
            d.process_key(L_SHIFT, true, t + 10);
            assert!(!d.process_key(L_SHIFT, false, t + 20));
            d.process_key(L_CONTROL, false, t + 30);
        }
    }

    #[test]
    fn both_shift_keys_count_as_one_tap_until_both_released() {
        let mut d = DoubleShiftDetector::new();
        d.process_key(L_SHIFT, true, 1000);
        d.process_key(R_SHIFT, true, 1010);
        assert!(!d.process_key(R_SHIFT, false, 1020)); // left Shift still held
        assert!(!d.process_key(L_SHIFT, false, 1030)); // first tap
        assert!(tap(&mut d, R_SHIFT, 1100, 1150)); // second tap, other Shift key
    }

    #[test]
    fn auto_repeat_of_held_shift_is_ignored() {
        let mut d = DoubleShiftDetector::new();
        d.process_key(L_SHIFT, true, 1000);
        d.process_key(L_SHIFT, true, 1030); // Windows auto-repeat
        d.process_key(L_SHIFT, true, 1060);
        assert!(!d.process_key(L_SHIFT, false, 1090));
        assert!(tap(&mut d, L_SHIFT, 1200, 1250));
    }

    #[test]
    fn triple_tap_fires_once_fourth_tap_fires_again() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, L_SHIFT, 1000, 1050));
        assert!(tap(&mut d, L_SHIFT, 1100, 1150));
        assert!(!tap(&mut d, L_SHIFT, 1200, 1250)); // sequence was reset after firing
        assert!(tap(&mut d, L_SHIFT, 1300, 1350));
    }

    // Ported on purpose: another key between the taps resets only "shift alone", not the timer.
    #[test]
    fn other_key_between_taps_does_not_reset_timer_same_as_macos() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, L_SHIFT, 1000, 1050));
        tap(&mut d, KEY_A, 1100, 1120);
        assert!(tap(&mut d, L_SHIFT, 1200, 1250));
    }

    // Ported on purpose: releasing Ctrl while Shift is still held makes the Shift press "alone"
    // again.
    #[test]
    fn releasing_other_modifier_while_shift_held_counts_as_clean_tap_same_as_macos() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, L_SHIFT, 1000, 1050));
        d.process_key(L_SHIFT, true, 1100);
        d.process_key(L_CONTROL, true, 1110);
        d.process_key(L_CONTROL, false, 1120);
        assert!(d.process_key(L_SHIFT, false, 1130));
    }

    #[test]
    fn reset_clears_state() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, L_SHIFT, 1000, 1050));
        d.reset();
        assert!(!tap(&mut d, L_SHIFT, 1100, 1150));
    }

    #[test]
    fn shift_keys_are_recognized() {
        assert!(DoubleShiftDetector::is_shift_key(0x10));
        assert!(DoubleShiftDetector::is_shift_key(R_SHIFT));
        assert!(!DoubleShiftDetector::is_shift_key(L_CONTROL));
    }
}
