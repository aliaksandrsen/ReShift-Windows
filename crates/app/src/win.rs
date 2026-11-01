//! Small helpers around the Win32 API: UTF-16 strings, RAII handles and registry access.

use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, HINSTANCE};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_ROUTINE_FLAGS, REG_SZ, RRF_NOEXPAND,
    RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_MULTI_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegDeleteValueW, RegEnumKeyExW, RegEnumValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// Copies text into a fixed-size UTF-16 buffer (struct fields such as `szTip`), truncating it
/// and always leaving a terminating NUL.
pub fn copy_to_fixed(destination: &mut [u16], text: &str) {
    let capacity = destination.len().saturating_sub(1);
    let mut length = 0;
    for (slot, unit) in destination
        .iter_mut()
        .zip(text.encode_utf16().take(capacity))
    {
        *slot = unit;
        length += 1;
    }
    if let Some(terminator) = destination.get_mut(length) {
        *terminator = 0;
    }
}

/// The text of a NUL-terminated UTF-16 buffer.
pub fn from_wide(buffer: &[u16]) -> String {
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

/// The module handle of ReShift.exe (resources: icons).
pub fn instance() -> HINSTANCE {
    // SAFETY: querying the handle of the running executable has no preconditions
    unsafe { GetModuleHandleW(PCWSTR::null()) }
        .map_or(HINSTANCE::default(), |module| HINSTANCE(module.0))
}

/// `MAKEINTRESOURCE`: an integer resource ID passed as a string pointer.
pub fn resource_id(id: u16) -> PCWSTR {
    PCWSTR(usize::from(id) as *const u16)
}

/// A kernel handle closed on drop.
#[derive(Debug)]
pub struct OwnedHandle(pub HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: the handle is owned by this wrapper and closed exactly once
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

// SAFETY: kernel handles may be used and closed from any thread
unsafe impl Send for OwnedHandle {}
// SAFETY: kernel handles may be used from several threads at once (e.g. SetEvent)
unsafe impl Sync for OwnedHandle {}

/// An icon destroyed on drop.
#[derive(Debug, Default)]
pub struct OwnedIcon(pub HICON);

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: the icon was created by us (LoadIconMetric) and is destroyed exactly once
            let _ = unsafe { DestroyIcon(self.0) };
        }
    }
}

// MARK: - Registry

/// An open registry key, closed on drop.
pub struct RegKey(HKEY);

impl RegKey {
    /// Opens a key for reading.
    pub fn open(root: HKEY, path: &str) -> Option<Self> {
        let mut key = HKEY::default();
        // SAFETY: `key` receives the handle on success
        let result = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ, &mut key) };
        (result == ERROR_SUCCESS).then_some(Self(key))
    }

    /// Opens a key for reading and writing, creating it if needed.
    pub fn create(root: HKEY, path: &str) -> Option<Self> {
        let mut key = HKEY::default();
        // SAFETY: `key` receives the handle on success
        let result = unsafe {
            RegCreateKeyExW(
                root,
                &HSTRING::from(path),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_WRITE,
                None,
                &mut key,
                None,
            )
        };
        (result == ERROR_SUCCESS).then_some(Self(key))
    }

    /// Names of the direct subkeys.
    pub fn subkey_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut buffer = [0u16; 256];
        for index in 0.. {
            let mut length = buffer.len() as u32;
            // SAFETY: the buffer and its length describe valid writable memory
            let result = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if result != ERROR_SUCCESS {
                break;
            }
            names.push(String::from_utf16_lossy(&buffer[..length as usize]));
        }
        names
    }

    /// Names of the values of this key.
    pub fn value_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut buffer = vec![0u16; 16_384];
        for index in 0.. {
            let mut length = buffer.len() as u32;
            // SAFETY: the buffer and its length describe valid writable memory
            let result = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if result != ERROR_SUCCESS {
                break;
            }
            names.push(String::from_utf16_lossy(&buffer[..length as usize]));
        }
        names
    }

    /// A REG_SZ / REG_EXPAND_SZ value (environment variables are not expanded).
    pub fn string(&self, name: &str) -> Option<String> {
        let data = self.raw_value(name, RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND)?;
        Some(from_wide(&data))
    }

    /// A REG_MULTI_SZ value.
    pub fn multi_string(&self, name: &str) -> Option<Vec<String>> {
        let data = self.raw_value(name, RRF_RT_REG_MULTI_SZ)?;
        Some(
            data.split(|unit| *unit == 0)
                .filter(|part| !part.is_empty())
                .map(String::from_utf16_lossy)
                .collect(),
        )
    }

    pub fn has_value(&self, name: &str) -> bool {
        self.string(name).is_some()
    }

    pub fn set_string(&self, name: &str, value: &str) -> bool {
        let bytes: Vec<u8> = value
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        // SAFETY: the key is open for writing, the data is a NUL-terminated UTF-16 string
        unsafe {
            RegSetValueExW(self.0, &HSTRING::from(name), None, REG_SZ, Some(&bytes))
                == ERROR_SUCCESS
        }
    }

    pub fn delete_value(&self, name: &str) -> bool {
        // SAFETY: the key is open for writing
        unsafe { RegDeleteValueW(self.0, &HSTRING::from(name)) == ERROR_SUCCESS }
    }

    /// Reads a value as UTF-16 units (size query, then read).
    fn raw_value(&self, name: &str, flags: REG_ROUTINE_FLAGS) -> Option<Vec<u16>> {
        let name = HSTRING::from(name);
        let mut size = 0u32;
        // SAFETY: a size query without a data buffer
        let result = unsafe {
            RegGetValueW(
                self.0,
                PCWSTR::null(),
                &name,
                flags,
                None,
                None,
                Some(&mut size),
            )
        };
        if result != ERROR_SUCCESS {
            return None;
        }
        let mut data = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut size = (data.len() * 2) as u32;
        // SAFETY: `data` has room for `size` bytes
        let result = unsafe {
            RegGetValueW(
                self.0,
                PCWSTR::null(),
                &name,
                flags,
                None,
                Some(data.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if result != ERROR_SUCCESS {
            return None;
        }
        data.truncate(size as usize / 2);
        Some(data)
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: the key is owned by this wrapper and closed exactly once
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_buffers_are_truncated_and_terminated() {
        let mut buffer = [0xFFFFu16; 4];
        copy_to_fixed(&mut buffer, "abcdef");
        assert_eq!(
            buffer,
            [u16::from(b'a'), u16::from(b'b'), u16::from(b'c'), 0]
        );
        assert_eq!(from_wide(&buffer), "abc");

        copy_to_fixed(&mut buffer, "й");
        assert_eq!(from_wide(&buffer), "й");
    }

    #[test]
    fn registry_values_can_be_read() {
        use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
        let key = RegKey::open(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        )
        .unwrap();
        assert!(
            key.string("ProductName")
                .is_some_and(|name| name.contains("Windows"))
        );
        assert!(!key.value_names().is_empty());
        assert!(key.string("NoSuchValueForReShift").is_none());
    }
}
