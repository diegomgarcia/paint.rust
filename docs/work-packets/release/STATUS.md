# Release-engineering lane status — PARTIAL

The packaging/automation lane is implemented and locally tested. This file preserves the
original lane's red-to-green receipt; its library-only statements and historical test counts
are not current integration status. A real `paint-ui` binary, native session UI, deterministic
headless smoke, internal archive, and extracted-package smoke now exist. Current demonstrated
scope and remaining native three-OS gates are tracked in `README.md` and
`docs/PARITY_LEDGER.md`. No completion or public-release claim is made.

## Implemented behavior and exact scope

- Python-standard-library deterministic packaging for Windows x86_64 ZIP, macOS ARM64
  `.app` in ZIP, and Linux x86_64 tar.gz.
- Sorted entries, fixed timestamps/gzip metadata, normalized modes and tar ownership,
  payload `SHA256SUMS.txt`, and outer archive `.sha256` sidecars.
- Mandatory invocation of the real `tests/e2e/preview_smoke.py` before production packaging;
  missing/wrong binary, missing inputs, failed/missing evidence, and public mode fail. The
  staged binary hash must match the smoke evidence.
- Embedded internal-preview README, an unmodified copy of the current parity ledger, and
  normalized smoke evidence. No placeholder or skip-smoke route exists.
- CycloneDX 1.6 SBOM and resolved dependency edges from locked Cargo metadata, plus actual
  discovered third-party license/notice texts and an explicit gap inventory.
- Manual three-OS workflow with pinned Rust/.NET, locked build/tests, Rust and .NET
  verification, cross-language conformance, mandatory smoke/package, and artifact upload
  only. It never creates a GitHub release or publishes a package.

## Owned files changed

- `.github/workflows/release.yml`
- `packaging/INTERNAL_PREVIEW.md`
- `tools/package/.gitignore`
- `tools/package/README.md`
- `tools/package/preview_package.py`
- `tools/package/tests/test_preview_package.py`
- `tools/package/tests/test_release_workflow.py`
- `docs/work-packets/release/DEVELOPMENT_PREVIEW.md`
- `docs/work-packets/release/STATUS.md`

No root Cargo file, lockfile, existing CI file, production crate, plugin-host/VM file, or
parity ledger was edited by this lane. Current `main` was merged without conflicts; the
lane-versus-main diff contains only the owned paths above.

## Red-to-green evidence

Before implementation:

```text
python3 -m unittest discover -s tools/package/tests -p 'test_*.py' -v
```

failed with `ModuleNotFoundError: No module named 'preview_package'` and a missing
`.github/workflows/release.yml` (`FileNotFoundError`). After the first implementation all
17 tests passed. Adding missing-report and relocated-checkout SBOM checks produced the
18-test suite. After merging `main`, a new workflow-gate assertion first failed because
`scan-stubs` was absent; adding the stub, locked .NET, and conformance gates returned the
suite to green.

## Commands run and observed results after merging main

- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/package/tests -p 'test_*.py' -v`
  — **18 passed, 0 failed**. Covers archive byte determinism, metadata, manifests,
  SBOM/notices, layouts, smoke failures, public refusal, and workflow policy.
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/e2e -p 'test_*.py' -v`
  — **14 passed, 0 failed** independent PNG oracle tests; these are not product smoke.
- `./tools/cargo-pinned.sh run -p xtask --locked -- verify` — exit 0: formatting, strict
  all-feature Clippy, locked workspace tests, locked metadata, and the supplemental stub
  scan all passed. **120 Rust tests passed, 0 failed, 0 ignored**; 44 Rust files scanned
  with 0 violations and 0 allowlisted findings.
- `dotnet restore plugin-host/Paint.PluginHost.sln --locked-mode && dotnet test
  plugin-host/Paint.PluginHost.sln --configuration Release --no-restore` — **12 .NET tests
  passed, 0 failed, 0 skipped**.
- `plugin-host/test-cross-language.sh` — **8 live Rust-to-.NET conformance checks passed,
  0 failed, 0 skipped**.
- Real `load_locked_metadata` / SBOM / notice integration check using the pinned wrapper —
  466 SBOM components; 457 third-party packages; 745 discovered notice files for 397
  packages. Extraction is **incomplete for 60 packages** and is marked accordingly.
  The egui `OFL.txt`, `UFL.txt`, Hack notice, egui-icons notice, and emoji-font MIT license
  were found, as was `unicode-ident`'s `LICENSE-UNICODE`.
- Real CLI with absent `target/release/paint-ui` — expected exit 1, `binary is missing`, and
  no output directory/archive. This was a failure-path check, not a product smoke.
- Python `compile(...)` syntax checks for the tool and tests — passed without writing caches.
- `ruby -e 'require "yaml"; YAML.parse_file(".github/workflows/release.yml")'` — YAML syntax
  passed. `actionlint` and PyYAML were unavailable locally; GitHub runner execution remains.
- `python3 tools/package/preview_package.py --help` — passed; no `--skip-smoke` option.

Before the merge, the same packaging suite and a zero-test Rust foundation baseline passed.
A host PATH conflict initially selected Clippy 0.1.94; explicitly selecting the 1.95 binary
directory made Clippy pass. The merged `cargo-pinned.sh` now handles that PATH selection,
and the final integrated `xtask verify` used the pinned 1.95 tools directly.

## Test counts by requested category

- Owned Python unit/policy tests: **18 passed**.
- Additional independent PNG unit/property tests: **14 passed**.
- Integrated Rust unit/property/integration/doc tests: **120 passed; 0 failed/ignored**.
- Integrated .NET tests: **12 passed**; live cross-language checks: **8 passed**.
- Real locked-metadata integration assertion harness: **passed** (counts above).
- Real product smoke/usability/package tests: **not run at the original library-only lane handoff**. Subsequent integrated evidence is recorded in the parity ledger; it does not satisfy every native platform gate.
- Three-OS workflow runs: **not run locally; manual workflow not dispatched**.

## Dependency and license additions

No Python package, Rust dependency, root manifest, lockfile, or product license was added or
changed by this lane. The tool uses only the Python standard library. The workflow uses
official `actions/checkout@v4`, `actions/setup-python@v5`, `actions/setup-dotnet@v4`, and
`actions/upload-artifact@v4` only. No public license, signing identity, or notarization
claim was invented.

## Known gaps and remaining gates

- The real executable and session/UI are now integrated. Rebuild and repeat real trace,
  usability, native-window, and package smoke on the final candidate and every required
  native binary; existing macOS evidence does not satisfy Windows or Linux gates.
- Execute and review the manual workflow on Windows x64, macOS ARM64, and Linux x86_64;
  actionlint/GitHub schema validation and native artifact/desktop smoke remain CI gates.
- Complete reference, usability, PDN, and other parity-ledger gates as applicable.
  Packaging does not change any parity-ledger status.
- Resolve public project/asset/font licensing and perform legal notice review. The 60-package
  figure above is the original lane's historical result; current exact coverage and gap counts
  live in the latest notice-validation receipts and `docs/PARITY_LEDGER.md`. Non-Cargo/native
  assets also need audit. Keep public packaging refused while coverage or obligations remain
  unverified.
- The SBOM is a locked-workspace superset, not an exact linked-binary/native dependency SBOM.
- Installers, AppImage/AppDir runtime, distro/desktop integration, signing, notarization,
  updater, provenance/attestation, and store distribution remain separate scoped work.

The early implementation, main merge, and final integrated lane commit hashes are supplied
in the agent completion report.
