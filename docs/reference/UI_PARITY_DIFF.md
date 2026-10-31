# Paint.R × Paint.NET UI parity diff (initial release batch)

Reference sources: `references/pdn-re/controls`, `designer`, `decompiled`, `strings.json`,
`PaintDotNet.PaintDotNet.Strings.3.resources`. Proprietary code/assets are read-only reference.

## Current baseline

Implemented and verified on `main` before this batch: 19 tools (plus one disabled Freeform
cell), multi-document tabs, Save/Save As, PNG open/export, selections with masks and combine
modes, Image operations, effects/adjustments, layers with drag reorder, clipboard, rulers,
grid, units, blend modes, text/clone/line/gradient/brush options.

## P0 — release blockers / core parity

| Area | Paint.NET reference | Current gap | Action |
|---|---|---|---|
| Toolbox | 19 tools, exact order in `DocumentWorkspace` static ctor; two-column 48 px strip | 20th disabled Freeform cell; no shortcut hints in tooltips | Remove the disabled slot, keep exact 19 order, add shortcut/tooltip metadata |
| Move Selected Pixels | `ToolConfigStrip` resampling section (`Resampling:`) + EnumSplitButton; Commit button “Finish”, Enter shortcut | Only Finish; no Resampling selector; no Enter shortcut | Add Mode enum/selector and functional subpixel resampling where it changes output; wire Enter to Finish |
| File | New (dialog), Open, Save, Save As, Export, Close/Exit | New creates a fixed blank doc; no New Image dialog; Open Recent absent | Add New Image width/height/background dialog; Open Recent or explicitly defer |
| Edit | Select All, Deselect, Invert Selection, Erase Selection, Fill Selection, Cut/Copy/Paste, Paste Into | Select All/Invert/Erase/Fill/Paste Into missing | Implement all as commands + shortcuts; keep text-field shortcut isolation |
| Toolbox options | Per-tool `ToolConfigStrip` visibility map | Many controls absent: fill sampling/blend, gradient blend, text align/blend, clone spacing, recolor sampling, picker 11/31/51, shape draw mode | Add the controls in Paint.NET order; disable only genuinely unsupported combinations |
| Panels/layout | Tools/Colors/Layers/History separate floating forms; bottom status | Our positions/sizes/headers differ; Tools 68 px; compact top bar | Align panel sizes, titles, bottom bars, status spacing; keep dark palette |
| Icons | Reference has distinct 16/48 px icons for tools/actions | Lucide approximations; some actions text-only | Replace text-only actions with vector/Lucide icons where semantics match; never copy PDN bitmaps |
| Help/About | About dialog, shortcuts help | Present but sparse | Include version/build, license links, shortcut table |

## P1 — feature parity

- **Shapes**: Freeform shape, shape draw mode (Outline/Fill/Both + secondary fill), arrow/line
  flyouts, shape types.
- **Gradient**: blend mode, color blend, transparency mode, spiral/alpha variants.
- **Brush/Eraser**: pressure sensitivity, brush fill style, blend mode, spacing slider range.
- **Clone Stamp**: spacing control, source layer choice, pattern modes.
- **Recolor**: sampling mode (layer/image), blend mode.
- **Text**: alignment, line spacing, blend mode, font list, editable text object.
- **Move**: true scale/rotate and resampling parity; floating transform handles.
- **Picker**: 11/31/51 averages, secondary-button sampling.
- **Selections**: per-pixel ants contour (currently boundary tracing is exact but ant path is
  axis-aligned), selection draw modes/fixed size, antialiased edges.
- **Effects/Adjustments**: Levels, Curves, Posterize, Threshold, Black & White, Motion/Radial
  blur variants, noise/clouds etc. Only expose real implementations.
- **Image**: Color Profile, DPI/print size, resampling method dropdown.
- **Layers**: blend mode setter (done), layer import, rotate/zoom, layer group semantics.

## P2 — later

- Plugin/effect browser, custom brushes, color management, scripting, accessibility audit,
  localization, macOS/Windows packaging and code signing.

## Batch ownership proposal

1. `toolbox-move`: exact 19-tool strip, tooltips/shortcuts, Move Resampling/Finish Enter.
2. `edit-file`: New Image dialog, Select All/Invert/Erase/Fill/Paste Into, Open Recent.
3. `tool-options`: fill/gradient/text/clone/recolor/picker options in reference order.
4. `panels-icons`: panel geometry/titles/status, icon-only actions, colors/layers/history polish.
5. `effects-adjust`: Levels/Posterize/Black&White and remaining safe effects (only real ones).
6. `release`: version metadata, README/known-gaps, screenshots, tag `v0.1.0` after integration.

Every lane: TDD, strict clippy, full workspace tests, no proprietary assets, no fake controls.
