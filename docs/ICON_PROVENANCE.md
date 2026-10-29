# Icon provenance

The toolbar and window icons embedded under `crates/ui/assets/lucide/` are
high-resolution PNG renders of Lucide SVG geometry, tinted at runtime by Paint.R.
They are not copied Paint.NET bitmaps, icon fonts, or application artwork. No
Paint.NET trademark, logo, or application icon is reproduced.

Lucide is ISC-licensed. The complete notice is in
`docs/licenses/LUCIDE-ISC.txt`.

## Lucide source names

- `square-dashed` — Rectangle Select
- `lasso-select` — Lasso Select
- `wand-sparkles` — Magic Wand
- `box-select` — Move Selection
- `move` — Move Selected Pixels
- `zoom-in` — Zoom
- `paint-bucket` — Paint Bucket
- `gradient` — Gradient
- `paintbrush` — Paintbrush
- `eraser` — Eraser
- `pencil` — Pencil
- `pipette` — Color Picker
- `stamp` — Clone Stamp
- `type` — Text
- `line` — Line / Curve
- `shapes` — Shapes
- `spline` — Shapes (Freeform)
- `rectangle-horizontal` — Rectangle shape slot
- `circle` — Ellipse shape slot
- `plus` — Add Layer
- `undo` / `redo` — history actions
- `file-plus` / `folder-open` / `file-down` — document actions
- `swap` / `rotate-ccw` — color slot actions
- `menu` — window controls

The PNGs are rendered from the corresponding Lucide SVG paths; the repository
keeps no third-party SVG source copied into the build.
