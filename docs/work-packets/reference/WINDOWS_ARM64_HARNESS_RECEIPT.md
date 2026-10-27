# Windows ARM64 Harness Receipt — Scoped PASS for `5914c4b`; Earlier Failures Retained

## Scope and boundary

This receipt covers black-box reference automation, isolated native Windows ARM64 tool provisioning, exact-source transfer, and bounded Harness/build/test attempts. Exact candidate `5914c4b8cd9155d28baa18617437f271d134e7e5` has a **scoped native Windows ARM64 gnullvm fmt/clippy/workspace-test/xtask/release-build PASS** below. This does **not** claim a complete Windows release/platform gate, Windows x64 coverage, MSVC ABI coverage, writer acceptance, or broad Paint.NET compatibility.

Paint.NET remained analysis-only through visible UI and noninteractive inventory/hash/image commands. No REA, static IL inspection, decompilation, credentials, or secrets were used. PE evidence reads only the machine header. No VM reset, stop, suspend, or reboot command was issued by this harness. Production sources were not edited in this lane.

Reference automation commit: `b4a78a9` (`test: add bounded native reference automation`). Host Python suite: **15/15 passed**. Prior A/A and authentic-PDN evidence remains in their separate receipts; this receipt does not expand those limited observations.

## Native environment actually established

All tools were isolated below `C:\PaintR-Reference`; there was no system-wide Visual Studio installation or persistent PATH modification.

| Tool | Observed result |
|---|---|
| Rust | 1.95.0; native `aarch64-pc-windows-gnullvm` host and target, with clippy/rustfmt; ARM64 MSVC host toolchain also present but not used for final gates |
| LLVM-MinGW | 20261020 UCRT ARM64; native ARM64 linker/archiver |
| Node/npm | Node 24.22.1; npm 11.19.1 |
| .NET | SDK 10.0.201; `dotnet --info` reported `win-arm64` |
| DeepSeek Harness | `@deepseek-ai/dsh` 0.2.1-rc.2 |

The compiler, Cargo, Cargo-clippy launcher, rustfmt, Node, .NET host, and LLVM tools were proven ARM64 (`PE machine 0xAA64`). Native QGA PowerShell inventory remains x64-emulated and was not mislabeled as native ARM64. See [gnullvm-tool-evidence.json](artifacts/reference-local/harness-source/gnullvm-tool-evidence.json), [tool-pe-evidence.json](artifacts/reference-local/harness-source/tool-pe-evidence.json), and [native-tool-versions.json](artifacts/reference-local/harness-source/native-tool-versions.json).

Publisher integrity checks passed for rustup-init, Node, LLVM-MinGW, and the .NET archive. The .NET SDK ZIP was 294,944,999 bytes, above the 256 MiB QGA-push limit, so bounded guest `curl --max-time 240` was used and its official SHA-512 was verified. Other archives were hash-verified QGA pushes. Evidence is under `artifacts/reference-local/harness-source/`, including [tool-transfers.json](artifacts/reference-local/harness-source/tool-transfers.json) and [dotnet-guest-download.json](artifacts/reference-local/harness-source/dotnet-guest-download.json).

An initial MSVC-host baseline Cargo attempt failed because host build scripts required `link.exe`. The official native ARM64 gnullvm host toolchain resolved that prerequisite without Visual Studio. A baseline `paint-core` test on old source `e12d5c80…` then exited 0; that was setup/baseline evidence only and is not a final-candidate result.

## Detached-job evidence and reboot safety

`guest_jobs.py` records command/environment, guest PID/start time, atomic exit JSON, stdout/stderr, timeout, and pulled guest/host log hashes. PowerShell 5.1 redirection writes logs as UTF-16LE. Secret-like environment keys are refused. Two early launch-response losses were recovered without duplicate jobs.

During the first revised-candidate test, the guest rebooted unexpectedly. Its original wrapper PID 768 was subsequently reused by `fontdrvhost.exe`. The host poller was terminated before its deadline could issue a PID-only tree kill. No test count was claimed from that partial run.

Before the single permitted retry, cleanup was changed to require exact executable path and creation-time identity. The new [Stop-ReferenceJob.ps1](tools/reference/guest/Stop-ReferenceJob.ps1) uses no `taskkill`: it verifies the wrapper and descendants and calls `Process.Kill()` only on identity-checked live handles. Polling also requires exact wrapper identity. Preflight then confirmed QGA responsiveness, unchanged boot time, no matching build process, and exact source hashes; see [pre-retry-stability.json](artifacts/reference-local/harness-final-eb2e/pre-retry-stability.json).

## Historical candidate `8c6d0c2ca3813efdce110be81f098dab1f5cdb30`

Exact archive SHA-256: `4ab9294e3f014f27d94bef6ab6f1155526b8ff80e0b6bebd8e0012cb9a7c0585` (410,892 bytes). All **174/174** unique relative paths, lengths, and SHA-256 values matched host/guest. See [source-verification.json](artifacts/reference-local/harness-final/source-verification.json).

Executed natively with host/target `aarch64-pc-windows-gnullvm`:

1. `cargo fmt --all -- --check` — **PASS**, exit 0. Evidence: `artifacts/reference-local/jobs/final8c6d-fmt/result.json`.
2. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — **FAIL**, exit 101 after 65.183 seconds. `crates\image-io\src\atomic.rs:3:21` reported unused `File` in `use std::fs::{self, File, OpenOptions};`; `-D unused-imports` was implied by `-D warnings`.
3. Workspace tests, `cargo run -p xtask --locked -- verify`, and release build were **not run** by the fail-stop sequence.

The raw clippy stderr SHA-256 is `5fbe4ce642ef493d5ee19f16d85e947ae7e23a9382e435ad2a063ba6b15eecc3`; see `artifacts/reference-local/jobs/final8c6d-clippy/stderr.txt`. The complete summary is [rust-gate.json](artifacts/reference-local/harness-final/rust-gate.json).

A corrected post-gate comparison confirmed all 174 source files remained byte-identical. The earlier post-check FAIL was a harness filter bug that incorrectly excluded two legitimate Rust `src/bin` files; it was retained, and [source-post-gate-verification-final.json](artifacts/reference-local/harness-final/source-post-gate-verification-final.json) records the correction and PASS.

**This is a historical failed-lint candidate, not a Windows pass.**

## Revised product-code candidate `eb2eefe0596a176d177f3a80c6801117fa5c024d`

Exact archive SHA-256: `3f0e60066d2a5bd6073217c7fcec58c0f868c007a2d1e8dcf3ea5898636476a7` (412,884 bytes). All **176/176** unique relative paths, lengths, and SHA-256 values matched host/guest. See [source-verification.json](artifacts/reference-local/harness-final-eb2e/source-verification.json).

The archive diff from `8c6d0c2` is recorded in [source-diff-from-8c6d.json](artifacts/reference-local/harness-final-eb2e/source-diff-from-8c6d.json): `atomic.rs` and `PARITY_LEDGER.md` changed, two macOS/Linux validation JSON files were added, and no files were removed. Coordinator-identified successor `ec7440ab…` was docs-only over `eb2eefe`; this lane did not restart or relabel its product-code gate for that successor.

### Format and strict lint

- `cargo fmt --all -- --check` — **PASS**, exit 0, 4.325 seconds. Evidence: `artifacts/reference-local/jobs/finaleb2er2-fmt/result.json` and adjacent logs.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — **PASS**, exit 0, 27.926 seconds. Evidence: `artifacts/reference-local/jobs/finaleb2er2-clippy/result.json` and adjacent logs.

An earlier driver attempt produced no launch, process, or test result after a QGA timeout; this was preserved separately and not counted as a product failure.

### First test attempt — INTERRUPTED, no count

`cargo test --workspace --all-features --locked` started as `finaleb2er2-test` with an 1,800-second bound. The guest's observed `LastBootUpTime` became `2026-10-27T01:29:31.5399410-04:00`; no original wrapper/build process or child exit record survived. Partial stdout must not be counted as a completed run.

The interruption, PID-reuse evidence, and preserved logs are in `artifacts/reference-local/jobs/finaleb2er2-test/`; the consolidated no-count summary is [rust-gate-eb2e-interrupted.json](artifacts/reference-local/harness-final/rust-gate-eb2e-interrupted.json). Xtask and release did not run in that sequence.

### Single permitted retry — product test failure

The one permitted retry used fresh job IDs and the existing cached target. It had no second reboot and no QGA observation error.

Exact command:

```text
cargo test --workspace --all-features --locked
```

Result: **FAIL**, exit 101 after 37.971 seconds. Failures:

- `active_violations_have_precise_relative_diagnostics`, `xtask\tests\scanner.rs:33:5`;
- `allowlist_does_not_leak_to_production_source`, `xtask\tests\scanner.rs:176:5`.

Both assertions received Windows `src\lib.rs` but expected POSIX `src/lib.rs`.

Across the **31 emitted suite result records**, the observed aggregate was **201 passed, 2 failed, 0 ignored**: 30 passing suite records plus the xtask scanner record with 6 passed and 2 failed. This is not a complete workspace/doc-test count because Cargo stopped at `-p xtask --test scanner`. See [test-counts-eb2e.json](artifacts/reference-local/harness-final/test-counts-eb2e.json).

Evidence:

- [rust-gate-eb2e-retry.json](artifacts/reference-local/harness-final/rust-gate-eb2e-retry.json);
- `artifacts/reference-local/jobs/finaleb2er3-test/result.json` and `config.json`;
- `artifacts/reference-local/jobs/finaleb2er3-test/stdout.txt`, SHA-256 `017d3dfb9f9edbe06e35bdbc926166404c5bd34bd27d0d9e80d7531748b24905`;
- `artifacts/reference-local/jobs/finaleb2er3-test/stderr.txt`, SHA-256 `94527b6dfe3a0291bfd0038e37ea470eda274a140cf535616e32434caf12bb7b`.

`cargo run -p xtask --locked -- verify` and the release build were **not run** after this failure. No further retry was made. Consequently there is no final release-binary PE evidence or passing final test count for this candidate.

## Corrected candidate `5914c4b8cd9155d28baa18617437f271d134e7e5` — SCOPED PASS

One fresh bounded gate was run with new source `C:\PaintR-Reference\candidate5914-src`, new target `C:\PaintR-Reference\target-5914-gnullvm`, and fresh `final5914-*` job IDs. There were no automatic retries, installations, resets, or source edits. Preflight and post-gate boot identity remained `2026-10-27T01:29:31.5399410-04:00`; no build process was active before launch.

Exact archive SHA-256: `68802c1d81cc69bccf743a9baa71031cef388bb9ff37c5bcf12d2547dd25ff75` (413,849 bytes). All **178/178** unique relative paths, lengths, and SHA-256 values matched host/guest before the gate and remained identical after every step. See [source-verification.json](artifacts/reference-local/harness-final-5914/source-verification.json), [pre-gate-stability.json](artifacts/reference-local/harness-final-5914/pre-gate-stability.json), and [post-gate-verification.json](artifacts/reference-local/harness-final-5914/post-gate-verification.json).

The diff from `eb2eefe` contains `xtask/tests/scanner.rs`, `docs/PARITY_LEDGER.md`, and two added macOS/Linux validation JSON artifacts; no shipped Rust product source changed in that archive diff. See [source-diff-from-eb2e.json](artifacts/reference-local/harness-final-5914/source-diff-from-eb2e.json).

### Exact gate results

| Step | Result | Seconds | stdout SHA-256 | stderr SHA-256 |
|---|---:|---:|---|---|
| `cargo fmt --all -- --check` | exit 0 | 6.823 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | exit 0 | 37.428 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` | `199619337dc062cc3c9bc68a116ab501ba19c3874f1900869743fd5b182650ec` |
| `cargo test --workspace --all-features --locked` | exit 0 | 142.137 | `a9bb4d01acaf05fec44c7ce8f22a511f4d2646b5eca60b82316e13f89483ca8d` | `20a381947f800c35bb31658c81e5fae9a13ea4fc3d415029c6b66288369b05b9` |
| `cargo run -p xtask --locked -- verify` | exit 0 | 8.354 | `5599479d52a11c27eea6d9097704046d93a6de7dad3b1cebcaeebfd8cddd543d` | `ff30213879b4d20c30e92c06a52b242db36cb51900fc2f6c20d2b8f361dd6308` |
| `cargo build -p paint-ui --release --locked --target aarch64-pc-windows-gnullvm` | exit 0 | 121.706 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` | `e9d66e4f365ca79b2b319b4f9185d1d156153b86fd8a5c921264963086ac48a3` |

Every command and environment is in `artifacts/reference-local/jobs/final5914-*/config.json`; exits and log metadata are in adjacent `result.json` files. The driver summary is [rust-gate-5914.json](artifacts/reference-local/harness-final/rust-gate-5914.json), SHA-256 `3b667fa5e238b5e6cca97610656a7da8b55c929a4cf7dab6c69d982531238ad0`.

### Complete test count and xtask verification

The single completed direct workspace command emitted **40 suite result records: 203 passed, 0 failed, 0 ignored, 0 measured, 0 filtered out**. This is the full count for that command. No result from the earlier interrupted/failed attempts and no duplicate xtask test execution was added. See [test-counts-5914.json](artifacts/reference-local/harness-final/test-counts-5914.json).

Xtask verified four Cargo steps plus the supplemental stub scan: **70 Rust files scanned, 0 violations, 0 allowlisted**. Its output ends `verification passed: 4 Cargo steps and a supplemental stub scan`.

One transient `command exceeded 45s: osascript` status-poll error occurred during `final5914-test`. The same identity-checked job continued to an atomic exit-0 record; no gate command was rerun. This polling recovery is included in the consolidated manifest rather than hidden.

### Release artifact

Guest binary: `C:\PaintR-Reference\target-5914-gnullvm\aarch64-pc-windows-gnullvm\release\paint-ui.exe`.

- PE machine: `0xAA64` (**ARM64**);
- length: **14,940,160 bytes**;
- SHA-256: `2c8148e58946b6d8da1ab67eed4d954316618798ce44ba41be445f29a7e5cfa4`.

See [release-pe-evidence.json](artifacts/reference-local/harness-final-5914/release-pe-evidence.json). This is a native ARM64 gnullvm release-build result, not an x64 binary, MSVC ABI claim, package validation, native-window launch, or headless-smoke result.

The consolidated machine-readable handoff is [final-validation.json](artifacts/reference-local/harness-final-5914/final-validation.json), status `PASS_SCOPED_NATIVE_WINDOWS_ARM64_GNULLVM`.

## Harness CLI / delegation

The isolated npm install completed with exit 0 and installed 612 packages. npm reported five install scripts not covered by its allowScripts policy; no DSH script approval or reinstall was performed. `dsh --version`, `dsh --help`, and `dsh headless --help` exited 0 under native ARM64 Node and confirmed version 0.2.1-rc.2.

An isolated no-tool headless invocation exited 1 with `MISSING_CREDENTIAL` for provider route `deepseek-official`. No API key was copied from the host or written into an environment manifest. Therefore native Harness delegation did not occur. Exact events and diagnostics are in `artifacts/reference-local/jobs/dsh-headless-noauth-01/result.json`, `stdout.txt`, and `stderr.txt`; version/help jobs are in `artifacts/reference-local/jobs/dsh-version-01/`, `dsh-help-01/`, and `dsh-headless-help-01/`.

## Unexecuted gates and writer blocker

- Final .NET restore/build/test, eight cross-language checks, and headless `paint-ui` smoke were **not run** on `8c6d0c2`, `eb2eefe`, or the directed `5914c4b` gate. Only native .NET SDK inventory/version was established.
- Windows x64 was not attempted and remains a distinct required platform gate.
- No native window/UI product claim is made from QGA/session-0 commands.
- Stock writer-open remains without an acceptance/rejection result. `writer-raw-01` failed during environment/UI setup before File → Open because interactive host automation was unavailable; the gzip variant was not run. No unlock was attempted. Existing untouched input SHA-256 values remain `fb45b7e03d58dd480b1abbc567d6428b2723154e4533bb44475bf5890e5c399a` (raw) and `80e1d49045e77c9dc239a2b5c6b262f017d0ea7b3660561a90e519295bc9b36f` (gzip). Self-round-trip was never treated as writer acceptance.

## Handoff verdict

**SCOPED PASS for exact `5914c4b8cd9155d28baa18617437f271d134e7e5` on native Windows ARM64 gnullvm:** format, strict clippy, the complete 203-test workspace command, xtask verification, and the ARM64 release build are green with pre/post source identity and release PE/hash evidence. Earlier `8c6d0c2` and `eb2eefe` failures and the no-count reboot interruption remain preserved as history. Windows x64, .NET/cross-language, headless smoke, native-window behavior, packaging, Harness delegation, and writer-open acceptance remain separate unproven lanes; this scoped result must not be expanded into those claims.
