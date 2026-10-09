//! Process-level argument handling.
//!
//! Packaging, installers, and the self-update path query the binary instead of
//! clicking it, so those flags must answer without opening a window or booting
//! the embedded runtime.

/// What the process was asked to do before any window exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessCommand {
    /// No informational flag was passed: start the shell window.
    Run,
    /// `--version` / `-V`.
    Version,
    /// `--help` / `-h`.
    Help,
}

pub const BIN_NAME: &str = "openbitfun-gpui-shell";

pub fn parse_process_args<'a>(args: impl IntoIterator<Item = &'a str>) -> ProcessCommand {
    for arg in args {
        match arg {
            "-V" | "--version" => return ProcessCommand::Version,
            "-h" | "--help" => return ProcessCommand::Help,
            _ => {}
        }
    }
    ProcessCommand::Run
}

pub fn help_text() -> String {
    format!(
        "{BIN_NAME} {}\n\
         \n\
         Native GPU-rendered desktop frontend for OpenBitFun.\n\
         \n\
         Usage: {BIN_NAME} [options]\n\
         \n\
         Options:\n\
         \x20 -h, --help     Print this help and exit\n\
         \x20 -V, --version  Print the version and exit\n",
        env!("CARGO_PKG_VERSION")
    )
}

pub fn version_text() -> String {
    format!("{BIN_NAME} {}", env!("CARGO_PKG_VERSION"))
}

/// Answers an informational flag. Returns the process exit code when the caller
/// should exit without starting the shell.
pub fn answer_process_command(command: ProcessCommand) -> Option<i32> {
    match command {
        ProcessCommand::Run => None,
        ProcessCommand::Version => {
            println!("{}", version_text());
            Some(0)
        }
        ProcessCommand::Help => {
            print!("{}", help_text());
            Some(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn informational_flags_map_to_process_commands() {
        assert_eq!(parse_process_args(["--version"]), ProcessCommand::Version);
        assert_eq!(parse_process_args(["-V"]), ProcessCommand::Version);
        assert_eq!(parse_process_args(["--help"]), ProcessCommand::Help);
        assert_eq!(parse_process_args(["-h"]), ProcessCommand::Help);
        assert_eq!(
            parse_process_args(["extra", "--version"]),
            ProcessCommand::Version
        );
    }

    #[test]
    fn flags_are_read_before_any_other_argument() {
        assert_eq!(
            parse_process_args(["--", "--version"]),
            ProcessCommand::Version
        );
        assert_eq!(parse_process_args(Vec::<&str>::new()), ProcessCommand::Run);
        assert_eq!(parse_process_args(["--unknown"]), ProcessCommand::Run);
    }

    #[test]
    fn version_and_help_report_the_package_identity() {
        assert!(version_text().starts_with(BIN_NAME));
        assert!(version_text().contains(env!("CARGO_PKG_VERSION")));
        let help = help_text();
        assert!(help.contains("Usage:"));
        assert!(help.contains("--version"));
        assert_eq!(answer_process_command(ProcessCommand::Version), Some(0));
        assert_eq!(answer_process_command(ProcessCommand::Help), Some(0));
        assert_eq!(answer_process_command(ProcessCommand::Run), None);
    }
}
