//! MARK: - Launch at Login
//!
//! HKCU Run key. Always starts ReShift with the user's normal rights.

use windows::Win32::System::Registry::HKEY_CURRENT_USER;

use crate::log::debug;
use crate::win::RegKey;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "ReShift";

/// Adds or removes the Run entry. The entry always points at the running executable.
pub fn set_enabled(enabled: bool) {
    let Some(key) = RegKey::create(HKEY_CURRENT_USER, RUN_KEY_PATH) else {
        debug!("launch at login: could not open the Run key");
        return;
    };
    if enabled {
        match std::env::current_exe() {
            Ok(exe) => {
                if !key.set_string(VALUE_NAME, &format!("\"{}\"", exe.display())) {
                    debug!("launch at login: could not write the Run entry");
                }
            }
            Err(error) => debug!("launch at login: unknown executable path: {error}"),
        }
    } else if key.has_value(VALUE_NAME) && !key.delete_value(VALUE_NAME) {
        debug!("launch at login: could not remove the Run entry");
    }
}
