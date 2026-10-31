# Paint.R

An original, cross-platform Rust image editor being developed against a pinned Paint.NET 5.1.12 reference environment.

> **Status:** 0.1.0 internal development-preview candidate. The native application and headless automation path are implemented, but this is not a pixel-identical Paint.NET clone, a certified PDN implementation, or a general Paint.NET binary-plugin host. Public distribution remains blocked. See the [release status matrix](docs/RELEASE_STATUS.md) and exact scope in `docs/PARITY_LEDGER.md`.

## Implemented preview slice

- Native `eframe`/`egui` editor with multi-document tabs, dirty markers, per-tab paths,
  close guard, Save/Save As (PNG), Open, and atomic Export.
- Paint.NET-oriented workspace: native macOS menus plus in-window File/Edit/View/Image/
  Layers/Adjustments/Effects/Window/Help menus; compact icon toolbar; two-column Tools
  strip; Tool Options; floating/docked Colors, Layers and History panels; canvas; status
  bar with units and zoom.
- All 19 Paint.NET tools are present in the reference order: Rectangle/Ellipse/Lasso/Magic
  Wand selection, Move Selected Pixels, Move Selection, Zoom, Pan, Paint Bucket, Gradient,
  Paintbrush, Eraser, Pencil, Color Picker, Clone Stamp, Recolor, Text, Line/Curve and
  Shapes. Tool hotkeys and Shift cycles follow the reference metadata.
- Real per-pixel selection masks with Replace/Union/Subtract/Intersect/Xor, Shift/Alt
  modifiers, magic-wand Active Layer/Image sampling, blue tint and marching ants along
  the actual mask boundary.
- Raster tools with live previews: brush hardness/spacing, soft eraser, gradient
  types/repeat, shape Outline/Fill/Both, text rasterization, clone stamp, line caps/dash,
  recolor, flood fill, color picker sampling, and whole-layer/floating moves.
- Image operations: Crop to Selection, Resize, Canvas Size, Rotate 90/180, Flips and
  Flatten. Layer operations: add/delete/duplicate/merge/reorder, rename/properties,
  visibility, opacity, and real Normal/Multiply/Screen/Overlay blending.
- Edit actions: Select All, Deselect, Invert Selection, Erase Selection, Fill Selection,
  Copy/Cut/Paste, Paste Into New Layer/Image; New Image dialog with reference fills.
- Effects/Adjustments: Invert, Grayscale, Sepia, Gaussian/Motion blur, Sharpen, Add Noise,
  Brightness/Contrast, Hue/Saturation, Auto-Level, Levels, Posterize, Black and White and
  Threshold (with previews and undo). Move supports subpixel resampling modes.
- Clipboard Copy/Cut/Paste, Select All/Invert/Erase/Fill and Paste Into actions where
  implemented by the current menus; keyboard shortcuts are documented in Help.
- Grid, rulers, Pixel/Inch/Centimeter units, fit/zoom, and PNG-only document semantics.
- Deterministic headless trace/PNG/snapshot path and macOS native keyboard/dirty-close gate.

The development UI uses a custom dark `egui` theme and original first-party/Lucide vector
icons; it never embeds Paint.NET bitmaps or code. Paint.NET is a behavioral/layout reference
only. The editor is not pixel-identical to Paint.NET and does not load Paint.NET plugins.

Known gaps for 0.1.0: layered documents are not saved (Save writes a flattened PNG);
advanced effects/adjustments (Curves, Exposure, etc.) remain disabled; Move has subpixel
resampling but no scale/rotation handles; Freeform shapes are not implemented; shape
interior for `Both` uses the primary color; OS clipboard and native-menu interaction are not
fully exercised by headless tests; DPI/print/color-profile metadata is absent. See
[UI_PARITY_DIFF.md](docs/reference/UI_PARITY_DIFF.md) and
[release notes](docs/releases/v0.1.0.md) for the current matrix.

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

Packaging always runs the real-binary smoke gate, then reopens the archive and repeats that smoke against the extracted executable before writing its verification receipt. This remains headless, not native-window, evidence. Internal archives are unsigned and unnotarized. `public` packaging is deliberately refused while asset/font rights and required third-party notices remain unresolved.

## License

Paint.R's first-party source code is licensed under the [MIT License](LICENSE). Third-party
components retain their own licenses and notices; the generated notice inventory does not
relicense them. Paint.NET and its assemblies are reference-only and are not distributed with
Paint.R.

## Engineering rules

- The document/session is the single writer; rendering and plugin workers return versioned patches.
- CPU document semantics are authoritative. GPU presentation is not a correctness oracle.
- Every feature starts with failing behavioral tests and ends with independent integration verification.
- No stub implementations, ignored acceptance tests, auto-blessed goldens, or unreported platform exceptions.
- Paint.NET and its assemblies are reference-only and are never product dependencies.

See `docs/ENGINEERING_CONTRACT.md`, `docs/WORK_PACKETS.md`, `docs/PARITY_LEDGER.md`, `crates/pdn3/FORMAT_NOTES.md`, and `crates/pdn3/REFERENCE_FIXTURES.md`.
