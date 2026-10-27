# Paint.R

An original, cross-platform Rust image editor being developed against a pinned Paint.NET 5.1.12 reference environment.

> **Status:** 0.1.0 internal development-preview candidate. The native application and headless automation path are implemented, but this is not a pixel-identical Paint.NET clone, a certified PDN implementation, or a general Paint.NET binary-plugin host. Public distribution remains blocked. Exact scope is tracked in `docs/PARITY_LEDGER.md`.

## Implemented preview slice

- A real `eframe`/`egui` native window with an 800×600 opaque-white new document and an actual composed-surface canvas.
- A denser graphite editor shell with first-party runtime-drawn New/Open/Export, Undo/Redo, selection, fill, and Add Layer icons; a primary-color preview with hex and alpha; an interactive HSV color wheel; exact RGB/RGBA hex input; horizontal and vertical active-layer flips; a composed-canvas color picker; an 11-swatch palette; cached per-layer thumbnails; and a bounded Open-dialog folder browser with explicit PNG thumbnails. The canvas has 25%-step zoom controls and fit-to-window. The icons are original geometry, not copied Paint.NET assets; see [icon provenance](docs/ICON_PROVENANCE.md).
- Rectangle selection, chosen-BGRA fill, RGB-only Invert, undo/redo, layer creation/selection, visibility, and opacity through the single-writer `AppSession`.
- Flat PNG open and atomic PNG export. Opening creates one layer; exporting preserves the layered session, history, and modified state. PNG is not lossless layered-document persistence.
- Dirty New, Open, Command-Q, Command-W, File → Close Window/Quit, and native window-close paths require explicit Discard confirmation; Cancel preserves the current session, and exporting does not bypass the close guard.
- Keyboard-only core workflows with real Tab/Shift+Tab focus navigation, Enter/Space activation, arrow-adjusted values, and Escape dialog cancellation. The Keyboard menu documents preview-only Cmd/Ctrl bindings: N New, O Open, Shift+E Export, I Invert, F Fill, R Rectangle selection, Shift+L Add layer, Z Undo, and Shift+Z Redo.
- A deterministic real-binary headless protocol with semantic snapshots, exact output checks, malformed-input failures, and per-file atomic replacement.
- Bounded PNG and data-only PDN3/NRBF libraries. An authentic signed Paint.NET 5.1.12 2×2 Normal/BGRA fixture passes exact reader/composite acceptance. The writer remains experimental and self-round-trip-tested only until stock acceptance is demonstrated.
- An isolated Rust↔.NET source-owned plugin transport proof. This does not load arbitrary Paint.NET plugin DLLs.
- The current source also builds a reproducible x86_64 Linux artifact in an isolated ARM64 Lima VZ/Rosetta VM. It passed 219 Rust tests across 40 suite records, strict Linux Clippy/format, deterministic two-target packaging, and independent extraction smoke. This is emulated userspace evidence, not native x86_64 hardware CI or native Linux window acceptance.

The development UI uses a custom dark `egui` theme, original first-party icon geometry, and a text-path PNG dialog. On macOS the default native menu bar is intentionally disabled so Command-Q and Command-W pass through the document dirty guard; application actions remain in the in-window menu. The UI is not a pixel-parity skin and does not include a native OS file picker or full image-library management. The Open dialog has an app-drawn, bounded folder browser: it lists at most 200 directories and regular `.png` files, skips hidden and symlink entries, and previews one bounded PNG without changing the current session. Keyboard evidence is semantic `egui_kittest` input/focus coverage, not native OS, screen-reader, or Paint.NET shortcut-parity evidence. Use disposable test documents; the preview is not a complete document-safety or Paint.NET workflow implementation.

## Prerequisites

- Rust 1.95.0 with `rustfmt` and `clippy` (provided by `rust-toolchain.toml`).
- Python 3.12 for independent image oracles, end-to-end tests, and packaging.
- .NET SDK 10.0.201 for the isolated plugin-host development lane.
- For reference generation only: the licensed Windows 11 ARM64 UTM VM and Paint.NET 5.1.12 described in `docs/IMPLEMENTATION_PLAN.md`.

## Verify

Use the pinned Cargo wrapper so companion tools cannot drift to another installed toolchain:

```sh
./tools/cargo-pinned.sh run --locked -p xtask -- verify
python3 -m unittest discover -s tests/e2e -p 'test_*.py' -v
python3 -m unittest discover -s tools/package/tests -p 'test_*.py' -v
python3 -m unittest discover -s tools/reference/tests -p 'test_*.py' -v
```

For the plugin development lane:

```sh
dotnet restore plugin-host/Paint.PluginHost.sln --locked-mode
dotnet test plugin-host/Paint.PluginHost.sln --configuration Release --no-restore
plugin-host/test-cross-language.sh
```

The full release gate additionally requires native platform builds, real-binary and native-window smoke tests, deterministic package checks, and the platform/reference evidence described in the parity ledger. A container result for one architecture is not evidence for another.

On macOS, with Accessibility and Screen Recording permission for the calling terminal, the
native keyboard/dirty-close gate is:

```sh
python3 tools/macos_native_usability_gate.py target/release/paint-ui \
  --output-dir "artifacts/reference-local/native-usability-$(date +%Y%m%d-%H%M%S)"
```

The output directory must not exist. See
[MACOS_NATIVE_USABILITY.md](docs/work-packets/release/MACOS_NATIVE_USABILITY.md) for the full
preflight, limitations, and evidence requirements. The current unsigned internal macOS
package was also reproduced deterministically, extracted independently, and its `.app`
executable passed the same live gate; this is still not a signed or public release.

## Run

Native development preview:

```sh
./tools/cargo-pinned.sh run --locked -p paint-ui
```

Exact headless invocation:

```sh
./tools/cargo-pinned.sh run --locked -p paint-ui -- \
  --headless-smoke --trace tests/e2e/traces/invert-800x600.json \
  --output-dir artifacts/preview/headless
```

The direct binary interface is:

```text
paint-ui --headless-smoke --trace <trace.json> --output-dir <directory>
```

A successful headless run writes `composite.png` and `snapshot.json` and prints one JSON object. The files are individually atomic, not a transactional two-file commit. The PNG is flattened.

## Internal packaging

```sh
./tools/cargo-pinned.sh build --release --locked -p paint-ui
python3 tools/package/preview_package.py \
  --binary target/release/paint-ui \
  --platform macos-aarch64 \
  --output-dir artifacts/preview/macos-aarch64 \
  --repeat 2 --cargo-command ./tools/cargo-pinned.sh
```

Packaging always runs the real-binary smoke gate, then reopens the archive and repeats that smoke against the extracted executable before writing its verification receipt. This remains headless, not native-window, evidence. Internal archives are unsigned and unnotarized. `public` packaging is deliberately refused while project licensing, asset/font rights, and required notices remain unresolved.

## Engineering rules

- The document/session is the single writer; rendering and plugin workers return versioned patches.
- CPU document semantics are authoritative. GPU presentation is not a correctness oracle.
- Every feature starts with failing behavioral tests and ends with independent integration verification.
- No stub implementations, ignored acceptance tests, auto-blessed goldens, or unreported platform exceptions.
- Paint.NET and its assemblies are reference-only and are never product dependencies.

See `docs/ENGINEERING_CONTRACT.md`, `docs/WORK_PACKETS.md`, `docs/PARITY_LEDGER.md`, `crates/pdn3/FORMAT_NOTES.md`, and `crates/pdn3/REFERENCE_FIXTURES.md`.
