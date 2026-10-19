//! Process-level argument handling.
//!
//! Packaging, installers, and the self-update path query the binary instead of
//! clicking it, so those flags must answer without opening a window or booting
//! the embedded runtime.

/// What the process was asked to do before any window exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessCommand {
    /// No informational flag was passed: start the shell window.
    Run,
    /// `--version` / `-V`.
    Version,
    /// `--help` / `-h`.
    Help,
    /// `--check-update`: report whether a newer release is available.
    CheckUpdate { manifest: Option<String> },
    /// `--self-update`: download, verify, and install the newest release.
    SelfUpdate { manifest: Option<String> },
    /// An argument the binary cannot act on; the message is the reason.
    Invalid(String),
}

impl ProcessCommand {
    /// The manifest override carried by an update command, if any.
    pub fn manifest(&self) -> Option<&str> {
        match self {
            Self::CheckUpdate { manifest } | Self::SelfUpdate { manifest } => manifest.as_deref(),
            _ => None,
        }
    }

    fn is_update(&self) -> bool {
        matches!(self, Self::CheckUpdate { .. } | Self::SelfUpdate { .. })
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Version => "--version",
            Self::Help => "--help",
            Self::CheckUpdate { .. } => "--check-update",
            Self::SelfUpdate { .. } => "--self-update",
            Self::Invalid(_) => "invalid",
        }
    }
}

pub const BIN_NAME: &str = "openbitfun-gpui-shell";

/// Usage error: unknown or conflicting options, and builds that cannot update
/// themselves.
pub const EXIT_USAGE: i32 = 2;
/// `--check-update` exit code when a newer release exists (older otherwise: 0).
pub const EXIT_UPDATE_AVAILABLE: i32 = 10;

/// Reads the process arguments. Informational and update flags are recognized
/// wherever they appear; an unknown `-flag` is reported instead of silently
/// opening a window on what was probably a typo. Plain positional arguments are
/// tolerated (a desktop launcher may pass one) and ignored.
pub fn parse_process_args<'a>(args: impl IntoIterator<Item = &'a str>) -> ProcessCommand {
    let mut command = ProcessCommand::Run;
    let mut manifest: Option<String> = None;
    let mut args = args.into_iter().peekable();

    while let Some(arg) = args.next() {
        let candidate = match arg {
            "-h" | "--help" => ProcessCommand::Help,
            "-V" | "--version" => ProcessCommand::Version,
            "--check-update" => ProcessCommand::CheckUpdate { manifest: None },
            "--self-update" => ProcessCommand::SelfUpdate { manifest: None },
            "--update-manifest" => {
                let Some(value) = args.next() else {
                    return ProcessCommand::Invalid(
                        "`--update-manifest` needs a URL or a file path".to_string(),
                    );
                };
                manifest = Some(value.to_string());
                continue;
            }
            other if other.starts_with("--update-manifest=") => {
                manifest = Some(other["--update-manifest=".len()..].to_string());
                continue;
            }
            // A lone `--` separates options from values; skipping it keeps the
            // historical "flags are read wherever they appear" contract.
            "--" => continue,
            other if other.starts_with('-') && other != "-" => {
                return ProcessCommand::Invalid(format!("unknown option `{other}`"));
            }
            _ => continue,
        };
        command = match (command, candidate) {
            (ProcessCommand::Run, candidate) => candidate,
            (current, candidate) if current.is_update() && candidate.is_update() => {
                return ProcessCommand::Invalid(format!(
                    "`{}` and `{}` cannot be combined",
                    current.name(),
                    candidate.name()
                ));
            }
            // An update command paired with an informational flag: one of them
            // would have to be ignored silently, so the combination is refused.
            (current, candidate) if current.is_update() || candidate.is_update() => {
                let (informational, update) = if current.is_update() {
                    (candidate, current)
                } else {
                    (current, candidate)
                };
                return ProcessCommand::Invalid(format!(
                    "`{}` and `{}` cannot be combined",
                    informational.name(),
                    update.name()
                ));
            }
            // Two informational flags: the first one wins, matching the
            // historical behavior of `--help --version` printing help.
            (current, _) => current,
        };
    }

    match (manifest, command.is_update()) {
        (Some(manifest), true) => match command {
            ProcessCommand::CheckUpdate { .. } => ProcessCommand::CheckUpdate {
                manifest: Some(manifest),
            },
            ProcessCommand::SelfUpdate { .. } => ProcessCommand::SelfUpdate {
                manifest: Some(manifest),
            },
            other => other,
        },
        (Some(_), false) => ProcessCommand::Invalid(
            "`--update-manifest` only applies to `--check-update` or `--self-update`".to_string(),
        ),
        (None, _) => command,
    }
}

pub fn help_text() -> String {
    format!(
        r#"{BIN_NAME} {}

Native GPU-rendered desktop frontend for OpenBitFun.

Usage: {BIN_NAME} [options]

Options:
  -h, --help                  Print this help and exit
  -V, --version               Print the version and exit
      --check-update          Report whether a newer release is available and
                              exit (exit code 10 when one is)
      --self-update           Download, verify, and install the newest release
                              in place
      --update-manifest <url|path>
                              Read the release manifest from this URL or file
                              instead of the official feed
"#,
        env!("CARGO_PKG_VERSION")
    )
}

pub fn version_text() -> String {
    format!("{BIN_NAME} {}", env!("CARGO_PKG_VERSION"))
}

/// Answers a process-level flag that needs no runtime or network. Returns the
/// process exit code when the caller should exit without starting the shell;
/// update commands are answered by `crate::update`.
pub fn answer_process_command(command: &ProcessCommand) -> Option<i32> {
    match command {
        ProcessCommand::Run
        | ProcessCommand::CheckUpdate { .. }
        | ProcessCommand::SelfUpdate { .. } => None,
        ProcessCommand::Version => {
            println!("{}", version_text());
            Some(0)
        }
        ProcessCommand::Help => {
            print!("{}", help_text());
            Some(0)
        }
        ProcessCommand::Invalid(message) => {
            eprintln!("{BIN_NAME}: {message}");
            eprintln!("Try `{BIN_NAME} --help` for the available options.");
            Some(EXIT_USAGE)
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
        // Positional arguments are tolerated (a launcher may pass one).
        assert_eq!(parse_process_args(["/some/path"]), ProcessCommand::Run);
    }

    #[test]
    fn update_flags_carry_an_optional_manifest() {
        assert_eq!(
            parse_process_args(["--check-update"]),
            ProcessCommand::CheckUpdate { manifest: None }
        );
        assert_eq!(
            parse_process_args(["--self-update"]),
            ProcessCommand::SelfUpdate { manifest: None }
        );
        assert_eq!(
            parse_process_args([
                "--self-update",
                "--update-manifest",
                "http://127.0.0.1:9/m.json"
            ]),
            ProcessCommand::SelfUpdate {
                manifest: Some("http://127.0.0.1:9/m.json".to_string())
            }
        );
        let command = parse_process_args(["--update-manifest=/tmp/m.json", "--check-update"]);
        assert_eq!(
            command,
            ProcessCommand::CheckUpdate {
                manifest: Some("/tmp/m.json".to_string())
            }
        );
        assert_eq!(command.manifest(), Some("/tmp/m.json"));
        assert_eq!(
            parse_process_args(["--check-update"]).manifest(),
            None::<&str>
        );
    }

    #[test]
    fn option_mistakes_are_reported_instead_of_opening_a_window() {
        assert!(matches!(
            parse_process_args(["--unknown"]),
            ProcessCommand::Invalid(_)
        ));
        assert!(matches!(
            parse_process_args(["--self-updat"]),
            ProcessCommand::Invalid(_)
        ));
        assert!(matches!(
            parse_process_args(["--update-manifest"]),
            ProcessCommand::Invalid(_)
        ));
        assert!(matches!(
            parse_process_args(["--update-manifest", "/tmp/m.json"]),
            ProcessCommand::Invalid(_)
        ));
        assert!(matches!(
            parse_process_args(["--check-update", "--self-update"]),
            ProcessCommand::Invalid(_)
        ));
        assert!(matches!(
            parse_process_args(["--self-update", "--version"]),
            ProcessCommand::Invalid(_)
        ));
        // A single dash is a value, not a flag.
        assert_eq!(parse_process_args(["-"]), ProcessCommand::Run);
    }

    #[test]
    fn the_first_informational_flag_wins() {
        assert_eq!(
            parse_process_args(["--help", "--version"]),
            ProcessCommand::Help
        );
        assert_eq!(
            parse_process_args(["--version", "--help"]),
            ProcessCommand::Version
        );
    }

    #[test]
    fn version_and_help_report_the_package_identity() {
        assert!(version_text().starts_with(BIN_NAME));
        assert!(version_text().contains(env!("CARGO_PKG_VERSION")));
        let help = help_text();
        assert!(help.contains("Usage:"));
        assert!(help.contains("--version"));
        assert!(help.contains("--check-update"));
        assert!(help.contains("--self-update"));
        assert!(help.contains("--update-manifest"));
        assert_eq!(answer_process_command(&ProcessCommand::Version), Some(0));
        assert_eq!(answer_process_command(&ProcessCommand::Help), Some(0));
        assert_eq!(answer_process_command(&ProcessCommand::Run), None);
        assert_eq!(
            answer_process_command(&ProcessCommand::CheckUpdate { manifest: None }),
            None
        );
        assert_eq!(
            answer_process_command(&ProcessCommand::Invalid("boom".to_string())),
            Some(EXIT_USAGE)
        );
    }
}
