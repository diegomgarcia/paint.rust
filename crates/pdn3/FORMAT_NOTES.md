# PDN3 format coverage and provenance

## Status: PARTIAL

This crate implements a bounded PDN3 subset over the canonical `paint_document` and straight-alpha `paint_core::Surface` models. It is not a claim of complete Paint.NET compatibility.

### Read and write

- `PDN3` magic, little-endian 24-bit UTF-8 XML length, the PDN `00 01` data indicator, and MS-NRBF 1.0 data records.
- Normal bitmap layers in bottom-to-top order, exact BGRA bytes (including RGB under zero alpha), names, visibility, background flags, and all opacity values.
- Document/layer/surface dimension agreement, packed or padded 32-bit BGRA strides, numbered raw and gzip chunks, out-of-order chunks, and shared surface storage. Shared surfaces become independent canonical surfaces; padding is not semantic pixel data.
- Fresh Normal/BGRA writing with raw or gzip chunks. The writer uses producer version `0.1.0.0`, `PaintDotNet.Data` and `PaintDotNet.Core` library names, and actual NRBF class/array/string records. It does not use file-named type instantiation.

The writer is **experimental and self-round-trip-tested only**; stock Paint.NET 5.1.12 acceptance is unverified. Independently authored wire fixtures test the reader and implementation semantics, not the writer's Paint.NET acceptance. Fresh writing does not automatically copy the preservation payload from `DecodedPdn` and has no atomic-file API.

### Preservation, reports, and explicit exclusions

`DecodedPdn` retains the original header XML and complete parsed NRBF graph. It reports nonempty document/layer metadata counts, unmapped known-class members, unreachable objects, padded/shared layers, and retains/reports trailing bytes. This is a preservation payload, not a promise that every metadata field has a canonical semantic equivalent.

Currently rejected with typed errors: non-Normal blend modes/operations, non-bitmap layers, unknown required class shapes, unsupported chunk versions, non-1.0 or method-invocation NRBF streams, ambiguous/undersized non-BGRA strides, inconsistent memory lengths, and shared memory whose parent has not been decoded. Unknown NRBF record types are errors rather than executable callbacks.

Defaults include 512 MiB input/output and total canonical surface limits, 256 MiB per stored surface, 64 MiB pixels, 256 layers, 1 MiB XML, XML depth 64, 8192 XML elements, 16 MiB uncompressed chunks, 64 MiB stored chunks, and one million chunks per surface. Nested NRBF has independent record/object/string/depth/allocation/output limits. DTD/entity declarations are rejected.

### Research and license provenance

- [MS-NRBF, v13.0, 2024-11-19](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-nrbf/75b9fe09-be15-475f-85b8-ae7b7558cfe5) is the normative record-layout source. Class names and library names are data only.
- [pypdn](https://github.com/addisonElliott/pypdn) (MIT) was used as independent public research for the PDN XML indicator and big-endian chunked pixel container. No production Python code is included.
- [PhotoCraft PDN import notes](https://github.com/sabrogden/PhotoCraft) were used only as public format research. No GPL/OpenPDN routines, decompilation-derived routines, Paint.NET assemblies, or Paint.NET DLLs were used or copied.

A third-party pypdn 4.21 sample (`Untitled3.pdn`, 8,773 bytes, SHA-256 `a4cb06a92bba14c56123651530a442bba42ed81c37dfe52bca5f82b7891c92f2`) was inspected locally. Its second Additive layer is rejected as unsupported. A temporary derived probe retaining only its original first Normal layer and original gzip chunks decoded 800×600/480,000 pixels. Neither the source nor derived file is committed, and neither is a stock Paint.NET 5.1.12 compatibility oracle.

No dependency or manifest changes were made. The crate uses the workspace-approved existing `flate2`, `quick-xml`, `thiserror`, `proptest` (development), `paint-core`, and `paint-document` dependencies.

See [REFERENCE_FIXTURES.md](REFERENCE_FIXTURES.md) for the evidence still required from authentic stock Paint.NET 5.1.12.
