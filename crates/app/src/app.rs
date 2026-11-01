//! MARK: - App
//!
//! The Win32 tray application shell without a main window.
//! The main thread owns the settings, a hidden window (tray callbacks, WM_HOTKEY, broadcasts)
//! and the message loop. It never waits for a conversion.

use std::cell::{Cell, RefCell};
use std::sync::mpsc;

use reshift_core::l10n::{self, fill, t};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Globalization::{GetUserDefaultUILanguage, LCIDToLocaleName};
use windows::Win32::System::Registry::HKEY_CURRENT_USER;
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetMessageW, MB_ICONINFORMATION, MB_OK, MSG, MSGFLT_ALLOW, MessageBoxW, PBT_APMRESUMEAUTOMATIC,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, TranslateMessage,
    WM_APP, WM_DESTROY, WM_ENDSESSION, WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_WTSSESSION_CHANGE,
    WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP, WTS_CONSOLE_CONNECT, WTS_REMOTE_CONNECT,
    WTS_SESSION_UNLOCK,
};
use windows::core::{HSTRING, w};

use crate::cli::CommandLineOptions;
use crate::conversion::{
    ConversionRequest, ConversionSettings, ConversionWorker, MainEvent, WM_APP_MAIN_EVENT,
};
use crate::foreground::ForegroundTracker;
use crate::hook::{HookHandlers, KeyboardHook};
use crate::hotkey::{self, HotkeyManager};
use crate::log::{self, debug};
use crate::paths::AppPaths;
use crate::settings::{self, Settings};
use crate::single_instance::{self, Acquire, SingleInstance};
use crate::terminal::TerminalInputTracker;
use crate::tray::{self, MenuCommand, TrayIcon, TrayMessage};
use crate::win::{RegKey, from_wide, instance};
use crate::window_kind::WindowKind;
use crate::{about, elevation, layouts, settings_window, startup, window_kind};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// A second launch of ReShift asks this one to show its settings.
pub const WM_APP_ACTIVATE: u32 = WM_APP + 3;
/// Posted by the hook thread when it detects a double Shift.
const WM_APP_DOUBLE_SHIFT: u32 = WM_APP + 4;
/// Posted by the WinEvent hook: the foreground window (wParam) changed.
const WM_APP_FOREGROUND: u32 = WM_APP + 6;
/// Quit after a restart as administrator was started.
const WM_APP_QUIT: u32 = WM_APP + 7;

/// Notices longer than this are cut (balloon text limit).
const MAX_NOTIFICATION_LENGTH: usize = 250;

const WINDOW_CLASS: windows::core::PCWSTR = w!("ReShift.Window");

/// What started a conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionTrigger {
    DoubleShift,
    CustomHotkey,
}

struct App {
    window: HWND,
    /// Forwarded when restarting as administrator (keeps an isolated data folder)
    options: CommandLineOptions,
    paths: AppPaths,
    settings: Settings,
    /// Stopped first: its handlers feed the hotkey manager's state
    hook: Option<KeyboardHook>,
    /// Stopped before the tray: it may still report a notification
    worker: Option<ConversionWorker>,
    /// Events from the conversion thread (notifications)
    events: mpsc::Receiver<MainEvent>,
    _foreground: ForegroundTracker,
    /// The line typed in the active terminal (shared with the hook and conversion threads)
    terminal: TerminalInputTracker,
    hotkeys: HotkeyManager,
    /// True when the custom hotkey could not be registered (taken by another app).
    hotkey_registration_failed: bool,
    /// An administrator window is active: gray icon and an explaining tooltip.
    blocked_by_elevated_window: bool,
    _session_notifications: SessionNotifications,
    tray: TrayIcon,
    // Released last: the lock is held while anything else is alive
    _instance: SingleInstance,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static MENU_OPEN: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` with the app state. `None` before startup, after shutdown, or if the state is
/// already borrowed (a modal loop re-entered the window procedure).
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut app = cell.try_borrow_mut().ok()?;
        app.as_mut().map(f)
    })
}

/// Entry point; returns the process exit code.
pub fn run() -> i32 {
    std::panic::set_hook(Box::new(|info| log::write(&format!("panic: {info}"))));

    let options = CommandLineOptions::from_env();
    log::set_file(options.log_file.clone());
    let paths = AppPaths::new(options.data_directory.as_deref());
    if let Some(process_id) = options.wait_for_process_id {
        single_instance::wait_for_process_exit(process_id);
    }

    let mut settings = settings::load(&paths.settings_file(), layouts::installed_layouts);
    apply_language(&mut settings, &paths);

    let scope = if paths.is_overridden() {
        paths.data_directory().display().to_string()
    } else {
        String::new()
    };
    let mut instance = match single_instance::try_acquire(&scope) {
        Acquire::Acquired(instance) => instance,
        Acquire::AlreadyRunning => return 0,
        Acquire::AlreadyRunningElevated => {
            // SAFETY: a plain modal message box without owner
            unsafe {
                MessageBoxW(
                    None,
                    &HSTRING::from(t("alert.alreadyRunning")),
                    w!("ReShift"),
                    MB_OK | MB_ICONINFORMATION,
                )
            };
            return 0;
        }
    };

    debug!(
        "App launched. Executable: {}, PID: {}, data: {}, elevated: {}",
        std::env::current_exe().unwrap_or_default().display(),
        std::process::id(),
        paths.data_directory().display(),
        elevation::is_current_process_elevated()
    );

    let Some(window) = create_window() else {
        debug!("ERROR: could not create the message window");
        return 1;
    };
    // SAFETY: registering a message name has no preconditions
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    // Explorer runs non-elevated: without this filter an elevated ReShift would never hear
    // about an Explorer restart and its tray icon would vanish (UIPI)
    // SAFETY: our own window
    let _ = unsafe { ChangeWindowMessageFilterEx(window, taskbar_created, MSGFLT_ALLOW, None) };

    let tray = TrayIcon::new(window, taskbar_created);
    // A second launch of ReShift opens the settings of this one
    instance.listen_for_activation(window, WM_APP_ACTIVATE);
    if settings.launch_at_login {
        // Keep the Run entry pointing at the current executable
        startup::set_enabled(true);
    }

    let (events_sender, events) = mpsc::channel();
    let terminal = TerminalInputTracker::new(layouts::read_layout_id_map());
    let mut app = App {
        window,
        options,
        paths,
        settings,
        hook: None,
        worker: ConversionWorker::start(window, events_sender, terminal.clone()),
        events,
        _foreground: ForegroundTracker::start(window, WM_APP_FOREGROUND),
        terminal,
        hotkeys: HotkeyManager::new(window),
        hotkey_registration_failed: false,
        blocked_by_elevated_window: false,
        // The keyboard hook can be dropped by Windows; refresh it when the session comes back
        _session_notifications: SessionNotifications::register(window),
        tray,
        _instance: instance,
    };
    app.register_hotkey();
    app.update_keyboard_hook();
    app.update_tooltip();
    // SAFETY: no preconditions
    app.on_foreground_changed(unsafe { GetForegroundWindow() });
    APP.with(|cell| *cell.borrow_mut() = Some(app));

    let exit_code = message_loop();
    shutdown();
    exit_code
}

impl App {
    fn update_tooltip(&mut self) {
        let tooltip = if self.blocked_by_elevated_window {
            t("menu.tooltipInactive").to_owned()
        } else {
            fill(t("menu.tooltip"), &self.settings.hotkey_description())
        };
        self.tray.set_tooltip(&tooltip);
    }

    fn save_settings(&self) {
        if let Err(error) = settings::save(&self.paths.settings_file(), &self.settings) {
            debug!("settings: could not save: {error}");
        }
    }

    /// Applies a settings change from the settings window: saves it and updates whatever
    /// depends on the changed values.
    fn update_settings(&mut self, change: impl FnOnce(&mut Settings)) -> bool {
        let previous = self.settings.clone();
        change(&mut self.settings);
        if self.settings == previous {
            return false;
        }
        self.save_settings();
        if self.settings.launch_at_login != previous.launch_at_login {
            startup::set_enabled(self.settings.launch_at_login);
        }
        if self.settings.app_language != previous.app_language
            && let Some(language) = &self.settings.app_language
        {
            l10n::set_current_language(language);
        }
        if self.settings.hotkey_changed(&previous) {
            self.register_hotkey();
        }
        if self.settings.use_double_shift != previous.use_double_shift
            || self.settings.terminal_support != previous.terminal_support
        {
            self.update_keyboard_hook();
            // SAFETY: no preconditions
            self.on_foreground_changed(unsafe { GetForegroundWindow() });
        }
        self.update_tooltip();
        true
    }

    // MARK: - Hotkey Registration

    fn register_hotkey(&mut self) {
        let failed = if self.settings.use_double_shift {
            self.hotkeys.register_double_shift();
            false
        } else {
            !self.hotkeys.register(
                self.settings.hotkey_key_code,
                self.settings.hotkey_modifiers,
            )
        };
        if failed {
            self.show_alert(
                "alert.hotkeyFailedTitle",
                "alert.hotkeyFailedMessage",
                Some(&self.settings.hotkey_description()),
            );
        }
        self.hotkey_registration_failed = failed;
        // The tracker must not treat the hotkey's own key press as a shortcut that edits the line
        self.terminal
            .set_hotkey((!self.settings.use_double_shift && !failed).then_some((
                self.settings.hotkey_key_code,
                self.settings.hotkey_modifiers,
            )));
        debug!(
            "Hotkey ready: {} (registered={})",
            self.settings.hotkey_description(),
            !failed
        );
    }

    /// One low-level hook serves the double Shift detector and the terminal line tracker; it
    /// only runs while one of them is enabled.
    fn update_keyboard_hook(&mut self) {
        let needed = self.settings.needs_keyboard_hook();
        // Mouse clicks are watched only for the terminal tracker
        let needs_mouse = self.settings.terminal_support;
        if self
            .hook
            .as_ref()
            .is_some_and(|hook| !needed || hook.has_mouse() != needs_mouse)
        {
            self.hook = None;
        }
        if needed && self.hook.is_none() {
            self.hook = KeyboardHook::start(self.hook_handlers(needs_mouse));
        }
    }

    fn hook_handlers(&self, with_mouse: bool) -> HookHandlers {
        let double_shift = self.hotkeys.double_shift_state();
        let terminal = self.terminal.clone();
        let mouse_terminal = self.terminal.clone();
        // HWND is not Send; the hook thread only needs its value to post a message
        let window = self.window.0 as isize;
        HookHandlers {
            on_key: Box::new(move |event| {
                terminal.process_key(event);
                if hotkey::process_key(&double_shift, event) {
                    // SAFETY: posting to our window from any thread is allowed
                    let _ = unsafe {
                        PostMessageW(
                            Some(HWND(window as *mut _)),
                            WM_APP_DOUBLE_SHIFT,
                            WPARAM(0),
                            LPARAM(0),
                        )
                    };
                }
            }),
            on_mouse_button_down: with_mouse.then(|| {
                Box::new(move |point| mouse_terminal.process_mouse_button_down(point))
                    as Box<dyn FnMut(POINT) + Send>
            }),
        }
    }

    fn reinstall_keyboard_hook(&self) {
        if let Some(hook) = &self.hook {
            hook.reinstall();
        }
    }

    // MARK: - Conversion

    fn trigger_conversion(&mut self, trigger: ConversionTrigger) {
        let Some(worker) = &self.worker else {
            debug!("performConversion: the conversion thread is not running");
            return;
        };
        let request = ConversionRequest {
            trigger,
            settings: ConversionSettings::from(&self.settings),
        };
        // The conversion runs on its own thread; guard against re-entry
        if !worker.request(request) {
            debug!("performConversion: already running, ignoring trigger");
        }
    }

    /// Notifications reported by the conversion thread.
    fn handle_main_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                MainEvent::Notification { title, text } => {
                    self.tray.show_notification(&title, &text)
                }
            }
        }
    }

    /// Tells the terminal tracker which window is active and grays the icon while an
    /// administrator window is active (UIPI blocks our keystrokes there).
    fn on_foreground_changed(&mut self, window: HWND) {
        // Our own windows and the shell (taskbar, tray menu) keep the current state
        if window.is_invalid()
            || window_kind::is_own_window(window)
            || window_kind::is_shell_window(window)
        {
            self.terminal.pause();
            return;
        }
        let kind = window_kind::classify(window);
        self.terminal.set_window(
            window,
            self.settings.terminal_support
                && matches!(kind, WindowKind::Terminal | WindowKind::TerminalHost),
        );
        self.blocked_by_elevated_window = window_kind::is_blocked_by_uipi(window);
        self.tray.set_inactive(self.blocked_by_elevated_window);
        self.update_tooltip();
    }

    // MARK: - Feedback

    /// Important notices are shown regardless of the notification setting.
    fn show_alert(&self, title_key: &str, message_key: &str, argument: Option<&str>) {
        let message = match argument {
            Some(argument) => fill(t(message_key), argument),
            None => t(message_key).to_owned(),
        };
        self.tray
            .show_notification(t(title_key), &truncate(&message, MAX_NOTIFICATION_LENGTH));
    }
}

/// Cuts text to `max` characters, ending with "...".
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let mut cut: String = text.chars().take(max.saturating_sub(3)).collect();
        cut.push_str("...");
        cut
    }
}

/// WM_WTSSESSION_CHANGE registration of the main window, removed on drop.
struct SessionNotifications(Option<HWND>);

impl SessionNotifications {
    fn register(window: HWND) -> Self {
        // SAFETY: our own window
        match unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) } {
            Ok(()) => Self(Some(window)),
            Err(error) => {
                debug!("session notifications unavailable: {error}");
                Self(None)
            }
        }
    }
}

impl Drop for SessionNotifications {
    fn drop(&mut self) {
        if let Some(window) = self.0 {
            // SAFETY: registered on this window in `register`
            let _ = unsafe { WTSUnRegisterSessionNotification(window) };
        }
    }
}

/// Initial language: the saved one, or Russian if any preferred Windows language is Russian.
fn apply_language(settings: &mut Settings, paths: &AppPaths) {
    let saved = settings.app_language.as_deref().filter(|code| {
        l10n::AVAILABLE_LANGUAGES
            .iter()
            .any(|language| language.code == *code)
    });
    let language = match saved {
        Some(code) => code.to_owned(),
        None => {
            let detected =
                l10n::detect_default_language(preferred_languages().iter().map(String::as_str));
            settings.app_language = Some(detected.to_owned());
            if let Err(error) = settings::save(&paths.settings_file(), settings) {
                debug!("settings: could not save the detected language: {error}");
            }
            detected.to_owned()
        }
    };
    l10n::set_current_language(&language);
}

/// The user's language list (Settings > Time & language), e.g. ["en-US", "ru"].
fn preferred_languages() -> Vec<String> {
    let languages = RegKey::open(
        HKEY_CURRENT_USER,
        r"Control Panel\International\User Profile",
    )
    .and_then(|key| key.multi_string("Languages"))
    .unwrap_or_default();
    if !languages.is_empty() {
        return languages;
    }
    let mut name = [0u16; 85];
    // SAFETY: the buffer is writable and its length is passed along
    let length =
        unsafe { LCIDToLocaleName(u32::from(GetUserDefaultUILanguage()), Some(&mut name), 0) };
    if length > 0 {
        vec![from_wide(&name)]
    } else {
        Vec::new()
    }
}

// MARK: - Window and message loop

fn create_window() -> Option<HWND> {
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_procedure),
        hInstance: instance(),
        lpszClassName: WINDOW_CLASS,
        ..Default::default()
    };
    // Hidden top-level window: message-only windows (HWND_MESSAGE) do not get broadcasts such
    // as TaskbarCreated, so this is a real but never shown WS_POPUP tool window
    // SAFETY: the class structure is fully initialized; the strings are static
    unsafe {
        if RegisterClassExW(&class) == 0 {
            return None;
        }
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            WINDOW_CLASS,
            w!("ReShift"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance()),
            None,
        )
        .ok()
    }
}

fn message_loop() -> i32 {
    let mut message = MSG::default();
    // SAFETY: standard message loop on the thread that owns the window
    unsafe {
        // 0 = WM_QUIT, -1 = error
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            // Keyboard navigation and the hotkey recorder of the settings window
            if settings_window::pre_translate_message(&message) {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    message.wParam.0 as i32
}

unsafe extern "system" fn window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // A panic must neither cross the FFI boundary nor kill the tray app
    let result = std::panic::catch_unwind(|| handle_message(window, message, wparam, lparam));
    match result {
        Ok(Some(result)) => result,
        Ok(None) => {
            // SAFETY: default processing of a message of our window
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        Err(_) => {
            log::write("ERROR: panic in the window procedure");
            // SAFETY: as above
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
    }
}

fn handle_message(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    match message {
        _ if HotkeyManager::is_hotkey_message(message, wparam) => {
            with_app(|app| app.trigger_conversion(ConversionTrigger::CustomHotkey));
            Some(LRESULT(0))
        }
        WM_APP_MAIN_EVENT => {
            with_app(App::handle_main_events);
            Some(LRESULT(0))
        }
        WM_APP_FOREGROUND => {
            with_app(|app| app.on_foreground_changed(HWND(wparam.0 as *mut _)));
            Some(LRESULT(0))
        }
        WM_APP_DOUBLE_SHIFT => {
            with_app(|app| app.trigger_conversion(ConversionTrigger::DoubleShift));
            Some(LRESULT(0))
        }
        WM_APP_ACTIVATE => {
            show_settings();
            Some(LRESULT(0))
        }
        WM_APP_QUIT => {
            quit(window);
            Some(LRESULT(0))
        }
        WM_WTSSESSION_CHANGE => {
            if matches!(
                wparam.0 as u32,
                WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT
            ) {
                with_app(|app| app.reinstall_keyboard_hook());
            }
            Some(LRESULT(0))
        }
        WM_POWERBROADCAST => {
            if wparam.0 as u32 == PBT_APMRESUMEAUTOMATIC {
                with_app(|app| app.reinstall_keyboard_hook());
            }
            Some(LRESULT(1))
        }
        WM_QUERYENDSESSION => Some(LRESULT(1)),
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                shutdown();
            }
            Some(LRESULT(0))
        }
        WM_DESTROY => {
            shutdown();
            // SAFETY: ends the message loop of this thread
            unsafe { PostQuitMessage(0) };
            Some(LRESULT(0))
        }
        _ => match with_app(|app| app.tray.handle_message(message, wparam, lparam))? {
            TrayMessage::NotTray => None,
            TrayMessage::Handled => Some(LRESULT(0)),
            TrayMessage::ShowMenu(at) => {
                show_tray_menu(window, at);
                Some(LRESULT(0))
            }
        },
    }
}

/// The tray menu runs a modal loop, so it is shown without holding the app state.
fn show_tray_menu(window: HWND, at: POINT) {
    if MENU_OPEN.replace(true) {
        return;
    }
    let command = tray::show_menu(window, at);
    MENU_OPEN.set(false);
    match command {
        Some(MenuCommand::Settings) => show_settings(),
        Some(MenuCommand::About) => about::show(),
        Some(MenuCommand::Quit) => quit(window),
        None => {}
    }
}

fn show_settings() {
    settings_window::show();
}

/// Quit ReShift (tray menu): remove the icon first, then end the message loop.
fn quit(window: HWND) {
    settings_window::close();
    shutdown();
    // SAFETY: our own window; WM_DESTROY posts WM_QUIT
    let _ = unsafe { DestroyWindow(window) };
}

// MARK: - Settings window access
// The settings window runs on the main thread and calls these outside of any app borrow.

/// The settings and the hotkey state shown by the settings window.
pub struct SettingsView {
    pub settings: Settings,
    pub hotkey_registration_failed: bool,
}

pub fn settings_view() -> Option<SettingsView> {
    with_app(|app| SettingsView {
        settings: app.settings.clone(),
        hotkey_registration_failed: app.hotkey_registration_failed,
    })
}

/// Changes the settings: saved and applied at once. True if anything changed.
pub fn change_settings(change: impl FnOnce(&mut Settings)) -> bool {
    with_app(|app| app.update_settings(change)).unwrap_or(false)
}

/// The hotkey recorder needs the keystrokes the registered hotkey would swallow.
pub fn suspend_hotkey() {
    with_app(|app| app.hotkeys.unregister());
}

pub fn resume_hotkey() {
    with_app(App::register_hotkey);
}

/// Starts an elevated copy (UAC prompt) and quits this one, which the copy waits for.
pub fn restart_as_administrator() {
    let Some((arguments, window)) = with_app(|app| (app.options.forwarded_arguments(), app.window))
    else {
        return;
    };
    if elevation::restart_as_administrator(&arguments) {
        // SAFETY: our own window
        let _ = unsafe { PostMessageW(Some(window), WM_APP_QUIT, WPARAM(0), LPARAM(0)) };
    }
}

/// Drops the app state: stops the hook, unregisters the hotkey, removes the tray icon and
/// releases the single-instance lock.
fn shutdown() {
    let app = APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut app| app.take()));
    if let Some(app) = app {
        app.save_settings();
        drop(app);
        debug!("App stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_notices_are_truncated() {
        assert_eq!(truncate("short", 250), "short");
        let long = "ж".repeat(300);
        let cut = truncate(&long, 250);
        assert_eq!(cut.chars().count(), 250);
        assert!(cut.ends_with("..."));
    }
}
