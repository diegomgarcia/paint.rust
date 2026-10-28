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
