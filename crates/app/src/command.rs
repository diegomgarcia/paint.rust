use paint_core::{Bgra, Rect};

/// One UI-neutral operation submitted to the single-writer session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    /// Sets a signed, half-open selection. Raster operations clip it to the canvas.
    SelectRect(Rect),
    ClearSelection,
    /// Fills the active layer and selection, or the whole active layer without a selection.
    Fill(Bgra),
    /// Inverts RGB in the active layer and selection while preserving alpha.
    Invert,
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
