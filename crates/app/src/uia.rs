//! MARK: - UI Automation probe
//!
//! True if the keyboard focus is in an xterm.js terminal (VS Code integrated terminal and
//! similar, TerminalWindows.IsXtermFocusedAsync). Asked through UI Automation on a short-lived
//! thread with a timeout: a hung target app must not hang the conversion. Any failure means
//! "no".

use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

use crate::log::debug;

/// The class of the hidden textarea that receives the keys of an xterm.js terminal.
const XTERM_TEXTAREA_CLASS: &str = "xterm-helper-textarea";

pub fn is_xterm_focused(timeout: Duration) -> bool {
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("uia probe".to_owned())
        .spawn(move || {
            // The receiver may be gone after a timeout - nothing to do then
            let _ = sender.send(focused_class_name());
        });
    if let Err(error) = spawned {
        debug!("uia: could not start the probe thread: {error}");
        return false;
    }
    match receiver.recv_timeout(timeout) {
        Ok(Some(class_name)) => class_name.contains(XTERM_TEXTAREA_CLASS),
        Ok(None) => false,
        Err(_) => {
            debug!("uia: no answer within {} ms", timeout.as_millis());
            false
        }
    }
}

/// Class name of the focused UI Automation element.
fn focused_class_name() -> Option<String> {
    // SAFETY: COM is initialized for this thread and uninitialized before it ends; the
    // automation objects are released before CoUninitialize
    unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return None;
        }
        let class_name = (|| {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let element = automation.GetFocusedElement().ok()?;
            element.CurrentClassName().ok().map(|name| name.to_string())
        })();
        CoUninitialize();
        class_name
    }
}
