//! MARK: - Window classification
//!
//! Which conversion path a foreground window gets.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};
use windows::core::PWSTR;

use crate::elevation;
use crate::win::{OwnedHandle, from_wide};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    /// Regular app: clipboard-based conversion (the macOS approach).
    Normal,
    /// Console / terminal window: typed-line conversion.
    Terminal,
    /// Editor that hosts xterm.js terminals (VS Code family): depends on the focused element.
    TerminalHost,
    /// Remote Desktop and similar: keystrokes would go elsewhere.
    Skipped,
}

const TERMINAL_CLASSES: [&str; 6] = [
    "ConsoleWindowClass",            // classic console (conhost)
    "CASCADIA_HOSTING_WINDOW_CLASS", // Windows Terminal
    "mintty",                        // Git Bash, Cygwin, MSYS2
    "VirtualConsoleClass",           // ConEmu / Cmder
    "PuTTY",
    "org.wezfurlong.wezterm", // WezTerm
];

const SKIPPED_CLASSES: [&str; 1] = [
    "TscShellContainerClass", // Remote Desktop client
];

/// Electron apps whose integrated terminal is xterm.js (process names, case-insensitive).
const TERMINAL_HOST_PROCESSES: [&str; 9] = [
    "Code",
    "Code - Insiders",
    "Cursor",
    "Windsurf",
    "Kiro",
    "VSCodium",
    "Hyper",
    "Tabby",
    "Trae",
];

/// Taskbar, notification area, desktop and shell popups - never conversion targets.
const SHELL_WINDOW_CLASSES: [&str; 7] = [
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
    "Progman",
    "WorkerW",
    "Windows.UI.Core.CoreWindow",
];

pub fn classify(window: HWND) -> WindowKind {
    kind_for(&class_name(window), || {
        process_name(process_id(window)).unwrap_or_default()
    })
}

/// Classification from the window class (and, lazily, the process name). Unit-tested.
pub fn kind_for(class_name: &str, process_name: impl FnOnce() -> String) -> WindowKind {
    if SKIPPED_CLASSES.contains(&class_name) {
        WindowKind::Skipped
    } else if TERMINAL_CLASSES.contains(&class_name) {
        WindowKind::Terminal
    } else {
        let process = process_name();
        if TERMINAL_HOST_PROCESSES
            .iter()
            .any(|host| host.eq_ignore_ascii_case(&process))
        {
            WindowKind::TerminalHost
        } else {
            WindowKind::Normal
        }
    }
}

pub fn class_name(window: HWND) -> String {
    let mut buffer = [0u16; 256];
    // SAFETY: the buffer is writable; GetClassNameW NUL-terminates it
    let length = unsafe { GetClassNameW(window, &mut buffer) };
    if length > 0 {
        String::from_utf16_lossy(&buffer[..length as usize])
    } else {
        String::new()
    }
}

pub fn process_id(window: HWND) -> u32 {
    let mut process_id = 0;
    // SAFETY: the output pointer is valid
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    process_id
}

/// The executable name without extension (`Process.ProcessName`).
pub fn process_name(process_id: u32) -> Option<String> {
    // SAFETY: the handle is owned below
    let process = OwnedHandle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?,
    );
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    // SAFETY: the buffer and its length describe valid writable memory
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .ok()?;
    let path = from_wide(&buffer[..length as usize]);
    std::path::Path::new(&path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

pub fn is_own_window(window: HWND) -> bool {
    process_id(window) == std::process::id()
}

pub fn is_shell_window(window: HWND) -> bool {
    SHELL_WINDOW_CLASSES.contains(&class_name(window).as_str())
}

/// True if the window belongs to an elevated process while we are not elevated.
pub fn is_blocked_by_uipi(window: HWND) -> bool {
    if elevation::is_current_process_elevated() || window.is_invalid() {
        return false;
    }
    let process_id = process_id(window);
    process_id != 0 && elevation::is_process_elevated(process_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_classified_like_the_csharp_version() {
        let no_process = || panic!("the process is only needed for unknown classes");
        assert_eq!(
            kind_for("TscShellContainerClass", no_process),
            WindowKind::Skipped
        );
        assert_eq!(
            kind_for("ConsoleWindowClass", no_process),
            WindowKind::Terminal
        );
        assert_eq!(
            kind_for("CASCADIA_HOSTING_WINDOW_CLASS", no_process),
            WindowKind::Terminal
        );
        assert_eq!(
            kind_for("Chrome_WidgetWin_1", || "Code".to_owned()),
            WindowKind::TerminalHost
        );
        assert_eq!(
            kind_for("Chrome_WidgetWin_1", || "cursor".to_owned()),
            WindowKind::TerminalHost
        );
        assert_eq!(
            kind_for("Chrome_WidgetWin_1", || "chrome".to_owned()),
            WindowKind::Normal
        );
        assert_eq!(kind_for("Notepad", String::new), WindowKind::Normal);
    }

    #[test]
    fn own_process_name_is_resolved() {
        let name = process_name(std::process::id()).unwrap();
        assert!(!name.is_empty() && !name.ends_with(".exe"));
    }
}
