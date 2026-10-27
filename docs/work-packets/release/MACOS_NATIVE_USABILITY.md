# macOS native keyboard and dirty-close acceptance

## Scope and current state

`tools/macos_native_usability_gate.py` and its Swift/AppKit helper exercise the real
`paint-ui` window through native Accessibility, Core Graphics keyboard events, and window
screenshots. This is separate from the semantic `egui_kittest` usability tests and from
headless trace smoke.

The orchestration and Swift compilation are verified. The current locked-host run proved
only the fail-closed preflight: `BLOCKED`, `native_execution=NOT_RUN`, exit status 75, and
no product launch or native input. A real native keyboard/window **PASS has not been
observed**. Do not describe this tool, its fake tests, or a successful build as native UI
acceptance or Paint.NET parity.

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
4. AXPress the exact root window’s `AXCloseButton` and require the exact dirty-close
   title/explanation, enabled Discard, and enabled **focused Cancel** while the modified
   session remains open.
5. Send Escape and require the dialog to disappear without losing the modified document.
   The complete accessible text set must equal the pre-close modified observation, not merely
   retain the required Modified/invert/canvas labels.
6. AXPress that close button again, confirm the same protections, then explicitly AXPress
   Discard and require process exit status zero.

Command shortcuts are not used for the acceptance close. Live runs confirmed that pinned
eframe/winit on macOS sends Command-Q directly to `Event::LoopExiting`, bypassing the app’s
`CloseRequested` guard, while Command-W was ignored. Those native shortcut behaviors remain
separate platform limitations; failure cleanup may still use Command-Q.

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
