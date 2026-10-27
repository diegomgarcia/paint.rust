# Internal development-preview release runbook

## Scope and trust boundary

This lane supplies packaging and manual CI automation for a verified **0.1.0 development
preview**, not public Paint.NET parity. Archives are internal CI artifacts. The project
owner has not selected a public license, and the parity ledger records public asset/font
rights as blocked pending review. The packager rejects `--distribution public`; the
workflow has read-only repository permission and contains no release/publication step.

A generated archive is evidence only of the checks actually executed for that binary. It
must not be described as signed, notarized, reference-accepted, pixel-identical, PDN
writer-accepted, or compatible with arbitrary Paint.NET plugins.

## Native matrix

`.github/workflows/release.yml` runs only through `workflow_dispatch`:

| Runner | Package platform | Architecture represented |
|---|---|---|
| `windows-2022` | `windows-x86_64` | Windows x64 |
| `macos-14` | `macos-aarch64` | macOS ARM64 |
| `ubuntu-24.04` | `linux-x86_64` | Linux x86_64 |

Each lane installs Rust 1.95.0, Python 3.12, and .NET 10.0.201, uses
`tools/cargo-pinned.sh`, fetches with `--locked`, runs packaging tests, formatting, strict
all-feature Clippy, all workspace tests, the stub scan, locked .NET tests, real
Rust-to-.NET conformance, independent PNG tests, and reference-harness unit tests (not VM
execution). It then builds the actual `paint-ui` release package into two fresh target
directories and requires identical executable bytes. The packager runs real trace smoke
before archiving, then reopens each archive, verifies its exact payload/modes/hashes, and
repeats smoke on the extracted executable. Both complete archive/sidecar/verification
trees must be identical. `actions/upload-artifact` uploads only the first `dist-a/` tree
with a source-qualified name, seven-day retention, and `if-no-files-found: error`; no
GitHub release is created. Ordinary PR/push CI runs the same reproduction/package gate
without uploading.

The only third-party actions are the official checkout, Python setup, .NET setup, and
artifact-upload actions. Cross-compilation is not used as evidence for another
OS/architecture lane.

### Local emulated Linux x64 supplement

At `ce90eab`, an isolated Lima 2.2.1 VZ VM with Rosetta/binfmt ran the pinned amd64 Rust
image and passed 205 Rust tests, 86 Python tests, strict Clippy/format, locked metadata,
a 78-file stub scan, and a release workspace build. All 203 tracked source files matched
before and after. This is x86_64 userspace emulation on an ARM64 Linux VM, not native
Linux x86_64, hosted CI, .NET/cross-language coverage, or native-window evidence. The
separate Rancher Desktop VM and its active Kubernetes workloads were not reconfigured or
restarted. Actionlint 1.7.12 and ShellCheck 0.11.0 also reported no findings for the two
workflow files locally; this is not a hosted Actions result. See
`artifacts/validation/2026-10-27-linux-x64-rosetta-gate-ce90.json`. Any clean-rebuild or
package acceptance is recorded separately with exact binary/archive hashes and remains
scoped to emulation.

The completed follow-up at `0f6edd2` used two fresh release targets, produced identical
ELF binaries and complete output trees, and passed independent extracted-package smoke.
The Linux archive SHA-256 is
`3d261f5a58c61f6da02557f5fa50e71a41e4ba2be34f7cc2c390d7d73645ea7c`.
A separate .NET SDK 10.0.201 lane passed 12/12 tests with zero skips and all eight live
cross-language checks. See `LINUX_X64_ROSETTA.md` and
`artifacts/validation/2026-10-27-linux-x64-rosetta-package-0f6edd2.json`. These remain
unsigned, emulated, headless engineering results, not native OS/window or public-release
acceptance.

## Artifact acceptance checklist

1. Confirm the workflow run was manually dispatched from the intended commit and all three
   native jobs passed without ignored acceptance tests.
2. Download artifacts only through the authorized internal CI workflow. Review the clean
   binary/output-tree comparison log, verify each outer `.sha256` sidecar, and require the
   adjacent `.verification.json` receipt to report post-extraction success for those bytes.
3. Inspect `README-PREVIEW.md` and `PARITY_LEDGER.md`; do not broaden their claims.
4. Verify extracted `SHA256SUMS.txt`, then compare `PREVIEW_BINARY.json` and
   `PREVIEW_SMOKE.json` hashes and semantic results with the binary in the payload.
5. Review `third-party-notices/index.json` for extraction gaps and errors. The SBOM is a
   locked-workspace inventory, not proof of exact binary contents or redistribution
   compliance.
6. On macOS, confirm `Paint.R.app/Contents/MacOS/paint-ui` remains executable after ZIP
   extraction. On Linux, confirm the tar executable mode. Windows requires no POSIX mode.
7. Exercise the actual desktop UI on each native platform separately from the headless
   trace smoke. Record any platform exception rather than silently accepting it.

## Remaining release gates

### Product and evidence gates

- The real `paint-ui` binary and session/UI are integrated. Keep the mandatory executable
  and smoke requirements; do not regress to a library-only or substitute fixture artifact.
- Run the real `preview_smoke.py` against every final native artifact and have the integrator
  independently review the reports, traces, screenshots/usability evidence, and hashes.
- Pass the strict workspace gate and all integrated workflows on all three primary OS lanes.
- Complete the reference differential, PDN, plugin, malformed-input, crash, and failed-save
  gates required by the engineering contract and parity ledger. Do not mark parity entries
  from packaging evidence.

### Distribution and legal gates

- The owner must select the public distribution model and project license.
- Resolve asset/font redistribution rights and complete a reviewed third-party/native
  dependency notice audit. Heuristic Cargo-source extraction is not sufficient where texts
  are missing or where non-Cargo assets are present.
- If full notice extraction and obligations cannot be verified, keep public packaging
  blocked. Internal CI artifacts must not be promoted to public downloads by renaming them.

### Platform/distribution hardening (explicitly not implemented)

- Decide whether installers, AppDir/AppImage, distro packages, desktop integration, or an
  updater are needed; implement and test them as separate scoped work.
- Decide on Windows signing and macOS signing/notarization only after identities, policies,
  and credentials are available. The current artifacts make no such claims.
- Validate native runtime dependencies and actual desktop launch/usability on each target.
- Review artifact access/retention, provenance/attestation, and vulnerability scanning
  requirements before a public release process is approved. Keep the existing immutable
  action pins under review; they are not a complete provenance or runtime-security claim.

The first scope intentionally stops at deterministic ZIP/tar.gz archives, `.app` bundling on
macOS, locked Cargo SBOM/notices, SHA-256 evidence, post-extraction headless acceptance,
same-runner clean rebuild comparisons, and non-publishing manual automation.
