//! Command line options:
//!
//! - `--data-dir <path>`   keep settings in another folder (tests, E2E runs)
//! - `--wait-for-pid <id>` wait for that process to exit first (restart as administrator)
//! - `--log-file <path>`   also write the diagnostic log to a file (troubleshooting)

use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandLineOptions {
    pub data_directory: Option<PathBuf>,
    pub wait_for_process_id: Option<u32>,
    pub log_file: Option<PathBuf>,
}

impl CommandLineOptions {
    /// Parses the arguments after the executable name. Unknown arguments are ignored.
    pub fn parse(args: &[String]) -> Self {
        let mut options = Self::default();
        let mut i = 0;
        while i + 1 < args.len() {
            match args[i].as_str() {
                "--data-dir" => {
                    i += 1;
                    options.data_directory = Some(PathBuf::from(&args[i]));
                }
                "--log-file" => {
                    i += 1;
                    options.log_file = Some(PathBuf::from(&args[i]));
                }
                "--wait-for-pid" => {
                    if let Ok(pid) = args[i + 1].parse() {
                        options.wait_for_process_id = Some(pid);
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        options
    }

    /// The process arguments (lossy UTF-8).
    pub fn from_env() -> Self {
        let args: Vec<String> = std::env::args_os()
            .skip(1)
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        Self::parse(&args)
    }

    /// Arguments to pass on when restarting (keeps an isolated data folder).
    pub fn forwarded_arguments(&self) -> Vec<String> {
        self.data_directory
            .iter()
            .flat_map(|dir| ["--data-dir".to_owned(), dir.display().to_string()])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> CommandLineOptions {
        CommandLineOptions::parse(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn all_flags_are_parsed() {
        let options = parse(&[
            "--data-dir",
            r"C:\e2e",
            "--wait-for-pid",
            "42",
            "--log-file",
            "log.txt",
        ]);
        assert_eq!(options.data_directory, Some(PathBuf::from(r"C:\e2e")));
        assert_eq!(options.wait_for_process_id, Some(42));
        assert_eq!(options.log_file, Some(PathBuf::from("log.txt")));
        assert_eq!(options.forwarded_arguments(), ["--data-dir", r"C:\e2e"]);
    }

    #[test]
    fn missing_values_and_unknown_flags_are_ignored() {
        assert_eq!(parse(&["--data-dir"]), CommandLineOptions::default());
        assert_eq!(
            parse(&["--wait-for-pid", "abc"]),
            CommandLineOptions::default()
        );
        assert_eq!(
            parse(&["--verbose", "--log-file", "x"]).log_file,
            Some(PathBuf::from("x"))
        );
        assert!(parse(&[]).forwarded_arguments().is_empty());
    }
}
