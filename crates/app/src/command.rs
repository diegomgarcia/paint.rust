use paint_core::{Bgra, Point, Rect, Size};
use paint_raster::TextStyle;

use crate::Effect;

/// Placement of existing content when the canvas is resized.
///
/// The old canvas is placed at a top-left offset inside the new canvas. Left/top anchors
/// use offset `0`, centered anchors use `(new - old) / 2` with integer division truncating
/// toward zero, and right/bottom anchors use `new - old`. Negative offsets crop the old
/// content at the corresponding edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanvasAnchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Middle,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl CanvasAnchor {
    /// Returns the top-left offset of the old canvas inside the new canvas.
    pub const fn offsets(self, old: Size, new: Size) -> (i64, i64) {
        let delta_x = new.width() as i64 - old.width() as i64;
        let delta_y = new.height() as i64 - old.height() as i64;
        let x = match self {
            Self::TopLeft | Self::Left | Self::BottomLeft => 0,
            Self::Top | Self::Middle | Self::Bottom => delta_x / 2,
            Self::TopRight | Self::Right | Self::BottomRight => delta_x,
        };
        let y = match self {
            Self::TopLeft | Self::Top | Self::TopRight => 0,
            Self::Left | Self::Middle | Self::Right => delta_y / 2,
            Self::BottomLeft | Self::Bottom | Self::BottomRight => delta_y,
        };
        (x, y)
    }
}

/// One UI-neutral operation submitted to the single-writer session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    /// Sets a signed, half-open selection. Raster operations clip it to the canvas.
    SelectRect(Rect),
    ClearSelection,
    /// Crops the canvas to the current selection, clipped to the canvas, and clears it.
    CropToSelection,
    /// Resamples every layer to the requested pixel dimensions.
    ResizeImage {
        width: u32,
        height: u32,
    },
    /// Pads or crops the canvas around the existing layers using `anchor`.
    CanvasSize {
        width: u32,
        height: u32,
        anchor: CanvasAnchor,
        fill: Bgra,
    },
    /// Rotates every layer 90° clockwise and swaps the canvas dimensions.
    Rotate90Clockwise,
    /// Rotates every layer 90° counter-clockwise and swaps the canvas dimensions.
    Rotate90CounterClockwise,
    /// Rotates every layer 180° without changing the canvas dimensions.
    Rotate180,
    /// Fills the active layer and selection, or the whole active layer without a selection.
    Fill(Bgra),
    /// Paints a hard round stroke. `None` erases to transparent.
    Stroke {
        label: &'static str,
        points: Vec<Point>,
        size: u32,
        color: Option<Bgra>,
    },
    /// Fills the contiguous region around a canvas point.
    FloodFill {
        point: Point,
        color: Bgra,
        tolerance: u8,
        global: bool,
    },
    /// Fills a half-open canvas rectangle.
    FillRect {
        rect: Rect,
        color: Bgra,
    },
    /// Fills an ellipse inscribed in a half-open canvas rectangle.
    FillEllipse {
        rect: Rect,
        color: Bgra,
    },
    /// Paints a linear gradient between two canvas points.
    Gradient {
        start: Point,
        end: Point,
        from: Bgra,
        to: Bgra,
    },
    /// Recolors matching colors only under a round brush stroke.
    Recolor {
        points: Vec<Point>,
        size: u32,
        target: Bgra,
        replacement: Bgra,
        tolerance: u8,
    },
    /// Rasterizes one text placement into the active layer and selection.
    PlaceText {
        origin: Point,
        content: String,
        style: TextStyle,
        color: Bgra,
    },
    /// Clones pixels from a fixed source along a round brush stroke.
    CloneStamp {
        points: Vec<Point>,
        size: u32,
        source: Point,
        hardness: u8,
    },
    /// Moves the selection contents, or the whole layer without a selection.
    Move {
        dx: i32,
        dy: i32,
    },
    /// Inverts RGB in the active layer and selection while preserving alpha.
    Invert,
    /// Applies an implemented raster effect to the active layer and selection as one mutation.
    ApplyEffect(Effect),
    /// Flips the complete active layer left-to-right, ignoring the selection.
    FlipLayerHorizontal,
    /// Flips the complete active layer top-to-bottom, ignoring the selection.
    FlipLayerVertical,
    /// Flips every layer in the image left-to-right.
    FlipImageHorizontal,
    /// Flips every layer in the image top-to-bottom.
    FlipImageVertical,
    /// Composes all layers into one background layer.
    FlattenImage,
    /// Adds a transparent layer at the top and makes it active.
    AddLayer {
        name: String,
        opacity: u8,
        visible: bool,
    },
    /// Selects a layer by its bottom-to-top index without creating a content edit.
    SetActiveLayer {
        index: usize,
    },
    /// Removes a layer while preserving at least one layer.
    RemoveLayer {
        index: usize,
    },
    /// Duplicates a layer immediately above itself.
    DuplicateLayer {
        index: usize,
    },
    /// Merges a layer down into the layer below it.
    MergeLayerDown {
        index: usize,
    },
    /// Moves a layer by a signed delta in bottom-to-top order.
    MoveLayer {
        index: usize,
        offset: i32,
    },
    /// Renames a layer, preserving history and the active layer identity.
    RenameLayer {
        index: usize,
        name: String,
    },
    SetLayerVisibility {
        index: usize,
        visible: bool,
    },
    SetLayerOpacity {
        index: usize,
        opacity: u8,
    },
    Undo,
    Redo,
}
