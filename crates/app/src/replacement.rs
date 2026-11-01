//! MARK: - Text Replacement Service
//!
//! Gets and replaces text in other applications via simulated keystrokes and the clipboard
//! (Ctrl+C / Ctrl+Insert, Ctrl+V / Shift+Insert, Ctrl+Shift+Left, Shift+Home).
//! Runs on the conversion thread.

use std::time::{Duration, Instant};

use reshift_core::{ClipboardKeys, ConversionResult};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_C, VK_CONTROL, VK_HOME, VK_INSERT, VK_LEFT, VK_RIGHT, VK_SHIFT, VK_V,
};

use crate::clipboard::{Clipboard, ClipboardSnapshot};
use crate::input::{send_chord, send_key};
use crate::log::debug;
use crate::wait::pump_sleep;

/// Max wait for the target app to put the selection on the clipboard (macOS: fixed 150 ms).
const COPY_TIMEOUT: Duration = Duration::from_millis(200);
/// Time for the target app to process a selection keystroke (macOS: usleep 150 ms).
const SELECTION_DELAY: Duration = Duration::from_millis(150);
/// Delay before the original clipboard is put back (macOS: 0.3 s).
pub const RESTORE_DELAY: Duration = Duration::from_millis(300);

/// A completed replacement.
pub struct Replaced {
    pub input: String,
    pub output: String,
    pub target_layout_id: String,
    /// The user's clipboard, to be put back once the target app has pasted.
    pub restore: Option<PendingRestore>,
}

/// The user's clipboard waiting to be put back after our paste.
pub struct PendingRestore {
    pub snapshot: ClipboardSnapshot,
    /// The sequence number right after our write: a different number means the user copied
    /// something new in the meantime - then nothing is restored.
    pub our_sequence: u32,
    pub due: Instant,
}

impl PendingRestore {
    /// Restores the snapshot unless the clipboard changed since our paste.
    pub fn apply(self, clipboard: &Clipboard) {
        if Clipboard::sequence_number() == self.our_sequence {
            clipboard.restore(&self.snapshot);
        }
    }
}

pub struct TextReplacement<'a> {
    clipboard: &'a Clipboard,
    keys: ClipboardKeys,
}

impl<'a> TextReplacement<'a> {
    pub fn new(clipboard: &'a Clipboard, keys: ClipboardKeys) -> Self {
        Self { clipboard, keys }
    }

    // MARK: - Selected Text Conversion

    /// Gets the selected text and replaces it using the clipboard. `None` if no text was
    /// selected/copied or the converter declined.
    pub fn get_and_replace_selected_text(
        &self,
        convert: impl FnOnce(&str) -> Option<ConversionResult>,
    ) -> Option<Replaced> {
        let snapshot = self.clipboard.take_snapshot();
        let sequence_before = Clipboard::sequence_number();

        let Some(selected) = self.copy_selection() else {
            debug!("getAndReplaceSelectedText: no text on clipboard after copy");
            self.restore_if_changed(snapshot.as_ref(), sequence_before);
            return None;
        };
        debug!(
            "getAndReplaceSelectedText: copied {} characters",
            selected.chars().count()
        );
        let Some(result) = convert(&selected) else {
            debug!("getAndReplaceSelectedText: converter returned nil");
            self.restore_if_changed(snapshot.as_ref(), sequence_before);
            return None;
        };
        let restore = self.paste(&result.text, snapshot);
        Some(Replaced {
            input: selected,
            output: result.text,
            target_layout_id: result.target_layout_id,
            restore,
        })
    }

    // MARK: - Smart Selection

    /// Greedy Line: select from the cursor to the start of the line (Shift+Home), copy, pass to
    /// the converter, paste the result back. The converter decides which portion to convert and
    /// returns the full replacement text.
    pub fn select_line_and_replace(
        &self,
        convert: impl FnOnce(&str) -> Option<ConversionResult>,
    ) -> Option<Replaced> {
        self.select_and_replace("selectLineAndReplace", VK_HOME, &[VK_SHIFT], convert)
    }

    /// Last Word: select the word left of the cursor (Ctrl+Shift+Left), copy, convert, paste back.
    pub fn select_and_replace_last_word(
        &self,
        convert: impl FnOnce(&str) -> Option<ConversionResult>,
    ) -> Option<Replaced> {
        self.select_and_replace(
            "selectAndReplaceLastWord",
            VK_LEFT,
            &[VK_CONTROL, VK_SHIFT],
            convert,
        )
    }

    fn select_and_replace(
        &self,
        name: &str,
        select_key: VIRTUAL_KEY,
        select_modifiers: &[VIRTUAL_KEY],
        convert: impl FnOnce(&str) -> Option<ConversionResult>,
    ) -> Option<Replaced> {
        let snapshot = self.clipboard.take_snapshot();
        let sequence_before = Clipboard::sequence_number();

        send_chord(select_key, select_modifiers);
        pump_sleep(SELECTION_DELAY);

        let Some(selected) = self.copy_selection() else {
            debug!("{name}: no text copied, deselecting");
            // Same as macOS: Right collapses a selection the copy could not pick up
            send_key(VK_RIGHT);
            self.restore_if_changed(snapshot.as_ref(), sequence_before);
            return None;
        };
        debug!("{name}: copied {} characters", selected.chars().count());

        let Some(result) = convert(&selected) else {
            debug!("{name}: converter returned nil, deselecting");
            // No conversion needed - deselect by pressing Right arrow
            send_key(VK_RIGHT);
            self.restore_if_changed(snapshot.as_ref(), sequence_before);
            return None;
        };
        // Paste replaces the selection
        let restore = self.paste(&result.text, snapshot);
        Some(Replaced {
            input: selected,
            output: result.text,
            target_layout_id: result.target_layout_id,
            restore,
        })
    }

    // MARK: - Clipboard round trip

    /// Simulates "copy" and returns the copied text, or `None` if nothing was really selected.
    fn copy_selection(&self) -> Option<String> {
        let before = Clipboard::sequence_number();
        match self.keys {
            ClipboardKeys::CtrlC => send_chord(VK_C, &[VK_CONTROL]),
            ClipboardKeys::CtrlInsert => send_chord(VK_INSERT, &[VK_CONTROL]),
        }
        // Nothing selected => most apps do not touch the clipboard at all
        if !Clipboard::wait_for_change(before, COPY_TIMEOUT) {
            return None;
        }
        let copied = self.clipboard.read_copied_text()?;
        if copied.text.is_empty() {
            return None;
        }
        if copied.is_whole_line_copy {
            debug!("copy: the app copied the whole line without a selection - ignoring");
            return None;
        }
        Some(copied.text)
    }

    fn paste(&self, text: &str, snapshot: Option<ClipboardSnapshot>) -> Option<PendingRestore> {
        self.clipboard.set_text(text);
        let our_sequence = Clipboard::sequence_number();
        match self.keys {
            ClipboardKeys::CtrlC => send_chord(VK_V, &[VK_CONTROL]),
            ClipboardKeys::CtrlInsert => send_chord(VK_INSERT, &[VK_SHIFT]),
        }
        snapshot.map(|snapshot| PendingRestore {
            snapshot,
            our_sequence,
            due: Instant::now() + RESTORE_DELAY,
        })
    }

    fn restore_if_changed(&self, snapshot: Option<&ClipboardSnapshot>, sequence_before: u32) {
        if let Some(snapshot) = snapshot
            && Clipboard::sequence_number() != sequence_before
        {
            self.clipboard.restore(snapshot);
        }
    }
}
