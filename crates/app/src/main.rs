//! ReShift for Windows: a tray utility that converts text typed in the wrong keyboard layout.

// No console window: ReShift is a tray app
#![windows_subsystem = "windows"]
// The binary is ReShift.exe
#![allow(non_snake_case)]

mod about;
mod app;
mod cli;
mod clipboard;
mod conversion;
mod elevation;
mod feedback;
mod foreground;
mod hook;
mod hotkey;
mod hotkey_format;
mod input;
mod layouts;
mod log;
mod paths;
mod replacement;
mod settings;
mod settings_window;
mod single_instance;
mod startup;
mod terminal;
mod tray;
mod uia;
mod wait;
mod win;
mod window_kind;

fn main() {
    std::process::exit(app::run());
}
