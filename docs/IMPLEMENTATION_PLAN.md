# Paint.R — cross-platform Paint.NET 5.1.12 parity plan

**Revision:** 2, 2026-10-26  
**Status:** planning plus initial reference-VM verification; no clone code or parity comparisons have been executed  
**Supersedes:** the earlier macOS-first, non-identical-UI, deferred-compatibility plan  
**Reference target:** Paint.NET **5.1.12**, released 2026-03-08; 5.2 beta is excluded

## 1. Requirement contract

The project goal is an original Rust implementation with:

1. **UI parity:** pixel-matched Paint.NET interface states on agreed reference configurations.
2. **Image-output parity:** exact decoded pixels for scripted operations on agreed reference configurations.
3. **PDN compatibility:** semantic read/write interoperability with stock Paint.NET 5.1.12.
4. **Selected plugin compatibility:** unmodified, named third-party plugin DLLs on a documented per-OS compatibility matrix.
5. **Multiplatform product:** Windows x64, macOS ARM64, and Linux x86_64 designed, built, and tested from the first foundation milestone.

The central rule is:

> **Reduce feature coverage when necessary; never label approximate behavior as parity.**

“Latest stable” means **5.1.12 pinned by installer SHA-256 and application build**, not a moving target. A newer Paint.NET release triggers an explicit re-baselining milestone. Paint.NET 5.2 beta and the planned 6.0 format/renderer are separate future compatibility cycles.

### What is not being promised

- Byte-identical PDN, PNG, JPEG, or installer files.
- Universal compatibility with every historical or third-party Paint.NET plugin.
- Identical output on every GPU, driver, OS theme, DPI, font configuration, or color profile.
- Permission to redistribute Paint.NET-specific art, fonts, binaries, or documentation.
- Broad Paint.NET feature coverage in a few weeks.

## 2. Acceptance definitions

### 2.1 UI pixel parity

For each versioned UI state:

- Drive stock Paint.NET and the clone through the same state script.
- Capture raw app-controlled pixels under the pinned Windows build, Paint.NET build, theme, DPI, window size, locale, font configuration, color profile, and renderer.
- Require **zero differing pixels inside declared app-controlled comparison regions**. Do not use a broad percentage or perceptual tolerance to hide mismatches.
- Predeclare narrow dynamic masks only for genuinely nondeterministic content such as a caret, cursor animation, clock, or OS-owned surface. Record every mask in the manifest.
- Cover normal, hover, pressed, focused, disabled, selected, menu-open, dialog, error, and dirty-document states as applicable.
- Repeat the reference capture before trusting it. If Paint.NET itself is nondeterministic, identify the cause before creating a baseline.

The parity surface includes the Paint.NET client area and application-owned dialogs. OS-owned window decorations, native file pickers, and the macOS global menu require an explicit P0 decision:

- **Platform-surface option:** keep those OS-owned surfaces native and record them as exceptions; or
- **Strict-surface option:** implement custom window chrome/dialog replacements, with additional accessibility, platform, and schedule cost.

No OS-owned surface will be silently excluded from the requirement.

### 2.2 Image-output parity

- Compare decoded BGRA/RGBA values exactly, including transparent and hidden RGB values where the file/model preserves them.
- Use identical source documents, tool parameters, pointer scripts, selections, layer states, color settings, and operation ordering.
- Require exact pixels for every operation marked parity-complete on the pinned reference configuration.
- PNG/PDN encoded bytes may differ when semantics and decoded pixels are equal.
- Lossy codecs require their own differential gate. If exact decoded JPEG parity cannot be achieved, that codec remains parity-incomplete rather than receiving an arbitrary tolerance.
- Keep document-renderer correctness separate from GPU display. The GPU is never the correctness oracle.

### 2.3 PDN semantic compatibility

A compatible file cycle preserves:

- Canvas dimensions and layer order.
- Exact layer pixels, including odd strides/sizes and hidden RGB under alpha zero where representable.
- Unicode names, visibility, opacity, background flags, and all supported blend modes.
- Supported resolution, profile, and metadata fields.
- Unknown or unsupported information through lossless preservation where possible, or an explicit unsupported/loss report. Silent dropping is a failure.

Acceptance requires both directions:

1. **Paint.NET → clone:** exact semantic read and an explicit report for unsupported data.
2. **Clone → Paint.NET:** stock Paint.NET opens without repair/warning, exposes equivalent semantics, exports, and re-saves.
3. **Two-way cycles:** PDN → clone → PDN → Paint.NET and clone → PDN → Paint.NET → PDN → clone remain semantically equal after accounting for documented reference normalization.

Byte diffs are diagnostic only. XML ordering, thumbnail encoding, NRBF member ordering, gzip choices, chunking, and producer-version metadata need not be identical.

### 2.4 Selected-plugin compatibility

For every named plugin and exact Paint.NET API/runtime target:

- Load the **unmodified third-party DLL** with its permitted dependencies.
- Run identical source images and settings in stock Paint.NET and the clone.
- Require exact output pixels or file semantics and matching configuration-UI states within the declared parity surface.
- Record Windows x64, Windows ARM64 reference, macOS ARM64, and Linux x86_64 support independently.
- Mark native-code, WinForms/WPF, `System.Drawing`, P/Invoke, or undocumented-API dependencies explicitly; never infer portability from “managed DLL.”
- A plugin that works only on one OS is a documented platform exception, not cross-platform compatibility.

### 2.5 Completion status

Every feature uses one of:

```text
unimplemented
implemented-unverified
parity-pinned
platform-exception
blocked-legal
blocked-technical
```

Only `parity-pinned` counts as complete for the corresponding target and reference configuration.

## 3. Critical feasibility findings

### Paint.NET and VM

- Official releases identify **5.1.12** as current stable, with native x64 and ARM64 packages.
- This Mac is an **Apple M5 Max, 128 GB RAM, approximately 1.1 TiB free**, with **UTM 4.7.5** and hardware virtualization support.
- UTM and Microsoft document virtualized Windows 11 ARM64 installation on Apple Silicon. A valid Windows license is required.
- Paint.NET requires Direct3D 11 and at least four cores. Actual D3D11/runtime reliability in this exact UTM configuration is **unverified** and is the first infrastructure gate.

### PDN

- No official public byte-level PDN3 specification was found.
- Independent implementations describe PDN3, an XML preamble, NRBF object data, and raw/gzip pixel chunks; this is reverse-engineered knowledge, not a normative specification.
- Useful read references include MIT `pypdn` and Apache-2.0/MIT PhotoCraft code, subject to provenance and coverage review.
- EricKrouss/OpenPDN reports current Rust read/write support but explicitly reports decompilation-derived routines. It is **not** an automatic clean-room dependency; legal/provenance review is mandatory before reuse.
- Paint.NET’s roadmap says 6.0 will introduce a new PDN format with public documentation planned. Current support is therefore a versioned **PDN3 codec**, not a permanent universal PDN implementation.

### Plugins

- Paint.NET’s license prohibits using its DLLs in another application. The clone must not ship or host Paint.NET assemblies as its plugin runtime.
- Selected-plugin support therefore requires a legally reviewed clean-room compatibility surface and an isolated .NET host, or a separately justified approach.
- Managed code can still depend on Windows-only UI, native libraries, undocumented internals, or architecture-specific instructions.

### UI assets and fonts

- Current Paint.NET icons have commercial Axialis/Icons8 origins and Paint.NET-specific modifications/artwork. They are not assumed to be redistributable open-source assets.
- Exact public UI parity requires an asset-by-asset provenance and rights matrix, potentially including commercial licenses and permission for Paint.NET-specific art.
- Font files, cursors, logos, and modified icons are separate rights decisions from functional behavior.
- If rights cannot be secured, the affected assets are `blocked-legal`; the plan will not disguise substitute art as pixel-identical.

These are feasibility gates, not reasons to abandon the goal. They do mean that broad parity cannot be guaranteed before the P0 spikes.

## 4. Target architecture

See [the interactive cross-platform parity architecture](architecture.html).

### Product modules

| Module | Responsibility |
|---|---|
| `core` | Geometry, color/pixel types, coordinates, IDs, limits, errors; no GUI or OS types |
| `document` | Layers, selections, history, immutable snapshots, tiled patches, semantic equality |
| `raster` | Deterministic compositing, tools, transforms, adjustments, effects, scalar reference kernels |
| `io` | Standard image formats, EXIF/color boundaries, resource limits, safe save behavior |
| `pdn3` | Bounded PDN3/NRBF reader/writer, versioned semantic mapping, unknown-data reporting |
| `app` | Single-writer session, commands, tool state machines, job ordering, UI-neutral view models |
| `ui` | Custom Paint.NET-compatible widgets, explicit layout, themes, input, app-controlled window surfaces |
| `plugin-bridge` | Versioned IPC, shared tile buffers, cancellation, timeouts, plugin compatibility manifests |
| `platform` | OS integration that is not part of the identical app-controlled UI |
| `test-support`, `xtask` | Fixture generation, differential runners, manifests, comparisons, baseline review tools |

### External components

- **`plugin-host`:** a separate .NET process with clean-room compatibility assemblies and a custom assembly-loading boundary. It must not load Paint.NET’s own DLLs as the clone runtime.
- **`reference-driver`:** a test-only Windows automation component, using Microsoft UI Automation through FlaUI where Paint.NET exposes usable controls, with coordinate/image fallbacks for custom surfaces.
- **Windows reference VM:** stock Paint.NET 5.1.12 and the pinned Windows/tooling configuration.
- **Golden/evidence store:** versioned screenshots, decoded pixels, PDN files, semantic manifests, plugin outputs, environment hashes, and reviewed diffs.

### Runtime rules

- The app session remains the single writer. Plugin results return as versioned patches and are validated before commit.
- Workers consume immutable snapshots; fixed-order operations and exact arithmetic prevent worker-count differences.
- Plugin and reference processes are outside the UI thread and communicate through bounded protocols.
- Plugin process isolation improves crash containment but is **not automatically a security sandbox**. Trusted-plugin and restricted-plugin modes must be described honestly.
- Paint.NET is a reference oracle only, never a product runtime dependency.

## 5. P0 reference laboratory

### Current local VM evidence (2026-10-26)

The project owner supplied a running UTM VM. Direct inspection established:

- **Virtualization:** QEMU/HVF ARM64, 4 virtual cores, 12 GiB RAM, `virtio-ramfb-gl`; not x64 emulation.
- **Guest:** Windows 11 Pro build 26100 ARM64. The attached ISO filename identifies a 26100.4349 ARM64 image; its ISO hash, Windows license status, and update state have not yet been audited.
- **Application:** Paint.NET 5.1.12, executable file version `5.112.9563.32325`. `PaintDotNet.exe` has a valid DOTPDN LLC signature and SHA-256 `17B27AEE8EB0D18E7B39964E6C1C43B1DAC5705F23727AF550F2CDE7FE737E2E`.
- **Installer artifact:** the guest contains identical copies of `paint.net.5.1.12.install.anycpu.web.zip`, SHA-256 `311C28433616C80FE0E3FC56A23B87068A8213E38F52481F88F5F9DB147D3E5F`. This is a web-installer ZIP, not a complete offline package.
- **Graphics:** DxDiag reports DDI 12 and feature levels through `12_1`, including `11_1` and `11_0`, on the Red Hat VirtIO GPU DOD controller. This clears the Direct3D 11 prerequisite; it does not by itself certify Paint.NET rendering or output stability.
- **UI and first operation:** Paint.NET’s main window and default blank image were captured on screen. A scripted **Adjustments → Invert Colors → PNG export** completed successfully after an interactive relaunch. The 800×600 output is 436 bytes and decodes to 480,000 identical opaque-black RGBA pixels. See the [local observation manifest](../artifacts/reference-local/paintnet-5.1.12-invert.manifest.json) and [reference PNG](../artifacts/reference-local/paintnet-5.1.12-invert.png).

Initial guest-agent diagnostics worked, but the agent/RPC channel became unresponsive during the first interactive scheduled-task probe. Recovery required a normal restart followed by a UTM reset at the firmware boot handoff; subsequent guest-agent checks and file export succeeded. Semantic UI Automation enumeration, repeatable A/A captures, a clean snapshot, and a pinned DPI/theme/font/color configuration are still missing. The first output is an **uncertified single-run observation**, so the VM is **not yet a certified reference oracle**.

### 5.1 VM setup

For a reproducible rebuild or a second reference lane:

1. Obtain a valid Windows license and the official Windows 11 ARM64 ISO; verify its SHA-256.
2. Create a UTM **Virtualize**, not emulated, ARM64 VM. Start with 8 virtual cores, 16 GB RAM, and a 128 GB dynamically allocated disk; adjust only after measurement.
3. Install the pinned SPICE guest tools and a project-dedicated shared directory.
4. Verify Direct3D 11 and install Paint.NET **5.1.12 ARM64**. Record the installer hash and verify its publisher/signature where available.
5. If Paint.NET cannot launch and complete basic rendering reliably, time-box driver diagnosis and test another VM backend. Do not emulate x64 as the default reference path.
6. Create pristine snapshots: clean OS, post-tools, post-Paint.NET, and per-suite clean state.
7. Disable automatic Paint.NET updates and control Windows updates during a compatibility cycle.

### 5.2 Pinned configuration manifest

Every artifact records:

```text
Paint.NET version/build and installer SHA-256
Windows ISO/build/architecture and update state
UTM/QEMU/SPICE versions and VM hardware configuration
GPU/display driver and renderer information
DPI/scale, resolution, window bounds, theme, locale, keyboard layout
Font files/hashes, font smoothing, text-rendering settings
Color profile and relevant application settings
Fixture/parameters/source hashes and automation-script version
Capture timestamp, repeat count, dynamic masks, and comparator version
```

### 5.3 Initial reference configurations

Start narrowly, then expand:

| Configuration | Purpose |
|---|---|
| Windows 11 ARM64, 1920×1080, 100% scale, en-US, SDR/sRGB, explicit Light theme | Primary UI and raster baseline |
| Same baseline at 150% and 200% scale | Non-integral and high-DPI layout, icon, text, and input mapping |
| Light, Dark, Blue, and Default under fixed OS light/dark modes | Theme-state parity as separate single-factor rows |
| Windows x64 comparison lane | Detect CPU/architecture-specific output and plugin issues |

The ARM64 VM is the local primary oracle. The smaller x64 lane may use a separately provisioned machine when needed; Windows ARM x64 emulation is not assumed to produce identical results. Add 125%/400%, mixed-DPI, alternate Windows chrome, rendering, localization, and HDR/WCG rows only as separately scheduled single-factor configurations—not one unmanageable Cartesian product.

### 5.4 Automation and security

- Use FlaUI/UI Automation for menus and accessible controls; use raw input and screenshot-region assertions for custom Paint.NET surfaces.
- Capture lossless physical pixels with Windows Graphics Capture for composed/DXGI content; do not rely solely on `PrintWindow`. Wait for stable consecutive frames and record window/client rectangles and color space.
- Restore a clean snapshot for each destructive suite.
- Keep the reference runner private. Do not expose a self-hosted VM runner to untrusted fork PRs or secrets.
- Use project-authored or permissively licensed fixtures. Keep proprietary reference screenshots/artifacts in appropriate private storage unless redistribution rights are confirmed.

## 6. Clean-room parity workflow

1. **Specify:** write the observable behavior, input domain, UI states, output contract, and unknowns before implementation.
2. **Capture:** generate reference evidence through documented UI/file behavior using original fixtures.
3. **Repeat:** establish reference determinism and record any environment dependence.
4. **Test first:** add the failing differential test and the smallest independent unit/property tests.
5. **Implement:** write original Rust or clean-room compatibility code against the specification.
6. **Compare:** produce actual/expected/diff artifacts and the first differing pixel/field/state.
7. **Review:** approve baselines and semantic normalizations manually. CI never auto-blesses.
8. **Certify narrowly:** mark parity only for the tested Paint.NET build, feature domain, and platform matrix.

Use one canonical UI-neutral command/trace format and semantic snapshot model to drive both the clone and Paint.NET reference automation. Only the reference lane generates reference goldens; implementation lanes consume approved specifications, public standards/docs, and black-box fixtures. Keep an independent holdout corpus so implementations cannot merely fit published examples.

A `parity-ledger` entry contains:

```text
feature_id
reference_build_and_configuration
behavior_specification
source_and_fixture_provenance
ui_state_ids
raster_script_ids
pdn_mapping
plugin/api_version
asset_and_code_rights_status
windows_macos_linux_results
exceptions_and_unknowns
reviewed_baseline_hashes
```

REA remains an analysis tool, not an editor dependency. Static IL/metadata or other reverse-engineering analysis is allowed only in a separately reviewed legal/provenance lane. It is not required for black-box UI and file differential testing and is not on the critical path by default.

## 7. UI parity implementation strategy

### Do not assume stock widgets are sufficient

The previous default `eframe`/`egui` choice is now a **candidate**, not a foregone conclusion. Stock widgets, native dialogs, native font handling, and arbitrary GPU output cannot be assumed to produce Paint.NET-identical pixels.

P0 runs a measured UI spike:

- **Candidate A:** custom-painted `eframe`/`egui` widgets with explicit layout and a deterministic screenshot path.
- **Candidate B:** a narrower custom `winit` window/rendering layer with software-rendered controls if Candidate A cannot match text, geometry, or input within the spike.

Test a main window, menu, tooltip, Layers/Colors panels, and one modal dialog at 96/144/192 DPI with actual-window captures on all three OSes, plus focus, keyboard, IME, accessibility, and mixed-DPI behavior. If passing requires replacing most of egui’s layout, widgets, text, and rendering, stop and re-estimate for a custom retained UI rather than fighting the framework.

Do not build a general GUI toolkit. Build only the required Paint.NET-compatible controls and layout primitives. GTK/native widgets are not the default for identical cross-platform visuals. GPU acceleration remains a display optimization behind a deterministic software reference renderer.

### UI specification package

For each screen/dialog and state, capture:

- Logical and physical bounds, spacing, alignment, z-order, clipping, and scrolling.
- Colors, gradients, borders, shadows, opacity, and disabled/hover/focus treatments.
- Font family/file/hash, size, weight, style, shaping, baseline, hinting, smoothing, and DPI behavior.
- Icon/cursor source, upstream license, modifications, and redistribution status.
- Keyboard/mouse navigation, shortcuts, focus order, accessibility names, and platform aliases.
- Reference screenshots and a machine-readable geometry/style manifest.

### High-risk UI areas

- DirectWrite-compatible text and glyph rasterization.
- Segoe/other font redistribution and cross-platform availability.
- High-DPI icons and custom cursors.
- Custom window chrome, file dialogs, docking/floating panels, and menu behavior.
- Paint.NET’s canvas text tool, which is both a UI and raster-parity problem.

These areas receive isolated P0 tests before broad UI implementation.

## 8. Raster and color parity strategy

Target Paint.NET 5.1.12’s 32-bit BGRA/UI8 document behavior first; do not import 5.2 beta FP32 semantics into the stable baseline.

### Differential probe families

- Source-over and every supported layer blend mode over transparent, partial-alpha, opaque, and hidden-RGB cases.
- Brush/pencil/eraser spacing, antialiasing, opacity, size, falloff, clipping, and pointer interpolation.
- Selection shapes, combination modes, antialiasing/coverage, and edge coordinates.
- Fill, gradients, move, crop, resize, canvas size, rotation, and every resampler.
- Every adjustment/effect over parameter sweeps and color/alpha grids.
- Text rendering over explicitly licensed font fixtures.
- ICC/color-profile conversion, DPI metadata, EXIF orientation, and codec behavior.

Use exact integer/fixed-point kernels where they match the reference. If the reference is architecture- or renderer-dependent, record separate baselines and define the supported compatibility domain rather than forcing one approximate formula everywhere.

Required properties include clipping, tile/chunk/worker invariance, exact undo/redo, and cross-OS semantic equality. Differential tests remain necessary even when properties pass.

## 9. PDN3 implementation plan

1. Implement a bounded, safe NRBF/PDN3 reader that never instantiates classes named by a file.
2. Map the stream into an independent semantic model; enforce allocation, depth, reference, chunk, and decompression limits.
3. Use permitted independent readers as references only after license/provenance review. Keep GPL and decompilation-derived code outside the clean-room implementation unless explicitly approved.
4. Build an authentic 5.1.12 corpus: 1×1 and odd dimensions, transparent/hidden-RGB pixels, Unicode/long names, visibility, opacity boundaries, all 14 blend modes, many layers, multi-chunk surfaces, DPI, profiles, and metadata.
5. Add the writer and validate with stock Paint.NET—not self-round-trips or embedded thumbnails.
6. Add reference→reference, reference→clone, clone→reference, and clone→clone semantic checks, both two-way cycles, reference normalization reports, and explicit unknown/unsupported behavior.
7. Fuzz malformed magic, lengths, graphs, references, cycles, chunks, compression, and unknown classes/blends.
8. Quarantine historical versions and 5.2 beta into separate lanes. Treat the future 6.0 format as a new codec milestone with its planned public documentation.

An optional legal/provenance review may evaluate OpenPDN as an acceleration path. The schedule must not depend on accepting it.

## 10. Selected-plugin implementation plan

### Compatibility architecture

- Pin the host to the plugin compatibility runtime: Paint.NET 5.1.12 bundles **.NET 9**, and its stable-era samples target `net9.0-windows`. Archive the 5.1 API evidence; current rolling samples/docs are already moving toward 5.2/.NET 11 and a new FileType architecture.
- Run an unmodified allowlisted third-party plugin in a separate .NET host process. The host must never probe, copy, or load the real Paint.NET installation.
- Resolve the required public Paint.NET API surface through clean-room compatibility assemblies and an explicit assembly-loading/remapping layer. Use `System.Reflection.Metadata` to inventory references and APIs without decompilation.
- Share one set of shim assemblies for type identity; isolate only plugin-owned dependencies. `AssemblyLoadContext` alone does not solve duplicate-type identity, assembly-version, or native-dependency issues.
- Exchange versioned commands and canonical BGRA buffers through IPC, starting with copies for correctness and adding shared tiles only after profiling. Never pass managed objects, pointers, handles, or Direct2D resources across the boundary.
- Map plugin configuration UI only for tested API patterns. Arbitrary WinForms/WPF hosting is not assumed portable; `System.Drawing`, Direct2D, and native Windows dependencies remain explicit platform boundaries.
- Start Windows-first. Native macOS/Linux hosting is allowed only for individually proven portable binaries. A Windows VM fallback may host Windows-dependent plugins but does not remove the shim work or guarantee seamless UI.
- Return versioned patches to the single-writer app session; support cancellation, timeout, crash reporting, and stale-result rejection.
- Require explicit trust for full-capability plugins. Process isolation alone is not a sandbox.

### Named-plugin test set

P0 will inventory and obtain project-owner approval for these representative candidates against Paint.NET 5.1.12. They are candidates, not compatibility results:

1. **Classic CPU adjustment:** [`HCL/L*a*b* color` v0.1](https://github.com/RunDevelopment/hcl-color-adjust-plugin), MIT. It exercises `Surface`, `ColorBgra`, selection/ROI, cancellation, and IndirectUI properties. The released DLL’s target framework and 5.1.12 compatibility are unverified and must be checked before selection.
2. **Modern CPU effect API oracle:** official MIT [`SquareBlurBitmapEffect`](https://github.com/paintdotnet/PdnV5EffectSamples/blob/73997a36a52b0fab955e89e340829657659b8b27/Bitmap/SquareBlurBitmapEffect.cs) at the stable-era `net9.0-windows` sample commit. This tests `PropertyBasedBitmapEffect`, property tokens, clipping, tiled rendering, floating-point/color conversion, and cancellation; it is an API oracle, not third-party binary proof. `HueRotateEffect` is a separate Windows-only GPU stress case.
3. **FileType plus native dependency:** Nicholas Hayes’ [`WebPFileType` v1.6.0](https://github.com/0xC0000054/pdn-webp/tree/v1.6.0), MIT with separate native-codec notices. It exercises the stable FileType/host/document/stream surface, metadata, progress/cancellation, and x64/ARM64 native resolution. Native Windows libraries are not native macOS/Linux libraries; expect an explicit platform matrix and account for Paint.NET’s bundled WebP when designing oracle tests.

The final allowlist records plugin/version/package hash, source and binary licenses, transitive dependencies, target framework, assembly references/PublicKeyTokens, required APIs, native architecture, supported OS/runtime, and redistribution permission. If a candidate fails, replace it or narrow its claim. Recompiling source or porting an algorithm is a separate **source-adapter** track, not proof of unmodified binary compatibility. CodeLab itself is a later stress test, not a first target.

### Go/no-go criteria

- **Go:** the unmodified selected DLL loads and produces exact reference results; the API shim has a lawful provenance path; crashes cannot corrupt the document; the declared OS matrix is proven.
- **Narrow:** select a different representative plugin or mark a specific OS/API feature as a platform exception.
- **No-go for broad compatibility:** if support depends on shipping Paint.NET assemblies or emulating undocumented Windows internals, do not claim general plugin compatibility.

## 11. Cross-platform build and test matrix

| Lane | Every PR | Nightly / release |
|---|---|---|
| Windows x64 | Build, unit/property tests, headless goldens, UI smoke | Full differential suite, packaging, selected plugins |
| macOS ARM64 | Build, unit/property tests, headless goldens, UI smoke | Retina/input/VoiceOver checks, packaging, selected plugins |
| Linux x86_64 | Build, unit/property tests, headless goldens, UI smoke | Full differential suite, packaging, selected plugins |
| Windows ARM64 reference VM | Small deterministic reference/candidate smoke | Clean-snapshot reference capture and full parity run |
| Windows x64 reference | Architecture-sensitive subset on a scheduled lane | Expanded codec, plugin, and renderer comparison |

Additional rules:

- Pin Rust, .NET, GUI, font, codec, and compatibility-shim dependencies; commit lockfiles.
- Keep reference credentials and proprietary artifacts out of public CI.
- Compare decoded pixels and semantic documents, not compressed bytes alone.
- Run release/debug and 1/2/N-worker differential suites for raster code.
- Fuzz PDN/image/plugin protocols independently and with resource limits.
- Package Windows, macOS, and Linux builds from the first exact vertical slice to expose platform gaps early.

## 12. Subagent execution model

The expanded scope needs five focused specialist lanes plus one integrator for the estimates below. More agents may assist research, but integration remains the bottleneck.

| Lane | Ownership |
|---|---|
| **A — document/raster** | `core`, `document`, `raster`, exact kernels, properties, raster differential tests |
| **B — UI and assets** | `ui`, custom controls/layout, font/render spike, geometry/style manifests, asset provenance |
| **C — formats/PDN** | `io`, `pdn3`, semantic corpus, two-way validation, parser fuzzing |
| **D — plugin host** | `plugin-bridge`, .NET host, clean-room API surface, selected-plugin manifests and tests |
| **E — reference and platforms** | VM, FlaUI driver, capture orchestration, three-OS CI, packaging, evidence store |
| **Integrator** | Scope, API/ADR review, dependency policy, merges, legal/provenance coordination, release decisions |

Split document and raster into separate lanes after P0 if throughput requires it.

### Conflict and quality controls

- Initialize crate shells and contracts before feature fan-out.
- Use one Git worktree/branch per agent and strict module ownership.
- Only the integrator changes workspace manifests, lockfiles, and shared APIs; consumers provide failing contract tests for requested changes.
- Keep PRs small and merge specification/test contracts before large implementations.
- Give each fixture its own auto-discovered directory to avoid shared-manifest conflicts.
- Every agent reports observed red/green evidence, reference hashes, exact platform coverage, and unresolved exceptions.
- No “test later,” baseline auto-approval, hidden asset copying, or silently narrowed parity claims.

## 13. Roadmap and honest time horizon

Estimates assume one dedicated integrator, the five effective specialist lanes above, timely Windows/asset licensing decisions, and no broad scope additions. They are planning ranges, not guarantees; adding agents does not linearly compress reference, rights, or integration work.

| Milestone | Elapsed planning range | Outcome |
|---|---:|---|
| **P0 — feasibility and reference lab** | **8–16 weeks** | Decisions/rights review plus reproducible VM captures, UI/font/asset spike, exact raster probes, basic PDN two-way proof, one selected-plugin proof, and go/no-go report |
| **P1 — exact vertical-slice preview** | **7–13 months total** | A deliberately narrow release-quality slice covering all five hard requirements on three OSes: exact included UI states, exact raster output, basic PDN interoperability, and one named plugin |
| **P2 — core-editor parity beta** | **18–30 months total** | Broader tools, selections, blend modes, transforms, adjustments/effects, PDN corpus, and the production named-plugin matrix |
| **P3 — broad 5.1.12 coverage** | **4–8 years** | Extensive built-in tools, UI states/DPIs/localizations, codecs, color/text, and sustained three-OS parity; still not arbitrary historical-plugin compatibility |

P0 combines a 2–6-week decision/rights/baseline stage with a 6–10-week oracle/architecture-spike stage. P1 includes narrow headless parity, UI/plugin/platform closure, and release hardening. A wholesale custom text/widget stack may add **3–6 months**; external rights/legal delays are excluded.

A strict custom replacement for OS-owned window/file surfaces, unresolved asset permission, broad color/text parity, or difficult plugin UI dependencies can extend these ranges. P0 must refine the roadmap using measured evidence. Arbitrary plugin parity has no responsible finite estimate, and tracking future Paint.NET releases is ongoing maintenance rather than a one-time finish line.

### P1 narrow-slice definition

The first release-quality slice deliberately contains only: English/one theme and a pinned reference family; a small main-window/menu/dialog set; a 256×256 two-layer document; Normal blending, opacity, visibility, rectangle selection, solid fill, Invert, and undo/redo; lossless PNG; the specified PDN3 subset with four-way interoperability; and one suitable allowlisted managed effect. It must pass the declared three-OS and 96/144/192-DPI gates. This is small by design so that every hard requirement is proven without approximate features.

### P0 exit gates

Do not start broad feature fan-out until:

1. Paint.NET launches and performs reliable reference operations in the VM; repeated captures are understood and reproducible.
2. One representative main-window state and text/icon configuration have a credible exact-match rendering path.
3. Source-over, one brush path, one resampler, and one adjustment have exact differential probes.
4. A basic PDN3 document survives both read/write directions in stock Paint.NET.
5. One selected unmodified plugin produces exact output through the clean-room host on the declared initial OS matrix.
6. The asset/font matrix identifies a lawful distribution path or marks precise blockers.
7. All three target OSes build and run the foundation, with CI and private reference storage operational.

If a gate fails, narrow the affected compatibility claim or change the approach. Do not compensate by adding more approximate features.

## 14. Immediate next actions

### Inputs needed from the project owner

- Confirmation of the valid Windows license and ISO hash; permission to use this VM for automated tests is already provided.
- Project distribution model and license preference, needed for asset/plugin compatibility review.
- Approval of the three named plugin candidates after the P0 audit.
- Decision on native OS surfaces versus strict custom window/dialog parity.

### First implementation sequence

1. Stabilize the existing Windows ARM64 VM and guest-agent automation; verify the Windows license/ISO hash, repeat the completed Invert/PNG probe under a pinned configuration, and create clean snapshots.
2. Pin all reference hashes and create the parity ledger, artifact schema, and clean fixture corpus.
3. Automate one repeatable Paint.NET screenshot, one raster export, and one PDN save.
4. In parallel, run the UI-renderer, PDN reader/writer, and selected-plugin feasibility spikes.
5. Establish Windows/macOS/Linux CI before feature work.
6. Review P0 evidence and only then commit to the P1 delivery date and exact feature list.

This remains the fastest responsible route to the requested goal: it proves the hardest parity assumptions before years of feature work are built on them.

## 15. Principal risks

| Risk | Required response |
|---|---|
| UTM cannot provide a reliable D3D11 Paint.NET reference | Time-box diagnosis; switch VM backend or provision a separate reference machine; never invent baselines |
| Asset/font rights block public pixel-identical UI | Asset-level provenance matrix, commercial licenses/permission where possible, explicit `blocked-legal` status otherwise |
| Stock/custom UI text or rasterization cannot match | Deterministic software-rendered custom controls; isolate DirectWrite/font problems before selecting the final UI path |
| Paint.NET output varies by renderer/architecture | Pin the supported reference domain and maintain architecture-specific differential lanes |
| PDN writer passes self-tests but fails stock Paint.NET | Stock two-way acceptance is mandatory; self-round-trips never certify compatibility |
| Reused code has incompatible license or decompilation provenance | Provenance review and clean-room isolation; no schedule dependency on unapproved reuse |
| Plugin requires Paint.NET DLLs or undocumented Windows internals | Clean-room public-surface shim; narrow named support; no broad compatibility claim |
| Plugin process compromises or crashes the host | Explicit trust model, bounded IPC, timeouts, crash isolation, document-generation checks; no false sandbox claim |
| “Latest” changes during development | Pin 5.1.12; re-baseline only through an explicit future milestone |
| Agent output scales faster than integration | Five owned lanes, small PRs, contract-first merges, and integrator-controlled scope |

## 16. Primary references

- [Paint.NET 5.1.12 release](https://github.com/paintdotnet/release/releases/tag/v5.1.12), [download](https://www.getpaint.net/download.html), [system requirements](https://www.getpaint.net/doc/latest/SystemRequirements.html), and [roadmap](https://paint.net/roadmap.html).
- [Paint.NET license](https://paint.net/license.html), [layers](https://paint.net/doc/latest/LayersWindow.html), and [blend modes](https://paint.net/doc/latest/BlendModes.html).
- [UTM Windows guide](https://docs.getutm.app/guides/windows/) and [Microsoft Windows 11 ARM64 media](https://www.microsoft.com/en-us/software-download/windows11arm64).
- [Microsoft UI Automation](https://learn.microsoft.com/en-us/windows/win32/winauto/entry-uiauto-win32) and [FlaUI](https://github.com/FlaUI/FlaUI).
- [MS-NRBF](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-nrbf/), [pypdn](https://github.com/addisonElliott/pypdn), [PhotoCraft PDN notes](https://github.com/storytold/photocraft/blob/main/docs/pdn.md), and [OpenPDN compatibility/provenance notes](https://github.com/EricKrouss/OpenPDN/blob/main/docs/pdn-compatibility.md).
- [Paint.NET high-DPI icon history](https://blog.paint.net/2019/03/01/paint-net-4-1-6-alpha-build-7000-with-high-dpi-icons/), [settings](https://paint.net/doc/latest/SettingsDialog.html), [Text tool](https://paint.net/doc/latest/TextTool.html), [Axialis icon licensing](https://www.axialis.com/icons/license.html), and [Microsoft font redistribution FAQ](https://learn.microsoft.com/en-us/typography/fonts/font-faq).
- [Stable-era Paint.NET effect samples](https://github.com/paintdotnet/PdnV5EffectSamples/tree/73997a36a52b0fab955e89e340829657659b8b27), [AssemblyLoadContext](https://learn.microsoft.com/en-us/dotnet/core/dependency-loading/understanding-assemblyloadcontext), [.NET strong-name behavior](https://learn.microsoft.com/en-us/dotnet/standard/assembly/strong-named), and [System.Drawing Windows-only support](https://learn.microsoft.com/en-us/dotnet/core/compatibility/core-libraries/6.0/system-drawing-common-windows-only).

## 17. Recommended approval

Approve **P0 only**: the 8–16-week feasibility/reference-lab phase. Its output is not an approximate editor; it is the evidence required to commit responsibly to an exact cross-platform parity slice and a refined long-term schedule.
