# Reference receipt: authentic 2×2 Normal/BGRA PDN (PASS)

## Scope and provenance

One authentic fixture was saved by stock, validly signed Paint.NET 5.1.12 on native ARM64 Windows. The runner created a 2×2 document, applied Invert Colors to the default Background layer, saved PDN, closed that document, reopened the same file through File → Open, and exported PNG only after reopening. No ancillary tool edited the PDN. No independent PDN parser, REA, IL inspection, decompilation, clone comparison, or writer-acceptance test was used.

- Passing run: `artifacts/reference-local/runs/pdn-fixture-20261026t2342z`
- Automation/validator commit: `cce9b2f61c46641db33146ea11747390c693aca1`
- Paint.NET file/product version: `5.112.9563.32325`
- Executable SHA-256: `17b27aee8eb0d18e7b39964e6c1c43b1dac5705f23727af550f2cde7fe737e2e`
- Signature: `Valid`, signer `CN=DOTPDN LLC, O=DOTPDN LLC, S=Washington, C=US`
- `authentic_stock_output: true`; `ancillary_tool_edited_file: false`

## Commands

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/observe_pdn_fixture.py \
  --run-id pdn-fixture-20261026t2342z
```

Result: exit 0, all nine recorded checks true.

Independent validation (recomputes PDN/PNG/environment/UI hashes, action ordering, RGBA/BGRA, and a fresh FFmpeg decode):

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/reference/validate_pdn_fixture.py \
  artifacts/reference-local/runs/pdn-fixture-20261026t2342z/manifest.json \
  --output artifacts/reference-local/runs/pdn-fixture-20261026t2342z/validation.json
```

Result: exit 0, PASS, 32 ordered UI actions and 22 hashed UI evidence items. The expanded Python suite has 10 passing tests; Swift build and all Python AST checks also passed.

## Exact fixture and pixel evidence

| Evidence | Value |
|---|---|
| PDN | `fixture-kwudzv.pdn`, 3,569 bytes, magic `PDN3` |
| PDN SHA-256 | `86736d36d886e08943fe57d3045a092051c83ad7f919aa6f21b177c84007c09b` |
| PDN stability | Guest hash/length equal before reopen, after reopen, and after pull; host hash equal |
| Post-reopen PNG | `fixture-kwudzv-reopened.png`, 351 bytes |
| PNG SHA-256 | `008b013d9c7a9a645fe9889386284608d8969d61a1b2a2044cddf108d9e08904` |
| Dimensions/layout | 2×2, 16 row-major RGBA8 bytes |
| Exact pixels | Four `[0, 0, 0, 255]` pixels; one unique color |
| RGBA SHA-256 | `cc0fa51d4d0a97b664030be5052f3b2b69a1267f89ace4c9bbc65007566725df` |
| Straight-alpha BGRA SHA-256 | `cc0fa51d4d0a97b664030be5052f3b2b69a1267f89ace4c9bbc65007566725df` (black is unchanged by R/B swap) |
| Independent decoders | Guest System.Drawing, host stdlib PNG, and fresh FFmpeg raw-RGBA decode agree |
| Manifest SHA-256 | `1c60140f726d101f2461218b4d02be780b77d7136ddab7733feec4c4c41be7e8` |
| Environment SHA-256 | `67ae3ed0c774f5f20109951542fce8d5313a352d943221795688ec4e73d11329` |
| Validation SHA-256 | `12b6eb331c4a2d5d204571f78be1c4206196bc07f03c59a7922b52cfcd77b749` |

Document semantics in the manifest:

- Canvas 2×2; one layer in bottom-to-top order.
- Layer 0: `Background`, visible, background=true, blend mode `Normal`, opacity 255.
- These attributes are the stock New-document defaults and are explicitly labeled **not separately queried through Layer Properties**.
- Exact straight-alpha BGRA is derived by swapping R/B in the independently decoded post-reopen composite. This is valid for the sole visible layer covering the whole canvas; it is not an independent extraction of internal PDN layer streams.

The environment manifest contains the detailed native OS, display, DPI/theme/font/color, QGA architecture, tool hashes, and unpinned fields. This fixture covers only one tiny all-black Normal/background document; it does not satisfy the broader semantic corpus in `crates/pdn3/REFERENCE_FIXTURES.md` and does not establish that stock Paint.NET accepts clone-written PDNs.

## Retained failed attempts

Every stopped attempt retains `failure.json` and captures under `artifacts/reference-local/runs`; none is counted as the passing fixture:

1. `pdn-fixture-20261026t2301z`: stopped before save when selected-type OCR showed no PDN value; alternate OCR proved the filter was still PNG.
2. `pdn-fixture-20261026t2320z`: native PDN path was saved, but title OCR misread `l` as `h`; stopped before close/reopen, so no certified fixture claim.
3. `pdn-fixture-20261026t2323z`: stopped in New when the `New size` label was misread; the 4.7 KB estimate was present.
4. `pdn-fixture-20261026t2325z`: saved and closed a PDN, but OCR omitted the Open title; stopped before typing/opening.
5. `pdn-fixture-20261026t2328z`: Open-field OCR dropped one `x`; stopped before Enter. The final runner permits at most one filename OCR edit and retains the capture.
6. `pdn-fixture-20261026t2333z`: New click was missed and the File menu remained visible; no document was created.
7. `pdn-fixture-20261026t2336z`: runner variable-name error after creating a 2×2 document, before Invert/save. That known dirty Untitled was explicitly closed with Don't Save.
8. `pdn-fixture-20261026t2338z`: native PDN save/close/reopen succeeded, but PNG dropdown OCR read `*.png` as `*,png`; stopped before export.

`pdn-fixture-20261026t2342z` is the only complete, independently validated fixture in this lane. Local proprietary artifacts remain ignored and are not public-release assets or parity goldens.
