# Authentic Paint.NET 5.1.12 fixture requests

One authentic stock 5.1.12 fixture is now committed: `tests/fixtures/paintnet-5.1.12-invert-2x2.pdn`, with its stock-reopened PNG and provenance manifest. It establishes one 2×2 Normal/BGRA reader baseline only. The remaining corpus below is still required; the third-party 4.21 research sample is not a substitute.

Each new PDN artifact must be saved by stock Paint.NET 5.1.12 and accompanied by a manifest containing: exact product/build, source SHA-256, canvas dimensions, bottom-to-top layer order, names, visibility, background flags, opacity, blend modes, exact straight-alpha BGRA pixel hashes per layer, and the UI/script actions used to produce it. Record whether any ancillary tool edited the file; such a file is derived, not authentic stock output. Reference-agent execution is required; lane B did not operate the VM.

## Minimum semantic corpus

1. **Dimensions and exact pixels**
   - 1×1 opaque Normal/background with a nontrivial BGRA color.
   - 1×1 fully transparent with nonzero hidden RGB; independently record those source bytes before saving.
   - 1×3 and 3×1 images with distinct pixels in every position, plus an odd 3×3 image.
   - A two-color 512×512 image to exceed 256 KiB chunking and exercise a multi-chunk surface and final short chunk.
2. **Layer attributes**
   - Two Normal layers with distinct pixels and reversed creation/reorder actions to establish bottom-to-top order.
   - Empty/ASCII names, `Layer <&> " ☃ 漢字`, and a long name at the stock UI limit; report that limit.
   - Hidden and visible layers; background and non-background layers; opacity 0, 1, 127, 128, 254, and 255.
   - A many-layer file (at least 64 layers, or the stock limit if lower) with a documented top/middle/bottom marker.
3. **Blend coverage**
   - One file for each of the 14 stock blend modes using the same two small source layers.
   - One mixed-mode stack with visibility and opacity variations.
   - These currently verify explicit unsupported behavior except Normal; they must not be used to claim canonical blending support.
4. **Storage variants**
   - Stock files demonstrating gzip (`0`) and, if obtainable from stock 5.1.12, raw (`1`) pixel chunks. If stock cannot emit raw, label raw coverage synthetic and do not invent a reference result.
   - Any stock-obtainable padded-stride or shared-surface cases. Record the exact stride, memory length, and object sharing in the manifest. Current padded/shared coverage is independently authored synthetic coverage.
5. **DPI, color, and metadata**
   - 72, 96, and 300 DPI; asymmetric DPI if the UI permits it.
   - Default/sRGB and any other stock-selectable color profile/color-context cases; record profile bytes or hash.
   - Nonempty document and layer metadata, Unicode/long metadata values, thumbnail presence, and resave-after-metadata-edit cases. Identify which fields are semantic versus preserved-only.
6. **Writer acceptance, after reference automation exists**
   - Two untouched 2×2 Normal/BGRA clone probes (raw and gzip) have been generated for the stock-open gate; acceptance remains pending until the reference run reports actual stock results.
   - Open Paint.R experimental Normal/BGRA output at 1×1, 3×3, and two layers in stock 5.1.12; capture exact success/failure, names/attributes, and per-layer BGRA after stock resave.
   - Test both raw and gzip writer variants only if stock has a defined path for the selected chunk version.
   - Until these checks pass, writer status remains experimental/self-round-trip-tested only.

Malformed/truncated/resource-limit cases derived from these files must be labeled as derived mutations with mutation offsets and parent SHA-256. Their expected typed errors are implementation requirements, not stock reference behavior unless separately run through the reference workflow. No fuzz result may be presented as authentic reference evidence without that gate.
