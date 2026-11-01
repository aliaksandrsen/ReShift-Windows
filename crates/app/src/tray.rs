//! MARK: - Tray Icon
//!
//! Shell_NotifyIcon wrapper and the tray menu: "Settings...", "About" and "Quit".

use reshift_core::l10n::t;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Controls::{_LI_METRIC, LIM_LARGE, LIM_SMALL, LoadIconMetric};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_LARGE_ICON, NIIF_NOSOUND,
    NIIF_USER, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NOTIFY_ICON_DATA_FLAGS,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, MF_SEPARATOR, MF_STRING, PostMessageW,
    SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_APP,
    WM_CONTEXTMENU, WM_NULL,
};
use windows::core::{HSTRING, PCWSTR};

use crate::log::debug;
use crate::win::{OwnedIcon, copy_to_fixed, instance, resource_id};

/// Window message of the tray icon callbacks.
pub const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 2;
const ICON_ID: u32 = 1;
/// NIN_SELECT | NINF_KEY: the icon was selected with the keyboard.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
/// Resource IDs of the icons (res/app.rc).
const APP_ICON: u16 = 1;
const INACTIVE_ICON: u16 = 2;

/// What a window message meant for the tray icon.
pub enum TrayMessage {
    NotTray,
    Handled,
    /// The icon was clicked: show the menu at this screen point.
    ShowMenu(POINT),
}

pub struct TrayIcon {
    window: HWND,
    taskbar_created_message: u32,
    normal_icon: OwnedIcon,
    inactive_icon: OwnedIcon,
    balloon_icon: OwnedIcon,
    inactive: bool,
    tooltip: String,
    added: bool,
}

impl TrayIcon {
    /// Adds the icon. `taskbar_created_message` is the registered "TaskbarCreated" message.
    pub fn new(window: HWND, taskbar_created_message: u32) -> Self {
        let mut tray = Self {
            window,
            taskbar_created_message,
            normal_icon: load_icon(APP_ICON, LIM_SMALL),
            inactive_icon: load_icon(INACTIVE_ICON, LIM_SMALL),
            balloon_icon: load_icon(APP_ICON, LIM_LARGE),
            inactive: false,
            tooltip: "ReShift".to_owned(),
            added: false,
        };
        tray.add();
        tray
    }

    pub fn set_tooltip(&mut self, tooltip: &str) {
        if self.tooltip != tooltip {
            self.tooltip = tooltip.to_owned();
            self.update(NIF_TIP | NIF_SHOWTIP);
        }
    }

    /// Gray icon while ReShift cannot work in the active window (UIPI).
    pub fn set_inactive(&mut self, inactive: bool) {
        if self.inactive != inactive {
            self.inactive = inactive;
            self.update(NIF_ICON);
        }
    }

    /// Shows a notification (a toast on Windows 10/11) without sound.
    pub fn show_notification(&self, title: &str, text: &str) {
        let mut data = self.data(NIF_INFO);
        copy_to_fixed(&mut data.szInfoTitle, title);
        copy_to_fixed(&mut data.szInfo, text);
        data.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON | NIIF_NOSOUND;
        data.hBalloonIcon = self.balloon_icon.0;
        // SAFETY: `data` is fully initialized
        if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
            debug!("tray: failed to show notification");
        }
    }

    /// Handles the tray callback and the TaskbarCreated broadcast.
    pub fn handle_message(&mut self, message: u32, wparam: WPARAM, lparam: LPARAM) -> TrayMessage {
        if message == self.taskbar_created_message {
            // Explorer restarted - the icon is gone, add it again
            self.added = false;
            self.add();
            return TrayMessage::Handled;
        }
        if message != TRAY_CALLBACK_MESSAGE {
            return TrayMessage::NotTray;
        }
        // NOTIFYICON_VERSION_4: the event is in the low word of lParam, the anchor point in wParam
        match (lparam.0 & 0xFFFF) as u32 {
            NIN_SELECT | NIN_KEYSELECT | WM_CONTEXTMENU => TrayMessage::ShowMenu(POINT {
                x: i32::from((wparam.0 & 0xFFFF) as u16 as i16),
                y: i32::from(((wparam.0 >> 16) & 0xFFFF) as u16 as i16),
            }),
            _ => TrayMessage::Handled,
        }
    }

    fn add(&mut self) {
        let mut data = self.data(NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP);
        // SAFETY: `data` is fully initialized
        self.added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool();
        if !self.added {
            debug!("tray: Shell_NotifyIcon(NIM_ADD) failed");
            return;
        }
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        // SAFETY: `data` is fully initialized
        let _ = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) };
    }

    fn update(&self, flags: NOTIFY_ICON_DATA_FLAGS) {
        if self.added {
            let data = self.data(flags);
            // SAFETY: `data` is fully initialized
            let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
        }
    }

    fn data(&self, flags: NOTIFY_ICON_DATA_FLAGS) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.window,
            uID: ICON_ID,
            uFlags: flags,
            uCallbackMessage: TRAY_CALLBACK_MESSAGE,
            hIcon: if self.inactive {
                self.inactive_icon.0
            } else {
                self.normal_icon.0
            },
            ..Default::default()
        };
        copy_to_fixed(&mut data.szTip, &self.tooltip);
        data
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        if self.added {
            let data = self.data(NOTIFY_ICON_DATA_FLAGS(0));
            // SAFETY: `data` identifies our icon
            let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
        }
    }
}

fn load_icon(id: u16, metric: _LI_METRIC) -> OwnedIcon {
    // SAFETY: the resource is part of our executable
    match unsafe { LoadIconMetric(Some(instance()), resource_id(id), metric) } {
        Ok(icon) => OwnedIcon(icon),
        Err(error) => {
            debug!("tray: could not load icon {id}: {error}");
            OwnedIcon::default()
        }
    }
}

// MARK: - Menu

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    Settings = 1,
    About = 2,
    Quit = 3,
}

struct OwnedMenu(HMENU);

impl Drop for OwnedMenu {
    fn drop(&mut self) {
        // SAFETY: the menu was created by us and is destroyed exactly once
        let _ = unsafe { DestroyMenu(self.0) };
    }
}

/// Shows the tray menu and returns the chosen command. Runs a modal loop: the caller must not
/// hold any borrow of the app state.
pub fn show_menu(window: HWND, at: POINT) -> Option<MenuCommand> {
    // SAFETY: the menu is owned by the guard below
    let menu = OwnedMenu(unsafe { CreatePopupMenu() }.ok()?);
    let items = [
        (MenuCommand::Settings, "menu.settings"),
        (MenuCommand::About, "menu.about"),
    ];
    // SAFETY: the menu handle is valid, the strings outlive the calls
    unsafe {
        for (command, key) in items {
            AppendMenuW(menu.0, MF_STRING, command as usize, &HSTRING::from(t(key))).ok()?;
        }
        AppendMenuW(menu.0, MF_SEPARATOR, 0, PCWSTR::null()).ok()?;
        AppendMenuW(
            menu.0,
            MF_STRING,
            MenuCommand::Quit as usize,
            &HSTRING::from(t("menu.quit")),
        )
        .ok()?;
    }

    // Without being foreground, the menu would not close when the user clicks elsewhere
    // SAFETY: plain window calls on our own window
    let command = unsafe {
        let _ = SetForegroundWindow(window);
        let command = TrackPopupMenuEx(
            menu.0,
            (TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON).0,
            at.x,
            at.y,
            window,
            None,
        );
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        command.0
    };
    [MenuCommand::Settings, MenuCommand::About, MenuCommand::Quit]
        .into_iter()
        .find(|item| *item as i32 == command)
}
