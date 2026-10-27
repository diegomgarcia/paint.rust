# Paint.NET reference automation (local evidence only)

This directory contains the lane-D host/UTM/QGA harness. It does not load Paint.NET assemblies into product code, inspect IL, decompile, or perform REA. Paint.NET is operated as a black box through its visible UI. Guest PowerShell is noninteractive and is used only for inventory, hashes, file presence/transfer, and independent image decoding. Do not introduce scheduled-task interactive probes.

## Prerequisites and pinned UI assumptions

- macOS host with UTM 4.7.5 and a running VM named `Windows`; the observed VM is Windows 11 Pro ARM64 with Paint.NET 5.1.12 open in interactive session 1.
- UTM QEMU guest agent is healthy. Accessibility and Screen Recording are granted to the host automation client.
- `/usr/bin/swiftc`, `/usr/bin/osascript`, `/usr/sbin/screencapture`, Python 3, Tesseract, and FFmpeg are available.
- The Paint.NET UI automation requires a stable **1024×808** UTM `Windows` window and the observed Documents save location. It checks these conditions and OCR gates rather than clicking through an unknown layout.
- Filename generation uses unambiguous lowercase ASCII. Text entry uses a US keyboard mapping. Menu actions do not rely on the unreliable modifier-shortcut path.
- Guest paths currently use `C:\Users\CodeFreak\Documents`. Change the explicit constant for a differently provisioned VM; do not silently substitute another user profile.

Keep the VM running. The harness never resets, stops, suspends, or snapshots it. Do not run another Paint.NET UI agent concurrently.

## Build and test

Run from the worktree root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/host_ui.py build
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/reference/tests -v
```

The Swift binary and module caches are written only below `artifacts/reference-local`. The Python suite covers known RGBA decoding, packed palette/transparency, PNG filters, malformed input, LOGFONT decoding, reviewed Paint.NET identity preflight and forged-evidence rejection, A/A mismatch rejection, bounded PDN filename OCR matching, stock-writer accept/reject ambiguity and stale-title refusal, safe guest paths, secret-like environment rejection, and native gate command construction.

## Durable command building blocks

UTM configuration (read-only):

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/utm.py inventory
```

Ordinary QGA execution, with a bounded poll and host timeout:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/utm.py guest-exec -- \
  'C:\Windows\System32\cmd.exe' /c echo hello
```

Run a guest PowerShell script through bounded UTF-16 `-EncodedCommand` input:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/utm.py guest-script \
  tools/reference/guest/Get-PngEvidence.ps1 \
  --set 'Path=C:\Users\CodeFreak\Documents\paint-r-reference-invert.png' \
  --timeout 60
```

Pull a guest file to a **new** host path:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/utm.py pull \
  'C:\Users\CodeFreak\Documents\paint-r-reference-invert.png' \
  artifacts/reference-local/new-pull/invert.png
```

The destination directory must already exist. UTM AppleScript requires a host file object before `pull`; the helper performs its overwrite check, creates a zero-length placeholder, and removes that placeholder if transfer fails. It verifies a nonempty result. Use separate guest and host hashes to prove content.

Push a host file to a **new** guest path (1 byte through 256 MiB), with guest/host length and SHA-256 verification:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/utm.py push \
  artifacts/reference-local/source.tar.gz \
  'C:\PaintR-Reference\downloads\source.tar.gz'
```

The push helper refuses an existing guest destination. Larger downloads must use a separately bounded guest download with an independent publisher hash check rather than weakening this limit.

Host window operations:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/host_ui.py focus
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/host_ui.py list
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/host_ui.py capture \
  artifacts/reference-local/new-capture/window.png
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/host_ui.py key escape
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/host_ui.py type 'ascii-text.png'
```

`capture` waits for stable window bounds, uses the current CGWindow ID, and writes a PNG plus OCR text. `click` accepts host-global coordinates and should normally be used only through `paintnet_ui.py`, which computes positions from the pinned window origin.

Decode a host PNG without external Python packages:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/png_evidence.py image.png
```

The decoder validates chunk CRCs and allocation bounds and supports non-interlaced PNG color types 0/2/3/4/6. It emits encoded SHA-256, dimensions, row-major RGBA8 SHA-256, first pixel, and a bounded histogram. Interlaced input fails explicitly. The formal runner additionally requires FFmpeg's independent raw RGBA bytes to match exactly.

## Environment inventory

Create the new output directory first; existing files are never overwritten:

```sh
mkdir -p artifacts/reference-local/environment-new
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/collect_environment.py \
  --output artifacts/reference-local/environment-new/environment.json
```

The JSON contains host/UTM versions and display configuration, guest OS/build/hotfixes, native-versus-QGA process architecture, Paint.NET executable version/signature/hash, CIM display data, per-user DPI/text/theme/color/locale/keyboard registry values, decoded LOGFONT structures, selected font and color-profile hashes, source-tool hashes, and an explicit `unpinned_fields` list. It intentionally does not collect product keys, license identifiers, user name/SID, recent-file databases, clipboard contents, or hardware serials. CIM architecture identifies native ARM64; the QGA PowerShell runtime is separately identified as x64-emulated.

### Reviewed executable identity preflight

`paintnet_identity.py` is the shared preflight for all three observation runners and both independent validators. It requires the reviewed Paint.NET 5.1.12 executable evidence: file version `5.112.9563.32325`, product version `5.112.9563.32325`, signature status `Valid`, and executable SHA-256 `17b27aee8eb0d18e7b39964e6c1c43b1dac5705f23727af550f2cde7fe737e2e`. These values come from the authentic passing run inventories, not from display-title text. Any mismatch stops observation before UI automation and fails validation even when a manifest and its collected evidence are internally consistent.

Manifest display/title version is decoded from the observed file-version fields (`5.112…` represents display `5.1.12`), not assigned from a separate constant. The independent validators also recompute the referenced `environment.json` SHA-256 before applying the preflight. The PDN validator compares every manifest product identity field with the validated observation; the Invert validator checks every recorded window-title version against it.

## Three-run Invert A/A observation

Use a new run ID and keep the VM exclusively assigned to this harness:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/observe_invert.py \
  --run-id invert-aa-YYYYMMDDTHHMMSSZ --runs 3
```

For each run the tool:

1. Requires a previously nonexistent guest output.
2. Normalizes inherited menu/modal state, then opens File → New and accepts the observed default 800×600 document.
3. Verifies `Untitled` and `Background`, then selects the visible Adjustments → Invert Colors item.
4. Uses File → Save As, verifies the exact filename and PNG type, and accepts `Auto-detect` in Save Configuration.
5. Waits for a nonempty, stable guest file; captures the saved window; records guest encoded hash and System.Drawing-derived RGBA hash.
6. Pulls the file, rechecks guest stability, and compares host encoded hash, stdlib RGBA decoding, and independent FFmpeg RGBA bytes.
7. Writes per-run observation JSON and stops on a mismatch. A failure JSON and any completed observations are retained; it does not print a three-run pass.

Validate the completed manifest in a separate process:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/validate_observations.py \
  artifacts/reference-local/runs/invert-aa-YYYYMMDDTHHMMSSZ/manifest.json
```

Validation requires at least three sequential observations, per-run guest/host encoded and RGBA agreement, 800×600 dimensions, 1,920,000 RGBA bytes, and exactly 480,000 opaque-black pixels. Cross-run decoded RGBA identity is required. Encoded-byte identity is measured and reported but is not treated as a general compatibility requirement.

## Authentic tiny PDN fixture

This optional workflow creates one 2×2 document, applies Invert Colors to its default Background layer, saves a real Paint.NET PDN, closes it, reopens it through File → Open, and exports a PNG **after that reopen**. It never parses or modifies the PDN container and never overwrites an existing output:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/observe_pdn_fixture.py \
  --run-id pdn-fixture-YYYYMMDDTHHMMSSZ
```

The runner requires the visible format dropdown to show both PDN and PNG. It captures that popup without reactivating UTM (`capture_window(..., focus_before=False)`), selects the format with ordinary keys, and verifies the selected `Save as type` line with Tesseract layout mode 6. A one-edit allowance exists only for the captured Open filename OCR; the deterministic typed action, exact guest path, close/reopen sequence, and artifact checks remain mandatory.

Independently validate a passing run:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/validate_pdn_fixture.py \
  artifacts/reference-local/runs/pdn-fixture-YYYYMMDDTHHMMSSZ/manifest.json \
  --output artifacts/reference-local/runs/pdn-fixture-YYYYMMDDTHHMMSSZ/validation.json
```

The manifest records exact product/build/signature/source hash, canvas and bottom-to-top layer order, name, visibility/background flag, Normal blend mode, opacity 255, per-layer straight-alpha BGRA hash, UI actions, environment, and guest/host stability. Those attributes are the stock New-document defaults and are labeled as not separately queried through Layer Properties. For this sole visible full-canvas layer, BGRA is derived by an R/B swap of the independently decoded post-reopen composite. The validator recomputes all artifact/UI/environment hashes, required action ordering, exact pixels, and a fresh FFmpeg decode.

`PDN3` magic is only a file-format sanity check, not an independent PDN parse. This single all-black 2×2 fixture is not the full semantic corpus and does not establish writer acceptance or clone compatibility.

## Stock writer-open acceptance (blocked unless interactive UI is available)

For an already transferred, untouched clone-written input whose guest hash matches the host input, use a new run ID and the expected SHA-256 explicitly:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/observe_writer_acceptance.py \
  --run-id writer-raw-01 \
  --host-input '/path/to/clone-normal-2x2-raw.pdn' \
  --expected-sha256 '64-lowercase-hex-characters' \
  --variant raw
```

The runner does not convert the input or accept a clone self-round-trip as evidence. It requires stock File → Open UI proof, rejects ambiguous accept/error OCR and stale same-file titles, rechecks input hashes, and—only after a proven open—exports and independently decodes the stock post-open PNG. `REJECTED_BY_STOCK` is a valid observed outcome; `INDETERMINATE_OR_AUTOMATION_FAILURE` is not a rejection claim. Never unlock, reset, or suspend the VM to make this gate available. A locked host/console blocks the UI workflow before File → Open.

## Detached native guest jobs and Windows ARM64 gates

Foreground QGA execution is capped at 300 seconds. For downloads/builds/tests, `guest_jobs.py` starts a unique detached job, records its wrapper PID, captures stdout/stderr and an atomic exit JSON, polls within a deadline, kills only that job tree on timeout, and pulls hash-verified logs. PowerShell 5.1 redirection writes those logs as UTF-16LE. Secret-like environment keys are rejected rather than recorded.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/guest_jobs.py \
  --job-id job-smoke-01 --timeout 30 --workdir 'C:\PaintR-Reference' -- \
  'C:\PaintR-Reference\tools\node\node.exe' --version
```

`run_windows_arm64_gate.py` runs selected commands against an exact archived source using an isolated native ARM64 gnullvm toolchain and output outside the source. It does not install tools. The source commit, guest source root, target directory, and unique job prefix are required; no historical candidate defaults are inherited. The available steps are `fmt`, `clippy`, `test`, `xtask`, `release`, `dotnet-restore`, `dotnet-build`, `dotnet-test`, `conformance`, and `smoke`; a failed selected step stops that sequence and later steps require an explicit independent selection.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/run_windows_arm64_gate.py \
  --source-commit 5914c4b8cd9155d28baa18617437f271d134e7e5 \
  --root 'C:\PaintR-Reference\candidate5914-src' \
  --target-dir 'C:\PaintR-Reference\target-5914-gnullvm' \
  --prefix final5914 \
  --steps fmt,clippy,test,xtask,release \
  --summary rust-gate-5914.json
```

Every result is labeled native Windows ARM64 gnullvm, not Windows x64 and not MSVC ABI coverage. Source verification must compare all unique relative paths, lengths, and hashes before and after generated outputs. The source-hash helper excludes Cargo target and .NET `plugin-host` bin/obj output only; it must not exclude legitimate Rust `src/bin` sources. `Get-PeEvidence.ps1` reads only the PE machine header for architecture evidence; it does not disassemble or inspect IL.

## Bounds, failures, and evidence handling

- CLI failures are nonzero (normally exit 2); successful observations/validation return 0. AppleScript polls and every host subprocess have timeouts. QGA timeouts are configurable from 1–300 seconds.
- Captured QGA output is bounded to 8 MiB. Encoded PowerShell is limited to 32,000 characters, below the Windows 32,767-character full-command-line limit after fixed options.
- PNG encoded input is limited to 64 MiB and 16 million pixels. Unsupported/interlaced or malformed data fails rather than returning a placeholder.
- Output directories and JSON files must be new. Screenshots, OCR, manifests, and failed attempts remain available for audit.
- `artifacts/reference-local` is ignored for new local artifacts. **Do not publish proprietary Paint.NET UI screenshots or local evidence as release assets.** These tools do not bless goldens, update the parity ledger, or establish clone parity. A real clone binary must be compared separately.

The pinned environment is an observation, not a claim that every DPI/theme/font/color/display field is controlled. Read the run's `environment.json`, especially `unpinned_fields`, before reusing evidence.
