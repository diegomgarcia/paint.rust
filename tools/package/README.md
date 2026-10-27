# Development-preview packaging tool

`preview_package.py` is a Python-standard-library packager for the **future real
`paint-ui` release binary**. It creates internal engineering artifacts only. It does not
create a GitHub release, publish packages, select a license, or sign/notarize a product.

## Mandatory product gate

Every production invocation must pass the repository's real black-box smoke before an
output directory or archive is created:

```text
python3 tests/e2e/preview_smoke.py \
  --binary <actual-paint-ui> \
  --trace tests/e2e/traces/invert-800x600.json \
  --trace tests/e2e/traces/undo-invert-2x2.json \
  --repeat 2 --timeout 30 --report <temporary-report>
```

The packager invokes that runner itself (with every sorted `tests/e2e/traces/*.json` by
default). A missing or wrongly named binary, missing runner/trace, nonzero exit, timeout,
missing report, non-pass report, incomplete repeat evidence, or missing negative-path
evidence aborts packaging. There is no skip-smoke option and no placeholder-binary path.
The normalized `PREVIEW_SMOKE.json` is derived from the real report and omits absolute
paths and wall-clock data. Every run must have the exact dimension/hash/snapshot schema,
three lowercase SHA-256 values, valid layer/history metadata, and identical repeated
evidence. Missing, malformed, extra, or nondeterministic entries abort packaging. Raw
rejection diagnostics are not presented as product output.

The expected executable names are `paint-ui.exe` for Windows and `paint-ui` for macOS and
Linux. Before smoke, the packager inspects the exact PE x86_64, thin ARM64 Mach-O, or ELF
x86_64 machine header required by the selected platform. It re-inspects the staged binary
and requires its complete identity and SHA-256 to match both the original and smoke
evidence. `PREVIEW_BINARY.json` records this executable-header evidence. It does not prove
native/non-emulated execution, runner architecture, signing, or application behavior.

After archiving, the tool verifies the outer sidecar, exact entry set, safe regular-file
paths, archived bytes and modes, tar ownership/timestamps, and the recomputed internal
manifest. It extracts into an isolated temporary directory with explicit mode restoration,
requires a non-Windows executable to remain executable, re-inspects its machine identity,
and repeats the real smoke. Pre/post normalized evidence must be identical. Only then are
the archive, sidecar, and `<archive>.verification.json` success receipt moved into the
output directory. Any failure leaves no new success outputs. This is still headless evidence,
not native-window, signing, notarization, or Paint.NET parity acceptance.

## Build and package

Use the pinned Cargo wrapper. Equivalent local commands from the repository root are:

```sh
bash ./tools/cargo-pinned.sh build --release --locked --package paint-ui

python3 tools/package/preview_package.py \
  --binary target/release/paint-ui \
  --platform linux-x86_64 \
  --output-dir dist \
  --cargo-command bash ./tools/cargo-pinned.sh
```

Use `--binary target/release/paint-ui.exe --platform windows-x86_64` on Windows and
`--binary target/release/paint-ui --platform macos-aarch64` on macOS ARM64. Keep
`--cargo-command` last because it accepts a command plus arguments. Add `--trace` one or
more times to select repository traces explicitly; omitting it selects the full trace
directory. `--repeat` defaults to 2 and `--timeout` to 30 seconds per product invocation.

`--distribution internal-preview` is the default and only accepted value. `public` is
refused before smoke or packaging because public project/asset licensing is unresolved
and notice extraction is not legal verification.

## Archive layouts

| Platform | Artifact | Binary location | Scope limitation |
|---|---|---|---|
| Windows x86_64 | `paint-r-<version>-windows-x86_64.zip` | `paint-ui.exe` at payload root | ZIP only; no installer or signature |
| macOS ARM64 | `paint-r-<version>-macos-aarch64.zip` | `Paint.R.app/Contents/MacOS/paint-ui` | Unsigned/unnotarized app; no universal-binary claim |
| Linux x86_64 | `paint-r-<version>-linux-x86_64.tar.gz` | `paint-ui` at payload root | No AppImage/AppDir runtime, distro package, or desktop integration |

Every archive has one top-level `paint-r-<version>-<platform>/` directory. The common
payload includes:

- `README-PREVIEW.md` with internal-distribution and platform limitations;
- the unmodified `PARITY_LEDGER.md` disclosure;
- `PREVIEW_SMOKE.json` from the mandatory real smoke;
- `PREVIEW_BINARY.json` with executable format, machine, size, and hash evidence;
- `sbom.cdx.json` from locked Cargo metadata;
- `THIRD_PARTY_NOTICES.md`, `third-party-notices/index.json`, and discovered source
  license/notice texts under `third-party-notices/files/`;
- `SHA256SUMS.txt` covering every other payload file.

The macOS ZIP also places the summary README, parity ledger, smoke evidence, notice
summary, binary evidence, and SBOM in `Paint.R.app/Contents/Resources/Preview/`. A `<archive>.sha256`
sidecar and a deterministic `<archive>.verification.json` post-extraction receipt are
written next to every accepted archive. The receipt contains only archive/binary hashes,
entry counts, platform architecture, and headless verification status—no local paths or
wall-clock data. No installer, signing identity, notarization ticket, or update mechanism
is fabricated.

## Determinism and verification

- Entry paths are sorted POSIX names; host absolute paths are excluded.
- ZIP and tar entries use `SOURCE_DATE_EPOCH` (default `946684800`, 2000-01-01 UTC).
- The workflow pins that epoch explicitly. ZIP timestamps and gzip `mtime` are fixed.
- Tar ownership is `0:0` with empty names. Regular-file modes are normalized to `0644`
  and executables to `0755`; ZIP uses Unix modes and a fixed creator system.
- JSON is UTF-8 with sorted keys and stable indentation. Manifests are sorted and exclude
  the manifest itself. The gzip header has no source archive filename.
- Unit tests compare complete archive bytes across different creation orders and mtimes.
- The complete package integration test also compares archive, sidecar, and verification
  receipt bytes and exercises extracted-smoke, sidecar, manifest, and mode failures.
- Ordinary PR/push CI runs this same verified release-package gate on all three OS lanes
  without uploading. Manual internal artifacts are source-qualified with `github.sha`.

Given identical payload bytes and epoch, these rules produce repeatable archives in the
tested environment. Reproducing product bytes additionally requires identical Rust
build inputs and toolchain behavior; package determinism does not make different product
builds identical.

Both workflows go further: they build `paint-ui` into fresh `repro-a` and `repro-b`
`CARGO_TARGET_DIR` directories, compare the real executables, package each binary with the
full pre/post-extraction gate, and compare every archive/sidecar/verification byte. The
manual workflow uploads only the first tree after both comparisons pass. The comparator
can also be run locally:

```sh
python3 tools/package/verify_reproduction.py repro-a/release/paint-ui repro-b/release/paint-ui
python3 tools/package/verify_reproduction.py dist-a dist-b
```

It rejects content differences, missing/extra files, empty trees, and symbolic-link
outputs. Its JSON contains relative paths and hashes, not host paths or timestamps. This
proves byte equality only for those two builds on one checkout/runner; it is not cross-OS
identity, a frozen SDK/runner image, or proof of reproduction across time.

From the directory containing an archive and sidecar, use `sha256sum -c <sidecar>` (or
`shasum -a 256 -c <sidecar>` on macOS). From an extracted payload root, the internal
manifest can be checked the same way with `SHA256SUMS.txt`.

## SBOM and third-party notices

The tool runs the configured Cargo command as:

```text
<cargo-command> metadata --locked --format-version 1
```

It emits CycloneDX 1.6 with deterministic serial numbers, package URLs, declared license
expressions, and resolved dependency edges. Local checkout paths are not copied into the
SBOM. This is the **locked workspace graph**, including target/feature dependencies that
may not be linked into the delivered binary; it is not an exact native/link/asset audit.

The notice collector copies conventional license, licence, copying, notice, copyright,
OFL/UFL, and font-directory text files from locally resolved third-party Cargo sources.
The index includes source-relative paths and hashes and lists packages with no discovered
text. Discovery is heuristic, not a legal review. Cargo license expressions remain metadata;
they are never substituted for license or notice texts.

For registry packages whose crate archives omit those files, the optional checked-in
`notice_supplement/index.json` associates curated upstream bytes with an exact package name,
version, registry source, `Cargo.lock` checksum, declared repository, `.cargo_vcs_info.json`
commit, and package path in that commit. Each resource has a safe local path, source-relative
path, full-commit HTTPS source URL, and SHA-256. The loader rejects malformed identities,
moving or source-path-mismatched URLs, unsafe paths, missing files, and hash mismatches. Collection performs
no network access, uses validated in-memory bytes, and verifies every emitted file's hash
again. Package-source texts take precedence; a failed identity match produces no supplemented
text and leaves the package incomplete.

The current curated bundle is deliberately limited to 20 packages: the eight AccessKit package
versions; the three listed egui-family 0.37.0 packages (`eframe`, `egui_kittest`, and
`egui_system_font_provider`); these six Android-related packages; and the three additional packages
in the second table:

| Package | Declared Cargo repository | Exact `.cargo_vcs_info` commit | Package path in that commit |
|---|---|---|---|
| `jni` 0.22.4 | `https://github.com/jni-rs/jni-rs` | `5ae9458a4ec44c5318f37ddc7569c1d4ae8a69e7` | `crates/jni` |
| `jni-macros` 0.22.4 | `https://github.com/jni-rs/jni-rs` | `33045a124105c939d1e2cbdcb5a39e5d868ffa03` | `crates/jni-macros` |
| `jni-sys-macros` 0.4.1 | `https://github.com/jni-rs/jni-sys` | `64d77b7a5f119d7b55b4e2c169a4668067ff59e6` | `jni-sys-macros` |
| `ndk` 0.9.0 | `https://github.com/rust-mobile/ndk` | `49bbbba16c58ff63cb8a0ad0eca5a9fb7ecaec25` | `ndk` |
| `ndk-context` 0.1.1 | `https://github.com/rust-windowing/android-ndk-rs` | `10f2ba388fca20f7349996ebae26ccda7a6fda5c` | `ndk-context` |
| `ndk-sys` 0.6.0+11769913 | `https://github.com/rust-mobile/ndk` | `49bbbba16c58ff63cb8a0ad0eca5a9fb7ecaec25` | `ndk-sys` |

| Package | Declared Cargo repository | Exact `.cargo_vcs_info` commit | Package path | Exact root resources |
|---|---|---|---|---|
| `block2` 0.5.1 | `https://github.com/madsmtm/objc2` | `4fc083f1c6d6784577e38b0ee8dbd344481e2fd2` | `crates/block2` | `LICENSE.txt` |
| `block2` 0.6.2 | `https://github.com/madsmtm/objc2` | `b4167b582b2f75f9a1be75495c41b765344fd03c` | `crates/block2` | `LICENSE.md` |
| `profiling` 1.0.18 | `https://github.com/aclysma/profiling` | `8271551172eb6fa4cba47369aedd93790c623df9` | `profiling` | `LICENSE-APACHE`, `LICENSE-MIT` |

All 17 pre-existing bindings are unchanged. Every curated resource was retrieved separately from
the exact package-specific full-commit URL recorded in the manifest and hash-verified before being
checked in. For the six Android-related packages and three additional packages, curation also
matched real locked Cargo metadata and registry sources, verified each cached `.crate` archive
SHA-256 against `Cargo.lock`, and matched the archived and extracted `.cargo_vcs_info.json` commit
and package path. Exact-commit Git trees established every root resource path before retrieval;
the newly retrieved bytes also matched the corresponding exact-tree Git blob IDs. No package
subdirectory, moving branch, latest revision, release tag, SPDX expression, or other commit supplied
substitute text.

The requested `dispatch` 0.2.0 and `r-efi` 5.3.0/6.0.0 archives and extracted sources passed those
same lock-checksum and archived/extracted VCS checks but remain uncovered. Real Cargo metadata
declares the `dispatch` repository as `http://github.com/SSheldon/rust-dispatch`, which fails closed
against the loader's exact HTTPS-repository requirement; it was not rewritten to HTTPS. Neither
exact `r-efi` commit (`97b55bed1c2c91dcbf787674849f05337ff80b33` and
`7e1b0322d31d625f81a5656096330934f9cd835d`) has a root license/notice resource, so no other path
or revision supplied substitute text.

The seven other locked egui-family 0.37.0 packages remain uncovered because exact commit-pinned
upstream license content for their recorded VCS commit `7c0b40402271e8d40cda69f87cf3c2617c2e4382`
was unavailable during curation; no moving-branch, latest, or release-tag content was substituted.
Other package gaps remain explicit. Curated retrieval and file coverage are not legal review or a
claim of redistribution compliance. Incomplete extraction is disclosed in internal artifacts;
public packaging remains refused. No first-party license is invented.

## Tests

```sh
python3 -m unittest discover -s tools/package/tests -p 'test_*.py' -v
```

Tests use clearly named temporary, disposable non-product fixtures. They cover ZIP and
tar.gz byte determinism, timestamps/modes/order, exact SHA-256 manifests and sidecars,
SBOM path independence, font notice extraction and gap reporting, offline curated-notice
identity/hash/provenance checks, the checked-in AccessKit, scoped egui, Android, block2, and profiling bundles, platform layouts,
PE/Mach-O/ELF architecture acceptance and rejection, exact smoke-run validation,
public-release refusal, and mandatory-smoke failure handling. A complete `package_release`
integration test additionally compares two deterministic archives, sidecars, binary
evidence, and post-extraction receipts while doubling only the external smoke/metadata
commands. Separate failure cases prove rejected archives are not moved into the output.
Reproduction-comparator tests cover identical files/trees and content, entry, empty-tree,
and input-kind failures. These fixtures do not
claim a real product smoke or native execution. Only a native build of the actual
`paint-ui` binary can satisfy that product gate.
