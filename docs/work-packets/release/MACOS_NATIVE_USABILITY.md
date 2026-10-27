# macOS native keyboard and dirty-close acceptance

## Scope and current state

`tools/macos_native_usability_gate.py` and its Swift/AppKit helper exercise the real
`paint-ui` window through native Accessibility, Core Graphics keyboard events, and window
screenshots. This is separate from the semantic `egui_kittest` usability tests and from
headless trace smoke.

A live run on 2026-10-27 at product source `bbb6bea97b0855d69f5bb504cf405cf25f374e81` and
native-gate source `3471a735756df3251572d0b5071fc3feec19f513` produced `PASS`,
`native_execution=COMPLETED`, process exit status 0, ten passing assertions, and five captured
PNGs. It covered Command-Q for the first dirty close and the native `AXCloseButton` for the
second. Independent decoding confirmed all RGBA hashes and the expected transitions:
before != modified, modified == canceled, and first confirmation == second confirmation.
The evidence is recorded in
[`2026-10-27-png-browser-bbb6bea.json`](../../../artifacts/validation/2026-10-27-png-browser-bbb6bea.json).
This proves only this macOS native keyboard/window slice on the recorded host; it is not
Paint.NET parity or a public-release claim. A separately reproduced internal package from
source `de30ca9e7663266cc3455efe173d802e641d23df` was independently extracted, its recorded
executable mode restored, and the packaged `.app` passed the same full live gate. See
[`2026-10-27-packaged-app-de30ca9.json`](../../../artifacts/validation/2026-10-27-packaged-app-de30ca9.json).
Earlier locked-host runs remain fail-closed `BLOCKED`/`NOT_RUN` evidence.

## Prerequisites

Run from a current source checkout on macOS only after deliberately unlocking the console
session and authorizing this exact native-input run.

- Build the current executable with
  `bash ./tools/cargo-pinned.sh build --release --locked --package paint-ui`.
- Pre-grant Accessibility and Screen Recording permissions to the executing terminal or
  responsible application. The helper uses read-only preflight APIs and does not request
  permissions, bypass TCC, or unlock the screen.
- Use an active, unlocked console session. Unknown session/lock state, a locked screen, or
  missing permissions blocks the run. Preflight occurs before launch and before native
  input or capture.
- Choose a new evidence directory. Existing directories are never reused.

The helper is compiled with the host Swift compiler inside the evidence directory. Its
Clang/Swift module caches and temporary directory also remain under that directory.

## Non-native regression checks

```sh
PYTHONDONTWRITEBYTECODE=1 \
  python3 -m unittest discover -s tools/tests -p 'test_*.py' -v
swiftc -typecheck -swift-version 5 tools/macos_native_usability_gate.swift
```

Native transport uses a deterministic fake backend; process-wrapper tests also launch real,
short-lived Python children. They cover lock/permission failures, process exit and timeout
failures, exact-PID cleanup, dirty-close cancellation, focused Cancel, unchanged accessible
document text after Cancel, explicit Discard, exact-limit output, combined stdout/stderr
overflow, early termination of a continuing oversized stream, binary/harness source identities,
and pixel-evidence failures. They never launch Paint.R or post native input.

## Live acceptance command

```sh
python3 tools/macos_native_usability_gate.py \
  target/release/paint-ui \
  --output-dir "artifacts/reference-local/native-usability-$(date +%Y%m%d-%H%M%S)"
```

Optional `--startup-timeout`, `--observation-timeout`, `--quit-timeout`,
`--cleanup-timeout`, `--command-timeout`, `--overall-timeout`, and
`--confirmation-hold` values have enforced upper bounds. The default overall deadline is
120 seconds and its maximum is 180 seconds. Helper stdout and stderr are consumed concurrently
with a combined retained-output cap. Crossing that cap terminates the exact spawned process
immediately instead of buffering the complete stream; timeout cleanup also kills and reaps
that process without a shell.

Exit statuses are:

- `0`: every native observation, input, decoded-pixel comparison, and clean exit passed;
- `75`: environment or permission preflight blocked the run, or the session became blocked;
- `1`: a launched run or native assertion failed;
- `66`: command-line, executable, configuration, or evidence-directory setup failed.

A blocked result before launch has no product PID and `native_execution=NOT_RUN`. A blocked
or failed result after launch is `INCOMPLETE`, never a pass. Cleanup targets only the
spawned process, attempts graceful native close first, and uses TERM then KILL only when
necessary.

## Required native sequence

1. Observe one on-screen main window owned by the spawned PID, with the exact development
   title, clean document state, and composed-canvas Accessibility label.
2. Capture the window, send Command-I through `CGEvent`, and require Modified state, the
   exact invert status, and the same native window.
3. Capture again and require different decoded RGBA pixels.
4. Send Command-Q and require the exact dirty-close title/explanation, enabled Discard,
   and enabled **focused Cancel** while the modified session remains open.
5. Send Escape and require the dialog to disappear without losing the modified document.
   The complete accessible text set must equal the pre-close modified observation, not merely
   retain the required Modified/invert/canvas labels.
6. AXPress the exact root window’s `AXCloseButton`, confirm the same protections, then
   explicitly AXPress Discard and require process exit status zero.

The native default menu is disabled so Command-Q reaches the app instead of terminating
directly through `Event::LoopExiting`; the app routes it through the same dirty guard.
The acceptance covers both Command-Q and the native window close button. Command-W remains
outside the verified shortcut surface.

## Evidence and pixel oracle

A run writes `manifest.json`, `events.jsonl`, timestamped observations, up to five PNG
window captures, and the spawned process log. Every observation and screenshot is hashed.
Screenshot records contain both the exact PNG SHA-256 and `rgba_sha256`, the SHA-256 of the
independently decoded RGBA8 pixel buffer. The manifest's `harness` object records
`capture_stage=gate_initialization` plus the source-relative path, byte length, and SHA-256 of
`tools/macos_native_usability_gate.py`, `tools/macos_native_usability_gate.swift`, and
`tests/e2e/png_rgba.py`. These identities describe the files read during initialization; they
do not prove a clean checkout or convert fake/blocked evidence into a native pass.

The gate lazily loads the repository's bounded `tests/e2e/png_rgba.py` oracle. It rejects
malformed or unsupported PNGs and enforces encoded-size, dimension, pixel, inflation, and
RGBA-length bounds. Before/after and confirmation/canceled acceptance compares decoded
RGBA hashes, not merely PNG file hashes. Valid metadata-only or compression-only changes
therefore cannot establish a visual change.

The evidence still requires human review for layout quality, focus behavior not covered by
the assertions, and Paint.NET-specific visual or interaction parity. No public-release,
signing, notarization, or broad compatibility decision is implied by this gate alone.
