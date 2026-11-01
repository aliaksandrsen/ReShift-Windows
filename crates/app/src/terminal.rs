//! MARK: - Terminal Input Tracker
//!
//! Reconstructs the line typed in a terminal from the physical keys (hook thread) so that it can
//! be converted without a selection. Memory only: the text is never
//! logged or saved, and it is forgotten on Enter, navigation keys, shortcuts, clicks and window
//! changes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use reshift_core::terminal_input::{TypedKey, TypedLineBuffer, VK_PACKET, classify};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, GetKeyboardLayout, HKL, MAPVK_VK_TO_CHAR, MapVirtualKeyExW,
    VK_CAPITAL, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetForegroundWindow, GetWindowThreadProcessId, WindowFromPoint,
};

use crate::hook::HookKeyEvent;
use crate::hotkey_format::HotkeyModifiers;
use crate::layouts::{input_thread_id, klid_from_layout_handle};

/// Shared by the hook thread (typing), the main thread (window changes) and the conversion
/// thread (reading and replacing the line).
#[derive(Clone)]
pub struct TerminalInputTracker {
    state: Arc<Mutex<TrackerState>>,
    layout_id_map: Arc<HashMap<String, String>>,
}

struct TrackerState {
    buffer: TypedLineBuffer,
    /// The window whose line is tracked (as a number: HWND is not Send).
    window: isize,
    is_recording: bool,
    /// The thread whose keyboard layout decides what each key types (0 = foreground thread).
    input_thread_id: u32,
    /// Last HKL => KLID, so the hook thread does not format a string per key.
    layout_cache: Option<(isize, Option<String>)>,
    /// The registered custom hotkey (virtual key + modifiers), if one is in use.
    hotkey: Option<(u32, HotkeyModifiers)>,
}

impl TerminalInputTracker {
    /// `layout_id_map`: "Layout Id" registry values => KLIDs of the variant layouts.
    pub fn new(layout_id_map: HashMap<String, String>) -> Self {
        Self {
            state: Arc::new(Mutex::new(TrackerState {
                buffer: TypedLineBuffer::new(),
                window: 0,
                is_recording: false,
                input_thread_id: 0,
                layout_cache: None,
                hotkey: None,
            })),
            layout_id_map: Arc::new(layout_id_map),
        }
    }

    fn lock(&self) -> MutexGuard<'_, TrackerState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The line typed in the current terminal window.
    pub fn current_line(&self) -> String {
        self.lock().buffer.text()
    }

    /// After a conversion the line holds the converted text (pressing again converts back).
    pub fn replace_line(&self, line: &str) {
        self.lock().buffer.replace(line);
    }

    /// Called when an external window becomes active. Coming back to the same window (e.g.
    /// after the tray menu) keeps the line; another window starts from scratch.
    pub fn set_window(&self, window: HWND, record: bool) {
        // Resolved once per window: the layout of this thread decides what each key types
        let input_thread = if record { input_thread_id(window) } else { 0 };
        let mut state = self.lock();
        if window.0 as isize != state.window {
            state.buffer.reset();
            state.window = window.0 as isize;
        }
        state.input_thread_id = input_thread;
        state.is_recording = record;
    }

    /// Our own window or the shell is active: stop recording but keep the line.
    pub fn pause(&self) {
        self.lock().is_recording = false;
    }

    /// The custom hotkey in use (`None` in double Shift mode). Its key press must not reset the
    /// line it is about to convert.
    pub fn set_hotkey(&self, hotkey: Option<(u32, HotkeyModifiers)>) {
        self.lock().hotkey = hotkey;
    }

    /// Called on the hook thread for every key event.
    pub fn process_key(&self, event: &HookKeyEvent) {
        let mut state = self.lock();
        if !state.is_recording || !event.is_key_down {
            return;
        }
        let shift = is_down(VK_SHIFT.0);
        let control = is_down(VK_CONTROL.0);
        let alt = is_down(VK_MENU.0);
        let win = is_down(VK_LWIN.0) || is_down(VK_RWIN.0);
        // Do not let the hotkey's own key press reset the line right before conversion reads it
        if state.hotkey.is_some_and(|hotkey| {
            is_hotkey_press(hotkey, event.virtual_key, shift, control, alt, win)
        }) {
            return;
        }
        let thread_id = if state.input_thread_id != 0 {
            state.input_thread_id
        } else {
            // SAFETY: plain window queries
            unsafe { GetWindowThreadProcessId(GetForegroundWindow(), None) }
        };
        // SAFETY: no preconditions
        let layout = unsafe { GetKeyboardLayout(thread_id) };
        self.update_layout_cache(&mut state, layout);
        // The high bit of MAPVK_VK_TO_CHAR marks dead keys (accents) in the active layout
        let is_dead_key = event.virtual_key != VK_PACKET
            // SAFETY: no preconditions
            && unsafe { MapVirtualKeyExW(event.virtual_key, MAPVK_VK_TO_CHAR, Some(layout)) } & 0x8000_0000 != 0;
        let action = classify(&TypedKey {
            virtual_key: event.virtual_key,
            scan_code: event.scan_code,
            is_extended: event.is_extended,
            shift,
            control,
            alt,
            win,
            // SAFETY: no preconditions
            caps_lock: unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) } & 1 != 0,
            is_dead_key,
            layout_id: state
                .layout_cache
                .as_ref()
                .and_then(|(_, layout_id)| layout_id.as_deref()),
        });
        state.buffer.apply(action);
    }

    /// Called on the hook thread: a click inside the active window may move the cursor.
    pub fn process_mouse_button_down(&self, point: POINT) {
        if !self.lock().is_recording {
            return;
        }
        // SAFETY: plain window queries
        let clicked = unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) };
        // SAFETY: as above
        if clicked == unsafe { GetForegroundWindow() } {
            self.lock().buffer.reset();
        }
    }

    fn update_layout_cache(&self, state: &mut TrackerState, layout: HKL) {
        let handle = layout.0 as isize;
        if !state
            .layout_cache
            .as_ref()
            .is_some_and(|(cached, _)| *cached == handle)
        {
            let layout_id = klid_from_layout_handle(handle as i64, &self.layout_id_map);
            state.layout_cache = Some((handle, layout_id));
        }
    }
}

/// True if a key press is the registered hotkey: its key with exactly its modifiers.
fn is_hotkey_press(
    (hotkey_key, modifiers): (u32, HotkeyModifiers),
    virtual_key: u32,
    shift: bool,
    control: bool,
    alt: bool,
    win: bool,
) -> bool {
    virtual_key == hotkey_key
        && shift == modifiers.contains(HotkeyModifiers::SHIFT)
        && control == modifiers.contains(HotkeyModifiers::CONTROL)
        && alt == modifiers.contains(HotkeyModifiers::ALT)
        && win == modifiers.contains(HotkeyModifiers::WIN)
}

/// The async state is the logical state after key remappers (e.g. PowerToys) - what the app sees.
fn is_down(virtual_key: u16) -> bool {
    // SAFETY: no preconditions
    (unsafe { GetAsyncKeyState(i32::from(virtual_key)) }) as u16 & 0x8000 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_is_kept_per_window_and_replaced_after_conversion() {
        let tracker = TerminalInputTracker::new(HashMap::new());
        tracker.lock().buffer.append("ghbdtn");
        tracker.replace_line("привет");
        assert_eq!(tracker.current_line(), "привет");

        // Another window starts from scratch; pausing keeps the line
        tracker.set_window(HWND(std::ptr::dangling_mut()), false);
        assert_eq!(tracker.current_line(), "");
        tracker.lock().buffer.append("abc");
        tracker.pause();
        assert_eq!(tracker.current_line(), "abc");
    }

    #[test]
    fn keys_are_ignored_while_not_recording() {
        let tracker = TerminalInputTracker::new(HashMap::new());
        tracker.process_key(&HookKeyEvent {
            virtual_key: 0x47,
            scan_code: 0x22,
            is_key_down: true,
            is_extended: false,
            is_injected: false,
        });
        assert_eq!(tracker.current_line(), "");
    }

    #[test]
    fn only_the_exact_hotkey_chord_is_ignored() {
        let ctrl_shift_f11 = (0x7A, HotkeyModifiers::CONTROL | HotkeyModifiers::SHIFT);
        assert!(is_hotkey_press(
            ctrl_shift_f11,
            0x7A,
            true,
            true,
            false,
            false
        ));
        // Another key, a missing or an extra modifier is a regular shortcut (resets the line)
        assert!(!is_hotkey_press(
            ctrl_shift_f11,
            0x7B,
            true,
            true,
            false,
            false
        ));
        assert!(!is_hotkey_press(
            ctrl_shift_f11,
            0x7A,
            false,
            true,
            false,
            false
        ));
        assert!(!is_hotkey_press(
            ctrl_shift_f11,
            0x7A,
            true,
            true,
            true,
            false
        ));
    }
}
