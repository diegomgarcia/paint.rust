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
Rust-to-.NET conformance, and the independent PNG tests. It then builds only the actual
`paint-ui` release package and calls the packager. The packager must run the real trace
smoke successfully before creating its archive. `actions/upload-artifact` uploads `dist/`
with a seven-day retention period and `if-no-files-found: error`; no GitHub release is
created.

The only third-party actions are the official checkout, Python setup, .NET setup, and
artifact-upload actions. Cross-compilation is not used as evidence for another
OS/architecture lane.

## Artifact acceptance checklist

1. Confirm the workflow run was manually dispatched from the intended commit and all three
   native jobs passed without ignored acceptance tests.
2. Download artifacts only through the authorized internal CI workflow. Verify each outer
   `.sha256` sidecar before extraction.
3. Inspect `README-PREVIEW.md` and `PARITY_LEDGER.md`; do not broaden their claims.
4. Verify extracted `SHA256SUMS.txt`, then compare `PREVIEW_SMOKE.json`'s binary hash and
   semantic results with the binary in the payload.
5. Review `third-party-notices/index.json` for extraction gaps and errors. The SBOM is a
   locked-workspace inventory, not proof of exact binary contents or redistribution
   compliance.
6. On macOS, confirm `Paint.R.app/Contents/MacOS/paint-ui` remains executable after ZIP
   extraction. On Linux, confirm the tar executable mode. Windows requires no POSIX mode.
7. Exercise the actual desktop UI on each native platform separately from the headless
   trace smoke. Record any platform exception rather than silently accepting it.

## Remaining release gates

### Product and evidence gates

- Integrate a real `paint-ui` binary target and the complete session/UI behavior. A library-
  only `paint-ui` crate cannot pass this packaging lane.
- Run the real `preview_smoke.py` against every native artifact and have the integrator
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
- Review action-pinning, artifact access/retention, provenance/attestation, and vulnerability
  scanning requirements before a public release process is approved.

The first scope intentionally stops at deterministic ZIP/tar.gz archives, `.app` bundling on
macOS, locked Cargo SBOM/notices, SHA-256 evidence, and non-publishing manual automation.
