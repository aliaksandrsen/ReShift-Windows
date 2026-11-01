//! MARK: - Core Conversion Logic
//!
//! Runs on its own thread, so the UI thread never waits: the thread owns the clipboard window,
//! dispatches its messages while it waits, and gets requests with a copy of the settings. A trigger that
//! arrives while a conversion runs is ignored.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use reshift_core::l10n::t;
use reshift_core::{
    ClipboardKeys, KeyboardLayout, LayoutSwitchMode, SmartConversionMode, TextConverter,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentThreadId, INFINITE, SetEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, HWND_MESSAGE, MSG,
    MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_NOREMOVE, PeekMessageW, PostMessageW,
    PostThreadMessageW, QS_ALLINPUT, RegisterClassExW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_QUIT, WNDCLASSEXW,
};
use windows::core::w;

use crate::app::{ConversionTrigger, truncate};
use crate::clipboard::Clipboard;
use crate::elevation;
use crate::feedback;
use crate::hotkey_format::HotkeyModifiers;
use crate::input::{send_backspaces_and_text, send_menu_mask, wait_for_modifiers_released};
use crate::layouts;
use crate::log::debug;
use crate::replacement::{PendingRestore, Replaced, TextReplacement};
use crate::settings::Settings;
use crate::terminal::TerminalInputTracker;
use crate::uia;
use crate::wait::{pump_pending, pump_sleep};
use crate::win::{OwnedHandle, instance};
use crate::window_kind::{self, WindowKind};
use reshift_core::terminal_input::compute_terminal_edit;

/// Posted to the main window when `MainEvent`s are waiting.
pub const WM_APP_MAIN_EVENT: u32 = WM_APP + 5;

const MAX_NOTIFICATION_LENGTH: usize = 250;
/// Max wait for the UI Automation answer about an editor's focused element.
const XTERM_PROBE_TIMEOUT: Duration = Duration::from_millis(600);

/// The settings a conversion needs (copied from the main thread with every request).
#[derive(Clone, Debug)]
pub struct ConversionSettings {
    pub enabled_layouts: Vec<KeyboardLayout>,
    pub smart_conversion_mode: SmartConversionMode,
    pub layout_switch_mode: LayoutSwitchMode,
    pub clipboard_keys: ClipboardKeys,
    pub terminal_support: bool,
    pub play_sounds: bool,
    pub show_notifications: bool,
    pub hotkey_modifiers: HotkeyModifiers,
}

impl From<&Settings> for ConversionSettings {
    fn from(settings: &Settings) -> Self {
        Self {
            enabled_layouts: settings.enabled_layouts.clone(),
            smart_conversion_mode: settings.smart_conversion_mode,
            layout_switch_mode: settings.layout_switch_mode,
            clipboard_keys: settings.clipboard_keys,
            terminal_support: settings.terminal_support,
            play_sounds: settings.play_sounds,
            show_notifications: settings.show_notifications,
            hotkey_modifiers: settings.hotkey_modifiers,
        }
    }
}

pub struct ConversionRequest {
    pub trigger: ConversionTrigger,
    pub settings: ConversionSettings,
}

/// Work for the main thread (it owns the tray icon).
pub enum MainEvent {
    Notification { title: String, text: String },
}

// MARK: - Worker thread

pub struct ConversionWorker {
    requests: mpsc::Sender<ConversionRequest>,
    wake: Arc<OwnedHandle>,
    busy: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    thread_id: u32,
}

impl ConversionWorker {
    /// Starts the conversion thread. Events for the main thread go to `events`, announced by
    /// `WM_APP_MAIN_EVENT` posted to `main_window`.
    pub fn start(
        main_window: HWND,
        events: mpsc::Sender<MainEvent>,
        tracker: TerminalInputTracker,
    ) -> Option<Self> {
        // SAFETY: an unnamed auto-reset event, owned below
        let wake = Arc::new(OwnedHandle(
            unsafe { CreateEventW(None, false, false, windows::core::PCWSTR::null()) }.ok()?,
        ));
        let busy = Arc::new(AtomicBool::new(false));
        let (requests, receiver) = mpsc::channel();
        let (started, thread_id) = mpsc::channel();
        let main_window = main_window.0 as isize;
        let thread = {
            let wake = Arc::clone(&wake);
            let busy = Arc::clone(&busy);
            std::thread::Builder::new()
                .name("conversion".to_owned())
                .spawn(move || {
                    run(
                        &receiver,
                        &wake,
                        &busy,
                        events,
                        tracker,
                        main_window,
                        &started,
                    );
                })
                .inspect_err(|error| {
                    debug!("ERROR: could not start the conversion thread: {error}")
                })
                .ok()?
        };
        let thread_id = thread_id
            .recv_timeout(Duration::from_secs(5))
            .inspect_err(|_| debug!("ERROR: the conversion thread did not start"))
            .ok()?;
        Some(Self {
            requests,
            wake,
            busy,
            thread: Some(thread),
            thread_id,
        })
    }

    /// Queues a conversion. False if one is already running (the trigger is ignored).
    pub fn request(&self, request: ConversionRequest) -> bool {
        if self.busy.swap(true, Ordering::AcqRel) {
            return false;
        }
        if self.requests.send(request).is_err() {
            self.busy.store(false, Ordering::Release);
            return false;
        }
        // SAFETY: a valid event handle
        let _ = unsafe { SetEvent(self.wake.0) };
        true
    }
}

impl Drop for ConversionWorker {
    fn drop(&mut self) {
        // SAFETY: posting to a thread id is safe even if the thread has ended
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    requests: &mpsc::Receiver<ConversionRequest>,
    wake: &OwnedHandle,
    busy: &AtomicBool,
    events: mpsc::Sender<MainEvent>,
    tracker: TerminalInputTracker,
    main_window: isize,
    started: &mpsc::Sender<u32>,
) {
    let mut message = MSG::default();
    // SAFETY: creates the message queue of this thread before anyone posts to it
    let _ = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) };
    let owner = create_clipboard_window();
    let mut coordinator = Coordinator {
        clipboard: Clipboard::new(owner),
        tracker,
        events,
        main_window,
        shown_elevated_notice: false,
        shown_sticky_keys_notice: false,
        pending_restore: None,
    };
    // SAFETY: no preconditions
    let _ = started.send(unsafe { GetCurrentThreadId() });

    loop {
        let timeout = coordinator
            .pending_restore
            .as_ref()
            .map_or(INFINITE, |restore| {
                let remaining = restore.due.saturating_duration_since(Instant::now());
                u32::try_from(remaining.as_millis()).unwrap_or(INFINITE - 1)
            });
        // SAFETY: waits on our event and the message queue of this thread
        let result = unsafe {
            MsgWaitForMultipleObjectsEx(Some(&[wake.0]), timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if result == WAIT_OBJECT_0 {
            while let Ok(request) = requests.try_recv() {
                let outcome =
                    std::panic::catch_unwind(AssertUnwindSafe(|| coordinator.perform(&request)));
                if outcome.is_err() {
                    debug!("ERROR: the conversion panicked");
                }
                busy.store(false, Ordering::Release);
            }
        } else if result.0 == WAIT_OBJECT_0.0 + 1 {
            if !pump_pending() {
                break;
            }
        } else if result == WAIT_TIMEOUT {
            coordinator.restore_clipboard_when_due();
        } else {
            debug!("ERROR: the conversion thread wait failed");
            pump_sleep(Duration::from_millis(100));
        }
    }

    // Never leave our converted text on the user's clipboard
    coordinator.restore_clipboard_when_due();
    // SAFETY: our own window
    let _ = unsafe { DestroyWindow(owner) };
}

/// Message-only window that owns our clipboard writes.
fn create_clipboard_window() -> HWND {
    let class_name = w!("ReShift.Clipboard");
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(clipboard_window_procedure),
        hInstance: instance(),
        lpszClassName: class_name,
        ..Default::default()
    };
    // SAFETY: the class structure is fully initialized; the strings are static
    unsafe {
        RegisterClassExW(&class);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            windows::core::PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance()),
            None,
        )
    }
    .unwrap_or_else(|error| {
        debug!("ERROR: could not create the clipboard window: {error}");
        HWND::default()
    })
}

unsafe extern "system" fn clipboard_window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: default processing of a message of our window
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

// MARK: - Pipeline

struct Coordinator {
    clipboard: Clipboard,
    tracker: TerminalInputTracker,
    events: mpsc::Sender<MainEvent>,
    main_window: isize,
    // Track whether we've already shown these notices this session
    shown_elevated_notice: bool,
    shown_sticky_keys_notice: bool,
    /// The user's clipboard waiting to be put back after our last paste.
    pending_restore: Option<PendingRestore>,
}

impl Coordinator {
    fn perform(&mut self, request: &ConversionRequest) {
        debug!("performConversion() called, trigger={:?}", request.trigger);
        // The previous paste must be finished before the clipboard is touched again
        self.restore_clipboard_now();
        let settings = &request.settings;

        // SAFETY: no preconditions
        let window = unsafe { GetForegroundWindow() };
        let Some(kind) = self.check_foreground_window(window, settings) else {
            return;
        };

        if request.trigger == ConversionTrigger::DoubleShift && elevation::is_sticky_keys_on() {
            // Two Shift presses just locked Shift: every simulated chord would get an extra Shift
            debug!("performConversion: Sticky Keys is on, skipping double Shift conversion");
            if !self.shown_sticky_keys_notice {
                self.shown_sticky_keys_notice = true;
                self.show_alert("alert.stickyKeysTitle", "alert.stickyKeysMessage");
            }
            return;
        }

        if request.trigger == ConversionTrigger::CustomHotkey
            && settings
                .hotkey_modifiers
                .intersects(HotkeyModifiers::ALT | HotkeyModifiers::WIN)
        {
            // Keep the release of Alt/Win from opening the window menu or Start
            send_menu_mask();
        }
        if !wait_for_modifiers_released(Duration::from_secs(1), pump_sleep) {
            debug!("performConversion: modifiers are still held, aborting");
            return;
        }

        // Terminals (and xterm.js terminals inside editors) have no selection: retype the typed line
        if kind == WindowKind::Terminal
            || (kind == WindowKind::TerminalHost && uia::is_xterm_focused(XTERM_PROBE_TIMEOUT))
        {
            self.perform_terminal_conversion(settings);
            return;
        }

        let converter = TextConverter::new(&settings.enabled_layouts);
        let replaced = {
            let replacement = TextReplacement::new(&self.clipboard, settings.clipboard_keys);
            // Try the clipboard-based approach first (works when text is selected)
            match replacement.get_and_replace_selected_text(|text| {
                converter.convert_selected_text_with_info(text)
            }) {
                Some(replaced) => {
                    debug!("Direct conversion succeeded");
                    Some(replaced)
                }
                None => {
                    debug!("No selected text, trying smart conversion after short delay...");
                    pump_sleep(Duration::from_millis(50));
                    self.perform_smart_conversion(&replacement, &converter, settings)
                }
            }
        };
        if let Some(replaced) = replaced {
            self.on_conversion_succeeded(replaced, settings);
        }
    }

    /// When no text is selected, use the configured smart conversion mode.
    fn perform_smart_conversion(
        &self,
        replacement: &TextReplacement<'_>,
        converter: &TextConverter<'_>,
        settings: &ConversionSettings,
    ) -> Option<Replaced> {
        debug!(
            "performSmartConversion() mode={:?}",
            settings.smart_conversion_mode
        );
        match settings.smart_conversion_mode {
            SmartConversionMode::Disabled => {
                debug!("Smart conversion is disabled");
                None
            }
            // Last Word mode: select one word left, convert if it looks wrong
            SmartConversionMode::LastWord => replacement.select_and_replace_last_word(|text| {
                if !converter.looks_like_wrong_layout(text) {
                    return None;
                }
                converter.convert_selected_text_with_info(text)
            }),
            // Greedy Line mode: select to line start, find boundary, convert wrong-layout tail
            SmartConversionMode::GreedyLine => replacement
                .select_line_and_replace(|line| converter.convert_line_greedy_with_info(line)),
        }
    }

    /// Terminal path (Windows only): converts the line reconstructed from the typed keys with
    /// the Smart Conversion rules, erases the changed part with Backspace and types the result.
    fn perform_terminal_conversion(&mut self, settings: &ConversionSettings) {
        let line = self.tracker.current_line();
        // The typed line is never logged - only its length
        debug!(
            "terminal: typed line has {} characters, mode={:?}",
            line.chars().count(),
            settings.smart_conversion_mode
        );
        if line.is_empty() {
            return;
        }
        let converter = TextConverter::new(&settings.enabled_layouts);
        let Some(edit) = compute_terminal_edit(&converter, &line, settings.smart_conversion_mode)
        else {
            debug!("terminal: nothing to convert");
            return;
        };
        send_backspaces_and_text(edit.erase_count, &edit.insert);
        // Pressing the hotkey again converts back
        self.tracker.replace_line(&edit.new_line);
        self.finish_conversion(&edit.input, &edit.output, &edit.target_layout_id, settings);
    }

    fn on_conversion_succeeded(&mut self, replaced: Replaced, settings: &ConversionSettings) {
        self.pending_restore = replaced.restore;
        self.finish_conversion(
            &replaced.input,
            &replaced.output,
            &replaced.target_layout_id,
            settings,
        );
    }

    /// Sound, notification and layout switch after a successful conversion.
    fn finish_conversion(
        &self,
        input: &str,
        output: &str,
        target_layout_id: &str,
        settings: &ConversionSettings,
    ) {
        if settings.play_sounds {
            feedback::play_tink();
        }
        if settings.show_notifications {
            // Sound is handled separately by the playSounds setting
            self.notify(
                "ReShift",
                &truncate(&format!("{input} => {output}"), MAX_NOTIFICATION_LENGTH),
            );
        }
        // As in the macOS app, the switch only ever happens after a successful conversion, so
        // all three modes behave the same
        match settings.layout_switch_mode {
            LayoutSwitchMode::Always
            | LayoutSwitchMode::IfLastWordConverted
            | LayoutSwitchMode::IfAnyWordConverted => layouts::switch_to_layout(target_layout_id),
        }
    }

    // MARK: - Window checks

    /// Which path the foreground window gets, or `None` if it has to be skipped.
    fn check_foreground_window(
        &mut self,
        window: HWND,
        settings: &ConversionSettings,
    ) -> Option<WindowKind> {
        if window.is_invalid() || window_kind::is_own_window(window) {
            debug!("performConversion: no external foreground window");
            return None;
        }
        let kind = window_kind::classify(window);
        if kind == WindowKind::Skipped {
            // Keystrokes into a Remote Desktop window go to the remote machine
            debug!("performConversion: skipping remote desktop window");
            return None;
        }
        if kind == WindowKind::Terminal && !settings.terminal_support {
            // Without the typed-line tracker the only option would be Ctrl+C (SIGINT) - skip
            debug!("performConversion: terminal support is off, skipping terminal window");
            return None;
        }
        if window_kind::is_blocked_by_uipi(window) {
            // SendInput into an elevated window fails silently - explain instead
            debug!("performConversion: foreground window is elevated, ReShift is not");
            if !self.shown_elevated_notice {
                self.shown_elevated_notice = true;
                self.show_alert("alert.elevatedTargetTitle", "alert.elevatedTargetMessage");
            }
            return None;
        }
        // Without terminal support an editor gets the clipboard path, terminal or not
        Some(
            if kind == WindowKind::TerminalHost && !settings.terminal_support {
                WindowKind::Normal
            } else {
                kind
            },
        )
    }

    // MARK: - Clipboard restore

    /// Waits for the target app to finish the last paste, then puts the user's clipboard back.
    fn restore_clipboard_now(&mut self) {
        if let Some(restore) = &self.pending_restore {
            pump_sleep(restore.due.saturating_duration_since(Instant::now()));
            self.restore_clipboard_when_due();
        }
    }

    fn restore_clipboard_when_due(&mut self) {
        if let Some(restore) = self.pending_restore.take() {
            restore.apply(&self.clipboard);
        }
    }

    // MARK: - Feedback

    fn show_alert(&self, title_key: &str, message_key: &str) {
        self.notify(
            t(title_key),
            &truncate(t(message_key), MAX_NOTIFICATION_LENGTH),
        );
    }

    fn notify(&self, title: &str, text: &str) {
        let event = MainEvent::Notification {
            title: title.to_owned(),
            text: text.to_owned(),
        };
        if self.events.send(event).is_ok() {
            // SAFETY: posting to the main window from any thread is allowed
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(self.main_window as *mut _)),
                    WM_APP_MAIN_EVENT,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        }
    }
}
