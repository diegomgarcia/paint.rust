# GitHub repository setup

Copy these values into the GitHub "Create repository" and "About" forms.

## Repository

| Field | Value |
|---|---|
| Repository name | `paint-rust` |
| Display name / product | **Paint.Rust** |
| Default branch | `main` |
| Visibility | Public |
| Description | A fast, modular Paint.NET-inspired image editor written in Rust. Native desktop UI, deterministic raster core, honest parity. |
| Website | Leave empty for now |
| Social preview | Upload [`docs/images/paint-rust-preview.png`](images/paint-rust-preview.png) |
| License | MIT |

## Topics

```text
rust
egui
eframe
image-editor
paint-net
raster-graphics
desktop-app
macos
windows
linux
mit
```

## About panel

Enable:

- Releases
- Issues
- Discussions (optional, recommended)
- "Include in the home page" / About

Do **not** claim Paint.NET compatibility, plugin compatibility or pixel parity in the repository
description. The honest positioning is "Paint.NET-inspired", not "Paint.NET-compatible".

## Suggested first release

Create the `v0.1.1` tag on the current `main` commit.

- Release title: `Paint.Rust 0.1.1 — PDR sessions and multi-format open`
- Tag: `v0.1.1`
- Target: `main`
- Pre-release: **yes** (development preview, no installers)
- Release notes: paste [`docs/releases/v0.1.1.md`](releases/v0.1.1.md), or link to it and summarize

Suggested short body:

```md
Paint.Rust is an original Rust image editor inspired by Paint.NET's tool-first workflow.

This preview includes all 19 tools in Paint.NET order, mask-backed selections, live tool
previews, inline canvas text editing, layers/blending, clipboard and image operations,
effects with previews, a compact Paint.NET-style toolbar and icon menu rows. Save As supports
the complete PDR session format, layered PDN and PNG, JPEG, TIFF, BMP, TGA, DDS, GIF and
lossless WebP, through one native save panel with an in-panel File type selector. Open accepts
PDR, PDN and all eight raster formats.

Verification: 619 workspace tests, strict Clippy, xtask verify and the native macOS
keyboard/dirty-close gate pass.

This is a development preview. There are no installers or public binaries yet; build from
source. PDN is layered; the other raster formats store composed pixels. Paint.Rust is not
affiliated with Paint.NET.
```

## First push

After creating an empty GitHub repository:

```sh
git remote add origin git@github.com:diegomgarcia/paint-rust.git
git push -u origin main
```

Verify what will be published first:

```sh
git status
git log --oneline -10
git tag -n
```

The repository intentionally excludes `references/`, `target/` and local artifacts through
`.gitignore`. Do not force-add them.

## Before publishing

- [ ] `./tools/cargo-pinned.sh run -p xtask --locked -- verify` passes.
- [ ] Native macOS gate passes on the release binary.
- [ ] README screenshot and links render correctly on GitHub.
- [ ] No proprietary Paint.NET assets or reference dumps are tracked.
- [ ] Tag/release points at the intended commit.
- [ ] Public distribution limitations in `docs/RELEASE_STATUS.md` are still accurate.
