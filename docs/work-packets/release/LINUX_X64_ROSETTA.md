# Local Linux x64 Rosetta engineering evidence

## Scope

This document records local **x86_64 Linux userspace emulation** on an isolated ARM64
Lima VM using VZ and Rosetta. It is not native Linux x86_64 acceptance, a hosted CI
result, native-window evidence, or public-release approval.

The full Rust/Python gate ran at
`ce90eab02637092fd38ed81d39c2f29593b7fbc7`. Packaging and the .NET gate ran at
`0f6edd2f77fe37f99c0a88cfc849232b66d7d8ea`. The intervening delta was exactly two
Markdown documents and two validation receipts; no Rust, C#, packager, workflow, or
fixture changed.

## Isolated environment

- Lima 2.2.1, VZ, ARM64 Ubuntu 26.04, two virtual CPUs, 4 GiB RAM, and a 20 GiB disk.
- Ubuntu image digest
  `sha256:63a93bd5a8d76e33b15ceb5daa3657bd79be804748051ab178e643b0f5da22e7`.
- Docker server 29.9.2 on ARM64 with an enabled Rosetta x86_64 ELF binfmt handler.
- Pinned amd64 Rust image
  `rust@sha256:f49565f188ee00bc2a18dd418183f2c5f23ef7d6e691890517ed341a598f67c3`.
- Rust 1.95.0 and Python 3.13.5 in the Rust/Python containers.
- Every Docker command used the explicit isolated socket
  `unix://$HOME/.lima/paint-r-x64/sock/docker.sock`.
- Rancher Desktop retained the `rancher-desktop` context and
  `virtualMachine.useRosetta=false`; its VM and Kubernetes workloads were not stopped,
  restarted, or reconfigured. The Windows reference VM was not stopped or modified.

Earlier QEMU attempts faulted inside `rustc -vV`. Those are recorded as environment
failures, not product failures or passes. The successful lane used the separate
Lima/Rosetta environment above.

## Completed checks

| Lane | Result | Receipt |
|---|---|---|
| Rust/Python full gate at `ce90eab` | 205 Rust tests across 40 suite records; 18 end-to-end + 38 package/workflow + 30 reference Python tests; zero Rust failed/ignored and zero Python failures | `artifacts/validation/2026-10-27-linux-x64-rosetta-gate-ce90.json` |
| Rust quality/release | Format, strict Clippy, locked metadata, 78-file stub scan with zero violations/allowlist entries, and release workspace build passed | Same full-gate receipt |
| Workflow lint | Local actionlint 1.7.12 and ShellCheck 0.11.0 found no issues in either workflow | Full-gate receipt log hashes; not hosted Actions |
| .NET at `0f6edd2` | SDK 10.0.201, runtime 10.0.5; 12 passed, zero failed/skipped; build had zero warnings/errors | `artifacts/validation/2026-10-27-linux-x64-dotnet-0f6edd2.json` |
| Live cross-language | Real .NET child passed all 8 checks, including copied BGRA, stale generation, cancellation, timeout, crash, malformed/oversized frames, and wrong protocol version | Same .NET receipt |
| Clean reproduction at `0f6edd2` | Two fresh release targets produced identical binaries; both complete package output trees matched | `artifacts/validation/2026-10-27-linux-x64-rosetta-package-0f6edd2.json` |
| Package acceptance | Pre/post-extraction smoke, independent extraction/smoke, exact payload modes/hashes, sidecar and output-tree verification passed | Same package receipt |

The full-gate source manifest contained 203 tracked files and was byte-identical before
and after. Packaging used a read-only source mount and ended with 205 tracked files and
a clean checkout. The host independently reopened the copied archive and repeated the
path, mode, manifest, binary, ledger, and smoke comparisons.

## .NET supply-chain and source isolation

The SDK came from the official .NET release metadata and SDK download endpoints. The
Linux x64 archive was 242,025,193 bytes and matched SHA-512:

`a33354e3291aa21ea5e1983733f001d45264c31ee3c0dc4a85509d4cb6d4896cfec57fd2143a7b54c93bc42b328e059b16802b4085257a367ef2652f1c8fa424`

The .NET projects came from a fresh `git archive` of the exact source commit. Their
`bin`/`obj` output stayed in a writable temporary copy; the original checkout remained
read-only. No unpinned install script was executed.

## Reproduction and archive results

Two fresh target volumes were used. The successful second attempt built and packaged A,
repeated the process in B, compared binaries and complete output trees, independently
extracted the archive, and ran two traces twice against the extracted executable.

- Archive: `paint-r-0.1.0-linux-x86_64.tar.gz`
- Archive bytes: **9,067,205**
- Archive SHA-256:
  `3d261f5a58c61f6da02557f5fa50e71a41e4ba2be34f7cc2c390d7d73645ea7c`
- ELF x86_64 binary bytes: **22,760,520**
- Binary SHA-256:
  `de1b55cc14a37d6176e14b2d03d8d73650b2fbd0f97eab8726717f6db66787e7`
- Three-file output-tree SHA-256:
  `554dfcd4fd9709b21e87ad417ac1982d63561d85e5dc80baa785d0e6d28b61ae`
- Payload entries: **754**; SHA256SUMS records: **753**, all independently verified.
- `paint-ui` mode: **0755**; all other payload files: **0644**.
- Embedded post-extract smoke matched independent extracted smoke exactly. Two traces
  passed with repeat count two and deterministic decoded RGBA hashes.

The accepted host copies are under `artifacts/preview/final-linux-0f6edd2-a/` and the
second comparison copy is under `artifacts/preview/final-linux-0f6edd2-b/`. These are
local ignored artifacts, not published releases. The sidecar and verification JSON sit
beside each archive.

The first package attempt failed before any Cargo build or archive because a fresh
container lacked `rustfmt`, which `tools/cargo-pinned.sh` resolves for every command.
The receipt preserves that environment failure. The remediation explicitly installed
`rustfmt` and `clippy`, then used a new container and fresh target volumes; no partial
output was reused.

## Evidence boundaries and remaining gates

- The archive is **unsigned, unnotarized, internal 0.1.0 engineering evidence**. No
  installer or public release was produced.
- Headless trace determinism is not native-window usability, broad Paint.NET UI parity,
  stock writer-open, or broad PDN/plugin compatibility evidence.
- Native Linux x86_64 CI/window execution, native Windows x64/MSVC execution, native
  Windows/macOS window and OS-integration usability, and hosted Actions remain open.
- Windows x64 tooling passed its reference tests, but QGA failed before exact-source
  verification or any x64 Cargo/smoke command. The raw writer-open gate stopped before
  UI actions because Paint.NET was not running.
- Public packaging remains blocked by the owner-selected project license, asset/font
  rights, signing/notarization, and legal notice review. A local inventory has 457
  packages: 397 with discovered texts and 60 missing/error entries requiring exact
  upstream review, not invented notices.

The isolated Lima instance was gracefully stopped only after all artifacts and evidence
were copied and independently verified on the host. It is preserved for reuse.
