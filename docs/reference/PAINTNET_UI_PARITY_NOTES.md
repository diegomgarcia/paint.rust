# Paint.NET 5.1.12 UI reference notes

These notes are derived from the installed Paint.NET binaries in the authorized Windows
reference VM, using REA 6.8.0 and the bundled `ildasm.exe`. They are static observations,
not assumptions from screenshots.

- Windows guest: `WIN-25GCKO0IRNR`
- Paint.NET product version: `5.112.9563.32325`
- `paintdotnet.dll` SHA-256: `c75d3aa4c291fa67c2253f9064d43b9648fc2668c82b83bdd1c260d28ef7ecc4`
- `PaintDotNet.Framework.dll` SHA-256: `49bf8f32bc7763a95fc3813f2dd367288b80cbc2277511bb2721c450ab81b67d`

## Verified UI facts

### Toolbox

- `PaintDotNet.Controls.ToolsControl` contains `private static literal int32 tbWidth = 2`.
  The real toolbox is **two columns**, matching the corrected Paint.R grid.
- The initial `ToolsControl` control is `48×328`; `OnLayout` derives width from two tool
  item widths plus padding, then adds one pixel to the tool strip height.
- Real tool types include RectangleSelect, EllipseSelect, LassoSelect, MagicWand, Move,
  MoveSelection, MoveSelectedPixels, Zoom, Pan, PaintBucket, Gradient, Pencil, PaintBrush,
  Eraser, CloneStamp, Recolor, ColorPicker, Text, LineCurve, Shapes (Rectangle/Ellipse/
  Freeform), plus the base Selection/Transacted tools.

### Floating windows

- `PaintDotNet.Dialogs.FloatingToolForm` declares:
  - `translucentOpacity = 0.75`
  - `opaqueOpacity = 1.0`
- `ReevaluateOpacity()` chooses opaque when `ForceOpaque` is set, input has entered the
  form, or the cursor is under the form; otherwise it uses translucent opacity.
- Floating forms participate in `SnapObstacleController` docking/snapping.
- Paint.R approximates this with a 0.75 alpha frame that becomes opaque when the pointer
  enters; full snap/dock geometry is not yet implemented.

### Colors window

`PaintDotNet.Dialogs.ColorsForm` contains primary/secondary user colors, color wheel,
palette control, RGB and HSV up/down controls, alpha control, hex box, RGB/HSV/alpha
headers, and a More/Less button. The minimized/expanded sizes are computed from
`moreSize`/`lessSize`. Paint.R now has the two color slots, active-slot editing, wheel,
palette, hex, RGB/HSV/Opacity controls, and More/Less.

### Main menus and panels

`PaintDotNet.Menus.PdnMainMenu` owns File, Edit, View, Image, Layers, Adjustments, and
Effects menus. `HistoryForm`, `LayersForm`, and `ToolsForm` are separate floating forms.
Paint.R currently exposes functional subsets; Image/Effects and full floating snap/dock
behavior remain future work.

## Reproduction

REA was run inside the VM through the DeepSeek Harness bundled Node and pnpm:

```text
node.exe pnpm.mjs dlx rea-agents@6.8.0 inspect-managed-members \
  "C:\Program Files\Paint.NET\paintdotnet.dll" --format json \
  --type PaintDotNet.Controls.ToolsControl \
  --type PaintDotNet.Dialogs.FloatingToolForm \
  --type PaintDotNet.Dialogs.ColorsForm ...
```

`ildasm.exe /UTF8 /ITEM=<type> /OUT=<file>.il "C:\Program Files\Paint.NET\paintdotnet.dll"`
was used to read exact literal constants and opacity branches. Static inspection does not
execute target code and does not establish visual identity or full behavior parity.

## Runtime observations (Windows session automation)

Paint.NET 5.1.12 was launched in the authorized Windows reference VM as the interactive
user and inspected through Win32 window capture plus accessibility/UIA metadata. A hidden
scheduled-task helper clicked real tool buttons and captured the resulting Tool Options.

- Paint Bucket Tool Options: Flood Mode `Contiguous`, Fill `Solid Color`, Tolerance `50%`,
  Sampling `Layer`, Blend Mode `Normal`. A canvas click filled the entire contiguous white
  document, added one `Paint Bucket` history entry, and updated the layer thumbnail.
- Pencil exposes only `Blend Mode: Normal`; it is a fixed one-pixel tool.
- Paintbrush defaults to `Brush size: 45`, `Hardness: 75%`, `Spacing: 15%`, Blend Mode
  `Normal`.
- Eraser exposes Brush size, Hardness and Spacing.
- Recolor is a brush tool with Brush size, Hardness, Spacing and Tolerance; it is not a
  global color replacement.
- Move Selected Pixels exposes Sampling `Bicubic` and a `Finish` action.
- Gradient exposes five gradient-shape buttons, `No Repeat`, Blend Mode `Normal`, and a
  `Finish` action.
- Text exposes font family, font size, bold/italic/underline, alignment, antialiasing,
  Blend Mode and Finish.
- The real Paint.NET toolbox is two columns; Move is near the top, Paint Bucket and
  Gradient share a row, and Brush/Eraser and Text/Shape groups follow.

Paint.R now aligns the Paint Bucket to Contiguous/Global plus percentage tolerance,
defaults the brush to 45 px, keeps Pencil fixed at one pixel, and makes Recolor a brush
stroke. Hardness, spacing, bicubic move sampling, and Finish-style transacted previews
are recorded as remaining parity work.

### Move / Picker / Line Tool Options (implemented)

The Move, Move Selection, Color Picker, and Line / Curve options use separate grouped
rows instead of one wrapped row, matching the real multi-row Tool Options strip.

- **Move Selected Pixels** exposes the real `Finish` action, which stamps the floating
  selection at its dragged destination as one history entry. Paint.R's move is an
  integer translation only: it does not rotate or scale the floating surface, so a
  Nearest/Bilinear/Bicubic resampling selector would have no effect on the stamp and is
  intentionally not exposed. Scaled/rotated moves and their resampling selector remain
  parity work.
- **Move Selection** exposes numeric `Offset X` / `Offset Y` fields with `Apply offset`
  and `Cancel offset`. Apply uses the existing undo-free `TranslateSelection` command;
  Cancel resets the pending fields without touching the selection.
- **Color Picker** exposes Click behavior (`Do not switch`, `Switch to previous tool`),
  Sampling (`Layer`, `Composed image`), and Sample size (`1 px`, `3×3 average`,
  `5×5 average`). Layer sampling reads only the active layer; composed sampling reads the
  composed image. Average samples use premultiplied-alpha averaging and clip the square to
  the canvas. With `Do not switch`, the picker stays active for repeated samples.
- **Line / Curve** exposes Start cap and End cap (`None`, `Rounded`, `Arrow`) and Dash
  style (`Solid`, `Dash`, `Dot`). Caps and dashes are rasterized into real pixels:
  rounded caps extend half a line width past an endpoint, arrows draw chevrons, and Dash
  / Dot use width-scaled 3:1 and 1:1 GDI-style patterns. The stroke is clipped to the
  active selection and commits as one undoable history entry.

### Compact toolbox measurements

- Real `ToolsForm` window bounds: approximately `67×292` px at the captured 100% DPI.
- Real tool strip bounds: `51×253` px, two columns, about `25×27` px per tool cell.
- Paint.R now uses a `68` px floating Tools window, `25×27` px tool cells, 1 px grid
  spacing, a square 17 px icon box, and no duplicated inner `Tools` heading.
- The tool order now follows the real sequence for the implemented subset:
  Rectangle Select, Move, Zoom, Paint Bucket, Pan, Gradient, Color Picker, Paintbrush,
  Eraser, Pencil, Recolor, Line, Rectangle, Ellipse.
- Pan is implemented as a non-editing canvas scroll action.

### Exact tool order from runtime tooltips

The 20 Paint.NET 5.1.12 toolbox cells, confirmed by hovering each real cell and reading
the tooltip, are:

1. Rectangle Select
2. Move Selected Pixels
3. Lasso Select
4. Move Selection
5. Ellipse Select
6. Zoom
7. Magic Wand
8. Pan
9. Paint Bucket
10. Gradient
11. Paintbrush
12. Eraser
13. Pencil
14. Color Picker
15. Clone Stamp
16. Recolor
17. Text
18. Line / Curve
19. Shapes
20. Shapes (second shape mode)

Paint.R currently implements the subset at the same relative order:
Rectangle Select, Move Selected Pixels, Zoom, Pan, Paint Bucket, Gradient, Paintbrush,
Eraser, Pencil, Color Picker, Recolor, Line, Rectangle, Ellipse.

### Floating window sizes

Measured real window bounds at 100% DPI: Tools `67×292`, Colors `225×256`,
Layers `196×219`, History `196×219`. Paint.R now defaults Colors and Layers to floating
windows instead of one large right dock. The compact Colors panel is 232 px wide, uses a
150 px wheel and a single-row palette; the palette label and extra headings were removed.
The docked test mode remains available through `PaintApp::new_docked` for canvas-focused
tests.

### Colors window behavior

The real Colors window uses a 225 px compact layout: overlapping primary/secondary slots,
two small icon buttons (swap and black/white reset), a More/Less button, a 149 px color
wheel, and a one-row palette. Paint.R now uses `Icon::SwapColors` and `Icon::ResetColors`
instead of text buttons, an 18 px one-row palette, a 150 px wheel, and a 232 px window.
The wheel updates the active color continuously on pointer down and drag, not only on
click release; this is covered by the UI acceptance test
`color_wheel_updates_primary_color_continuously_while_dragging`.

### Colors refinements

The real palette is a one-row strip of 16 solid 12 px swatches with no rounding and no
gaps; expanding the Colors window reveals three rows (the base palette plus lighter and
darker variants). The RGB/HSV/Opacity controls are gradient bars with a triangle marker
below the bar, not standard sliders. The swap icon sits at the top of the color slots, the
black/white reset icon sits below the slots, and More/Less is right-aligned on the top row.
Paint.R now follows all of these.


### Toolbox parity update

Paint.R now renders all **20** Paint.NET toolbox cells in the measured two-column
order, in rows of two with 25×25 px cells and 0 px vertical gap. The floating Tools
window uses a custom 68 px-wide titlebar so the native titlebar cannot force it to
96 px, and the grid is centered inside that width. The toolbox height is bounded
below 310 px, close to the measured real `67×292` window.

Implemented cells: Rectangle Select, Move Selected Pixels, Move Selection, Zoom,
Pan, Paint Bucket, Gradient, Paintbrush, Eraser, Pencil, Color Picker, Recolor,
Line / Curve, Shapes (Rectangle/Ellipse modes).

Visible but disabled pending larger features: Lasso Select, Ellipse Select,
Magic Wand, Clone Stamp, Text, and Shapes (Freeform). Ellipse/Lasso/Magic Wand
need a non-rectangular selection mask; Clone Stamp needs source sampling; Text
needs glyph rasterization; Freeform needs polygon fill. The slots are shown so the
layout is complete, but they are explicitly disabled rather than faking behavior.


### Layers bottom bar

Paint.R now matches the real Layers window structure: layer list above, fixed bottom
button bar with Add, Delete, Duplicate, Merge Down, Move Up, Move Down, and Layer
Properties. Add/Delete/Duplicate/Merge/Move are real document commands with undo;
Delete is disabled for the last layer, Merge Down for the bottom layer, and Move for
the respective edge. Double-clicking a layer row opens Layer Properties for that exact
layer; the dialog renames (validated, one undoable command), changes opacity and
visibility, and shows the live blend-mode label. Per-layer opacity sliders were removed
from the list, matching Paint.NET's properties-dialog placement.

Pointer-dragging a row reorders layers with a painted insertion indicator and commits
exactly one undoable `MoveLayer` on release; the active layer keeps its identity even
when a different row is dragged. The list uses a reserved solid scrollbar at the right
edge, so rows and their text never sit underneath the scrollbar.


### Live stroke preview and document tab

Brush, pencil, and eraser strokes are previewed directly in the composed canvas while
the pointer is down, using a session-level stroke snapshot that is committed as one
undoable mutation on release. The document tab strip (floating layout) shows the
current filename and a checkerboard-backed mini preview of the composed document.

The layer thumbnails also render over a transparency checkerboard. Image menu now
exposes functional Flip Horizontal/Vertical and Flatten Image; unsupported image
operations remain disabled rather than simulated.


### Multiple document tabs

Paint.R keeps one complete document state per tab: session/undo history, zoom, composed
texture, document thumbnail, layer thumbnails, selection, drag state, floating selection,
and layer numbering. New and Open create a new tab instead of discarding the current
document. The compact toolbar shows every document as a checkerboard-backed chip at the
end of the file-action icons; clicking a chip switches documents. Unsaved inactive tabs
are preserved and the quit guard checks all tabs.


### Reference extraction pass

The extracted controls/designers in `references/pdn-re` were used to align concrete
metrics and menu distribution. `VerticalImageStrip` sets `thumbEdgeLength = 50` dips and
`imagePaddingPx = 4`, giving `50 + 3 × 4 = 62` px layer items; Paint.R now uses a 50 px
thumbnail frame, 4 px item padding, 62 px rows, a 22×54 px right-side eye hit area, and
a selected-row highlight. The Image menu order
was aligned to Crop/Resize/Canvas Size, Flip, Rotate, Color Profile, Flatten; View exposes
Zoom to Window/Selection and outlines Grid/Rulers; the Layers menu now exposes add,
delete, duplicate, merge, visibility, flips, go-to, move, and properties entries in the
Paint.NET order. Proprietary Paint.NET icon bitmaps are used only as visual reference and
are not embedded.


### Parallel tool-options pass

Integrated lanes added brush hardness/spacing with Paint.NET defaults (45/75/15),
shape Outline/Fill/Both plus border width, and gradient types Linear/Reflected/
Diamond/Radial/Conical with None/Repeat modes. Rectangle, Ellipse, Lasso, and
Magic Wand now expose Replace/Union/Subtract/Intersect/Xor combine modes, with
Shift/Alt modifiers and Active Layer/Image magic-wand sampling. Document tabs
close individually with a dirty guard, View exposes a real grid and pixel/inch/cm
units, and Layer Properties supports Multiply/Screen/Overlay blend modes that are
composited for real. Selection remains replace-only at the trace-wire boundary. Mask selections now
render a translucent blue tint and marching ants along the real pixel boundary;
shape `Both` currently paints the interior with the primary color instead of the
secondary.


### Final tool-options/adjustments pass

Color Picker now supports Do-not-switch/previous-tool behavior, Layer/Image
sampling, and 1/3x3/5x5 averages; Line/Curve has real Flat/Rounded/Arrow caps
and Solid/Dash/Dot patterns; Move Selected Pixels has a real Finish action. The
Adjustments menu adds Brightness/Contrast, Hue/Saturation, and Auto-Level with
live previews and one undo entry. Intentional remaining gaps: no scaled/rotated
move resampling, selection combine modes are not serialized in the trace wire,
and shape Both still uses the primary color for its interior.
