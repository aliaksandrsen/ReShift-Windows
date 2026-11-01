//! MARK: - Settings
//!
//! User preferences persisted as JSON. Keys of removed
//! features (statistics, conversion log) are ignored on load and not written.

use std::fs;
use std::io;
use std::path::Path;

use reshift_core::l10n;
use reshift_core::{ClipboardKeys, KeyboardLayout, LayoutSwitchMode, SmartConversionMode};
use serde::{Deserialize, Serialize};

use crate::hotkey_format::{
    self, DEFAULT_HOTKEY_KEY_CODE, DEFAULT_HOTKEY_MODIFIERS, HotkeyModifiers,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub enabled_layouts: Vec<KeyboardLayout>,
    /// true = double-shift mode, false = regular modifier+key hotkey.
    pub use_double_shift: bool,
    /// Virtual-key code of the custom hotkey.
    pub hotkey_key_code: u32,
    pub hotkey_modifiers: HotkeyModifiers,
    pub launch_at_login: bool,
    pub show_notifications: bool,
    pub play_sounds: bool,
    pub smart_conversion_mode: SmartConversionMode,
    pub layout_switch_mode: LayoutSwitchMode,
    /// Shortcuts used to copy the selection and paste the result (Windows only).
    pub clipboard_keys: ClipboardKeys,
    /// Convert in terminals by remembering the typed line in memory (Windows only).
    pub terminal_support: bool,
    /// UI language code; `None` until the user or the first launch chooses one.
    pub app_language: Option<String>,
}

impl Settings {
    /// Defaults of the macOS app (plus the Windows-only settings).
    pub fn defaults(system_layouts: Vec<KeyboardLayout>) -> Self {
        Self {
            enabled_layouts: system_layouts,
            use_double_shift: true,
            hotkey_key_code: DEFAULT_HOTKEY_KEY_CODE,
            hotkey_modifiers: DEFAULT_HOTKEY_MODIFIERS,
            launch_at_login: false,
            show_notifications: true,
            play_sounds: true,
            smart_conversion_mode: SmartConversionMode::GreedyLine,
            layout_switch_mode: LayoutSwitchMode::Always,
            clipboard_keys: ClipboardKeys::CtrlInsert,
            terminal_support: true,
            app_language: None,
        }
    }

    /// "Shift Shift (Double Shift)" or e.g. "Ctrl+Alt+Space", in the current UI language.
    pub fn hotkey_description(&self) -> String {
        if self.use_double_shift {
            l10n::t("hotkey.doubleShiftDescription").to_owned()
        } else {
            hotkey_format::describe(self.hotkey_key_code, self.hotkey_modifiers)
        }
    }

    /// Whether the hotkey has to be re-registered after a change (macOS: .hotkeySettingsChanged):
    /// a mode change always does; the custom key only matters while it is in use.
    pub fn hotkey_changed(&self, previous: &Self) -> bool {
        self.use_double_shift != previous.use_double_shift
            || (!self.use_double_shift
                && (self.hotkey_key_code != previous.hotkey_key_code
                    || self.hotkey_modifiers != previous.hotkey_modifiers))
    }

    /// The low-level hook is needed for double Shift and for the terminal line tracker.
    pub fn needs_keyboard_hook(&self) -> bool {
        self.use_double_shift || self.terminal_support
    }
}

// MARK: - Persistence

/// Loads the settings; a missing or unreadable file means defaults. Missing values fall back
/// to defaults one by one, like `defaults.object(forKey:) ?? default`.
pub fn load(path: &Path, system_layouts: impl FnOnce() -> Vec<KeyboardLayout>) -> Settings {
    let data = match fs::read_to_string(path) {
        // Editors and PowerShell save a UTF-8 BOM; strip it if present
        Ok(json) => serde_json::from_str::<SettingsData>(json.trim_start_matches('\u{FEFF}'))
            .unwrap_or_else(|error| {
                crate::log::debug!(
                    "settings: could not parse {}: {error} - using defaults",
                    path.display()
                );
                SettingsData::default()
            }),
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                crate::log::debug!(
                    "settings: could not read {}: {error} - using defaults",
                    path.display()
                );
            }
            SettingsData::default()
        }
    };

    let saved_layouts: Vec<KeyboardLayout> = data
        .enabled_layouts
        .unwrap_or_default()
        .into_iter()
        .map(|layout| {
            KeyboardLayout::new(&layout.id, &layout.localized_name, &layout.language_code)
        })
        .collect();
    let mut settings = Settings::defaults(Vec::new());
    settings.enabled_layouts = if saved_layouts.is_empty() {
        system_layouts()
    } else {
        saved_layouts
    };
    let defaults = settings.clone();
    settings.use_double_shift = data.use_double_shift.unwrap_or(defaults.use_double_shift);
    settings.hotkey_key_code = data.hotkey_key_code.unwrap_or(defaults.hotkey_key_code);
    settings.hotkey_modifiers = data
        .hotkey_modifiers
        .map_or(defaults.hotkey_modifiers, HotkeyModifiers);
    settings.launch_at_login = data.launch_at_login.unwrap_or(defaults.launch_at_login);
    settings.show_notifications = data
        .show_notifications
        .unwrap_or(defaults.show_notifications);
    settings.play_sounds = data.play_sounds.unwrap_or(defaults.play_sounds);
    settings.smart_conversion_mode = data
        .smart_conversion_mode
        .and_then(SmartConversionMode::from_i64)
        .unwrap_or(defaults.smart_conversion_mode);
    settings.layout_switch_mode = data
        .layout_switch_mode
        .and_then(LayoutSwitchMode::from_i64)
        .unwrap_or(defaults.layout_switch_mode);
    settings.clipboard_keys = data
        .clipboard_keys
        .and_then(ClipboardKeys::from_i64)
        .unwrap_or(defaults.clipboard_keys);
    settings.terminal_support = data.terminal_support.unwrap_or(defaults.terminal_support);
    settings.app_language = data.app_language;
    settings
}

/// Saves the settings (write-then-rename, so a crash never leaves a half-written file).
pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    let data = SettingsData {
        enabled_layouts: Some(
            settings
                .enabled_layouts
                .iter()
                .map(|layout| LayoutData {
                    id: layout.id.clone(),
                    localized_name: layout.localized_name.clone(),
                    language_code: layout.language_code.clone(),
                })
                .collect(),
        ),
        use_double_shift: Some(settings.use_double_shift),
        hotkey_key_code: Some(settings.hotkey_key_code),
        hotkey_modifiers: Some(settings.hotkey_modifiers.0),
        launch_at_login: Some(settings.launch_at_login),
        show_notifications: Some(settings.show_notifications),
        play_sounds: Some(settings.play_sounds),
        smart_conversion_mode: Some(settings.smart_conversion_mode as i64),
        layout_switch_mode: Some(settings.layout_switch_mode as i64),
        clipboard_keys: Some(settings.clipboard_keys as i64),
        terminal_support: Some(settings.terminal_support),
        app_language: settings.app_language.clone(),
    };
    let json = serde_json::to_string_pretty(&data).map_err(io::Error::other)?;
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json)?;
    fs::rename(&temp, path)
}

/// On-disk shape (camelCase). Every value is optional.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsData {
    enabled_layouts: Option<Vec<LayoutData>>,
    use_double_shift: Option<bool>,
    hotkey_key_code: Option<u32>,
    hotkey_modifiers: Option<u32>,
    launch_at_login: Option<bool>,
    show_notifications: Option<bool>,
    play_sounds: Option<bool>,
    smart_conversion_mode: Option<i64>,
    layout_switch_mode: Option<i64>,
    clipboard_keys: Option<i64>,
    terminal_support: Option<bool>,
    app_language: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutData {
    id: String,
    localized_name: String,
    language_code: String,
}

#[cfg(test)]
pub(crate) mod tests {
    //! Test suite.

    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const US: &str = "00000409";
    const RUSSIAN: &str = "00000419";
    const GERMAN: &str = "00000407";

    /// A unique, initially empty folder under %TEMP%, removed on drop.
    pub(crate) struct TempDir(pub PathBuf);

    impl TempDir {
        pub(crate) fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let name = format!(
                "ReShiftTests-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(name);
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn system_layouts() -> Vec<KeyboardLayout> {
        vec![
            KeyboardLayout::new(US, "US", "en"),
            KeyboardLayout::new(RUSSIAN, "Russian", "ru"),
        ]
    }

    fn ids(settings: &Settings) -> Vec<&str> {
        settings
            .enabled_layouts
            .iter()
            .map(|layout| layout.id.as_str())
            .collect()
    }

    fn write_file(dir: &TempDir, json: &str) -> PathBuf {
        fs::create_dir_all(&dir.0).unwrap();
        let path = dir.0.join("settings.json");
        fs::write(&path, json).unwrap();
        path
    }

    #[test]
    fn defaults_match_macos_app() {
        let dir = TempDir::new();
        let settings = load(&dir.0.join("settings.json"), system_layouts);

        assert!(settings.use_double_shift);
        assert!(settings.show_notifications);
        assert!(settings.play_sounds);
        assert!(!settings.launch_at_login);
        assert_eq!(
            settings.smart_conversion_mode,
            SmartConversionMode::GreedyLine
        );
        assert_eq!(settings.layout_switch_mode, LayoutSwitchMode::Always);
        // Windows-specific defaults
        assert_eq!(settings.clipboard_keys, ClipboardKeys::CtrlInsert);
        assert!(settings.terminal_support);
        assert_eq!(settings.hotkey_key_code, 0x20); // Space
        assert_eq!(
            settings.hotkey_modifiers,
            HotkeyModifiers::CONTROL | HotkeyModifiers::ALT
        );
        assert_eq!(settings.app_language, None);
        assert_eq!(ids(&settings), [US, RUSSIAN]);
    }

    #[test]
    fn changes_are_persisted() {
        let dir = TempDir::new();
        let path = dir.0.join("settings.json");
        let mut settings = load(&path, system_layouts);
        settings.use_double_shift = false;
        settings.hotkey_key_code = u32::from(b'K');
        settings.hotkey_modifiers = HotkeyModifiers::CONTROL | HotkeyModifiers::SHIFT;
        settings.smart_conversion_mode = SmartConversionMode::LastWord;
        settings.clipboard_keys = ClipboardKeys::CtrlC;
        settings.terminal_support = false;
        settings.app_language = Some("ru".to_owned());
        settings.enabled_layouts = vec![
            KeyboardLayout::new(GERMAN, "German", "de"),
            KeyboardLayout::new(RUSSIAN, "Russian", "ru"),
        ];
        save(&path, &settings).unwrap();

        let reloaded = load(&path, system_layouts);

        assert_eq!(reloaded, settings);
        assert_eq!(ids(&reloaded), [GERMAN, RUSSIAN]);
        assert_eq!(reloaded.enabled_layouts[0].localized_name, "German");
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = TempDir::new();
        let settings = load(&write_file(&dir, "{ not json"), system_layouts);

        assert!(settings.use_double_shift);
        assert_eq!(
            settings.smart_conversion_mode,
            SmartConversionMode::GreedyLine
        );
    }

    #[test]
    fn file_with_utf8_bom_is_read() {
        let dir = TempDir::new();
        let path = write_file(&dir, "\u{FEFF}{ \"useDoubleShift\": false }");
        assert!(!load(&path, system_layouts).use_double_shift);
    }

    #[test]
    fn out_of_range_enums_fall_back_to_defaults() {
        let dir = TempDir::new();
        let path = write_file(
            &dir,
            r#"{ "smartConversionMode": 42, "layoutSwitchMode": -1, "enabledLayouts": [] }"#,
        );
        let settings = load(&path, system_layouts);

        assert_eq!(
            settings.smart_conversion_mode,
            SmartConversionMode::GreedyLine
        );
        assert_eq!(settings.layout_switch_mode, LayoutSwitchMode::Always);
        assert_eq!(ids(&settings), [US, RUSSIAN]); // empty list => system layouts
    }

    #[test]
    fn hotkey_changes_follow_macos_rules() {
        let initial = Settings::defaults(system_layouts());

        // Double Shift mode: a custom key change does not matter
        let mut changed = initial.clone();
        changed.hotkey_key_code = u32::from(b'K');
        assert!(!changed.hotkey_changed(&initial));

        // A mode change always re-registers
        let mut custom = changed.clone();
        custom.use_double_shift = false;
        assert!(custom.hotkey_changed(&changed));

        let mut modifiers = custom.clone();
        modifiers.hotkey_modifiers = HotkeyModifiers::WIN;
        assert!(modifiers.hotkey_changed(&custom));
    }

    #[test]
    fn settings_json_with_extra_keys_is_read() {
        // Includes unknown/extra keys: must be parsed safely
        let dir = TempDir::new();
        let path = write_file(
            &dir,
            r#"{
  "enabledLayouts": [
    { "id": "00000419", "localizedName": "Русская", "languageCode": "ru" },
    { "id": "00000409", "localizedName": "US", "languageCode": "en" }
  ],
  "useDoubleShift": false,
  "hotkeyKeyCode": 122,
  "hotkeyModifiers": 6,
  "launchAtLogin": true,
  "showNotifications": false,
  "playSounds": false,
  "smartConversionMode": 0,
  "layoutSwitchMode": 2,
  "conversionCount": 1234,
  "loggingEnabled": true,
  "logMaxEntries": 100,
  "clipboardKeys": 1,
  "terminalSupport": false,
  "appLanguage": "ru"
}"#,
        );
        let settings = load(&path, || panic!("saved layouts must be used"));

        assert_eq!(ids(&settings), [RUSSIAN, US]);
        assert_eq!(settings.enabled_layouts[0].localized_name, "Русская");
        assert!(!settings.use_double_shift);
        assert_eq!(
            hotkey_format::describe(settings.hotkey_key_code, settings.hotkey_modifiers),
            "Ctrl+Shift+F11"
        );
        assert!(settings.launch_at_login && !settings.show_notifications && !settings.play_sounds);
        assert_eq!(
            settings.smart_conversion_mode,
            SmartConversionMode::LastWord
        );
        assert_eq!(
            settings.layout_switch_mode,
            LayoutSwitchMode::IfAnyWordConverted
        );
        assert_eq!(settings.clipboard_keys, ClipboardKeys::CtrlC);
        assert!(!settings.terminal_support);
        assert_eq!(settings.app_language.as_deref(), Some("ru"));
    }

    #[test]
    fn saved_file_uses_the_csharp_keys() {
        let dir = TempDir::new();
        let path = dir.0.join("settings.json");
        save(&path, &Settings::defaults(system_layouts())).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(json["useDoubleShift"], true);
        assert_eq!(json["hotkeyKeyCode"], 32);
        assert_eq!(json["hotkeyModifiers"], 3);
        assert_eq!(json["smartConversionMode"], 1);
        assert_eq!(json["enabledLayouts"][1]["localizedName"], "Russian");
        assert!(json.get("conversionCount").is_none());
        assert!(!dir.0.join("settings.json.tmp").exists());
    }
}
