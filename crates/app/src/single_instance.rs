//! MARK: - Single Instance
//!
//! One ReShift per user session. A second launch asks the running
//! instance to show its settings window and exits.

use std::ffi::c_void;

use windows::Win32::Foundation::{
    E_ACCESSDENIED, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM,
    WPARAM,
};
use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, OpenProcess,
    PROCESS_SYNCHRONIZE, RegisterWaitForSingleObject, ReleaseMutex, SetEvent, UnregisterWaitEx,
    WT_EXECUTEDEFAULT, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, PostMessageW};
use windows::core::HSTRING;

use crate::log::debug;
use crate::win::OwnedHandle;

const MUTEX_NAME: &str = r"Local\ReShift.Windows.SingleInstance";
const ACTIVATE_EVENT_NAME: &str = r"Local\ReShift.Windows.Activate";

pub enum Acquire {
    Acquired(SingleInstance),
    AlreadyRunning,
    /// The running instance is elevated: its objects are off-limits for us.
    AlreadyRunningElevated,
}

pub struct SingleInstance {
    mutex: Option<OwnedHandle>,
    activate_event: Option<OwnedHandle>,
    wait: HANDLE,
}

/// Separates instances that use different data directories (E2E runs): ".<first 16 hex digits
/// of SHA-256(scope)>".
pub fn scope_suffix(scope: &str) -> String {
    if scope.is_empty() {
        return String::new();
    }
    let mut hash = [0u8; 32];
    // SAFETY: input and output are valid slices; the SHA-256 pseudo handle needs no setup
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, scope.as_bytes(), &mut hash) };
    if status.is_err() {
        debug!("single instance: SHA-256 failed ({:#x})", status.0);
    }
    let hex: String = hash
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02X}"))
        .collect();
    format!(".{hex}")
}

pub fn try_acquire(scope: &str) -> Acquire {
    let suffix = scope_suffix(scope);
    let mutex_name = HSTRING::from(format!("{MUTEX_NAME}{suffix}"));
    let event_name = HSTRING::from(format!("{ACTIVATE_EVENT_NAME}{suffix}"));

    // SAFETY: the name outlives the call; the handle is owned below
    let created = unsafe { CreateMutexW(None, true, &mutex_name) };
    // SAFETY: read right after the call, before anything else can change it
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let mutex = match created {
        Ok(handle) => OwnedHandle(handle),
        Err(error) if error.code() == E_ACCESSDENIED => return Acquire::AlreadyRunningElevated,
        Err(error) => {
            // Better to run without the lock than not at all
            debug!("single instance: CreateMutex failed: {error}");
            return Acquire::Acquired(SingleInstance {
                mutex: None,
                activate_event: None,
                wait: HANDLE::default(),
            });
        }
    };
    if already_exists {
        drop(mutex);
        signal_running_instance(&event_name);
        return Acquire::AlreadyRunning;
    }
    // SAFETY: the name outlives the call; the handle is owned below
    let activate_event = unsafe { CreateEventW(None, false, false, &event_name) }
        .map(OwnedHandle)
        .inspect_err(|error| debug!("single instance: CreateEvent failed: {error}"))
        .ok();
    Acquire::Acquired(SingleInstance {
        mutex: Some(mutex),
        activate_event,
        wait: HANDLE::default(),
    })
}

/// Waits for a previous instance (restart as administrator) to exit.
pub fn wait_for_process_exit(process_id: u32) {
    // SAFETY: the handle is owned below and only waited on
    if let Ok(process) = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, process_id) } {
        let process = OwnedHandle(process);
        // SAFETY: a valid process handle
        unsafe { WaitForSingleObject(process.0, 10_000) };
    }
}

impl SingleInstance {
    /// Posts `message` to `window` whenever another launch signals this instance.
    pub fn listen_for_activation(&mut self, window: HWND, message: u32) {
        let Some(event) = &self.activate_event else {
            return;
        };
        ACTIVATION_MESSAGE.store(message, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: the callback only posts a message; the wait is unregistered before the event
        // handle is closed (Drop)
        let result = unsafe {
            RegisterWaitForSingleObject(
                &mut self.wait,
                event.0,
                Some(on_activate),
                Some(window.0.cast_const()),
                INFINITE,
                WT_EXECUTEDEFAULT,
            )
        };
        if let Err(error) = result {
            debug!("single instance: RegisterWaitForSingleObject failed: {error}");
        }
    }
}

static ACTIVATION_MESSAGE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Thread pool callback: the event was signaled by a second launch.
unsafe extern "system" fn on_activate(context: *mut c_void, _timed_out: bool) {
    let message = ACTIVATION_MESSAGE.load(std::sync::atomic::Ordering::Relaxed);
    // SAFETY: posting to a window handle is safe even if the window no longer exists
    let _ = unsafe { PostMessageW(Some(HWND(context)), message, WPARAM(0), LPARAM(0)) };
}

fn signal_running_instance(event_name: &HSTRING) {
    // Let the running instance bring its window to the front
    // SAFETY: no preconditions
    let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    // SAFETY: the name outlives the call; the handle is owned below
    match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, event_name) } {
        Ok(event) => {
            let event = OwnedHandle(event);
            // SAFETY: a valid event handle
            let _ = unsafe { SetEvent(event.0) };
        }
        Err(error) => debug!("single instance: could not signal the running instance: {error}"),
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        if !self.wait.is_invalid() {
            // SAFETY: waits for running callbacks, then frees the wait
            let _ = unsafe { UnregisterWaitEx(self.wait, Some(INVALID_HANDLE_VALUE)) };
        }
        if let Some(mutex) = &self.mutex {
            // SAFETY: the mutex is owned by this (main) thread since CreateMutexW
            let _ = unsafe { ReleaseMutex(mutex.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_suffix_matches_the_csharp_version() {
        assert_eq!(scope_suffix(""), "");
        // SHA-256("abc") = BA7816BF8F01CFEA...
        assert_eq!(scope_suffix("abc"), ".BA7816BF8F01CFEA");
    }
}
