//! MARK: - Settings Window
//!
//! Win32 settings window built from standard Windows controls: four tabs -
//! General, Layouts, Hotkey, Permissions.
//! Lives on the main thread, is created on demand and destroyed - its memory freed - when closed.
//! Changes apply immediately.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;

use reshift_core::l10n::{self, fill, t};
use reshift_core::{ClipboardKeys, LayoutSwitchMode, SmartConversionMode};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, COLOR_WINDOW, CreateFontIndirectW, DT_CALCRECT, DT_EDITCONTROL, DT_NOPREFIX,
    DT_WORDBREAK, DeleteObject, DrawTextW, GetDC, GetMonitorInfoW, GetSysColor, GetSysColorBrush,
    HBRUSH, HDC, HFONT, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RedrawWindow, ReleaseDC, SelectObject, SetBkColor,
    SetTextColor,
};
use windows::Win32::UI::Controls::{
    ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, ICC_TAB_CLASSES, INITCOMMONCONTROLSEX,
    InitCommonControlsEx, LIM_LARGE, LIM_SMALL, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVITEMW,
    LoadIconMetric, NMHDR, SetWindowTheme, TCIF_TEXT, TCITEMW, TCN_SELCHANGE, WC_LISTVIEWW,
    WC_TABCONTROLW,
};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForWindow, SystemParametersInfoForDpi,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetDlgCtrlID,
    HMENU, HWND_BOTTOM, IsChild, IsDialogMessageW, IsIconic, MSG, NONCLIENTMETRICSW, PostMessageW,
    RegisterClassExW, SPI_GETNONCLIENTMETRICS, SW_HIDE, SW_RESTORE, SW_SHOWNA, SW_SHOWNORMAL,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetForegroundWindow,
    SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE, WM_APP,
    WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_KEYDOWN,
    WM_KEYUP, WM_NOTIFY, WM_SETFONT, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSEXW,
    WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_CONTROLPARENT,
    WS_EX_TOPMOST, WS_GROUP, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::app;
use crate::hotkey_format::{self, HotkeyModifiers};
use crate::log::debug;
use crate::win::{OwnedIcon, instance, resource_id};
use crate::{elevation, layouts};

const CLASS_NAME: PCWSTR = w!("ReShift.Settings");
/// Re-read the settings and the system state (posted on activation).
const WM_APP_REFRESH: u32 = WM_APP + 20;

// Logical (96 DPI) metrics
const CLIENT_WIDTH: i32 = 800;
const MARGIN: i32 = 10;
const PADDING: i32 = 14;
const COLUMN_GAP: i32 = 18;
const ROW_GAP: i32 = 4;
const SECTION_GAP: i32 = 12;
const LINE_HEIGHT: i32 = 20;
const BUTTON_HEIGHT: i32 = 26;
const COMBO_HEIGHT: i32 = 24;
const GROUP_PADDING: i32 = 10;
const CHECK_GLYPH: i32 = 22;
const LIST_HEIGHT: i32 = 210;

// Control styles and messages (plain numbers: the Win32 headers mix several types for them)
const BS_PUSHBUTTON: u32 = 0x0;
const BS_AUTOCHECKBOX: u32 = 0x3;
const BS_AUTORADIOBUTTON: u32 = 0x9;
const BS_MULTILINE: u32 = 0x2000;
const SS_NOPREFIX: u32 = 0x80;
const SS_WHITERECT: u32 = 0x6;
const SS_SUNKEN: u32 = 0x1000;
const SS_CENTERIMAGE: u32 = 0x200;
const CBS_DROPDOWNLIST: u32 = 0x3;
const LVS_REPORT: u32 = 0x1;
const LVS_SINGLESEL: u32 = 0x4;
const LVS_NOCOLUMNHEADER: u32 = 0x4000;
const LVS_EX_FULLROWSELECT: isize = 0x20;
const LVS_EX_DOUBLEBUFFER: isize = 0x0001_0000;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: isize = 1;
const BN_CLICKED: u32 = 0;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_RESETCONTENT: u32 = 0x014B;
const CB_SETCURSEL: u32 = 0x014E;
const CBN_SELCHANGE: u32 = 1;
const TCM_GETCURSEL: u32 = 0x130B;
const TCM_SETITEMW: u32 = 0x133D;
const TCM_INSERTITEMW: u32 = 0x133E;
const TCM_ADJUSTRECT: u32 = 0x1328;
const LVM_DELETEALLITEMS: u32 = 0x1009;
const LVM_SETCOLUMNWIDTH: u32 = 0x101E;
const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = 0x1036;
const LVM_INSERTITEMW: u32 = 0x104D;
const LVM_INSERTCOLUMNW: u32 = 0x1061;
const LVM_SETITEMTEXTW: u32 = 0x1074;
const IDCANCEL: u16 = 2;
const VK_ESCAPE: u32 = 0x1B;
const VK_PROCESSKEY: u32 = 0xE5;
/// Warning text color (red), RGB(196, 43, 28).
const WARNING_COLOR: COLORREF = COLORREF(0x001C_2BC4);

// MARK: - Controls

mod id {
    pub const TAB: u16 = 100;
    pub const PANEL: u16 = 101;
    // General
    pub const LANGUAGE_LABEL: u16 = 110;
    pub const LANGUAGE_COMBO: u16 = 111;
    pub const LAUNCH_AT_LOGIN: u16 = 112;
    pub const PLAY_SOUND: u16 = 113;
    pub const SHOW_NOTIFICATIONS: u16 = 114;
    pub const TERMINAL_SUPPORT: u16 = 115;
    pub const TERMINAL_HINT: u16 = 116;
    pub const HOW_GROUP: u16 = 117;
    pub const HOW_TEXT: u16 = 118;
    pub const SMART_GROUP: u16 = 120;
    pub const SMART_MODE: u16 = 121;
    pub const SMART_LAST_WORD: u16 = 122;
    pub const SMART_GREEDY: u16 = 123;
    pub const SMART_DISABLED: u16 = 124;
    pub const SMART_DESC: u16 = 125;
    pub const SWITCH_GROUP: u16 = 130;
    pub const SWITCH_MODE: u16 = 131;
    pub const SWITCH_ALWAYS: u16 = 132;
    pub const SWITCH_IF_LAST: u16 = 133;
    pub const SWITCH_IF_ANY: u16 = 134;
    pub const SWITCH_DESC: u16 = 135;
    pub const CLIPBOARD_GROUP: u16 = 140;
    pub const CLIPBOARD_CTRL_INSERT: u16 = 141;
    pub const CLIPBOARD_CTRL_C: u16 = 142;
    pub const CLIPBOARD_DESC: u16 = 143;
    // Layouts
    pub const LAYOUTS_TITLE: u16 = 200;
    pub const LAYOUTS_DESC: u16 = 201;
    pub const LAYOUTS_LIST: u16 = 202;
    pub const LAYOUTS_REFRESH: u16 = 203;
    pub const LAYOUTS_COUNT: u16 = 204;
    // Hotkey
    pub const HOTKEY_TITLE: u16 = 300;
    pub const HOTKEY_DESC: u16 = 301;
    pub const CURRENT_LABEL: u16 = 302;
    pub const CURRENT_VALUE: u16 = 303;
    pub const DOUBLE_SHIFT: u16 = 304;
    pub const DOUBLE_SHIFT_HINT: u16 = 305;
    pub const STICKY_WARNING: u16 = 306;
    pub const CUSTOM_GROUP: u16 = 307;
    pub const RECORDER_PROMPT: u16 = 308;
    pub const RECORDER_DISPLAY: u16 = 309;
    pub const RECORD_BUTTON: u16 = 310;
    pub const MODIFIER_HINT: u16 = 311;
    pub const REGISTER_FAILED: u16 = 312;
    // Permissions
    pub const PERMISSIONS_TITLE: u16 = 400;
    pub const PERMISSIONS_DESC: u16 = 401;
    pub const ADMIN_TITLE: u16 = 402;
    pub const ADMIN_STATUS: u16 = 403;
    pub const RESTART_BUTTON: u16 = 404;
    pub const GOOD_TO_KNOW: u16 = 405;
    pub const STEP1: u16 = 406;
    pub const STEP2: u16 = 407;
    pub const STEP3: u16 = 408;
    pub const STEP4: u16 = 409;
    pub const REFRESH_STATUS: u16 = 410;
}

/// Controls drawn in the warning color.
const WARNING_IDS: [u16; 3] = [id::STICKY_WARNING, id::MODIFIER_HINT, id::REGISTER_FAILED];

const TAB_KEYS: [&str; 4] = [
    "settings.tab.general",
    "settings.tab.layouts",
    "settings.tab.hotkey",
    "settings.tab.permissions",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Label,
    Title,
    Check,
    Radio,
    /// The first radio button of a group.
    FirstRadio,
    Group,
    Button,
    Combo,
    List,
    Display,
}

/// Every control of the window: page, kind and the localization key of its fixed text.
const CONTROLS: &[(u16, usize, Kind, Option<&str>)] = &[
    // General - left column
    (id::LANGUAGE_LABEL, 0, Kind::Label, Some("general.language")),
    (id::LANGUAGE_COMBO, 0, Kind::Combo, None),
    (
        id::LAUNCH_AT_LOGIN,
        0,
        Kind::Check,
        Some("general.launchAtLogin"),
    ),
    (id::PLAY_SOUND, 0, Kind::Check, Some("general.playSound")),
    (
        id::SHOW_NOTIFICATIONS,
        0,
        Kind::Check,
        Some("general.showNotifications"),
    ),
    (
        id::TERMINAL_SUPPORT,
        0,
        Kind::Check,
        Some("general.terminalSupport"),
    ),
    (
        id::TERMINAL_HINT,
        0,
        Kind::Label,
        Some("general.terminalSupportHint"),
    ),
    (id::HOW_GROUP, 0, Kind::Group, Some("general.howItWorks")),
    (id::HOW_TEXT, 0, Kind::Label, Some("general.howItWorksText")),
    // General - right column
    (
        id::SMART_GROUP,
        0,
        Kind::Group,
        Some("general.smartConversion"),
    ),
    (id::SMART_MODE, 0, Kind::Label, Some("general.mode")),
    (
        id::SMART_LAST_WORD,
        0,
        Kind::FirstRadio,
        Some("smartMode.lastWord.name"),
    ),
    (
        id::SMART_GREEDY,
        0,
        Kind::Radio,
        Some("smartMode.greedyLine.name"),
    ),
    (
        id::SMART_DISABLED,
        0,
        Kind::Radio,
        Some("smartMode.disabled.name"),
    ),
    (id::SMART_DESC, 0, Kind::Label, None),
    (
        id::SWITCH_GROUP,
        0,
        Kind::Group,
        Some("general.layoutSwitch"),
    ),
    (id::SWITCH_MODE, 0, Kind::Label, Some("general.mode")),
    (
        id::SWITCH_ALWAYS,
        0,
        Kind::FirstRadio,
        Some("layoutSwitchMode.always.name"),
    ),
    (
        id::SWITCH_IF_LAST,
        0,
        Kind::Radio,
        Some("layoutSwitchMode.ifLastWord.name"),
    ),
    (
        id::SWITCH_IF_ANY,
        0,
        Kind::Radio,
        Some("layoutSwitchMode.ifAnyWord.name"),
    ),
    (id::SWITCH_DESC, 0, Kind::Label, None),
    (
        id::CLIPBOARD_GROUP,
        0,
        Kind::Group,
        Some("general.clipboardKeys"),
    ),
    (
        id::CLIPBOARD_CTRL_INSERT,
        0,
        Kind::FirstRadio,
        Some("clipboardKeys.ctrlInsert.name"),
    ),
    (
        id::CLIPBOARD_CTRL_C,
        0,
        Kind::Radio,
        Some("clipboardKeys.ctrlC.name"),
    ),
    (id::CLIPBOARD_DESC, 0, Kind::Label, None),
    // Layouts
    (id::LAYOUTS_TITLE, 1, Kind::Title, Some("layouts.title")),
    (
        id::LAYOUTS_DESC,
        1,
        Kind::Label,
        Some("layouts.description"),
    ),
    (id::LAYOUTS_LIST, 1, Kind::List, None),
    (
        id::LAYOUTS_REFRESH,
        1,
        Kind::Button,
        Some("layouts.refresh"),
    ),
    (id::LAYOUTS_COUNT, 1, Kind::Label, None),
    // Hotkey
    (id::HOTKEY_TITLE, 2, Kind::Title, Some("hotkey.title")),
    (id::HOTKEY_DESC, 2, Kind::Label, Some("hotkey.description")),
    (id::CURRENT_LABEL, 2, Kind::Label, Some("hotkey.current")),
    (id::CURRENT_VALUE, 2, Kind::Title, None),
    (
        id::DOUBLE_SHIFT,
        2,
        Kind::Check,
        Some("hotkey.useDoubleShift"),
    ),
    (
        id::DOUBLE_SHIFT_HINT,
        2,
        Kind::Label,
        Some("hotkey.doubleShiftHint"),
    ),
    (
        id::STICKY_WARNING,
        2,
        Kind::Label,
        Some("hotkey.stickyKeys"),
    ),
    (id::CUSTOM_GROUP, 2, Kind::Group, Some("hotkey.customTitle")),
    (
        id::RECORDER_PROMPT,
        2,
        Kind::Label,
        Some("hotkeyRecorder.prompt"),
    ),
    (id::RECORDER_DISPLAY, 2, Kind::Display, None),
    (id::RECORD_BUTTON, 2, Kind::Button, None),
    (
        id::MODIFIER_HINT,
        2,
        Kind::Label,
        Some("hotkey.requireModifier"),
    ),
    (
        id::REGISTER_FAILED,
        2,
        Kind::Label,
        Some("hotkey.registerFailed"),
    ),
    // Permissions
    (
        id::PERMISSIONS_TITLE,
        3,
        Kind::Title,
        Some("permissions.title"),
    ),
    (
        id::PERMISSIONS_DESC,
        3,
        Kind::Label,
        Some("permissions.description"),
    ),
    (
        id::ADMIN_TITLE,
        3,
        Kind::Title,
        Some("permissions.adminTitle"),
    ),
    (id::ADMIN_STATUS, 3, Kind::Label, None),
    (
        id::RESTART_BUTTON,
        3,
        Kind::Button,
        Some("permissions.restartAsAdmin"),
    ),
    (
        id::GOOD_TO_KNOW,
        3,
        Kind::Title,
        Some("permissions.howToEnable"),
    ),
    (id::STEP1, 3, Kind::Label, Some("permissions.step1")),
    (id::STEP2, 3, Kind::Label, Some("permissions.step2")),
    (id::STEP3, 3, Kind::Label, Some("permissions.step3")),
    (id::STEP4, 3, Kind::Label, Some("permissions.step4")),
    (
        id::REFRESH_STATUS,
        3,
        Kind::Button,
        Some("permissions.refreshStatus"),
    ),
];

struct Control {
    id: u16,
    hwnd: HWND,
    page: usize,
    kind: Kind,
    key: Option<&'static str>,
}

/// What the window shows besides the settings.
#[derive(Default)]
struct ViewState {
    settings: Option<crate::settings::Settings>,
    hotkey_registration_failed: bool,
    sticky_keys_on: bool,
    elevated: bool,
    show_modifier_hint: bool,
}

struct SettingsWindow {
    hwnd: HWND,
    tab: HWND,
    panel: HWND,
    controls: Vec<Control>,
    font: OwnedFont,
    bold_font: OwnedFont,
    dpi: u32,
    page: usize,
    state: ViewState,
    _icons: (OwnedIcon, OwnedIcon),
}

struct OwnedFont(HFONT);

impl Drop for OwnedFont {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: a font created by us, deleted once
            let _ = unsafe { DeleteObject(HGDIOBJ(self.0.0)) };
        }
    }
}

thread_local! {
    static WINDOW: RefCell<Option<SettingsWindow>> = const { RefCell::new(None) };
    /// Quick access from the message loop (no borrow of WINDOW needed).
    static WINDOW_HANDLE: Cell<isize> = const { Cell::new(0) };
    static RECORDING: Cell<bool> = const { Cell::new(false) };
}

fn with_window<R>(f: impl FnOnce(&mut SettingsWindow) -> R) -> Option<R> {
    WINDOW.with(|cell| {
        let mut window = cell.try_borrow_mut().ok()?;
        window.as_mut().map(f)
    })
}

// MARK: - Public API

/// Shows the settings window, or brings it to the front if it is open.
pub fn show() {
    let existing = WINDOW_HANDLE.get();
    if existing != 0 {
        let window = HWND(existing as *mut c_void);
        // SAFETY: our own window
        unsafe {
            if IsIconic(window).as_bool() {
                let _ = ShowWindow(window, SW_RESTORE);
            }
            let _ = SetForegroundWindow(window);
        }
        return;
    }
    let Some(window) = SettingsWindow::create() else {
        debug!("ERROR: could not create the settings window");
        return;
    };
    let hwnd = window.hwnd;
    WINDOW_HANDLE.set(hwnd.0 as isize);
    WINDOW.with(|cell| *cell.borrow_mut() = Some(window));
    refresh();
    with_window(SettingsWindow::center);
    // SAFETY: our own window
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Closes the window (when ReShift quits).
pub fn close() {
    let handle = WINDOW_HANDLE.get();
    if handle != 0 {
        // SAFETY: our own window, destroyed on its thread
        let _ = unsafe { DestroyWindow(HWND(handle as *mut c_void)) };
    }
}

/// Handles the keys of the hotkey recorder and dialog navigation (Tab, arrows, Esc). Called by
/// the main message loop for every message; true if the message was consumed.
pub fn pre_translate_message(message: &MSG) -> bool {
    let handle = WINDOW_HANDLE.get();
    if handle == 0 {
        return false;
    }
    let window = HWND(handle as *mut c_void);
    // SAFETY: plain window queries
    let ours = message.hwnd == window || unsafe { IsChild(window, message.hwnd) }.as_bool();
    if !ours {
        return false;
    }
    if RECORDING.get() {
        match message.message {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                on_record_key(message.wParam.0 as u32);
                return true;
            }
            // The recorder swallows everything else a key produces
            WM_KEYUP | WM_SYSKEYUP | WM_CHAR | WM_SYSCHAR => return true,
            _ => {}
        }
    }
    // SAFETY: our own window
    unsafe { IsDialogMessageW(window, message) }.as_bool()
}

/// Re-reads the settings and the system state and updates the controls.
pub fn refresh() {
    let view = app::settings_view();
    with_window(|window| {
        if let Some(view) = view {
            window.state.settings = Some(view.settings);
            window.state.hotkey_registration_failed = view.hotkey_registration_failed;
        }
        window.state.sticky_keys_on = elevation::is_sticky_keys_on();
        window.state.elevated = elevation::is_current_process_elevated();
        window.update_values();
        window.apply_texts();
        window.layout();
    });
}

// MARK: - Window

impl SettingsWindow {
    fn create() -> Option<Self> {
        let icons = (load_icon(LIM_LARGE), load_icon(LIM_SMALL));
        // SAFETY: common controls and class registration; repeated registration fails harmlessly
        unsafe {
            let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_TAB_CLASSES | ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES,
            });
            RegisterClassExW(&WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_procedure),
                hInstance: instance(),
                hIcon: icons.0.0,
                hIconSm: icons.1.0,
                hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as usize as *mut c_void),
                lpszClassName: CLASS_NAME,
                ..Default::default()
            });
        }
        // SAFETY: the class is registered; the window is owned by this struct until WM_DESTROY
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_CONTROLPARENT,
                CLASS_NAME,
                &HSTRING::from(t("settings.windowTitle")),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                100,
                100,
                None,
                None,
                Some(instance()),
                None,
            )
        }
        .ok()?;
        // SAFETY: our own window
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let (font, bold_font) = create_fonts(dpi);
        let tab = create_control(
            hwnd,
            WC_TABCONTROLW,
            WS_VISIBLE.0 | WS_TABSTOP.0 | WS_GROUP.0 | WS_CLIPSIBLINGS.0,
            id::TAB,
        );
        for (page, key) in TAB_KEYS.iter().enumerate() {
            let mut text = wide(t(key));
            let item = TCITEMW {
                mask: TCIF_TEXT,
                pszText: PWSTR(text.as_mut_ptr()),
                ..Default::default()
            };
            send(tab, TCM_INSERTITEMW, page, (&raw const item) as isize);
        }

        // A plain window-colored panel over the tab's display area: the themed pane of Windows 11
        // is slightly gray, the labels and check boxes are drawn on the window color
        let panel = create_control(
            hwnd,
            w!("STATIC"),
            SS_WHITERECT | WS_VISIBLE.0 | WS_CLIPSIBLINGS.0,
            id::PANEL,
        );

        let controls = CONTROLS
            .iter()
            .map(|&(control_id, page, kind, key)| Control {
                id: control_id,
                hwnd: create_for_kind(hwnd, kind, control_id),
                page,
                kind,
                key,
            })
            .collect();
        let window = Self {
            hwnd,
            tab,
            panel,
            controls,
            font,
            bold_font,
            dpi,
            page: 0,
            state: ViewState::default(),
            _icons: icons,
        };
        window.setup_lists();
        window.apply_fonts();
        window.arrange_z_order();
        Some(window)
    }

    /// New child windows go to the bottom of the z-order, so the tab control and the panel
    /// (created first) ended up above the controls and painted over them on every repaint -
    /// the controls only came back when the mouse hovered them. Controls on top (they never
    /// overlap each other), the panel and the tab control at the bottom; both clip their siblings.
    fn arrange_z_order(&self) {
        let to_bottom = |hwnd: HWND| {
            // SAFETY: our own child windows
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    Some(HWND_BOTTOM),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            };
        };
        to_bottom(self.panel);
        to_bottom(self.tab);
    }

    fn get(&self, control_id: u16) -> HWND {
        self.controls
            .iter()
            .find(|control| control.id == control_id)
            .map_or(HWND::default(), |control| control.hwnd)
    }

    fn setup_lists(&self) {
        let list = self.get(id::LAYOUTS_LIST);
        send(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER,
        );
        // SAFETY: our own control
        let _ = unsafe { SetWindowTheme(list, w!("Explorer"), PCWSTR::null()) };
        for column in 0..2 {
            let item = LVCOLUMNW {
                mask: LVCF_WIDTH,
                cx: 100,
                ..Default::default()
            };
            send(list, LVM_INSERTCOLUMNW, column, (&raw const item) as isize);
        }
    }

    fn apply_fonts(&self) {
        for control in &self.controls {
            let font = if matches!(control.kind, Kind::Title | Kind::Display | Kind::Group) {
                &self.bold_font
            } else {
                &self.font
            };
            send(control.hwnd, WM_SETFONT, font.0.0 as usize, 1);
        }
        send(self.tab, WM_SETFONT, self.font.0.0 as usize, 1);
    }

    // MARK: - Values

    /// Puts the settings and the system state into the controls.
    fn update_values(&mut self) {
        let Some(settings) = self.state.settings.clone() else {
            return;
        };
        let combo = self.get(id::LANGUAGE_COMBO);
        send(combo, CB_RESETCONTENT, 0, 0);
        for language in &l10n::AVAILABLE_LANGUAGES {
            let text = wide(language.name);
            send(combo, CB_ADDSTRING, 0, text.as_ptr() as isize);
        }
        let selected = l10n::AVAILABLE_LANGUAGES
            .iter()
            .position(|language| Some(language.code) == settings.app_language.as_deref())
            .unwrap_or(0);
        send(combo, CB_SETCURSEL, selected, 0);

        set_checked(self.get(id::LAUNCH_AT_LOGIN), settings.launch_at_login);
        set_checked(self.get(id::PLAY_SOUND), settings.play_sounds);
        set_checked(
            self.get(id::SHOW_NOTIFICATIONS),
            settings.show_notifications,
        );
        set_checked(self.get(id::TERMINAL_SUPPORT), settings.terminal_support);
        set_checked(self.get(id::DOUBLE_SHIFT), settings.use_double_shift);
        for (control_id, mode) in [
            (id::SMART_LAST_WORD, SmartConversionMode::LastWord),
            (id::SMART_GREEDY, SmartConversionMode::GreedyLine),
            (id::SMART_DISABLED, SmartConversionMode::Disabled),
        ] {
            set_checked(self.get(control_id), settings.smart_conversion_mode == mode);
        }
        for (control_id, mode) in [
            (id::SWITCH_ALWAYS, LayoutSwitchMode::Always),
            (id::SWITCH_IF_LAST, LayoutSwitchMode::IfLastWordConverted),
            (id::SWITCH_IF_ANY, LayoutSwitchMode::IfAnyWordConverted),
        ] {
            set_checked(self.get(control_id), settings.layout_switch_mode == mode);
        }
        for (control_id, keys) in [
            (id::CLIPBOARD_CTRL_INSERT, ClipboardKeys::CtrlInsert),
            (id::CLIPBOARD_CTRL_C, ClipboardKeys::CtrlC),
        ] {
            set_checked(self.get(control_id), settings.clipboard_keys == keys);
        }

        let list = self.get(id::LAYOUTS_LIST);
        send(list, LVM_DELETEALLITEMS, 0, 0);
        for (row, layout) in settings.enabled_layouts.iter().enumerate() {
            let mut name = wide(&layout.display_name());
            let mut klid = wide(&layout.id);
            let item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: row as i32,
                pszText: PWSTR(name.as_mut_ptr()),
                ..Default::default()
            };
            send(list, LVM_INSERTITEMW, 0, (&raw const item) as isize);
            let sub_item = LVITEMW {
                iSubItem: 1,
                pszText: PWSTR(klid.as_mut_ptr()),
                ..Default::default()
            };
            send(list, LVM_SETITEMTEXTW, row, (&raw const sub_item) as isize);
        }
    }

    /// Texts in the current language (fixed ones from the table, dynamic ones from the state).
    fn apply_texts(&self) {
        // SAFETY: our own window
        let _ = unsafe { SetWindowTextW(self.hwnd, &HSTRING::from(t("settings.windowTitle"))) };
        for (page, key) in TAB_KEYS.iter().enumerate() {
            let mut text = wide(t(key));
            let item = TCITEMW {
                mask: TCIF_TEXT,
                pszText: PWSTR(text.as_mut_ptr()),
                ..Default::default()
            };
            send(self.tab, TCM_SETITEMW, page, (&raw const item) as isize);
        }
        for control in &self.controls {
            if let Some(key) = control.key {
                set_text(control.hwnd, t(key));
            }
        }
        let Some(settings) = &self.state.settings else {
            return;
        };
        set_text(
            self.get(id::SMART_DESC),
            t(settings.smart_conversion_mode.description_key()),
        );
        set_text(
            self.get(id::SWITCH_DESC),
            t(settings.layout_switch_mode.description_key()),
        );
        set_text(
            self.get(id::CLIPBOARD_DESC),
            t(settings.clipboard_keys.description_key()),
        );
        set_text(
            self.get(id::LAYOUTS_COUNT),
            &fill(
                t("layouts.count"),
                &settings.enabled_layouts.len().to_string(),
            ),
        );
        set_text(self.get(id::CURRENT_VALUE), &settings.hotkey_description());
        let recording = RECORDING.get();
        set_text(
            self.get(id::RECORDER_DISPLAY),
            &if recording {
                t("hotkeyRecorder.pressKey").to_owned()
            } else {
                hotkey_format::describe(settings.hotkey_key_code, settings.hotkey_modifiers)
            },
        );
        set_text(
            self.get(id::RECORD_BUTTON),
            t(if recording {
                "hotkeyRecorder.cancel"
            } else {
                "hotkeyRecorder.record"
            }),
        );
        set_text(
            self.get(id::ADMIN_STATUS),
            t(if self.state.elevated {
                "permissions.adminOn"
            } else {
                "permissions.adminOff"
            }),
        );
    }

    /// Whether a control is shown (on its page).
    fn is_visible(&self, control_id: u16) -> bool {
        let use_double_shift = self
            .state
            .settings
            .as_ref()
            .is_none_or(|settings| settings.use_double_shift);
        match control_id {
            id::STICKY_WARNING => self.state.sticky_keys_on && use_double_shift,
            id::CUSTOM_GROUP | id::RECORDER_PROMPT | id::RECORDER_DISPLAY | id::RECORD_BUTTON => {
                !use_double_shift
            }
            id::MODIFIER_HINT => !use_double_shift && self.state.show_modifier_hint,
            id::REGISTER_FAILED => !use_double_shift && self.state.hotkey_registration_failed,
            id::RESTART_BUTTON => !self.state.elevated,
            _ => true,
        }
    }

    // MARK: - Layout

    fn px(&self, logical: i32) -> i32 {
        logical * self.dpi as i32 / 96
    }

    /// Height of wrapped text in a control of the given width.
    fn text_height(&self, control_id: u16, width: i32) -> i32 {
        let Some(control) = self
            .controls
            .iter()
            .find(|control| control.id == control_id)
        else {
            return 0;
        };
        let text = window_text(control.hwnd);
        let font = if matches!(control.kind, Kind::Title | Kind::Display | Kind::Group) {
            &self.bold_font
        } else {
            &self.font
        };
        measure_text(&text, font, width.max(1))
    }

    /// Places every control, sizes the window to the tallest page and shows the current page.
    fn layout(&mut self) {
        let margin = self.px(MARGIN);
        let padding = self.px(PADDING);
        let client_width = self.px(CLIENT_WIDTH);
        // Insets of the tab control's display area
        let mut probe = RECT {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 1000,
        };
        send(self.tab, TCM_ADJUSTRECT, 0, (&raw mut probe) as isize);
        let content_x = margin + probe.left + padding;
        let content_y = margin + probe.top + padding;
        let content_width =
            client_width - 2 * margin - probe.left - (1000 - probe.right) - 2 * padding;

        let mut placements: Vec<(HWND, RECT)> = Vec::new();
        let bottoms = [
            self.layout_general(content_x, content_y, content_width, &mut placements),
            self.layout_layouts(content_x, content_y, content_width, &mut placements),
            self.layout_hotkey(content_x, content_y, content_width, &mut placements),
            self.layout_permissions(content_x, content_y, content_width, &mut placements),
        ];
        let page_bottom = bottoms.into_iter().max().unwrap_or(content_y);
        let client_height = page_bottom + padding + (1000 - probe.bottom) + margin;

        for (hwnd, rect) in placements {
            place(hwnd, rect);
        }
        place(
            self.tab,
            RECT {
                left: margin,
                top: margin,
                right: client_width - margin,
                bottom: client_height - margin,
            },
        );
        place(
            self.panel,
            RECT {
                left: margin + probe.left,
                top: margin + probe.top,
                right: client_width - margin - (1000 - probe.right),
                bottom: client_height - margin - (1000 - probe.bottom),
            },
        );
        self.update_visibility();

        let mut frame = RECT {
            left: 0,
            top: 0,
            right: client_width,
            bottom: client_height,
        };
        // SAFETY: plain size computation and resize of our own window
        unsafe {
            let _ = AdjustWindowRectExForDpi(
                &mut frame,
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                false,
                WS_EX_TOPMOST | WS_EX_CONTROLPARENT,
                self.dpi,
            );
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                frame.right - frame.left,
                frame.bottom - frame.top,
                SWP_NOZORDER | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE,
            );
            // Controls moved and texts changed: repaint everything at once, bottom to top
            let _ = RedrawWindow(
                Some(self.hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
            );
        }
    }

    fn update_visibility(&self) {
        for control in &self.controls {
            let visible = control.page == self.page && self.is_visible(control.id);
            // SAFETY: our own control
            let _ = unsafe { ShowWindow(control.hwnd, if visible { SW_SHOWNA } else { SW_HIDE }) };
        }
    }

    /// A label (or check box / radio button) of the given width at `y`; returns the next `y`.
    fn flow(
        &self,
        placements: &mut Vec<(HWND, RECT)>,
        control_id: u16,
        x: i32,
        y: i32,
        width: i32,
    ) -> i32 {
        if !self.is_visible(control_id) {
            return y;
        }
        let kind = self
            .controls
            .iter()
            .find(|control| control.id == control_id)
            .map_or(Kind::Label, |control| control.kind);
        let glyph = if matches!(kind, Kind::Check | Kind::Radio | Kind::FirstRadio) {
            self.px(CHECK_GLYPH)
        } else {
            0
        };
        let height = (self.text_height(control_id, width - glyph) + self.px(2)).max(if glyph > 0 {
            self.px(LINE_HEIGHT)
        } else {
            0
        });
        placements.push((self.get(control_id), rect(x, y, width, height)));
        y + height + self.px(ROW_GAP)
    }

    /// A group box around `children`; returns the next `y`.
    fn group(
        &self,
        placements: &mut Vec<(HWND, RECT)>,
        group_id: u16,
        children: &[u16],
        x: i32,
        y: i32,
        width: i32,
    ) -> i32 {
        if !self.is_visible(group_id) {
            return y;
        }
        // Section title, then the content indented under it
        let mut inner_y = self.flow(placements, group_id, x, y, width) + self.px(ROW_GAP);
        let inner_x = x + self.px(GROUP_PADDING);
        let inner_width = width - self.px(GROUP_PADDING);
        for child in children {
            inner_y = self.flow(placements, *child, inner_x, inner_y, inner_width);
        }
        inner_y - self.px(ROW_GAP) + self.px(SECTION_GAP * 2)
    }

    fn button(
        &self,
        placements: &mut Vec<(HWND, RECT)>,
        control_id: u16,
        x: i32,
        y: i32,
    ) -> (i32, i32) {
        let text_width = measure_text_width(&window_text(self.get(control_id)), &self.font);
        let width = (text_width + self.px(24)).max(self.px(110));
        placements.push((
            self.get(control_id),
            rect(x, y, width, self.px(BUTTON_HEIGHT)),
        ));
        (width, y + self.px(BUTTON_HEIGHT))
    }

    fn layout_general(
        &self,
        x: i32,
        y: i32,
        width: i32,
        placements: &mut Vec<(HWND, RECT)>,
    ) -> i32 {
        let column = (width - self.px(COLUMN_GAP)) / 2;
        // Left column: language, switches, terminals, how it works
        let combo_width = self.px(170);
        let row = self.px(COMBO_HEIGHT);
        placements.push((
            self.get(id::LANGUAGE_LABEL),
            rect(
                x,
                y + self.px(3),
                column - combo_width - self.px(8),
                row - self.px(3),
            ),
        ));
        // The height of a drop-down combo box includes its list
        placements.push((
            self.get(id::LANGUAGE_COMBO),
            rect(x + column - combo_width, y, combo_width, self.px(200)),
        ));
        let mut left = y + row + self.px(SECTION_GAP);
        for control_id in [id::LAUNCH_AT_LOGIN, id::PLAY_SOUND, id::SHOW_NOTIFICATIONS] {
            left = self.flow(placements, control_id, x, left, column);
        }
        left += self.px(SECTION_GAP);
        left = self.flow(placements, id::TERMINAL_SUPPORT, x, left, column);
        left = self.flow(
            placements,
            id::TERMINAL_HINT,
            x + self.px(CHECK_GLYPH),
            left,
            column - self.px(CHECK_GLYPH),
        );
        left += self.px(SECTION_GAP);
        left = self.group(placements, id::HOW_GROUP, &[id::HOW_TEXT], x, left, column);

        // Right column: the three modes
        let right_x = x + column + self.px(COLUMN_GAP);
        let mut right = y;
        right = self.group(
            placements,
            id::SMART_GROUP,
            &[
                id::SMART_MODE,
                id::SMART_LAST_WORD,
                id::SMART_GREEDY,
                id::SMART_DISABLED,
                id::SMART_DESC,
            ],
            right_x,
            right,
            column,
        );
        right = self.group(
            placements,
            id::SWITCH_GROUP,
            &[
                id::SWITCH_MODE,
                id::SWITCH_ALWAYS,
                id::SWITCH_IF_LAST,
                id::SWITCH_IF_ANY,
                id::SWITCH_DESC,
            ],
            right_x,
            right,
            column,
        );
        right = self.group(
            placements,
            id::CLIPBOARD_GROUP,
            &[
                id::CLIPBOARD_CTRL_INSERT,
                id::CLIPBOARD_CTRL_C,
                id::CLIPBOARD_DESC,
            ],
            right_x,
            right,
            column,
        );
        left.max(right) - self.px(SECTION_GAP)
    }

    fn layout_layouts(
        &self,
        x: i32,
        y: i32,
        width: i32,
        placements: &mut Vec<(HWND, RECT)>,
    ) -> i32 {
        let mut y = self.flow(placements, id::LAYOUTS_TITLE, x, y, width);
        y = self.flow(placements, id::LAYOUTS_DESC, x, y, width) + self.px(ROW_GAP);
        let list = self.get(id::LAYOUTS_LIST);
        placements.push((list, rect(x, y, width, self.px(LIST_HEIGHT))));
        // Name column takes the space the ID column does not need (inside the 1 px borders)
        let id_column = self.px(110);
        send(
            list,
            LVM_SETCOLUMNWIDTH,
            0,
            (width - id_column - 4) as isize,
        );
        send(list, LVM_SETCOLUMNWIDTH, 1, id_column as isize);
        y += self.px(LIST_HEIGHT) + self.px(SECTION_GAP);
        let (button_width, bottom) = self.button(placements, id::LAYOUTS_REFRESH, x, y);
        let count_x = x + button_width + self.px(12);
        placements.push((
            self.get(id::LAYOUTS_COUNT),
            rect(
                count_x,
                y + self.px(5),
                x + width - count_x,
                self.px(LINE_HEIGHT),
            ),
        ));
        bottom
    }

    fn layout_hotkey(&self, x: i32, y: i32, width: i32, placements: &mut Vec<(HWND, RECT)>) -> i32 {
        let mut y = self.flow(placements, id::HOTKEY_TITLE, x, y, width);
        y = self.flow(placements, id::HOTKEY_DESC, x, y, width) + self.px(SECTION_GAP);
        let label_width =
            measure_text_width(&window_text(self.get(id::CURRENT_LABEL)), &self.font) + self.px(12);
        placements.push((
            self.get(id::CURRENT_LABEL),
            rect(x, y, label_width, self.px(LINE_HEIGHT)),
        ));
        placements.push((
            self.get(id::CURRENT_VALUE),
            rect(
                x + label_width,
                y,
                width - label_width,
                self.px(LINE_HEIGHT),
            ),
        ));
        y += self.px(LINE_HEIGHT) + self.px(SECTION_GAP);
        y = self.flow(placements, id::DOUBLE_SHIFT, x, y, width);
        y = self.flow(
            placements,
            id::DOUBLE_SHIFT_HINT,
            x + self.px(CHECK_GLYPH),
            y,
            width - self.px(CHECK_GLYPH),
        );
        y = self.flow(
            placements,
            id::STICKY_WARNING,
            x + self.px(CHECK_GLYPH),
            y,
            width - self.px(CHECK_GLYPH),
        );
        y += self.px(SECTION_GAP);

        if !self.is_visible(id::CUSTOM_GROUP) {
            return y - self.px(SECTION_GAP);
        }
        // Custom shortcut: section title, prompt, display + Record button, hints
        let mut inner_y = self.flow(placements, id::CUSTOM_GROUP, x, y, width) + self.px(ROW_GAP);
        let inner_x = x + self.px(GROUP_PADDING);
        let inner_width = width - self.px(GROUP_PADDING);
        inner_y = self.flow(
            placements,
            id::RECORDER_PROMPT,
            inner_x,
            inner_y,
            inner_width,
        );
        let button_width = self.px(130);
        placements.push((
            self.get(id::RECORDER_DISPLAY),
            rect(
                inner_x,
                inner_y,
                inner_width - button_width - self.px(8),
                self.px(BUTTON_HEIGHT),
            ),
        ));
        placements.push((
            self.get(id::RECORD_BUTTON),
            rect(
                inner_x + inner_width - button_width,
                inner_y,
                button_width,
                self.px(BUTTON_HEIGHT),
            ),
        ));
        inner_y += self.px(BUTTON_HEIGHT) + self.px(ROW_GAP * 2);
        inner_y = self.flow(placements, id::MODIFIER_HINT, inner_x, inner_y, inner_width);
        inner_y = self.flow(
            placements,
            id::REGISTER_FAILED,
            inner_x,
            inner_y,
            inner_width,
        );
        inner_y - self.px(ROW_GAP)
    }

    fn layout_permissions(
        &self,
        x: i32,
        y: i32,
        width: i32,
        placements: &mut Vec<(HWND, RECT)>,
    ) -> i32 {
        let mut y = self.flow(placements, id::PERMISSIONS_TITLE, x, y, width);
        y = self.flow(placements, id::PERMISSIONS_DESC, x, y, width) + self.px(SECTION_GAP);
        y = self.flow(placements, id::ADMIN_TITLE, x, y, width);
        y = self.flow(placements, id::ADMIN_STATUS, x, y, width);
        if self.is_visible(id::RESTART_BUTTON) {
            y = self
                .button(placements, id::RESTART_BUTTON, x, y + self.px(ROW_GAP))
                .1;
        }
        y += self.px(SECTION_GAP * 2);
        y = self.flow(placements, id::GOOD_TO_KNOW, x, y, width);
        for step in [id::STEP1, id::STEP2, id::STEP3, id::STEP4] {
            y = self.flow(placements, step, x, y, width);
        }
        y += self.px(SECTION_GAP);
        self.button(placements, id::REFRESH_STATUS, x, y).1
    }

    /// Centers the window on the monitor under the mouse.
    fn center(&mut self) {
        let mut cursor = POINT::default();
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mut frame = RECT::default();
        // SAFETY: plain queries and a move of our own window
        unsafe {
            let _ = GetCursorPos(&mut cursor);
            let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return;
            }
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut frame);
            let width = frame.right - frame.left;
            let height = frame.bottom - frame.top;
            let work = info.rcWork;
            let left = work.left + ((work.right - work.left - width) / 2).max(0);
            let top = work.top + ((work.bottom - work.top - height) / 2).max(0);
            let _ = SetWindowPos(
                self.hwnd,
                None,
                left,
                top,
                0,
                0,
                SWP_NOZORDER | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
            );
        }
    }

    // MARK: - Events

    fn on_command(&mut self, control_id: u16, code: u32) {
        let checked = |control_id| is_checked(self.get(control_id));
        match (control_id, code) {
            (id::LANGUAGE_COMBO, CBN_SELCHANGE) => {
                let index = send(self.get(id::LANGUAGE_COMBO), CB_GETCURSEL, 0, 0);
                if let Some(language) = usize::try_from(index)
                    .ok()
                    .and_then(|index| l10n::AVAILABLE_LANGUAGES.get(index))
                {
                    change(|settings| settings.app_language = Some(language.code.to_owned()));
                }
            }
            (id::LAUNCH_AT_LOGIN, BN_CLICKED) => {
                let value = checked(id::LAUNCH_AT_LOGIN);
                change(|settings| settings.launch_at_login = value);
            }
            (id::PLAY_SOUND, BN_CLICKED) => {
                let value = checked(id::PLAY_SOUND);
                change(|settings| settings.play_sounds = value);
            }
            (id::SHOW_NOTIFICATIONS, BN_CLICKED) => {
                let value = checked(id::SHOW_NOTIFICATIONS);
                change(|settings| settings.show_notifications = value);
            }
            (id::TERMINAL_SUPPORT, BN_CLICKED) => {
                let value = checked(id::TERMINAL_SUPPORT);
                change(|settings| settings.terminal_support = value);
            }
            (id::DOUBLE_SHIFT, BN_CLICKED) => {
                let value = checked(id::DOUBLE_SHIFT);
                change(|settings| settings.use_double_shift = value);
            }
            (id::SMART_LAST_WORD | id::SMART_GREEDY | id::SMART_DISABLED, BN_CLICKED) => {
                let mode = match control_id {
                    id::SMART_LAST_WORD => SmartConversionMode::LastWord,
                    id::SMART_GREEDY => SmartConversionMode::GreedyLine,
                    _ => SmartConversionMode::Disabled,
                };
                change(|settings| settings.smart_conversion_mode = mode);
            }
            (id::SWITCH_ALWAYS | id::SWITCH_IF_LAST | id::SWITCH_IF_ANY, BN_CLICKED) => {
                let mode = match control_id {
                    id::SWITCH_ALWAYS => LayoutSwitchMode::Always,
                    id::SWITCH_IF_LAST => LayoutSwitchMode::IfLastWordConverted,
                    _ => LayoutSwitchMode::IfAnyWordConverted,
                };
                change(|settings| settings.layout_switch_mode = mode);
            }
            (id::CLIPBOARD_CTRL_INSERT | id::CLIPBOARD_CTRL_C, BN_CLICKED) => {
                let keys = if control_id == id::CLIPBOARD_CTRL_C {
                    ClipboardKeys::CtrlC
                } else {
                    ClipboardKeys::CtrlInsert
                };
                change(|settings| settings.clipboard_keys = keys);
            }
            (id::LAYOUTS_REFRESH, BN_CLICKED) => {
                let system_layouts = layouts::installed_layouts();
                change(move |settings| settings.enabled_layouts = system_layouts);
            }
            (id::RECORD_BUTTON, BN_CLICKED) => {
                if RECORDING.get() {
                    self.stop_recording(true);
                } else {
                    self.start_recording();
                }
            }
            (id::RESTART_BUTTON, BN_CLICKED) => app::restart_as_administrator(),
            (id::REFRESH_STATUS, BN_CLICKED) => {}
            (IDCANCEL, _) => {
                // Esc closes the window (IsDialogMessage turns it into IDCANCEL)
                // SAFETY: our own window; WM_CLOSE is handled outside of any borrow
                let _ = unsafe { PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
                return;
            }
            _ => return,
        }
        // Every action re-reads the state it changed
        // SAFETY: our own window
        let _ = unsafe { PostMessageW(Some(self.hwnd), WM_APP_REFRESH, WPARAM(0), LPARAM(0)) };
    }

    fn on_tab_changed(&mut self) {
        if RECORDING.get() {
            // When the recorder unloads with its tab
            self.stop_recording(true);
        }
        self.page = usize::try_from(send(self.tab, TCM_GETCURSEL, 0, 0)).unwrap_or(0);
        self.update_visibility();
    }

    // MARK: - Hotkey recorder

    fn start_recording(&mut self) {
        RECORDING.set(true);
        // The registered hotkey would swallow the very keystroke we want to record
        app::suspend_hotkey();
        self.state.show_modifier_hint = false;
        self.apply_texts();
        self.layout();
        // SAFETY: our own control
        let _ = unsafe { SetFocus(Some(self.get(id::RECORD_BUTTON))) };
    }

    fn stop_recording(&mut self, resume_hotkey: bool) {
        if !RECORDING.replace(false) {
            return;
        }
        self.state.show_modifier_hint = false;
        if resume_hotkey {
            app::resume_hotkey();
        }
        self.apply_texts();
        self.layout();
    }

    fn on_dpi_changed(&mut self, dpi: u32, suggested: RECT) {
        self.dpi = dpi.max(96);
        let (font, bold_font) = create_fonts(self.dpi);
        self.font = font;
        self.bold_font = bold_font;
        self.apply_fonts();
        // SAFETY: our own window
        let _ = unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                suggested.left,
                suggested.top,
                suggested.right - suggested.left,
                suggested.bottom - suggested.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        self.layout();
    }
}

/// A key press while the recorder runs (from the message loop, outside of any borrow).
fn on_record_key(virtual_key: u32) {
    // Wait for the main key of the combination
    if matches!(virtual_key, 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5 | VK_PROCESSKEY) {
        return;
    }
    let modifiers = [
        (0x11, HotkeyModifiers::CONTROL),
        (0x12, HotkeyModifiers::ALT),
        (0x10, HotkeyModifiers::SHIFT),
        (0x5B, HotkeyModifiers::WIN),
        (0x5C, HotkeyModifiers::WIN),
    ]
    .into_iter()
    // SAFETY: GetKeyState reads the key state of this thread's message stream
    .filter(|(key, _)| (unsafe { GetKeyState(*key) }) as u16 & 0x8000 != 0)
    .fold(HotkeyModifiers::NONE, |all, (_, flag)| all | flag);

    if virtual_key == VK_ESCAPE && modifiers == HotkeyModifiers::NONE {
        with_window(|window| window.stop_recording(true));
        return;
    }
    // Require at least Ctrl, Alt or Win
    if !hotkey_format::is_allowed(virtual_key, modifiers) {
        with_window(|window| {
            window.state.show_modifier_hint = true;
            window.layout();
        });
        return;
    }
    with_window(|window| window.stop_recording(false));
    let changed = app::change_settings(|settings| {
        settings.hotkey_key_code = virtual_key;
        settings.hotkey_modifiers = modifiers;
    });
    if !changed {
        // Same shortcut as before: nothing changed, so re-register here
        app::resume_hotkey();
    }
    refresh();
}

/// Applies a settings change through the app (saved and applied at once).
fn change(f: impl FnOnce(&mut crate::settings::Settings)) {
    app::change_settings(f);
}

// MARK: - Window procedure

unsafe extern "system" fn window_procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let result = std::panic::catch_unwind(|| handle_message(hwnd, message, wparam, lparam));
    match result {
        Ok(Some(result)) => result,
        Ok(None) => {
            // SAFETY: default processing of a message of our window
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        Err(_) => {
            debug!("ERROR: panic in the settings window");
            // SAFETY: as above
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

fn handle_message(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    match message {
        WM_COMMAND => {
            let control_id = (wparam.0 & 0xFFFF) as u16;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            with_window(|window| window.on_command(control_id, code));
            Some(LRESULT(0))
        }
        WM_NOTIFY => {
            // SAFETY: lParam of WM_NOTIFY points to an NMHDR
            let header = unsafe { &*(lparam.0 as *const NMHDR) };
            if header.code == TCN_SELCHANGE {
                with_window(SettingsWindow::on_tab_changed);
            }
            Some(LRESULT(0))
        }
        WM_CTLCOLORSTATIC => {
            let dc = HDC(wparam.0 as *mut c_void);
            // SAFETY: plain GDI calls on the DC of the control being painted
            unsafe {
                let control_id = GetDlgCtrlID(HWND(lparam.0 as *mut c_void)) as u16;
                SetBkColor(dc, COLORREF(GetSysColor(COLOR_WINDOW)));
                if WARNING_IDS.contains(&control_id) {
                    SetTextColor(dc, WARNING_COLOR);
                }
                Some(LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize))
            }
        }
        WM_ACTIVATE => {
            if wparam.0 & 0xFFFF != 0 {
                // Re-read the system state whenever the window is activated
                // SAFETY: our own window
                let _ = unsafe { PostMessageW(Some(hwnd), WM_APP_REFRESH, WPARAM(0), LPARAM(0)) };
            }
            None
        }
        WM_APP_REFRESH => {
            refresh();
            Some(LRESULT(0))
        }
        WM_DPICHANGED => {
            let dpi = ((wparam.0 >> 16) & 0xFFFF) as u32;
            // SAFETY: lParam of WM_DPICHANGED points to the suggested window rectangle
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            with_window(|window| window.on_dpi_changed(dpi, suggested));
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            // SAFETY: our own window
            let _ = unsafe { DestroyWindow(hwnd) };
            Some(LRESULT(0))
        }
        WM_DESTROY => {
            if RECORDING.replace(false) {
                app::resume_hotkey();
            }
            WINDOW_HANDLE.set(0);
            // Fonts, icons and the state go away with the window
            let window = WINDOW.with(|cell| {
                cell.try_borrow_mut()
                    .ok()
                    .and_then(|mut window| window.take())
            });
            drop(window);
            Some(LRESULT(0))
        }
        _ => None,
    }
}

// MARK: - Helpers

fn create_for_kind(parent: HWND, kind: Kind, control_id: u16) -> HWND {
    let group = WS_GROUP.0;
    let tab_stop = WS_TABSTOP.0;
    match kind {
        Kind::Label | Kind::Title => {
            create_control(parent, w!("STATIC"), SS_NOPREFIX | group, control_id)
        }
        Kind::Display => create_control(
            parent,
            w!("STATIC"),
            SS_NOPREFIX | SS_SUNKEN | SS_CENTERIMAGE | group,
            control_id,
        ),
        Kind::Check => create_control(
            parent,
            w!("BUTTON"),
            BS_AUTOCHECKBOX | BS_MULTILINE | group | tab_stop,
            control_id,
        ),
        Kind::FirstRadio => create_control(
            parent,
            w!("BUTTON"),
            BS_AUTORADIOBUTTON | BS_MULTILINE | group | tab_stop,
            control_id,
        ),
        Kind::Radio => create_control(
            parent,
            w!("BUTTON"),
            BS_AUTORADIOBUTTON | BS_MULTILINE,
            control_id,
        ),
        // A section title, not a group box: a group box does not paint its inside, and as a
        // window under the controls it left stale pixels behind when they moved
        Kind::Group => create_control(parent, w!("STATIC"), SS_NOPREFIX | group, control_id),
        Kind::Button => create_control(
            parent,
            w!("BUTTON"),
            BS_PUSHBUTTON | group | tab_stop,
            control_id,
        ),
        Kind::Combo => create_control(
            parent,
            w!("COMBOBOX"),
            CBS_DROPDOWNLIST | group | tab_stop,
            control_id,
        ),
        Kind::List => create_control(
            parent,
            WC_LISTVIEWW,
            LVS_REPORT | LVS_SINGLESEL | LVS_NOCOLUMNHEADER | WS_BORDER.0 | group | tab_stop,
            control_id,
        ),
    }
}

fn create_control(parent: HWND, class: PCWSTR, style: u32, control_id: u16) -> HWND {
    // SAFETY: a child control of our window; destroyed with it
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR::null(),
            WINDOW_STYLE(WS_CHILD.0 | style),
            0,
            0,
            0,
            0,
            Some(parent),
            Some(HMENU(usize::from(control_id) as *mut c_void)),
            Some(instance()),
            None,
        )
    }
    .unwrap_or_default()
}

fn create_fonts(dpi: u32) -> (OwnedFont, OwnedFont) {
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    // SAFETY: the output buffer is a NONCLIENTMETRICSW with its size set
    let _ = unsafe {
        SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            metrics.cbSize,
            Some((&raw mut metrics).cast::<c_void>()),
            0,
            dpi,
        )
    };
    let mut bold = metrics.lfMessageFont;
    bold.lfWeight = 600;
    // SAFETY: valid LOGFONTW structures
    unsafe {
        (
            OwnedFont(CreateFontIndirectW(&metrics.lfMessageFont)),
            OwnedFont(CreateFontIndirectW(&bold)),
        )
    }
}

fn load_icon(metric: windows::Win32::UI::Controls::_LI_METRIC) -> OwnedIcon {
    // SAFETY: the resource is part of our executable
    unsafe { LoadIconMetric(Some(instance()), resource_id(1), metric) }
        .map(OwnedIcon)
        .unwrap_or_default()
}

/// Height of `text` wrapped at `width` pixels.
fn measure_text(text: &str, font: &OwnedFont, width: i32) -> i32 {
    measure(
        text,
        font,
        width,
        DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX | DT_EDITCONTROL,
    )
    .1
}

/// Width of `text` on one line.
fn measure_text_width(text: &str, font: &OwnedFont) -> i32 {
    measure(text, font, 0, DT_CALCRECT | DT_NOPREFIX).0
}

fn measure(
    text: &str,
    font: &OwnedFont,
    width: i32,
    format: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT,
) -> (i32, i32) {
    if text.is_empty() {
        return (0, 0);
    }
    let mut units: Vec<u16> = text.encode_utf16().collect();
    let mut bounds = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: 0,
    };
    // SAFETY: a screen DC, released below; the font is selected and restored
    unsafe {
        let dc = GetDC(None);
        let previous = SelectObject(dc, HGDIOBJ(font.0.0));
        DrawTextW(dc, &mut units, &mut bounds, format);
        SelectObject(dc, previous);
        ReleaseDC(None, dc);
    }
    (bounds.right - bounds.left, bounds.bottom - bounds.top)
}

fn window_text(hwnd: HWND) -> String {
    let mut buffer = [0u16; 1024];
    // SAFETY: the buffer is writable and its length is passed along
    let length =
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

fn send(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: messages to our own controls with valid arguments
    unsafe { SendMessageW(hwnd, message, Some(WPARAM(wparam)), Some(LPARAM(lparam))) }.0
}

fn set_text(hwnd: HWND, text: &str) {
    if window_text(hwnd) != text {
        // SAFETY: our own control
        let _ = unsafe { SetWindowTextW(hwnd, &HSTRING::from(text)) };
    }
}

fn set_checked(hwnd: HWND, checked: bool) {
    send(hwnd, BM_SETCHECK, usize::from(checked), 0);
}

fn is_checked(hwnd: HWND) -> bool {
    send(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED
}

fn place(hwnd: HWND, bounds: RECT) {
    // SAFETY: our own control
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            None,
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

fn rect(x: i32, y: i32, width: i32, height: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_control_has_a_unique_id_and_existing_text() {
        let mut ids: Vec<u16> = CONTROLS
            .iter()
            .map(|(control_id, ..)| *control_id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CONTROLS.len());
        for key in CONTROLS.iter().filter_map(|(.., key)| *key).chain(TAB_KEYS) {
            assert_ne!(l10n::lookup("en", key), key, "missing string {key}");
        }
    }

    #[test]
    fn radio_groups_start_with_a_first_radio() {
        // BS_AUTORADIOBUTTON groups run from a WS_GROUP control to the next one
        let kinds: Vec<Kind> = CONTROLS.iter().map(|(_, _, kind, _)| *kind).collect();
        for (index, kind) in kinds.iter().enumerate() {
            if *kind == Kind::Radio {
                assert!(matches!(kinds[index - 1], Kind::Radio | Kind::FirstRadio));
            }
        }
    }

    /// Regression test for controls vanishing until hovered: the panel and the tab control were
    /// above the controls (and group boxes overlapped them), so every repaint covered them.
    #[test]
    fn controls_lie_above_the_panel_and_never_overlap() {
        use windows::Win32::UI::WindowsAndMessaging::{
            GW_CHILD, GW_HWNDNEXT, GetWindow, GetWindowRect,
        };

        let mut window = SettingsWindow::create().expect("settings window");
        let mut settings = crate::settings::Settings::defaults(vec![
            reshift_core::KeyboardLayout::new("00000409", "US", "en"),
            reshift_core::KeyboardLayout::new("00000419", "Russian", "ru"),
        ]);
        // Custom shortcut mode shows the most controls on the Hotkey tab
        settings.use_double_shift = false;
        window.state.settings = Some(settings);
        window.state.hotkey_registration_failed = true;
        window.state.show_modifier_hint = true;
        window.update_values();
        window.apply_texts();
        window.layout();

        // Children from the top of the z-order to the bottom
        let mut order = Vec::new();
        // SAFETY: plain window queries on our own windows
        let mut child = unsafe { GetWindow(window.hwnd, GW_CHILD) }.unwrap_or_default();
        while !child.is_invalid() {
            order.push(child);
            // SAFETY: as above
            child = unsafe { GetWindow(child, GW_HWNDNEXT) }.unwrap_or_default();
        }
        assert_eq!(order[order.len() - 2..], [window.panel, window.tab]);

        let bounds = |hwnd| {
            let mut bounds = RECT::default();
            // SAFETY: our own control
            let _ = unsafe { GetWindowRect(hwnd, &mut bounds) };
            bounds
        };
        for page in 0..TAB_KEYS.len() {
            window.page = page;
            window.update_visibility();
            let visible: Vec<(u16, RECT)> = window
                .controls
                .iter()
                .filter(|control| control.page == page && window.is_visible(control.id))
                .map(|control| (control.id, bounds(control.hwnd)))
                .collect();
            for (index, (first_id, first)) in visible.iter().enumerate() {
                for (second_id, second) in &visible[index + 1..] {
                    let overlap = first.left < second.right
                        && second.left < first.right
                        && first.top < second.bottom
                        && second.top < first.bottom;
                    assert!(
                        !overlap,
                        "page {page}: control {first_id} overlaps {second_id}"
                    );
                }
            }
        }
        // SAFETY: our own window
        let _ = unsafe { DestroyWindow(window.hwnd) };
    }
}
