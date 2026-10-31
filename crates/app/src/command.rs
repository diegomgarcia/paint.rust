use paint_core::{Bgra, Point, Rect};

use crate::Effect;

/// One UI-neutral operation submitted to the single-writer session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    /// Sets a signed, half-open selection. Raster operations clip it to the canvas.
    SelectRect(Rect),
    ClearSelection,
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
