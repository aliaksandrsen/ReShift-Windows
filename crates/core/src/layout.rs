//! MARK: - Keyboard Layout Model

/// A system keyboard layout.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyboardLayout {
    /// Windows keyboard layout ID (KLID), e.g. "00000419".
    pub id: String,
    /// e.g. "US" or "Русская".
    pub localized_name: String,
    /// e.g. "en", "ru".
    pub language_code: String,
}

impl KeyboardLayout {
    pub fn new(id: &str, localized_name: &str, language_code: &str) -> Self {
        Self {
            id: id.to_owned(),
            localized_name: localized_name.to_owned(),
            language_code: language_code.to_owned(),
        }
    }

    /// e.g. "Russian (RU)".
    pub fn display_name(&self) -> String {
        format!(
            "{} ({})",
            self.localized_name,
            self.language_code.to_uppercase()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_shows_the_language_code() {
        assert_eq!(
            KeyboardLayout::new("00000419", "Russian", "ru").display_name(),
            "Russian (RU)"
        );
    }
}
