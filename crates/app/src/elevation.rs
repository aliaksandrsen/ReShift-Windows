//! MARK: - Elevation (UIPI)
//!
//! The Windows counterpart of the Accessibility permission: a non-elevated
//! process cannot send keystrokes to, or even see keystrokes going to, windows of elevated
//! processes.

use std::ffi::c_void;
use std::sync::OnceLock;

use windows::Win32::Foundation::{E_ACCESSDENIED, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::win::OwnedHandle;

/// True if ReShift itself runs as administrator (checked once).
pub fn is_current_process_elevated() -> bool {
    static ELEVATED: OnceLock<bool> = OnceLock::new();
    // SAFETY: the pseudo handle of the current process needs no closing
    *ELEVATED.get_or_init(|| token_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false))
}

/// True if the process runs elevated. If its token cannot be read because of access rights,
/// it is treated as elevated (that is exactly what UIPI will block).
pub fn is_process_elevated(process_id: u32) -> bool {
    // SAFETY: OpenProcess has no memory preconditions; the handle is owned below
    match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) } {
        Ok(process) => {
            let process = OwnedHandle(process);
            token_elevated(process.0).unwrap_or_else(|denied| denied)
        }
        Err(error) => error.code() == E_ACCESSDENIED,
    }
}

/// Starts an elevated copy of ReShift (UAC prompt). The new instance waits for this one to
/// exit before taking the single-instance lock. False if the user cancelled or it failed.
pub fn restart_as_administrator(extra_arguments: &[String]) -> bool {
    use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    let mut arguments = vec!["--wait-for-pid".to_owned(), std::process::id().to_string()];
    arguments.extend_from_slice(extra_arguments);
    let parameters = HSTRING::from(
        arguments
            .iter()
            .map(|argument| quote_argument(argument))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let file = HSTRING::from(executable.as_os_str());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: the structure and its strings outlive the call
    match unsafe { ShellExecuteExW(&mut info) } {
        Ok(()) => true,
        Err(error) => {
            crate::log::debug!("restart as administrator: {error}");
            false
        }
    }
}

/// Quotes a command line argument so that CommandLineToArgvW / the Rust runtime read it back
/// unchanged (backslashes before a quote are doubled).
pub fn quote_argument(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        return argument.to_owned();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for c in argument.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(c);
                backslashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// Sticky Keys turns two Shift presses into a locked Shift.
pub fn is_sticky_keys_on() -> bool {
    use windows::Win32::UI::Accessibility::{SKF_STICKYKEYSON, STICKYKEYS};
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETSTICKYKEYS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };

    let mut sticky_keys = STICKYKEYS {
        cbSize: size_of::<STICKYKEYS>() as u32,
        ..Default::default()
    };
    // SAFETY: the output buffer is a STICKYKEYS with its size set
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETSTICKYKEYS,
            sticky_keys.cbSize,
            Some((&raw mut sticky_keys).cast::<c_void>()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    result.is_ok() && sticky_keys.dwFlags.contains(SKF_STICKYKEYSON)
}

/// Elevation of a process token. `Err(true)` when access to the token is denied.
fn token_elevated(process: HANDLE) -> Result<bool, bool> {
    let mut token = HANDLE::default();
    // SAFETY: `token` receives the handle on success
    if let Err(error) = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } {
        return Err(error.code() == E_ACCESSDENIED);
    }
    let token = OwnedHandle(token);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut length = 0u32;
    // SAFETY: the output buffer is a TOKEN_ELEVATION of the size passed along
    let result = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&raw mut elevation).cast::<c_void>()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        )
    };
    result
        .map(|()| elevation.TokenIsElevated != 0)
        .map_err(|_| false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_quoted_for_the_command_line() {
        assert_eq!(quote_argument("--data-dir"), "--data-dir");
        assert_eq!(quote_argument(r"C:\My Data"), r#""C:\My Data""#);
        // A trailing backslash must not escape the closing quote
        assert_eq!(quote_argument(r"C:\My Data\"), r#""C:\My Data\\""#);
        assert_eq!(quote_argument(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_argument(""), r#""""#);
    }

    #[test]
    fn own_process_elevation_matches_the_token() {
        assert_eq!(
            is_current_process_elevated(),
            is_process_elevated(std::process::id())
        );
    }
}
