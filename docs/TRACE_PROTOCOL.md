# Deterministic trace and headless-smoke protocol

`schemas/command-trace-v1.schema.json` defines the versioned input. Unknown format versions, commands, fields, invalid rectangles, out-of-range values, and failed commands are errors; they are never ignored.

## Product CLI

```text
paint-ui --headless-smoke --trace <trace.json> --output-dir <directory>
```

Headless mode must execute the real `paint-app` session without initializing a window or GPU and must write:

- `composite.png`: the flattened final document.
- `snapshot.json`: the final semantic snapshot described below.

It prints one JSON result to stdout and exits nonzero on any command, I/O, or trace-expectation failure. Output paths and wall-clock time are excluded from semantic snapshots. `--trace` is invalid without `--headless-smoke`. Output directories may already exist, but existing output files must be replaced atomically.

The external verifier independently decodes `composite.png`, compares exact RGBA and `expect.decoded_rgba_sha256`, and checks `snapshot.json`. The product's own expectation check is additional, not a substitute for the external check.

## Snapshot

```json
{
  "format": "paint.r.semantic-snapshot",
  "version": 1,
  "width": 800,
  "height": 600,
  "layer_count": 1,
  "active_layer_index": 0,
  "dirty": true,
  "can_undo": true,
  "can_redo": false,
  "layers": [
    {
      "name": "Background",
      "visible": true,
      "opacity": 255,
      "is_background": true,
      "blend_mode": "normal"
    }
  ],
  "selection": null,
  "decoded_rgba_sha256": "a7aff0f8e3f44663c369e1e75a981b90b66949586d71035771921871d67e334e"
}
```

`selection` is either `null` or an object with half-open `left`, `top`, `right`, and `bottom` integer edges. Layer order is bottom to top. `decoded_rgba_sha256` hashes the flattened row-major RGBA bytes with no row padding. All snapshot fields except the illustrative values are required.

## Initial traces

- `tests/e2e/traces/invert-800x600.json`: matches the first stock Paint.NET observation semantically; its expected RGBA hash is independently derived from 480,000 opaque black pixels.
- `tests/e2e/traces/undo-invert-2x2.json`: verifies inverse-patch history and redo availability after undo.

A trace passing against Paint.R is not itself Paint.NET parity. The reference lane must execute the equivalent operations in stock Paint.NET before an entry becomes `parity-pinned`.
