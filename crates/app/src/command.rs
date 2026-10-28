use paint_core::{Bgra, Point, Rect};

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
    /// Replaces colors near a target with a replacement color.
    Recolor {
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
    /// Flips the complete active layer left-to-right, ignoring the selection.
    FlipLayerHorizontal,
    /// Flips the complete active layer top-to-bottom, ignoring the selection.
    FlipLayerVertical,
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
