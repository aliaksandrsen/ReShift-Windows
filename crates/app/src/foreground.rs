//! MARK: - Foreground Tracker
//!
//! Reports foreground window changes for the administrator-window
//! indicator and the terminal line tracker. The WinEvent callback only posts a message to the
//! main window, so the change is handled outside of any modal loop.

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, PostMessageW, WINEVENT_OUTOFCONTEXT,
};

use crate::log::debug;

static TARGET_WINDOW: AtomicIsize = AtomicIsize::new(0);
static TARGET_MESSAGE: AtomicU32 = AtomicU32::new(0);

pub struct ForegroundTracker {
    hooks: Vec<HWINEVENTHOOK>,
}

impl ForegroundTracker {
    /// Posts `message` with the new foreground window in wParam to `window`. Must be called on
    /// the thread that owns `window` (out-of-context WinEvents arrive through its message loop).
    pub fn start(window: HWND, message: u32) -> Self {
        TARGET_WINDOW.store(window.0 as isize, Ordering::Release);
        TARGET_MESSAGE.store(message, Ordering::Release);
        // Restoring a minimized window does not always raise a foreground event
        let hooks = [EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND]
            .into_iter()
            .filter_map(|event| {
                // SAFETY: an out-of-context hook with a plain callback function
                let hook = unsafe {
                    SetWinEventHook(
                        event,
                        event,
                        None,
                        Some(on_event),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    )
                };
                if hook.is_invalid() {
                    debug!("ERROR: SetWinEventHook failed for event {event:#x}");
                    None
                } else {
                    Some(hook)
                }
            })
            .collect();
        Self { hooks }
    }
}

impl Drop for ForegroundTracker {
    fn drop(&mut self) {
        for hook in &self.hooks {
            // SAFETY: hooks installed by `start`
            let _ = unsafe { UnhookWinEvent(*hook) };
        }
    }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let target = TARGET_WINDOW.load(Ordering::Acquire);
    if target != 0 && !window.is_invalid() {
        // SAFETY: posting to our own window
        let _ = unsafe {
            PostMessageW(
                Some(HWND(target as *mut _)),
                TARGET_MESSAGE.load(Ordering::Acquire),
                WPARAM(window.0 as usize),
                LPARAM(0),
            )
        };
    }
}
