//! MARK: - Clipboard Service
//!
//! Win32 clipboard access for the copy => convert => paste round trip. Runs on the conversion thread; the owner
//! window belongs to that thread.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::core::HSTRING;

use crate::log::debug;
use crate::wait::pump_sleep;

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CF_DIBV5: u32 = 17;

const MAX_FORMAT_BYTES: usize = 64 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;
const OPEN_ATTEMPTS: u32 = 20;

/// Text taken from the clipboard after a simulated copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopiedText {
    pub text: String,
    /// The app copied the whole current line because nothing was selected (Visual Studio,
    /// Notepad++/Scintilla, VS Code) - this is not a real selection.
    pub is_whole_line_copy: bool,
}

/// Clipboard contents saved before a conversion and put back afterwards.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    items: Vec<(u32, Vec<u8>)>,
}

/// Registered clipboard formats.
struct Formats {
    exclude_from_monitors: u32,
    can_include_in_history: u32,
    can_upload_to_cloud: u32,
    preferred_drop_effect: u32,
    png: u32,
    line_select: u32,
    vs_line_copy: u32,
    chromium_custom_data: u32,
}

impl Formats {
    fn register() -> Self {
        let register = |name: &str| {
            // SAFETY: registering a format name has no preconditions
            unsafe { RegisterClipboardFormatW(&HSTRING::from(name)) }
        };
        Self {
            exclude_from_monitors: register("ExcludeClipboardContentFromMonitorProcessing"),
            can_include_in_history: register("CanIncludeInClipboardHistory"),
            can_upload_to_cloud: register("CanUploadToCloudClipboard"),
            preferred_drop_effect: register("Preferred DropEffect"),
            png: register("PNG"),
            line_select: register("MSDEVLineSelect"),
            vs_line_copy: register("VisualStudioEditorOperationsLineCutCopyClipboardTag"),
            chromium_custom_data: register("Chromium Web Custom MIME Data Format"),
        }
    }
}

pub struct Clipboard {
    /// Our window. With a NULL owner SetClipboardData fails after EmptyClipboard.
    owner: HWND,
    formats: Formats,
}

/// The open clipboard; closed on drop.
struct OpenClipboardGuard;

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: the clipboard was opened by this thread
        let _ = unsafe { CloseClipboard() };
    }
}

impl Clipboard {
    pub fn new(owner: HWND) -> Self {
        Self {
            owner,
            formats: Formats::register(),
        }
    }

    /// Changes every time any app writes to (or empties) the clipboard.
    pub fn sequence_number() -> u32 {
        // SAFETY: no preconditions
        unsafe { GetClipboardSequenceNumber() }
    }

    /// Waits until the clipboard changes after `before`.
    pub fn wait_for_change(before: u32, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if Self::sequence_number() != before {
                return true;
            }
            pump_sleep(Duration::from_millis(10));
        }
        Self::sequence_number() != before
    }

    // MARK: - Save / Restore

    /// Saves text, copied files and - only when there is no text - images. Text-bearing data
    /// from Office is not rendered into heavy formats on every conversion. `None` if the
    /// clipboard could not be opened.
    pub fn take_snapshot(&self) -> Option<ClipboardSnapshot> {
        let _open = self.open()?;
        let mut snapshot = ClipboardSnapshot::default();
        let mut total = 0;
        let mut take = |format: u32| {
            if !is_available(format) {
                return;
            }
            let limit = MAX_FORMAT_BYTES.min(MAX_SNAPSHOT_BYTES - total);
            // SAFETY: the clipboard is open; the handle stays valid until it is closed
            let handle = unsafe { GetClipboardData(format) }.ok();
            if let Some(data) = handle.and_then(|handle| read_global(handle, limit)) {
                total += data.len();
                snapshot.items.push((format, data));
            }
        };

        let has_text = is_available(CF_UNICODETEXT);
        if has_text {
            take(CF_UNICODETEXT);
        }
        if is_available(CF_HDROP) {
            take(CF_HDROP);
            take(self.formats.preferred_drop_effect);
        }
        if !has_text {
            take(self.formats.png);
            take(if is_available(CF_DIBV5) {
                CF_DIBV5
            } else {
                CF_DIB
            });
        }
        Some(snapshot)
    }

    /// Puts a snapshot back. Our write is hidden from clipboard history and cloud sync.
    pub fn restore(&self, snapshot: &ClipboardSnapshot) -> bool {
        let Some(_open) = self.open() else {
            return false;
        };
        // SAFETY: the clipboard is open by this thread
        let _ = unsafe { EmptyClipboard() };
        if snapshot.items.is_empty() {
            return true;
        }
        for (format, data) in &snapshot.items {
            set_data(*format, data);
        }
        self.mark_private();
        true
    }

    // MARK: - Text

    /// Reads the text an app just copied, or `None` if there is none.
    pub fn read_copied_text(&self) -> Option<CopiedText> {
        let _open = self.open()?;
        if !is_available(CF_UNICODETEXT) {
            return None;
        }
        // SAFETY: the clipboard is open; the handle stays valid until it is closed
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.ok()?;
        let bytes = read_global(handle, MAX_FORMAT_BYTES)?;
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|unit| *unit != 0)
            .collect();
        let text = String::from_utf16_lossy(&units);

        let is_whole_line_copy = is_available(self.formats.line_select)
            || is_available(self.formats.vs_line_copy)
            || (is_available(self.formats.chromium_custom_data)
                // SAFETY: as above
                && is_vs_code_empty_selection_copy(
                    unsafe { GetClipboardData(self.formats.chromium_custom_data) }
                        .ok()
                        .and_then(|handle| read_global(handle, 1024 * 1024))
                        .as_deref(),
                ));
        Some(CopiedText {
            text,
            is_whole_line_copy,
        })
    }

    /// Puts text on the clipboard, hidden from clipboard history and cloud sync.
    pub fn set_text(&self, text: &str) -> bool {
        let Some(_open) = self.open() else {
            return false;
        };
        // SAFETY: the clipboard is open by this thread
        let _ = unsafe { EmptyClipboard() };
        let bytes: Vec<u8> = text
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let ok = set_data(CF_UNICODETEXT, &bytes);
        self.mark_private();
        ok
    }

    // MARK: - Helpers

    fn open(&self) -> Option<OpenClipboardGuard> {
        for _ in 0..OPEN_ATTEMPTS {
            // SAFETY: our own window becomes the clipboard owner
            if unsafe { OpenClipboard(Some(self.owner)) }.is_ok() {
                return Some(OpenClipboardGuard);
            }
            // Another app (often a clipboard manager) holds the clipboard - try again shortly
            pump_sleep(Duration::from_millis(10));
        }
        debug!("clipboard: could not open");
        None
    }

    fn mark_private(&self) {
        for format in [
            self.formats.exclude_from_monitors,
            self.formats.can_include_in_history,
            self.formats.can_upload_to_cloud,
        ] {
            set_data(format, &[0, 0, 0, 0]);
        }
    }
}

/// VS Code (Electron) stores its editor metadata in Chromium's custom MIME data as UTF-16 JSON;
/// `"isFromEmptySelection":true` means Ctrl+C/Ctrl+Insert copied the whole line without a
/// selection.
pub fn is_vs_code_empty_selection_copy(chromium_custom_data: Option<&[u8]>) -> bool {
    let Some(data) = chromium_custom_data else {
        return false;
    };
    let marker: Vec<u8> = "\"isFromEmptySelection\":true"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    data.windows(marker.len())
        .any(|window| window == marker.as_slice())
}

fn is_available(format: u32) -> bool {
    // SAFETY: no preconditions
    unsafe { IsClipboardFormatAvailable(format) }.is_ok()
}

fn read_global(handle: HANDLE, max_bytes: usize) -> Option<Vec<u8>> {
    let memory = HGLOBAL(handle.0);
    // SAFETY: a clipboard handle of the open clipboard
    let size = unsafe { GlobalSize(memory) };
    if size == 0 || size > max_bytes {
        return None;
    }
    // SAFETY: as above; unlocked right after copying
    let pointer = unsafe { GlobalLock(memory) };
    if pointer.is_null() {
        return None;
    }
    // SAFETY: GlobalLock returned `size` readable bytes
    let data = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), size) }.to_vec();
    // SAFETY: balanced with the GlobalLock above
    let _ = unsafe { GlobalUnlock(memory) };
    Some(data)
}

fn set_data(format: u32, data: &[u8]) -> bool {
    // SAFETY: a fresh movable block; ownership passes to the system only on success
    let Ok(memory) = (unsafe { GlobalAlloc(GMEM_MOVEABLE, data.len().max(1)) }) else {
        return false;
    };
    // SAFETY: the block was just allocated with at least `data.len()` bytes
    unsafe {
        let pointer = GlobalLock(memory);
        if pointer.is_null() {
            let _ = GlobalFree(Some(memory));
            return false;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), pointer.cast::<u8>(), data.len());
        let _ = GlobalUnlock(memory);
        if SetClipboardData(format, Some(HANDLE(memory.0))).is_err() {
            let _ = GlobalFree(Some(memory));
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    //! Test suite. (clipboard part). The live clipboard is never touched.

    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn vs_code_whole_line_copy_is_recognized() {
        // Chromium custom MIME data: a pickle of UTF-16 strings
        let mut bytes = vec![0x10, 0, 0, 0];
        bytes.extend(utf16(
            "vscode-editor-data{\"version\":1,\"isFromEmptySelection\":true,\"multicursorText\":null,\"mode\":\"plaintext\"}",
        ));
        assert!(is_vs_code_empty_selection_copy(Some(&bytes)));
    }

    #[test]
    fn vs_code_selection_copy_is_not_a_whole_line_copy() {
        let bytes = utf16(
            "vscode-editor-data{\"version\":1,\"isFromEmptySelection\":false,\"multicursorText\":null}",
        );
        assert!(!is_vs_code_empty_selection_copy(Some(&bytes)));
        assert!(!is_vs_code_empty_selection_copy(None));
    }
}
