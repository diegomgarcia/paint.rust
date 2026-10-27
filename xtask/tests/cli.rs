mod common;

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::TestTree;

fn xtask() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

#[test]
fn scan_cli_discovers_workspace_from_a_nested_directory() {
    let tree = TestTree::new("cli discovery");
    tree.write("Cargo.toml", "[workspace]\nmembers = []\n");
    tree.write("src/lib.rs", "pub fn real_code() -> u8 { 1 }\n");
    tree.write("nested/deeper/keep.txt", "discovery sentinel\n");

    let output = xtask()
        .current_dir(tree.root().join("nested/deeper"))
        .arg("scan-stubs")
        .output()
        .expect("run xtask");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
    let stdout = text(&output.stdout);
    assert!(stdout.contains("scanned 1 Rust file"), "{stdout}");
    assert!(stdout.contains("supplements tests"), "{stdout}");
}

#[test]
fn scan_cli_returns_nonzero_with_file_line_and_rule() {
    let tree = TestTree::new("cli violation");
    tree.write("Cargo.toml", "[workspace]\nmembers = []\n");
    tree.write("src/lib.rs", "fn broken() {\n    todo!();\n}\n");

    let output = xtask()
        .arg("scan-stubs")
        .arg("--workspace")
        .arg(tree.root())
        .output()
        .expect("run xtask");
    assert_failure(&output);
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(stderr.contains("src/lib.rs:2:5"), "{stderr}");
    assert!(stderr.contains("todo"), "{stderr}");
    assert!(stderr.contains("supplements tests"), "{stderr}");
}

#[test]
fn scan_cli_loads_an_explicit_fixture_allowlist() {
    let tree = TestTree::new("cli allowlist");
    tree.write("Cargo.toml", "[workspace]\nmembers = []\n");
    tree.write(
        "tests/fixtures/incomplete.rs",
        "fn fixture() { todo!(); }\n",
    );
    tree.write("fixture.allowlist", "tests/fixtures/incomplete.rs::todo\n");

    let output = xtask()
        .arg("--workspace")
        .arg(tree.root())
        .arg("scan-stubs")
        .arg("--allowlist")
        .arg("fixture.allowlist")
        .output()
        .expect("run xtask");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
    let stdout = text(&output.stdout);
    assert!(stdout.contains("1 allowlisted"), "{stdout}");
}

#[test]
fn cli_rejects_invalid_invocations_and_missing_workspaces() {
    let unknown = xtask().arg("not-a-command").output().expect("run xtask");
    assert_failure(&unknown);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(text(&unknown.stderr).contains("Usage:"));

    let missing_value = xtask().arg("--workspace").output().expect("run xtask");
    assert_failure(&missing_value);
    assert_eq!(missing_value.status.code(), Some(2));

    let tree = TestTree::new("cli missing workspace");
    let absent = tree.root().join("absent");
    let output = xtask()
        .arg("--workspace")
        .arg(absent)
        .arg("scan-stubs")
        .output()
        .expect("run xtask");
    assert_failure(&output);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("workspace"));
}

fn compile_fake_cargo(tree: &TestTree) -> OsString {
    let source = tree.write(
        "fake-cargo.rs",
        r#"use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let log_path = env::var_os("FAKE_CARGO_LOG").expect("log path");
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .expect("open log");
    writeln!(log, "{}", args.join(" ")).expect("write log");
    if env::var("FAKE_CARGO_FAIL").ok().as_deref() == args.first().map(String::as_str) {
        eprintln!("injected cargo failure");
        process::exit(17);
    }
    if args.first().map(String::as_str) == Some("metadata") {
        println!("{{}}");
    }
}
"#,
    );
    let executable = tree.root().join(if cfg!(windows) {
        "fake-cargo.exe"
    } else {
        "fake-cargo"
    });
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
    let status = Command::new(rustc)
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .status()
        .expect("compile fake cargo with rustc");
    assert!(status.success(), "fake cargo must compile");
    executable.into_os_string()
}

fn logged_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .expect("read fake cargo log")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn verify_cli_uses_invoking_cargo_in_order_and_stops_on_failure() {
    let tree = TestTree::new("cli verify");
    tree.write("Cargo.toml", "[workspace]\nmembers = []\n");
    tree.write("src/lib.rs", "pub fn real_code() {}\n");
    let fake_cargo = compile_fake_cargo(&tree);

    let success_log = tree.root().join("success.log");
    let success = xtask()
        .arg("--workspace")
        .arg(tree.root())
        .arg("verify")
        .env_remove("XTASK_CARGO")
        .env("CARGO", &fake_cargo)
        .env("FAKE_CARGO_LOG", &success_log)
        .output()
        .expect("run verify");
    assert!(
        success.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&success.stdout),
        text(&success.stderr)
    );
    assert_eq!(
        logged_lines(&success_log),
        vec![
            "fmt --all -- --check",
            "clippy --workspace --all-targets --all-features --locked -- -D warnings",
            "test --workspace --all-features --locked",
            "metadata --format-version 1 --locked --no-deps",
        ]
    );
    let stdout = text(&success.stdout);
    assert!(stdout.contains("verification passed"), "{stdout}");
    assert!(stdout.contains("supplemental"), "{stdout}");

    let failure_log = tree.root().join("failure.log");
    let failure = xtask()
        .arg("--workspace")
        .arg(tree.root())
        .arg("verify")
        .env("CARGO", "this-cargo-must-not-be-selected")
        .env("XTASK_CARGO", &fake_cargo)
        .env("FAKE_CARGO_LOG", &failure_log)
        .env("FAKE_CARGO_FAIL", "clippy")
        .output()
        .expect("run failing verify");
    assert_failure(&failure);
    assert_eq!(failure.status.code(), Some(1));
    assert_eq!(
        logged_lines(&failure_log),
        vec![
            "fmt --all -- --check",
            "clippy --workspace --all-targets --all-features --locked -- -D warnings",
        ]
    );
    let stderr = text(&failure.stderr);
    assert!(stderr.contains("clippy"), "{stderr}");
    assert!(stderr.contains("17"), "{stderr}");
    assert!(stderr.contains("injected cargo failure"), "{stderr}");
}
