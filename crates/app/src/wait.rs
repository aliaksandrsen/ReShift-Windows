//! Waiting on threads that own windows (the conversion thread owns the clipboard window): the
//! thread keeps dispatching its messages, so other apps never wait for it.

use std::time::{Duration, Instant};

use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, PostQuitMessage, QS_ALLINPUT, TranslateMessage, WM_QUIT,
};

/// Sleeps for `duration` while dispatching the messages of the calling thread.
pub fn pump_sleep(duration: Duration) {
    let deadline = Instant::now() + duration;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let milliseconds = u32::try_from(remaining.as_millis())
            .unwrap_or(u32::MAX)
            .max(1);
        // SAFETY: waits on the message queue of the current thread only
        unsafe {
            MsgWaitForMultipleObjectsEx(None, milliseconds, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if !pump_pending() {
            break;
        }
    }
}

/// Dispatches the messages already queued. Returns false if WM_QUIT was seen (it is posted
/// again, so the thread's main loop still ends).
pub fn pump_pending() -> bool {
    let mut message = MSG::default();
    // SAFETY: standard message dispatching on the current thread
    unsafe {
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            if message.message == WM_QUIT {
                PostQuitMessage(message.wParam.0 as i32);
                return false;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    true
}
