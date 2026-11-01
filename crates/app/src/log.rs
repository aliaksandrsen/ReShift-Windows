//! Diagnostic logging. Goes to
//! OutputDebugString - visible in DebugView. Nothing is written to disk unless a log file is
//! requested explicitly (`--log-file`, used for troubleshooting and E2E runs).
//!
//! Privacy: never log the user's text (selected or typed) - only lengths.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::core::HSTRING;

static LOG_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Also write the log to this file (`--log-file`).
pub fn set_file(path: Option<PathBuf>) {
    *LOG_FILE.lock().unwrap_or_else(PoisonError::into_inner) = path;
}

pub fn write(message: &str) {
    // SAFETY: OutputDebugStringW only reads the NUL-terminated string
    unsafe { OutputDebugStringW(&HSTRING::from(format!("[ReShift] {message}\n"))) };

    let file = LOG_FILE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(path) = file.as_ref() {
        // SAFETY: GetLocalTime has no preconditions
        let time = unsafe { GetLocalTime() };
        let line = format!(
            "{:02}:{:02}:{:02}.{:03} {message}\r\n",
            time.wHour, time.wMinute, time.wSecond, time.wMilliseconds
        );
        // Diagnostics must never break the app
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| file.write_all(line.as_bytes()));
    }
}

/// `debug!("...", args)` - writes a diagnostic line.
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::log::write(&format!($($arg)*))
    };
}

pub(crate) use debug;
