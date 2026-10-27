use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use crate::scanner::scan_workspace;
use crate::verify::verify;
use crate::workspace::Workspace;

const EXIT_SUCCESS: u8 = 0;
const EXIT_FAILURE: u8 = 1;
const EXIT_USAGE: u8 = 2;

const HELP: &str = "Paint.R workspace verification\n\nUsage: xtask [--workspace <PATH>] <COMMAND> [OPTIONS]\n\nCommands:\n  verify       Run format, strict clippy, locked tests/metadata, and stub scan\n  scan-stubs   Scan Rust source for stubs and suspicious tests\n\nGlobal options:\n  --workspace <PATH>  Use an explicit workspace instead of upward discovery\n  -h, --help          Print help\n";
const SCAN_HELP: &str = "Usage: xtask [--workspace <PATH>] scan-stubs [--allowlist <PATH>]...\n\nAllowlist entries use '<relative-glob>::<rule>'. Relative allowlist paths are\nresolved from the workspace. The scan supplements tests; it cannot replace them.\n";
const VERIFY_HELP: &str = "Usage: xtask [--workspace <PATH>] verify\n\nRuns, in order: cargo fmt --check, strict clippy, locked workspace tests, locked\nmetadata, and the supplemental stub scan. Any failure returns a nonzero status.\n";

pub fn run(
    arguments: impl IntoIterator<Item = OsString>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    match run_inner(arguments, out, err) {
        Ok(code) => code,
        Err(failure) => {
            let _ = writeln!(err, "error: {}", failure.message);
            if failure.usage {
                let _ = writeln!(err, "\n{HELP}");
            }
            failure.code
        }
    }
}

fn run_inner(
    arguments: impl IntoIterator<Item = OsString>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> std::result::Result<u8, Failure> {
    let parsed = parse(arguments.into_iter().collect()).map_err(Failure::usage)?;
    let options = match parsed {
        Parsed::Help(text) => {
            writeln!(out, "{text}").map_err(|error| Failure::operational(error.to_string()))?;
            return Ok(EXIT_SUCCESS);
        }
        Parsed::Run(options) => options,
    };
    let workspace = match options.workspace {
        Some(path) => Workspace::at(&path),
        None => Workspace::discover(),
    }
    .map_err(|error| Failure::usage(error.to_string()))?;

    match options.command {
        Command::Verify => verify(&workspace, out, err)
            .map(|_| EXIT_SUCCESS)
            .map_err(|error| Failure::new(EXIT_FAILURE, error.to_string(), false)),
        Command::ScanStubs { allowlists } => {
            let allowlists: Vec<PathBuf> = allowlists
                .into_iter()
                .map(|path| {
                    if path.is_absolute() {
                        path
                    } else {
                        workspace.root().join(path)
                    }
                })
                .collect();
            let report = scan_workspace(workspace.root(), &allowlists)
                .map_err(|error| Failure::usage(error.to_string()))?;
            for violation in &report.allowed {
                writeln!(out, "allowlisted: {}", violation.diagnostic())
                    .map_err(|error| Failure::operational(error.to_string()))?;
            }
            for violation in &report.violations {
                writeln!(err, "error: {}", violation.diagnostic())
                    .map_err(|error| Failure::operational(error.to_string()))?;
            }
            writeln!(
                out,
                "stub scan: scanned {} Rust file(s); {} violation(s); {} allowlisted; scanner supplements tests",
                report.scanned_files,
                report.violations.len(),
                report.allowed.len()
            )
            .map_err(|error| Failure::operational(error.to_string()))?;
            if report.violations.is_empty() {
                Ok(EXIT_SUCCESS)
            } else {
                writeln!(
                    err,
                    "stub scan failed with {} violation(s); scanner supplements tests and is not a substitute for them",
                    report.violations.len()
                )
                .map_err(|error| Failure::operational(error.to_string()))?;
                Ok(EXIT_FAILURE)
            }
        }
    }
}

#[derive(Debug)]
struct Failure {
    code: u8,
    message: String,
    usage: bool,
}

impl Failure {
    fn new(code: u8, message: String, usage: bool) -> Self {
        Self {
            code,
            message,
            usage,
        }
    }

    fn usage(message: String) -> Self {
        Self::new(EXIT_USAGE, message, true)
    }

    fn operational(message: String) -> Self {
        Self::new(EXIT_FAILURE, message, false)
    }
}

#[derive(Debug)]
enum Parsed {
    Help(&'static str),
    Run(Options),
}

#[derive(Debug)]
struct Options {
    workspace: Option<PathBuf>,
    command: Command,
}

#[derive(Debug)]
enum Command {
    Verify,
    ScanStubs { allowlists: Vec<PathBuf> },
}

fn parse(arguments: Vec<OsString>) -> std::result::Result<Parsed, String> {
    if arguments.is_empty() {
        return Err("a command is required".to_owned());
    }
    let mut workspace = None;
    let mut command: Option<Command> = None;
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "-h" || argument == "--help" {
            return Ok(Parsed::Help(match command {
                Some(Command::Verify) => VERIFY_HELP,
                Some(Command::ScanStubs { .. }) => SCAN_HELP,
                None => HELP,
            }));
        }
        if argument == "--workspace" {
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| "--workspace requires a path".to_owned())?;
            workspace = Some(PathBuf::from(value));
            index += 2;
            continue;
        }
        if argument == "--allowlist" {
            let Some(Command::ScanStubs { allowlists }) = command.as_mut() else {
                return Err("--allowlist is only valid after scan-stubs".to_owned());
            };
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| "--allowlist requires a path".to_owned())?;
            allowlists.push(PathBuf::from(value));
            index += 2;
            continue;
        }
        if command.is_some() {
            return Err(format!(
                "unexpected argument '{}'",
                argument.to_string_lossy()
            ));
        }
        let name = argument
            .to_str()
            .ok_or_else(|| "command name is not valid UTF-8".to_owned())?;
        command = Some(match name {
            "verify" => Command::Verify,
            "scan-stubs" => Command::ScanStubs {
                allowlists: Vec::new(),
            },
            "help" => return Ok(Parsed::Help(HELP)),
            _ => return Err(format!("unknown command '{name}'")),
        });
        index += 1;
    }
    command.map_or_else(
        || Err("a command is required".to_owned()),
        |command| Ok(Parsed::Run(Options { workspace, command })),
    )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{Command, Parsed, parse};

    #[test]
    fn parser_accepts_workspace_options_on_either_side_of_command() {
        for arguments in [
            vec!["--workspace", "root", "scan-stubs"],
            vec!["scan-stubs", "--workspace", "root"],
        ] {
            let parsed = parse(arguments.into_iter().map(OsString::from).collect())
                .expect("arguments should parse");
            let Parsed::Run(options) = parsed else {
                panic!("expected runnable options");
            };
            assert_eq!(
                options.workspace.expect("workspace").to_string_lossy(),
                "root"
            );
            assert!(matches!(options.command, Command::ScanStubs { .. }));
        }
    }

    #[test]
    fn parser_rejects_options_for_the_wrong_command() {
        let result = parse(vec![
            OsString::from("verify"),
            OsString::from("--allowlist"),
            OsString::from("file"),
        ]);
        assert!(result.is_err());
    }
}
