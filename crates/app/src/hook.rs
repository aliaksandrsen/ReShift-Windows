//! MARK: - Low-Level Keyboard Hook
//!
//! Passive global keyboard monitor (the NSEvent global monitor of the macOS app), plus an
//! optional mouse-click monitor used to forget the typed terminal line when the user clicks
//!. Runs on its own thread with its own message loop: if a
//! WH_KEYBOARD_LL callback is slow, Windows silently removes the hook, so it must never share a
//! thread with anything that waits. Self-healing: the hooks are re-installed every minute and
//! after session unlock / resume.

use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, LLKHF_INJECTED, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW,
    PostThreadMessageW, SetTimer, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
    WM_QUIT, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WM_XBUTTONDOWN,
};

use crate::input::EXTRA_INFO_MARKER;
use crate::log::debug;
use crate::win::instance;

const WM_REINSTALL: u32 = WM_APP + 1;
const REINSTALL_INTERVAL_MS: u32 = 60_000;

/// A key event seen by the global keyboard hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookKeyEvent {
    pub virtual_key: u32,
    /// Hardware scan code; for VK_PACKET (injected Unicode text) the UTF-16 unit.
    pub scan_code: u32,
    pub is_key_down: bool,
    pub is_extended: bool,
    pub is_injected: bool,
}

/// Callbacks run on the hook thread for every real event. They must return fast.
pub struct HookHandlers {
    pub on_key: Box<dyn FnMut(&HookKeyEvent) + Send>,
    pub on_mouse_button_down: Option<Box<dyn FnMut(POINT) + Send>>,
}

/// The running hook thread; stopped on drop.
pub struct KeyboardHook {
    thread: Option<JoinHandle<()>>,
    thread_id: u32,
    has_mouse: bool,
}

struct HookThreadState {
    handlers: HookHandlers,
    keyboard_hook: Option<HHOOK>,
    mouse_hook: Option<HHOOK>,
}

thread_local! {
    /// Lives on the hook thread only: the hook procedures run there.
    static STATE: RefCell<Option<HookThreadState>> = const { RefCell::new(None) };
}

impl KeyboardHook {
    pub fn start(handlers: HookHandlers) -> Option<Self> {
        let has_mouse = handlers.on_mouse_button_down.is_some();
        let (started, thread_id) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("keyboard hook".to_owned())
            .spawn(move || run(handlers, &started))
            .inspect_err(|error| debug!("ERROR: could not start the hook thread: {error}"))
            .ok()?;
        let thread_id = thread_id
            .recv_timeout(Duration::from_secs(5))
            .inspect_err(|_| debug!("ERROR: the hook thread did not start"))
            .ok()?;
        Some(Self {
            thread: Some(thread),
            thread_id,
            has_mouse,
        })
    }

    pub fn has_mouse(&self) -> bool {
        self.has_mouse
    }

    /// Re-installs the hooks (after session unlock / resume, when Windows may have dropped them).
    pub fn reinstall(&self) {
        // SAFETY: posting to a thread id is safe even if the thread has ended
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_REINSTALL, WPARAM(0), LPARAM(0)) };
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        // SAFETY: as above
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(handlers: HookHandlers, started: &mpsc::Sender<u32>) {
    let mut message = MSG::default();
    // SAFETY: plain calls on the current thread
    unsafe {
        // Create the message queue before anyone posts to it
        let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    }
    STATE.with(|state| {
        *state.borrow_mut() = Some(HookThreadState {
            handlers,
            keyboard_hook: None,
            mouse_hook: None,
        });
    });
    install();
    // Periodic re-install in case Windows dropped a hook after a timeout
    // SAFETY: a thread timer; WM_TIMER arrives in the loop below
    unsafe { SetTimer(None, 0, REINSTALL_INTERVAL_MS, None) };
    // SAFETY: no preconditions
    let _ = started.send(unsafe { GetCurrentThreadId() });

    // SAFETY: standard message loop of this thread
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            if message.message == WM_REINSTALL || message.message == WM_TIMER {
                install();
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    if let Some(state) = STATE.with(|state| state.borrow_mut().take()) {
        for hook in [state.keyboard_hook, state.mouse_hook]
            .into_iter()
            .flatten()
        {
            // SAFETY: hooks installed by this thread
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        }
    }
}

/// Installs fresh hooks first, then removes the old ones - no gap in coverage.
fn install() {
    STATE.with(|cell| {
        let Ok(mut guard) = cell.try_borrow_mut() else {
            return;
        };
        let Some(state) = guard.as_mut() else {
            return;
        };
        replace(&mut state.keyboard_hook, true);
        if state.handlers.on_mouse_button_down.is_some() {
            replace(&mut state.mouse_hook, false);
        }
    });
}

fn replace(slot: &mut Option<HHOOK>, keyboard: bool) {
    // SAFETY: the hook procedures are plain functions of this executable
    let fresh = unsafe {
        if keyboard {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_procedure),
                Some(instance()),
                0,
            )
        } else {
            SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_procedure), Some(instance()), 0)
        }
    };
    match fresh {
        Ok(fresh) => {
            if let Some(old) = slot.replace(fresh) {
                // SAFETY: a hook installed by this thread
                let _ = unsafe { UnhookWindowsHookEx(old) };
            }
        }
        Err(error) => debug!("ERROR: SetWindowsHookEx failed: {error}"),
    }
}

/// Runs a handler on the hook thread state; a panic is caught (it must not cross FFI).
fn with_handlers(f: impl FnOnce(&mut HookHandlers)) {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        STATE.with(|cell| {
            if let Ok(mut guard) = cell.try_borrow_mut()
                && let Some(state) = guard.as_mut()
            {
                f(&mut state.handlers);
            }
        });
    }));
    if result.is_err() {
        debug!("ERROR: panic in a hook handler");
    }
}

unsafe extern "system" fn keyboard_procedure(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lParam points to a KBDLLHOOKSTRUCT
        let data = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        let is_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let is_up = message == WM_KEYUP || message == WM_SYSKEYUP;
        // Our own SendInput keystrokes are invisible to the detector and the line tracker
        if (is_down || is_up) && data.dwExtraInfo != EXTRA_INFO_MARKER {
            let event = HookKeyEvent {
                virtual_key: data.vkCode,
                scan_code: data.scanCode,
                is_key_down: is_down,
                is_extended: data.flags.contains(LLKHF_EXTENDED),
                is_injected: data.flags.contains(LLKHF_INJECTED),
            };
            with_handlers(|handlers| (handlers.on_key)(&event));
        }
    }
    // SAFETY: passes the event on unchanged
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_procedure(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let message = wparam.0 as u32;
    if code == HC_ACTION as i32
        && matches!(
            message,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        )
    {
        // SAFETY: for HC_ACTION, lParam points to a MSLLHOOKSTRUCT
        let point = unsafe { (*(lparam.0 as *const MSLLHOOKSTRUCT)).pt };
        with_handlers(|handlers| {
            if let Some(on_mouse_button_down) = handlers.on_mouse_button_down.as_mut() {
                on_mouse_button_down(point);
            }
        });
    }
    // SAFETY: passes the event on unchanged
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
