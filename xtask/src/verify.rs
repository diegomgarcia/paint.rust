//! Ordered workspace verification using the invoking Cargo toolchain.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::scanner::{ScanReport, scan_workspace};
use crate::workspace::Workspace;
use crate::{Error, Result};

const CARGO_STEP_COUNT: usize = 4;

#[derive(Debug)]
pub struct VerificationReport {
    pub cargo_steps: usize,
    pub scan: ScanReport,
}

/// Select the child Cargo without discarding the invoking toolchain environment.
///
/// `CARGO` is supplied by `cargo run` and normally points at the exact Cargo binary,
/// including a rustup-pinned invocation. `XTASK_CARGO` is an explicit test/automation
/// override. All other environment, including `RUSTC`, `RUSTFMT`, and
/// `RUSTUP_TOOLCHAIN`, is inherited by child processes.
pub fn cargo_program() -> OsString {
    select_cargo_program(std::env::var_os("XTASK_CARGO"), std::env::var_os("CARGO"))
}

fn select_cargo_program(override_: Option<OsString>, invoking: Option<OsString>) -> OsString {
    override_
        .filter(|value| !value.is_empty())
        .or_else(|| invoking.filter(|value| !value.is_empty()))
        .unwrap_or_else(|| OsString::from("cargo"))
}

pub fn verify(
    workspace: &Workspace,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<VerificationReport> {
    let mut runner = ProcessRunner;
    verify_with_runner(workspace, &cargo_program(), &mut runner, out, err)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandSpec {
    name: &'static str,
    program: OsString,
    arguments: Vec<OsString>,
    capture_output: bool,
}

impl CommandSpec {
    fn display(&self) -> String {
        std::iter::once(self.program.as_os_str())
            .chain(self.arguments.iter().map(OsString::as_os_str))
            .map(display_argument)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn display_argument(argument: &OsStr) -> String {
    let value = argument.to_string_lossy();
    if value.is_empty() {
        "''".to_owned()
    } else if value.chars().any(char::is_whitespace) {
        format!("{value:?}")
    } else {
        value.into_owned()
    }
}

#[derive(Debug, Default)]
struct CommandOutcome {
    success: bool,
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

trait CommandRunner {
    fn run(&mut self, spec: &CommandSpec, workspace: &Workspace) -> Result<CommandOutcome>;
}

struct ProcessRunner;

/// Put Cargo's sibling tools first so `cargo clippy` cannot resolve a different
/// toolchain's `cargo-clippy` from the ambient PATH.
fn cargo_toolchain_path(cargo: &OsStr, existing: Option<OsString>) -> Result<Option<OsString>> {
    let cargo_path = Path::new(cargo);
    let Some(directory) = cargo_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(None);
    };
    let mut entries = vec![directory.to_path_buf()];
    if let Some(existing) = existing {
        entries.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(entries).map(Some).map_err(|error| {
        Error::new(format!(
            "cannot add Cargo toolchain directory '{}' to PATH: {error}",
            directory.display()
        ))
    })
}

impl CommandRunner for ProcessRunner {
    fn run(&mut self, spec: &CommandSpec, workspace: &Workspace) -> Result<CommandOutcome> {
        let mut command = Command::new(&spec.program);
        command.args(&spec.arguments).current_dir(workspace.root());
        if let Some(path) = cargo_toolchain_path(&spec.program, std::env::var_os("PATH"))? {
            command.env("PATH", path);
        }
        if spec.capture_output {
            let output = command.output().map_err(|error| {
                Error::new(format!(
                    "cannot execute verification command '{}': {error}",
                    spec.display()
                ))
            })?;
            Ok(CommandOutcome {
                success: output.status.success(),
                code: output.status.code(),
                stdout: output.stdout,
                stderr: output.stderr,
            })
        } else {
            let status = command.status().map_err(|error| {
                Error::new(format!(
                    "cannot execute verification command '{}': {error}",
                    spec.display()
                ))
            })?;
            Ok(CommandOutcome {
                success: status.success(),
                code: status.code(),
                ..CommandOutcome::default()
            })
        }
    }
}

fn verify_with_runner(
    workspace: &Workspace,
    cargo: &OsStr,
    runner: &mut dyn CommandRunner,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<VerificationReport> {
    let commands = verification_commands(cargo);
    for spec in &commands {
        writeln!(out, "==> {}: {}", spec.name, spec.display())
            .map_err(|error| Error::new(format!("cannot write verification report: {error}")))?;
        let outcome = runner.run(spec, workspace)?;
        if !outcome.success {
            write_failure(spec, &outcome, err)?;
            return Err(Error::new(format!(
                "verification step '{}' failed ({})",
                spec.name,
                exit_description(outcome.code)
            )));
        }
        writeln!(out, "ok: {}", spec.name)
            .map_err(|error| Error::new(format!("cannot write verification report: {error}")))?;
    }

    writeln!(
        out,
        "==> scan-stubs (supplemental; never a substitute for tests)"
    )
    .map_err(|error| Error::new(format!("cannot write verification report: {error}")))?;
    let scan = scan_workspace(workspace.root(), &[])?;
    for violation in &scan.violations {
        writeln!(err, "error: {}", violation.diagnostic())
            .map_err(|error| Error::new(format!("cannot write scan diagnostics: {error}")))?;
    }
    writeln!(
        out,
        "stub scan: scanned {} Rust file(s); {} violation(s); {} allowlisted",
        scan.scanned_files,
        scan.violations.len(),
        scan.allowed.len()
    )
    .map_err(|error| Error::new(format!("cannot write verification report: {error}")))?;
    if !scan.violations.is_empty() {
        return Err(Error::new(format!(
            "supplemental stub scan found {} violation(s)",
            scan.violations.len()
        )));
    }
    writeln!(
        out,
        "verification passed: {CARGO_STEP_COUNT} Cargo steps and a supplemental stub scan"
    )
    .map_err(|error| Error::new(format!("cannot write verification report: {error}")))?;

    Ok(VerificationReport {
        cargo_steps: commands.len(),
        scan,
    })
}

fn verification_commands(cargo: &OsStr) -> Vec<CommandSpec> {
    let definitions: [(&str, &[&str], bool); CARGO_STEP_COUNT] = [
        ("format", &["fmt", "--all", "--", "--check"], false),
        (
            "strict clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--locked",
                "--",
                "-D",
                "warnings",
            ],
            false,
        ),
        (
            "workspace tests",
            &["test", "--workspace", "--all-features", "--locked"],
            false,
        ),
        (
            "locked metadata",
            &["metadata", "--format-version", "1", "--locked", "--no-deps"],
            true,
        ),
    ];
    definitions
        .into_iter()
        .map(|(name, arguments, capture_output)| CommandSpec {
            name,
            program: cargo.to_os_string(),
            arguments: arguments.iter().map(OsString::from).collect(),
            capture_output,
        })
        .collect()
}

fn write_failure(spec: &CommandSpec, outcome: &CommandOutcome, err: &mut dyn Write) -> Result<()> {
    writeln!(
        err,
        "error: {} failed ({})",
        spec.name,
        exit_description(outcome.code)
    )
    .map_err(|error| Error::new(format!("cannot write verification failure: {error}")))?;
    if spec.capture_output {
        if !outcome.stdout.is_empty() {
            writeln!(err, "--- {} stdout ---", spec.name)
                .and_then(|()| err.write_all(&outcome.stdout))
                .and_then(|()| {
                    if outcome.stdout.ends_with(b"\n") {
                        Ok(())
                    } else {
                        writeln!(err)
                    }
                })
                .map_err(|error| {
                    Error::new(format!("cannot write captured command stdout: {error}"))
                })?;
        }
        if !outcome.stderr.is_empty() {
            writeln!(err, "--- {} stderr ---", spec.name)
                .and_then(|()| err.write_all(&outcome.stderr))
                .and_then(|()| {
                    if outcome.stderr.ends_with(b"\n") {
                        Ok(())
                    } else {
                        writeln!(err)
                    }
                })
                .map_err(|error| {
                    Error::new(format!("cannot write captured command stderr: {error}"))
                })?;
        }
    }
    Ok(())
}

fn exit_description(code: Option<i32>) -> String {
    code.map_or_else(
        || "terminated by signal".to_owned(),
        |code| format!("exit code {code}"),
    )
}

#[cfg(test)]
mod tests;
