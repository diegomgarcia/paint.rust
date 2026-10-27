use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    CommandOutcome, CommandRunner, CommandSpec, cargo_toolchain_path, select_cargo_program,
    verify_with_runner,
};
use crate::workspace::Workspace;
use crate::{Error, Result};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new() -> Self {
        let id = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("paint-r verify unit {} {id}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).expect("remove stale unit test root");
        }
        fs::create_dir_all(path.join("src")).expect("create unit test root");
        fs::write(path.join("Cargo.toml"), "[workspace]\nmembers = []\n")
            .expect("write unit test manifest");
        fs::write(path.join("src/lib.rs"), "pub fn real_code() {}\n")
            .expect("write unit test source");
        Self { path }
    }

    fn workspace(&self) -> Workspace {
        Workspace::at(&self.path).expect("valid unit test workspace")
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Default)]
struct MockRunner {
    outcomes: VecDeque<CommandOutcome>,
    commands: Vec<CommandSpec>,
}

impl MockRunner {
    fn succeeding(step_count: usize) -> Self {
        Self {
            outcomes: (0..step_count)
                .map(|_| CommandOutcome {
                    success: true,
                    code: Some(0),
                    ..CommandOutcome::default()
                })
                .collect(),
            commands: Vec::new(),
        }
    }
}

impl CommandRunner for MockRunner {
    fn run(&mut self, spec: &CommandSpec, _workspace: &Workspace) -> Result<CommandOutcome> {
        self.commands.push(spec.clone());
        self.outcomes
            .pop_front()
            .ok_or_else(|| Error::new("mock runner received an unexpected command"))
    }
}

#[test]
fn successful_verification_runs_every_cargo_step_in_order() {
    let root = TestRoot::new();
    let workspace = root.workspace();
    let mut runner = MockRunner::succeeding(4);
    let mut out = Vec::new();
    let mut err = Vec::new();

    let report = verify_with_runner(
        &workspace,
        OsStr::new("/pinned/tools/cargo"),
        &mut runner,
        &mut out,
        &mut err,
    )
    .expect("verification should pass");

    assert_eq!(report.cargo_steps, 4);
    assert_eq!(report.scan.scanned_files, 1);
    assert!(err.is_empty());
    let names: Vec<_> = runner.commands.iter().map(|command| command.name).collect();
    assert_eq!(
        names,
        vec![
            "format",
            "strict clippy",
            "workspace tests",
            "locked metadata"
        ]
    );
    assert!(
        runner
            .commands
            .iter()
            .all(|command| command.program == OsStr::new("/pinned/tools/cargo"))
    );
    assert!(runner.commands[3].capture_output);
    let output = String::from_utf8(out).expect("UTF-8 report");
    assert!(output.contains("verification passed"));
    assert!(output.contains("supplemental"));
}

#[test]
fn failing_clippy_stops_tests_and_metadata() {
    let root = TestRoot::new();
    let workspace = root.workspace();
    let mut runner = MockRunner::succeeding(1);
    runner.outcomes.push_back(CommandOutcome {
        success: false,
        code: Some(17),
        ..CommandOutcome::default()
    });
    let mut out = Vec::new();
    let mut err = Vec::new();

    let error = verify_with_runner(
        &workspace,
        OsStr::new("cargo"),
        &mut runner,
        &mut out,
        &mut err,
    )
    .expect_err("clippy failure must fail verification");

    assert!(error.to_string().contains("exit code 17"));
    assert_eq!(runner.commands.len(), 2);
    assert_eq!(runner.commands[1].name, "strict clippy");
    assert!(String::from_utf8(err).expect("UTF-8 errors").contains("17"));
}

#[test]
fn captured_metadata_failure_is_reported() {
    let root = TestRoot::new();
    let workspace = root.workspace();
    let mut runner = MockRunner::succeeding(3);
    runner.outcomes.push_back(CommandOutcome {
        success: false,
        code: Some(9),
        stdout: b"partial metadata\n".to_vec(),
        stderr: b"lockfile needs update\n".to_vec(),
    });
    let mut out = Vec::new();
    let mut err = Vec::new();

    verify_with_runner(
        &workspace,
        OsStr::new("cargo"),
        &mut runner,
        &mut out,
        &mut err,
    )
    .expect_err("metadata failure must fail verification");

    let errors = String::from_utf8(err).expect("UTF-8 errors");
    assert!(errors.contains("partial metadata"), "{errors}");
    assert!(errors.contains("lockfile needs update"), "{errors}");
    assert!(errors.contains("locked metadata"), "{errors}");
}

#[test]
fn cargo_selection_prefers_explicit_override_then_invoking_binary() {
    assert_eq!(
        select_cargo_program(
            Some(OsString::from("/override/cargo")),
            Some(OsString::from("/invoking/cargo")),
        ),
        OsStr::new("/override/cargo")
    );
    assert_eq!(
        select_cargo_program(None, Some(OsString::from("/invoking/cargo"))),
        OsStr::new("/invoking/cargo")
    );
    assert_eq!(select_cargo_program(None, None), OsStr::new("cargo"));
}

#[test]
fn cargo_sibling_tools_precede_the_existing_path() {
    let cargo = if cfg!(windows) {
        OsStr::new(r"C:\pinned\bin\cargo.exe")
    } else {
        OsStr::new("/pinned/bin/cargo")
    };
    let path = cargo_toolchain_path(cargo, Some(OsString::from("ambient-tools")))
        .expect("construct child PATH")
        .expect("absolute Cargo has a tools directory");
    let entries: Vec<_> = std::env::split_paths(&path).collect();
    assert_eq!(
        entries[0],
        PathBuf::from(cargo).parent().expect("parent").to_path_buf()
    );
    assert_eq!(entries[1], PathBuf::from("ambient-tools"));
    assert!(
        cargo_toolchain_path(OsStr::new("cargo"), None)
            .expect("bare Cargo uses ambient PATH")
            .is_none()
    );
}

#[test]
fn command_display_quotes_paths_with_spaces() {
    let spec = CommandSpec {
        name: "test",
        program: OsString::from("/path with spaces/cargo"),
        arguments: vec![OsString::from("test")],
        capture_output: false,
    };
    assert_eq!(spec.display(), "\"/path with spaces/cargo\" test");
}
