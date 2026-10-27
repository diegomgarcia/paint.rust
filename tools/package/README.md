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

## Build and package

Use the pinned Cargo wrapper. On all three workflow runners, including Windows, the
release workflow runs these commands from the repository root in Bash:

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
sidecar is written next to every archive. No installer, signing identity, notarization
ticket, or update mechanism is fabricated.

## Determinism and verification

- Entry paths are sorted POSIX names; host absolute paths are excluded.
- ZIP and tar entries use `SOURCE_DATE_EPOCH` (default `946684800`, 2000-01-01 UTC).
- The workflow pins that epoch explicitly. ZIP timestamps and gzip `mtime` are fixed.
- Tar ownership is `0:0` with empty names. Regular-file modes are normalized to `0644`
  and executables to `0755`; ZIP uses Unix modes and a fixed creator system.
- JSON is UTF-8 with sorted keys and stable indentation. Manifests are sorted and exclude
  the manifest itself. The gzip header has no source archive filename.
- Unit tests compare complete archive bytes across different creation orders and mtimes.

Given identical payload bytes and epoch, these rules produce repeatable archives in the
tested environment. Reproducing product bytes additionally requires identical Rust
build inputs and toolchain behavior; package determinism does not make different product
builds identical.

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
text. Discovery is heuristic, not a legal review. Incomplete extraction is disclosed in
internal artifacts; public packaging remains refused. No first-party license is invented.

## Tests

```sh
python3 -m unittest discover -s tools/package/tests -p 'test_*.py' -v
```

Tests use clearly named temporary, disposable non-product fixtures. They cover ZIP and
tar.gz byte determinism, timestamps/modes/order, exact SHA-256 manifests and sidecars,
SBOM path independence, font notice extraction and gap reporting, platform layouts,
PE/Mach-O/ELF architecture acceptance and rejection, exact smoke-run validation,
public-release refusal, and mandatory-smoke failure handling. A complete `package_release`
integration test additionally compares two deterministic archives and their binary
evidence while doubling only the external smoke/metadata commands. These fixtures do not
claim a real product smoke or native execution. Only a native build of the actual
`paint-ui` binary can satisfy that product gate.
