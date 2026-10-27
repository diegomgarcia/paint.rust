# Parity and release ledger

Statuses follow `IMPLEMENTATION_PLAN.md`: `unimplemented`, `implemented-unverified`, `parity-pinned`, `platform-exception`, `blocked-legal`, or `blocked-technical`.

No entry is marked `parity-pinned` by an implementation agent. The integrator requires reviewed reference evidence and passing platform gates.

| Feature / environment | Reference evidence | Product status | Platform scope | Next required gate |
|---|---|---|---|---|
| Windows reference VM | Signed 5.1.12; D3D11; audited 3/3 fresh-document Invert/PNG A/A/A run with matching guest/host/FFmpeg evidence | implemented-unverified | Windows 11 ARM64 / UTM | Pin remaining configuration fields; preserve clean snapshots; expand semantic corpus; Windows x64 remains separate |
| Main-window UI structure | Initial screen observation only | unimplemented | All | Real session-driven UI and usability tests, then reference screenshots |
| Exact UI pixels | None | unimplemented | All | Asset/font rights and zero-diff pinned comparisons |
| Default 800×600 document + Invert | Audited stock 5.1.12 A/A/A: three identical 436-byte PNGs; all 480,000 decoded pixels opaque black in every run | implemented-unverified | Engine/session; reference on Windows ARM64 | Real clone headless/UI differential comparison and native platform gates |
| Normal layer composition | Hand-computed unit/property tests; no Paint.NET differential yet | implemented-unverified | Engine, all CPU targets | Exhaustive alpha/opacity differential probes against stock Paint.NET |
| Other blend modes | Public documentation | unimplemented | All | Implement only with exact per-mode reference evidence |
| Rectangle selection and solid fill | Engine clipping/property tests; no reference corpus yet | implemented-unverified | Engine, all CPU targets | Session integration and boundary/coverage differential corpus |
| Undo/redo | Atomic forward/inverse patches tested; session history pending | unimplemented | All | Session integration/property tests and reference interaction traces |
| PNG decode/encode (bounded 8-bit subset) | Stock 1-bit palette decode; 19 Rust tests; independent Python decode of Rust output matched 480,000 pixels and RGBA SHA-256 | implemented-unverified | macOS ARM64; other native targets pending | Integrated session save/open, broader malformed/format corpus, native platform gates; 16-bit/APNG remain explicitly unsupported |
| JPEG/lossy codecs | None | unimplemented | All | Separate exact decoded-output specification; no arbitrary tolerance |
| PDN3 reader | Bounded data-only NRBF component tested; authentic local corpus pending | unimplemented | All | PDN container/semantic mapping and bounded reads of authentic files |
| PDN3 writer | No accepted clone output | unimplemented | All | Stock Paint.NET open/export/re-save and two-way semantic cycles |
| Isolated plugin transport | 16 Rust + 12 .NET + 8 live cross-language checks; source-owned Invert only | implemented-unverified | macOS ARM64; Windows/Linux pending | Native cross-language CI; no general Paint.NET binary-plugin compatibility claim |
| HCL/L*a*b* adjustment binary | Candidate identified; current compatibility unverified | unimplemented | Windows first | Hash/API inventory, clean-room shim, exact reference results |
| Modern CPU effect API | Stable sample candidate only | unimplemented | Windows first | Stable-era shim and exact output; no third-party binary claim |
| WebP FileType v1.6.0 | Candidate with native dependencies | unimplemented | Windows x64/ARM64 initially | API/native audit and per-OS support decision |
| macOS ARM64 native product | Toolchain available | unimplemented | macOS ARM64 | Native build/test/package and usability smoke |
| Linux x86_64 native product | No local lane yet | unimplemented | Linux x86_64 | Native CI build/test/package smoke |
| Windows x64 native product | ARM64 reference is not x64 proof | unimplemented | Windows x64 | Native build/test/package and architecture-sensitive comparisons |
| Public asset/font redistribution | Commercial/proprietary provenance found | blocked-legal pending review | Public releases | Asset-specific rights matrix and permission/licenses |
| Public project license | Not selected | blocked-legal pending owner decision | Public releases | Owner selects distribution model and license |

## Development-preview exit criteria

The 0.1.0 preview may be released as a **development preview only** when:

1. The strict workspace gate passes on all three primary OS CI lanes.
2. Integrated new/select/fill/invert/undo/redo/PNG workflows and usability tests pass without stubs or ignored acceptance tests.
3. The actual UI launches and performs a package smoke test; no static mock is substituted.
4. Crashes, malformed inputs, stale patches, and failed saves have explicit tested behavior.
5. The README and ledger state every unverified compatibility item. No full-parity or arbitrary-plugin claim is made.
6. The project owner resolves distribution licensing for any public artifact.

PDN and binary-plugin features are included only at the compatibility level independently demonstrated by the reference gates.
