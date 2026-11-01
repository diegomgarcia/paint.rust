# Release status

**Product:** Paint.Rust development preview  
**Version:** 0.1.x (post-UI-polish `main`)  
**Result:** internal development preview passes the demonstrated local gates; public binary
distribution remains blocked.

This is a status summary, not a release announcement.

## Gate matrix

| Gate | Status | Evidence or blocker |
|---|---|---|
| Workspace format, strict Clippy, tests | **PASS** | 579 tests; `-D warnings`; `xtask verify` |
| Deterministic core and headless traces | **PASS** | Workspace and UI test suites |
| macOS release build | **PASS** | `cargo build --release -p paint-ui --locked` |
| Native macOS window/input/dirty-close gate | **PASS** | `artifacts/reference-local/zoom-579` |
| Canvas-only zoom shortcuts | **PASS** | UI regression test and live Cmd+Plus/Cmd+Minus check |
| Windows native build/window gate | **NOT RUN** | No Windows gate in this checkout |
| Linux native window/input gate | **NOT RUN** | No native Linux desktop gate in this checkout |
| Hosted three-OS CI | **NOT CONFIGURED** | No git remote or CI workflow |
| Third-party notices | **INCOMPLETE** | Dependency notice curation is not complete |
| Code signing / notarization | **NOT RUN** | No Developer ID identity or notary submission |
| Installer / public binary | **NOT PRODUCED** | Preview builds from source only |
| Public release | **BLOCKED** | Notices, signing/notarization, CI and platform gates remain |

## What is demonstrated

- Native `eframe`/`egui` editor with multi-document tabs, dirty guards, PNG Open/Save/Export,
  clipboard actions and undo/redo.
- 19-tool toolbox, compact contextual ToolConfigStrip, inline canvas text editing and
  Paint.NET-inspired menus and panels.
- Mask-backed selections, layers/blending, image operations and previewable effects.
- Deterministic raster and document crates with integration and UI tests.
- Real macOS keyboard/focus/dirty-close evidence from the native gate.

## Remaining blockers for public distribution

1. Complete third-party notice curation and asset/legal review.
2. Add hosted CI and native Windows/Linux gates.
3. Produce installers and complete code signing/notarization.
4. Finish the documented parity gaps before making compatibility claims.
5. Re-run the full gate matrix on the exact release commit and artifacts.
