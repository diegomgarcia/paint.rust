# Release-engineering lane status — PARTIAL

The packaging/automation lane is implemented and locally tested, but the packet is
**PARTIAL**: this branch has only a `paint-ui` library target, no actual `paint-ui` binary.
Therefore no real product trace smoke, product archive, desktop package smoke, or native
three-OS workflow run has been performed. No completion or public-release claim is made.

## Implemented behavior and exact scope

- Python-standard-library deterministic packaging for Windows x86_64 ZIP, macOS ARM64
  `.app` in ZIP, and Linux x86_64 tar.gz.
- Sorted entries, fixed timestamps/gzip metadata, normalized modes and tar ownership,
  payload `SHA256SUMS.txt`, and outer archive `.sha256` sidecars.
- Mandatory invocation of the real `tests/e2e/preview_smoke.py` before production packaging;
  missing/wrong binary, missing inputs, failed/missing evidence, and public mode fail.
- Embedded internal-preview README, an unmodified copy of the parity ledger, and normalized
  smoke evidence. No placeholder or skip-smoke route exists.
- CycloneDX 1.6 SBOM and resolved dependency edges from locked Cargo metadata, plus actual
  discovered third-party license/notice texts and an explicit gap inventory.
- Manual three-OS workflow with pinned Rust, locked build/tests, mandatory smoke/package,
  and artifact upload only. It never creates a GitHub release or publishes a package.

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
parity ledger was edited.

## Red-to-green evidence

Before implementation:

```text
python3 -m unittest discover -s tools/package/tests -p 'test_*.py' -v
```

failed with `ModuleNotFoundError: No module named 'preview_package'` and a missing
`.github/workflows/release.yml` (`FileNotFoundError`). After the first implementation all
17 tests passed. After adding a missing-report failure test and a relocated-checkout SBOM
determinism check, the final suite passed all 18 tests.

## Commands run and observed results

- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/package/tests -p 'test_*.py' -v`
  — **18 passed, 0 failed**. Covers archive byte determinism, metadata, manifests,
  SBOM/notices, layouts, smoke failures, public refusal, and workflow policy.
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/e2e -p 'test_*.py' -v`
  — **12 passed, 0 failed** independent PNG oracle tests; these are not product smoke.
- `./tools/cargo-pinned.sh test --workspace --all-features --locked` — exit 0; this foundation
  branch currently contains **0 Rust unit/doc tests**, so this is a compile baseline only.
- `./tools/cargo-pinned.sh fmt --all -- --check` — exit 0.
- Strict Clippy with the pinned toolchain binary directory prepended to `PATH`:
  `./tools/cargo-pinned.sh clippy --workspace --all-targets --locked -- -D warnings` —
  exit 0 with Clippy 0.1.95. The first invocation picked the host's stable Clippy 0.1.94
  and failed rust-version checks; diagnostics identified the PATH conflict. No source or
  wrapper change was made for this host issue.
- Real `load_locked_metadata` / SBOM / notice integration check using the pinned wrapper —
  459 SBOM components; 450 third-party packages; 730 discovered notice files for 390
  packages. Extraction is **incomplete for 60 packages** and is marked accordingly.
  The egui `OFL.txt`, `UFL.txt`, Hack notice, egui-icons notice, and emoji-font MIT license
  were found, as was `unicode-ident`'s `LICENSE-UNICODE`.
- Real CLI with absent `target/release/paint-ui` — expected exit 1, `binary is missing`, and
  no output directory/archive. This was a failure-path check, not a product smoke.
- Python `compile(...)` syntax checks for the tool and tests — passed without writing caches.
- `ruby -e 'require "yaml"; YAML.parse_file(".github/workflows/release.yml")'` — YAML syntax
  passed. `actionlint` and PyYAML were unavailable locally; GitHub runner execution remains.
- `python3 tools/package/preview_package.py --help` — passed; no `--skip-smoke` option.

## Test counts by requested category

- Owned Python unit/policy tests: **18 passed**.
- Additional independent PNG unit tests: **12 passed**.
- Real locked-metadata integration assertion harness: **passed** (counts above).
- Rust unit/doc tests on the current foundation: **0 present; 0 failed**.
- Real product smoke/integration/usability/package tests: **not run; binary absent**.
- Three-OS workflow runs: **not run locally; manual workflow not dispatched**.

## Dependency and license additions

No Python package, Rust dependency, root manifest, lockfile, or product license was added or
changed. The tool uses only the Python standard library. The new workflow uses official
`actions/checkout@v4`, `actions/setup-python@v5`, and `actions/upload-artifact@v4` only.
No public license, signing identity, or notarization claim was invented.

## Known gaps and remaining gates

- Integrate the real executable and session/UI; rerun the real trace smoke and package each
  native binary. The current zero-test Rust baseline is not development-preview acceptance.
- Execute and review the manual workflow on Windows x64, macOS ARM64, and Linux x86_64;
  actionlint/GitHub schema validation and native artifact/desktop smoke remain CI gates.
- Complete reference, usability, malformed-input/crash/failed-save, PDN, and plugin gates as
  applicable. Packaging does not change any parity-ledger status.
- Resolve public project/asset/font licensing and perform legal notice review. Sixty Cargo
  packages currently lack discovered notice texts; non-Cargo/native assets also need audit.
  Keep public packaging refused while coverage or obligations remain unverified.
- The SBOM is a locked-workspace superset, not an exact linked-binary/native dependency SBOM.
- Installers, AppImage/AppDir runtime, distro/desktop integration, signing, notarization,
  updater, provenance/attestation, and store distribution remain separate scoped work.

The final lane commit hash is supplied in the agent completion report after committing.
