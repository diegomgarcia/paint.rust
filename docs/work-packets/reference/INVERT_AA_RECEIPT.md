# Reference receipt: three-run Invert A/A (PASS)

## Exact scope

Three new Paint.NET documents were created through the visible UI, each at the default 800×600 size. Each received Adjustments → Invert Colors and was exported as PNG with the default `Auto-detect` save configuration. This is reference A/A evidence only; it is not clone parity, a golden blessing, a PDN claim, or a public screenshot-release asset.

- Passing run: `artifacts/reference-local/runs/invert-aa-20261026t2242z`
- Harness/documentation commit: `3330d6dc2d426b0a178a198c57e6a6cc9671350b`
- VM remained running; QGA remained healthy. No reset, stop, suspend, snapshot, scheduled-task UI probe, REA, IL inspection, or decompilation was used.
- Local artifacts are ignored and were not added to the source commit. The pre-existing first observation was not modified.

The observation-time `environment.json` records exact hashes for all 14 tool source files then present. The later commit adds this documentation and Home/End entries to the host key map for optional fixture work; consequently its `host_window.swift` hash differs from the observation-time hash (`ac7baf31bb4150446a8235566c42b4a7859f74efc3672bea2565ca69cca6c713`). That later key-map addition is not used by the Invert A/A path. The manifest, not the commit message, is authoritative for observation-time source hashes.

## Commands and results

Formal run:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/observe_invert.py \
  --run-id invert-aa-20261026t2242z --runs 3
```

Result: exit 0, `status: PASS`, `observation_count: 3`, `encoded_bytes_repeatable: true`, and `decoded_rgba_repeatable: true`.

Independent validation rerun:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/validate_observations.py \
  artifacts/reference-local/runs/invert-aa-20261026t2242z/manifest.json
```

Result: exit 0 with the same PASS fields. A separate read-only audit recomputed all three host PNGs and every manifest-listed UI PNG/OCR hash; fresh validation exactly equaled stored `validation.json`. The independent coordinator also compared each separate observation JSON with its manifest entry and independently repeated all three FFmpeg RGBA decodes; all matched.

Tests:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/reference/tests -v
```

Initial red evidence before implementation: one import error, `ModuleNotFoundError: png_evidence`. Final result: **8 tests passed** (decoder, filters, palette/transparency, malformed input, LOGFONT, and manifest mismatch cases). Python AST/import checks and the bounded Swift build also passed. No Rust/unit/property/integration/usability production tests or dependency/license additions are claimed by this receipt.

## Pixel and artifact evidence

All three runs independently produced:

| Evidence | Value |
|---|---|
| Guest filenames | `praa-spcbgcdael.png`, `prab-spcbgcdael.png`, `prac-spcbgcdael.png` in the observed Documents directory |
| PNG size | 436 bytes each |
| Guest and host encoded SHA-256 | `0c8cdc8f496907b88ef445cfdfa8f88e36cf09863a5cdde4b8e7243637708163` |
| Decoded dimensions/layout | 800×600, row-major RGBA8, stride 3200, 1,920,000 bytes |
| Guest and host RGBA SHA-256 | `a7aff0f8e3f44663c369e1e75a981b90b66949586d71035771921871d67e334e` |
| Exact decoded histogram | 480,000 pixels of `[0, 0, 0, 255]`; one unique color |
| Guest decoder | Windows PowerShell `System.Drawing`, 32bpp ARGB bitmap converted from BGRA memory to RGBA |
| Host decoders | stdlib PNG decoder plus independent FFmpeg rawvideo RGBA; full bytes matched |
| Guest stability | Hash/length matched before and after every pull |

Artifact hashes:

- `environment.json`: `32ad17d43166194990b4e763343b13284878dae4af7efaf39fc4a2a6d142e084`
- `manifest.json`: `e372348b1f7c354666ffa6876b249506723c1a94b4b4c8944683d4d5ca337ce1`
- `validation.json`: `47986ca5ab87b1d46c316e64b3c8b3f178b4793a17116c5695ff115493e45845`

Each observation has eight manifest-listed UI capture/OCR pairs (New dialog, new document, Adjustments menu, inverted document, Save As, typed path, PNG configuration, saved document), plus a start capture in the local run directory. Exact actions, window bounds, hashes, guest metadata, and tool versions are in `observation-1.json` through `observation-3.json` and `manifest.json`.

## Environment captured, not fully controlled

The run environment records native Windows 11 Pro 24H2 ARM64 (`ARM64-based PC`), build 26100 with UBR 9711; QGA PowerShell is separately identified as x64 (`AMD64`). Paint.NET's window title reports 5.1.12; the executable is file version `5.112.9563.32325`, valid DOTPDN LLC signature, SHA-256 `17b27aee8eb0d18e7b39964e6c1c43b1dac5705f23727af550f2cde7fe737e2e`.

Other observed fields include:

- UTM 4.7.5, QEMU/HVF, 4 vCPUs, 12 GiB, `virtio-ramfb-gl`, dynamic resolution enabled, native resolution disabled.
- VirtIO GPU DOD driver `22.7.38.43`, 1024×768, 32 bits/pixel. CIM reports current refresh 1 and min/max 64; physical refresh is not inferred.
- `LogPixels`/`AppliedDPI` absent, desktop monitor logical DPI 96×96, `Win8DpiScaling=0`; effective per-monitor Paint.NET DPI remains unpinned.
- Font smoothing 2, type 2, gamma 0, orientation 1. Caption/icon/menu/message/small-caption/status LOGFONTs decode to Segoe UI; selected font file hashes are recorded.
- Light app/system theme values 1, transparency 1, color prevalence 0; raw DWM/system colors and high-contrast values are retained.
- Locale `en-CA`, keyboard preloads `00000409` and `00001009`, timezone Eastern Standard Time.
- Installed color-profile hashes are recorded; no per-display ICM associations were returned, and Paint.NET's actual profile/conversion selection remains unproven.

Explicitly unpinned fields in the manifest include ISO identity/hash, license state, update catalog/pending reboot, effective DPI/DPI-awareness context, HDR/advanced color/VRR/physical refresh, application ICC behavior, complete Paint.NET settings and default text font, exhaustive fonts/fallback/caches/shaping, host brightness/True Tone/Night Shift/ICC, wall clock/future updates, and future dynamic-resolution changes.

## Failed attempts retained, not counted

The passing set followed four stopped attempts; their `failure.json` files and UI captures remain under `artifacts/reference-local/runs`:

1. `invert-aa-20261026t2230z`: OCR read shortcut `I` as `!`; stopped before Invert/export (0 observations).
2. `invert-aa-20261026t2233z`: unexpected Save As dialog at the New gate; stopped before export (0 observations).
3. `invert-aa-20261026t2237z`: OCR read filename digit `1` as `l`; stopped before Save As submission (0 observations).
4. `invert-aa-20261026t2236z` completed one valid observation, then stopped in run 2 when noisy post-Invert OCR omitted redundant status-bar dimensions. Its dirty generated `Untitled` was closed through the visible Unsaved Changes UI; no app/VM restart occurred.

`invert-aa-20261026t2242z` is the only three-run PASS. These stopped attempts are automation limitations, not reference pixel differences. The final harness retains substantive OCR gates and all pixel/hash checks but does not require irrelevant shortcut text or redundant noisy post-effect status text.

## Remaining gates

A real clone binary must be diffed against this reference before any compatibility claim. PDN fixtures, broader commands, color management, effective DPI, additional platforms, and public-release packaging are outside this receipt. Proprietary UI screenshots and local artifacts must remain out of public release claims.
