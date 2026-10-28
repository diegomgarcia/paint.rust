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
