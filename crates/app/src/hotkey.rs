//! MARK: - Hotkey Manager
//!
//! Registers and manages the global shortcut: either a regular hotkey
//! (modifier+key, `RegisterHotKey` => WM_HOTKEY on the main window) or double Shift, detected on
//! the keyboard hook thread. The hook itself is owned by the app and shared with the terminal
//! line tracker.

use std::sync::{Arc, Mutex, PoisonError};

use reshift_core::DoubleShiftDetector;
use windows::Win32::Foundation::{HWND, WPARAM};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::WM_HOTKEY;

use crate::hook::HookKeyEvent;
use crate::hotkey_format::{self, HotkeyModifiers};
use crate::log::debug;

const HOTKEY_ID: i32 = 0x4C53;

/// Double Shift detection, shared with the hook thread.
#[derive(Debug, Default)]
pub struct DoubleShiftState {
    enabled: bool,
    detector: DoubleShiftDetector,
}

pub struct HotkeyManager {
    window: HWND,
    registered: bool,
    double_shift: Arc<Mutex<DoubleShiftState>>,
}

impl HotkeyManager {
    pub fn new(window: HWND) -> Self {
        Self {
            window,
            registered: false,
            double_shift: Arc::default(),
        }
    }

    /// The state the hook thread feeds (see `process_key`).
    pub fn double_shift_state(&self) -> Arc<Mutex<DoubleShiftState>> {
        Arc::clone(&self.double_shift)
    }

    /// Double Shift as the trigger (the app keeps the keyboard hook running).
    pub fn register_double_shift(&mut self) {
        self.unregister();
        lock(&self.double_shift).enabled = true;
    }

    /// A regular global hotkey (modifier + key). Returns false if it is taken.
    pub fn register(&mut self, virtual_key: u32, modifiers: HotkeyModifiers) -> bool {
        self.unregister();
        // SAFETY: our own window receives WM_HOTKEY
        let result = unsafe {
            RegisterHotKey(
                Some(self.window),
                HOTKEY_ID,
                HOT_KEY_MODIFIERS(modifiers.0) | MOD_NOREPEAT,
                virtual_key,
            )
        };
        self.registered = result.is_ok();
        if let Err(error) = result {
            debug!(
                "RegisterHotKey failed for {}: {error}",
                hotkey_format::describe(virtual_key, modifiers)
            );
        }
        self.registered
    }

    /// Unregisters everything (also while the hotkey recorder needs the keystrokes).
    pub fn unregister(&mut self) {
        if self.registered {
            // SAFETY: the hotkey was registered on our window
            let _ = unsafe { UnregisterHotKey(Some(self.window), HOTKEY_ID) };
            self.registered = false;
        }
        let mut state = lock(&self.double_shift);
        state.enabled = false;
        state.detector.reset();
    }

    /// True for the WM_HOTKEY of our hotkey.
    pub fn is_hotkey_message(message: u32, wparam: WPARAM) -> bool {
        message == WM_HOTKEY && wparam.0 == HOTKEY_ID as usize
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.unregister();
    }
}

/// Called on the hook thread for every key event - must stay fast. True when a double Shift
/// has just been detected.
pub fn process_key(state: &Mutex<DoubleShiftState>, event: &HookKeyEvent) -> bool {
    // With NumLock on, Shift + a numpad key makes the keyboard driver inject "fake" Shift
    // up/down events carrying the extended flag - they are not real presses
    if event.is_extended && DoubleShiftDetector::is_shift_key(event.virtual_key) {
        return false;
    }
    let mut state = lock(state);
    if !state.enabled {
        return false;
    }
    // SAFETY: GetTickCount64 has no preconditions
    let now = unsafe { GetTickCount64() };
    state
        .detector
        .process_key(event.virtual_key, event.is_key_down, now)
}

fn lock(state: &Mutex<DoubleShiftState>) -> std::sync::MutexGuard<'_, DoubleShiftState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    const L_SHIFT: u32 = 0xA0;

    fn event(virtual_key: u32, is_key_down: bool, is_extended: bool) -> HookKeyEvent {
        HookKeyEvent {
            virtual_key,
            scan_code: 0x2A,
            is_key_down,
            is_extended,
            is_injected: false,
        }
    }

    fn tap(state: &Mutex<DoubleShiftState>, is_extended: bool) -> bool {
        let down = process_key(state, &event(L_SHIFT, true, is_extended));
        let up = process_key(state, &event(L_SHIFT, false, is_extended));
        down | up
    }

    #[test]
    fn double_shift_fires_only_while_enabled() {
        let state = Mutex::new(DoubleShiftState::default());
        assert!(!tap(&state, false));
        assert!(!tap(&state, false));

        state.lock().unwrap().enabled = true;
        assert!(!tap(&state, false));
        assert!(tap(&state, false));
    }

    #[test]
    fn fake_extended_shift_events_are_ignored() {
        // NumLock + Shift + numpad key: the driver injects extended Shift events
        let state = Mutex::new(DoubleShiftState {
            enabled: true,
            ..Default::default()
        });
        assert!(!tap(&state, true));
        assert!(!tap(&state, true));
    }

    #[test]
    fn hotkey_messages_are_recognized() {
        assert!(HotkeyManager::is_hotkey_message(
            WM_HOTKEY,
            WPARAM(HOTKEY_ID as usize)
        ));
        assert!(!HotkeyManager::is_hotkey_message(WM_HOTKEY, WPARAM(1)));
    }
}
