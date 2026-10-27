# Parity and release ledger

Statuses follow `IMPLEMENTATION_PLAN.md`: `unimplemented`, `implemented-unverified`, `parity-pinned`, `platform-exception`, `blocked-legal`, or `blocked-technical`.

No entry is marked `parity-pinned` by an implementation agent. The integrator requires reviewed reference evidence and passing platform gates. A self-round-trip, a successful local test, or a file produced by a third-party library is not a Paint.NET compatibility oracle.

| Feature / environment | Reference evidence | Product status | Platform scope | Next required gate |
|---|---|---|---|---|
| Windows reference VM | Signed 5.1.12; D3D11; audited 3/3 fresh-document Invert/PNG A/A/A run with matching guest/host/FFmpeg evidence | implemented-unverified | Windows 11 ARM64 / UTM | Preserve clean snapshots; expand semantic and PDN corpus; Windows x64 remains separate |
| Main-window UI structure | Initial stock screen observation; original session-driven `egui` shell with real canvas, menus, tools, layers, colors, and status | implemented-unverified | Fourteen semantic usability tests (five keyboard-only), including dirty New/Open/viewport-close, signed clipped selection, layer history, and recoverable dialogs; real macOS window launch/graceful quit observed on earlier source | Actual OS keyboard/dirty-close probes; Windows/Linux native windows; reference region/layout comparisons |
| Exact UI pixels | None | unimplemented | All | Asset/font rights and zero-diff pinned comparisons; stock `egui` is not a parity skin |
| Default 800×600 document + Invert | Stock 5.1.12 A/A/A: three identical 436-byte PNGs and 480,000 opaque-black pixels per run; real clone debug/release/packaged outputs have the same decoded RGBA SHA-256 `a7aff0f8…e334e` | implemented-unverified | Exact decoded RGBA differential on macOS ARM64 and Linux ARM64 container; Windows pending | Broaden reference corpus and complete native platform gates; encoded PNG byte identity is not required and is not observed |
| Normal layer composition | Hand-computed unit/property tests; no Paint.NET differential yet | implemented-unverified | Engine and composed UI canvas, all tested CPU targets | Exhaustive alpha/opacity differential probes against stock Paint.NET |
| Other blend modes | Public documentation; non-Normal PDN operations return typed unsupported errors | unimplemented | All | Implement only with exact per-mode reference evidence |
| Rectangle selection and solid fill | Engine clipping/property tests and semantic UI workflows; no reference corpus yet | implemented-unverified | Engine/session/native UI on tested targets | Boundary/coverage differential corpus against stock Paint.NET |
| Undo/redo | Revision-rebased, before-validated patches; repeated history/property tests; semantic UI disabled-state and pixel checks | implemented-unverified | AppSession and native UI on tested targets | Reference interaction traces and native platform usability |
| PNG decode/encode (bounded 8-bit subset) | Stock 1-bit palette decode; 19 Rust codec tests; independent Python oracle; integrated AppSession open/export and real-binary deterministic output | implemented-unverified | macOS ARM64 and Linux ARM64 container; other native targets pending | Broader malformed/format corpus and native platform gates; 16-bit/APNG remain explicitly unsupported |
| JPEG/lossy codecs | None | unimplemented | All | Separate exact decoded-output specification; no arbitrary tolerance |
| PDN3 reader | Bounded XML/container/data-only NRBF and Normal/BGRA semantics; authentic signed 5.1.12 2×2 Normal Background fixture decodes with exact metadata and stock-reopened composite; 36 PDN tests | implemented-unverified | One authentic 2×2 reader baseline on macOS; broader corpus and other platforms pending | Expand authentic dimensions/layers/blends/metadata corpus; non-Normal and unknown required shapes remain unsupported |
| PDN3 writer | Real Normal/BGRA raw/gzip output with self-round-trip tests; no accepted clone output | implemented-unverified | Experimental library only; not wired into native UI | Stock Paint.NET 5.1.12 open/export/re-save and two-way semantic cycles; no compatibility claim from self-round-trips |
| Isolated plugin transport | 17 Rust + 12 .NET + 8 live cross-language checks; source-owned Invert only | implemented-unverified | Full local loop on macOS ARM64; Rust lifecycle/stress on Linux ARM64; Windows/Linux cross-language lanes pending | Native cross-language CI; no general Paint.NET binary-plugin compatibility claim |
| HCL/L*a*b* adjustment binary | Candidate identified; current compatibility unverified | unimplemented | Windows first | Hash/API inventory, clean-room shim, exact reference results |
| Modern CPU effect API | Stable sample candidate only | unimplemented | Windows first | Stable-era shim and exact output; no third-party binary claim |
| WebP FileType v1.6.0 | Candidate with native dependencies | unimplemented | Windows x64/ARM64 initially | API/native audit and per-OS support decision |
| macOS ARM64 native product | Real native window observed as `Paint.R Development Preview`; clean Command-Q exit; debug/release headless smoke; permission-preserving extracted `.app` smoke | implemented-unverified | Local macOS ARM64; unsigned/unnotarized internal package only | Rebuild final candidate after hardening; repeat deterministic archive and packaged native-window smoke |
| Linux x86_64 native product | Linux ARM64 container passes the full local Rust/Python/release-smoke gate; this is not x64 proof | implemented-unverified | ARM64 container green; local x86_64 QEMU (default and explicit Haswell) failed in `rustc -vV` with SIGSEGV before project code | Native/emulated x86_64 CI gate and native-window/package smoke; record emulator failure as an environment blocker, not a product pass/fail |
| Windows x64 native product | ARM64 reference is not x64 proof | implemented-unverified | No product gate yet | Native build/test/package and architecture-sensitive comparisons on Windows x64 |
| Public asset/font redistribution | Commercial/proprietary provenance found | blocked-legal pending review | Public releases | Asset-specific rights matrix and permission/licenses |
| Public project license | Not selected | blocked-legal pending owner decision | Public releases | Owner selects distribution model and license |

## Local evidence

- `artifacts/validation/2026-10-27-macos-arm64-headless-c62a178.json` records the real-binary deterministic traces and the direct stock-versus-clone RGBA comparison.
- `artifacts/validation/2026-10-27-macos-arm64-package-e82e293.json` records the permission-preserving extracted-package smoke. Python `zipfile.extractall` did not restore the archived executable bit; macOS `ditto` restored the correct `0755` mode. The failed extractor attempt is not counted as a product pass.
- `crates/pdn3/tests/stock_5_1_12.rs` and its adjacent fixture manifest record authentic signed Paint.NET 5.1.12 PDN reader acceptance. The clone composite matches stock's reopened PNG exactly for the committed 2×2 Normal/BGRA case.
- Integrated source `e12d5c80ee85ffd3b6ae0a3d88f3eec3ba51b8b2` passed 194 Rust tests, 16 end-to-end/oracle Python tests, 18 packaging tests, 12 .NET tests, and 8 live cross-language checks on macOS. The same source passed 194 Rust tests, 34 Python tests, release build, and deterministic release smoke in a Linux ARM64 container.
- Source `918a03ffaf24a867e630917152a931e351a9dd1e` passed a supplemental `PROPTEST_CASES=8192` run: 133 tests across core, document, raster, PNG, PDN, and AppSession, with no failed/ignored tests. The real plugin-child crash/broken-pipe race also passed 25/25 repeated macOS runs. These supplement the standard platform gates.
- Final internal-candidate source `8c6d0c2ca3813efdce110be81f098dab1f5cdb30` passed 203 Rust and 44 Python tests on macOS ARM64 and Linux ARM64, with strict Clippy/format and zero failed or ignored Rust tests. macOS additionally passed 12 .NET tests, 8 live plugin checks, both reference validators, release packaging, and permission-preserving extracted smoke. Two independent macOS archives were byte-identical: 7,110,075 bytes, SHA-256 `169ed2ba583b0bf9e9a0bb4f0307dbad4d2c009ba67f2415bf793ccf3a939963`; 758 extracted payload entries matched their manifest. The final native-window probe remained blocked by the host lock and is not counted as a product pass or failure.
- These receipts do not certify full UI parity, broad PDN interoperability or writer acceptance, arbitrary plugin compatibility, Windows x64, Linux x86_64, or public redistribution.

## Development-preview exit criteria

The 0.1.0 candidate may be published only as an **internal development preview** when:

1. The strict workspace gate passes on all three primary OS CI lanes.
2. Integrated new/select/fill/invert/undo/redo/PNG workflows and usability tests pass without stubs or ignored acceptance tests.
3. The actual UI launches and performs a package smoke test; no static mock is substituted.
4. Crashes, malformed inputs, stale patches, and failed saves have explicit tested behavior.
5. The README and ledger state every unverified compatibility item. No full-parity or arbitrary-plugin claim is made.
6. Internal artifacts remain private and are labeled unsigned development previews. Any public artifact additionally requires project/asset/font licensing, notice review, signing/notarization decisions, and owner approval.

The current candidate has substantial local evidence but has not completed the full three-OS release matrix or authentic PDN corpus. PDN and binary-plugin features are included only at the compatibility level independently demonstrated by the reference gates.
