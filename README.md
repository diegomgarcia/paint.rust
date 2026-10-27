# Paint.R

An original, cross-platform Rust image editor being developed against a pinned Paint.NET 5.1.12 reference environment.

> **Status:** 0.1.0 internal development-preview candidate. The native application and headless automation path are implemented, but this is not a pixel-identical Paint.NET clone, a certified PDN implementation, or a general Paint.NET binary-plugin host. Public distribution remains blocked. Exact scope is tracked in `docs/PARITY_LEDGER.md`.

## Implemented preview slice

- A real `eframe`/`egui` native window with an 800×600 opaque-white new document and an actual composed-surface canvas.
- Rectangle selection, chosen-BGRA fill, RGB-only Invert, undo/redo, layer creation/selection, visibility, and opacity through the single-writer `AppSession`.
- Flat PNG open and atomic PNG export. Opening creates one layer; exporting preserves the layered session, history, and modified state. PNG is not lossless layered-document persistence.
- Dirty New, Open, and native viewport-close paths require explicit Discard confirmation; Cancel preserves the current session, and exporting does not bypass the close guard.
- A deterministic real-binary headless protocol with semantic snapshots, exact output checks, malformed-input failures, and per-file atomic replacement.
- Bounded PNG and data-only PDN3/NRBF libraries. An authentic signed Paint.NET 5.1.12 2×2 Normal/BGRA fixture passes exact reader/composite acceptance. The writer remains experimental and self-round-trip-tested only until stock acceptance is demonstrated.
- An isolated Rust↔.NET source-owned plugin transport proof. This does not load arbitrary Paint.NET plugin DLLs.

The development UI uses stock `egui` styling and a text-path PNG dialog, not a pixel-parity skin or native OS file picker. Use disposable test documents; the preview is not a complete document-safety or Paint.NET workflow implementation.

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

On macOS, with accessibility permission for the calling terminal, the reusable native-window gate is:

```sh
tools/macos_native_window_smoke.sh target/debug/paint-ui
```

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

Packaging always runs the real-binary smoke gate. Internal archives are unsigned and unnotarized. `public` packaging is deliberately refused while project licensing, asset/font rights, and required notices remain unresolved.

## Engineering rules

- The document/session is the single writer; rendering and plugin workers return versioned patches.
- CPU document semantics are authoritative. GPU presentation is not a correctness oracle.
- Every feature starts with failing behavioral tests and ends with independent integration verification.
- No stub implementations, ignored acceptance tests, auto-blessed goldens, or unreported platform exceptions.
- Paint.NET and its assemblies are reference-only and are never product dependencies.

See `docs/ENGINEERING_CONTRACT.md`, `docs/WORK_PACKETS.md`, `docs/PARITY_LEDGER.md`, `crates/pdn3/FORMAT_NOTES.md`, and `crates/pdn3/REFERENCE_FIXTURES.md`.
