//! MARK: - Keyboard Layout Detector
//!
//! Detects the keyboard layouts installed in Windows.

use std::collections::HashMap;

use reshift_core::KeyboardLayout;
use reshift_core::layout_char_map::character_map;
use windows::Win32::Globalization::{GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SISO639LANGNAME};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayoutList, HKL};
use windows::Win32::UI::Shell::SHLoadIndirectString;
use windows::core::HSTRING;

use crate::log::debug;
use crate::win::{RegKey, from_wide};

const LAYOUTS_REGISTRY_PATH: &str = r"SYSTEM\CurrentControlSet\Control\Keyboard Layouts";

/// All keyboard layouts enabled in the Windows language settings that have a character map,
/// in the user's order. Falls back to US + Russian.
pub fn installed_layouts() -> Vec<KeyboardLayout> {
    build_layouts(
        &layout_handles(),
        &read_layout_id_map(),
        &read_preload_order(),
        read_layout_name,
        language_code_for,
    )
}

/// Switches the keyboard layout of the focused window to the given KLID. Posts
/// WM_INPUTLANGCHANGEREQUEST to the focused control, verifies after ~100 ms and retries on the
/// active top-level window if the first target ignored it. Runs on the conversion thread.
pub fn switch_to_layout(target_id: &str) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows::Win32::UI::WindowsAndMessaging::{
        GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, PostMessageW,
        WM_INPUTLANGCHANGEREQUEST,
    };

    let Some(layout) = find_layout_handle(target_id) else {
        debug!("switchToLayout: layout '{target_id}' not found in system layouts");
        return;
    };
    // SAFETY: plain window queries
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_invalid() {
        return;
    }
    let thread_id = input_thread_id(foreground);
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` has its size set
    let _ = unsafe { GetGUIThreadInfo(thread_id, &mut info) };

    let target = if info.hwndFocus.is_invalid() {
        foreground
    } else {
        info.hwndFocus
    };
    let request = |window| {
        // SAFETY: posting to another window is allowed; lParam is the HKL
        let _ = unsafe {
            PostMessageW(
                Some(window),
                WM_INPUTLANGCHANGEREQUEST,
                WPARAM(0),
                LPARAM(layout.0 as isize),
            )
        };
    };
    request(target);
    debug!("switchToLayout: requested '{target_id}' for thread {thread_id}");

    crate::wait::pump_sleep(std::time::Duration::from_millis(100));
    // SAFETY: no preconditions
    if unsafe { GetKeyboardLayout(thread_id) } == layout {
        return;
    }
    let fallback = if info.hwndActive.is_invalid() {
        foreground
    } else {
        info.hwndActive
    };
    if fallback != target {
        request(fallback);
        debug!("switchToLayout: retried on the active window");
    }
}

/// The thread whose keyboard layout applies to a window. Classic console windows are reported
/// as belonging to the console client (e.g. powershell.exe), but they are run by a conhost
/// thread - that thread translates the keys, so its layout is the one that counts.
pub fn input_thread_id(window: windows::Win32::Foundation::HWND) -> u32 {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GUITHREADINFO, GetGUIThreadInfo, GetWindowThreadProcessId,
    };

    // SAFETY: plain window query
    let thread_id = unsafe { GetWindowThreadProcessId(window, None) };
    if crate::window_kind::class_name(window) != "ConsoleWindowClass" {
        return thread_id;
    }
    // SAFETY: the snapshot handle is owned below
    let Ok(snapshot) =
        (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS | TH32CS_SNAPTHREAD, 0) })
    else {
        return thread_id;
    };
    let snapshot = crate::win::OwnedHandle(snapshot);

    let mut conhosts = Vec::new();
    let mut process = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: `process` has its size set; iteration over a valid snapshot
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut process) }.is_ok();
    while more {
        if from_wide(&process.szExeFile).eq_ignore_ascii_case("conhost.exe") {
            conhosts.push(process.th32ProcessID);
        }
        // SAFETY: as above
        more = unsafe { Process32NextW(snapshot.0, &mut process) }.is_ok();
    }

    let mut thread = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: `thread` has its size set; iteration over a valid snapshot
    let mut more = unsafe { Thread32First(snapshot.0, &mut thread) }.is_ok();
    while more {
        if conhosts.contains(&thread.th32OwnerProcessID) {
            let mut info = GUITHREADINFO {
                cbSize: size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            // SAFETY: `info` has its size set
            if unsafe { GetGUIThreadInfo(thread.th32ThreadID, &mut info) }.is_ok()
                && (info.hwndActive == window || info.hwndFocus == window)
            {
                return thread.th32ThreadID;
            }
        }
        // SAFETY: as above
        more = unsafe { Thread32Next(snapshot.0, &mut thread) }.is_ok();
    }
    thread_id
}

fn find_layout_handle(layout_id: &str) -> Option<HKL> {
    let layout_id_map = read_layout_id_map();
    layout_handles()
        .into_iter()
        .find(|hkl| {
            klid_from_layout_handle(*hkl, &layout_id_map)
                .is_some_and(|klid| klid.eq_ignore_ascii_case(layout_id))
        })
        .map(|hkl| HKL(hkl as isize as *mut _))
}

/// Default fallback layouts.
pub fn default_layouts() -> Vec<KeyboardLayout> {
    vec![
        KeyboardLayout::new("00000409", "US", "en"),
        KeyboardLayout::new("00000419", "Russian", "ru"),
    ]
}

// MARK: - Pure helpers (unit-tested)

/// Builds the layout list: resolves KLIDs, keeps only layouts with a character map, removes
/// duplicates and orders them like the user's language list (Preload).
pub fn build_layouts(
    layout_handles: &[i64],
    layout_id_to_klid: &HashMap<String, String>,
    preload_klids: &[String],
    name_for_klid: impl Fn(&str) -> String,
    language_code_for_lang_id: impl Fn(u32) -> String,
) -> Vec<KeyboardLayout> {
    let mut layouts: Vec<KeyboardLayout> = Vec::new();
    for &hkl in layout_handles {
        let Some(klid) = klid_from_layout_handle(hkl, layout_id_to_klid) else {
            continue;
        };
        // Only include layouts we have a character map for, once
        if layouts.iter().any(|layout| layout.id == klid) || character_map(&klid).is_none() {
            continue;
        }
        let language_code = language_code_for_lang_id((hkl & 0xFFFF) as u32);
        layouts.push(KeyboardLayout::new(
            &klid,
            &name_for_klid(&klid),
            &language_code,
        ));
    }

    // Order like the user's language list; layouts missing from Preload keep their order at the end
    let rank = |layout: &KeyboardLayout| {
        preload_klids
            .iter()
            .position(|klid| klid.eq_ignore_ascii_case(&layout.id))
            .unwrap_or(usize::MAX)
    };
    layouts.sort_by_key(rank);

    if layouts.is_empty() {
        default_layouts()
    } else {
        layouts
    }
}

/// HKL => KLID. The high word is the layout ("device") part: 0xFxxx means a layout variant
/// identified by its "Layout Id" registry value, 0xExxx is an IME (not a keyboard layout).
pub fn klid_from_layout_handle(
    hkl: i64,
    layout_id_to_klid: &HashMap<String, String>,
) -> Option<String> {
    let device = ((hkl >> 16) & 0xFFFF) as u32;
    match device & 0xF000 {
        0xE000 => None,
        0xF000 => layout_id_to_klid
            .get(&format!("{:04X}", device & 0x0FFF))
            .cloned(),
        _ => Some(format!("{device:08X}")),
    }
}

/// Resolves Preload entries ("d0010409" style substitutes) to real KLIDs.
pub fn resolve_preload(preload: &[String], substitutes: &HashMap<String, String>) -> Vec<String> {
    preload
        .iter()
        .map(|id| {
            substitutes
                .get(&id.to_ascii_uppercase())
                .unwrap_or(id)
                .to_ascii_uppercase()
        })
        .collect()
}

// MARK: - System access

/// The HKLs of the installed layouts (`GetKeyboardLayoutList`).
pub fn layout_handles() -> Vec<i64> {
    // SAFETY: a count query without a buffer
    let count = unsafe { GetKeyboardLayoutList(None) };
    if count <= 0 {
        return Vec::new();
    }
    let mut handles = vec![HKL::default(); count as usize];
    // SAFETY: the buffer has room for `count` handles
    let count = unsafe { GetKeyboardLayoutList(Some(&mut handles)) };
    handles
        .iter()
        .take(count.max(0) as usize)
        .map(|hkl| hkl.0 as isize as i64)
        .collect()
}

/// "Layout Id" registry values => KLIDs of the variant layouts (keys upper case).
pub fn read_layout_id_map() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(layouts) = RegKey::open(HKEY_LOCAL_MACHINE, LAYOUTS_REGISTRY_PATH) else {
        return map;
    };
    for klid in layouts.subkey_names() {
        let layout_id = RegKey::open(
            HKEY_LOCAL_MACHINE,
            &format!(r"{LAYOUTS_REGISTRY_PATH}\{klid}"),
        )
        .and_then(|key| key.string("Layout Id"))
        .filter(|layout_id| !layout_id.is_empty());
        if let Some(layout_id) = layout_id {
            map.entry(layout_id.to_ascii_uppercase())
                .or_insert_with(|| klid.to_ascii_uppercase());
        }
    }
    map
}

fn read_preload_order() -> Vec<String> {
    let Some(preload_key) = RegKey::open(HKEY_CURRENT_USER, r"Keyboard Layout\Preload") else {
        return Vec::new();
    };
    let mut preload: Vec<(u32, String)> = preload_key
        .value_names()
        .into_iter()
        .filter_map(|name| {
            let value = preload_key
                .string(&name)
                .filter(|value| !value.is_empty())?;
            Some((name.parse().unwrap_or(u32::MAX), value))
        })
        .collect();
    preload.sort_by_key(|(order, _)| *order);
    let preload: Vec<String> = preload.into_iter().map(|(_, value)| value).collect();

    let substitutes: HashMap<String, String> =
        RegKey::open(HKEY_CURRENT_USER, r"Keyboard Layout\Substitutes")
            .map(|key| {
                key.value_names()
                    .into_iter()
                    .filter_map(|name| {
                        key.string(&name)
                            .map(|real| (name.to_ascii_uppercase(), real))
                    })
                    .collect()
            })
            .unwrap_or_default();
    resolve_preload(&preload, &substitutes)
}

/// The localized layout name ("Layout Display Name" resource string, or "Layout Text").
fn read_layout_name(klid: &str) -> String {
    let Some(key) = RegKey::open(
        HKEY_LOCAL_MACHINE,
        &format!(r"{LAYOUTS_REGISTRY_PATH}\{klid}"),
    ) else {
        return klid.to_owned();
    };
    if let Some(indirect) = key
        .string("Layout Display Name")
        .filter(|name| name.starts_with('@'))
    {
        let mut buffer = [0u16; 256];
        // SAFETY: the buffer is writable; SHLoadIndirectString NUL-terminates it
        if unsafe { SHLoadIndirectString(&HSTRING::from(indirect), &mut buffer, None) }.is_ok() {
            let name = from_wide(&buffer);
            if !name.is_empty() {
                return name;
            }
        }
    }
    key.string("Layout Text").unwrap_or_else(|| klid.to_owned())
}

/// Two-letter ISO language code of a language ID (e.g. 0x0419 => "ru").
pub fn language_code_for(language_id: u32) -> String {
    let mut locale = [0u16; 85];
    // SAFETY: the buffer is writable and its length is passed along
    let length = unsafe { LCIDToLocaleName(language_id, Some(&mut locale), 0) };
    if length <= 0 {
        return "unknown".to_owned();
    }
    let mut code = [0u16; 16];
    // SAFETY: `locale` is NUL-terminated, the output buffer is writable
    let length = unsafe {
        GetLocaleInfoEx(
            windows::core::PCWSTR(locale.as_ptr()),
            LOCALE_SISO639LANGNAME,
            Some(&mut code),
        )
    };
    if length <= 0 {
        return "unknown".to_owned();
    }
    from_wide(&code)
}

#[cfg(test)]
mod tests {
    //! Test suite.

    use super::*;

    const US: &str = "00000409";
    const RUSSIAN: &str = "00000419";
    const GERMAN: &str = "00000407";

    /// "Layout Id" registry values => KLID (variant layouts)
    fn layout_ids() -> HashMap<String, String> {
        [
            ("0002", "00010409"), // US-Dvorak
            ("0001", "00020409"), // United States-International
            ("00C0", "00020422"), // Ukrainian (Enhanced)
        ]
        .into_iter()
        .map(|(id, klid)| (id.to_owned(), klid.to_owned()))
        .collect()
    }

    fn ids(layouts: &[KeyboardLayout]) -> Vec<&str> {
        layouts.iter().map(|layout| layout.id.as_str()).collect()
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn klid_from_layout_handle_resolves_layout_part() {
        let cases: [(i64, &str); 5] = [
            (0x0409_0409, "00000409"), // en-US + US
            (0x0419_0419, "00000419"), // ru-RU + Russian
            (0x0419_0409, "00000419"), // English language with the Russian keyboard
            (0xF001_0409, "00020409"), // variant: Layout Id 0001
            (0xF0C0_0422, "00020422"), // variant: Layout Id 00C0
        ];
        for (hkl, expected) in cases {
            assert_eq!(
                klid_from_layout_handle(hkl, &layout_ids()).as_deref(),
                Some(expected),
                "{hkl:#x}"
            );
        }
        // 64-bit HKLs are sign-extended
        assert_eq!(
            klid_from_layout_handle(0xF001_0409_u32 as i32 as i64, &layout_ids()).as_deref(),
            Some("00020409")
        );
    }

    #[test]
    fn klid_from_layout_handle_rejects_imes_and_unknown_variants() {
        assert_eq!(klid_from_layout_handle(0xE001_0411, &layout_ids()), None); // Japanese IME
        assert_eq!(klid_from_layout_handle(0xF0FF_0409, &layout_ids()), None); // unknown variant
    }

    #[test]
    fn build_layouts_keeps_supported_layouts_in_preload_order() {
        let layouts = build_layouts(
            &[0x0419_0419, 0x0411_0411, 0x0409_0409, 0x0409_0809],
            &layout_ids(),
            &strings(&[US, RUSSIAN]),
            |klid| format!("name-{klid}"),
            |lang_id| if lang_id == 0x0419 { "ru" } else { "en" }.to_owned(),
        );

        // Japanese (no character map) is skipped, the second US entry (en-GB language) is a duplicate
        assert_eq!(ids(&layouts), [US, RUSSIAN]);
        assert_eq!(layouts[1].localized_name, "name-00000419");
        assert_eq!(layouts[1].language_code, "ru");
    }

    #[test]
    fn build_layouts_layouts_missing_from_preload_go_last() {
        let layouts = build_layouts(
            &[0x0407_0407, 0x0419_0419, 0x0409_0409],
            &layout_ids(),
            &strings(&[RUSSIAN]),
            str::to_owned,
            |_| "xx".to_owned(),
        );
        assert_eq!(ids(&layouts), [RUSSIAN, GERMAN, US]);
    }

    #[test]
    fn build_layouts_falls_back_to_us_and_russian() {
        let layouts = build_layouts(&[0x0411_0411], &layout_ids(), &[], str::to_owned, |_| {
            "ja".to_owned()
        });
        assert_eq!(ids(&layouts), [US, RUSSIAN]);
    }

    #[test]
    fn resolve_preload_applies_substitutes() {
        let substitutes = HashMap::from([("D0010419".to_owned(), "00000422".to_owned())]);
        assert_eq!(
            resolve_preload(&strings(&["00000409", "d0010419"]), &substitutes),
            ["00000409", "00000422"]
        );
    }

    // MARK: - System (read-only)

    #[test]
    fn installed_layouts_are_detected() {
        let layouts = installed_layouts();
        assert!(!layouts.is_empty());
        assert!(
            layouts
                .iter()
                .all(|layout| character_map(&layout.id).is_some())
        );
        let unique: std::collections::HashSet<&str> = ids(&layouts).into_iter().collect();
        assert_eq!(unique.len(), layouts.len());
    }

    #[test]
    fn language_codes_are_two_letters() {
        assert_eq!(language_code_for(0x0409), "en");
        assert_eq!(language_code_for(0x0419), "ru");
    }
}
