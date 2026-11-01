// MARK: - English Strings
// To add a new language, copy this file as xx.rs, translate every value and register it in
// mod.rs (AVAILABLE_LANGUAGES and TABLES).

pub(super) static STRINGS: &[(&str, &str)] = &[
    // -- Tray menu --------------------------------------------------------
    ("menu.settings", "Settings..."),
    ("menu.about", "About ReShift"),
    ("menu.quit", "Quit ReShift"),
    ("menu.tooltip", "ReShift - %@ to convert"),
    (
        "menu.tooltipInactive",
        "ReShift - not available in this window (it runs as administrator)",
    ),
    // -- Settings Window --------------------------------------------------
    ("settings.windowTitle", "ReShift Settings"),
    ("settings.tab.general", "General"),
    ("settings.tab.layouts", "Layouts"),
    ("settings.tab.hotkey", "Hotkey"),
    ("settings.tab.permissions", "Permissions"),
    // -- General Tab ------------------------------------------------------
    ("general.language", "Language"),
    ("general.launchAtLogin", "Start with Windows"),
    ("general.playSound", "Play Sound on Conversion"),
    ("general.showNotifications", "Show Notifications"),
    ("general.smartConversion", "Smart Conversion (No Selection)"),
    ("general.mode", "Mode:"),
    ("general.layoutSwitch", "Layout Switch after Conversion"),
    ("general.clipboardKeys", "Copy/Paste Keys"),
    ("general.terminalSupport", "Convert in Terminals"),
    (
        "general.terminalSupportHint",
        "A terminal has no input line to select, so while a terminal is active ReShift remembers the line you type in memory (never on disk) and retypes it when you press the shortcut. Works in Windows Terminal, the console and terminals inside VS Code-like editors.",
    ),
    ("general.howItWorks", "How It Works"),
    (
        "general.howItWorksText",
        "When text is selected, the shortcut converts the selection.\nWhen no text is selected, the behavior depends on the Smart Conversion mode above.",
    ),
    // -- Smart Conversion Modes -------------------------------------------
    ("smartMode.lastWord.name", "Last Word Only"),
    ("smartMode.greedyLine.name", "Greedy (Entire Phrase)"),
    ("smartMode.disabled.name", "Disabled"),
    (
        "smartMode.lastWord.desc",
        "Converts only the last typed word before the cursor.",
    ),
    (
        "smartMode.greedyLine.desc",
        "Selects text to the start of line, finds where the wrong layout begins, and converts the entire wrong-layout phrase.",
    ),
    (
        "smartMode.disabled.desc",
        "Smart conversion is off. You must manually select text before pressing the hotkey.",
    ),
    // -- Layout Switch Modes ----------------------------------------------
    ("layoutSwitchMode.always.name", "Always switch layout"),
    (
        "layoutSwitchMode.ifLastWord.name",
        "Switch if last word was converted",
    ),
    (
        "layoutSwitchMode.ifAnyWord.name",
        "Switch if any word required conversion",
    ),
    (
        "layoutSwitchMode.always.desc",
        "Always switch the keyboard layout after pressing the hotkey, regardless of whether text was converted.",
    ),
    (
        "layoutSwitchMode.ifLastWord.desc",
        "Switch the layout only if the last word before the cursor was typed in the wrong layout and got reconverted.",
    ),
    (
        "layoutSwitchMode.ifAnyWord.desc",
        "Switch the layout if any word in the converted text needed reconversion to the correct layout.",
    ),
    // -- Copy/Paste Keys (Windows only) -----------------------------------
    (
        "clipboardKeys.ctrlInsert.name",
        "Ctrl+Insert / Shift+Insert",
    ),
    (
        "clipboardKeys.ctrlInsert.desc",
        "Recommended. Works in most apps and is safe in terminals, where Ctrl+C interrupts the running command.",
    ),
    ("clipboardKeys.ctrlC.name", "Ctrl+C / Ctrl+V"),
    (
        "clipboardKeys.ctrlC.desc",
        "Use it if an app does not react to Ctrl+Insert. Careful: in terminals Ctrl+C interrupts the running command.",
    ),
    // -- Layouts Tab ------------------------------------------------------
    ("layouts.title", "Active Keyboard Layouts"),
    (
        "layouts.description",
        "These layouts are detected from your Windows language settings. The converter maps characters between them based on physical key positions.",
    ),
    ("layouts.refresh", "Refresh from System"),
    ("layouts.count", "%d layouts detected"),
    // -- Hotkey Tab -------------------------------------------------------
    ("hotkey.title", "Keyboard Shortcut"),
    (
        "hotkey.description",
        "Press this shortcut to convert text. If text is selected it converts the selection; otherwise Smart Conversion selects the text before the cursor.",
    ),
    ("hotkey.current", "Current shortcut:"),
    ("hotkey.useDoubleShift", "Use Double Shift as shortcut"),
    (
        "hotkey.doubleShiftHint",
        "Quickly press Shift twice to trigger conversion. This is the recommended shortcut - fast and doesn't conflict with other apps.",
    ),
    (
        "hotkey.doubleShiftDescription",
        "Shift Shift (Double Shift)",
    ),
    ("hotkey.customTitle", "Custom Shortcut"),
    (
        "hotkey.registerFailed",
        "This shortcut is already used by another app or by Windows. Choose a different one.",
    ),
    (
        "hotkey.stickyKeys",
        "Sticky Keys is on: pressing Shift twice locks it, so Double Shift does not work. Turn off Sticky Keys or use a custom shortcut.",
    ),
    (
        "hotkey.requireModifier",
        "Use at least one of Ctrl, Alt or Win.",
    ),
    // -- Hotkey Recorder --------------------------------------------------
    (
        "hotkeyRecorder.prompt",
        "Click \"Record\" and press your desired shortcut:",
    ),
    ("hotkeyRecorder.pressKey", "Press a key combination..."),
    ("hotkeyRecorder.cancel", "Cancel"),
    ("hotkeyRecorder.record", "Record"),
    // -- Permissions Tab --------------------------------------------------
    ("permissions.title", "Permissions"),
    (
        "permissions.description",
        "Windows does not let regular apps send keystrokes to apps running as administrator. To convert text in such windows, ReShift has to run as administrator too.",
    ),
    ("permissions.adminTitle", "Administrator Mode"),
    (
        "permissions.adminOn",
        "Running as administrator - works in all windows",
    ),
    (
        "permissions.adminOff",
        "Running without administrator rights - works in all regular windows, but not in apps running as administrator",
    ),
    ("permissions.restartAsAdmin", "Restart as Administrator"),
    ("permissions.howToEnable", "Good to know:"),
    (
        "permissions.step1",
        "1. Most apps (browsers, Office, messengers) do not need administrator mode.",
    ),
    (
        "permissions.step2",
        "2. While an administrator window is active, the tray icon turns gray.",
    ),
    (
        "permissions.step3",
        "3. Start with Windows always launches ReShift without administrator rights.",
    ),
    (
        "permissions.step4",
        "4. Remote Desktop windows are skipped on purpose.",
    ),
    ("permissions.refreshStatus", "Refresh Status"),
    // -- About Window -----------------------------------------------------
    ("about.windowTitle", "About ReShift"),
    ("about.appName", "ReShift"),
    ("about.version", "Version %@"),
    (
        "about.tagline",
        "Open-source keyboard layout text converter for Windows",
    ),
    ("about.howToUse", "How to use:"),
    ("about.step1", "Select text typed in wrong layout"),
    ("about.step2", "Press ⇧⇧ (double Shift)"),
    ("about.step3", "Text is automatically converted!"),
    ("about.github", "GitHub"),
    ("about.license", "MIT License"),
    // -- Alerts -----------------------------------------------------------
    ("alert.elevatedTargetTitle", "Administrator window"),
    (
        "alert.elevatedTargetMessage",
        "This window runs as administrator, so Windows blocks ReShift from changing text in it. Restart ReShift as administrator (Settings > Permissions) to make it work there too.",
    ),
    ("alert.stickyKeysTitle", "Sticky Keys is on"),
    (
        "alert.stickyKeysMessage",
        "Pressing Shift twice locks it, so Double Shift conversion is disabled. Turn off Sticky Keys or set a custom shortcut (Settings > Hotkey).",
    ),
    ("alert.hotkeyFailedTitle", "Shortcut unavailable"),
    (
        "alert.hotkeyFailedMessage",
        "%@ is already used by another app or by Windows. Choose a different shortcut in Settings > Hotkey.",
    ),
    (
        "alert.alreadyRunning",
        "ReShift is already running as administrator.",
    ),
];
