# AGENTS.md - Instructions for AI Agents and Contributors

ReShift for Windows: a lean, fast tray utility that converts text typed in the wrong keyboard layout.
The goal is a responsive, reliable tool: it must never hang, use little memory, start instantly and stay lightweight.

## Testing Requirements

**Tests MUST be run after every edit of `.rs` files:**

```powershell
cargo test --workspace
```

### Rules

1. **After every code change**: run the unit tests before considering the task complete.
2. **No failures, no warnings**: before a commit also run `cargo fmt --all -- --check` and
   `cargo clippy --workspace --all-targets -- -D warnings` (`scripts\build.ps1` runs all of them).
3. **New functionality requires tests**: any new feature, bug fix or behavioral change needs unit tests.
4. **Release build**: `powershell -ExecutionPolicy Bypass -File scripts\build.ps1` => `artifacts\ReShift.exe`.

### Test Structure

- Unit tests live next to the code (`#[cfg(test)] mod tests`).

## Project Structure

- `crates/core` (`reshift-core`) - platform-neutral logic, no Windows APIs.
- `crates/app` (`reshift`, binary `ReShift.exe`) - Win32 tray app: interop, services, windows.
- `crates/app/res` - icons, manifest and the resource script; `build.rs` fills in the version from Cargo.toml.
- Section comments `// MARK: -` are kept to make navigation easy.

## Settings and Data

- `%LOCALAPPDATA%\ReShift\settings.json`: camelCase keys, enums stored as numbers, unknown keys ignored.
- Single-instance mutex `Local\ReShift.Windows.SingleInstance` and its activation event.
- Run key value `ReShift`, executable name `ReShift.exe`.
- Command line `--data-dir`, `--log-file`, `--wait-for-pid` and the log line `Hotkey ready`.

## Windows Mechanics - Do Not Break

- **Threads**: the UI thread never waits for a conversion; the conversion pipeline runs on its own thread;
  every wait has a timeout.
- The **low-level keyboard hook runs on its own thread**; its callback must stay fast (Windows silently
  removes slow hooks). It only feeds the double Shift detector and the terminal tracker and posts messages.
- Our own `SendInput` events carry the `0x4C535743` ("LSWC") extra info; the hook ignores only those.
- Arrows, Home, End, Insert, Delete need `KEYEVENTF_EXTENDEDKEY` (NumLock would turn them into numpad keys).
- Clipboard access goes through the clipboard module (owner window, retries, snapshot/restore,
  history and cloud exclusion formats).
- One low-level hook (keyboard + mouse clicks) feeds both the double Shift detector and the terminal tracker.
  It runs only while one of them is enabled.

## Reliability Rules

- No `unwrap`/`expect` outside tests (`clippy::unwrap_used` and `clippy::expect_used` are denied).
- Every `extern "system"` callback body and the conversion pipeline run inside `catch_unwind`:
  a panic must neither cross the FFI boundary nor kill the tray app.
- Win32 resources (icons, menus, hooks, handles) are owned by RAII wrappers that free them in `Drop`.

## Terminal Path (Windows only)

- Terminals cannot be selected/copied, so the terminal tracker reconstructs the typed line from scan codes
  and the active layout (physical key map + the same character maps); the terminal line converter turns the
  Smart Conversion result into Backspaces + Unicode text.
- **Privacy**: the typed line lives in memory only. Never log its content (only its length), never persist it.
  The same applies to selected text in the clipboard path.
- Anything the tracker cannot reproduce (shortcuts, navigation, dead keys, AltGr, clicks, window changes)
  must reset the line - a wrong line would erase the wrong characters.
- Classic console windows are attributed by user32 to the console client; resolve the conhost thread
  whose layout applies.

## Keyboard Layouts

- Layout IDs are Windows KLIDs (`"00000419"`); the core maps KLIDs to character maps.
- Built-in tables describe character maps. Adding a layout: add the map string, register its KLIDs, add tests.
  The tests enforce 94 entries and no duplicate values.

## Localization Guidelines

1. **All user-facing strings** go through the localization module of the core - never hardcode UI text.
2. **Keys use namespace prefixes**: `menu.*`, `settings.*`, `general.*`, `smartMode.*`, `layoutSwitchMode.*`,
   `clipboardKeys.*`, `layouts.*`, `hotkey.*`, `hotkeyRecorder.*`, `permissions.*`, `about.*`, `alert.*`, `common.*`.
3. **When adding a key**: add it to **every** language file; a test fails if the key sets differ.
4. **Typography**: plain hyphens (" - ") instead of dashes, straight quotes, "=>" instead of arrows
   (also enforced by a test).
5. **Placeholders**: `%@` for text, `%d` for numbers.
6. **Fallback**: a missing key falls back to English, then to the raw key.
