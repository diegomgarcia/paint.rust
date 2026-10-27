# Lane E: verification and release engineering

## Commands

From the workspace root, use the repository-pinned toolchain for every Cargo invocation:

```sh
./tools/cargo-pinned.sh run -p xtask --locked -- verify
./tools/cargo-pinned.sh run -p xtask --locked -- scan-stubs
```

The underlying CLI also supports the conventional forms `cargo run -p xtask -- verify` and `cargo run -p xtask -- scan-stubs`. Use `--workspace <PATH>` for an explicit root. Without it, the CLI searches the current directory and its ancestors for a `Cargo.toml` containing `[workspace]`, then tries the workspace compiled into `xtask` as a fallback.

`scan-stubs --help` and `verify --help` print command-specific usage. Exit status is zero for success, one for verification or scan findings, and two for invalid invocation, workspace discovery, or scan-configuration errors.

## Ordered verification gate

`verify` is fail-fast and runs these steps in order:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
3. `cargo test --workspace --all-features --locked`
4. `cargo metadata --format-version 1 --locked --no-deps`
5. The supplemental `scan-stubs` check

Each step is reported with its executable and arguments. Metadata output is captured to avoid dumping JSON on success and is reproduced on failure. A failed Cargo gate prevents later gates from running.

At runtime, the child executable is selected from `XTASK_CARGO` (an explicit automation/test override), then the invoking process's `CARGO`, then `cargo` on `PATH`. This preserves the exact Cargo selected by `cargo run` or `tools/cargo-pinned.sh`; `RUSTC`, `RUSTFMT`, `RUSTUP_TOOLCHAIN`, and the rest of the environment remain inherited. When Cargo has a parent directory, that directory is prepended to the child's `PATH`, ensuring companion tools such as `cargo-clippy` come from the same pinned toolchain rather than an unrelated ambient toolchain.

## Scanner behavior

The scanner deterministically visits Rust (`.rs`) files below the workspace. Directory names `.git`, `target`, and `.worktrees` are excluded wherever encountered. Symlinks are not followed. Markdown and other non-Rust documentation are not scanned. Rust line comments, nested block comments, quoted strings/chars, byte strings, and raw strings are masked before matching so examples in those contexts do not become findings.

| Rule | Active-code construct detected |
| --- | --- |
| `todo` | `todo!` invocations, including whitespace, delimiter, and qualified-path variants |
| `unimplemented` | `unimplemented!` invocations with the same variants |
| `placeholder-success` | Explicit `placeholder_success`, `placeholder_ok`, `fake_success`, `fake_ok`, `stub_success`, or `stub_ok` identifiers, case-insensitively |
| `trivial-assert` | `assert!` or `debug_assert!` whose first argument is the literal `true`, with optional message/trailing comma |
| `ignored-test` | Direct `#[ignore]`, `#![ignore]`, and `#[ignore = ...]` attributes |

Diagnostics include the workspace-relative file, one-based line and column, rule, explanation, and source line. Allowlisted findings are also printed and counted. See [the allowlist format](../../../tools/verify/README.md). The checked-in default allowlist has no active entries.

## Limits and acceptance role

This is a dependency-free lexical scanner, not a Rust parser and not a proof of completion. It does not infer arbitrary placeholder implementations such as every suspicious `Ok(())`, detect conditional `cfg_attr(..., ignore)`, analyze non-Rust source, or prove that a nonempty test exercises behavior. Those cases require code review and real tests. The scan always runs **after** the test gate in `verify` and is supplemental, never a substitute for tests.

The implementation uses only the Rust standard library and platform-neutral `std::process::Command`, filesystem, and path APIs. Test coverage uses self-authored temporary trees, including paths containing spaces and a Rust-compiled fake Cargo executable rather than platform-specific shell scripts. Host validation does not by itself establish the Windows/Linux acceptance gates.
