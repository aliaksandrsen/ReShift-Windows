//! Where ReShift keeps its data (`%LOCALAPPDATA%\ReShift`).

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AppPaths {
    data_directory: PathBuf,
    is_overridden: bool,
}

impl AppPaths {
    /// The default folder, or `--data-dir` (tests and E2E runs use an isolated folder).
    pub fn new(data_directory: Option<&Path>) -> Self {
        match data_directory {
            Some(directory) => Self {
                data_directory: std::path::absolute(directory)
                    .unwrap_or_else(|_| directory.to_path_buf()),
                is_overridden: true,
            },
            None => Self {
                data_directory: local_app_data().join("ReShift"),
                is_overridden: false,
            },
        }
    }

    pub fn data_directory(&self) -> &Path {
        &self.data_directory
    }

    pub fn is_overridden(&self) -> bool {
        self.is_overridden
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_directory.join("settings.json")
    }
}

fn local_app_data() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(r"AppData\Local"))
        })
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_folder_is_in_local_app_data() {
        let paths = AppPaths::new(None);
        assert!(!paths.is_overridden());
        assert!(paths.settings_file().ends_with(r"ReShift\settings.json"));
    }

    #[test]
    fn data_dir_override_is_made_absolute() {
        let paths = AppPaths::new(Some(Path::new("e2e-data")));
        assert!(paths.is_overridden());
        assert!(paths.data_directory().is_absolute());
        assert!(paths.settings_file().ends_with(r"e2e-data\settings.json"));
    }
}
