//! MARK: - About
//!
//! "About ReShift" as a TaskDialog on its own thread: no custom window, and the main thread
//! thread never waits for it.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use reshift_core::l10n::{fill, t};
use windows::Win32::Foundation::{HWND, LPARAM, S_OK, WPARAM};
use windows::Win32::UI::Controls::{
    LIM_LARGE, LoadIconMetric, TASKDIALOG_NOTIFICATIONS, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0,
    TDCBF_CLOSE_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TDF_ENABLE_HYPERLINKS, TDF_USE_HICON_MAIN,
    TDN_CREATED, TDN_HYPERLINK_CLICKED, TaskDialogIndirect,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNORMAL, SetForegroundWindow};
use windows::core::{HRESULT, HSTRING, PCWSTR, w};

use crate::log::debug;
use crate::win::{OwnedIcon, instance, resource_id};

/// The About link: this app's own repository.
const GITHUB_URL: &str = "https://github.com/aliaksandrsen/ReShift-Windows";

static IS_OPEN: AtomicBool = AtomicBool::new(false);
/// The dialog window while it is open (0 otherwise).
static OPEN_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Shows the dialog, or brings it to the front if it is already open.
pub fn show() {
    if IS_OPEN.swap(true, Ordering::AcqRel) {
        let window = OPEN_WINDOW.load(Ordering::Acquire);
        if window != 0 {
            // SAFETY: setting the foreground of a (possibly closed) window is harmless
            let _ = unsafe { SetForegroundWindow(HWND(window as *mut _)) };
        }
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("about".to_owned())
        .spawn(|| {
            if std::panic::catch_unwind(run_dialog).is_err() {
                debug!("about: the dialog panicked");
            }
            OPEN_WINDOW.store(0, Ordering::Release);
            IS_OPEN.store(false, Ordering::Release);
        });
    if let Err(error) = spawned {
        debug!("about: could not start the dialog thread: {error}");
        IS_OPEN.store(false, Ordering::Release);
    }
}

/// The dialog text: version, tagline, how to use, link.
fn content() -> String {
    format!(
        "{version}\n\n{tagline}\n\n{how_to_use}\n1. {step1}\n2. {step2}\n3. {step3}\n\n<a href=\"{GITHUB_URL}\">GitHub: aliaksandrsen/ReShift-Windows ↗</a>\n{license}",
        version = fill(t("about.version"), env!("CARGO_PKG_VERSION")),
        tagline = t("about.tagline"),
        how_to_use = t("about.howToUse"),
        step1 = t("about.step1"),
        step2 = t("about.step2"),
        step3 = t("about.step3"),
        license = t("about.license"),
    )
}

fn run_dialog() {
    let title = HSTRING::from(t("about.windowTitle"));
    let instruction = HSTRING::from(t("about.appName"));
    let content = HSTRING::from(content());
    // SAFETY: the resource is part of our executable
    let icon = unsafe { LoadIconMetric(Some(instance()), resource_id(1), LIM_LARGE) }
        .map(OwnedIcon)
        .unwrap_or_default();

    let config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        hInstance: instance(),
        dwFlags: TDF_ENABLE_HYPERLINKS | TDF_USE_HICON_MAIN | TDF_ALLOW_DIALOG_CANCELLATION,
        dwCommonButtons: TDCBF_CLOSE_BUTTON,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        Anonymous1: TASKDIALOGCONFIG_0 { hMainIcon: icon.0 },
        pszMainInstruction: PCWSTR(instruction.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        pfCallback: Some(callback),
        ..Default::default()
    };
    // SAFETY: the config and its strings outlive the (modal) call
    if let Err(error) = unsafe { TaskDialogIndirect(&config, None, None, None) } {
        debug!("about: TaskDialogIndirect failed: {error}");
    }
}

unsafe extern "system" fn callback(
    window: HWND,
    notification: TASKDIALOG_NOTIFICATIONS,
    _wparam: WPARAM,
    lparam: LPARAM,
    _data: isize,
) -> HRESULT {
    match notification {
        TDN_CREATED => {
            OPEN_WINDOW.store(window.0 as isize, Ordering::Release);
            // SAFETY: our own dialog window
            let _ = unsafe { SetForegroundWindow(window) };
        }
        TDN_HYPERLINK_CLICKED => {
            // SAFETY: lParam is the NUL-terminated href of the clicked link
            unsafe {
                ShellExecuteW(
                    Some(window),
                    w!("open"),
                    PCWSTR(lparam.0 as *const u16),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                );
            }
        }
        _ => {}
    }
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_has_version_and_link() {
        let text = content();
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        assert!(text.contains(&format!("<a href=\"{GITHUB_URL}\">")));
    }
}
