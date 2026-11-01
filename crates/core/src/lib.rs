//! Platform-neutral logic of ReShift: character maps, layout mapping, text conversion,
//! the double Shift state machine, the terminal line model and localization.
//!
//! No Windows APIs here. Diagnostics must never contain the user's text.

pub mod double_shift;
pub mod l10n;
pub mod layout;
pub mod layout_char_map;
pub mod layout_mapper;
pub mod modes;
pub mod terminal_input;
pub mod text_converter;
mod text_elements;

#[cfg(test)]
mod test_layouts;

pub use double_shift::DoubleShiftDetector;
pub use layout::KeyboardLayout;
pub use modes::{ClipboardKeys, LayoutSwitchMode, SmartConversionMode};
pub use text_converter::{ConversionResult, TextConverter, WrongLayoutBoundary};
